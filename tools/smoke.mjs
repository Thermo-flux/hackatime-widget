// Interaction smoke test against the running widget (start it with --remote-debugging-port=9222).
// Uses real input events so hit-testing problems show up. Run: node smoke.mjs [shotDir]
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

async function click(sel) {
  const pt = await ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; const r = e.getBoundingClientRect(); return {x: r.x + r.width / 2, y: r.y + r.height / 2, hit: document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2) === e || e.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2))}; })()`);
  if (!pt) return `MISSING ${sel}`;
  for (const type of ["mousePressed", "mouseReleased"])
    await send("Input.dispatchMouseEvent", { type, x: pt.x, y: pt.y, button: "left", clickCount: 1 });
  await sleep(150);
  if (pt.hit) return "ok";
  const what = await ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); const r = e.getBoundingClientRect(); const t = document.elementFromPoint(${pt.x}, ${pt.y}); return JSON.stringify({ selRect: [r.x, r.y, r.width, r.height].map(Math.round), top: t && (t.id || t.tagName + '.' + t.className), panelHidden: document.getElementById('panel-pomodoro').hidden, tab: document.querySelector('[role=tab][aria-selected=true]').id, up: Math.round(performance.now() / 1000) }); })()`);
  return `NOT HIT ${sel} ${what}`;
}
const VK = { Enter: [13, "Enter", "\r"], " ": [32, "Space", " "], Escape: [27, "Escape"], ArrowLeft: [37, "ArrowLeft"], ArrowUp: [38, "ArrowUp"], ArrowRight: [39, "ArrowRight"], ArrowDown: [40, "ArrowDown"], Home: [36, "Home"], End: [35, "End"] };
async function key(k) {
  const [vk, code, txt] = VK[k] ?? [0, k];
  await send("Input.dispatchKeyEvent", { type: txt ? "keyDown" : "rawKeyDown", key: k, code, windowsVirtualKeyCode: vk, ...(txt ? { text: txt } : {}) });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code, windowsVirtualKeyCode: vk });
  await sleep(150);
}
const log = (...a) => console.log(...a);
const text = (sel) => ev(`document.querySelector(${JSON.stringify(sel)})?.textContent.trim()`);

log("== stats");
await click("#tab-stats");
log("total today:", await text("#total"), "| badge:", await ev("!document.getElementById('mock-badge').hidden"));
for (const r of ["7d", "30d", "all", "today"]) {
  const res = await click(`[data-range=${r}]`);
  log(`range ${r}:`, res, await text("#total"), "| pressed:", await ev(`document.querySelector('[data-range=${r}]').getAttribute('aria-pressed')`), "| langs:", await ev("document.querySelectorAll('#languages li').length"));
}
await shot("1-stats");

log("== tabs (mouse + keyboard)");
log("click pomodoro tab:", await click("#tab-pomodoro"), "| selected:", await ev("document.getElementById('tab-pomodoro').getAttribute('aria-selected')"), "| badge visible:", await ev("getComputedStyle(document.getElementById('mock-badge')).display !== 'none'"));
await ev("document.getElementById('tab-pomodoro').focus()");
await key("ArrowLeft");
log("ArrowLeft ->", await ev("document.querySelector('[role=tab][aria-selected=true]').id"), "| focused:", await ev("document.activeElement.id"));
await key("ArrowRight");
log("ArrowRight ->", await ev("document.querySelector('[role=tab][aria-selected=true]').id"));

log("== pomodoro (the ring is the start/pause button)");
const ring = () => ev("(() => { const r = document.getElementById('pomo-toggle'); return r.dataset.state + ' | ' + document.getElementById('pomo-hint').textContent.trim() + ' | ' + r.getAttribute('aria-label'); })()");
let fails = 0;
const expect = (what, ok, got) => { if (!ok) fails++; log(`${ok ? "ok  " : "FAIL"} ${what}${got === undefined ? "" : ` -> ${got}`}`); };
expect("idle: reset hidden", await ev("document.getElementById('pomo-reset').classList.contains('is-reserved')"));
expect("idle ring", /^idle \| Start \| Start Focus/.test(await ring()), await ring());
log("start (mouse on ring):", await click("#pomo-toggle"));
await sleep(2200);
expect("running ring", /^running \| Pause/.test(await ring()), await ring());
log("after 2s:", await text("#pomo-time"), "| title:", await ev("document.title"), "| reset hidden:", await ev("document.getElementById('pomo-reset').classList.contains('is-reserved')"));
await shot("2-pomodoro-running");
await ev("document.getElementById('pomo-toggle').focus()");
await key("Enter"); // keyboard: Enter on the focused ring pauses
expect("Enter pauses", /^paused \| Resume/.test(await ring()), await ring());
const frozen = await text("#pomo-time"); await sleep(1500);
expect("frozen while paused", frozen === (await text("#pomo-time")), frozen);
await shot("2b-pomodoro-paused");
await key(" "); // Space resumes
expect("Space resumes", /^running/.test(await ring()), await ring());
log("pause again:", await click("#pomo-toggle"), await ring());
log("skip:", await click("#pomo-skip"), "| phase:", await text("#pomo-phase"), await text("#pomo-time"), "| data-phase:", await ev("document.getElementById('panel-pomodoro').dataset.phase"));
await shot("3-pomodoro-break");
log("skip again:", await click("#pomo-skip"), "| phase:", await text("#pomo-phase"), "| sessions:", await text("#pomo-sessions"));
expect("idle after skips: reset hidden", await ev("document.getElementById('pomo-reset').classList.contains('is-reserved')"));

