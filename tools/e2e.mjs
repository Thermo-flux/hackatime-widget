// End-to-end check of the real (non-mock) code path in the running widget, against fake-hackatime.mjs.
// Usage: node e2e.mjs <scenario> [shotDir]   scenarios: ok | no-ai | bad-key | down | no-key
import { writeFileSync } from "node:fs";

const [scenario, shotDir = "."] = process.argv.slice(2);
const pages = await (await fetch(`http://127.0.0.1:${process.env.CDP_PORT ?? 9222}/json`)).json();
const ws = new WebSocket(pages.find((p) => p.type === "page").webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = (m) => { const o = JSON.parse(m.data); pending.get(o.id)?.(o); };
const send = (method, params = {}) => new Promise((res) => { const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expr) => (await send("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true })).result.result.value;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const shot = async (name) => writeFileSync(`${shotDir}/${name}.png`, Buffer.from((await send("Page.captureScreenshot", { format: "png" })).result.data, "base64"));
const text = (sel) => ev(`document.querySelector(${JSON.stringify(sel)})?.textContent.trim()`);
const visible = (sel) => ev(`!document.querySelector(${JSON.stringify(sel)}).hidden`);
const click = (sel) => ev(`document.querySelector(${JSON.stringify(sel)}).click()`);
const log = console.log;

await sleep(1200);
log(`== scenario: ${scenario}`);
const state = (await visible("#stats-ok")) ? "ok" : (await visible("#stats-key")) ? "needs-key" : (await visible("#stats-error")) ? "error" : "loading";
log("stats view:", state);
if (state === "ok") {
  log("  total:", await text("#total"), "| human:", await text("#human"), await text("#human-pct") + "%", "| ai:", await text("#ai"), await text("#ai-pct") + "%");
  log("  languages:", await ev("[...document.querySelectorAll('#languages .lang__name')].map(e => e.textContent).join(' | ')"));
  log("  injected <img> elements:", await ev("document.querySelectorAll('#languages img').length"));
  for (const r of ["week", "all"]) {
    await click(`[data-range=${r}]`); await sleep(900);
    log(`  range ${r}:`, await text("#total"), "| ai:", await text("#ai"), "| view:", (await visible("#stats-ok")) ? "ok" : "NOT ok");
  }
  await click("[data-range=today]"); await sleep(600);
}
if (state === "needs-key") log("  message:", await text("#key-msg"), "| hint present:", await ev("!!document.querySelector('.hint')"));
if (state === "error") log("  message:", await text("#error-msg"));
await shot(`e2e-${scenario}-stats`);

if (state === "ok") {
  await click("#tab-sessions"); await sleep(600);
  await click("#sess-start"); await sleep(1500);
  await sleep(45_000); // enough heartbeats at 20s spacing; the app polls every 30s, so force a refresh
  await click("#tab-pomodoro"); await click("#tab-sessions"); await sleep(1500);
  log("== live session");
  log("  elapsed:", await text("#live-elapsed"), "| coding:", await text("#live-coding"), "| focus:", await text("#live-focus"));
  log("  human:", await text("#live-human"), "| ai:", await text("#live-ai"), "| note:", await text("#live-note"));
  await shot(`e2e-${scenario}-live`);
  await click("#sess-stop"); await sleep(800);
  log("  sessions after stop:", await text("#sum-count"), "| total:", await text("#sum-total"));
}
ws.close();
