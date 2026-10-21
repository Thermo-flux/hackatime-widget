//! The last good stats per range, kept on disk so the widget has something to show when it starts
//! offline or a refresh fails.
use crate::error::Result;
use crate::fsutil::write_atomic;
use crate::stats::Stats;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// A cached entry older than this is called stale. The UI still shows it, but says when it is from.
pub const STALE_AFTER_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub fetched_at: i64,
    pub stats: Stats,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct StatsCache(BTreeMap<String, Entry>);

impl StatsCache {
    /// A missing or unreadable file is an empty cache; it is only ever a convenience.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default()
    }

    pub fn get(&self, range: &str) -> Option<&Entry> {
        self.0.get(range)
    }

    pub fn put(&mut self, range: &str, stats: Stats, now: i64) {
        self.0.insert(range.to_string(), Entry { fetched_at: now, stats });
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string(self).map_err(|e| crate::error::Error::Config(e.to_string()))?;
        write_atomic(path, json.as_bytes())
    }
}

pub fn is_stale(entry: &Entry, now: i64) -> bool {
    now - entry.fetched_at > STALE_AFTER_SECS
}

/// Stores a fresh result for `range` in the cache file (read-modify-write, best effort for the caller).
pub fn store(path: &Path, range: &str, stats: &Stats, now: i64) -> Result<()> {
    let mut cache = StatsCache::load(path);
    cache.put(range, stats.clone(), now);
    cache.save(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(total: f64) -> Stats {
        Stats::from_parts(total, Some(total / 4.0), Vec::new(), Vec::new())
    }

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hackatime-widget-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("stats-cache.json")
    }

    #[test]
    fn round_trips_per_range() {
        let path = temp_file("cache-rt");
        store(&path, "today", &stats(600.0), 1_000).unwrap();
        store(&path, "7d", &stats(6000.0), 2_000).unwrap();
        store(&path, "today", &stats(900.0), 3_000).unwrap(); // replaces, keeps the other range
        let cache = StatsCache::load(&path);
        let today = cache.get("today").unwrap();
        assert_eq!((today.fetched_at, today.stats.total_seconds), (3_000, 900.0));
        assert_eq!(cache.get("7d").unwrap().stats.total_seconds, 6000.0);
        assert!(cache.get("30d").is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_or_corrupt_file_is_an_empty_cache() {
        let path = temp_file("cache-bad");
        assert!(StatsCache::load(&path).get("today").is_none());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert!(StatsCache::load(&path).get("today").is_none());
        // and storing over it recovers
        store(&path, "today", &stats(60.0), 5).unwrap();
        assert!(StatsCache::load(&path).get("today").is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn stale_decision() {
        let e = Entry { fetched_at: 10_000, stats: stats(1.0) };
        assert!(!is_stale(&e, 10_000));
        assert!(!is_stale(&e, 10_000 + STALE_AFTER_SECS));
        assert!(is_stale(&e, 10_000 + STALE_AFTER_SECS + 1));
    }
}
