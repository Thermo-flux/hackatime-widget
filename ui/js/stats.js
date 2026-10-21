import { $, el } from "./lib/dom.js";
import { short as fmt } from "./lib/format.js";
import { invoke } from "./lib/tauri.js";
import { signal } from "./mascot.js";

// UI states: loading | ok | needs-key | error
const VIEWS = { ok: "stats-ok", "needs-key": "stats-key", error: "stats-error" };

let range = "today";
let timer = null;

function show(state) {
  for (const [name, id] of Object.entries(VIEWS)) $(id).hidden = name !== state;
  $("panel-stats").setAttribute("aria-busy", String(state === "loading"));
}

function renderLanguages(languages) {
  const max = Math.max(1, ...languages.map((l) => l.seconds));
  $("languages").replaceChildren(
    ...languages.slice(0, 4).map((l) => {
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
  $("total").textContent = fmt(total);
  $("human").textContent = fmt(s.human_seconds);
  $("ai").textContent = known ? fmt(s.ai_seconds) : "–";
  $("human-pct").textContent = known ? `${human.toFixed(0)}%` : "–";
  $("ai-pct").textContent = known ? `${ai.toFixed(0)}%` : "–";
  $("bar-human").style.width = `${human}%`;
  $("bar-ai").style.width = `${ai}%`;
  $("stat-bar").setAttribute(
    "aria-label",
    total <= 0
      ? "No coding time recorded"
      : known
        ? `${human.toFixed(0)}% human, ${ai.toFixed(0)}% AI`
        : "AI share unavailable",
  );
  renderLanguages(s.languages);
  $("mock-badge").hidden = !s.mock;
  signal("ai", known && total > 0 ? ai : null);
}

export async function refresh() {
  $("panel-stats").setAttribute("aria-busy", "true"); // keep old values on screen, dimmed
  try {
    render(await invoke("get_stats", { range }));
    show("ok");
    signal("error", false);
  } catch (e) {
    signal("error", e.code === "network" || e.code === "server");
    if (e.code === "no_key" || e.code === "unauthorized") {
      $("key-msg").textContent =
        e.code === "unauthorized" ? "Hackatime rejected that key. Check it and try again." : "Connect your Hackatime account to see your stats.";
      show("needs-key");
    } else {
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
  timer = setInterval(refresh, Math.max(30, cfg.refresh_secs) * 1000);
}

/** Stop background refreshing while the window is hidden. */
export function pause() { clearInterval(timer); timer = null; }
export function resume() { refresh(); schedule(); }

export function init() {
  $("ranges").addEventListener("click", (e) => {
    const btn = e.target.closest("[data-range]");
    if (!btn) return;
    range = btn.dataset.range;
    document.querySelectorAll("#ranges .seg__btn").forEach((b) => b.setAttribute("aria-pressed", String(b === btn)));
    refresh();
  });

  $("retry").addEventListener("click", refresh);

  $("stats-key").addEventListener("submit", async (e) => {
    e.preventDefault();
    const save = $("save");
    save.disabled = true;
    try {
      const cfg = await invoke("get_config");
      cfg.api_key = $("key").value.trim();
      await invoke("set_config", { cfg });
      $("key").value = "";
      await refresh();
    } catch (err) {
      $("key-msg").textContent = `Could not save: ${err.message ?? err}`;
    } finally {
      save.disabled = false;
    }
  });

  resume();
}
