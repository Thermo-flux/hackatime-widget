// Validates ui/js/themes/data.js: structure, sprite grids, legends, patches and colour contrast.
// Usage: node check-themes.mjs      (exit code 1 if any error; warnings don't fail)
import { THEMES } from "../ui/js/themes/data.js";
import { EXPRESSIONS, LINE_KINDS, DEFAULT_LINES } from "../ui/js/mascot.js";

const REQUIRED = ["--bg", "--surface-2", "--fg", "--muted", "--border", "--accent", "--on-accent", "--human", "--ai", "--success", "--violet", "--warn", "--warn-bg", "--danger", "--focus", "--radius-sm", "--radius-md", "--radius-lg", "--shadow", "--font"];
const MOODS = LINE_KINDS; // every speech-line kind the engine uses
const PATCHES = ["blink", ...EXPRESSIONS.filter((e) => e !== "neutral")]; // every expression the engine can show
const MAX_LINE = 28; // the speech bubble is ~130px wide and every line must fit on one line
const errors = [], warnings = [];
const err = (t, m) => errors.push(`${t}: ${m}`);
const warn = (t, m) => warnings.push(`${t}: ${m}`);

const hex = (c) => /^#[0-9a-f]{6}$/i.test(c);
const lin = (v) => (v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
const lum = (c) => { const n = parseInt(c.slice(1), 16); return 0.2126 * lin(n >> 16) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255); };
const contrast = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
/** Rough deuteranopia simulation (Machado 2009, severity 1.0) to compare the human and AI colours. */
const deut = (c) => { const n = parseInt(c.slice(1), 16), [r, g, b] = [n >> 16, (n >> 8) & 255, n & 255]; const f = (v) => Math.max(0, Math.min(255, Math.round(v))); return "#" + [f(0.367322 * r + 0.860646 * g - 0.227968 * b), f(0.280085 * r + 0.672501 * g + 0.047413 * b), f(-0.01182 * r + 0.04294 * g + 0.968881 * b)].map((v) => v.toString(16).padStart(2, "0")).join(""); };

function checkLines(id, lines) {
  for (const [kind, list] of Object.entries(lines)) {
    if (!Array.isArray(list) || !list.length) { err(id, `lines.${kind} is empty`); continue; }
    if (list.length < 3) warn(id, `lines.${kind} has only ${list.length} line(s)`);
    const dup = list.filter((l, i) => list.indexOf(l) !== i);
    if (dup.length) err(id, `lines.${kind} repeats "${dup[0]}"`);
    for (const l of list) {
      if (typeof l !== "string" || !l.trim()) err(id, `lines.${kind} has an empty line`);
      else if (l.length > MAX_LINE) err(id, `lines.${kind} "${l}" is ${l.length} chars (max ${MAX_LINE})`);
    }
  }
}

// The fallback lines must cover every kind the engine says.
for (const k of LINE_KINDS) if (!DEFAULT_LINES[k]?.length) err("DEFAULT_LINES", `missing ${k}`);
checkLines("DEFAULT_LINES", DEFAULT_LINES);

