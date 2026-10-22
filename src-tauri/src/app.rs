//! Tauri glue: commands and the app builder. Compiled only for the real app, because
//! linking Tauri's UI code into unit-test binaries fails on Windows (no comctl32 v6 manifest).

use crate::config::{self, ConfigView, PomodoroLengths};
use crate::error::Result;
use crate::pomodoro::{Pomodoro, Snapshot};
use crate::sessions::{self, SessionStore, Summary};
use crate::stats::Stats;
use crate::{activity, api, cache, focus_day};
use chrono::{FixedOffset, Local, Utc};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use tauri::State;

/// Held in Tauri state so the timer keeps running if the UI reloads.
struct Timer {
    state: Mutex<Pomodoro>,
    /// Sample-data mode never writes files, so today's count stays in memory there.
    persist: bool,
    /// Wall-clock start of the running focus phase, fixed when first seen. Recomputing it from the
    /// clock on every call would wobble by a second and look like a new phase to the UI and the cache.
    began_unix: Mutex<Option<(Instant, i64)>>,
}

impl Timer {
    /// Runs `act` on the timer, then returns its snapshot. Rolls the day over first, and saves
    /// today's count whenever it changed.
    fn with(&self, act: impl FnOnce(&mut Pomodoro, Instant)) -> Snapshot {
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
        if self.persist && (snap.completed_focus != before.0 || before.1) {
            // The count is a convenience: failing to save it must not break the timer.
            let _ = focus_day::save(&config::data_path("pomodoro-day.json"), today, snap.completed_focus);
        }
        snap
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

/// The window is frameless and off the taskbar, so the UI needs its own way to quit.
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn pomodoro_state(timer: State<Timer>) -> Snapshot {
    timer.with(|_, _| {})
}

/// Coding activity since the current focus phase started, or `None` when none is running.
#[tauri::command]
async fn focus_activity(timer: State<'_, Timer>, beats: State<'_, FocusBeats>) -> Result<Option<FocusActivity>> {
    let Some(start) = timer.with(|_, _| {}).focus_started_at else { return Ok(None) };
    let now = Utc::now().timestamp();
    let live = activity::for_session(&config::load(), &beats.0, start, now).await?;
    Ok(Some(FocusActivity { elapsed_secs: (now - start).max(0), active_secs: live.active_secs }))
}

#[tauri::command]
fn pomodoro_start(timer: State<Timer>) -> Snapshot {
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

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Timer { state: Mutex::new(Pomodoro::new(cfg.pomodoro, today, saved_count)), persist: !cfg.mock, began_unix: Mutex::new(None) })
        .manage(Sessions(Mutex::new(store)))
        .manage(Heartbeats(Mutex::new(activity::HeartbeatCache::default())))
        .manage(FocusBeats(Mutex::new(activity::HeartbeatCache::default())))
        .setup(|app| {
            // Keep WebView2/WebKit data beside the config instead of in the OS app-data dir.
            let data_dir = config::data_path("webview-data");
            let window_cfg = app.config().app.windows[0].clone();
            // HACKATIME_WIDGET_HIDDEN=1 keeps the window off-screen for automated UI tests.
            let hidden = std::env::var("HACKATIME_WIDGET_HIDDEN").is_ok_and(|v| v == "1");
            tauri::WebviewWindowBuilder::from_config(app.handle(), &window_cfg)?
                .data_directory(data_dir)
                .visible(!hidden)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_stats,
            get_cached_stats,
            get_config,
            set_api_key,
            set_pomodoro,
            set_quiet_mascot,
            quit,
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
