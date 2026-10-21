use crate::api;
use crate::config::Config;
use crate::error::Result;
use crate::stats::is_ai;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

pub async fn for_session(cfg: &Config, start: i64, now: i64) -> Result<Live> {
    let (from, to) = (start as f64, now as f64);
    let hbs = if cfg.mock { mock_heartbeats(from, to) } else { api::fetch_heartbeats(cfg, start, now).await? };
    Ok(measure(&hbs, from, to))
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
}
