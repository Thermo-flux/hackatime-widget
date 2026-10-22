//! Tauri glue: commands and the app builder. Compiled only for the real app, because
//! linking Tauri's UI code into unit-test binaries fails on Windows (no comctl32 v6 manifest).

use crate::config::{self, ConfigView, PomodoroLengths};
use crate::error::Result;
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
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::{AppHandleExt as _, StateFlags};

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

/// What the UI may see of the config. The key and server URL never leave Rust.
#[tauri::command]
fn get_config() -> ConfigView {
    (&config::load()).into()
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

/// The window-state file lives in the OS config folder, which test runs must not write to:
/// `HACKATIME_WIDGET_NO_WINDOW_STATE=1` turns position memory off.
fn remembers_position() -> bool {
    std::env::var("HACKATIME_WIDGET_NO_WINDOW_STATE").map_or(true, |v| v != "1")
}

fn remember_position(app: &AppHandle) {
    if remembers_position() {
        let _ = app.save_window_state(StateFlags::POSITION);
    }
}

/// Remembers where the widget is, then quits. (`exit` alone would skip the window-state save.)
fn quit_app(app: &AppHandle) {
    remember_position(app);
    app.exit(0);
}

/// The `x` button. Hides the widget; if there is no tray icon to restore it from, quits instead.
#[tauri::command]
fn hide_window(app: AppHandle, tray: State<HasTray>) {
    if !tray.0 {
        return quit_app(&app);
    }
    if let Some(w) = app.get_webview_window("main") {
        remember_position(&app);
        let _ = w.hide();
    }
}

fn toggle_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if w.is_visible().unwrap_or(false) {
        remember_position(&app);
        let _ = w.hide();
    } else {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Moves the widget back onto a monitor if its remembered position is on one that is gone.
fn keep_on_screen(w: &tauri::WebviewWindow) {
    let (Ok(pos), Ok(size), Ok(monitors)) = (w.outer_position(), w.outer_size(), w.available_monitors()) else { return };
    let areas: Vec<placement::Rect> = monitors
        .iter()
        .map(|m| {
            let a = m.work_area();
            placement::Rect { x: a.position.x, y: a.position.y, w: a.size.width, h: a.size.height }
        })
        .collect();
    let (x, y) = placement::clamp_into((pos.x, pos.y), (size.width, size.height), &areas);
    if (x, y) != (pos.x, pos.y) {
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let handle = app.handle();
    let toggle = MenuItem::with_id(handle, "toggle", "Show / Hide", true, None::<&str>)?;
    // Enabled together with compact mode (Phase 7).
    let compact = MenuItem::with_id(handle, "compact", "Compact mode", false, None::<&str>)?;
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
            let _ = app.emit_to("main", "compact-hotkey", ());
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
    if remembers_position() {
        // Size is the app's to decide (compact mode), so only the position is remembered.
        builder = builder.plugin(tauri_plugin_window_state::Builder::new().with_state_flags(StateFlags::POSITION).build());
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
        .setup(move |app| {
            let _ = app.state::<Timer>().app.set(app.handle().clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                handle.state::<Timer>().tick();
            });
            // Keep WebView2/WebKit data beside the config instead of in the OS app-data dir.
            let data_dir = config::data_path("webview-data");
            let window_cfg = app.config().app.windows[0].clone();
            // HACKATIME_WIDGET_HIDDEN=1 keeps the window off-screen for automated UI tests.
            let hidden = std::env::var("HACKATIME_WIDGET_HIDDEN").is_ok_and(|v| v == "1");
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_cfg)?
                .data_directory(data_dir)
                .visible(!hidden)
                .build()?;
            keep_on_screen(&window);
            let has_tray = match build_tray(app) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("tray icon unavailable, the x button will quit: {e}");
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
            session_state,
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
