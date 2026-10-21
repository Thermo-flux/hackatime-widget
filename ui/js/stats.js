import { $, el } from "./lib/dom.js";
import { ago, clock, short as fmt } from "./lib/format.js";
import { invoke } from "./lib/tauri.js";
import { react, signal } from "./mascot.js";

const KEY_SHAPE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const SETUP_URL = "https://hackatime.hackclub.com/my/wakatime_setup";

// UI states: loading | ok | needs-key | error
const VIEWS = { ok: "stats-ok", "needs-key": "stats-key", error: "stats-error" };

let range = "today";
let timer = null;

function show(state) {
  for (const [name, id] of Object.entries(VIEWS)) $(id).hidden = name !== state;
  $("panel-stats").setAttribute("aria-busy", String(state === "loading"));
}

const LIST_KEY = "hackatime-widget.stats-list";
const LIST_ROWS = 3; // always three, so every theme shows the same amount
let listKind = (() => {
  try { return localStorage.getItem(LIST_KEY) === "projects" ? "projects" : "languages"; } catch { return "languages"; }
})();
let lastStats = null;

function renderList() {
  const items = lastStats?.[listKind] ?? [];
  document.querySelectorAll("#stats-ok .list-toggle__btn").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.list === listKind)));
  if (items.length === 0) {
    $("languages").replaceChildren(el("li", { class: "lang lang--none", text: `No ${listKind} yet` }));
    return;
  }
  const max = Math.max(1, ...items.map((l) => l.seconds));
  $("languages").replaceChildren(
    ...items.slice(0, LIST_ROWS).map((l) => {
      const fill = el("span", { class: "lang__fill" });
      fill.style.width = `${(l.seconds / max) * 100}%`;
      return el(
        "li",
        { class: "lang" },
        el("span", { class: "lang__name", text: l.name }),
        el("span", { class: "lang__time tabular", text: fmt(l.seconds) }),
        el("span", { class: "lang__bar" }, fill),
      );
    }),
  );
}

function render(s) {
  const total = s.total_seconds;
  const known = s.ai_available !== false; // false: the AI figure couldn't be fetched
  const ai = known ? (s.ai_percent ?? 0) : 0;
  const human = total > 0 ? 100 - ai : 0;
  const unknown = !known && total > 0; // can't split the bar without the AI figure, so don't claim 100% human
  $("total").textContent = fmt(total);
  const zero = total <= 0; // nothing to chart: say so once instead of showing empty parts
  $("zero-note").hidden = !zero;
  $("stat-bar").hidden = zero;
  document.querySelector("#stats-ok .langs").hidden = zero;
  $("stat-bar").classList.toggle("stat-bar--unknown", unknown);
  $("legend-split").hidden = unknown || zero;
  $("legend-unknown").hidden = !unknown;
  $("legend-unknown").textContent = `${fmt(total)} coded · AI share unavailable`;
  $("human").textContent = fmt(s.human_seconds);
  $("ai").textContent = known ? fmt(s.ai_seconds) : "–";
  $("bar-human").style.width = `${unknown ? 0 : human}%`;
  $("bar-ai").style.width = `${unknown ? 0 : ai}%`;
  $("stat-bar").setAttribute(
    "aria-label",
    total <= 0
      ? "No coding time recorded"
      : known
        ? `${human.toFixed(0)}% human, ${ai.toFixed(0)}% AI`
        : "AI share unavailable",
  );
  lastStats = s;
  renderList();
  $("mock-badge").hidden = !s.mock;
  signal("ai", known && total > 0 ? ai : null);
  signal("empty", total === 0);
}

const BACKOFF_SECS = [30, 60, 120, 300]; // then the normal interval takes over
let retryTimer = null;
let tickTimer = null;
let failures = 0;
let paused = false; // the window is hidden: no timers may be (re)armed
let shown = null; // { range, fetchedAt } for the data on screen, if any

const nowSecs = () => Math.floor(Date.now() / 1000);

/** Dim the data; `banner` also shows the "Offline · showing 14:02" strip. */
function setStale(on, banner = on) {
  $("stats-ok").classList.toggle("is-stale", on);
  $("stale-banner").hidden = !banner || !shown;
  if (banner && shown) {
    $("stale-text").textContent = `${offlineKind} · showing ${clock(shown.fetchedAt)}`;
  }
}
let offlineKind = "Offline";

function updateAgo() {
  $("updated").textContent = shown ? `Updated ${ago(nowSecs() - shown.fetchedAt)}` : "";
}

function showData(stats, fetchedAt, forRange) {
  render(stats);
  shown = { range: forRange, fetchedAt };
  updateAgo();
  show("ok");
}

/** Paint the saved copy of `forRange` right away, if there is one. Returns whether it did. */
async function showCached(forRange) {
  try {
    const c = await invoke("get_cached_stats", { range: forRange });
    if (!c || forRange !== range) return false;
    showData(c.stats, c.fetched_at, forRange);
    setStale(c.stale, false); // an old copy stays dimmed until fresh data replaces it
    return true;
  } catch {
    return false;
  }
}

