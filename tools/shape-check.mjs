// Shape checker: for every theme x state, checks that nothing is clipped by the theme's silhouette.
// clip-path / mask also clip hit-testing, so we sample points with document.elementFromPoint:
//  - interactive elements (buttons, inputs): centre + 4 corners (inset) must hit the element or a descendant;
//  - text (every text line box) and content boxes (tiles, bubble, bars, ring...): must hit something inside .card;
//  - panel content must not run into the footer (mascot bar), and nothing may be cut by an overflow box
//    (items wholly outside one, like languages that don't fit, are intentionally hidden and skipped).
// States: stats, stats with 4 languages, sessions, live session, pomodoro (idle, running, paused, settings panel,
// settings with an error note), theme picker.
// Usage: node shape-check.mjs [--size WxH | --native] [theme ...]
//   default size is the real window (320x436, via CDP device-metrics override); --native uses the hidden
//   window's own size (about 314x407, a tighter stress test). App: mock mode, --remote-debugging-port=9222.
// Exits 1 on any failure.

const args = process.argv.slice(2);
let size = "320x436";
const only = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--size") size = args[++i];
  else if (args[i] === "--native") size = null;
  else only.push(args[i]);
}

const pages = await (await fetch(`http://127.0.0.1:${process.env.CDP_PORT ?? 9222}/json`)).json();
const ws = new WebSocket(pages.find((p) => p.type === "page").webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = (m) => { const o = JSON.parse(m.data); pending.get(o.id)?.(o); };
const send = (method, params = {}) => new Promise((res) => { const n = ++id; pending.set(n, res); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expr) => {
  const r = (await send("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true })).result;
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

if (size) {
  const [width, height] = size.split("x").map(Number);
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await sleep(300);
}
const viewport = await ev("innerWidth + 'x' + innerHeight");

// Runs in the page. `scopeSel` limits the check to one subtree (the picker covers everything else when open).
const CHECK = (scopeSel) => `(() => {
  const card = document.querySelector('.card');
  const scopeSel = ${JSON.stringify(scopeSel)};
  const scope = document.querySelector(scopeSel);
  const foot = document.querySelector('.mascot-bar');
  const footTop = foot.getBoundingClientRect().top;
  const fails = [];
  const name = (e) => !e ? 'null' : (e.id ? '#' + e.id : e.tagName.toLowerCase() + (typeof e.className === 'string' && e.className ? '.' + e.className.trim().split(/\\s+/)[0] : ''));
  const shown = (e) => e.getClientRects().length > 0 && e.checkVisibility({ visibilityProperty: true });
  // Nearest ancestor (below .card) that clips its overflow. A scroll box (picker grid) keeps everything reachable.
  const clipBox = (e) => {
    for (let p = e.parentElement; p && p !== card; p = p.parentElement) {
      const s = getComputedStyle(p);
      if (s.overflowX !== 'visible' || s.overflowY !== 'visible') {
        const r = p.getBoundingClientRect();
        return { top: r.top, bottom: r.bottom, left: r.left, right: r.right, scroll: /auto|scroll/.test(s.overflowX + s.overflowY) };
      }
    }
    return null;
  };
  const pts = (r, inset) => {
    const [a, b, c, d] = inset;
    return [[r.left + r.width / 2, r.top + r.height / 2, 'centre'], [r.left + a, r.top + a, 'tl'], [r.right - b, r.top + b, 'tr'], [r.right - c, r.bottom - c, 'br'], [r.left + d, r.bottom - d, 'bl']];
  };
  // Corner samples step inside rounded or clipped corners of the element itself (hit-testing honours them).
  const cornerInset = (e, r) => {
    const cs = getComputedStyle(e);
    const lim = Math.min(r.width, r.height) / 2;
    const own = cs.clipPath !== 'none' ? 6 : 0;
    return ['borderTopLeftRadius', 'borderTopRightRadius', 'borderBottomRightRadius', 'borderBottomLeftRadius']
      .map((k) => Math.max(own, 1 + 0.3 * Math.min(parseFloat(cs[k]) || 0, lim)));
  };
  const test = (owner, r, inset, ok, kind) => {
    const cb = clipBox(owner);
    if (cb) {
      const outside = r.bottom <= cb.top + 0.5 || r.top >= cb.bottom - 0.5 || r.right <= cb.left + 0.5 || r.left >= cb.right - 0.5;
      if (outside) return; // wholly hidden on purpose (a language that doesn't fit) or scrolled away
      // Glyph boxes of tall fonts poke a little out of their line box; allow that for text.
      const tol = kind === 'text' ? Math.max(0.5, r.height * 0.3) : 0.5;
      const cut = r.top < cb.top - tol || r.bottom > cb.bottom + tol || r.left < cb.left - tol || r.right > cb.right + tol;
      if (cut && !cb.scroll) { fails.push({ kind, el: name(owner), where: 'cut by overflow', x: Math.round(r.left), y: Math.round(r.top), hit: '-' }); return; }    }
    if (!foot.contains(owner) && scopeSel === '.card' && r.bottom > footTop + 0.5) {
      fails.push({ kind, el: name(owner), where: 'runs into footer', x: Math.round(r.left), y: Math.round(r.bottom), hit: '-' }); return;
    }
    for (const [x, y, where] of pts(r, inset)) {
      if (cb && cb.scroll && (y < cb.top || y > cb.bottom || x < cb.left || x > cb.right)) continue;
      const hit = document.elementFromPoint(x, y);
      if (!ok(hit)) { fails.push({ kind, el: name(owner), where, x: Math.round(x), y: Math.round(y), hit: name(hit) }); return; }
    }
  };
  const inCard = (h) => !!h && card.contains(h);

  const interactive = [...scope.querySelectorAll('button, input, select, textarea, a[href]')].filter(shown);
  for (const e of interactive) {
    const r = e.getBoundingClientRect();
    // A squeezed button whose label spills out (e.g. tabs in a too-narrow header).
    if (e.tagName === 'BUTTON' && e.textContent.trim() && e.scrollWidth > e.clientWidth + 1) fails.push({ kind: 'interactive', el: name(e), where: 'label overflows', x: Math.round(r.left), y: Math.round(r.top), hit: '-' });
    test(e, r, cornerInset(e, r), (h) => !!h && (h === e || e.contains(h)), 'interactive');
  }

  const boxes = [...scope.querySelectorAll('.seg, .tile, .mascot-say, .phase, .stat-bar, .chart, .ring, .ring__hint, .sessions, .chips, .stepper, .pomo-settings__title, .lang__bar, .badge, .notice, .field__input, .picker__item, .legend, .live-head')]
    .filter(shown).filter((e) => !interactive.includes(e) && !e.closest('.picker') === (scopeSel === '.card'));
  for (const e of boxes) { const r = e.getBoundingClientRect(); test(e, r, cornerInset(e, r), inCard, 'box'); }

  const walker = document.createTreeWalker(scope, NodeFilter.SHOW_TEXT, { acceptNode: (n) => n.nodeValue.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT });
  for (let n; (n = walker.nextNode()); ) {
    const p = n.parentElement;
    if (!shown(p) || getComputedStyle(p).opacity === '0' || p.closest('.sr-only')) continue;
    const range = document.createRange(); range.selectNodeContents(n);
    for (const r of range.getClientRects()) if (r.width >= 1 && r.height >= 1) test(p, r, [1, 1, 1, 1], inCard, 'text');
  }
  return { fails, counted: interactive.length + boxes.length };
})()`;

const ids = await ev("import('/js/themes.js').then(m => m.list().map(t => t.id))");
const themes = only.length ? ids.filter((t) => only.includes(t)) : ids;
const click = (elId) => ev(`document.getElementById(${JSON.stringify(elId)}).click()`);
const hidden = (elId) => ev(`document.getElementById(${JSON.stringify(elId)}).hidden`);

const rows = [];
let total = 0;
for (const theme of themes) {
  await ev(`import('/js/themes.js').then(m => m.apply(${JSON.stringify(theme)}, { remember: false }))`);
  await sleep(150);
  const states = [];
  for (const tab of ["stats", "sessions", "pomodoro"]) {
    await click(`tab-${tab}`);
    await sleep(350);
    states.push([tab, await ev(CHECK(".card"))]);
    if (tab === "stats") {
      // Real accounts show up to 4 languages; the mock has 2. Clone them to check the fuller list.
      await ev("(() => { const ul = document.getElementById('languages'); for (const li of [...ul.children].slice(0, 2)) { const c = li.cloneNode(true); c.dataset.shapeCheck = '1'; ul.append(c); } })()");
      await sleep(50);
      states.push(["stats+4langs", await ev(CHECK(".card"))]);
      await ev("document.querySelectorAll('[data-shape-check]').forEach((e) => e.remove())");
      // The API-key form and the error view replace the stats for users without a key / when offline.
      for (const view of ["stats-key", "stats-error"]) {
        await ev(`(() => { const ok = document.getElementById('stats-ok'); ok.dataset.wasHidden = ok.hidden; ok.hidden = true;
          const v = document.getElementById(${JSON.stringify(view)}); v.dataset.wasHidden = v.hidden; v.hidden = false;
          const m = document.getElementById('error-msg'); if (!m.textContent) { m.textContent = "Couldn't reach Hackatime. Check your connection."; m.dataset.shapeCheck = '1'; } })()`);
        await sleep(50);
        states.push([view, await ev(CHECK(".card"))]);
        await ev(`(() => { for (const id of ['stats-ok', ${JSON.stringify(view)}]) { const e = document.getElementById(id); e.hidden = e.dataset.wasHidden === 'true'; delete e.dataset.wasHidden; }
          const m = document.getElementById('error-msg'); if (m.dataset.shapeCheck) { m.textContent = ''; delete m.dataset.shapeCheck; } })()`);
      }
    }
    if (tab === "sessions") {
      if (await ev("document.getElementById('sess-start').disabled")) { states.push(["sessions-live", { fails: [{ kind: "setup", el: "#sess-start", where: "-", hit: "disabled" }], counted: 0 }]); continue; }
      await click("sess-start");
      for (let i = 0; i < 20 && (await hidden("sess-live")); i++) await sleep(150);
      await sleep(300);
      states.push(["sessions-live", await ev(CHECK(".card"))]);
      await click("sess-stop");
      for (let i = 0; i < 20 && (await hidden("sess-idle")); i++) await sleep(150);
    }
    if (tab === "pomodoro") {
      // The ring is the start/pause button; its label and pill change per state. Then the settings panel,
      // opened on a started timer (longest note text), with an error message as the tallest variant.
      const ringState = () => ev("document.getElementById('pomo-toggle').dataset.state");
      await click("pomo-toggle");
      for (let i = 0; i < 20 && (await ringState()) !== "running"; i++) await sleep(100);
      await sleep(1100);
      states.push(["pomo-running", await ev(CHECK(".card"))]);
      await click("pomo-toggle");
      for (let i = 0; i < 20 && (await ringState()) !== "paused"; i++) await sleep(100);
      states.push(["pomo-paused", await ev(CHECK(".card"))]);
      await click("pomo-set");
      await sleep(150);
      states.push(["pomo-settings", await ev(CHECK(".card"))]);
      await ev("(() => { const n = document.getElementById('pomo-settings-note'); n.textContent = \"Couldn't save: the config folder is not writable.\"; n.classList.add('notice--error'); })()");
      states.push(["pomo-settings-err", await ev(CHECK(".card"))]);
      await click("pomo-cancel");
      await ev("window.__TAURI__.core.invoke('pomodoro_reset')");
      await sleep(1100); // let the poll render the reset timer
    }
  }
  await click("tab-stats");
  await click("theme-btn");
  await sleep(250);
  states.push(["picker", await ev(CHECK("#theme-picker"))]);
  await click("picker-close");
  await sleep(100);

  let themeFails = 0;
  for (const [state, res] of states) {
    total += res.counted;
    for (const f of res.fails) { rows.push({ theme, state, ...f }); themeFails++; }
  }
  console.log(`${theme.padEnd(10)} ${themeFails ? `FAIL (${themeFails})` : "ok"}  [${states.map(([s, r]) => `${s}:${r.counted}`).join(" ")}]`);
}
await ev("import('/js/themes.js').then(m => m.apply('classic', { remember: false }))");
if (size) await send("Emulation.clearDeviceMetricsOverride");
ws.close();

console.log(`viewport ${viewport}`);
if (rows.length) {
  console.log("\nFailures:");
  console.table(rows.map(({ theme, state, kind, el, where, x, y, hit }) => ({ theme, state, kind, el, where, at: x === undefined ? "" : `${x},${y}`, hit })));
  process.exit(1);
}
console.log(`All ${themes.length} themes clean (${total} element checks plus every text line).`);
