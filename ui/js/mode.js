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

export const MORPH_MS = 220;
export const FADE_MS = 120; // full content out during the first 120 ms, pill content in during the last 120 ms
const SURFACE_FADE_MS = 90; // the plain morph surface fades in under the fading silhouette (and out on expand)
export const EASE = "cubic-bezier(.2,.8,.2,1)";
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

const cardRadius = () => parseFloat(getComputedStyle(root).getPropertyValue("--radius-lg")) || 16;

/** clip-path that shows `r` (viewport box) of the element with box `el`. */
function clipTo(el, r, radius) {
  const t = r.top - el.top, l = r.left - el.left;
  const b = el.top + el.height - (r.top + r.height), rt = el.left + el.width - (r.left + r.width);
  return `inset(${px(t)} ${px(rt)} ${px(b)} ${px(l)} round ${px(radius)})`;
}
// Clips nothing: reaches past the decorations that hang outside the shell (ears, stems, tentacles).
const NO_CLIP = "inset(-80px round 0px)";

function shellBox() {
  const r = $("shell").getBoundingClientRect();
  return { left: r.left, top: r.top, width: r.width, height: r.height };
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

/** Collapse, inside the full window: the card's box shrinks to the pill's box (top-right), the themed widget
 *  (silhouette, decorations, content) fades out first, the pill's content fades in last. */
function collapseMorph() {
  const from = cardBox(), to = pillBox();
  const morph = $("morph"), shell = $("shell"), pill = $("pill");
  play(morph, [box(from, cardRadius()), box(to, PILL.radius)], { duration: MORPH_MS, easing: EASE });
  play(morph, [{ opacity: 0 }, { opacity: 1 }], { duration: SURFACE_FADE_MS, easing: "linear" });
  play(shell, [{ opacity: 1 }, { opacity: 0 }], { duration: FADE_MS, easing: "ease-out" });
  // While it fades, the themed widget is cut to the travelling box, so the card itself visibly shrinks into it.
  play(shell, [{ clipPath: NO_CLIP }, { clipPath: clipTo(shellBox(), to, PILL.radius) }], { duration: MORPH_MS, easing: EASE });
  play(pill, [{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, delay: MORPH_MS - FADE_MS, easing: "ease-in" });
  return Promise.all(anims.map((a) => a.finished));
}

/** Expand, inside the already grown window: the reverse. `dx/dy` is where the pill really is relative to the
 *  window's top-right (non-zero when clamping moved the window to keep it on screen). */
function expandMorph(dx, dy) {
  const from = pillBox(dx, dy), to = cardBox();
  const morph = $("morph"), shell = $("shell"), pill = $("pill");
  const shift = `translate(${px(dx)}, ${px(dy)})`;
  play(morph, [box(from, PILL.radius), box(to, cardRadius())], { duration: MORPH_MS, easing: EASE });
  play(morph, [{ opacity: 1 }, { opacity: 0 }], { duration: SURFACE_FADE_MS, delay: MORPH_MS - SURFACE_FADE_MS, easing: "linear" });
  play(pill, [{ opacity: 1, transform: shift }, { opacity: 0, transform: shift }], { duration: FADE_MS, easing: "ease-out" });
  play(shell, [{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, delay: MORPH_MS - FADE_MS, easing: "ease-in" });
  play(shell, [{ clipPath: clipTo(shellBox(), from, PILL.radius) }, { clipPath: NO_CLIP }], { duration: MORPH_MS, easing: EASE });
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
    root.classList.add("is-morphing");
    await collapseMorph();
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
}

/** Switches to "full" or "compact", animated unless reduced motion is on (or the window is hidden). Resolves when
 *  everything has settled. A call while a switch is running is ignored (it resolves with that switch). */
export function setMode(next) {
  if (next !== "full" && next !== "compact") return Promise.reject(new Error(`unknown mode ${next}`));
  if (busy) return busy;
  if (next === mode) return Promise.resolve();
  busy = (next === "compact" ? collapse() : expand()).finally(() => { busy = null; });
  return busy;
}

export const toggle = () => setMode(mode === "compact" ? "full" : "compact");

/** Starts in the persisted mode, without a morph (Rust already created the window at that mode's size). */
export function init(initial) {
  setAttr(initial === "compact" ? "compact" : "full");
  if (mode === "compact") compact.start();
  window.__TAURI__?.event?.listen?.("compact-hotkey", () => toggle());
}
