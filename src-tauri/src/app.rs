//! Tauri glue: commands and the app builder. Compiled only for the real app, because
//! linking Tauri's UI code into unit-test binaries fails on Windows (no comctl32 v6 manifest).

use crate::config::{self, ConfigView, PomodoroLengths};
use crate::error::{Error, Result};
use crate::modes::{self, Mode};
use crate::pomodoro::{Phase, Pomodoro, Snapshot};
use crate::sessions::{self, SessionStore, Summary};
use crate::stats::Stats;
use crate::{activity, api, cache, focus_day};
use chrono::{FixedOffset, Local, Utc};
use serde::Serialize;
use crate::{notice, placement, themes};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::{StateFlags, WindowExt as _};

/// Held in Tauri state so the timer keeps running if the UI reloads.
struct Timer {
    state: Mutex<Pomodoro>,
    /// Sample-data mode never writes files, so today's count stays in memory there.
    persist: bool,
    /// Wall-clock start of the running focus phase, fixed when first seen. Recomputing it from the
    /// clock on every call would wobble by a second and look like a new phase to the UI and the cache.
    began_unix: Mutex<Option<(Instant, i64)>>,
    /// Set once the app exists, so a finished phase can raise a native notification from here.
    app: OnceLock<AppHandle>,
    /// The notification permission is asked for once per run, on the first Pomodoro start.
    asked_permission: AtomicBool,
    /// A phase the background tick saw finish (and notified about) that the UI hasn't been told yet.
    /// The UI's next snapshot reports it, so the beep and mascot still fire when the widget was hidden.
    unreported: Mutex<Option<Phase>>,
}

impl Timer {
    /// Runs `act` on the timer, then returns its snapshot for the UI, including a phase end the
    /// background tick already saw.
    fn with(&self, act: impl FnOnce(&mut Pomodoro, Instant)) -> Snapshot {
        let mut snap = self.step(act);
        let mut pending = self.unreported.lock().unwrap();
        match snap.finished {
            None => snap.finished = pending.take(),
            Some(_) => *pending = None,
        }
        snap
    }

    /// Advances the timer while nobody is looking: keeps the notification on time when the widget is
    /// hidden (the webview stops polling then). The phase end waits in `unreported` for the UI.
    fn tick(&self) {
        let snap = self.step(|_, _| {});
        if snap.finished.is_some() {
            *self.unreported.lock().unwrap() = snap.finished;
        }
    }

    /// Runs `act`, rolling the day over first, and saves today's count whenever it changed.
    /// A phase end is notified here, once: `finished` is reported by exactly one snapshot.
    fn step(&self, act: impl FnOnce(&mut Pomodoro, Instant)) -> Snapshot {
        let now = Instant::now();
        let today = Local::now().date_naive();
        let mut t = self.state.lock().unwrap();
        let before = (t.completed_focus(), t.roll_day(today));
        act(&mut t, now);
        let mut snap = t.snapshot(now);
        let mut known = self.began_unix.lock().unwrap();
        snap.focus_started_at = match t.focus_began() {
            None => {
                *known = None;
                None
            }
            Some(began) => {
                let unix = match *known {
                    Some((at, unix)) if at == began => unix,
                    _ => Utc::now().timestamp() - now.saturating_duration_since(began).as_secs() as i64,
                };
                *known = Some((began, unix));
                Some(unix)
            }
        };
        drop(known);
        drop(t); // never show a notification (which may block) while holding the timer
        if let (Some(app), Some((title, body))) = (self.app.get(), notice::phase_end(&snap)) {
            let _ = app.notification().builder().title(title).body(body).show();
        }
        if self.persist && (snap.completed_focus != before.0 || before.1) {
            // The count is a convenience: failing to save it must not break the timer.
            let _ = focus_day::save(&config::data_path("pomodoro-day.json"), today, snap.completed_focus);
        }
        snap
    }

    /// Start of the running focus phase as last reported. Unlike `with`, this does not advance the timer,
    /// so a background poll can never swallow the snapshot that reports a finished phase.
    fn focus_started_at(&self) -> Option<i64> {
        self.began_unix.lock().unwrap().map(|(_, unix)| unix)
    }

