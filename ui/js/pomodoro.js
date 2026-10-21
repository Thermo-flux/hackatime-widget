import { $ } from "./lib/dom.js";
import { mmss } from "./lib/format.js";
import { invoke } from "./lib/tauri.js";
import { signal, cheer } from "./mascot.js";

const LABELS = { focus: "Focus", short_break: "Short break", long_break: "Long break" };
const CIRCUMFERENCE = 2 * Math.PI * 52;
const LENGTH_KEY = { focus: "focus_min", short_break: "short_break_min", long_break: "long_break_min" };

let state = null;
let poll = null;
let audio = null;

function chime() {
  try {
    audio ??= new AudioContext();
    const osc = audio.createOscillator();
    osc.frequency.value = 880;
    osc.connect(audio.destination);
    osc.start();
    osc.stop(audio.currentTime + 0.4);
  } catch {}
}

const announce = (msg) => { $("pomo-status").textContent = msg; };

/** idle: the phase hasn't started (or was reset); paused: started but stopped. */
const modeOf = (s) => (s.running ? "running" : s.remaining_secs >= s.total_secs ? "idle" : "paused");
/** The configured length differs from the phase in progress (lengths were changed after it started). */
const pendingLengths = (s) => s[LENGTH_KEY[s.phase]] * 60 !== s.total_secs;

const MAX_DOTS = 8;

/** One dot per focus session in the current long-break cycle, filled as they finish; many sessions collapse to a count. */
function renderDots(s) {
  const done = s.completed_focus;
  const box = $("pomo-dots");
  if (done > MAX_DOTS) {
    box.replaceChildren(`●×${done}`);
    return;
  }
  const cycle = Math.min(MAX_DOTS, s.long_break_every > 0 ? s.long_break_every : 4);
  // During the long break the cycle just completed stays full; after it, the row empties for the next one.
  const filled = done > 0 && done % cycle === 0 && s.phase === "long_break" ? cycle : done % cycle;
  box.replaceChildren(
    ...Array.from({ length: cycle }, (_, i) => {
      const dot = document.createElement("span");
      dot.className = i < filled ? "pdot pdot--on" : "pdot";
      return dot;
    }),
  );
}

function render(s) {
  const prev = state;
  state = s;
  const label = LABELS[s.phase];
  const time = mmss(s.remaining_secs);
  const mode = modeOf(s);
  const verb = { idle: "Start", running: "Pause", paused: "Resume" }[mode];

  const panel = $("panel-pomodoro");
  panel.dataset.phase = s.phase;
  panel.dataset.state = mode;
  $("pomo-phase").textContent = label;
  $("pomo-time").textContent = time;
  $("pomo-hint-text").textContent = verb;

  const ring = $("pomo-toggle"); // the ring itself is the start/pause button
  ring.dataset.state = mode;
  ring.setAttribute("aria-label", `${verb} ${label}, ${time} left`);

  // The ring sweeps smoothly second by second; a jump (new phase, reset, new length) snaps instead of rewinding.
  const bar = $("pomo-progress");
  const jump = !prev || prev.phase !== s.phase || prev.total_secs !== s.total_secs || s.remaining_secs > prev.remaining_secs;
  bar.classList.toggle("is-snap", jump);
  bar.style.strokeDasharray = String(CIRCUMFERENCE);
  bar.style.strokeDashoffset = String(CIRCUMFERENCE * (1 - s.remaining_secs / s.total_secs));

  // The lengths live on the Times button (tooltip, and read out by screen readers); the line under the ring is today's progress.
  const lengths = `Focus ${s.focus_min} min, rest ${s.short_break_min} min, long rest ${s.long_break_min} min`;
  const text = pendingLengths(s) ? `${lengths} (from the next phase)` : lengths;
  $("pomo-lengths").textContent = text;
  $("pomo-set").title = text;
  renderDots(s);
  $("pomo-count").textContent = s.completed_focus > MAX_DOTS ? "today" : `${s.completed_focus} today`; // the dots row already holds the number when it collapses
  document.title = s.running ? `${time} - ${label}` : "Hackatime";

  $("pomo-reset").disabled = mode === "idle" && !pendingLengths(s);

  signal("pomo", { running: s.running, phase: s.phase });
  if (s.finished) {
    chime();
    cheer();
    announce(`${LABELS[s.finished]} done. ${label} next, ${mmss(s.total_secs)}.`);
  } else if (prev && prev.phase !== s.phase) {
    announce(`${label}, ${time}.`);
  }
}

