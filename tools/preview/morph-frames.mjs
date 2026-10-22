// Frame-by-frame screenshots of the compact-mode morph (mode.js), per theme, plus numeric checks that the pill lands
// exactly where the compact window will show it. No Tauri: the stub answers set_mode; the window resize is simulated
// with the viewport size (320x436 full, 240x60 compact, the window's top-right corner fixed).
//   node tools/preview/morph-frames.mjs out/ [--themes classic,avocado] [--shift 80,0]
// Writes <theme>__collapse__<ms>.png / <theme>__expand__<ms>.png and sheet-morph.png. Exits 1 if a check fails.
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";
import { startServer } from "./serve.mjs";

const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1]?.split(",") : null; };
const out = args.find((a, i) => !a.startsWith("--") && !args[i - 1]?.startsWith("--")) ?? "out-morph";
const shift = (flag("shift") ?? ["0", "0"]).map(Number);
mkdirSync(out, { recursive: true });
const FULL = [320, 436], COMPACT = [240, 60];
const TIMES = [0, 30, 60, 90, 120, 150, 180, 220];

const server = await startServer(0);
const base = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: FULL[0], height: FULL[1] }, deviceScaleFactor: 2 });
await ctx.addInitScript(() => { try { localStorage.setItem("hackatime-widget.compact-hint", "1"); } catch {} });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const failures = [];
const check = (ok, msg) => { if (!ok) failures.push(msg); };

const page = await ctx.newPage();
page.on("pageerror", (e) => failures.push(`page error: ${e.message}`));
await page.goto(`${base}?scenario=normal`);
await page.waitForLoadState("networkidle");
const themes = flag("themes") ?? (await page.evaluate(() => import("/js/themes.js").then((m) => m.list().map((t) => t.id))));

// Starts a switch and freezes its animations as soon as they exist. Returns once they are paused.
const startFrozen = (to) => page.evaluate(async (to) => {
  const m = await import("/js/mode.js");
  window.__switch = m.setMode(to);
  const ids = new Set(["morph", "shell", "pill"]);
  const ours = () => document.getAnimations().filter((a) => ids.has(a.effect?.target?.id));
  for (let i = 0; i < 200 && !ours().length; i++) await new Promise((r) => setTimeout(r, 5));
  // Expand creates its animations after the (stubbed) window resize settles; all are created in one task.
  for (const a of ours()) a.pause();
  return ours().length;
}, to);
const seek = (t) => page.evaluate((t) => {
  for (const a of document.getAnimations()) if (["morph", "shell", "pill"].includes(a.effect?.target?.id)) a.currentTime = t;
}, t);
const finish = () => page.evaluate(async () => {
  for (const a of document.getAnimations()) if (["morph", "shell", "pill"].includes(a.effect?.target?.id)) a.play();
  await window.__switch;
});
const rect = (sel) => page.evaluate((sel) => { const r = document.querySelector(sel).getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; }, sel);
const near = (a, b) => a.every((v, i) => Math.abs(v - b[i]) < 0.6);

