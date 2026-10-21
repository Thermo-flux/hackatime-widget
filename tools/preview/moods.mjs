// Checks the mascot's mood and speech line against the data state in the preview harness (any OS).
//   node tools/preview/moods.mjs        exit code 1 on failure
import { chromium } from "playwright";
import { startServer } from "./serve.mjs";

const EXPECT = { zero: "empty", no_key: "no_key", unauthorized: "no_key", offline: "offline", normal: ["human", "ai", "streak", "idle"] };
const server = await startServer(0);
const base = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch();
let failed = 0;
for (const [scenario, want] of Object.entries(EXPECT)) {
  const page = await browser.newPage({ viewport: { width: 320, height: 436 } });
  await page.goto(`${base}?scenario=${scenario}`);
  await page.waitForTimeout(1500);
  const got = await page.evaluate(() => ({ mood: document.getElementById("mascot-bar").dataset.mood, say: document.getElementById("mascot-say").textContent }));
  const ok = [].concat(want).includes(got.mood);
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"} ${scenario}: mood=${got.mood} line="${got.say}"`);
  await page.close();
}
await browser.close();
server.close();
process.exit(failed ? 1 : 0);