function scheduleRetry() {
  clearTimeout(retryTimer);
  const delay = BACKOFF_SECS[failures - 1];
  if (delay && !paused) retryTimer = setTimeout(refresh, delay * 1000);
}

export async function refresh() {
  clearTimeout(retryTimer);
  const asked = range;
  $("panel-stats").setAttribute("aria-busy", "true"); // keep old values on screen, dimmed
  try {
    const stats = await invoke("get_stats", { range: asked });
    if (asked !== range) return; // the user switched ranges meanwhile; that refresh will paint
    failures = 0;
    showData(stats, nowSecs(), asked);
    setStale(false);
    signal("no_key", false);
    signal("offline", false);
    signal("error", false);
  } catch (e) {
    if (asked !== range) return;
    const needsKey = e.code === "no_key" || e.code === "unauthorized";
    signal("no_key", needsKey);
    signal("offline", e.code === "network");
    signal("error", e.code === "server");
    if (needsKey) {
      failures = 0;
      shown = null;
      signal("empty", false);
      $("key-msg").textContent =
        e.code === "unauthorized" ? "Hackatime rejected that key. Check it and try again." : "Connect your Hackatime account to see your stats.";
      show("needs-key");
      return;
    }
    failures += 1;
    scheduleRetry();
    // Keep whatever we have for this range: what's on screen, else the saved copy.
    let haveData = shown && shown.range === asked;
    if (!haveData) {
      haveData = await showCached(asked);
      if (asked !== range) return; // switched ranges while reading the saved copy
    }
    if (haveData) {
      offlineKind = e.code === "network" ? "Offline" : "Can't refresh";
      show("ok");
      setStale(true);
    } else {
      shown = null;
      const detail = e.message ?? String(e);
      $("error-msg").textContent =
        e.code === "network" ? "Can't reach Hackatime. Check your connection and retry." : detail;
      $("error-msg").title = detail;
      show("error");
    }
  }
}

async function schedule() {
  clearInterval(timer);
  const cfg = await invoke("get_config");
  if (paused) return;
  timer = setInterval(refresh, Math.max(30, cfg.refresh_secs) * 1000);
}

/** Stop background refreshing while the window is hidden. */
export function pause() { paused = true; clearInterval(timer); clearInterval(tickTimer); clearTimeout(retryTimer); timer = null; }
export function resume() {
  paused = false;
  failures = 0;
  showCached(range).then(() => refresh());
  schedule();
  clearInterval(tickTimer);
  tickTimer = setInterval(updateAgo, 30_000);
}

export function init() {
  $("ranges").addEventListener("click", (e) => {
    const btn = e.target.closest("[data-range]");
    if (!btn) return;
    range = btn.dataset.range;
    document.querySelectorAll("#ranges .seg__btn").forEach((b) => b.setAttribute("aria-pressed", String(b === btn)));
    failures = 0;
    showCached(range).then(() => refresh());
  });

  const retryNow = () => { failures = 0; refresh(); };
  $("stale-retry").addEventListener("click", retryNow);
  $("retry").addEventListener("click", retryNow);

  $("stats-ok").querySelector(".list-toggle").addEventListener("click", (e) => {
    const btn = e.target.closest("[data-list]");
    if (!btn || btn.dataset.list === listKind) return;
    listKind = btn.dataset.list;
    try { localStorage.setItem(LIST_KEY, listKind); } catch { /* private mode etc.: just don't remember */ }
    renderList();
  });

  const keyError = (on) => {
    $("key-error").hidden = !on;
    $("key-hint").hidden = on; // the error takes the hint's place, so the form never grows
    $("key").setAttribute("aria-invalid", String(on));
    if (on) $("key").setAttribute("aria-describedby", "key-error");
    else $("key").removeAttribute("aria-describedby");
  };
  $("key").addEventListener("input", () => {
    const trimmed = $("key").value.trim(); // pasted keys often carry a newline or spaces
    if (trimmed !== $("key").value) $("key").value = trimmed;
    keyError(false);
  });
  $("get-key").addEventListener("click", () => {
    invoke("plugin:opener|open_url", { url: SETUP_URL }).catch(() => {
      $("key-msg").textContent = `Open ${SETUP_URL.replace("https://", "")} in your browser to find your key.`;
    });
  });

  $("stats-key").addEventListener("submit", async (e) => {
    e.preventDefault();
    const key = $("key").value.trim();
    if (!KEY_SHAPE.test(key)) { // rejected here, without calling the server
      keyError(true);
      $("key").focus();
      return;
    }
    const save = $("save");
    save.disabled = true;
    try {
      await invoke("set_api_key", { key });
      $("key").value = "";
      await refresh();
      if (!$("stats-ok").hidden) react("connected");
    } catch (err) {
      if (err.code === "invalid_key") keyError(true);
      else $("key-msg").textContent = `Could not save: ${err.message ?? err}`;
    } finally {
      save.disabled = false;
    }
  });

  resume();
}
