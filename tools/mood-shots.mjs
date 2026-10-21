// Mascot checks against the running app (mock mode, --remote-debugging-port=$CDP_PORT, default 9222).
// Usage: node mood-shots.mjs <outDir> [mode]
//   moods        (default) forces each theme's mascot into several moods and saves a close-up of the bar for each.
//   expressions  saves every expression of every theme (<theme>__<expr>.png); stitch with
//                mood-sheet.ps1 -Dir <outDir> -Moods (list printed at the end).
//   lines        puts every speech line of every theme into the bubble, fails (exit 1) if one needs more than
//                two lines or overflows, and saves the bar with each theme's tallest/widest line (<theme>__longest.png).
import { writeFileSync, mkdirSync } from "node:fs";

const out = process.argv[2] ?? ".";
const mode = process.argv[3] ?? "moods";
mkdirSync(out, { recursive: true });
const pages = await (await fetch(`http://127.0.0.1:${process.env.CDP_PORT ?? 9222}/json`)).json();
const ws = new WebSocket(pages.find((p) => p.type === "page").webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = (m) => { const o = JSON.parse(m.data); pending.get(o.id)?.(o); };
const send = (method, params = {}) => new Promise((res) => { const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expr) => {
  const r = (await send("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true })).result;
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const rect = (sel) => ev(`(() => { const b = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return { x: b.x, y: b.y, w: b.width, h: b.height }; })()`);
const shoot = async (file, clip) => {
  const shot = await send("Page.captureScreenshot", { format: "png", clip: { ...clip, scale: clip.scale ?? 1.5 } });
  writeFileSync(`${out}/${file}`, Buffer.from(shot.result.data, "base64"));
};
const resetSignals = "m.signal('error', false); m.signal('pomo', null); m.signal('session', false); m.signal('ai', null); m.signal('streak', 0);";

const ids = await ev("import('/js/themes.js').then(m => m.list().map(t => t.id))");
// Stop background polling (mock stats/sessions/pomodoro) so it can't trigger reactions mid-shot; resumed at the end.
const polling = (fn) => ev(`Promise.all(['stats', 'sessions', 'pomodoro'].map((p) => import('/js/' + p + '.js').then((m) => m.${fn}())))`);
await polling("pause");
let failed = false;

if (mode === "moods") {
  const MOODS = {
    idle: "{}",
    ai: "{ ai: 80 }",
    session: "{ session: true }",
    break: "{ pomo: { running: true, phase: 'short_break' } }",
    error: "{ error: true }",
  };
  for (const t of ids) {
    await ev(`import('/js/themes.js').then(m => m.apply(${JSON.stringify(t)}, { remember: false }))`);
    for (const [mood, signals] of Object.entries(MOODS)) {
      await ev(`import('/js/mascot.js').then(m => { ${resetSignals} const s = ${signals}; for (const [k, v] of Object.entries(s)) m.signal(k, v); m.express(m.currentMood() === 'error' ? 'sad' : 'neutral', 1); })`);
      await sleep(180);
      const r = await rect("#mascot-bar");
      await shoot(`${t}__${mood}.png`, { x: r.x, y: r.y - 4, width: 130, height: r.h + 8 });
    }
  }
} else if (mode === "expressions") {
  // Freeze motion so every frame is a clean, unshifted sprite (CSSOM, because the CSP blocks injected <style>).
  await ev(`(() => { const c = document.getElementById('mascot').style; for (const [k, v] of [['animation', 'none'], ['transform', 'none'], ['filter', 'none'], ['opacity', '1']]) c.setProperty(k, v, 'important'); })()`);
  const exprs = await ev("import('/js/mascot.js').then(m => m.EXPRESSIONS)");
  for (const t of ids) {
    await ev(`import('/js/themes.js').then(m => m.apply(${JSON.stringify(t)}, { remember: false }))`);
    await ev(`import('/js/mascot.js').then(m => { ${resetSignals} })`);
    for (const e of exprs) {
      // Background polling (mock sessions/stats) can trigger reactions, so set the expression and check it right before each shot.
      let shown;
      for (let tries = 0; tries < 5 && shown !== e; tries++) {
        shown = await ev(`import('/js/mascot.js').then(m => { m.express(${JSON.stringify(e)}, 60000); return m.currentExpression(); })`);
        await sleep(60);
        shown = await ev("import('/js/mascot.js').then(m => m.currentExpression())");
      }
      if (shown !== e) { console.log(`  ERROR ${t}: asked for ${e}, engine shows ${shown}`); failed = true; }
      const r = await rect("#mascot");
      await shoot(`${t}__${e}.png`, { x: r.x - 4, y: r.y - 4, width: r.w + 8, height: r.h + 8, scale: 2 });
    }
    await ev(`import('/js/mascot.js').then(m => m.express('neutral', 1))`);
  }
  await ev("document.getElementById('mascot').removeAttribute('style')");
  console.log(`saved ${ids.length} x ${exprs.length}; sheet: mood-sheet.ps1 -Dir ${out} -Moods ${exprs.join(",")}`);
} else if (mode === "lines") {
  for (const t of ids) {
    await ev(`import('/js/themes.js').then(m => m.apply(${JSON.stringify(t)}, { remember: false }))`);
    await sleep(120); // fonts
    const res = await ev(`import('/js/mascot.js').then((m) => {
      const t = ${JSON.stringify(t)};
      return import('/js/themes.js').then(({ list }) => {
        const theme = list().find((x) => x.id === t);
        const b = document.getElementById('mascot-say'), bar = document.getElementById('mascot-bar');
        const cs = getComputedStyle(b), lh = parseFloat(cs.lineHeight), pad = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);
        const barH = bar.getBoundingClientRect().height;
        const bad = []; let worst = null, worstScore = -1, count = 0;
        for (const kind of m.LINE_KINDS) {
          const lines = theme.lines?.[kind]?.length ? theme.lines[kind] : m.DEFAULT_LINES[kind];
          for (const line of lines) {
            count++;
            b.textContent = line;
            const rows = Math.round((b.clientHeight - pad) / lh);
            const over = b.scrollWidth > b.clientWidth + 1;
            const score = rows * 1000 + b.scrollWidth;
            if (score > worstScore) { worstScore = score; worst = line; }
            if (rows > 2 || over) bad.push(kind + ': "' + line + '" (' + rows + ' rows' + (over ? ', overflows' : '') + ')');
          }
        }
        b.textContent = worst; b.classList.remove('is-faded');
        return { count, bad, worst, width: b.clientWidth, barH, newBarH: bar.getBoundingClientRect().height };
      });
    })`);
    console.log(`${t}: ${res.count} lines, bubble ${res.width}px wide, tallest "${res.worst}" (bar ${res.barH}px -> ${res.newBarH}px)`);
    for (const b of res.bad) { console.log("  ERROR " + b); failed = true; }
    const r = await rect("#mascot-bar");
    await shoot(`${t}__longest.png`, { x: r.x, y: r.y - 4, width: r.w, height: r.h + 8 });
  }
} else {
  console.log(`unknown mode ${mode}`);
  failed = true;
}
await ev("import('/js/themes.js').then(m => m.apply('classic', { remember: false }))");
await polling("resume");
ws.close();
process.exit(failed ? 1 : 0);
