// Pure formatters shared by runtime rendering and offline synchronization.
// "2026-09-27" to "27 September 2026".
export function dateLabel(iso: string): string {
  const months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  const p = iso.split("-");
  if (p.length !== 3) return iso;
  return String(parseInt(p[2], 10)) + " " + months[parseInt(p[1], 10) - 1] + " " + p[0];
}

export function pct(passed: number, total: number): number {
  if (total <= 0) return 0;
  return Math.round((passed / total) * 1000) / 10;
}

// 100 only when every case passes; otherwise one decimal, never rounded up to 100.
export function pctLabel(passed: number, total: number): string {
  if (total > 0 && passed === total) return "100%";
  const p = Math.min(pct(passed, total), 99.9);
  return p.toFixed(1) + "%";
}

export function num(n: number): string {
  const s = String(Math.round(n));
  let out = "";
  for (let i = 0; i < s.length; i++) {
    if (i > 0 && (s.length - i) % 3 === 0) out += ",";
    out += s[i];
  }
  return out;
}
