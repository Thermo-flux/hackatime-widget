//! Tauri glue: commands and the app builder. Compiled only for the real app, because
//! linking Tauri's UI code into unit-test binaries fails on Windows (no comctl32 v6 manifest).

use crate::config::{self, ConfigView, PomodoroLengths};
use crate::error::Result;
use crate::pomodoro::{Pomodoro, Snapshot};
use crate::sessions::{self, SessionStore, Summary};
use crate::stats::Stats;
use crate::{activity, api, cache};
use chrono::{FixedOffset, Local, Utc};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use tauri::State;

/// Held in Tauri state so the timer keeps running if the UI reloads.
struct Timer(Mutex<Pomodoro>);

struct Sessions(Mutex<SessionStore>);

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
async fn session_state(store: State<'_, Sessions>) -> Result<SessionView> {
    let (now, tz) = (Utc::now().timestamp(), local_offset());
    // Read what we need and release the lock before any network await.
    let (start, summary) = {
        let s = store.0.lock().unwrap();
        (s.active().map(|a| a.start), sessions::summarize(s.sessions(), now, tz))
    };
    let active = match start {
        None => None,
        Some(start) => {
            let (activity, activity_error) = match activity::for_session(&config::load(), start, now).await {
                Ok(a) => (Some(a), None),
                Err(e) => (None, Some(e.to_string())),
            };
            Some(LiveSession { start, elapsed_secs: now - start, activity, activity_error })
        }
    };
    Ok(SessionView { active, summary })
}

#[tauri::command]
fn session_start(store: State<Sessions>) -> Result<()> {
    store.0.lock().unwrap().start(Utc::now().timestamp())
}

/// Returns false when the session was too short to keep.
#[tauri::command]
fn session_stop(store: State<Sessions>) -> Result<bool> {
    store.0.lock().unwrap().stop(Utc::now().timestamp())
}

#[tauri::command]
async fn get_stats(range: String) -> Result<Stats> {
    let cfg = config::load();
    let stats = api::fetch_stats(&cfg, api::Range::parse(&range)).await?;
    if !cfg.mock {
        // The cache is a convenience: failing to write it must not fail the refresh.
        let _ = cache::store(&config::data_path("stats-cache.json"), &range, &stats, Utc::now().timestamp());
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
    let entry = cache::StatsCache::load(&config::data_path("stats-cache.json")).get(&range)?.clone();
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

/// Saves new timer lengths without touching the rest of the config, applies them, and returns the timer.
#[tauri::command]
fn set_pomodoro(timer: State<Timer>, lengths: PomodoroLengths) -> Result<Snapshot> {
    let saved = config::save_lengths(lengths)?;
    let mut t = timer.0.lock().unwrap();
    t.set_config(saved);
    Ok(t.snapshot(Instant::now()))
}

/// The window is frameless and off the taskbar, so the UI needs its own way to quit.
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn pomodoro_state(timer: State<Timer>) -> Snapshot {
    timer.0.lock().unwrap().snapshot(Instant::now())
}

#[tauri::command]
fn pomodoro_start(timer: State<Timer>) -> Snapshot {
    let mut t = timer.0.lock().unwrap();
    t.start(Instant::now());
    t.snapshot(Instant::now())
}

#[tauri::command]
fn pomodoro_pause(timer: State<Timer>) -> Snapshot {
    let mut t = timer.0.lock().unwrap();
    t.pause(Instant::now());
    t.snapshot(Instant::now())
}

#[tauri::command]
fn pomodoro_reset(timer: State<Timer>) -> Snapshot {
    let mut t = timer.0.lock().unwrap();
    t.reset();
    t.snapshot(Instant::now())
}

#[tauri::command]
fn pomodoro_skip(timer: State<Timer>) -> Snapshot {
    let mut t = timer.0.lock().unwrap();
    t.skip(Instant::now());
    t.snapshot(Instant::now())
}

pub fn run() {
    let cfg = config::load();
    let store = if cfg.mock {
        // Mock mode keeps sample history in memory and never touches sessions.json.
        SessionStore::in_memory(sessions::sample_history(Utc::now().timestamp(), local_offset()))
    } else {
        SessionStore::load(config::data_path("sessions.json"))
    };

    tauri::Builder::default()
        .manage(Timer(Mutex::new(Pomodoro::new(cfg.pomodoro))))
        .manage(Sessions(Mutex::new(store)))
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
            quit,
            session_state,
            session_start,
            session_stop,
            pomodoro_state,
            pomodoro_start,
            pomodoro_pause,
            pomodoro_reset,
            pomodoro_skip
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hackatime widget");
}
