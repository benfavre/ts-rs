// Progress page: the verified wave log as a chart and a timeline.
//
// Data comes from src/lib/repo.generated.ts (docs/verified-waves.md in the
// repository). The chart is inline SVG built here; public/site.js adds the
// crosshair and tooltip from the JSON in #wave-data. The timeline below the
// chart is the table view of the same data.

import { REPO_BLOB, dateLabel, num, pctLabel, currentLanes } from "../lib/site";
import { waveLog } from "../lib/repo.generated";
import { esc, button } from "../lib/html";

interface Point {
  i: number;
  date: string;
  title: string;
  c: number | null;
  f: number | null;
}

function inlineCode(text: string): string {
  return esc(text).replace(/`([^`]+)`/g, "<code>$1</code>");
}

function clip(text: string, max: number): string {
  if (text.length <= max) return text;
  const cut = text.slice(0, max);
  const at = cut.lastIndexOf(" ");
  return (at > max * 0.6 ? cut.slice(0, at) : cut) + "...";
}

function shortDate(iso: string): string {
  const parts = dateLabel(iso).split(" ");
  return parts[0] + " " + parts[1].slice(0, 3);
}

// Step-after path through the known points of one series.
function stepPath(xs: number[], ys: (number | null)[]): string {
  let d = "";
  let prevY: number | null = null;
  for (let i = 0; i < xs.length; i++) {
    const y = ys[i];
    if (y === null) continue;
    if (prevY === null) d += "M" + xs[i].toFixed(1) + " " + y.toFixed(1);
    else d += " H" + xs[i].toFixed(1) + " V" + y.toFixed(1);
    prevY = y;
  }
  return d;
}

function chart(points: Point[], baseC: number, baseF: number): string {
  const W = 960;
  const H = 330;
  const L = 46;
  const R = 22;
  const T = 14;
  const B = 36;
  const n = points.length;
  let top = 0;
  for (let i = 0; i < n; i++) {
    const p = points[i];
    if (p.c !== null) top = Math.max(top, p.c - baseC);
    if (p.f !== null) top = Math.max(top, p.f - baseF);
  }
  const yMax = Math.max(50, Math.ceil(top / 50) * 50);
  const xAt = (i: number) => L + (n === 1 ? 0 : (i * (W - L - R)) / (n - 1));
  const yAt = (v: number) => T + (H - T - B) * (1 - v / yMax);

  const xs: number[] = [];
  const yc: (number | null)[] = [];
  const yf: (number | null)[] = [];
  for (let i = 0; i < n; i++) {
    xs.push(xAt(i));
    yc.push(points[i].c === null ? null : yAt((points[i].c as number) - baseC));
    yf.push(points[i].f === null ? null : yAt((points[i].f as number) - baseF));
  }

  let grid = "";
  for (let v = 0; v <= yMax; v += 50) {
    const y = yAt(v).toFixed(1);
    grid +=
      '<line class="cv-grid" x1="' + L + '" x2="' + (W - R) + '" y1="' + y + '" y2="' + y + '"/>' +
      '<text class="cv-tick" x="' + (L - 10) + '" y="' + y + '" dy="0.34em" text-anchor="end">' + (v === 0 ? "0" : "+" + v) + "</text>";
  }

  // One x label per calendar day. A day too narrow to hold its label (a single
  // wave squeezed between two busy days) goes unlabelled; the tooltip has it.
  let ticks = "";
  let lastDate = "";
  for (let i = 0; i < n; i++) {
    if (points[i].date === lastDate) continue;
    lastDate = points[i].date;
    let end = i;
    while (end + 1 < n && points[end + 1].date === lastDate) end++;
    if (i > 0 && xs[end] - xs[i] < 60) continue;
    ticks +=
      '<line class="cv-grid" x1="' + xs[i].toFixed(1) + '" x2="' + xs[i].toFixed(1) + '" y1="' + (H - B) + '" y2="' + (H - B + 5) + '"/>' +
      '<text class="cv-tick" x="' + xs[i].toFixed(1) + '" y="' + (H - B + 20) + '" text-anchor="' + (i === 0 ? "start" : "middle") + '">' +
      esc(shortDate(points[i].date)) + "</text>";
  }

  const endC = yc[n - 1];
  const endF = yf[n - 1];
  const endX = xs[n - 1].toFixed(1);
  return (
    '<svg class="cv-svg" viewBox="0 0 ' + W + " " + H + '" role="img" aria-label="Diagnostic cases gained per wave, compiler and conformance suites" ' +
    'data-l="' + L + '" data-r="' + R + '" data-t="' + T + '" data-b="' + B + '" data-w="' + W + '" data-h="' + H + '" data-ymax="' + yMax + '">' +
    grid + ticks +
    '<path class="cv-line cv-s2" d="' + stepPath(xs, yf) + '"/>' +
    '<path class="cv-line cv-s1" d="' + stepPath(xs, yc) + '"/>' +
    (endF === null ? "" : '<circle class="cv-dot cv-s2" cx="' + endX + '" cy="' + endF.toFixed(1) + '" r="5"/>') +
    (endC === null ? "" : '<circle class="cv-dot cv-s1" cx="' + endX + '" cy="' + endC.toFixed(1) + '" r="5"/>') +
    '<g class="cv-hover" hidden><line class="cv-cross" y1="' + T + '" y2="' + (H - B) + '"/>' +
    '<circle class="cv-dot cv-s2" r="5"/><circle class="cv-dot cv-s1" r="5"/></g>' +
    "</svg>"
  );
}

function delta(now: number | null, before: number | null): number {
  if (now === null || before === null) return 0;
  return now - before;
}

export function progressHtml(): string {
  const waves = waveLog();
  const lanes = currentLanes();
  const diag = lanes[1];
  const points: Point[] = [];
  let baseC = 0;
  let baseF = 0;
  let haveC = false;
  let haveF = false;
  for (let i = 0; i < waves.length; i++) {
    const w = waves[i];
    if (!haveC && w.compilerDiag !== null) { baseC = w.compilerDiag; haveC = true; }
    if (!haveF && w.conformanceDiag !== null) { baseF = w.conformanceDiag; haveF = true; }
    points.push({ i: i, date: w.date, title: w.title, c: w.compilerDiag, f: w.conformanceDiag });
  }
  const last = waves[waves.length - 1];
  const gainC = delta(last.compilerDiag, baseC);
  const gainF = delta(last.conformanceDiag, baseF);

  // Timeline, newest first, grouped by day.
  let list = "";
  let day = "";
  for (let i = waves.length - 1; i >= 0; i--) {
    const w = waves[i];
    const prev = i > 0 ? waves[i - 1] : null;
    if (w.date !== day) {
      if (day) list += "</ol>";
      day = w.date;
      list += '<h3 class="wv-day">' + esc(dateLabel(w.date)) + '</h3><ol class="wv-list">';
    }
    const dc = prev ? delta(w.compilerDiag, prev.compilerDiag) : 0;
    const df = prev ? delta(w.conformanceDiag, prev.conformanceDiag) : 0;
    let chips = "";
    if (dc > 0) chips += '<span class="wv-chip"><i class="cv-key cv-s1"></i>+' + dc + " compiler</span>";
    if (df > 0) chips += '<span class="wv-chip"><i class="cv-key cv-s2"></i>+' + df + " conformance</span>";
    for (let k = 0; k < w.codes.length; k++) chips += '<span class="wv-code">' + esc(w.codes[k]) + "</span>";
    list +=
      '<li class="wv" id="wave-' + i + '"><div class="wv-head"><h4>' + esc(w.title.charAt(0).toUpperCase() + w.title.slice(1)) + "</h4>" +
      (chips ? '<div class="wv-chips">' + chips + "</div>" : "") + "</div>" +
      "<p>" + inlineCode(clip(w.summary, 300)) + "</p>" +
      '<div class="wv-foot"><span>' +
      (w.compilerDiag === null ? "" : num(w.compilerDiag) + " compiler") +
      (w.conformanceDiag === null ? "" : " &middot; " + num(w.conformanceDiag) + " conformance cases passing") +
      '</span><a class="t-link" href="' + REPO_BLOB + "docs/verified-waves.md#" + esc(w.anchor) + '" target="_blank" rel="noopener">Validation and timings</a></div></li>';
  }
  if (day) list += "</ol>";

  const data = JSON.stringify({ baseC: baseC, baseF: baseF, points: points }).replace(/</g, "\\u003c");

  return (
    '<div class="t-wrap">' +
    '<header class="t-page-head"><span class="t-eyebrow">Progress</span>' +
    "<h1>One verified wave at a time.</h1>" +
    "<p>tsc-rs moves in small waves. Each one targets a family of diagnostics, and is merged only with a cache-free before and after comparison " +
    "that shows which cases newly pass and that no passing case was lost.</p>" +
    '<div class="t-stamp"><span><b>Waves</b> ' + waves.length + "</span><span><b>From</b> " + esc(dateLabel(waves[0].date)) +
    "</span><span><b>To</b> " + esc(dateLabel(last.date)) + "</span></div></header>" +

    '<section class="t-block"><div class="cv rv">' +
    '<div class="cv-head"><div><h2>Diagnostic cases gained</h2>' +
    "<p>Whole-case diagnostic matches gained since the first recorded wave, by test suite.</p></div>" +
    '<ul class="cv-legend">' +
    '<li><i class="cv-key cv-s1"></i><span>Compiler suite</span><b>+' + gainC + "</b><em>" + num(diag.compiler.passed) + " of " + num(diag.compiler.total) +
    " passing, " + pctLabel(diag.compiler.passed, diag.compiler.total) + "</em></li>" +
    '<li><i class="cv-key cv-s2"></i><span>Conformance suite</span><b>+' + gainF + "</b><em>" + num(diag.conformance.passed) + " of " + num(diag.conformance.total) +
    " passing, " + pctLabel(diag.conformance.passed, diag.conformance.total) + "</em></li>" +
    "</ul></div>" +
    '<div class="cv-plot" id="wave-chart">' + chart(points, baseC, baseF) +
    '<div class="cv-tip" hidden></div></div>' +
    '<p class="cv-note">Each step is one wave. JavaScript emit stayed at 100% of baselines throughout. Hover or tap the chart for a wave; the full list is below.</p>' +
    '<script type="application/json" id="wave-data">' + data + "</script>" +
    "</div></section>" +

    '<section class="t-block"><div class="t-block-head"><div class="wv-side"><h2>The wave log</h2>' +
    "<p>Newest first. Each entry links to its validation record: the tests added, the manifest comparison, and the timing of the change.</p>" +
    '<div style="margin-top:1.4rem;display:flex;flex-direction:column;gap:.6rem;align-items:flex-start">' +
    button({ href: "/conformance", label: "Conformance report", kind: "ghost" }) +
    button({ href: "/docs/testing", label: "How a wave is verified", kind: "ghost" }) +
    button({ href: "/feed.xml", label: "Atom feed", kind: "ghost" }) +
    "</div></div><div>" + list + "</div></div></section>" +
    "</div>"
  );
}

// The three most recent waves, for the home page.
export function latestWavesHtml(): string {
  const waves = waveLog();
  let items = "";
  for (let i = waves.length - 1; i >= Math.max(0, waves.length - 3); i--) {
    const w = waves[i];
    items +=
      '<a class="nw" href="/progress#wave-' + i + '"><span class="nw-date">' + esc(shortDate(w.date)) + "</span>" +
      '<span class="nw-title">' + esc(w.title.charAt(0).toUpperCase() + w.title.slice(1)) + "</span></a>";
  }
  return items;
}
