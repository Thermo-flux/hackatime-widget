import { $, el } from "./lib/dom.js";
import { hms, short } from "./lib/format.js";
import { invoke } from "./lib/tauri.js";
import { signal } from "./mascot.js";
import { fitTiles } from "./fit.js";

const POLL_MS = 30_000;
let poll = null;
let tick = null;
let live = null; // { elapsed, at } so the clock can run locally between polls

const pct = (n) => `${Math.round(n)}%`;

function renderChart(daily) {
  const max = Math.max(1, ...daily.map((d) => d.secs));
  $("sum-chart").replaceChildren(
    ...daily.map((d, i) => {
      const bar = el("span", { class: "chart__bar" });
      bar.classList.toggle("chart__bar--empty", d.secs === 0);
      bar.classList.toggle("chart__bar--today", i === daily.length - 1);
      bar.style.height = d.secs === 0 ? "" : `${Math.max(6, (d.secs / max) * 100)}%`;
      bar.title = `${d.date}: ${d.secs ? short(d.secs) : "no session"}`;
      return bar;
    }),
  );
}

function renderSummary(s) {
  $("sum-count").textContent = String(s.total_sessions);
  $("sum-total").textContent = short(s.total_secs);
  $("sum-avg").textContent = short(s.avg_secs);
  $("sum-streak").textContent = String(s.current_streak);
  $("sum-best").textContent = String(s.longest_streak);
  $("sum-active").textContent = String(s.active_days);
  $("sum-window").textContent = String(s.chart_days);
  $("sum-today").textContent = short(s.today_secs);
  $("sum-week").textContent = short(s.week_secs);
  renderChart(s.daily);
}

function renderLive(a, summary) {
  live = { elapsed: a.elapsed_secs, at: performance.now() };
  paintClock();
  $("live-today").textContent = short(summary.today_secs);

  const act = a.activity;
  if (!act) {
    for (const id of ["live-human", "live-ai", "live-coding", "live-focus"]) $(id).textContent = "–";
    $("live-bar-human").style.width = "0%";
    $("live-bar-ai").style.width = "0%";
    $("live-bar").setAttribute("aria-label", "No coding activity available");
    $("live-note").textContent = a.activity_error ? `Coding activity unavailable: ${a.activity_error}` : "";
    return;
  }
  const ai = act.ai_percent;
  const human = act.active_secs > 0 ? 100 - ai : 0;
  $("live-human").textContent = short(act.human_secs);
  $("live-ai").textContent = short(act.ai_secs);
  $("live-coding").textContent = short(act.active_secs);
  $("live-focus").textContent = pct(act.focus_percent);
  $("live-bar-human").style.width = `${human}%`;
  $("live-bar-ai").style.width = `${ai}%`;
  $("live-bar").setAttribute(
    "aria-label",
    act.active_secs > 0 ? `${pct(human)} human, ${pct(ai)} AI` : "No coding activity yet",
  );
  $("live-note").textContent = [act.top_project, act.top_language].filter(Boolean).join(" · ");
}

function paintClock() {
  if (!live) return;
  $("live-elapsed").textContent = hms(live.elapsed + (performance.now() - live.at) / 1000);
}

function showError(message) {
  $("sess-error").hidden = !message;
  $("sess-error").textContent = message ?? "";
}

export async function refresh() {
  try {
    const view = await invoke("session_state");
    showError(null);
    renderSummary(view.summary);
    const running = view.active !== null;
    $("sess-idle").hidden = running;
    $("sess-live").hidden = !running;
    $("sess-start").disabled = false;
    if (running) renderLive(view.active, view.summary);
    else live = null;
    signal("session", running);
    signal("streak", view.summary.current_streak);
    fitTiles();
  } catch (e) {
    showError(e.message ?? String(e));
  }
}

async function act(cmd, button) {
  button.disabled = true;
  try {
    await invoke(cmd);
  } catch (e) {
    showError(e.message ?? String(e));
  }
  await refresh();
  button.disabled = false;
}

export function pause() {
  clearInterval(poll);
  clearInterval(tick);
  poll = tick = null;
}

export function resume() {
  refresh();
  clearInterval(poll);
  clearInterval(tick);
  poll = setInterval(refresh, POLL_MS);
  tick = setInterval(paintClock, 1000);
}

export function init() {
  $("sess-start").addEventListener("click", () => act("session_start", $("sess-start")));
  $("sess-stop").addEventListener("click", () => act("session_stop", $("sess-stop")));
  resume();
}
