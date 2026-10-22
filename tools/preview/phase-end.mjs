// Checks the mascot line a finished focus round produces, per how much of it was coded (stub scenarios pomo_finishing*).
//   node tools/preview/phase-end.mjs
import { chromium } from "playwright";
import { startServer } from "./serve.mjs";
import { DEFAULT_LINES } from "../../ui/js/mascot.js";

const server = await startServer(0);
const base = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch();
let failed = false;
for (const [scenario, kind] of [["pomo_finishing", "focusHigh"], ["pomo_finishing_low", "focusLow"]]) {
  const page = await (await browser.newContext({ viewport: { width: 320, height: 436 } })).newPage();
  await page.goto(`${base}?scenario=${scenario}`);
  await page.click("#tab-pomodoro");
  // The line is on screen for a few seconds around the third poll; collect everything the bubble says.
  const seen = new Set();
  for (let i = 0; i < 40; i++) {
    seen.add(await page.evaluate(() => document.querySelector("#mascot-say")?.textContent?.trim() ?? ""));
    await page.waitForTimeout(150);
  }
  const said = [...seen].find((t) => DEFAULT_LINES[kind].includes(t)) ?? [...seen].join(" | ");
  const ok = DEFAULT_LINES[kind].includes(said);
  console.log(`${scenario}: "${said}" ${ok ? "ok" : `NOT one of ${kind}`}`);
  failed ||= !ok;
}
await browser.close();
server.close();
process.exit(failed ? 1 : 0);
