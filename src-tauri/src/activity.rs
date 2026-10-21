use crate::api;
use crate::config::Config;
use crate::error::Result;
use crate::stats::is_ai;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Mutex;

/// WakaTime's rule: heartbeats closer together than this count as continuous work.
pub const TIMEOUT_SECS: f64 = 120.0;

#[derive(Debug, Clone, Deserialize)]
pub struct Heartbeat {
    pub time: f64,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

/// `GET /api/v1/my/heartbeats` response (other fields are ignored).
#[derive(Debug, Deserialize)]
pub struct HeartbeatsResponse {
    #[serde(default)]
    pub heartbeats: Vec<Heartbeat>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Live {
    pub active_secs: f64,
    pub ai_secs: f64,
    pub human_secs: f64,
    pub ai_percent: f64,
    /// Share of the session spent actually coding.
    pub focus_percent: f64,
    pub top_project: Option<String>,
    pub top_language: Option<String>,
}

fn top(map: &BTreeMap<String, f64>) -> Option<String> {
    map.iter().max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, _)| k.clone())
}

/// Active coding time inside [from, to], from heartbeat gaps.
pub fn measure(hbs: &[Heartbeat], from: f64, to: f64) -> Live {
    let mut v: Vec<&Heartbeat> = hbs.iter().filter(|h| h.time >= from && h.time <= to).collect();
    v.sort_by(|a, b| a.time.total_cmp(&b.time));

    let (mut active, mut ai) = (0.0, 0.0);
    let (mut projects, mut languages): (BTreeMap<String, f64>, BTreeMap<String, f64>) = Default::default();
    for w in v.windows(2) {
        let gap = w[1].time - w[0].time;
        if gap <= 0.0 || gap > TIMEOUT_SECS {
            continue;
        }
        active += gap;
        if w[0].category.as_deref().is_some_and(is_ai) {
            ai += gap;
        }
        if let Some(p) = &w[0].project {
            *projects.entry(p.clone()).or_default() += gap;
        }
        if let Some(l) = &w[0].language {
            *languages.entry(l.clone()).or_default() += gap;
        }
    }

    let span = (to - from).max(0.0);
    Live {
        active_secs: active,
        ai_secs: ai,
        human_secs: active - ai,
        ai_percent: if active > 0.0 { ai / active * 100.0 } else { 0.0 },
        focus_percent: if span > 0.0 { (active / span * 100.0).min(100.0) } else { 0.0 },
        top_project: top(&projects),
        top_language: top(&languages),
    }
}

/// Deterministic fake activity for mock mode: 7 of every 10 minutes active, every 4th minute AI.
pub fn mock_heartbeats(from: f64, to: f64) -> Vec<Heartbeat> {
    let mut out = Vec::new();
    let mut t = from;
    while t <= to {
        let minute = ((t - from) / 60.0) as u64;
        if minute % 10 < 7 {
            out.push(Heartbeat {
                time: t,
                category: Some(if minute % 4 == 0 { "AI Coding" } else { "Coding" }.into()),
                project: Some("hackatime-widget".into()),
                language: Some(if minute % 3 == 0 { "TypeScript" } else { "Rust" }.into()),
            });
        }
        t += 60.0;
    }
    out
}

/// Each poll re-fetches this much before the last fetch's end, so a gap that straddles the
/// boundary (a heartbeat just before it, the next just after) is still measured.
///
/// Limitation: a heartbeat that reaches the server more than this long after its own timestamp
/// (wakatime-cli's offline queue, a machine whose clock runs behind, ingestion lag) is not picked up
/// by an incremental poll. The periodic full re-fetch (`RESYNC_SECS`) catches it later.
pub const OVERLAP_SECS: i64 = 150;

/// Every this often the cache is dropped and the whole session is fetched again.
pub const RESYNC_SECS: i64 = 30 * 60;

/// Heartbeats already downloaded for the running session, so a poll only asks for what's new.
#[derive(Debug, Default)]
pub struct HeartbeatCache {
    session_start: i64,
    /// Unix seconds the last successful fetch ran up to; 0 means nothing is cached.
    fetched_until: i64,
    /// When the cache was last filled from the session's start.
    last_full: i64,
    /// Bumped by `clear`, so a poll that was in flight across a clear doesn't write into the new state.
    generation: u64,
    hbs: Vec<Heartbeat>,
}

type Identity = (u64, Option<String>, Option<String>, Option<String>);

fn identity(h: &Heartbeat) -> Identity {
    (h.time.to_bits(), h.category.clone(), h.project.clone(), h.language.clone())
}

