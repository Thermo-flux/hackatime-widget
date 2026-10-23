// Click vs drag on the pill. It carries no data-tauri-drag-region (that would swallow the click), so the pointer
// is watched here: a press that moves more than DRAG_PX hands over to a native window drag, a press that doesn't
// is a click and expands the pill. Keyboard (Enter/Space) reaches the same `expand` through the button's own click.
import { $ } from "./lib/dom.js";

const DRAG_PX = 4;

export function bind(expand) {
  const pill = $("pill");
  let press = null; // { x, y, dragged } while the primary button is down

  pill.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    press = { x: e.clientX, y: e.clientY, dragged: false };
  });

  pill.addEventListener("pointermove", (e) => {
    if (!press || press.dragged) return;
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) <= DRAG_PX) return;
    press.dragged = true;
    // Once the OS takes over the drag no pointerup arrives; the flag keeps the next press clean either way.
    Promise.resolve(window.__TAURI__?.window?.getCurrentWindow?.().startDragging?.()).catch(() => {});
  });

  pill.addEventListener("pointerup", () => {
    const clicked = press && !press.dragged;
    press = null;
    if (clicked) expand();
  });
  pill.addEventListener("pointercancel", () => { press = null; });

  // detail === 0 is a keyboard activation; a mouse click was already handled on pointerup.
  pill.addEventListener("click", (e) => { if (e.detail === 0) expand(); });
}