log("== pomodoro: set times");
const val = (k) => ev(`document.querySelector('.stepper[data-key=${k}] [role=spinbutton]').getAttribute('aria-valuenow')`);
const btn = (k, dir) => `.stepper[data-key=${k}] .stepper__btn[data-dir="${dir}"]`;
log("open:", await click("#pomo-set"));
expect("settings shown, ring hidden", await ev("!document.getElementById('pomo-settings').hidden && document.getElementById('pomo-toggle').hidden"));
expect("focus on first value", (await ev("document.activeElement.getAttribute('aria-labelledby')")) === "pomo-l-focus");
expect("save always enabled", !(await ev("document.getElementById('pomo-save').disabled")));
log("chip 50/10:", await click('[data-preset="50,10,30"]'));
expect("preset applied", `${await val("focus_min")}/${await val("short_break_min")}/${await val("long_break_min")}` === "50/10/30");
log("focus -:", await click(btn("focus_min", -1)));
expect("focus 45, chip released", (await val("focus_min")) === "45" && (await ev("document.querySelector('[data-preset=\"50,10,30\"]').getAttribute('aria-pressed')")) === "false", await val("focus_min"));
await ev("document.querySelector('.stepper[data-key=short_break_min] [role=spinbutton]').focus()");
await key("ArrowDown");
expect("ArrowDown on rest -> 9", (await val("short_break_min")) === "9", await val("short_break_min"));
await key("Home");
expect("Home -> min 1", (await val("short_break_min")) === "1");
await key("End");
expect("End -> max 60", (await val("short_break_min")) === "60");
for (let i = 0; i < 12; i++) await click(btn("short_break_min", -1));
expect("12 x minus from 60 -> 8 (fives down to 10, then ones)", (await val("short_break_min")) === "8", await val("short_break_min"));
expect("note says applies now", /right away/.test(await text("#pomo-settings-note")), await text("#pomo-settings-note"));
await shot("5-pomodoro-settings");
log("save:", await click("#pomo-save"));
await sleep(300);
expect("settings closed", await ev("document.getElementById('pomo-settings').hidden && !document.getElementById('pomo-toggle').hidden"));
expect("idle timer shows the new length", (await text("#pomo-time")) === "45:00", await text("#pomo-time"));
expect("lengths shown", (await text("#pomo-lengths")) === "45 / 8 min", await text("#pomo-lengths"));
expect("status announced", /Saved/.test(await text("#pomo-status")), await text("#pomo-status"));
await shot("6-pomodoro-saved");
const cfgPath = process.env.HACKATIME_WIDGET_CONFIG;
if (cfgPath) {
  const { readFileSync } = await import("node:fs");
  const cfg = JSON.parse(readFileSync(cfgPath, "utf8").replace(/^ï»¿/, ""));
  const p = cfg.pomodoro ?? {};
  expect("config.json saved", p.focus_min === 45 && p.short_break_min === 8 && p.long_break_min === 30, JSON.stringify(p));
  expect("config.json keeps other keys", "api_key" in cfg && "base_url" in cfg && "long_break_every" in p, Object.keys(cfg).join(","));
}

log("== pomodoro: change times while running");
await click("#pomo-toggle");
await sleep(1200);
await click("#pomo-set");
expect("note says it keeps its time", /keeps its time/.test(await text("#pomo-settings-note")), await text("#pomo-settings-note"));
await click('[data-preset="25,5,15"]');
await click("#pomo-save");
await sleep(300);
const t = await text("#pomo-time");
expect("running timer kept its time", /^4[45]:\d\d$/.test(t) && (await ev("document.getElementById('pomo-toggle').dataset.state")) === "running", t);
expect("lengths say next", (await text("#pomo-lengths")) === "Next: 25 / 5 min", await text("#pomo-lengths"));
await click("#pomo-reset");
expect("reset applies new length", (await text("#pomo-time")) === "25:00", await text("#pomo-time"));
log("open + Escape cancels:", await click("#pomo-set"));
await click(btn("focus_min", 1));
await key("Escape");
expect("Escape closed without saving", (await ev("document.getElementById('pomo-settings').hidden")) && (await text("#pomo-time")) === "25:00" && (await ev("document.activeElement.id")) === "pomo-set");
log(fails ? `POMODORO: ${fails} FAILED` : "POMODORO: all checks passed");

log("== layout");
log(JSON.stringify(await ev(`(() => { const c = document.querySelector('.card').getBoundingClientRect(); const bad = [...document.querySelectorAll('.card *')].filter(e => { const r = e.getBoundingClientRect(); return r.width && (r.right > c.right + 0.5 || r.left < c.left - 0.5 || r.bottom > c.bottom + 0.5); }).map(e => e.id || e.className); return { scrollH: document.scrollingElement.scrollHeight, innerH: innerHeight, overflowing: bad }; })()`)));

log("== setup/error forms reachable");
await click("#tab-stats");
log("close button present:", await ev("!!document.getElementById('close')"));
await shot("4-final");
log("hide:", await click("#close"));
await sleep(800);
ws.close();
