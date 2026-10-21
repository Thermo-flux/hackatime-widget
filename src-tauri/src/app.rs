//! Tauri glue: commands and the app builder. Compiled only for the real app, because
//! linking Tauri's UI code into unit-test binaries fails on Windows (no comctl32 v6 manifest).

use crate::config::{self, Config, PomodoroLengths};
use crate::error::Result;
use crate::pomodoro::{Pomodoro, Snapshot};
use crate::sessions::{self, SessionStore, Summary};
use crate::stats::Stats;
use crate::{activity, api};
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
    api::fetch_stats(&config::load(), api::Range::parse(&range)).await
}

/// What's saved on disk; a key picked up from `.wakatime.cfg` is used but never copied into it.
#[tauri::command]
fn get_config() -> Config {
    config::load_file()
}

#[tauri::command]
fn set_config(timer: State<Timer>, cfg: Config) -> Result<()> {
    config::save(&cfg)?;
    // A started timer keeps its time (saving the API key mustn't reset a running Pomodoro).
    timer.0.lock().unwrap().set_config(cfg.pomodoro);
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
            get_config,
            set_config,
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
