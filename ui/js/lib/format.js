export function mmss(secs) {
  return `${String(Math.floor(secs / 60)).padStart(2, "0")}:${String(secs % 60).padStart(2, "0")}`;
}

export function hms(secs) {
  const s = Math.max(0, Math.floor(secs));
  const p = (n) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}

/** Duration for every label except the running clocks: under a minute "0m", under an hour "42m", else "1h 5m". */
export function short(secs) {
  const m = Math.floor(Math.max(0, secs) / 60);
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** "just now", "3m ago", "2h ago" for how long ago something happened. */
export function ago(secs) {
  const m = Math.floor(Math.max(0, secs) / 60);
  return m < 1 ? "just now" : m < 60 ? `${m}m ago` : `${Math.floor(m / 60)}h ago`;
}

/** Local clock time as 24-hour "14:02". */
export function clock(unixSecs) {
  const d = new Date(unixSecs * 1000);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
