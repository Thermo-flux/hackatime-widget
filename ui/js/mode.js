// Full widget <-> compact pill (Phase 7.3). Rust's `set_mode` does all the window work (size, position, clamping,
// persistence); this module animates around it. A native window can't be resized smoothly frame by frame, so the
// morph happens inside the transparent window while it is the larger of the two sizes:
//   collapse: animate card -> pill inside the full window, then shrink the window around the pill;
//   expand:   grow the window around the pill (the new area is transparent), then animate pill -> card.
// The pill is pinned to the window's top-right and the window resizes around its top-right corner, so the pill stays
// put on screen across the resize. While the window changes size the page hides itself (.is-snapping) for those few
// frames, so the in-between geometry (new size at the old position, or the reverse) is never seen.
import { $ } from "./lib/dom.js";
import { invoke } from "./lib/tauri.js";
import * as compact from "./compact.js";
import * as hint from "./hint.js";

export const MORPH_MS = 380;
export const FADE_MS = 200; // full content out during the first 200 ms, pill content in during the last 200 ms
const SURFACE_FADE_MS = 140; // the plain morph surface fades in under the fading silhouette (and out on expand)
export const EASE = "cubic-bezier(.32,.72,.24,1)";
// Must match compact.css (--pill-w / --pill-h / --pill-margin / --pill-radius) and modes.rs (COMPACT_SIZE).
const PILL = { w: 220, h: 40, margin: 10, radius: 20 };

const root = document.documentElement;
let mode = root.dataset.mode === "compact" ? "compact" : "full";
let busy = null; // the running switch, so re-entrant calls just wait for it
let anims = [];

export const getMode = () => mode;

const reducedMotion = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

/** One animation frame; a hidden window gets no frames, so a timer stands in. */
const frame = () => new Promise((resolve) => {
  let done = false;
  const go = () => { if (!done) { done = true; resolve(); } };
  requestAnimationFrame(go);
  setTimeout(go, 100);
});

/** Waits until the viewport has taken the new window size (a few frames at most), then two more frames. */
async function settle(width, height) {
  for (let i = 0; i < 10 && (Math.abs(innerWidth - width) > 1 || Math.abs(innerHeight - height) > 1); i++) await frame();
  await frame();
  await frame();
}

const px = (v) => `${v}px`;
const box = (r, radius) => ({ left: px(r.left), top: px(r.top), width: px(r.width), height: px(r.height), borderRadius: px(radius) });

/** The pill's box in the current viewport, moved by (dx, dy). */
const pillBox = (dx = 0, dy = 0) => ({ left: innerWidth - PILL.margin - PILL.w + dx, top: PILL.margin + dy, width: PILL.w, height: PILL.h });

function cardBox() {
  const r = document.querySelector(".card").getBoundingClientRect();
  return { left: r.left, top: r.top, width: r.width, height: r.height };
}

/** The card's top-right corner radius when it is a plain length (classic: 16px); 0 for shaped cards (elliptical or
 *  percentage radii, clip-path or mask silhouettes), whose shape lies inside their box anyway. */
function cardRadius() {
  const r = getComputedStyle(document.querySelector(".card")).borderTopRightRadius.trim();
  return /^[\d.]+px$/.test(r) ? parseFloat(r) : 0;
}

/** clip-path that shows `r` (viewport box) of the element with box `el`. */
function clipTo(el, r, radius) {
  const t = r.top - el.top, l = r.left - el.left;
  const b = el.top + el.height - (r.top + r.height), rt = el.left + el.width - (r.left + r.width);
  return `inset(${px(t)} ${px(rt)} ${px(b)} ${px(l)} round ${px(radius)})`;
}

function shellBox() {
  const r = $("shell").getBoundingClientRect();
  return { left: r.left, top: r.top, width: r.width, height: r.height };
}

/** The whole themed widget's box: the card plus any decoration hanging outside it (ears, stems, lid, tentacles).
 *  The collapse clip starts here, so nothing is cut on the first frame; for an undecorated theme this is exactly the
 *  card's box (and radius), which is where the morph surface starts too, so the two travel in step. */
