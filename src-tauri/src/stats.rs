use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---- Sample-data format (summaries-style fixture used by mock mode) ----------

#[derive(Debug, Deserialize)]
pub struct SummariesResponse {
    #[serde(default)]
    pub data: Vec<Day>,
}

#[derive(Debug, Deserialize)]
pub struct Day {
    #[serde(default)]
    pub grand_total: GrandTotal,
    #[serde(default)]
    pub categories: Vec<Item>,
    #[serde(default)]
    pub languages: Vec<Item>,
    #[serde(default)]
    pub projects: Vec<Item>,
}

#[derive(Debug, Default, Deserialize)]
pub struct GrandTotal {
    #[serde(default)]
    pub total_seconds: f64,
}

#[derive(Debug, Deserialize)]
pub struct Item {
    pub name: String,
    #[serde(default)]
    pub total_seconds: f64,
}

// ---- What the widget UI receives -----------------------------------------

#[derive(Debug, Serialize, Default)]
pub struct Stats {
    pub total_seconds: f64,
    pub ai_seconds: f64,
    pub human_seconds: f64,
    pub ai_percent: f64,
    pub categories: Vec<Slice>,
    pub languages: Vec<Slice>,
    pub projects: Vec<Slice>,
    /// True when this is bundled sample data, so the UI can say so.
    pub mock: bool,
    /// False when the AI share couldn't be fetched, so the UI shows "unknown" instead of 0%.
    pub ai_available: bool,
}

impl Stats {
    /// `ai` is `None` when the AI figure is unavailable.
    pub fn from_parts(total: f64, ai: Option<f64>, languages: Vec<Slice>, projects: Vec<Slice>) -> Self {
        let ai_secs = ai.unwrap_or(0.0).clamp(0.0, total.max(0.0));
        Stats {
            total_seconds: total,
            ai_seconds: ai_secs,
            human_seconds: (total - ai_secs).max(0.0),
            ai_percent: if total > 0.0 { ai_secs / total * 100.0 } else { 0.0 },
            categories: Vec::new(),
            languages,
            projects,
            mock: false,
            ai_available: ai.is_some(),
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Slice {
    pub name: String,
    pub seconds: f64,
}

/// WakaTime reports AI time as the "AI Coding" category.
pub(crate) fn is_ai(category: &str) -> bool {
    let c = category.to_ascii_lowercase();
    c == "ai" || c.starts_with("ai ") || c.contains("ai coding")
}

fn merge(days: &[Day], pick: impl Fn(&Day) -> &Vec<Item>) -> Vec<Slice> {
    let mut acc: BTreeMap<String, f64> = BTreeMap::new();
    for d in days {
        for i in pick(d) {
            *acc.entry(i.name.clone()).or_default() += i.total_seconds;
        }
    }
    let mut v: Vec<Slice> = acc
        .into_iter()
        .map(|(name, seconds)| Slice { name, seconds })
        .collect();
    v.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    v
}

pub fn aggregate(resp: SummariesResponse) -> Stats {
    let days = resp.data;
    let total: f64 = days.iter().map(|d| d.grand_total.total_seconds).sum();
    let categories = merge(&days, |d| &d.categories);
    let ai: f64 = categories
        .iter()
        .filter(|c| is_ai(&c.name))
        .map(|c| c.seconds)
        .sum();

    Stats {
        total_seconds: total,
        ai_seconds: ai,
        human_seconds: (total - ai).max(0.0),
        ai_percent: if total > 0.0 { ai / total * 100.0 } else { 0.0 },
        categories,
        languages: merge(&days, |d| &d.languages),
        projects: merge(&days, |d| &d.projects),
        mock: false,
        ai_available: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_ai_from_human() {
        let json = r#"{"data":[{"grand_total":{"total_seconds":3600},
            "categories":[{"name":"Coding","total_seconds":2400},{"name":"AI Coding","total_seconds":1200}],
            "languages":[],"projects":[]}]}"#;
        let s = aggregate(serde_json::from_str(json).unwrap());
        assert_eq!(s.ai_seconds, 1200.0);
        assert_eq!(s.human_seconds, 2400.0);
        assert!((s.ai_percent - 33.333).abs() < 0.01);
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    fn run(json: &str) -> Stats {
        aggregate(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn empty_data_is_all_zero() {
        let s = run(r#"{"data":[]}"#);
        assert_eq!((s.total_seconds, s.ai_percent), (0.0, 0.0));
    }

    #[test]
    fn missing_fields_default() {
        let s = run(r#"{"data":[{}]}"#);
        assert_eq!(s.total_seconds, 0.0);
    }

    #[test]
    fn merges_and_sorts_across_days() {
        let s = run(r#"{"data":[
            {"grand_total":{"total_seconds":10},"languages":[{"name":"Go","total_seconds":4},{"name":"Rust","total_seconds":6}]},
            {"grand_total":{"total_seconds":10},"languages":[{"name":"Go","total_seconds":7}]}]}"#);
        assert_eq!(s.languages[0].name, "Go");
        assert_eq!(s.languages[0].seconds, 11.0);
    }

    #[test]
    fn ai_category_matching() {
        assert!(is_ai("AI Coding") && is_ai("ai coding") && is_ai("AI"));
        assert!(!is_ai("Coding") && !is_ai("Debugging") && !is_ai("Maintaining"));
    }

    #[test]
    fn from_parts_splits_and_flags_missing_ai() {
        let s = Stats::from_parts(3600.0, Some(900.0), vec![], vec![]);
        assert_eq!((s.ai_seconds, s.human_seconds, s.ai_percent), (900.0, 2700.0, 25.0));
        assert!(s.ai_available);
        let s = Stats::from_parts(3600.0, None, vec![], vec![]);
        assert_eq!((s.ai_seconds, s.human_seconds), (0.0, 3600.0));
        assert!(!s.ai_available);
    }

    #[test]
    fn from_parts_clamps_ai_to_the_total() {
        let s = Stats::from_parts(100.0, Some(500.0), vec![], vec![]);
        assert_eq!((s.ai_seconds, s.human_seconds, s.ai_percent), (100.0, 0.0, 100.0));
        assert_eq!(Stats::from_parts(0.0, Some(0.0), vec![], vec![]).ai_percent, 0.0);
    }

    #[test]
    fn human_time_never_negative() {
        let s = run(r#"{"data":[{"grand_total":{"total_seconds":10},"categories":[{"name":"AI Coding","total_seconds":20}]}]}"#);
        assert_eq!(s.human_seconds, 0.0);
    }
}
