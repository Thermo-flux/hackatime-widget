use crate::error::{Error, Result};
use crate::modes::Mode;
use crate::pomodoro::PomodoroConfig;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const DEFAULT_BASE_URL: &str = "https://hackatime.hackclub.com/api/hackatime/v1";
pub const DEFAULT_HOTKEY: &str = "CommandOrControl+Alt+Shift+H";

/// Every field is optional so a hand-written `{"api_key": "..."}` is enough.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    pub refresh_secs: u64,
    /// Serve bundled sample data instead of calling the API.
    pub mock: bool,
    /// The mascot keeps its faces but only speaks when petted.
    pub quiet_mascot: bool,
    /// Global shortcut that toggles compact mode.
    pub compact_hotkey: String,
    /// Full widget or compact pill; the app starts in the mode it was left in.
    #[serde(deserialize_with = "lenient")]
    pub ui_mode: Mode,
    /// Window position per mode (outer top-left, physical pixels; see `modes.rs` for why physical).
    #[serde(deserialize_with = "lenient")]
    pub full_pos: Option<WindowPos>,
    #[serde(deserialize_with = "lenient")]
    pub compact_pos: Option<WindowPos>,
    pub pomodoro: PomodoroConfig,
}

/// A broken window-state value (hand edit, older or newer app) falls back to its default instead of
/// failing the whole file, which would also drop the API key.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> std::result::Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPos {
    pub x: i32,
    pub y: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: DEFAULT_BASE_URL.into(),
            refresh_secs: 300,
            mock: false,
            quiet_mascot: false,
            compact_hotkey: DEFAULT_HOTKEY.into(),
            ui_mode: Mode::Full,
            full_pos: None,
            compact_pos: None,
            pomodoro: PomodoroConfig::default(),
        }
    }
}

impl Config {
    /// The position remembered for `mode`.
    pub fn pos_for(&self, mode: Mode) -> Option<(i32, i32)> {
        let p = match mode {
            Mode::Full => self.full_pos,
            Mode::Compact => self.compact_pos,
        };
        p.map(|p| (p.x, p.y))
    }

    /// This config with the window state changed: the mode (when given) and positions per mode. The rest is kept.
    pub fn with_window(mut self, mode: Option<Mode>, positions: &[(Mode, (i32, i32))]) -> Self {
        if let Some(m) = mode {
            self.ui_mode = m;
        }
        for &(m, (x, y)) in positions {
            let slot = match m {
                Mode::Full => &mut self.full_pos,
                Mode::Compact => &mut self.compact_pos,
            };
            *slot = Some(WindowPos { x, y });
        }
        self
    }
}

/// Saves the window mode and/or positions; nothing else in the file is lost.
pub fn save_window(mode: Option<Mode>, positions: &[(Mode, (i32, i32))]) -> Result<()> {
    update(|c| Ok(c.with_window(mode, positions))).map(|_| ())
}

// ---- Where the config lives ------------------------------------------------

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".hackatime-widget-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Standard per-user config directory for `os` (`std::env::consts::OS`), given an env lookup.
fn os_config_dir(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    match os {
        "windows" => var("APPDATA"),
        "macos" => var("HOME").map(|h| h.join("Library").join("Application Support")),
        _ => var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config"))),
    }
}

fn default_config_path() -> PathBuf {
    let beside_exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("config.json")));
    if let Some(p) = &beside_exe {
        // Portable: keep it next to the app when that already holds a config or is writable.
        // Installed under e.g. Program Files, it isn't, so fall back to the user config dir.
        if p.exists() || p.parent().is_some_and(dir_writable) {
            return p.clone();
        }
    }
    os_config_dir(std::env::consts::OS, &|k| std::env::var(k).ok())
        .map(|d| d.join("hackatime-widget").join("config.json"))
        .or(beside_exe)
        .unwrap_or_else(|| PathBuf::from("config.json"))
}

/// `HACKATIME_WIDGET_CONFIG` if set, else `config.json` beside the executable (portable use),
/// else the per-user config directory when the app's folder isn't writable.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("HACKATIME_WIDGET_CONFIG") {
        return PathBuf::from(p);
    }
    static DEFAULT: OnceLock<PathBuf> = OnceLock::new();
    DEFAULT.get_or_init(default_config_path).clone()
}

/// Other app data (sessions, webview cache) lives in the same folder as the config.
pub fn data_path(file: &str) -> PathBuf {
    config_path().with_file_name(file)
}

// ---- Reusing an existing Hackatime/WakaTime setup --------------------------