impl HeartbeatCache {
    pub fn clear(&mut self) {
        let generation = self.generation + 1;
        *self = Self { generation, ..Self::default() };
    }

    /// Prepares a fetch for the session starting at `start` and returns `(from, generation)`: where the
    /// fetch should begin and which cache generation it belongs to. Starts over (a full fetch) for a
    /// different session, an empty cache, a clock that went backwards, or when a resync is due.
    fn begin_fetch(&mut self, start: i64, now: i64) -> (i64, u64) {
        if self.session_start != start
            || self.fetched_until == 0
            || now < self.fetched_until
            || now - self.last_full >= RESYNC_SECS
        {
            self.clear();
            self.session_start = start;
            self.last_full = now;
            return (start, self.generation);
        }
        ((self.fetched_until - OVERLAP_SECS).max(start).min(now), self.generation)
    }

    /// Adds a chunk fetched from `from`, skipping heartbeats the cache already holds. Only the cached
    /// entries inside the chunk's window are compared, by full identity, so two different heartbeats
    /// that share a timestamp both survive. The list stays sorted by time.
    fn merge(&mut self, chunk: Vec<Heartbeat>, until: i64, from: i64) {
        let held: std::collections::HashSet<Identity> =
            self.hbs.iter().filter(|h| h.time >= from as f64).map(identity).collect();
        self.hbs.extend(chunk.into_iter().filter(|h| h.time >= from as f64 && !held.contains(&identity(h))));
        self.hbs.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.fetched_until = self.fetched_until.max(until);
    }
}

/// Live activity for the session that began at `start`, fetching through `fetch(from, to)` only the
/// heartbeats not already cached. The lock is never held while `fetch` runs.
pub async fn poll<F, Fut>(cache: &Mutex<HeartbeatCache>, start: i64, now: i64, fetch: F) -> Result<Live>
where
    F: FnOnce(i64, i64) -> Fut,
    Fut: Future<Output = Result<Vec<Heartbeat>>>,
{
    let (from, generation) = cache.lock().unwrap().begin_fetch(start, now);
    let chunk = fetch(from, now).await?;
    let mut c = cache.lock().unwrap();
    if c.generation != generation {
        // The session was stopped or restarted while we were fetching: don't touch the new cache.
        return Ok(measure(&chunk, start as f64, now as f64));
    }
    c.merge(chunk, now, from);
    Ok(measure(&c.hbs, start as f64, now as f64))
}

