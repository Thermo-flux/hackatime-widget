// The Tauri glue is excluded from test builds, which leaves some helpers unused there.
#![cfg_attr(test, allow(dead_code))]

mod activity;
mod api;
mod cache;
#[cfg(not(test))]
mod app;
mod config;
mod error;
mod focus_day;
mod fsutil;
mod modes;
mod notice;
mod placement;
mod pomodoro;
mod sessions;
mod stats;
mod themes;

#[cfg(not(test))]
pub use app::run;