/// `(api_key, api_url)` from the `[settings]` section of a `.wakatime.cfg`.
pub fn parse_wakatime_cfg(text: &str) -> (Option<String>, Option<String>) {
    let (mut key, mut url, mut in_settings) = (None, None, false);
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_settings = section.trim().eq_ignore_ascii_case("settings");
        } else if in_settings {
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim().trim_matches('"').to_string();
                match k.trim().to_ascii_lowercase().as_str() {
                    "api_key" | "apikey" if !v.is_empty() => key = Some(v),
                    "api_url" if !v.is_empty() => url = Some(v),
                    _ => {}
                }
            }
        }
    }
    (key, url)
}

fn wakatime_cfg_path() -> Option<PathBuf> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let home = var("WAKATIME_HOME").or_else(|| var("USERPROFILE")).or_else(|| var("HOME"))?;
    Some(PathBuf::from(home).join(".wakatime.cfg"))
}

/// Fills in the key (and server) from a `.wakatime.cfg` when the widget has no key of its own.
/// A cfg pointing at wakatime.com is ignored: that key isn't a Hackatime key.
pub fn with_wakatime_cfg(mut cfg: Config, wakatime: (Option<String>, Option<String>)) -> Config {
    if !cfg.api_key.trim().is_empty() {
        return cfg;
    }
    let (key, url) = wakatime;
    if url.as_deref().is_some_and(|u| u.contains("wakatime.com")) {
        return cfg;
    }
    if let Some(key) = key {
        cfg.api_key = key;
        if let (Some(url), true) = (url, cfg.base_url == DEFAULT_BASE_URL) {
            cfg.base_url = url;
        }
    }
    cfg
}

// ---- Load / save -------------------------------------------------------------

/// Parse config text, tolerating the UTF-8 BOM that Notepad and PowerShell add.
fn parse(text: &str) -> Option<Config> {
    let mut cfg: Config = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    cfg.pomodoro = cfg.pomodoro.clamped();
    Some(cfg)
}

/// What the webview may know about the config: never the key, never the server URL.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigView {
    pub has_key: bool,
    pub refresh_secs: u64,
    pub mock: bool,
    pub quiet_mascot: bool,
    pub compact_hotkey: String,
    pub ui_mode: Mode,
    pub pomodoro: PomodoroConfig,
}

impl From<&Config> for ConfigView {
    fn from(c: &Config) -> Self {
        Self {
            has_key: !c.api_key.trim().is_empty(),
            refresh_secs: c.refresh_secs,
            mock: c.mock,
            quiet_mascot: c.quiet_mascot,
            compact_hotkey: c.compact_hotkey.clone(),
            ui_mode: c.ui_mode,
            pomodoro: c.pomodoro.clone(),
        }
    }
}

/// A Hackatime key is a UUID (8-4-4-4-12 hex digits). Returns it trimmed.
pub fn validate_api_key(key: &str) -> Result<String> {
    let key = key.trim();
    let groups: Vec<&str> = key.split('-').collect();
    let shaped = groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()));
    if shaped { Ok(key.to_string()) } else { Err(Error::InvalidKey) }
}

impl Config {
    /// This config with a new, validated API key; everything else is kept.
    pub fn with_api_key(mut self, key: &str) -> Result<Self> {
        self.api_key = validate_api_key(key)?;
        Ok(self)
    }
}

/// Saves a new API key; nothing else in the file is lost.
pub fn save_api_key(key: &str) -> Result<()> {
    update(|c| c.with_api_key(key)).map(|_| ())
}

/// The three lengths the settings UI edits, in minutes.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PomodoroLengths {
    pub focus_min: u64,
    pub short_break_min: u64,
    pub long_break_min: u64,
}

impl Config {
    /// This config with new timer lengths (clamped). Everything else, the API key and the other
    /// Pomodoro options included, is kept.
    pub fn with_lengths(mut self, l: PomodoroLengths) -> Self {
        let p = &mut self.pomodoro;
        (p.focus_min, p.short_break_min, p.long_break_min) = (l.focus_min, l.short_break_min, l.long_break_min);
        self.pomodoro = self.pomodoro.clamped();
        self
    }
}

/// Saves new timer lengths; nothing else in the file is lost. Returns the saved timer settings.
pub fn save_lengths(lengths: PomodoroLengths) -> Result<PomodoroConfig> {
    update(|c| Ok(c.with_lengths(lengths))).map(|c| c.pomodoro)
}

/// Saves the quiet-mascot switch; nothing else in the file is lost.
pub fn save_quiet_mascot(quiet: bool) -> Result<()> {
    update(|c| Ok(Config { quiet_mascot: quiet, ..c })).map(|_| ())
}

