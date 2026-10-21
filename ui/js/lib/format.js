export function fmt(secs) {
  const m = Math.round(secs / 60);
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

export function mmss(secs) {
  return `${String(Math.floor(secs / 60)).padStart(2, "0")}:${String(secs % 60).padStart(2, "0")}`;
}

export function hms(secs) {
  const s = Math.max(0, Math.floor(secs));
  const p = (n) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}

/** Short duration: "42m", "1h 05m"; never "0h 0m" for tiny values. */
export function short(secs) {
  if (secs < 60) return `${Math.round(secs)}s`; // keeps parts of a short session consistent with their sum
  const m = Math.round(secs / 60);
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}
