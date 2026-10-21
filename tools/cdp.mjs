// Dev helper: drive the running widget over WebView2's debug port (--remote-debugging-port=9222).
// Usage: node cdp.mjs "<js expression>" [screenshot.png]
import { writeFileSync } from "node:fs";

const [js, shot] = process.argv.slice(2);
const pages = await (await fetch(`http://127.0.0.1:${process.env.CDP_PORT ?? 9222}/json`)).json();
const ws = new WebSocket(pages.find((p) => p.type === "page").webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });

let id = 0;
const pending = new Map();
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  pending.get(msg.id)?.(msg);
};
const send = (method, params = {}) =>
  new Promise((res) => { const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params })); });

if (js) {
  const r = await send("Runtime.evaluate", { expression: js, returnByValue: true, awaitPromise: true });
  console.log(r.result?.exceptionDetails ? "ERROR " + JSON.stringify(r.result.exceptionDetails) : r.result.result.value);
}
if (shot) {
  const r = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(shot, Buffer.from(r.result.data, "base64"));
}
ws.close();
