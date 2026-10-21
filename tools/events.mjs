// Dev helper: press a selector with a real click and print page lifecycle/console events for a few seconds.
// Usage: node events.mjs "<selector>" [seconds]
const [sel, secs = "5"] = process.argv.slice(2);
const pages = await (await fetch(`http://127.0.0.1:${process.env.CDP_PORT ?? 9222}/json`)).json();
const ws = new WebSocket(pages.find((p) => p.type === "page").webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
const t0 = Date.now();
ws.onmessage = (m) => {
  const o = JSON.parse(m.data);
  if (o.id) return pending.get(o.id)?.(o);
  const at = `+${Date.now() - t0}ms`;
  if (o.method === "Page.frameNavigated") console.log(at, "NAVIGATED", o.params.frame.url);
  else if (o.method === "Runtime.exceptionThrown") console.log(at, "EXCEPTION", JSON.stringify(o.params.exceptionDetails.exception?.description ?? o.params.exceptionDetails.text));
  else if (o.method === "Runtime.consoleAPICalled") console.log(at, "console." + o.params.type, o.params.args.map((a) => a.value ?? a.description).join(" "));
  else if (o.method === "Log.entryAdded") console.log(at, "log", o.params.entry.level, o.params.entry.text, o.params.entry.url ?? "");
  else if (o.method === "Inspector.targetCrashed") console.log(at, "CRASHED");
};
const send = (method, params = {}) => new Promise((res) => { const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params })); });
ws.onclose = () => console.log(`+${Date.now() - t0}ms socket closed`);
for (const d of ["Page", "Runtime", "Log", "Inspector"]) await send(`${d}.enable`);
const ev = async (e) => (await send("Runtime.evaluate", { expression: e, returnByValue: true })).result.result.value;
await ev(`window.__log = []; const o = Element.prototype.setAttribute; Element.prototype.setAttribute = function (n, v) { if (n === 'aria-selected' && this.id) window.__log.push(this.id + '=' + v + ' <- ' + new Error().stack.split('\\n').slice(2, 6).map((s) => s.trim().replace('http://tauri.localhost', '')).join(' | ')); return o.call(this, n, v); };
  for (const t of ['mousedown','mouseup','click','keydown','pointerdown']) document.addEventListener(t, (e) => window.__log.push(t + ' on ' + (e.target.id || e.target.tagName + '.' + e.target.className)), true);`);
await ev("document.getElementById('tab-pomodoro').click()");
const pt = await ev(`(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return {x: r.x + r.width / 2, y: r.y + r.height / 2}; })()`);
console.log("pressing", sel, JSON.stringify(pt));
for (const type of ["mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x: pt.x, y: pt.y, button: "left", clickCount: 1 });
await new Promise((r) => setTimeout(r, Number(secs) * 1000));
console.log((await ev("window.__log.join('\\n')")));
console.log("uptime s:", await ev("Math.round(performance.now() / 1000)"), "selected:", await ev("document.querySelector('[role=tab][aria-selected=true]')?.id"));
ws.close();
