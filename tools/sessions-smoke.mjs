// Sessions-tab interaction test against the running widget (mock mode, --remote-debugging-port=9222).
// Usage: node sessions-smoke.mjs [shotDir]
import { writeFileSync } from "node:fs";

const shotDir = process.argv[2] ?? ".";
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

async function click(sel) {
  const pt = await ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; const r = e.getBoundingClientRect(); const t = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2); return { x: r.x + r.width / 2, y: r.y + r.height / 2, hit: t === e || e.contains(t) }; })()`);
  if (!pt) return `MISSING ${sel}`;
  for (const type of ["mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x: pt.x, y: pt.y, button: "left", clickCount: 1 });
  await sleep(400);
  return pt.hit ? "ok" : `NOT HIT ${sel}`;
}
const log = console.log;

log("tab:", await click("#tab-sessions"), "| selected:", await ev("document.querySelector('[role=tab][aria-selected=true]').id"));
await sleep(500);
log("== idle summary");
for (const id of ["sum-count", "sum-total", "sum-avg", "sum-streak", "sum-best", "sum-active", "sum-today", "sum-week"]) log(`  ${id}:`, await text("#" + id));
log("  chart bars:", await ev("document.querySelectorAll('#sum-chart .chart__bar').length"), "| empty:", await ev("document.querySelectorAll('#sum-chart .chart__bar--empty').length"));
log("  start enabled:", await ev("!document.getElementById('sess-start').disabled"), "| live hidden:", await ev("document.getElementById('sess-live').hidden"));
await shot("s1-idle");

log("== start session:", await click("#sess-start"));
await sleep(1500);
log("  idle hidden:", await ev("document.getElementById('sess-idle').hidden"), "| live visible:", await ev("!document.getElementById('sess-live').hidden"));
const t1 = await text("#live-elapsed"); await sleep(2200); const t2 = await text("#live-elapsed");
log("  clock ticking:", t1, "->", t2, t1 !== t2);
for (const id of ["live-human", "live-ai", "live-idle", "live-focus", "live-today", "live-note"]) log(`  ${id}:`, await text("#" + id));
await shot("s2-live");

log("== switch tabs while running, then return");
await click("#tab-pomodoro"); await sleep(300);
await click("#tab-sessions"); await sleep(600);
log("  still live:", await ev("!document.getElementById('sess-live').hidden"), "| clock:", await text("#live-elapsed"));

log("== end too-quickly (accidental) session");
log("  stop:", await click("#sess-stop"));
await sleep(600);
log("  back to idle:", await ev("!document.getElementById('sess-idle').hidden"), "| sessions:", await text("#sum-count"), "(unchanged = discarded)");

log("== layout");
log(" ", JSON.stringify(await ev(`(() => { const c = document.querySelector('.card').getBoundingClientRect(); const bad = [...document.querySelectorAll('.card *')].filter(e => { const r = e.getBoundingClientRect(); return r.width && (r.right > c.right + 0.5 || r.left < c.left - 0.5 || r.bottom > c.bottom + 0.5); }).map(e => e.id || e.className); return { scrollH: document.scrollingElement.scrollHeight, innerH: innerHeight, overflowing: bad }; })()`)));
log("  error shown:", await ev("!document.getElementById('sess-error').hidden"));
await shot("s3-after");
ws.close();