const call = (cmd, args) => invoke(cmd, args).then(render);

// ---- Focus / rest length settings ----
let draft = null;
const MAIN = ["pomo-phase", "pomo-toggle", "pomo-controls", "pomo-sessions"];
const ROWS = () => document.querySelectorAll("#pomo-settings .stepper");
const same = (a, b) => a.focus_min === b.focus_min && a.short_break_min === b.short_break_min && a.long_break_min === b.long_break_min;
const current = () => ({ focus_min: state.focus_min, short_break_min: state.short_break_min, long_break_min: state.long_break_min });

/** Fine steps below 10 minutes, steps of 5 above (going down from 10 steps by 1 again). */
const stepValue = (v, dir) => v + dir * ((dir > 0 ? v >= 10 : v > 10) ? 5 : 1);

function setDraft(key, value, row) {
  const min = Number(row.dataset.min);
  const max = Number(row.dataset.max);
  draft[key] = Math.min(max, Math.max(min, value));
  paintSettings();
}

function paintSettings() {
  for (const row of ROWS()) {
    const { key, min, max } = row.dataset;
    const v = draft[key];
    const val = row.querySelector('[role="spinbutton"]');
    val.textContent = `${v} min`;
    val.setAttribute("aria-valuenow", String(v));
    val.setAttribute("aria-valuetext", `${v} minute${v === 1 ? "" : "s"}`);
    const [less, more] = row.querySelectorAll(".stepper__btn");
    less.disabled = v <= Number(min);
    more.disabled = v >= Number(max);
  }
  for (const chip of document.querySelectorAll("#pomo-presets .chip")) {
    const [f, r] = chip.dataset.preset.split(",").map(Number);
    chip.setAttribute("aria-pressed", String(draft.focus_min === f && draft.short_break_min === r));
  }
  const unchanged = same(draft, current());
  $("pomo-save").disabled = unchanged;
  const note = $("pomo-settings-note");
  note.classList.remove("notice--error");
  note.textContent = modeOf(state) === "idle"
    ? "New times apply right away."
    : `Your ${LABELS[state.phase].toLowerCase()} keeps its time; new times start next phase or on Reset.`;
}

function showSettings(open) {
  $("pomo-settings").hidden = !open;
  for (const id of MAIN) $(id).hidden = open;
  $("pomo-set").setAttribute("aria-expanded", String(open));
  $("panel-pomodoro").classList.toggle("is-editing", open);
  if (open) document.querySelector('#pomo-settings [role="spinbutton"]').focus();
  else $("pomo-set").focus({ preventScroll: true });
}

function openSettings() {
  if (!state) return;
  draft = current();
  paintSettings();
  showSettings(true);
}

async function save() {
  if (same(draft, current())) return showSettings(false);
  const was = modeOf(state);
  let s;
  try {
    s = await invoke("set_pomodoro", { lengths: { ...draft } });
  } catch (e) {
    const note = $("pomo-settings-note");
    note.textContent = `Couldn't save: ${e?.message ?? e}`;
    note.classList.add("notice--error");
    return;
  }
  render(s);
  showSettings(false);
  const lens = `Focus ${s.focus_min}, rest ${s.short_break_min}, long rest ${s.long_break_min} minutes`;
  announce(was === "idle" ? `Saved. ${lens}.` : `Saved. ${lens}, from the next phase.`);
  const line = $("pomo-sessions");
  line.classList.remove("is-updated");
  void line.offsetWidth; // restart the highlight animation
  line.classList.add("is-updated");
}

