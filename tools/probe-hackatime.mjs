// Prints what the live Hackatime API calls its categories, so the AI category spelling the widget filters on
// (`AI_CATEGORY` in src-tauri/src/api.rs, `is_ai` in stats.rs) can be checked against real data.
//   HACKATIME_KEY=<your key> node tools/probe-hackatime.mjs        (optional: HACKATIME_ORIGIN=https://host)
// The key is only sent as a Bearer token to that origin and is never printed.
const key = process.env.HACKATIME_KEY;
const origin = (process.env.HACKATIME_ORIGIN ?? "https://hackatime.hackclub.com").replace(/\/$/, "");
if (!key) { console.error("Set HACKATIME_KEY first."); process.exit(2); }

const get = async (path, params) => {
  const res = await fetch(`${origin}${path}?${new URLSearchParams(params)}`, { headers: { Authorization: `Bearer ${key}` } });
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`);
  return res.json();
};

const now = new Date();
const weekAgo = new Date(now.getTime() - 7 * 86_400_000);
console.log(`Stats categories, ${weekAgo.toISOString().slice(0, 10)} to ${now.toISOString().slice(0, 10)}:`);
const stats = await get("/api/v1/users/my/stats", { start_date: weekAgo.toISOString(), end_date: now.toISOString(), features: "categories" });
const cats = stats?.data?.categories ?? [];
if (!cats.length) console.log("  (none returned)");
for (const c of cats) console.log(`  ${JSON.stringify(c.name)}  ${Math.round(c.total_seconds ?? 0)} s`);

console.log("\nHeartbeat categories, last hour:");
const hb = await get("/api/v1/my/heartbeats", { start_time: new Date(now.getTime() - 3_600_000).toISOString(), end_time: now.toISOString() });
const seen = new Map();
for (const h of hb.heartbeats ?? []) seen.set(h.category ?? "(none)", (seen.get(h.category ?? "(none)") ?? 0) + 1);
if (!seen.size) console.log("  (no heartbeats in the last hour)");
for (const [name, n] of seen) console.log(`  ${JSON.stringify(name)}  ${n} heartbeats`);

const filtered = await get("/api/v1/users/my/stats", { start_date: weekAgo.toISOString(), end_date: now.toISOString(), total_seconds: "true", filter_by_category: "ai coding" });
console.log(`\nfilter_by_category="ai coding" total: ${Math.round(filtered.total_seconds ?? 0)} s`);