/// The config file as saved on disk (what the settings UI edits). Unreadable reads as the defaults; that is fine
/// for reading, but never for a save, which goes through `update`.
pub fn load_file() -> Config {
    read_strict(&config_path()).ok().flatten().unwrap_or_default()
}

/// The file at `path`: `None` when there is none, an error when it exists but can't be read or parsed.
fn read_strict(path: &Path) -> Result<Option<Config>> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Config(format!("{}: {e}", path.display()))),
        Ok(text) => parse(&text).map(Some).ok_or_else(|| Error::Config(format!("{} is not valid config JSON; not overwriting it", path.display()))),
    }
}

/// Every read-modify-write of the config file holds this lock, so two writers (the UI's settings, the window
/// position flush, a mode switch) can never interleave and drop each other's change.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Reads the file, applies `change`, writes it back, all under `WRITE_LOCK`. A missing file starts from the
/// defaults; a file that exists but can't be read or parsed is left alone and the change is refused, so a
/// transient read error or a typo from a hand edit never replaces the user's config (and key) with defaults.
fn update_at(path: &Path, change: impl FnOnce(Config) -> Result<Config>) -> Result<Config> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = change(read_strict(path)?.unwrap_or_default())?;
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| Error::Config(e.to_string()))?;
    crate::fsutil::write_atomic(path, json.as_bytes())?;
    Ok(cfg)
}

fn update(change: impl FnOnce(Config) -> Result<Config>) -> Result<Config> {
    update_at(&config_path(), change)
}