pub async fn for_session(cfg: &Config, cache: &Mutex<HeartbeatCache>, start: i64, now: i64) -> Result<Live> {
    if cfg.mock {
        return Ok(measure(&mock_heartbeats(start as f64, now as f64), start as f64, now as f64));
    }
    poll(cache, start, now, |from, to| api::fetch_heartbeats(cfg, from, to)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hb(t: f64, cat: &str) -> Heartbeat {
        Heartbeat { time: t, category: Some(cat.into()), project: Some("p".into()), language: Some("Rust".into()) }
    }

    #[test]
    fn no_heartbeats_means_no_activity() {
        let l = measure(&[], 0.0, 600.0);
        assert_eq!((l.active_secs, l.focus_percent, l.top_project), (0.0, 0.0, None));
    }

    #[test]
    fn continuous_heartbeats_sum_their_gaps() {
        let l = measure(&[hb(0.0, "Coding"), hb(60.0, "Coding"), hb(120.0, "Coding")], 0.0, 600.0);
        assert_eq!(l.active_secs, 120.0);
        assert!((l.focus_percent - 20.0).abs() < 1e-9);
    }

    #[test]
    fn gaps_over_the_timeout_are_idle_time() {
        let l = measure(&[hb(0.0, "Coding"), hb(60.0, "Coding"), hb(600.0, "Coding"), hb(660.0, "Coding")], 0.0, 700.0);
        assert_eq!(l.active_secs, 120.0); // 60 + 60, the 540s gap is skipped
    }

    #[test]
    fn a_gap_of_exactly_the_timeout_counts() {
        assert_eq!(measure(&[hb(0.0, "Coding"), hb(120.0, "Coding")], 0.0, 200.0).active_secs, 120.0);
    }

    #[test]
    fn ai_time_is_attributed_by_category() {
        let l = measure(&[hb(0.0, "AI Coding"), hb(60.0, "Coding"), hb(120.0, "Coding")], 0.0, 200.0);
        assert_eq!((l.ai_secs, l.human_secs, l.active_secs), (60.0, 60.0, 120.0));
        assert_eq!(l.ai_percent, 50.0);
    }

    #[test]
    fn heartbeats_outside_the_window_and_unsorted_input() {
        let l = measure(&[hb(120.0, "Coding"), hb(-500.0, "Coding"), hb(0.0, "Coding"), hb(60.0, "Coding"), hb(9999.0, "Coding")], 0.0, 300.0);
        assert_eq!(l.active_secs, 120.0);
    }

    #[test]
    fn picks_the_busiest_project() {
        let mut a = hb(0.0, "Coding");
        a.project = Some("alpha".into());
        let mut b = hb(60.0, "Coding");
        b.project = Some("beta".into());
        let c = Heartbeat { project: Some("beta".into()), ..hb(120.0, "Coding") };
        let d = Heartbeat { project: Some("beta".into()), ..hb(180.0, "Coding") };
        let l = measure(&[a, b, c, d], 0.0, 300.0);
        assert_eq!(l.top_project.as_deref(), Some("beta")); // beta 120s vs alpha 60s
    }

    #[test]
    fn focus_never_exceeds_100() {
        let l = measure(&[hb(0.0, "Coding"), hb(60.0, "Coding")], 0.0, 30.0);
        assert!(l.focus_percent <= 100.0);
    }

    #[test]
    fn mock_activity_is_deterministic_and_plausible() {
        let hbs = mock_heartbeats(0.0, 3600.0);
        let l = measure(&hbs, 0.0, 3600.0);
        assert_eq!(l, measure(&mock_heartbeats(0.0, 3600.0), 0.0, 3600.0));
        assert!(l.focus_percent > 50.0 && l.focus_percent < 80.0, "{}", l.focus_percent);
        assert!(l.ai_secs > 0.0 && l.human_secs > l.ai_secs);
    }

    #[test]
    fn hackatime_category_spelling_counts_as_ai() {
        let l = measure(&[hb(0.0, "ai coding"), hb(60.0, "coding"), hb(120.0, "coding")], 0.0, 200.0);
        assert_eq!(l.ai_secs, 60.0);
    }

    // ---- incremental polling ----

    type Log = Mutex<Vec<(i64, i64)>>;

    /// A fake server holding `all` heartbeats; records the (from, to) of every request.
    fn fake<'a>(all: &'a [Heartbeat], log: &'a Log) -> impl FnOnce(i64, i64) -> std::future::Ready<Result<Vec<Heartbeat>>> + 'a {
        move |from, to| {
            log.lock().unwrap().push((from, to));
            std::future::ready(Ok(all.iter().filter(|h| h.time >= from as f64 && h.time <= to as f64).cloned().collect()))
        }
    }

    /// One heartbeat a minute for four hours, with a 5-minute hole at 1h.
    fn four_hours() -> Vec<Heartbeat> {
        (0..=240)
            .filter(|m| !(60..65).contains(m))
            .map(|m| hb(m as f64 * 60.0, if m % 5 == 0 { "AI Coding" } else { "Coding" }))
            .collect()
    }

    /// Runs a future to completion; these futures are always immediately ready.
    fn ready<T>(fut: impl Future<Output = T>) -> T {
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => panic!("future was not ready"),
        }
    }

    #[test]
    fn merge_drops_duplicates_and_keeps_order() {
        let mut c = HeartbeatCache::default();
        c.merge(vec![hb(60.0, "Coding"), hb(0.0, "Coding")], 100, 0);
        c.merge(vec![hb(60.0, "Coding"), hb(120.0, "Coding")], 200, 0);
        let times: Vec<f64> = c.hbs.iter().map(|h| h.time).collect();
        assert_eq!(times, vec![0.0, 60.0, 120.0]);
        assert_eq!(c.fetched_until, 200);
        c.merge(vec![], 150, 0); // never moves backwards
        assert_eq!(c.fetched_until, 200);
    }

    #[test]
    fn the_first_poll_fetches_everything_and_later_ones_only_the_tail() {
        let (cache, log, all) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()), four_hours());
        for now in [600, 630, 1_000] {
            ready(poll(&cache, 0, now, fake(&all, &log))).unwrap();
        }
        let log = log.lock().unwrap();
        assert_eq!(log[0], (0, 600));
        assert_eq!(log[1], (600 - OVERLAP_SECS, 630)); // a small request, with the overlap
        assert_eq!(log[2], (630 - OVERLAP_SECS, 1_000));
    }

    #[test]
    fn chunked_polling_measures_the_same_as_one_fetch() {
        let (cache, log, all) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()), four_hours());
        let mut last = None;
        // Poll every 30 s through the session, including polls that land inside the 5-minute hole.
        for now in (30..=14_400).step_by(30) {
            last = Some(ready(poll(&cache, 0, now, fake(&all, &log))).unwrap());
        }
        assert_eq!(last.unwrap(), measure(&all, 0.0, 14_400.0));
        // Every request is small except the first and the periodic full re-fetches.
        let log = log.lock().unwrap();
        let big = log.iter().filter(|(f, t)| t - f > OVERLAP_SECS + 30).count();
        assert!((1..=14_400 / RESYNC_SECS as usize).contains(&big), "{big} large requests"); // only the resyncs
    }

    #[test]
    fn a_gap_straddling_a_boundary_is_measured() {
        // Heartbeats at 100 and 190; polls at 150 and 200. The first poll only sees the one at 100.
        let (cache, log) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()));
        let all = vec![hb(100.0, "Coding"), hb(190.0, "Coding")];
        ready(poll(&cache, 0, 150, fake(&all, &log))).unwrap();
        let live = ready(poll(&cache, 0, 200, fake(&all, &log))).unwrap();
        assert_eq!(live.active_secs, 90.0);
    }

    #[test]
    fn a_new_session_or_a_clear_starts_over_and_a_failed_fetch_changes_nothing() {
        let (cache, log, all) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()), four_hours());
        ready(poll(&cache, 0, 600, fake(&all, &log))).unwrap();
        ready(poll(&cache, 1_000, 1_200, fake(&all, &log))).unwrap(); // a different session
        assert_eq!(log.lock().unwrap()[1], (1_000, 1_200));
        cache.lock().unwrap().clear();
        ready(poll(&cache, 1_000, 1_300, fake(&all, &log))).unwrap();
        assert_eq!(log.lock().unwrap()[2], (1_000, 1_300));
        let before = cache.lock().unwrap().fetched_until;
        let failed = ready(poll(&cache, 1_000, 1_400, |_, _| std::future::ready(Err(crate::error::Error::Status(500)))));
        assert!(failed.is_err());
        assert_eq!(cache.lock().unwrap().fetched_until, before);
    }

    #[test]
    fn a_clock_that_goes_backwards_starts_over() {
        let (cache, log, all) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()), four_hours());
        ready(poll(&cache, 0, 1_000, fake(&all, &log))).unwrap();
        let live = ready(poll(&cache, 0, 500, fake(&all, &log))).unwrap();
        assert_eq!(log.lock().unwrap()[1], (0, 500));
        assert_eq!(live, measure(&all, 0.0, 500.0));
    }

    #[test]
    fn heartbeats_sharing_a_timestamp_are_all_kept() {
        let all = vec![hb(0.0, "Coding"), hb(60.0, "AI Coding"), hb(60.0, "Coding"), hb(120.0, "Coding")];
        let (cache, log) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()));
        for now in [70, 100, 130] {
            ready(poll(&cache, 0, now, fake(&all, &log))).unwrap();
        }
        assert_eq!(ready(poll(&cache, 0, 140, fake(&all, &log))).unwrap(), measure(&all, 0.0, 140.0));
        assert_eq!(cache.lock().unwrap().hbs.len(), 4);
    }

    #[test]
    fn a_poll_in_flight_across_a_clear_does_not_write_into_the_new_cache() {
        let (cache, log, all) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()), four_hours());
        ready(poll(&cache, 0, 600, fake(&all, &log))).unwrap();
        // The fetch itself clears the cache (as a session stop/start would while it is in flight).
        let _ = ready(poll(&cache, 0, 700, |_, _| {
            cache.lock().unwrap().clear();
            std::future::ready(Ok(vec![hb(650.0, "Coding")]))
        }));
        let c = cache.lock().unwrap();
        assert!(c.hbs.is_empty());
        assert_eq!(c.fetched_until, 0);
    }

    #[test]
    fn late_heartbeats_are_picked_up_by_the_periodic_resync() {
        let (cache, log) = (Mutex::new(HeartbeatCache::default()), Mutex::new(Vec::new()));
        let mut all = vec![hb(0.0, "Coding"), hb(60.0, "Coding")];
        ready(poll(&cache, 0, 100, fake(&all, &log))).unwrap();
        ready(poll(&cache, 0, 1_000, fake(&all, &log))).unwrap();
        all.push(hb(90.0, "Coding")); // arrives late, with an old timestamp
        let missed = ready(poll(&cache, 0, 1_100, fake(&all, &log))).unwrap();
        assert_eq!(missed.active_secs, 60.0);
        let resynced = ready(poll(&cache, 0, 100 + RESYNC_SECS, fake(&all, &log))).unwrap();
        assert_eq!(resynced.active_secs, 90.0);
    }
}
