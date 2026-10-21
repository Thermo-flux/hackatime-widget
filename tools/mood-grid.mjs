// Renders every expression of every theme from ui/js/themes/data.js without the app (no build needed) into a JSON
// pixel grid that mood-sheet.ps1 -Grid turns into a contact sheet. Usage: node mood-grid.mjs <out.json>
import { writeFileSync } from "node:fs";
import { THEMES } from "../ui/js/themes/data.js";
import { EXPRESSIONS } from "../ui/js/mascot.js";

const out = process.argv[2] ?? "mood-grid.json";
const themes = THEMES.filter((t) => t.sprite).map((t) => ({
  id: t.id,
  bg: t.tokens?.["--bg"] ?? "#ffffff",
  frames: Object.fromEntries(EXPRESSIONS.map((e) => {
    const grid = t.sprite.map((row) => row.split(""));
    for (const [x, y, ch] of e === "neutral" ? [] : t.patches[e] ?? []) grid[y][x] = ch;
    return [e, grid.map((row) => row.map((ch) => (ch === "." ? "" : t.legend[ch] ?? "#ff00ff")))];
  })),
}));
writeFileSync(out, JSON.stringify({ expressions: EXPRESSIONS, themes }));
console.log(`${themes.length} themes x ${EXPRESSIONS.length} expressions -> ${out}`);
