use crate::error::{Error, Result};
use chrono::{Duration, FixedOffset, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Sessions shorter than this are treated as accidental clicks and dropped.
pub const MIN_SESSION_SECS: i64 = 30;
const CHART_DAYS: i64 = 14;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: u64,
    /// Unix seconds.
    pub start: i64,
    /// `None` while the session is running.
    pub end: Option<i64>,
}

impl Session {
    fn duration(&self, now: i64) -> i64 {
        (self.end.unwrap_or(now) - self.start).max(0)
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct FileFormat {
    sessions: Vec<Session>,
}

pub struct SessionStore {
    sessions: Vec<Session>,
    /// `None` keeps everything in memory (mock mode).
    path: Option<PathBuf>,
}

impl SessionStore {
    pub fn in_memory(sessions: Vec<Session>) -> Self {
        Self { sessions, path: None }
    }

    /// A missing file starts an empty history. A file that exists but can't be read as sessions is
    /// moved aside to `sessions.corrupt-<timestamp>.json` first, so it can be recovered by hand.
    pub fn load(path: PathBuf) -> Self {
        let sessions = match std::fs::read_to_string(&path) {
            Ok(text) => from_json(&text).unwrap_or_else(|| {
                set_aside(&path);
                Vec::new()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => {
                set_aside(&path);
                Vec::new()
            }
        };
        Self { sessions, path: Some(path) }
    }

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    pub fn active(&self) -> Option<&Session> {
        self.sessions.iter().find(|s| s.end.is_none())
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        crate::fsutil::write_atomic(path, to_json(&self.sessions)?.as_bytes())
    }

    /// Starts a session; if one is already running it is left alone.
    pub fn start(&mut self, now: i64) -> Result<()> {
        if self.active().is_none() {
            let id = self.sessions.iter().map(|s| s.id).max().unwrap_or(0) + 1;
            self.sessions.push(Session { id, start: now, end: None });
            self.save()?;
        }
        Ok(())
    }

    /// Ends the running session. Returns false if it was too short and got discarded.
    pub fn stop(&mut self, now: i64) -> Result<bool> {
        let Some(i) = self.sessions.iter().position(|s| s.end.is_none()) else { return Ok(true) };
        let kept = now - self.sessions[i].start >= MIN_SESSION_SECS;
        if kept {
            self.sessions[i].end = Some(now);
        } else {
            self.sessions.remove(i);
        }
        self.save()?;
        Ok(kept)
    }
}

/// Renames an unreadable sessions file out of the way (best effort).
fn set_aside(path: &std::path::Path) {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let _ = std::fs::rename(path, path.with_file_name(format!("sessions.corrupt-{stamp}.json")));
}

pub fn to_json(sessions: &[Session]) -> Result<String> {
    serde_json::to_string_pretty(&FileFormat { sessions: sessions.to_vec() })
        .map_err(|e| Error::Config(e.to_string()))
}

/// Tolerates the UTF-8 BOM that Notepad and PowerShell add.
pub fn from_json(text: &str) -> Option<Vec<Session>> {
    serde_json::from_str::<FileFormat>(text.trim_start_matches('\u{feff}')).ok().map(|f| f.sessions)
}

// ---- Statistics ------------------------------------------------------------

#[derive(Debug, Serialize, PartialEq)]
pub struct DayTotal {
    pub date: String,
    pub secs: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Summary {
    pub total_sessions: usize,
    pub total_secs: i64,
    pub avg_secs: i64,
    pub longest_secs: i64,
    pub sessions_today: usize,
    pub today_secs: i64,
    pub week_secs: i64,
    pub current_streak: u32,
    pub longest_streak: u32,
    /// Days with a session in the last 14 days (today included).
    pub active_days: u32,
    pub chart_days: u32,
    /// Last 14 days, oldest first.
    pub daily: Vec<DayTotal>,
}

fn day_of(ts: i64, tz: FixedOffset) -> NaiveDate {
    tz.timestamp_opt(ts, 0).single().expect("valid timestamp").date_naive()
}

fn midnight(d: NaiveDate, tz: FixedOffset) -> i64 {
    tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap()).single().expect("fixed offset is unambiguous").timestamp()
}

/// Splits [start, end) at local midnights so a late-night session counts toward both days.
fn split_by_day(start: i64, end: i64, tz: FixedOffset) -> Vec<(NaiveDate, i64)> {
    let mut out = Vec::new();
    let mut cur = start;
    while cur < end {
        let d = day_of(cur, tz);
        let seg_end = end.min(midnight(d + Duration::days(1), tz));
        out.push((d, seg_end - cur));
        cur = seg_end;
    }
    out
}

/// A running session counts as ending now, so totals tick up live.
pub fn summarize(sessions: &[Session], now: i64, tz: FixedOffset) -> Summary {
    let today = day_of(now, tz);
    let mut per_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for s in sessions {
        for (d, secs) in split_by_day(s.start, s.end.unwrap_or(now), tz) {
            *per_day.entry(d).or_default() += secs;
        }
    }

    let total_secs: i64 = sessions.iter().map(|s| s.duration(now)).sum();
    let n = sessions.len();
    let active: BTreeSet<NaiveDate> = per_day.iter().filter(|(_, &v)| v > 0).map(|(&d, _)| d).collect();

    // The streak survives until the end of today, so an empty morning doesn't reset it.
    let mut d = if active.contains(&today) { today } else { today - Duration::days(1) };
    let mut current_streak = 0;
    while active.contains(&d) {
        current_streak += 1;
        d -= Duration::days(1);
    }

    let (mut longest_streak, mut run, mut prev): (u32, u32, Option<NaiveDate>) = (0, 0, None);
    for &day in &active {
        run = if prev.is_some_and(|p| p + Duration::days(1) == day) { run + 1 } else { 1 };
        longest_streak = longest_streak.max(run);
        prev = Some(day);
    }

    let daily: Vec<DayTotal> = (0..CHART_DAYS)
        .rev()
        .map(|k| {
            let date = today - Duration::days(k);
            DayTotal { date: date.to_string(), secs: per_day.get(&date).copied().unwrap_or(0) }
        })
        .collect();

    Summary {
        total_sessions: n,
        total_secs,
        avg_secs: if n > 0 { total_secs / n as i64 } else { 0 },
        longest_secs: sessions.iter().map(|s| s.duration(now)).max().unwrap_or(0),
        sessions_today: sessions.iter().filter(|s| day_of(s.start, tz) == today).count(),
        today_secs: per_day.get(&today).copied().unwrap_or(0),
        week_secs: daily.iter().rev().take(7).map(|d| d.secs).sum(),
        current_streak,
        longest_streak,
        active_days: daily.iter().filter(|d| d.secs > 0).count() as u32,
        chart_days: CHART_DAYS as u32,
        daily,
    }
}

/// Believable sample history for mock mode: skips two days to show a streak break.
pub fn sample_history(now: i64, tz: FixedOffset) -> Vec<Session> {
    let today = day_of(now, tz);
    let mut out = Vec::new();
    let mut id = 0;
    let mut push = |start: i64, mins: i64| {
        id += 1;
        out.push(Session { id, start, end: Some(start + mins * 60) });
    };
    for k in (1..=13).rev().filter(|k| *k != 3 && *k != 8) {
        let base = midnight(today - Duration::days(k), tz) + 10 * 3600;
        push(base, 40 + (k * 17) % 80);
        if k % 2 == 0 {
            push(base + 4 * 3600, 30 + (k * 11) % 45);
        }
    }
    push(now - 3 * 3600 - 1800, 45);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTC: i32 = 0;
    fn tz(secs: i32) -> FixedOffset {
        FixedOffset::east_opt(secs).unwrap()
    }
    /// 2026-10-21 00:00:00 UTC.
    const DAY0: i64 = 1_792_540_800;
    const D: i64 = 86_400;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hackatime-widget-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_and_the_store_starts_empty() {
        let dir = temp_dir("corrupt");
        let path = dir.join("sessions.json");
        std::fs::write(&path, "{\"sessions\": [ {\"id\": 1, \"sta").unwrap();
        let store = SessionStore::load(path.clone());
        assert!(store.sessions().is_empty());
        assert!(!path.exists());
        let moved: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("sessions.corrupt-") && n.ends_with(".json"))
            .collect();
        assert_eq!(moved.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_starts_empty_and_a_saved_store_round_trips() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("sessions.json");
        let mut store = SessionStore::load(path.clone());
        assert!(store.sessions().is_empty());
        store.start(DAY0).unwrap();
        assert!(store.stop(DAY0 + 600).unwrap());
        let back = SessionStore::load(path);
        assert_eq!(back.sessions(), &[Session { id: 1, start: DAY0, end: Some(DAY0 + 600) }]);
        let leftovers = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 1); // no .tmp or .corrupt files
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn done(id: u64, start: i64, mins: i64) -> Session {
        Session { id, start, end: Some(start + mins * 60) }
    }

    #[test]
    fn empty_history_is_all_zero() {
        let s = summarize(&[], DAY0 + 3600, tz(UTC));
        assert_eq!((s.total_sessions, s.total_secs, s.avg_secs, s.current_streak, s.active_days), (0, 0, 0, 0, 0));
        assert_eq!(s.daily.len(), 14);
    }

    #[test]
    fn totals_average_and_longest() {
        let sessions = [done(1, DAY0 + 3600, 60), done(2, DAY0 + 7200, 30)];
        let s = summarize(&sessions, DAY0 + 6 * 3600, tz(UTC));
        assert_eq!(s.total_sessions, 2);
        assert_eq!(s.total_secs, 90 * 60);
        assert_eq!(s.avg_secs, 45 * 60);
        assert_eq!(s.longest_secs, 60 * 60);
        assert_eq!((s.sessions_today, s.today_secs), (2, 90 * 60));
    }

    #[test]
    fn running_session_counts_up_to_now() {
        let sessions = [Session { id: 1, start: DAY0 + 3600, end: None }];
        let s = summarize(&sessions, DAY0 + 3600 + 600, tz(UTC));
        assert_eq!((s.total_secs, s.today_secs), (600, 600));
    }

    #[test]
    fn session_over_midnight_splits_between_days() {
        // 23:30 -> 00:30
        let sessions = [done(1, DAY0 - 1800, 60)];
        let s = summarize(&sessions, DAY0 + 3600, tz(UTC));
        assert_eq!(s.today_secs, 1800);
        assert_eq!(s.daily[12].secs, 1800); // yesterday
        assert_eq!(s.total_secs, 3600);
        assert_eq!(s.active_days, 2);
    }

    #[test]
    fn local_timezone_moves_the_day_boundary() {
        // 22:00 UTC is already tomorrow at UTC+3.
        let sessions = [done(1, DAY0 - 2 * 3600, 30)];
        let s = summarize(&sessions, DAY0 + 3600, tz(3 * 3600));
        assert_eq!(s.today_secs, 1800);
    }

    #[test]
    fn streak_counts_consecutive_days_ending_today() {
        let sessions = [done(1, DAY0 - 2 * D + 3600, 30), done(2, DAY0 - D + 3600, 30), done(3, DAY0 + 3600, 30)];
        let s = summarize(&sessions, DAY0 + 5 * 3600, tz(UTC));
        assert_eq!((s.current_streak, s.longest_streak), (3, 3));
    }

    #[test]
    fn streak_survives_an_empty_today() {
        let sessions = [done(1, DAY0 - 2 * D + 3600, 30), done(2, DAY0 - D + 3600, 30)];
        let s = summarize(&sessions, DAY0 + 3600, tz(UTC));
        assert_eq!(s.current_streak, 2);
    }

    #[test]
    fn streak_breaks_after_a_missed_day() {
        let sessions = [done(1, DAY0 - 3 * D + 3600, 30), done(2, DAY0 + 3600, 30)];
        let s = summarize(&sessions, DAY0 + 5 * 3600, tz(UTC));
        assert_eq!((s.current_streak, s.longest_streak), (1, 1));
    }

    #[test]
    fn longest_streak_remembers_the_past() {
        let mut v = Vec::new();
        for k in 10..=14 {
            v.push(done(k as u64, DAY0 - k * D + 3600, 30));
        }
        v.push(done(99, DAY0 + 3600, 30));
        let s = summarize(&v, DAY0 + 5 * 3600, tz(UTC));
        assert_eq!((s.current_streak, s.longest_streak), (1, 5));
    }

    #[test]
    fn active_days_and_week_use_the_window() {
        let sessions = [done(1, DAY0 - 20 * D + 3600, 60), done(2, DAY0 - 5 * D + 3600, 60), done(3, DAY0 - 8 * D + 3600, 60)];
        let s = summarize(&sessions, DAY0 + 3600, tz(UTC));
        assert_eq!(s.active_days, 2); // the 20-day-old one is outside the chart
        assert_eq!(s.week_secs, 3600); // only the 5-day-old one is in the last 7 days
        assert_eq!(s.total_sessions, 3);
    }

    #[test]
    fn start_is_idempotent_and_ids_increase() {
        let mut st = SessionStore::in_memory(vec![done(7, DAY0, 60)]);
        st.start(DAY0 + 4 * 3600).unwrap();
        st.start(DAY0 + 5 * 3600).unwrap();
        assert_eq!(st.sessions().len(), 2);
        assert_eq!(st.active().unwrap().id, 8);
        assert_eq!(st.active().unwrap().start, DAY0 + 4 * 3600);
    }

    #[test]
    fn stop_keeps_real_sessions_and_drops_accidental_ones() {
        let mut st = SessionStore::in_memory(vec![]);
        st.start(DAY0).unwrap();
        assert!(!st.stop(DAY0 + 5).unwrap());
        assert!(st.sessions().is_empty());
        st.start(DAY0).unwrap();
        assert!(st.stop(DAY0 + 600).unwrap());
        assert_eq!(st.sessions()[0].end, Some(DAY0 + 600));
        assert!(st.active().is_none());
    }

    #[test]
    fn stop_without_a_session_is_harmless() {
        assert!(SessionStore::in_memory(vec![]).stop(DAY0).unwrap());
    }

    #[test]
    fn json_round_trip_and_bom() {
        let sessions = vec![done(1, DAY0, 10), Session { id: 2, start: DAY0 + 100, end: None }];
        let text = to_json(&sessions).unwrap();
        assert_eq!(from_json(&text).unwrap(), sessions);
        assert_eq!(from_json(&format!("\u{feff}{text}")).unwrap(), sessions);
        assert!(from_json("nope").is_none());
    }

    #[test]
    fn sample_history_has_streak_breaks_and_a_session_today() {
        let now = DAY0 + 15 * 3600;
        let h = sample_history(now, tz(UTC));
        assert!(h.iter().all(|s| s.end.is_some()));
        let s = summarize(&h, now, tz(UTC));
        assert_eq!(s.sessions_today, 1);
        assert!(s.active_days < 14 && s.active_days >= 10);
        assert!(s.longest_streak > s.current_streak || s.current_streak >= 1);
    }
}
