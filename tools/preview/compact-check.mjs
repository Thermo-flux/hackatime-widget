// Checks the compact pill's pure logic in a real page (no Tauri): what it shows per state, its tooltip text, the hotkey label.
//   PLAYWRIGHT_BROWSERS_PATH=D:/dev/pw node tools/preview/compact-check.mjs
import { chromium } from "playwright";
import { startServer } from "./serve.mjs";

const server = await startServer(0);
const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(`http://127.0.0.1:${server.address().port}/?scenario=normal`);
await page.waitForLoadState("networkidle");

const out = await page.evaluate(async () => {
  const { content, describe } = await import("/js/compact.js");
  const { label } = await import("/js/hint.js");
  const H = 3600;
  const day = { total_seconds: 6.25 * H, ai_seconds: 2.25 * H, human_seconds: 4 * H, ai_available: true };
  const unknown = { ...day, ai_available: false, ai_seconds: 0, human_seconds: day.total_seconds };
  const pomo = { running: true, phase: "focus", remaining_secs: 17 * 60 + 23 };
  const rest = { running: true, phase: "short_break", remaining_secs: 192 };
  const none = { running: false, phase: "focus", remaining_secs: 1500 };
  return {
    total: content(day, none, null),
    pomo: content(day, pomo, 3725),
    brk: content(day, rest, null),
    session: content(day, none, 3725),
    noData: content(null, none, null),
    describeTotal: describe(day, content(day, none, null)),
    describeUnknown: describe(unknown, content(unknown, none, null)),
    describePomo: describe(day, content(day, pomo, null)),
    win: label("CommandOrControl+Alt+Shift+H", false),
    mac: label("CommandOrControl+Alt+Shift+H", true),
  };
});

const want = {
  total: { text: "6h 15m", dot: null },
  pomo: { text: "17:23", dot: "focus" },
  brk: { text: "03:12", dot: "break" },
  session: { text: "01:02:05", dot: "session" },
  noData: { text: "–", dot: null },
};
let failed = 0;
const check = (name, got, expected) => { const ok = got === expected; if (!ok) failed++; console.log(`${ok ? "ok  " : "FAIL"} ${name}: ${got}${ok ? "" : `  (wanted ${expected})`}`); };
for (const [k, w] of Object.entries(want)) { check(`${k}.text`, out[k].text, w.text); check(`${k}.dot`, out[k].dot, w.dot); }
check("tooltip", out.describeTotal, "6h 15m today · Human 4h 00m · AI 2h 15m. Click to expand.");
check("tooltip, AI unknown", out.describeUnknown, "6h 15m today. Click to expand.");
check("tooltip, pomodoro", out.describePomo, "Focus 17:23 left · 6h 15m today · Human 4h 00m · AI 2h 15m. Click to expand.");
check("hotkey label", out.win, "Ctrl+Alt+Shift+H");
check("hotkey label, mac", out.mac, "Cmd+Option+Shift+H");
await browser.close();
server.close();
process.exit(failed ? 1 : 0);
