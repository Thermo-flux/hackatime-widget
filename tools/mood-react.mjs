// Behaviour test for the mascot mood engine against the running app (mock mode, --remote-debugging-port=$CDP_PORT).
// Checks pokes (happy -> surprised -> dizzy), petting (love), Pomodoro cheer (proud vs excited), errors (grumpy then sad),
// session start/end, streaks, greetings, dozing off / waking, the long-session line and no line repeated twice in a row.
// Usage: node mood-react.mjs   (exit code 1 on failure)
const pages = await (await fetch(`http://127.0.0.1:${process.env.CDP_PORT ?? 9222}/json`)).json();
const ws = new WebSocket(pages.find((p) => p.type === "page").webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = (m) => { const o = JSON.parse(m.data); pending.get(o.id)?.(o); };
const send = (method, params = {}) => new Promise((res) => { const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expr) => {
  const r = (await send("Runtime.evaluate", { expression: `(async () => { const m = await import('/js/mascot.js'); const th = await import('/js/themes.js'); const $ = (i) => document.getElementById(i); ${expr} })()`, returnByValue: true, awaitPromise: true })).result;
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, detail = "") => { console.log(`${ok ? "  ok  " : "  FAIL"} ${name}${detail ? ` (${detail})` : ""}`); if (!ok) failures++; };
const state = () => ev("return { expr: m.currentExpression(), mood: m.currentMood(), say: $('mascot-say').textContent, react: 'react' in $('mascot-bar').dataset };");
const reset = () => ev("m.signal('error', false); m.signal('pomo', null); m.signal('session', false); m.signal('ai', null); m.signal('streak', 0); m.express('neutral', 1); return true;");
/** Hidden windows throttle timers (up to once a minute), so poll for clock-driven reactions. */
const waitFor = async (pred, ms = 75000) => { const end = Date.now() + ms; let st; do { st = await state(); if (pred(st)) return st; await sleep(500); } while (Date.now() < end); return st; };
const linesOf = (theme, kind) => ev(`const t = th.list().find((x) => x.id === ${JSON.stringify(theme)}); return (t.lines?.[${JSON.stringify(kind)}]?.length ? t.lines[${JSON.stringify(kind)}] : m.DEFAULT_LINES[${JSON.stringify(kind)}]);`);

// Stop background polling so mock data can't change signals mid-test.
await ev("for (const p of ['stats', 'sessions', 'pomodoro']) (await import('/js/' + p + '.js')).pause(); return true;");
let lastPet = 0;
for (const theme of ["dog", "phosphor", "classic"]) {
  console.log(`theme ${theme}`);
  await reset();
  await ev(`th.apply(${JSON.stringify(theme)}, { remember: false }); return true;`);
  let s = await state();
  const greet = ["morning", "afternoon", "evening", "night"];
  const greetLines = (await Promise.all(greet.map((k) => linesOf(theme, k)))).flat();
  check("greets on theme switch", greetLines.includes(s.say), s.say);
  await reset(); await sleep(50);

  // Pokes: 1 -> happy/wink, 3 -> surprised, 6 -> dizzy.
  await ev("$('mascot-btn').click(); return true;");
  s = await state();
  check("1 poke: happy or wink", ["happy", "wink"].includes(s.expr) && (await linesOf(theme, "poke")).includes(s.say), `${s.expr} "${s.say}"`);
  await ev("$('mascot-btn').click(); $('mascot-btn').click(); return true;");
  s = await state();
  check("3 pokes: surprised", s.expr === "surprised" && (await linesOf(theme, "pokeMore")).includes(s.say), `${s.expr} "${s.say}"`);
  await ev("for (let i = 0; i < 3; i++) $('mascot-btn').click(); return true;");
  s = await state();
  check("6 pokes: dizzy", s.expr === "dizzy" && s.react && (await linesOf(theme, "dizzy")).includes(s.say), `${s.expr} "${s.say}"`);
  await sleep(3000);
  s = await state();
  check("reaction wears off", !s.react, s.expr);

  // Petting: stroke over the mascot (love has an 8 s cooldown).
  if (Date.now() - lastPet < 8500) await sleep(8500 - (Date.now() - lastPet));
  lastPet = Date.now();
  await ev(`const b = $('mascot-btn'), r = b.getBoundingClientRect();
    for (let i = 0; i < 30; i++) b.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: r.x + 5 + (i % 2) * 30, clientY: r.y + 20 }));
    return true;`);
  s = await state();
  check("petting: love", s.expr === "love" && (await linesOf(theme, "love")).includes(s.say), `${s.expr} "${s.say}"`);

  // Pomodoro: focus round finishes -> proud; a break finishing -> excited.
  await reset();
  await ev("m.signal('pomo', { running: true, phase: 'focus' }); m.signal('pomo', { running: true, phase: 'short_break' }); m.cheer(); return true;");
  s = await state();
  check("focus round done: proud", s.expr === "proud" && (await linesOf(theme, "pomoDone")).includes(s.say), `${s.expr} "${s.say}"`);
  await ev("m.signal('pomo', { running: true, phase: 'focus' }); m.cheer(); return true;");
  s = await state();
  check("break done: excited", s.expr === "excited" && (await linesOf(theme, "cheer")).includes(s.say), `${s.expr} "${s.say}"`);
  await reset();

  // Error: grumpy reaction, then the sad mood face.
  await ev("m.signal('error', true); return true;");
  s = await state();
  check("error: grumpy first", s.expr === "grumpy" && s.mood === "error" && (await linesOf(theme, "error")).includes(s.say), `${s.expr} "${s.say}"`);
  await sleep(2900);
  s = await state();
  check("error: then sad", s.expr === "sad", s.expr);
  await reset();

  // Sessions.
  await ev("m.signal('session', true); return true;");
  s = await state();
  check("session start: excited", s.expr === "excited" && (await linesOf(theme, "sessionStart")).includes(s.say), `${s.expr} "${s.say}"`);
  await ev("m.signal('session', false); return true;");
  s = await state();
  check("session end: proud", s.expr === "proud" && (await linesOf(theme, "sessionEnd")).includes(s.say), `${s.expr} "${s.say}"`);
  await reset();

  // Streak.
  await ev("m.signal('streak', 5); return true;");
  s = await state();
  check("streak: excited", s.expr === "excited" && (await linesOf(theme, "streak")).includes(s.say), `${s.expr} "${s.say}"`);
  await reset();

  // No line twice in a row (same kind, many times).
  const said = [];
  for (let i = 0; i < 30; i++) said.push(await ev("m.react('poke'); return $('mascot-say').textContent;"));
  const repeats = said.filter((l, i) => i && l === said[i - 1]).length;
  check("no line repeated twice in a row", repeats === 0, `${new Set(said).size} distinct of 30`);
  await reset();
}

// Clock-based behaviour: shift Date.now forward instead of waiting.
console.log("time-based");
await ev("window.__realNow = window.__realNow ?? Date.now; return true;");
await ev("m.signal('session', true); return true;");
await ev("const real = window.__realNow; Date.now = () => real() + 61 * 60 * 1000; return true;");
let s = await waitFor((st) => st.expr === "proud");
check("long session: proud", s.expr === "proud" && (await linesOf("classic", "longSession")).includes(s.say), `${s.expr} "${s.say}"`);
await ev("m.signal('session', false); return true;");
await reset();
await ev("const real = window.__realNow; Date.now = () => real() + 2 * 60 * 60 * 1000; return true;");
s = await waitFor((st) => st.expr === "sleepy");
check("idle a long time: dozes off", s.expr === "sleepy" && (await linesOf("classic", "drowsy")).includes(s.say), `${s.expr} "${s.say}"`);
await ev("document.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true })); return true;");
s = await state();
check("input wakes it up", s.expr === "surprised" && (await linesOf("classic", "wake")).includes(s.say), `${s.expr} "${s.say}"`);
await ev("Date.now = window.__realNow; return true;");
await reset();

await ev("for (const p of ['stats', 'sessions', 'pomodoro']) (await import('/js/' + p + '.js')).resume(); return true;");
ws.close();
console.log(failures ? `${failures} failure(s)` : "all mascot behaviour checks passed");
process.exit(failures ? 1 : 0);
