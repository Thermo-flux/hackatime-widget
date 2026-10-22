import { $ } from "./lib/dom.js";
import { invoke } from "./lib/tauri.js";
import * as stats from "./stats.js";
import * as pomodoro from "./pomodoro.js";
import * as sessions from "./sessions.js";
import * as mascot from "./mascot.js";
import * as themes from "./themes.js";
import { fitTiles } from "./fit.js";
import * as compact from "./compact.js";
import * as mode from "./mode.js";
import * as pill from "./pill.js";

const TABS = ["stats", "sessions", "pomodoro"];

function selectTab(name, { focus = false } = {}) {
  for (const t of TABS) {
    const tab = $(`tab-${t}`);
    const on = t === name;
    tab.setAttribute("aria-selected", String(on));
    tab.tabIndex = on ? 0 : -1;
    $(`panel-${t}`).hidden = !on;
    if (on && focus) tab.focus();
  }
  $("mock-badge").classList.toggle("is-off", name === "pomodoro"); // sample data only affects stats and sessions
  fitTiles();
  if (name === "sessions") sessions.refresh();
}

function initTabs() {
  const tablist = $("tabs");
  tablist.addEventListener("click", (e) => {
    const tab = e.target.closest('[role="tab"]');
    if (tab) selectTab(tab.id.replace("tab-", ""));
  });
  tablist.addEventListener("keydown", (e) => {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
    if (!step) return;
    const current = TABS.findIndex((t) => $(`tab-${t}`).getAttribute("aria-selected") === "true");
    selectTab(TABS[(current + step + TABS.length) % TABS.length], { focus: true });
  });
}

// The x hides the widget; the tray icon brings it back and has Quit.
$("close").addEventListener("click", () => invoke("hide_window"));
// Events from the tray menu (and, with Phase 7, the global hotkey).
const listen = window.__TAURI__?.event?.listen;
listen?.("set-theme", (e) => themes.apply(e.payload));
// No browser context menu (Reload / Inspect) on a desktop widget.
document.addEventListener("contextmenu", (e) => e.preventDefault());

initTabs();
mascot.init();
themes.init();
stats.init();
sessions.init();
pomodoro.init();

// Start in the mode the widget was left in (Rust already sized the window for it). An init script set
// data-mode="compact" before the first paint when starting compact; this confirms it and starts the pill.
pill.bind(() => mode.setMode("full"));
invoke("get_config").then((cfg) => mode.init(cfg.ui_mode), () => mode.init(mode.getMode()));

// Don't poll the backend while the window is hidden.
document.addEventListener("visibilitychange", () => {
  const hidden = document.hidden;
  hidden ? stats.pause() : stats.resume();
  hidden ? sessions.pause() : sessions.resume();
  hidden ? pomodoro.pause() : pomodoro.resume();
  if (hidden) compact.stop();
  else if (mode.getMode() === "compact") compact.start(); // the pill's own timers
});