// Press and hold a +/- button to keep stepping (faster after a moment).
let repeat = null;
let pointerStepped = false;
function stopRepeat() { clearTimeout(repeat); repeat = null; }
function stepRow(btn) {
  const row = btn.closest(".stepper");
  if (btn.disabled) return false;
  setDraft(row.dataset.key, stepValue(draft[row.dataset.key], Number(btn.dataset.dir)), row);
  return !btn.disabled;
}

function initSettings() {
  $("pomo-set").addEventListener("click", openSettings);
  const panel = $("pomo-settings");

  panel.addEventListener("pointerdown", (e) => {
    const btn = e.target.closest(".stepper__btn");
    if (!btn || e.button !== 0) return;
    e.preventDefault(); // keep focus on the value so arrow keys keep working
    btn.closest(".stepper").querySelector('[role="spinbutton"]').focus({ preventScroll: true });
    pointerStepped = true;
    stopRepeat();
    if (!stepRow(btn)) return;
    let delay = 420;
    const tick = () => { if (stepRow(btn)) { delay = Math.max(70, delay * 0.7); repeat = setTimeout(tick, delay); } };
    repeat = setTimeout(tick, delay);
  });
  for (const type of ["pointerup", "pointerleave", "pointercancel"]) panel.addEventListener(type, stopRepeat);
  window.addEventListener("blur", stopRepeat);

  panel.addEventListener("click", (e) => {
    const btn = e.target.closest(".stepper__btn");
    if (btn) {
      if (pointerStepped) pointerStepped = false; // already stepped on pointerdown
      else stepRow(btn); // keyboard or scripted click
      return;
    }
    const chip = e.target.closest(".chip");
    if (chip) {
      const [focus_min, short_break_min, long_break_min] = chip.dataset.preset.split(",").map(Number);
      draft = { focus_min, short_break_min, long_break_min };
      paintSettings();
    }
  });

  panel.addEventListener("keydown", (e) => {
    if (e.key === "Escape") { e.preventDefault(); showSettings(false); return; }
    const val = e.target.closest('[role="spinbutton"]');
    if (!val) return;
    if (e.key === "Enter") { e.preventDefault(); save(); return; }
    const row = val.closest(".stepper");
    const { key, min, max } = row.dataset;
    const v = draft[key];
    const next = {
      ArrowUp: stepValue(v, 1), ArrowRight: stepValue(v, 1),
      ArrowDown: stepValue(v, -1), ArrowLeft: stepValue(v, -1),
      PageUp: v + 10, PageDown: v - 10, Home: Number(min), End: Number(max),
    }[e.key];
    if (next === undefined) return;
    e.preventDefault();
    setDraft(key, next, row);
  });

  // Scroll over a value to change it.
  panel.addEventListener("wheel", (e) => {
    const row = e.target.closest(".stepper");
    if (!row || !e.deltaY) return;
    e.preventDefault();
    setDraft(row.dataset.key, stepValue(draft[row.dataset.key], e.deltaY < 0 ? 1 : -1), row);
  }, { passive: false });

  $("pomo-cancel").addEventListener("click", () => showSettings(false));
  $("pomo-save").addEventListener("click", save);
}

export const pause = () => { clearInterval(poll); poll = null; };
export function resume() {
  call("pomodoro_state");
  clearInterval(poll);
  poll = setInterval(() => call("pomodoro_state"), 1000);
}

export function init() {
  $("pomo-toggle").addEventListener("click", () => call(state?.running ? "pomodoro_pause" : "pomodoro_start"));
  initSettings();
  $("pomo-reset").addEventListener("click", () => call("pomodoro_reset"));
  $("pomo-skip").addEventListener("click", () => call("pomodoro_skip"));
  resume();
}