    /// Asks the OS for permission to notify, once per run, on the first Pomodoro start rather than at launch.
    fn ask_permission_once(&self) {
        if let (Some(app), false) = (self.app.get(), self.asked_permission.swap(true, Ordering::Relaxed)) {
            let _ = app.notification().request_permission();
        }
    }
}

struct Sessions(Mutex<SessionStore>);

/// Heartbeats for the running focus phase. Separate from `Heartbeats` (which belongs to a session)
/// so the two never evict each other's window.
struct FocusBeats(Mutex<activity::HeartbeatCache>);

/// How much of the running focus phase was spent coding.
#[derive(Serialize)]
struct FocusActivity {
    elapsed_secs: i64,
    active_secs: f64,
}

/// Heartbeats already downloaded for the running session (see `activity::poll`).
struct Heartbeats(Mutex<activity::HeartbeatCache>);

fn local_offset() -> FixedOffset {
    FixedOffset::east_opt(Local::now().offset().local_minus_utc()).unwrap_or(FixedOffset::east_opt(0).unwrap())
}

#[derive(Serialize)]
struct LiveSession {
    start: i64,
    elapsed_secs: i64,
    /// Coding activity inside the session; `None` if heartbeats couldn't be fetched.
    activity: Option<activity::Live>,
    activity_error: Option<String>,
}

/// Whether a heartbeat arrived in the last ~2 minutes (drives the compact pill's pulse). Errors read as "no".
#[tauri::command]
async fn recently_active() -> bool {
    activity::recently_active(&config::load(), Utc::now().timestamp()).await.unwrap_or(false)
}

#[derive(Serialize)]
struct SessionView {
    active: Option<LiveSession>,
    summary: Summary,
}

#[tauri::command]
async fn session_state(store: State<'_, Sessions>, beats: State<'_, Heartbeats>) -> Result<SessionView> {
    let (now, tz) = (Utc::now().timestamp(), local_offset());
    // Read what we need and release the lock before any network await.
    let (start, summary) = {
        let s = store.0.lock().unwrap();
        (s.active().map(|a| a.start), sessions::summarize(s.sessions(), now, tz))
    };
    let active = match start {
        None => None,
        Some(start) => {
            let (activity, activity_error) = match activity::for_session(&config::load(), &beats.0, start, now).await {
                Ok(a) => (Some(a), None),
                Err(e) => (None, Some(e.to_string())),
            };
            Some(LiveSession { start, elapsed_secs: now - start, activity, activity_error })
        }
    };
    Ok(SessionView { active, summary })
}

#[tauri::command]
fn session_start(store: State<Sessions>, beats: State<Heartbeats>) -> Result<()> {
    beats.0.lock().unwrap().clear();
    store.0.lock().unwrap().start(Utc::now().timestamp())
}

/// Returns false when the session was too short to keep.
#[tauri::command]
fn session_stop(store: State<Sessions>, beats: State<Heartbeats>) -> Result<bool> {
    beats.0.lock().unwrap().clear();
    store.0.lock().unwrap().stop(Utc::now().timestamp())
}

#[tauri::command]
async fn get_stats(range: String) -> Result<Stats> {
    let cfg = config::load();
    let range = api::Range::parse(&range);
    let stats = api::fetch_stats(&cfg, range).await?;
    if !cfg.mock {
        // The cache is a convenience: failing to write it must not fail the refresh.
        let _ = cache::store(&config::data_path("stats-cache.json"), range.name(), &stats, Utc::now().timestamp());
    }
    Ok(stats)
}

#[derive(Serialize)]
struct CachedStats {
    fetched_at: i64,
    stale: bool,
    stats: Stats,
}

/// The last good stats for a range, or `None` if there are none (or in demo mode).
#[tauri::command]
fn get_cached_stats(range: String) -> Option<CachedStats> {
    if config::load().mock {
        return None;
    }
    let now = Utc::now().timestamp();
    let entry = cache::StatsCache::load(&config::data_path("stats-cache.json")).get(api::Range::parse(&range).name())?.clone();
    Some(CachedStats { fetched_at: entry.fetched_at, stale: cache::is_stale(&entry, now), stats: entry.stats })
}

