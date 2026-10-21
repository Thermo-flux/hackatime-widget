// Applies every theme in the running widget and screenshots each tab, plus the picker.
// Usage: node theme-shots.mjs <outDir> [WxH]   (app running in mock mode with --remote-debugging-port=9222)
import { writeFileSync, mkdirSync } from "node:fs";

const out = process.argv[2] ?? ".";
mkdirSync(out, { recursive: true });
const pages = await (await fetch(`http://127.0.0.1:${process.env.CDP_PORT ?? 9222}/json`)).json();
const ws = new WebSocket(pages.find((p) => p.type === "page").webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = (m) => { const o = JSON.parse(m.data); pending.get(o.id)?.(o); };
const send = (method, params = {}) => new Promise((res) => { const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expr) => (await send("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true })).result.result.value;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const shot = async (name) => writeFileSync(`${out}/${name}.png`, Buffer.from((await send("Page.captureScreenshot", { format: "png" })).result.data, "base64"));

// Optional WxH (e.g. 320x436, the real window) overrides the hidden window's own, slightly smaller viewport.
const size = process.argv[3];
if (size) {
  const [width, height] = size.split("x").map(Number);
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await sleep(300);
}

const ids = await ev("import('/js/themes.js').then(m => m.list().map(t => t.id))");
console.log(ids.length, "themes:", ids.join(" "));
for (const t of ids) {
  await ev(`import('/js/themes.js').then(m => m.apply(${JSON.stringify(t)}, { remember: false }))`);
  for (const tab of ["stats", "sessions", "pomodoro"]) {
    await ev(`document.getElementById('tab-${tab}').click()`);
    await sleep(450);
    await shot(`${t}-${tab}`);
  }
  await ev("document.getElementById('theme-btn').click()");
  await sleep(250);
  await shot(`${t}-picker`);
  await ev("document.getElementById('picker-close').click()");
  await ev("document.getElementById('tab-stats').click()");
}
await ev("import('/js/themes.js').then(m => m.apply('classic', { remember: false }))");
ws.close();
