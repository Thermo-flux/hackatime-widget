// The compact pill: today's total with its human/AI bar, or the running Pomodoro / session clock instead.
// It reads what the other views already hold (Pomodoro snapshot, session clock) and fetches the Day total itself,
// so the pill is right whichever range the Stats tab is on. Everything here runs only while the pill is on screen.
import { $ } from "./lib/dom.js";
import { invoke } from "./lib/tauri.js";
import { hms, mmss, short } from "./lib/format.js";
import { draw, currentExpression } from "./mascot.js";
import * as themes from "./themes.js";
import * as pomodoro from "./pomodoro.js";
import * as sessions from "./sessions.js";

const ACTIVE_POLL_MS = 60_000;
const PHASE_NAME = { focus: "Focus", short_break: "Break", long_break: "Long break" };

let day = null; // the last good Day stats, kept across a failed refresh
let active = false; // heartbeats in the last ~2 minutes
let timers = [];
let running = false;

/** What the pill shows right now: { text, dot, label }. Pure, so the previews and tests can call it. */
export function content(stats, snap, sessionSecs) {
  const total = stats ? short(stats.total_seconds) : "–";
  if (snap?.running) {
    const onBreak = snap.phase !== "focus";
    return { text: mmss(snap.remaining_secs), dot: onBreak ? "break" : "focus", label: `${PHASE_NAME[snap.phase]} ${mmss(snap.remaining_secs)} left` };
  }
  if (sessionSecs !== null) return { text: hms(sessionSecs), dot: "session", label: `Session ${hms(sessionSecs)}` };
  return { text: total, dot: null, label: stats ? `${total} today` : "Today's time unavailable" };
}

/** "6h 15m", "4h 00m", "42m": the minutes keep two digits once there are hours, as in the pill's tooltip. */
function spoken(secs) {
  const m = Math.floor(Math.max(0, secs) / 60);
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

/** Tooltip and screen-reader text: the day's total with its split, or the live clock first when one is shown. */
export function describe(stats, c) {
  const known = Boolean(stats?.ai_available) && stats.total_seconds > 0;
  const today = stats ? `${spoken(stats.total_seconds)} today` : "Today's time unavailable";
  const split = known ? ` · Human ${spoken(stats.human_seconds)} · AI ${spoken(stats.ai_seconds)}` : "";
  return `${c.dot ? `${c.label} · ` : ""}${today}${split}. Click to expand.`;
}

export function render() {
  paintMascot(); // cheap (16x16), and follows a theme or mood change while the pill is up
  const c = content(day, pomodoro.snapshot(), sessions.liveElapsed());
  $("pill-time").textContent = c.text;
  const dot = $("pill-dot");
  dot.hidden = !c.dot;
  if (c.dot) dot.dataset.kind = c.dot;

  const known = Boolean(day?.ai_available) && day.total_seconds > 0;
  const bar = $("pill-bar");
  if (known) bar.removeAttribute("data-unknown");
  else bar.setAttribute("data-unknown", "");
  const ai = known ? Math.min(100, Math.max(0, (day.ai_seconds / day.total_seconds) * 100)) : 0;
  $("pill-ai").style.width = `${ai}%`;
  $("pill-human").style.width = `${known ? 100 - ai : 0}%`;
  bar.classList.toggle("is-live", active);

  const full = describe(day, c);
  $("pill").setAttribute("aria-label", full);
  $("pill").title = full;
}

function paintMascot() {
  const theme = themes.list().find((t) => t.id === themes.currentId());
  if (theme) draw($("pill-mascot"), theme, currentExpression());
}

async function refreshDay() {
  try { day = await invoke("get_stats", { range: "today" }); } catch { /* keep the last value */ }
  render();
}

async function refreshActive() {
  try { active = Boolean(await invoke("recently_active")); } catch { active = false; }
  render();
}

/** Start refreshing; call when the pill appears. The Day total follows the same interval as the Stats tab. */
export async function start() {
  if (running) return;
  running = true;
  paintMascot();
  render();
  refreshDay();
  refreshActive();
  let dayMs = 300_000;
  try { dayMs = Math.max(30, (await invoke("get_config")).refresh_secs) * 1000; } catch { /* the default will do */ }
  if (!running || timers.length) return; // stopped (or restarted) while the config loaded
  timers = [setInterval(refreshDay, dayMs), setInterval(refreshActive, ACTIVE_POLL_MS), setInterval(render, 1000)];
}

/** Stop everything; call when the pill goes away or the window is hidden. */
export function stop() {
  running = false;
  for (const t of timers) clearInterval(t);
  timers = [];
}

export const isRunning = () => running;