/// What the UI may see of the config. The key and server URL never leave Rust. `ui_mode` is the window's
/// actual mode (not only the file's), so a page that loads or reloads always starts in step with the window.
#[tauri::command]
fn get_config(place: State<WindowPlace>) -> ConfigView {
    let mut view: ConfigView = (&config::load()).into();
    view.ui_mode = place.mode();
    view
}

/// Checks the key's shape before saving it, so a typo never costs a network call.
#[tauri::command]
fn set_api_key(key: String) -> Result<()> {
    config::save_api_key(&key)?;
    // Saved stats belong to the previous account.
    let _ = std::fs::remove_file(config::data_path("stats-cache.json"));
    Ok(())
}

/// Remembers whether the mascot may talk.
#[tauri::command]
fn set_quiet_mascot(quiet: bool) -> Result<()> {
    config::save_quiet_mascot(quiet)
}

/// Saves new timer lengths without touching the rest of the config, applies them, and returns the timer.
#[tauri::command]
fn set_pomodoro(timer: State<Timer>, lengths: PomodoroLengths) -> Result<Snapshot> {
    let saved = config::save_lengths(lengths)?;
    Ok(timer.with(|t, _| t.set_config(saved)))
}

/// Whether the tray icon exists: without it there would be no way to bring a hidden widget back.
struct HasTray(bool);

/// Window positions are remembered per mode in the config (`full_pos` / `compact_pos`, see `modes.rs`).
/// The window-state plugin from Phase 6 is kept only as a read-only, one-time fallback: when the config
/// has no full-mode position yet (first run after the upgrade), the position the plugin saved is
/// restored once. The plugin no longer restores on its own (`skip_initial_state`) and no longer tracks the
/// window (its filter rejects it), so it can never fight the per-mode positions.
/// Its file lives in the OS config folder, which test runs must not touch:
/// `HACKATIME_WIDGET_NO_WINDOW_STATE=1` leaves the plugin out entirely.
fn uses_window_state_plugin() -> bool {
    std::env::var("HACKATIME_WIDGET_NO_WINDOW_STATE").map_or(true, |v| v != "1")
}

/// The window's actual mode, and window state not yet written to the config. Dragging fires many `Moved`
/// events and a switch changes the mode; both only update `pending`, which the 1 s background tick writes
/// (a throttled save that never blocks the main thread while the page is hidden mid-switch).
struct WindowPlace {
    mode: Mutex<Mode>,
    pending: Mutex<modes::Pending>,
}

impl WindowPlace {
    fn mode(&self) -> Mode {
        *self.mode.lock().unwrap()
    }

    fn moved(&self, pos: (i32, i32)) {
        let mode = self.mode();
        self.pending.lock().unwrap().moved(mode, pos);
    }

    /// Writes the pending state, if any. A failed write is only logged: window state is a convenience, and
    /// a config that can't be read is left alone (see `config::update`).
    fn flush(&self) {
        let Some((mode, positions)) = self.pending.lock().unwrap().take() else { return };
        if let Err(e) = config::save_window(mode, &positions) {
            eprintln!("window state not saved: {e}");
        }
    }
}

/// Saves where the widget is, then quits.
fn quit_app(app: &AppHandle) {
    app.state::<WindowPlace>().flush();
    app.exit(0);
}

/// The `–` button. Hides the widget; if there is no tray icon to restore it from, quits instead.
#[tauri::command]
fn hide_window(app: AppHandle, tray: State<HasTray>) {
    if !tray.0 {
        return quit_app(&app);
    }
    if let Some(w) = app.get_webview_window("main") {
        app.state::<WindowPlace>().flush();
        let _ = w.hide();
    }
}

/// The `×` button: really quits (the tray menu and `–` are the ways to keep it running in the background).
#[tauri::command]
fn quit(app: AppHandle) {
    quit_app(&app);
}

fn toggle_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if w.is_visible().unwrap_or(false) {
        app.state::<WindowPlace>().flush();
        let _ = w.hide();
    } else {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The hotkey and the tray item: the page animates and then calls `set_mode`. A hidden widget is shown
/// first, so the switch is never invisible.
fn request_compact_toggle(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if !w.is_visible().unwrap_or(true) {
            let _ = w.show();
        }
    }
    let _ = app.emit_to("main", "compact-hotkey", ());
}

fn monitors(w: &tauri::WebviewWindow) -> Vec<modes::Monitor> {
    to_monitors(w.available_monitors().unwrap_or_default())
}

