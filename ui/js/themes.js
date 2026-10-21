import { $, el } from "./lib/dom.js";
import { THEMES } from "./themes/data.js";
import * as mascot from "./mascot.js";
import { fitTiles } from "./fit.js";

const STORAGE_KEY = "hackatime-widget.theme";
const TOKEN_NAMES = [...new Set(THEMES.flatMap((t) => Object.keys(t.tokens ?? {})))];

let current = THEMES[0];

function stored() {
  try { return localStorage.getItem(STORAGE_KEY); } catch { return null; }
}
function store(id) {
  try { localStorage.setItem(STORAGE_KEY, id); } catch { /* private mode etc.: just don't remember */ }
}

export function apply(id, { remember = true } = {}) {
  current = THEMES.find((t) => t.id === id) ?? THEMES[0];
  const root = document.documentElement;
  for (const name of TOKEN_NAMES) root.style.removeProperty(name);
  for (const [name, value] of Object.entries(current.tokens ?? {})) root.style.setProperty(name, value);
  root.style.colorScheme = current.mode ?? "light dark";
  root.dataset.skin = current.id;
  mascot.setTheme(current);
  requestAnimationFrame(fitTiles); // the new font changes text widths
  if (remember) store(current.id);
  for (const b of document.querySelectorAll(".picker__item")) b.setAttribute("aria-pressed", String(b.dataset.id === current.id));
}

export const list = () => THEMES;
export const currentId = () => current.id;

function renderPicker() {
  $("picker-grid").replaceChildren(
    ...THEMES.map((t) => {
      const preview = el("span", { class: "picker__preview" });
      preview.style.background = t.tokens?.["--bg"] ?? "";
      if (t.sprite) {
        const c = el("canvas");
        c.width = c.height = 16;
        mascot.draw(c, t);
        preview.append(c);
      }
      const b = el("button", { class: "picker__item" }, preview, el("span", { class: "picker__name", text: t.label }));
      b.type = "button";
      b.dataset.id = t.id;
      b.title = t.name;
      b.setAttribute("aria-label", t.name);
      b.setAttribute("aria-pressed", String(t.id === current.id));
      return b;
    }),
  );
}

function setPicker(open) {
  $("theme-picker").hidden = !open;
  $("theme-btn").setAttribute("aria-expanded", String(open));
  if (open) $("picker-grid").querySelector('[aria-pressed="true"]')?.focus();
  else $("theme-btn").focus();
}

export function init() {
  renderPicker();
  apply(stored() ?? THEMES[0].id, { remember: false });

  $("theme-btn").addEventListener("click", () => setPicker($("theme-picker").hidden));
  $("picker-close").addEventListener("click", () => setPicker(false));
  $("picker-grid").addEventListener("click", (e) => {
    const b = e.target.closest(".picker__item");
    if (b) apply(b.dataset.id);
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("theme-picker").hidden) setPicker(false);
  });
}