const rows = [];
for (const theme of themes) {
  await page.setViewportSize({ width: FULL[0], height: FULL[1] });
  await page.evaluate((id) => import("/js/themes.js").then((m) => m.apply(id, { remember: false })), theme);
  await page.evaluate(() => { window.__TAURI_STUB__.shift = null; });
  await sleep(250);
  const row = [];

  // ---- collapse ----
  check((await startFrozen("compact")) === 5, `${theme}: collapse should run 5 animations`);
  for (const t of TIMES) {
    await seek(t);
    const file = `${theme}__collapse__${t}.png`;
    await page.screenshot({ path: `${out}/${file}`, omitBackground: true });
    row.push({ file, label: `collapse ${t}ms`, w: FULL[0], h: FULL[1] });
  }
  // End of the morph, still in the full window: pill and surface on the pill's box, hand-over must be invisible.
  const morphEnd = await rect("#morph"), pillEnd = await rect("#pill");
  const target = [FULL[0] - 230, 10, 220, 40];
  check(near(morphEnd, target), `${theme}: morph surface ends at ${morphEnd}, want ${target}`);
  check(near(pillEnd, target), `${theme}: pill ends at ${pillEnd}, want ${target}`);
  // Simulate the window shrinking around its top-right corner while set_mode runs.
  const done = finish();
  await sleep(30);
  await page.setViewportSize({ width: COMPACT[0], height: COMPACT[1] });
  await done;
  const after = await rect("#pill");
  // Screen x: the compact window starts (320 - 240) px to the right of the full one.
  check(near([after[0] + FULL[0] - COMPACT[0], after[1], after[2], after[3]], target), `${theme}: pill jumps across the resize (${after})`);
  const state = await page.evaluate(() => ({ mode: document.documentElement.dataset.mode, cls: document.documentElement.className, calls: window.__TAURI_STUB__.modeCalls.slice(-1)[0] }));
  check(state.mode === "compact" && !/is-(morphing|snapping)/.test(state.cls) && state.calls === "compact", `${theme}: after collapse ${JSON.stringify(state)}`);
  const file = `${theme}__compact.png`;
  await page.screenshot({ path: `${out}/${file}`, omitBackground: true });
  row.push({ file, label: "compact window", w: COMPACT[0], h: COMPACT[1] });

  // ---- expand ---- (the window grows first; with --shift, clamping moved it by `shift`)
  await page.evaluate((s) => { window.__TAURI_STUB__.shift = s; }, shift);
  const grown = page.evaluate(() => new Promise((r) => requestAnimationFrame(r)));
  await grown;
  const startExpand = startFrozen("full");
  await sleep(20);
  await page.setViewportSize({ width: FULL[0], height: FULL[1] });
  check((await startExpand) === 5, `${theme}: expand should run 5 animations`);
  await seek(0);
  const pillStart = await rect("#pill");
  const want = [FULL[0] - 230 - shift[0], 10 - shift[1], 220, 40];
  check(near(pillStart, want), `${theme}: expand starts the pill at ${pillStart}, want ${want}`);
  for (const t of TIMES) {
    await seek(t);
    const f = `${theme}__expand__${t}.png`;
    await page.screenshot({ path: `${out}/${f}`, omitBackground: true });
    row.push({ file: f, label: `expand ${t}ms`, w: FULL[0], h: FULL[1] });
  }
  await finish();
  const end = await page.evaluate(() => ({ mode: document.documentElement.dataset.mode, cls: document.documentElement.className, shell: getComputedStyle(document.getElementById("shell")).opacity }));
  check(end.mode === "full" && !/is-(morphing|snapping)/.test(end.cls) && end.shell === "1", `${theme}: after expand ${JSON.stringify(end)}`);
  rows.push({ theme, row });
  console.log(`${theme.padEnd(10)} done`);
}

// One sheet: a row per theme, collapse frames then expand frames, on a mid-grey desktop so shadows show.
const cells = rows.map(({ theme, row }) => `<div class="r"><b>${theme}</b>${row.map((c) => `<figure><img src="${c.file}" style="width:${c.w / 2}px;height:${c.h / 2}px"><figcaption>${c.label}</figcaption></figure>`).join("")}</div>`).join("");
const html = `<style>body{margin:0;background:#8a8f99;font:10px sans-serif;color:#fff}.r{display:flex;gap:4px;align-items:flex-start;padding:4px}
  b{width:60px}figure{margin:0}img{display:block;outline:1px dashed rgba(255,255,255,.35)}</style>${cells}`;
const sheet = resolve(out, "sheet-morph.html");
writeFileSync(sheet, html);
const sp = await ctx.newPage();
await sp.goto(pathToFileURL(sheet).href);
await sp.screenshot({ path: `${out}/sheet-morph.png`, fullPage: true });
await browser.close();
server.close();
for (const f of failures) console.log("FAIL " + f);
console.log(failures.length ? `${failures.length} failure(s)` : `morph checks clean for ${themes.length} theme(s)`);
process.exit(failures.length ? 1 : 0);