fn to_monitors(list: Vec<tauri::Monitor>) -> Vec<modes::Monitor> {
    list.iter()
        .map(|m| {
            let a = m.work_area();
            let work_area = placement::Rect { x: a.position.x, y: a.position.y, w: a.size.width, h: a.size.height };
            modes::Monitor { work_area, scale: m.scale_factor() }
        })
        .collect()
}

/// Moves the widget back onto a monitor if its remembered position is on one that is gone.
fn keep_on_screen(w: &tauri::WebviewWindow) {
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else { return };
    let areas: Vec<placement::Rect> = monitors(w).iter().map(|m| m.work_area).collect();
    let (x, y) = placement::clamp_into((pos.x, pos.y), (size.width, size.height), &areas);
    if (x, y) != (pos.x, pos.y) {
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// What `set_mode` did, in logical pixels of the monitor the window is on. `shift_*` is how far clamping
/// moved the window away from the pure top-right anchor; the expand animation starts the pill there.
#[derive(Serialize)]
struct ModeRect {
    mode: Mode,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
    shift_x: f64,
    shift_y: f64,
}

/// Switches the window between the full widget and the compact pill: size, position (top-right anchor,
/// clamped as `modes::switch` says) and persistence (queued for the background tick). All window work
/// happens here, so the page needs no window size or position permissions. Animations are the page's
/// business (`mode.js`). With `preview: true` nothing changes: it returns where the switch would put the
/// window, so a collapse can aim its animation at a clamped pill.
#[tauri::command]
fn set_mode(app: AppHandle, place: State<WindowPlace>, mode: String, preview: Option<bool>) -> Result<ModeRect> {
    let to = Mode::parse(&mode).ok_or_else(|| Error::Window(format!("unknown mode {mode:?}")))?;
    let w = app.get_webview_window("main").ok_or_else(|| Error::Window("no main window".into()))?;
    let fail = |e: tauri::Error| Error::Window(e.to_string());
    let (pos, size) = (w.outer_position().map_err(fail)?, w.outer_size().map_err(fail)?);
    let current = placement::Rect { x: pos.x, y: pos.y, w: size.width, h: size.height };
    let p = modes::switch(to, current, &monitors(&w));

    if !preview.unwrap_or(false) {
        let from = std::mem::replace(&mut *place.mode.lock().unwrap(), to);
        // Position first, then size: the page hides itself for this moment (see mode.js), so the in-between
        // state is never seen whichever order the OS paints.
        w.set_position(tauri::PhysicalPosition::new(p.pos.0, p.pos.1)).map_err(fail)?;
        w.set_size(tauri::PhysicalSize::new(p.size.0, p.size.1)).map_err(fail)?;
        place.pending.lock().unwrap().switched(from, (pos.x, pos.y), to, p.pos);
    }
    let l = |v: i32| modes::to_logical(v as i64, p.scale);
    Ok(ModeRect {
        mode: to,
        x: l(p.pos.0),
        y: l(p.pos.1),
        width: modes::to_logical(p.size.0 as i64, p.scale),
        height: modes::to_logical(p.size.1 as i64, p.scale),
        scale: p.scale,
        shift_x: l(p.shift.0),
        shift_y: l(p.shift.1),
    })
}

/// Automated tests only: moves the window like a drag would (physical pixels), so the `Moved` -> config path
/// can be checked without a real pointer. Compiled into debug builds only, and refused there too unless
/// `HACKATIME_WIDGET_TEST_HOOKS=1`.
#[cfg(debug_assertions)]
#[tauri::command]
fn test_move_window(app: AppHandle, x: i32, y: i32) -> Result<()> {
    if !std::env::var("HACKATIME_WIDGET_TEST_HOOKS").is_ok_and(|v| v == "1") {
        return Err(Error::Window("test hooks are off".into()));
    }
    let w = app.get_webview_window("main").ok_or_else(|| Error::Window("no main window".into()))?;
    w.set_position(tauri::PhysicalPosition::new(x, y)).map_err(|e| Error::Window(e.to_string()))
}

fn build_tray(app: &tauri::App, hotkey: &str) -> tauri::Result<()> {
    let handle = app.handle();
    let toggle = MenuItem::with_id(handle, "toggle", "Show / Hide", true, None::<&str>)?;
    // The accelerator only shows the hotkey beside the label: a tray menu is not attached to a window, so
    // no accelerator table ever translates it, and the global shortcut (which fires the switch) swallows the
    // keystroke anyway. A hotkey string the menu can't parse just leaves the label bare.
    let compact = MenuItem::with_id(handle, "compact", "Compact mode", true, Some(hotkey))
        .or_else(|_| MenuItem::with_id(handle, "compact", "Compact mode", true, None::<&str>))?;
    let theme_items: Vec<MenuItem<_>> = themes::THEMES
        .iter()
        .map(|(id, label)| MenuItem::with_id(handle, format!("theme:{id}"), *label, true, None::<&str>))
        .collect::<tauri::Result<_>>()?;
    let theme_refs: Vec<&dyn tauri::menu::IsMenuItem<_>> = theme_items.iter().map(|i| i as _).collect();
    let theme_menu = Submenu::with_items(handle, "Theme", true, &theme_refs)?;
    let on_login = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(handle, "autostart", "Launch at login", true, on_login, None::<&str>)?;
    let quit = MenuItem::with_id(handle, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(handle)?;
    let menu = Menu::with_items(handle, &[&toggle, &compact, &theme_menu, &autostart, &sep, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Hackatime widget")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle" => toggle_window(app),
            "autostart" => {
                // The menu flips the check mark itself; follow it, and put it back if the OS refuses.
                let want = autostart.is_checked().unwrap_or(false);
                let launch = app.autolaunch();
                let done = if want { launch.enable() } else { launch.disable() };
                if let Err(e) = done {
                    eprintln!("launch at login: {e}");
                    let _ = autostart.set_checked(!want);
                }
            }
            "compact" => request_compact_toggle(app),
            "quit" => quit_app(app),
            id => {
                if let Some(theme) = id.strip_prefix("theme:").filter(|t| themes::is_theme(t)) {
                    let _ = app.emit_to("main", "set-theme", theme);
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_window(tray.app_handle());
            }
        });
    if let Some(icon) = handle.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(handle)?;
    Ok(())
}

/// Registers the compact-mode shortcut. Any failure (bad string, taken by another app, no shortcut
/// support such as Wayland) is logged and skipped: the tray item keeps working.
fn register_hotkey(app: &tauri::App, hotkey: &str) {
    let shortcut: Shortcut = match hotkey.parse() {
        Ok(s) => s,
        Err(e) => return eprintln!("compact hotkey {hotkey:?} ignored: {e}"),
    };
    let registered = app.global_shortcut().on_shortcut(shortcut, |app, _, event| {
        if event.state == ShortcutState::Pressed {
            request_compact_toggle(app);
        }
    });
    if let Err(e) = registered {
        eprintln!("compact hotkey {hotkey:?} not registered: {e}");
    }
}

#[tauri::command]
fn pomodoro_state(timer: State<Timer>) -> Snapshot {
    timer.with(|_, _| {})
}

/// Coding activity since the current focus phase started, or `None` when none is running.
#[tauri::command]
async fn focus_activity(timer: State<'_, Timer>, beats: State<'_, FocusBeats>) -> Result<Option<FocusActivity>> {
    let Some(start) = timer.focus_started_at() else { return Ok(None) };
    let now = Utc::now().timestamp();
    let live = activity::for_session(&config::load(), &beats.0, start, now).await?;
    Ok(Some(FocusActivity { elapsed_secs: (now - start).max(0), active_secs: live.active_secs }))
}

#[tauri::command]
fn pomodoro_start(timer: State<Timer>) -> Snapshot {
    timer.ask_permission_once();
    timer.with(|t, now| t.start(now))
}

#[tauri::command]
fn pomodoro_pause(timer: State<Timer>) -> Snapshot {
    timer.with(|t, now| t.pause(now))
}

#[tauri::command]
fn pomodoro_reset(timer: State<Timer>) -> Snapshot {
    timer.with(|t, _| t.reset())
}

#[tauri::command]
fn pomodoro_skip(timer: State<Timer>) -> Snapshot {
    timer.with(|t, now| t.skip(now))
}

pub fn run() {
    let cfg = config::load();
    let store = if cfg.mock {
        // Mock mode keeps sample history in memory and never touches sessions.json.
        SessionStore::in_memory(sessions::sample_history(Utc::now().timestamp(), local_offset()))
    } else {
        SessionStore::load(config::data_path("sessions.json"))
    };

    let today = Local::now().date_naive();
    let saved_count = if cfg.mock { 0 } else { focus_day::load(&config::data_path("pomodoro-day.json"), today) };

    let mut builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    if uses_window_state_plugin() {
        // Read-only legacy fallback, see `uses_window_state_plugin`.
        builder = builder.plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::POSITION)
                .skip_initial_state("main")
                .with_filter(|_| false)
                .build(),
        );
    }
    builder
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(Timer {
            state: Mutex::new(Pomodoro::new(cfg.pomodoro.clone(), today, saved_count)),
            persist: !cfg.mock,
            began_unix: Mutex::new(None),
            app: OnceLock::new(),
            asked_permission: AtomicBool::new(false),
            unreported: Mutex::new(None),
        })
        .manage(Sessions(Mutex::new(store)))
        .manage(Heartbeats(Mutex::new(activity::HeartbeatCache::default())))
        .manage(FocusBeats(Mutex::new(activity::HeartbeatCache::default())))
        .manage(WindowPlace { mode: Mutex::new(cfg.ui_mode), pending: Mutex::new(modes::Pending::default()) })
        .setup(move |app| {
            let _ = app.state::<Timer>().app.set(app.handle().clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                handle.state::<Timer>().tick();
                handle.state::<WindowPlace>().flush();
            });
            // Keep WebView2/WebKit data beside the config instead of in the OS app-data dir.
            let data_dir = config::data_path("webview-data");
            let window_cfg = app.config().app.windows[0].clone();
            // HACKATIME_WIDGET_HIDDEN=1 keeps the window off-screen for automated UI tests.
            let hidden = std::env::var("HACKATIME_WIDGET_HIDDEN").is_ok_and(|v| v == "1");
            // The window is created at its mode's size and remembered position. The builder takes logical
            // values, so the remembered physical position is handed over at its monitor's scale; the exact
            // physical position and size are then set again (a no-op unless the OS converted differently).
            let mode = cfg.ui_mode;
            let (lw, lh) = mode.logical_size();
            let launch = modes::at_launch(mode, cfg.pos_for(mode), &to_monitors(app.available_monitors().unwrap_or_default()));
            let mut window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_cfg)?
                .data_directory(data_dir)
                .inner_size(lw, lh)
                .visible(!hidden);
            if let Some(p) = launch {
                window = window.position(modes::to_logical(p.pos.0 as i64, p.scale), modes::to_logical(p.pos.1 as i64, p.scale));
            }
            if let Some(script) = modes::init_script(mode) {
                window = window.initialization_script(script);
            }
            let window = window.build()?;
            match launch {
                Some(p) => {
                    let _ = window.set_position(tauri::PhysicalPosition::new(p.pos.0, p.pos.1));
                    let _ = window.set_size(tauri::PhysicalSize::new(p.size.0, p.size.1));
                }
                // First run after Phase 7: the position the window-state plugin remembered, once.
                None if mode == Mode::Full && uses_window_state_plugin() => {
                    let _ = window.restore_state(StateFlags::POSITION);
                }
                None => {}
            }
            keep_on_screen(&window);
            let place = app.handle().clone();
            window.on_window_event(move |e| {
                if let WindowEvent::Moved(p) = e {
                    place.state::<WindowPlace>().moved((p.x, p.y));
                }
            });
            let has_tray = match build_tray(app, &cfg.compact_hotkey) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("tray icon unavailable, the minimise button will quit: {e}");
                    false
                }
            };
            app.manage(HasTray(has_tray));
            register_hotkey(app, &cfg.compact_hotkey);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_stats,
            get_cached_stats,
            get_config,
            set_api_key,
            set_pomodoro,
            set_quiet_mascot,
            hide_window,
            quit,
            set_mode,
            #[cfg(debug_assertions)]
            test_move_window,
            session_state,
            recently_active,
            session_start,
            session_stop,
            pomodoro_state,
            focus_activity,
            pomodoro_start,
            pomodoro_pause,
            pomodoro_reset,
            pomodoro_skip
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hackatime widget");
}
