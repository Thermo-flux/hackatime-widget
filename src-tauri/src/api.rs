//! Hackatime client. Endpoints (see hackclub/hackatime `config/routes.rb` and `swagger/v1/swagger.yaml`):
//!   GET /api/v1/users/my/stats       totals (`total_seconds=true`), AI share (`filter_by_category`), languages/projects (`features`)
//!   GET /api/v1/my/heartbeats        raw heartbeats in a time window, used for live session stats
//! Both accept the same API key as a Bearer token that the WakaTime clients use.
//! `/api/hackatime/v1/.../summaries` is not used: it returns empty `categories`/`languages`
//! and is capped at 366 days.

use crate::activity::{Heartbeat, HeartbeatsResponse};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::stats::{aggregate, Item, Slice, Stats, SummariesResponse};
use chrono::{Duration, Local, NaiveDate, TimeZone, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::sync::OnceLock;

/// The category Hackatime assigns to AI-assisted coding.
const AI_CATEGORY: &str = "ai coding";

/// Total time allowed for one request, and for connecting. A stalled connection fails instead of hanging the UI.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

fn build_client(timeout: std::time::Duration, connect_timeout: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(connect_timeout)
        .user_agent(concat!("hackatime-widget/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("HTTP client config is valid")
}

/// One client for the whole app, so connections are reused.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| build_client(REQUEST_TIMEOUT, CONNECT_TIMEOUT))
}

#[derive(Debug, Clone, Copy)]
pub enum Range {
    Today,
    /// The last 7 days, today included.
    Last7,
    /// The last 30 days, today included.
    Last30,
    AllTime,
}

impl Range {
    pub fn parse(s: &str) -> Self {
        match s {
            "7d" => Self::Last7,
            "30d" => Self::Last30,
            "all" => Self::AllTime,
            _ => Self::Today,
        }
    }

    /// Stable name, used as the stats-cache key (so unknown strings from the UI share Today's slot).
    pub fn name(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Last7 => "7d",
            Self::Last30 => "30d",
            Self::AllTime => "all",
        }
    }

    /// The equally long period just before this one, as `(first day, day after the last)`.
    /// `None` for All time, which has nothing before it.
    fn previous_window(self, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
        let len = match self {
            Self::Today => 1,
            Self::Last7 => 7,
            Self::Last30 => 30,
            Self::AllTime => return None,
        };
        let this_start = self.start_on(today)?;
        Some((this_start - Duration::days(len), this_start))
    }

    /// First local day of the range; `None` means "from the beginning" (the API defaults to 10 years).
    fn start(self) -> Option<NaiveDate> {
        self.start_on(Local::now().date_naive())
    }

    fn start_on(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Today => Some(today),
            Self::Last7 => Some(today - Duration::days(6)),
            Self::Last30 => Some(today - Duration::days(29)),
            Self::AllTime => None,
        }
    }
}

/// Server origin from the configured API URL, so a `.wakatime.cfg`-style `api_url`
/// (`https://host/api/hackatime/v1`) works as-is.
pub fn origin(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    match trimmed.find("/api/") {
        Some(i) => trimmed[..i].to_string(),
        None => trimmed.to_string(),
    }
}

fn local_midnight(day: NaiveDate) -> chrono::DateTime<Local> {
    Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .unwrap_or_else(Local::now)
}

fn local_midnight_iso(day: NaiveDate) -> String {
    local_midnight(day).to_rfc3339()
}

async fn get_json<T: DeserializeOwned>(http: &reqwest::Client, cfg: &Config, path: &str, query: &[(&str, String)]) -> Result<T> {
    if cfg.api_key.trim().is_empty() {
        return Err(Error::MissingApiKey);
    }
    let resp = http
        .get(format!("{}{}", origin(&cfg.base_url), path))
        .bearer_auth(cfg.api_key.trim())
        .query(query)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Status(resp.status().as_u16()));
    }
    Ok(resp.json::<T>().await?)
}

#[derive(Deserialize)]
struct TotalResponse {
    #[serde(default)]
    total_seconds: f64,
}

#[derive(Deserialize)]
struct StatsEnvelope {
    #[serde(default)]
    data: StatsData,
}