function widgetBox() {
  const boxes = [document.querySelector(".card"), ...document.querySelectorAll(".shell > .deco")]
    .filter((e) => e.getClientRects().length)
    .map((e) => e.getBoundingClientRect());
  let l = Math.min(...boxes.map((b) => b.left)), t = Math.min(...boxes.map((b) => b.top));
  let r = Math.max(...boxes.map((b) => b.right)), b = Math.max(...boxes.map((x) => x.bottom));
  const shell = $("shell");
  // ::before / ::after decorations (boba lid, dog tag, penguin feet) can't be measured; they stay within 14px.
  const pseudo = ["::before", "::after"].some((p) => !["none", "normal"].includes(getComputedStyle(shell, p).content));
  if (pseudo) { l -= 14; t -= 14; r += 14; b += 14; }
  const card = cardBox();
  const plain = !pseudo && l === card.left && t === card.top && r === card.left + card.width && b === card.top + card.height;
  return { box: { left: l, top: t, width: r - l, height: b - t }, radius: plain ? cardRadius() : 0 };
}

/** The pill box `pillBox(dx, dy)`, kept inside the viewport (a collapse can only draw inside the full window). */
function reachablePill(dx, dy) {
  const p = pillBox(dx, dy);
  return { ...p, left: Math.min(Math.max(p.left, 0), innerWidth - p.width), top: Math.min(Math.max(p.top, 0), innerHeight - p.height) };
}

function play(el, keyframes, options) {
  const a = el.animate(keyframes, { fill: "both", ...options });
  anims.push(a);
  return a;
}

function clearAnims() {
  for (const a of anims) a.cancel();
  anims = [];
}

/** Collapse, inside the full window: the card's box shrinks to the pill's box, the themed widget (silhouette,
 *  decorations, content) fades out first, the pill's content fades in last. `dx/dy` moves the target when the pill
 *  will be clamped back on screen (set_mode preview), so it lands where the compact window will show it. */
function collapseMorph(dx, dy) {
  const from = cardBox(), to = reachablePill(dx, dy), widget = widgetBox(), radius = cardRadius();
  const morph = $("morph"), shell = $("shell"), pill = $("pill");
  const at = `translate(${px(to.left - pillBox().left)}, ${px(to.top - pillBox().top)})`;
  play(morph, [box(from, radius), box(to, PILL.radius)], { duration: MORPH_MS, easing: EASE });
  play(morph, [{ opacity: 0 }, { opacity: 1 }], { duration: SURFACE_FADE_MS, easing: "linear" });
  play(shell, [{ opacity: 1 }, { opacity: 0 }], { duration: FADE_MS, easing: "ease-out" });
  // While it fades, the themed widget is cut to the travelling box, so the card itself visibly shrinks into it.
  const sb = shellBox();
  play(shell, [{ clipPath: clipTo(sb, widget.box, widget.radius) }, { clipPath: clipTo(sb, to, PILL.radius) }], { duration: MORPH_MS, easing: EASE });
  play(pill, [{ opacity: 0, transform: at }, { opacity: 1, transform: at }], { duration: FADE_MS, delay: MORPH_MS - FADE_MS, easing: "ease-in" });
  return Promise.all(anims.map((a) => a.finished));
}

/** Expand, inside the already grown window: the reverse. `dx/dy` is where the pill really is relative to the
 *  window's top-right (non-zero when clamping moved the window to keep it on screen). */