const ids = new Set();
for (const t of THEMES) {
  const id = t.id;
  if (!/^[a-z][a-z0-9-]*$/.test(id)) err(id, "id must be kebab-case");
  if (ids.has(id)) err(id, "duplicate id");
  ids.add(id);
  if (!t.name) err(id, "missing name");
  if (![null, "light", "dark"].includes(t.mode)) err(id, `bad mode ${t.mode}`);

  if (t.tokens) {
    for (const k of REQUIRED) if (!(k in t.tokens)) err(id, `missing token ${k}`);
    for (const k of Object.keys(t.tokens)) if (!REQUIRED.includes(k)) err(id, `unknown token ${k}`);
    const T = t.tokens;
    for (const k of ["--bg", "--surface-2", "--fg", "--muted", "--border", "--accent", "--on-accent", "--human", "--ai", "--success", "--violet", "--warn", "--warn-bg", "--danger", "--focus"]) if (T[k] && !hex(T[k])) err(id, `${k} must be #rrggbb, got ${T[k]}`);
    if (T["--fg"] && T["--bg"] && contrast(T["--fg"], T["--bg"]) < 4.5) err(id, `fg on bg ${contrast(T["--fg"], T["--bg"]).toFixed(2)} < 4.5`);
    if (T["--fg"] && T["--surface-2"] && contrast(T["--fg"], T["--surface-2"]) < 4.5) err(id, `fg on surface-2 ${contrast(T["--fg"], T["--surface-2"]).toFixed(2)} < 4.5`);
    if (T["--on-accent"] && T["--accent"] && contrast(T["--on-accent"], T["--accent"]) < 4.5) err(id, `on-accent on accent ${contrast(T["--on-accent"], T["--accent"]).toFixed(2)} < 4.5`);
    if (T["--muted"] && T["--bg"] && contrast(T["--muted"], T["--bg"]) < 4.5) err(id, `muted on bg ${contrast(T["--muted"], T["--bg"]).toFixed(2)} < 4.5`);
    if (T["--muted"] && T["--surface-2"] && contrast(T["--muted"], T["--surface-2"]) < 4.5) warn(id, `muted on surface-2 ${contrast(T["--muted"], T["--surface-2"]).toFixed(2)} < 4.5`);
    if (T["--human"] && T["--ai"]) {
      const normal = contrast(T["--human"], T["--ai"]), cb = contrast(deut(T["--human"]), deut(T["--ai"]));
      if (normal < 1.4) err(id, `human vs ai too similar (${normal.toFixed(2)})`);
      if (cb < 1.3) warn(id, `human vs ai hard to tell apart with deuteranopia (${cb.toFixed(2)})`);
      for (const k of ["--human", "--ai"]) if (T["--surface-2"] && contrast(T[k], T["--surface-2"]) < 2) warn(id, `${k} faint on surface-2 (${contrast(T[k], T["--surface-2"]).toFixed(2)})`);
    }
    for (const k of ["--danger", "--warn"]) if (T[k] && T["--bg"] && contrast(T[k], T["--bg"]) < 3) warn(id, `${k} on bg ${contrast(T[k], T["--bg"]).toFixed(2)} < 3`);
    if (T["--warn"] && T["--warn-bg"] && contrast(T["--warn"], T["--warn-bg"]) < 4.5) warn(id, `warn on warn-bg ${contrast(T["--warn"], T["--warn-bg"]).toFixed(2)} < 4.5`);
    if (!t.mode) err(id, "themes with tokens must declare mode light|dark");
  }

  if (t.sprite) {
    if (t.sprite.length !== 16) err(id, `sprite has ${t.sprite.length} rows, need 16`);
    t.sprite.forEach((row, i) => { if (row.length !== 16) err(id, `sprite row ${i} has ${row.length} chars: "${row}"`); });
    const used = new Set(t.sprite.join("").replaceAll(".", ""));
    for (const ch of used) if (!(ch in t.legend)) err(id, `sprite char '${ch}' missing from legend`);
    for (const [ch, c] of Object.entries(t.legend)) { if (!hex(c)) err(id, `legend ${ch} bad colour ${c}`); if (ch === ".") err(id, "'.' is reserved for transparent"); }
    if (used.size > 8) warn(id, `sprite uses ${used.size} colours (>8)`);
    for (const name of PATCHES) {
      const p = t.patches?.[name];
      if (!p) { err(id, `missing patch ${name}`); continue; }
      if (p.length > 40) warn(id, `patch ${name} has ${p.length} cells`);
      if (!p.length) err(id, `patch ${name} is empty`);
      for (const [x, y, ch] of p) {
        if (!(Number.isInteger(x) && Number.isInteger(y) && x >= 0 && x < 16 && y >= 0 && y < 16)) err(id, `patch ${name} cell out of range: ${x},${y}`);
        else if (ch !== "." && !(ch in t.legend)) err(id, `patch ${name} uses '${ch}' not in legend`);
      }
    }
    const filled = t.sprite.join("").replaceAll(".", "").length;
    if (filled < 60) warn(id, `sprite is sparse (${filled}px)`);
    for (const name of Object.keys(t.patches ?? {})) if (!PATCHES.includes(name)) warn(id, `patch ${name} is never used by the engine`);
    // Expressions must differ from each other and from the bare sprite, or they're invisible.
    const look = (name) => { const g = t.sprite.map((r) => r.split("")); for (const [x, y, ch] of name ? t.patches?.[name] ?? [] : []) if (g[y]?.[x] !== undefined) g[y][x] = ch; return g.map((r) => r.join("")).join("/"); };
    const seen = new Map([[look(null), "neutral"]]);
    for (const name of PATCHES) { const l = look(name); if (seen.has(l)) err(id, `expression ${name} looks identical to ${seen.get(l)}`); else seen.set(l, name); }
    for (const m of Object.keys(t.lines ?? {})) if (!MOODS.includes(m)) err(id, `unknown mood in lines: ${m}`);
    checkLines(id, t.lines ?? {});
    // A kind a theme doesn't override (e.g. empty, no_key, offline) falls back to DEFAULT_LINES; that is fine.
  } else if (id !== "classic") err(id, "missing sprite");
}
console.log(`${THEMES.length} themes checked`);
for (const w of warnings) console.log("  warn  " + w);
for (const e of errors) console.log("  ERROR " + e);
console.log(errors.length ? `${errors.length} error(s)` : "no errors");
process.exit(errors.length ? 1 : 0);