/// The config the app actually uses: the file, plus env overrides and a detected `.wakatime.cfg`.
/// `HACKATIME_WIDGET_NO_WAKATIME_CFG=1` turns the cfg lookup off.
pub fn load() -> Config {
    let mut cfg = load_file();
    if std::env::var("HACKATIME_WIDGET_MOCK").is_ok_and(|v| v == "1") {
        cfg.mock = true;
    }
    if !cfg.mock && std::env::var("HACKATIME_WIDGET_NO_WAKATIME_CFG").is_err() {
        if let Some(text) = wakatime_cfg_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            cfg = with_wakatime_cfg(cfg, parse_wakatime_cfg(&text));
        }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_utf8_bom() {
        let cfg = parse("\u{feff}{\"api_key\":\"k\",\"base_url\":\"http://x\",\"refresh_secs\":60}").unwrap();
        assert_eq!(cfg.api_key, "k");
        assert!(!cfg.mock);
        assert_eq!(cfg.pomodoro.focus_min, 25);
    }

    #[test]
    fn a_config_with_only_a_key_is_enough() {
        let cfg = parse(r#"{"api_key":"abc"}"#).unwrap();
        assert_eq!((cfg.api_key.as_str(), cfg.base_url.as_str(), cfg.refresh_secs), ("abc", DEFAULT_BASE_URL, 300));
        assert_eq!(parse("{}").unwrap().api_key, "");
    }

    #[test]
    fn partial_pomodoro_block_keeps_defaults() {
        let cfg = parse(r#"{"api_key":"k","pomodoro":{"focus_min":50}}"#).unwrap();
        assert_eq!((cfg.pomodoro.focus_min, cfg.pomodoro.short_break_min), (50, 5));
    }

    #[test]
    fn out_of_range_pomodoro_lengths_are_clamped_on_load() {
        let cfg = parse(r#"{"pomodoro":{"focus_min":0,"short_break_min":500,"long_break_min":91}}"#).unwrap();
        assert_eq!((cfg.pomodoro.focus_min, cfg.pomodoro.short_break_min, cfg.pomodoro.long_break_min), (1, 60, 90));
    }

    #[test]
    fn the_compact_hotkey_has_a_default_and_can_be_changed() {
        assert_eq!(parse("{}").unwrap().compact_hotkey, DEFAULT_HOTKEY);
        assert_eq!(parse(r#"{"compact_hotkey":"Ctrl+Alt+J"}"#).unwrap().compact_hotkey, "Ctrl+Alt+J");
    }

    #[test]
    fn quiet_mascot_defaults_off_and_survives_a_round_trip() {
        assert!(!parse("{}").unwrap().quiet_mascot);
        let cfg = parse(r#"{"api_key":"k","quiet_mascot":true}"#).unwrap();
        let back = parse(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert!(back.quiet_mascot);
        assert_eq!(back.api_key, "k");
    }

    #[test]
    fn changing_timer_lengths_keeps_the_rest_of_the_config() {
        let cfg = parse(r#"{"api_key":"k","base_url":"http://x","refresh_secs":60,"pomodoro":{"long_break_every":3,"auto_start":true}}"#).unwrap();
        let out = cfg.with_lengths(PomodoroLengths { focus_min: 50, short_break_min: 10, long_break_min: 300 });
        assert_eq!((out.api_key.as_str(), out.base_url.as_str(), out.refresh_secs), ("k", "http://x", 60));
        let p = &out.pomodoro;
        assert_eq!((p.focus_min, p.short_break_min, p.long_break_min, p.long_break_every, p.auto_start), (50, 10, 90, 3, true));
        // And it survives a save/load round trip.
        let back = parse(&serde_json::to_string_pretty(&out).unwrap()).unwrap();
        assert_eq!((back.api_key.as_str(), back.pomodoro.focus_min, back.pomodoro.long_break_min), ("k", 50, 90));
    }

    #[test]
    fn window_state_defaults_to_full_with_no_positions() {
        let cfg = parse("{}").unwrap();
        assert_eq!((cfg.ui_mode, cfg.full_pos, cfg.compact_pos), (Mode::Full, None, None));
        let view = ConfigView::from(&cfg);
        assert_eq!((view.ui_mode, view.compact_hotkey.as_str()), (Mode::Full, DEFAULT_HOTKEY));
    }

    #[test]
    fn window_state_survives_a_round_trip_and_keeps_the_rest() {
        let cfg = parse(r#"{"api_key":"k","quiet_mascot":true,"full_pos":{"x":5,"y":6}}"#).unwrap();
        let out = cfg.with_window(Some(Mode::Compact), &[(Mode::Compact, (-1200, 40)), (Mode::Full, (-1280, 40))]);
        let back = parse(&serde_json::to_string_pretty(&out).unwrap()).unwrap();
        assert_eq!(back.ui_mode, Mode::Compact);
        assert_eq!(back.pos_for(Mode::Compact), Some((-1200, 40)));
        assert_eq!(back.pos_for(Mode::Full), Some((-1280, 40)));
        assert_eq!((back.api_key.as_str(), back.quiet_mascot), ("k", true));
        // Only a position: the mode is kept.
        let moved = back.with_window(None, &[(Mode::Compact, (1, 2))]);
        assert_eq!((moved.ui_mode, moved.pos_for(Mode::Compact), moved.pos_for(Mode::Full)), (Mode::Compact, Some((1, 2)), Some((-1280, 40))));
    }

    #[test]
    fn a_broken_window_state_does_not_lose_the_key() {
        let cfg = parse(r#"{"api_key":"k","ui_mode":"tiny","full_pos":"left","compact_pos":{"x":1}}"#).unwrap();
        assert_eq!((cfg.api_key.as_str(), cfg.ui_mode, cfg.full_pos, cfg.compact_pos), ("k", Mode::Full, None, None));
        assert_eq!(parse(r#"{"ui_mode":"compact","compact_pos":null}"#).unwrap().ui_mode, Mode::Compact);
    }

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hackatime-widget-test-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    #[test]
    fn a_missing_file_starts_from_the_defaults() {
        let path = temp_file("missing");
        let saved = update_at(&path, |c| Ok(c.with_window(Some(Mode::Compact), &[]))).unwrap();
        assert_eq!((saved.ui_mode, saved.refresh_secs), (Mode::Compact, 300));
        assert_eq!(read_strict(&path).unwrap().unwrap().ui_mode, Mode::Compact);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_corrupt_file_is_left_untouched() {
        let path = temp_file("corrupt");
        let text = "{\"api_key\":\"11111111-2222-3333-4444-555555555555\", oops";
        std::fs::write(&path, text).unwrap();
        let err = update_at(&path, |c| Ok(c.with_window(Some(Mode::Compact), &[(Mode::Compact, (1, 2))]))).unwrap_err();
        assert_eq!(err.code(), "config");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        // A refused change is never applied, even partly.
        assert!(update_at(&path, |_| panic!("must not run on a corrupt file")).is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_failing_change_writes_nothing() {
        let path = temp_file("failing");
        std::fs::write(&path, r#"{"api_key":"k"}"#).unwrap();
        assert!(update_at(&path, |c| c.with_api_key("not a key")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"api_key":"k"}"#);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn concurrent_updates_do_not_lose_fields() {
        let path = temp_file("concurrent");
        std::fs::write(&path, r#"{"api_key":"k"}"#).unwrap();
        let writers: Vec<Box<dyn Fn(i32, Config) -> Config + Send + Sync>> = vec![
            Box::new(|i, c| c.with_window(None, &[(Mode::Compact, (i, i))])),
            Box::new(|i, c| c.with_window(Some(Mode::Compact), &[(Mode::Full, (-i, i))])),
            Box::new(|_, c| Config { quiet_mascot: true, ..c }),
            Box::new(|_, c| c.with_lengths(PomodoroLengths { focus_min: 50, short_break_min: 10, long_break_min: 30 })),
        ];
        std::thread::scope(|s| {
            for w in &writers {
                let path = &path;
                s.spawn(move || for i in 0..40 { update_at(path, |c| Ok(w(i, c))).unwrap(); });
            }
        });
        let cfg = read_strict(&path).unwrap().unwrap();
        assert_eq!(cfg.api_key, "k");
        assert_eq!((cfg.pos_for(Mode::Compact), cfg.pos_for(Mode::Full), cfg.ui_mode), (Some((39, 39)), Some((-39, 39)), Mode::Compact));
        assert!(cfg.quiet_mascot);
        assert_eq!(cfg.pomodoro.focus_min, 50);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse("not json").is_none());
    }

    const CFG: &str = "\u{feff}# comment\n[settings]\napi_url = https://hackatime.hackclub.com/api/hackatime/v1\napi_key=11111111-2222-3333-4444-555555555555\nheartbeat_rate_limit_seconds = 30\n\n[other]\napi_key = nope\n";

    #[test]
    fn reads_the_settings_section_of_a_wakatime_cfg() {
        let (key, url) = parse_wakatime_cfg(CFG);
        assert_eq!(key.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
        assert_eq!(url.as_deref(), Some("https://hackatime.hackclub.com/api/hackatime/v1"));
    }

    #[test]
    fn wakatime_cfg_edge_cases() {
        assert_eq!(parse_wakatime_cfg(""), (None, None));
        assert_eq!(parse_wakatime_cfg("[settings]\napi_key =\n"), (None, None));
        assert_eq!(parse_wakatime_cfg("api_key = outside-any-section\n"), (None, None));
        assert_eq!(parse_wakatime_cfg("[SETTINGS]\r\nAPI_KEY = \"quoted\"\r\n").0.as_deref(), Some("quoted"));
        assert_eq!(parse_wakatime_cfg("[settings]\n; api_key = commented\n").0, None);
    }

    #[test]
    fn detected_key_fills_an_empty_config() {
        let cfg = with_wakatime_cfg(Config::default(), parse_wakatime_cfg(CFG));
        assert_eq!(cfg.api_key, "11111111-2222-3333-4444-555555555555");
    }

    #[test]
    fn own_key_wins_over_wakatime_cfg() {
        let own = Config { api_key: "mine".into(), ..Default::default() };
        assert_eq!(with_wakatime_cfg(own, parse_wakatime_cfg(CFG)).api_key, "mine");
    }

    #[test]
    fn a_wakatime_com_key_is_not_a_hackatime_key() {
        let cfg = with_wakatime_cfg(Config::default(), (Some("k".into()), Some("https://api.wakatime.com/api/v1".into())));
        assert_eq!(cfg.api_key, "");
    }

    #[test]
    fn a_self_hosted_api_url_is_adopted() {
        let cfg = with_wakatime_cfg(Config::default(), (Some("k".into()), Some("https://time.example.org/api/hackatime/v1".into())));
        assert_eq!(cfg.base_url, "https://time.example.org/api/hackatime/v1");
        let custom = Config { base_url: "http://localhost:3000/api/hackatime/v1".into(), ..Default::default() };
        let cfg = with_wakatime_cfg(custom, (Some("k".into()), Some("https://elsewhere/api".into())));
        assert_eq!(cfg.base_url, "http://localhost:3000/api/hackatime/v1"); // an explicit URL is kept
    }

    #[test]
    fn config_dir_per_platform() {
        let env = |k: &str| match k {
            "APPDATA" => Some("C:/Users/x/AppData/Roaming".to_string()),
            "HOME" => Some("/home/x".to_string()),
            _ => None,
        };
        assert_eq!(os_config_dir("windows", &env), Some(PathBuf::from("C:/Users/x/AppData/Roaming")));
        assert_eq!(os_config_dir("macos", &env), Some(PathBuf::from("/home/x/Library/Application Support")));
        assert_eq!(os_config_dir("linux", &env), Some(PathBuf::from("/home/x/.config")));
        let xdg = |k: &str| (k == "XDG_CONFIG_HOME").then(|| "/cfg".to_string());
        assert_eq!(os_config_dir("linux", &xdg), Some(PathBuf::from("/cfg")));
        assert_eq!(os_config_dir("linux", &|_| None), None);
    }
}