function expandMorph(dx, dy) {
  const from = pillBox(dx, dy), to = cardBox(), widget = widgetBox(), radius = cardRadius();
  const morph = $("morph"), shell = $("shell"), pill = $("pill");
  const shift = `translate(${px(dx)}, ${px(dy)})`;
  play(morph, [box(from, PILL.radius), box(to, radius)], { duration: MORPH_MS, easing: EASE });
  play(morph, [{ opacity: 1 }, { opacity: 0 }], { duration: SURFACE_FADE_MS, delay: MORPH_MS - SURFACE_FADE_MS, easing: "linear" });
  play(pill, [{ opacity: 1, transform: shift }, { opacity: 0, transform: shift }], { duration: FADE_MS, easing: "ease-out" });
  play(shell, [{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, delay: MORPH_MS - FADE_MS, easing: "ease-in" });
  const sb = shellBox();
  play(shell, [{ clipPath: clipTo(sb, from, PILL.radius) }, { clipPath: clipTo(sb, widget.box, widget.radius) }], { duration: MORPH_MS, easing: EASE });
  return Promise.all(anims.map((a) => a.finished));
}

function setAttr(next) {
  mode = next;
  root.dataset.mode = next;
}

async function collapse() {
  const animate = !reducedMotion() && !document.hidden;
  if (!document.hidden) await hint.beforeCollapse(); // the one-time hotkey tip, said by the full widget's mascot
  compact.start(); // fill the pill before it fades in
  if (animate) {
    // Where the pill will land: normally exactly the full window's top-right, unless Rust must clamp it back on
    // screen (the widget was dragged far off). A failed preview just means no shift.
    const where = await invoke("set_mode", { mode: "compact", preview: true }).catch(() => null);
    root.classList.add("is-morphing");
    await collapseMorph(where?.shift_x ?? 0, where?.shift_y ?? 0);
  }
  // The pill now sits at its final box inside the full window. Become compact, hide, shrink the window around it.
  setAttr("compact");
  root.classList.add("is-snapping");
  try {
    await invoke("set_mode", { mode: "compact" });
  } catch (e) {
    console.error("set_mode compact failed", e);
    setAttr("full");
    compact.stop();
  }
  await settle(...(mode === "compact" ? [PILL.w + 2 * PILL.margin, PILL.h + 2 * PILL.margin] : [innerWidth, innerHeight]));
  clearAnims();
  root.classList.remove("is-morphing", "is-snapping");
  // Keyboard focus follows the widget: the pill now (Enter/Space expands it), the selected tab after expanding.
  // focusVisible: false keeps the ring off after a hotkey or mouse switch; keyboard users get it on their next key.
  if (mode === "compact") $("pill").focus({ preventScroll: true, focusVisible: false });
}

async function expand() {
  const animate = !reducedMotion() && !document.hidden;
  // Hide first: growing the window moves and resizes it in two steps.
  root.classList.add("is-snapping");
  if (animate) root.classList.add("is-morphing");
  let r;
  try {
    r = await invoke("set_mode", { mode: "full" });
  } catch (e) {
    console.error("set_mode full failed", e);
    root.classList.remove("is-morphing", "is-snapping");
    return;
  }
  await settle(r?.width ?? innerWidth, r?.height ?? innerHeight);
  setAttr("full");
  if (animate) {
    const done = expandMorph(-(r?.shift_x ?? 0), -(r?.shift_y ?? 0)); // first frame is painted with the hide lifted
    root.classList.remove("is-snapping");
    await done;
  }
  compact.stop();
  clearAnims();
  root.classList.remove("is-morphing", "is-snapping");
  document.querySelector('[role="tab"][aria-selected="true"]')?.focus({ preventScroll: true, focusVisible: false });
}

let ready = false; // set by init(): until the page knows the window's real mode, input is ignored

/** Switches to "full" or "compact", animated unless reduced motion is on (or the window is hidden). Resolves when
 *  everything has settled. A call while a switch is running, or before `init`, is ignored (it resolves with the
 *  running switch, or at once). */
export function setMode(next) {
  if (next !== "full" && next !== "compact") return Promise.reject(new Error(`unknown mode ${next}`));
  if (!ready) return Promise.resolve();
  if (busy) return busy;
  if (next === mode) return Promise.resolve();
  busy = (next === "compact" ? collapse() : expand()).finally(() => { busy = null; });
  return busy;
}

export const toggle = () => setMode(mode === "compact" ? "full" : "compact");

/** Starts in the window's actual mode (get_config's ui_mode comes from Rust's window state), without a morph: Rust
 *  already created the window at that mode's size. Input that arrived earlier was ignored. Safe to call once only. */
export function init(initial) {
  if (ready) return;
  setAttr(initial === "compact" ? "compact" : "full");
  if (mode === "compact") compact.start();
  ready = true;
}

// Listen from the moment the module loads, so a hotkey during start-up is never queued up unseen; toggle() ignores it
// until init() has run.
window.__TAURI__?.event?.listen?.("compact-hotkey", () => toggle());
