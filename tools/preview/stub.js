// Stands in for Tauri in a plain browser: window.__TAURI__.core.invoke returns canned data per command.
// The scenario comes from ?scenario=<name>. Shapes mirror the Rust structs (stats.rs, app.rs, pomodoro.rs, config.rs).
(() => {
  // compact_<name> shows the pill with the data of scenario <name> (shots.mjs switches the mode).
  const scenario = (new URLSearchParams(location.search).get("scenario") || "normal").replace(/^compact_/, "");
  const H = 3600;
  const reject = (code, message) => Promise.reject({ code, message });
  const slice = (name, seconds) => ({ name, seconds });

  const LANGS = [slice("Rust", 2.5 * H), slice("TypeScript", 1.8 * H), slice("CSS", 1.2 * H), slice("Markdown", 0.5 * H)];
  const PROJECTS = [slice("hackatime-widget", 3.4 * H), slice("portfolio-site", 1.9 * H), slice("dotfiles", 0.7 * H)];

  function stats(total, ai, { languages = LANGS, projects = PROJECTS, aiAvailable = true, mock = false, prev = null } = {}) {
    const aiSecs = aiAvailable ? ai : 0;
    return {
      total_seconds: total, ai_seconds: aiSecs, human_seconds: total - aiSecs,
      ai_percent: total > 0 ? (aiSecs / total) * 100 : 0,
      categories: [], languages, projects, mock, ai_available: aiAvailable, previous_total_seconds: prev,
    };
  }

  const RANGE_SCALE = { today: 1, "7d": 4.2, "30d": 16, all: 60 };
  const statsFor = (range) => {
    const k = RANGE_SCALE[range] ?? 1;
    switch (scenario) {
      case "zero": return stats(0, 0, { languages: [], projects: [], prev: 4.2 * H * k });
      case "down": return stats(6.25 * H * k, 2.25 * H * k, { prev: 9.5 * H * k });
      case "ai_unavailable": return stats(6.25 * H * k, 0, { aiAvailable: false });
      case "huge": return stats((999 * H + 59 * 60 + 412 * H), 412 * H, {
        languages: [slice("TypeScript React (TSX)", 300 * H), slice("Jupyter Notebook (Python)", 200 * H), slice("C++ (Objective)", 90 * H), slice("Markdown", 30 * H)],
        projects: [slice("a-very-long-project-name-for-testing", 500 * H), slice("monorepo/apps/web-dashboard", 300 * H), slice("dotfiles", 100 * H)],
      });
      case "mock": return stats(6.25 * H * k, 2.25 * H * k, { mock: true });
      default: return stats(6.25 * H * k, 2.25 * H * k, { prev: range === "all" ? null : 5.1 * H * k });
    }
  };

  const today = new Date();
  const daily = Array.from({ length: 14 }, (_, i) => {
    const d = new Date(today); d.setDate(d.getDate() - (13 - i));
    return { date: `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`, secs: [0, 5400, 3600, 0, 7200, 1800, 0, 4500, 5400, 0, 3600, 2700, 6300, 5400][i] };
  });
  const emptyDaily = daily.map((d) => ({ ...d, secs: 0 }));
  const summary = (empty) => empty
    ? { total_sessions: 0, total_secs: 0, avg_secs: 0, longest_secs: 0, sessions_today: 0, today_secs: 0, week_secs: 0, current_streak: 0, longest_streak: 0, active_days: 0, chart_days: 14, daily: emptyDaily }
    : { total_sessions: 23, total_secs: 23 * 3000, avg_secs: 3000, longest_secs: 7200, sessions_today: 1, today_secs: 5400, week_secs: 27900, current_streak: 4, longest_streak: 6, active_days: 10, chart_days: 14, daily };

  const live = {
    start: Math.floor(Date.now() / 1000) - 3725, elapsed_secs: 3725,
    activity: { active_secs: 2880, ai_secs: 900, human_secs: 1980, ai_percent: 31.25, focus_percent: 77.3, top_project: "hackatime-widget", top_language: "Rust" },
    activity_error: null,
  };
  const sessionView = () => {
    if (scenario === "session_live") return { active: live, summary: summary(false) };
    if (scenario === "session_live_noactivity") return { active: { ...live, activity: null, activity_error: "offline" }, summary: summary(false) };
    return { active: null, summary: summary(scenario === "sessions_empty") };
  };

  const POMO = { focus_min: 25, short_break_min: 5, long_break_min: 15 };
  let pomoCalls = 0;
  const pomo = () => {
    pomoCalls += 1;
    const base = { running: false, focus_started_at: null, completed_focus: scenario === "pomo_many" ? 12 : 2, long_break_every: 4, finished: null, ...POMO };
    switch (scenario) {
      case "pomo_running": return { ...base, focus_started_at: Math.floor(Date.now() / 1000) - 17 * 60, phase: "focus", running: true, remaining_secs: 17 * 60 + 23, total_secs: 1500 };
      // A focus round that ends on the third poll: the phase-end line depends on how much of it was coded.
      case "pomo_finishing": case "pomo_finishing_low": {
        if (pomoCalls < 3) return { ...base, focus_started_at: Math.floor(Date.now() / 1000) - 17 * 60, phase: "focus", running: true, remaining_secs: 5, total_secs: 1500 };
        return { ...base, phase: "short_break", remaining_secs: 300, total_secs: 300, completed_focus: 3, finished: pomoCalls === 3 ? "focus" : null };
      }
      case "pomo_nolong": return { ...base, long_break_every: 0, completed_focus: 3, phase: "focus", remaining_secs: 1500, total_secs: 1500 };
      case "pomo_many": return { ...base, phase: "focus", remaining_secs: 1500, total_secs: 1500 };
      case "pomo_paused": return { ...base, phase: "focus", remaining_secs: 9 * 60 + 40, total_secs: 1500 };
      case "pomo_break": return { ...base, phase: "short_break", running: true, remaining_secs: 3 * 60 + 12, total_secs: 300 };
      default: return { ...base, phase: "focus", remaining_secs: 1500, total_secs: 1500 };
    }
  };

  const config = () => ({
    has_key: scenario !== "no_key", refresh_secs: 300, mock: scenario === "mock",
    pomodoro: { ...POMO, long_break_every: 4, auto_start: false },
  });

  // "offline" has no saved copy; "offline_cached" has one from 14:02 and then fails like "offline".
  const cachedAt = () => { const d = new Date(); d.setHours(14, 2, 0, 0); return Math.floor(d.getTime() / 1000); };
  const handlers = {
    get_cached_stats: (a) => scenario === "offline_cached"
      ? { fetched_at: cachedAt(), stale: true, stats: statsFor(a?.range) }
      : null,
    get_config: config,
    set_quiet_mascot: () => null,
    hide_window: () => null,
    set_api_key: () => null,
    "plugin:opener|open_url": () => null,
    set_pomodoro: (a) => { const p = { ...pomo(), ...a.lengths }; const idle = p.remaining_secs >= p.total_secs; const t = (p.phase === "focus" ? p.focus_min : p.phase === "short_break" ? p.short_break_min : p.long_break_min) * 60; return idle ? { ...p, total_secs: t, remaining_secs: t } : p; },
    quit: () => null,
    recently_active: () => scenario !== "zero" && scenario !== "pomo_paused",
    get_stats: (a) => {
      if (scenario === "no_key") return reject("no_key", "no API key configured");
      if (scenario === "unauthorized") return reject("unauthorized", "server returned 401");
      if (scenario === "offline" || scenario === "offline_cached") return reject("network", "could not reach Hackatime: error sending request: connection refused");
      return statsFor(a?.range);
    },
    session_state: sessionView,
    session_start: () => null,
    session_stop: () => true,
    focus_activity: () => {
      const coded = { pomo_running: 14, pomo_finishing: 15, pomo_finishing_low: 3 }[scenario];
      return coded === undefined ? null : { elapsed_secs: 17 * 60, active_secs: coded * 60 };
    },
    pomodoro_state: pomo, pomodoro_start: pomo, pomodoro_pause: pomo, pomodoro_reset: pomo, pomodoro_skip: pomo,
  };

  window.__TAURI_STUB__ = { scenario, handlers };
  window.__TAURI__ = {
    core: {
      invoke(cmd, args) {
        const h = handlers[cmd];
        return h ? Promise.resolve().then(() => h(args)) : reject("unknown_command", `stub has no handler for ${cmd}`);
      },
    },
    window: { getCurrentWindow: () => ({ startDragging: () => Promise.resolve() }) },
  };
})();
