// The one-time tip about the compact-mode hotkey: the first time the widget collapses, the mascot says how to do it
// from the keyboard, and the collapse waits a moment so the line can be read.
import { invoke } from "./lib/tauri.js";
import { tell } from "./mascot.js";

const KEY = "hackatime-widget.compact-hint";
export const READ_MS = 1800;

const NAMES = {
  mac: { commandorcontrol: "Cmd", cmdorctrl: "Cmd", command: "Cmd", cmd: "Cmd", super: "Cmd", control: "Ctrl", ctrl: "Ctrl", alt: "Option", option: "Option", shift: "Shift" },
  other: { commandorcontrol: "Ctrl", cmdorctrl: "Ctrl", command: "Ctrl", cmd: "Ctrl", super: "Win", control: "Ctrl", ctrl: "Ctrl", alt: "Alt", option: "Alt", shift: "Shift" },
};

/** "CommandOrControl+Alt+Shift+H" as people say it: "Ctrl+Alt+Shift+H", or "Cmd+Option+Shift+H" on a Mac. */
export function label(hotkey, mac = /Mac/i.test(navigator.platform || navigator.userAgent)) {
  const names = NAMES[mac ? "mac" : "other"];
  return hotkey.split("+").map((part) => names[part.trim().toLowerCase()] ?? part.trim().toUpperCase()).join("+");
}

const seen = () => { try { return localStorage.getItem(KEY) === "1"; } catch { return true; } }; // can't remember: don't nag
const remember = () => { try { localStorage.setItem(KEY, "1"); } catch { /* nothing to do */ } };

/** Resolves once it is fine to collapse: immediately, or after the tip has had time to be read (first time only). */
export async function beforeCollapse() {
  if (seen()) return;
  let hotkey;
  try { hotkey = (await invoke("get_config")).compact_hotkey; } catch { return; }
  if (!hotkey) return;
  if (!tell(`Press ${label(hotkey)} to shrink me`)) return; // a quiet mascot says nothing, so the tip stays unseen
  remember();
  await new Promise((r) => setTimeout(r, READ_MS));
}
