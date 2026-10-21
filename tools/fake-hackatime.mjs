// Local stand-in for the Hackatime endpoints this widget uses, following the project's swagger spec.
//   node fake-hackatime.mjs            (env: PORT=4010 KEY=testkey FAIL_AI=1 FAIL_ALL=500 LOG=path)
// Point config.json at it: { "api_key": "testkey", "base_url": "http://127.0.0.1:4010/api/hackatime/v1" }
import { createServer } from "node:http";
import { appendFileSync } from "node:fs";

const PORT = Number(process.env.PORT ?? 4010);
const KEY = process.env.KEY ?? "testkey";
const LOG = process.env.LOG;
const json = (res, status, body) => { res.writeHead(status, { "content-type": "application/json" }); res.end(JSON.stringify(body)); };

createServer((req, res) => {
  const url = new URL(req.url, `http://127.0.0.1:${PORT}`);
  const q = url.searchParams;
  if (LOG) appendFileSync(LOG, `${req.method} ${url.pathname}?${q}  auth=${req.headers.authorization ?? "-"}\n`);

  if (process.env.FAIL_ALL) return json(res, Number(process.env.FAIL_ALL), { error: "forced" });
  if (req.headers.authorization !== `Bearer ${KEY}`) return json(res, 401, { error: "Unauthorized" });

  if (url.pathname === "/api/v1/users/my/stats") {
    const start = q.get("start_date"), end = q.get("end_date");
    const days = start ? Math.max(1, Math.round((Date.parse(end) - Date.parse(start)) / 86_400_000) + 1) : 400;
    const total = 7200 * days; // 2h per day
    if (q.get("total_seconds") === "true") {
      if (q.get("filter_by_category")) {
        if (process.env.FAIL_AI) return json(res, 500, { error: "forced" });
        return json(res, 200, { total_seconds: total * 0.3 });
      }
      return json(res, 200, { total_seconds: total });
    }
    return json(res, 200, {
      data: {
        total_seconds: total,
        languages: [
          { name: "Rust", total_seconds: total * 0.5, percent: 50 },
          { name: "TypeScript", total_seconds: total * 0.3, percent: 30 },
          { name: "<img src=x onerror=alert(1)>", total_seconds: total * 0.2, percent: 20 },
        ],
        projects: [{ name: "hackatime-widget", total_seconds: total, percent: 100 }],
      },
      trust_factor: { trust_level: "green", trust_value: 2 },
    });
  }

  if (url.pathname === "/api/v1/my/heartbeats") {
    const from = Date.parse(q.get("start_time")) / 1000, to = Date.parse(q.get("end_time")) / 1000;
    const heartbeats = [];
    for (let t = from, i = 0; t <= to; t += 20, i++)
      heartbeats.push({ id: i, time: t, type: "file", entity: "main.rs", project: "hackatime-widget", language: i % 4 ? "Rust" : "TypeScript", category: i % 3 === 0 ? "ai coding" : "coding" });
    return json(res, 200, { start_time: q.get("start_time"), end_time: q.get("end_time"), total_seconds: to - from, heartbeats });
  }

  json(res, 404, { error: "not found" });
}).listen(PORT, "127.0.0.1", () => console.log(`fake hackatime on :${PORT}`));