#[derive(Deserialize, Default)]
struct StatsData {
    #[serde(default)]
    languages: Vec<Item>,
    #[serde(default)]
    projects: Vec<Item>,
}

fn slices(items: Vec<Item>) -> Vec<Slice> {
    let mut v: Vec<Slice> = items.into_iter().map(|i| Slice { name: i.name, seconds: i.total_seconds }).collect();
    v.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    v
}

const SAMPLE: &str = include_str!("../fixtures/summaries_week.json");

/// Sample data: "today" is the last day of the fixture, every other range is the full week.
fn mock_stats(range: Range) -> Result<Stats> {
    let mut resp: SummariesResponse = serde_json::from_str(SAMPLE).map_err(|e| Error::Config(e.to_string()))?;
    if matches!(range, Range::Today) {
        let last = resp.data.pop();
        resp.data = last.into_iter().collect();
    }
    Ok(Stats { mock: true, ..aggregate(resp) })
}

pub async fn fetch_stats(cfg: &Config, range: Range) -> Result<Stats> {
    fetch_stats_with(client(), cfg, range).await
}

async fn fetch_stats_with(http: &reqwest::Client, cfg: &Config, range: Range) -> Result<Stats> {
    if cfg.mock {
        return mock_stats(range);
    }
    let mut window = vec![("end_date", Local::now().to_rfc3339())];
    if let Some(start) = range.start() {
        window.push(("start_date", local_midnight_iso(start)));
    }
    let with = |extra: &[(&'static str, &str)]| -> Vec<(&'static str, String)> {
        window.iter().cloned().chain(extra.iter().map(|(k, v)| (*k, v.to_string()))).collect()
    };

    let total_q = with(&[("total_seconds", "true")]);
    let ai_q = with(&[("total_seconds", "true"), ("filter_by_category", AI_CATEGORY)]);
    let detail_q = with(&[("features", "languages,projects")]);
    const PATH: &str = "/api/v1/users/my/stats";

    let prev_q = range.previous_window(Local::now().date_naive()).map(|(from, until)| {
        vec![
            ("start_date", local_midnight_iso(from)),
            // The last second of the final day, so the next period's first moment isn't counted twice.
            ("end_date", (local_midnight(until) - Duration::seconds(1)).to_rfc3339()),
            ("total_seconds", "true".to_string()),
        ]
    });
    let (total, ai, detail, prev) = tokio::join!(
        get_json::<TotalResponse>(http, cfg, PATH, &total_q),
        get_json::<TotalResponse>(http, cfg, PATH, &ai_q),
        get_json::<StatsEnvelope>(http, cfg, PATH, &detail_q),
        async {
            match &prev_q {
                Some(q) => get_json::<TotalResponse>(http, cfg, PATH, q).await.ok().map(|t| t.total_seconds),
                None => None,
            }
        },
    );
    let total = total?.total_seconds;
    let detail = detail?.data;
    // The AI figure is a bonus: if that one call fails, keep the rest and mark AI as unavailable.
    let ai = ai.ok().map(|t| t.total_seconds);
    Ok(Stats { previous_total_seconds: prev, ..Stats::from_parts(total, ai, slices(detail.languages), slices(detail.projects)) })
}

/// Raw heartbeats between two unix timestamps.
pub async fn fetch_heartbeats(cfg: &Config, from: i64, to: i64) -> Result<Vec<Heartbeat>> {
    let iso = |ts: i64| Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now).to_rfc3339();
    let q = [("start_time", iso(from)), ("end_time", iso(to))];
    Ok(get_json::<HeartbeatsResponse>(client(), cfg, "/api/v1/my/heartbeats", &q).await?.heartbeats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn the_previous_period_is_the_same_length_right_before() {
        let today = d(2026, 10, 21);
        assert_eq!(Range::Today.previous_window(today), Some((d(2026, 10, 20), d(2026, 10, 21))));
        assert_eq!(Range::Last7.previous_window(today), Some((d(2026, 10, 8), d(2026, 10, 15))));
        assert_eq!(Range::Last30.previous_window(today), Some((d(2026, 8, 23), d(2026, 9, 22))));
        assert_eq!(Range::AllTime.previous_window(today), None);
    }

    #[test]
    fn previous_windows_cross_month_year_and_leap_boundaries() {
        // 7d on 3 Mar 2024 covers 26 Feb..3 Mar; before it is 19..25 Feb.
        assert_eq!(Range::Last7.previous_window(d(2024, 3, 3)), Some((d(2024, 2, 19), d(2024, 2, 26))));
        // Today on 1 Jan looks at 31 Dec.
        assert_eq!(Range::Today.previous_window(d(2026, 1, 1)), Some((d(2025, 12, 31), d(2026, 1, 1))));
        // 30d on 15 Mar 2026: this is 14 Feb..15 Mar, before it is 15 Jan..13 Feb.
        assert_eq!(Range::Last30.previous_window(d(2026, 3, 15)), Some((d(2026, 1, 15), d(2026, 2, 14))));
    }

    #[test]
    fn previous_windows_are_whole_calendar_days_across_dst() {
        // Spring-forward and fall-back days are 23/25 hours long, so windows are built from dates, not
        // from 24-hour steps: every window still starts on a date and spans a whole number of days.
        for today in [d(2026, 3, 9), d(2026, 3, 30), d(2026, 11, 2), d(2026, 10, 26)] {
            for r in [Range::Today, Range::Last7, Range::Last30] {
                let (from, until) = r.previous_window(today).unwrap();
                assert_eq!(until, r.start_on(today).unwrap());
                let days = (until - from).num_days();
                assert_eq!(days, match r { Range::Today => 1, Range::Last7 => 7, _ => 30 });
            }
        }
    }
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn cfg(base_url: &str, key: &str, mock: bool) -> Config {
        Config { api_key: key.into(), base_url: base_url.into(), refresh_secs: 300, mock, ..Default::default() }
    }

    type Log = Arc<Mutex<Vec<String>>>;

    /// Local fake Hackatime. Each route is (text to find in the request, status, body); first match wins.
    /// Returns the base URL (in `.wakatime.cfg` form) and a log of raw requests.
    async fn serve(routes: Vec<(&'static str, &'static str, &'static str)>) -> (String, Log) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/hackatime/v1", listener.local_addr().unwrap());
        let log: Log = Default::default();
        let log2 = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let (routes, log) = (routes.clone(), log2.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let first_line = req.lines().next().unwrap_or("").to_string();
                    let (status, body) = routes
                        .iter()
                        .find(|(needle, _, _)| first_line.contains(needle))
                        .map(|(_, s, b)| (*s, *b))
                        .unwrap_or(("404 Not Found", "{}"));
                    log.lock().unwrap().push(req);
                    let reply = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(reply.as_bytes()).await;
                });
            }
        });
        (url, log)
    }

    const DETAIL: &str = r#"{"data":{"languages":[{"name":"Go","total_seconds":600,"percent":10},{"name":"Rust","total_seconds":2400,"percent":66.6}],"projects":[{"name":"widget","total_seconds":3600}]},"trust_factor":{"trust_level":"green","trust_value":2}}"#;

    #[test]
    fn origin_accepts_wakatime_cfg_style_urls() {
        assert_eq!(origin("https://hackatime.hackclub.com/api/hackatime/v1"), "https://hackatime.hackclub.com");
        assert_eq!(origin("https://hackatime.hackclub.com/api/hackatime/v1/"), "https://hackatime.hackclub.com");
        assert_eq!(origin("https://hackatime.hackclub.com/"), "https://hackatime.hackclub.com");
        assert_eq!(origin("  http://localhost:3000/api/v1  "), "http://localhost:3000");
    }

    #[tokio::test]
    async fn mock_week_totals() {
        let s = fetch_stats(&cfg("", "", true), Range::Last7).await.unwrap();
        assert_eq!(s.total_seconds, 53_100.0);
        assert_eq!(s.ai_seconds, 18_900.0);
        assert_eq!(s.human_seconds, 34_200.0);
        assert!(s.mock && s.ai_available);
        assert_eq!(s.languages[0].name, "Rust");
    }

    #[tokio::test]
    async fn mock_today_is_last_day_only() {
        let s = fetch_stats(&cfg("", "", true), Range::Today).await.unwrap();
        assert_eq!((s.total_seconds, s.ai_seconds), (6_300.0, 2_700.0));
    }

    #[tokio::test]
    async fn missing_key_is_reported_before_any_request() {
        let err = fetch_stats(&cfg("http://127.0.0.1:1", "  ", false), Range::Today).await.unwrap_err();
        assert!(matches!(err, Error::MissingApiKey));
    }

    #[tokio::test]
    async fn combines_total_ai_and_breakdown() {
        let (url, _) = serve(vec![
            ("filter_by_category", "200 OK", r#"{"total_seconds":1200}"#),
            ("total_seconds=true", "200 OK", r#"{"total_seconds":3600}"#),
            ("features=", "200 OK", DETAIL),
        ])
        .await;
        let s = fetch_stats(&cfg(&url, "k", false), Range::Last7).await.unwrap();
        assert_eq!((s.total_seconds, s.ai_seconds, s.human_seconds), (3600.0, 1200.0, 2400.0));
        assert!((s.ai_percent - 33.333).abs() < 0.01);
        assert!(s.ai_available && !s.mock);
        assert_eq!(s.languages[0].name, "Rust"); // sorted by time, not input order
        assert_eq!(s.projects[0].name, "widget");
    }

    #[tokio::test]
    async fn request_shape_matches_the_documented_api() {
        let (url, log) = serve(vec![("total_seconds=true", "200 OK", r#"{"total_seconds":0}"#), ("features=", "200 OK", "{}")]).await;
        fetch_stats(&cfg(&url, "secret", false), Range::Today).await.unwrap();
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 4, "total, AI, breakdown and previous-period requests");
        for req in log.iter() {
            assert!(req.starts_with("GET /api/v1/users/my/stats?"), "{req}");
            assert!(req.contains("start_date=") && req.contains("end_date="), "{req}");
            assert!(req.to_lowercase().contains("authorization: bearer secret"), "{req}");
        }
        let ai = log.iter().find(|r| r.contains("filter_by_category")).expect("an AI request");
        assert!(ai.contains("filter_by_category=ai%20coding") || ai.contains("filter_by_category=ai+coding"), "{ai}");
        assert!(ai.contains("total_seconds=true"));
    }

    #[tokio::test]
    async fn the_previous_period_total_is_fetched_with_its_own_window() {
        let (url, log) = serve(vec![("total_seconds=true", "200 OK", r#"{"total_seconds":900}"#), ("features=", "200 OK", "{}")]).await;
        let s = fetch_stats(&cfg(&url, "k", false), Range::Last7).await.unwrap();
        assert_eq!(s.previous_total_seconds, Some(900.0));
        let log = log.lock().unwrap();
        let starts: std::collections::HashSet<&str> =
            log.iter().filter_map(|r| r.split("start_date=").nth(1)).filter_map(|r| r.split(['&', ' ']).next()).collect();
        assert_eq!(starts.len(), 2, "this period and the previous one start on different days: {starts:?}");
    }

    #[tokio::test]
    async fn a_failing_previous_call_just_drops_the_comparison() {
        // The previous-period request is the only one with an end_date at 23:59:59.
        let (url, _) = serve(vec![
            ("T23%3A59%3A59", "500 Internal Server Error", "{}"),
            ("total_seconds=true", "200 OK", r#"{"total_seconds":3600}"#),
            ("features=", "200 OK", DETAIL),
        ])
        .await;
        let s = fetch_stats(&cfg(&url, "k", false), Range::Today).await.unwrap();
        assert_eq!((s.total_seconds, s.previous_total_seconds), (3600.0, None));
    }

    #[tokio::test]
    async fn all_time_has_no_comparison_and_no_extra_request() {
        let (url, log) = serve(vec![("total_seconds=true", "200 OK", r#"{"total_seconds":5}"#), ("features=", "200 OK", "{}")]).await;
        let s = fetch_stats(&cfg(&url, "k", false), Range::AllTime).await.unwrap();
        assert_eq!(s.previous_total_seconds, None);
        assert_eq!(log.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn all_time_omits_the_start_date() {
        let (url, log) = serve(vec![("total_seconds=true", "200 OK", r#"{"total_seconds":5}"#), ("features=", "200 OK", "{}")]).await;
        fetch_stats(&cfg(&url, "k", false), Range::AllTime).await.unwrap();
        assert!(log.lock().unwrap().iter().all(|r| !r.contains("start_date=")));
    }

    #[tokio::test]
    async fn a_failing_ai_call_degrades_instead_of_failing() {
        let (url, _) = serve(vec![
            ("filter_by_category", "500 Internal Server Error", "{}"),
            ("total_seconds=true", "200 OK", r#"{"total_seconds":3600}"#),
            ("features=", "200 OK", DETAIL),
        ])
        .await;
        let s = fetch_stats(&cfg(&url, "k", false), Range::Today).await.unwrap();
        assert_eq!(s.total_seconds, 3600.0);
        assert!(!s.ai_available);
        assert_eq!((s.ai_seconds, s.human_seconds), (0.0, 3600.0));
    }

    #[tokio::test]
    async fn rejected_key_is_reported() {
        let (url, _) = serve(vec![("/api/", "401 Unauthorized", "{}")]).await;
        let err = fetch_stats(&cfg(&url, "bad", false), Range::Today).await.unwrap_err();
        assert!(matches!(err, Error::Status(401)));
    }

    #[tokio::test]
    async fn unreachable_server_is_a_network_error() {
        let err = fetch_stats(&cfg("http://127.0.0.1:1/api/hackatime/v1", "k", false), Range::Today).await.unwrap_err();
        assert_eq!(err.code(), "network");
    }

    #[tokio::test]
    async fn a_server_that_never_replies_times_out_as_a_network_error() {
        // Accepts connections and then says nothing.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/hackatime/v1", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                held.push(sock); // keep it open, never write
            }
        });
        let short = std::time::Duration::from_millis(300);
        let http = build_client(short, short);
        let started = std::time::Instant::now();
        let err = fetch_stats_with(&http, &cfg(&url, "k", false), Range::Today).await.unwrap_err();
        assert_eq!(err.code(), "network");
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "took {:?}", started.elapsed());
    }

    #[test]
    fn range_starts() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        let today = d(2026, 3, 5);
        assert_eq!(Range::Today.start_on(today), Some(today));
        assert_eq!(Range::Last7.start_on(today), Some(d(2026, 2, 27))); // 7 days including today
        assert_eq!(Range::Last30.start_on(today), Some(d(2026, 2, 4))); // 30 days including today, across a month boundary
        assert_eq!(Range::AllTime.start_on(today), None);
    }

    #[test]
    fn range_keys_parse() {
        assert!(matches!(Range::parse("7d"), Range::Last7));
        assert!(matches!(Range::parse("30d"), Range::Last30));
        assert!(matches!(Range::parse("all"), Range::AllTime));
        assert!(matches!(Range::parse("today"), Range::Today));
        assert!(matches!(Range::parse("whatever"), Range::Today));
    }

    #[tokio::test]
    async fn malformed_json_is_an_error() {
        let (url, _) = serve(vec![("/api/", "200 OK", "not json")]).await;
        assert!(fetch_stats(&cfg(&url, "k", false), Range::Today).await.is_err());
    }

    #[tokio::test]
    async fn heartbeats_request_and_parse() {
        let body = r#"{"start_time":"x","end_time":"y","total_seconds":60,"heartbeats":[{"id":1,"time":1792540800.5,"category":"ai coding","project":"p","language":"Rust","entity":"a.rs"},{"time":1792540860.0}]}"#;
        let (url, log) = serve(vec![("/api/v1/my/heartbeats", "200 OK", body)]).await;
        let hbs = fetch_heartbeats(&cfg(&url, "secret", false), 1_792_540_800, 1_792_544_400).await.unwrap();
        assert_eq!(hbs.len(), 2);
        assert_eq!(hbs[0].category.as_deref(), Some("ai coding"));
        assert_eq!(hbs[1].project, None);
        let req = log.lock().unwrap()[0].clone();
        assert!(req.starts_with("GET /api/v1/my/heartbeats?start_time=2026-10-21T00%3A00%3A00"), "{req}");
        assert!(req.contains("end_time=2026-10-21T01%3A00%3A00"), "{req}");
        assert!(req.to_lowercase().contains("authorization: bearer secret"), "{req}");
    }

    #[tokio::test]
    async fn heartbeats_need_a_key() {
        let err = fetch_heartbeats(&cfg("http://127.0.0.1:1", "", false), 0, 1).await.unwrap_err();
        assert!(matches!(err, Error::MissingApiKey));
    }
}
