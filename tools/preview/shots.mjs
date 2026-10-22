// Screenshots every scenario x theme x tab at the real widget size (320x436 @2x, transparent), then builds one
// contact sheet per scenario. Needs only Node + Playwright's Chromium; no Tauri, any OS.
//   node tools/preview/shots.mjs out/ [--scenarios normal,zero] [--themes classic,phosphor] [--tabs stats,sessions] [--list projects]
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";
import { startServer } from "./serve.mjs";

export const SCENARIOS = [
  "normal", "no_key", "unauthorized", "offline", "offline_cached", "zero", "ai_unavailable", "huge", "session_live",
  "session_live_noactivity", "sessions_empty", "pomo_running", "pomo_paused", "pomo_break", "pomo_many", "pomo_nolong", "pomo_finishing", "pomo_finishing_low", "down", "mock",
  "compact_normal", "compact_pomo_running", "compact_pomo_break", "compact_session_live", "compact_ai_unavailable", "compact_zero", "compact_huge", "compact_offline",
];
const PILL = [240, 60]; // the compact window
const TABS = ["stats", "sessions", "pomodoro"];

const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1]?.split(",") : null; };
const out = args.find((a, i) => !a.startsWith("--") && !args[i - 1]?.startsWith("--")) ?? "out";
mkdirSync(out, { recursive: true });

const server = await startServer(0);
const base = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 320, height: 436 }, deviceScaleFactor: 2 });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const scenarios = flag("scenarios") ?? SCENARIOS;
const tabs = flag("tabs") ?? TABS;
const list = flag("list")?.[0]; // "projects" to show the Projects list on the Stats tab
const written = {};

for (const scenario of scenarios) {
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.error(`[${scenario}] page error:`, e.message));
  await page.goto(`${base}?scenario=${scenario}`);
  await page.waitForLoadState("networkidle");
  const compact = scenario.startsWith("compact_");
  if (compact) {
    await page.setViewportSize({ width: PILL[0], height: PILL[1] });
    await page.evaluate(async () => {
      document.documentElement.dataset.mode = "compact";
      await import("/js/compact.js").then((m) => m.start());
    });
    await sleep(300);
  }
  const themes = flag("themes") ?? (await page.evaluate(() => import("/js/themes.js").then((m) => m.list().map((t) => t.id))));
  written[scenario] = [];
  for (const theme of themes) {
    await page.evaluate((id) => import("/js/themes.js").then((m) => m.apply(id, { remember: false })), theme);
    for (const tab of compact ? ["pill"] : tabs) {
      if (compact) {
        await page.evaluate(() => import("/js/compact.js").then((m) => m.render()));
      } else await page.click(`#tab-${tab}`);
      if (tab === "stats" && scenario === "huge") await page.click('[data-range="all"]'); // widest strings: All-time
      if (tab === "stats" && list) await page.click(`[data-list="${list}"]`);
      await sleep(350);
      const file = `${scenario}__${theme}__${tab}.png`;
      await page.screenshot({ path: `${out}/${file}`, omitBackground: true });
      written[scenario].push({ theme, tab, file });
    }
  }
  await page.close();
}

// One sheet per scenario: a row per theme, a column per tab.
const sheetPage = await ctx.newPage();
for (const [scenario, items] of Object.entries(written)) {
  const compact = scenario.startsWith("compact_");
  const cols = compact ? 3 : tabs.length;
  const [w, h] = compact ? PILL : [320, 436];
  const cells = items.map((i) => `<figure><img src="${i.file}"><figcaption>${i.theme} · ${i.tab}</figcaption></figure>`).join("");
  const html = `<style>body{margin:0;background:#777;font:11px sans-serif;color:#fff}
    .g{display:grid;grid-template-columns:repeat(${cols},${w}px);gap:6px;padding:6px}
    figure{margin:0}img{width:${w}px;height:${h}px;display:block}figcaption{padding:2px 0}</style><div class="g">${cells}</div>`;
  const { pathToFileURL } = await import("node:url");
  const { resolve } = await import("node:path");
  const sheetFile = resolve(out, `sheet-${scenario}.html`);
  writeFileSync(sheetFile, html);
  await sheetPage.goto(pathToFileURL(sheetFile).href);
  await sheetPage.waitForLoadState("load");
  await sheetPage.screenshot({ path: `${out}/sheet-${scenario}.png`, fullPage: true });
  console.log(`sheet-${scenario}.png (${items.length} shots)`);
}
await browser.close();
server.close();
