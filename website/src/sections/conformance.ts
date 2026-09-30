// Conformance report page, assembled as one HTML string.
//
// All figures come from lib/site.ts, which mirrors the repository README and
// docs/verified-waves.md. Lanes that were not rerun for the current wave are
// labelled as such here, exactly as the README labels them.

import {
  REPO_BLOB,
  currentLanes, accuracyLanes, expandedLanes, measuredDate, baselineCounts, lspInventory, lspRows, latestWaveDate, dateLabel, pct, pctLabel, num,
} from "../lib/site";
import { esc, button, codeBlock, meter } from "../lib/html";

function laneRow(suite: string, passed: number, total: number): string {
  const full = passed === total;
  return (
    '<div class="t-lane-row"><span class="t-lane-suite">' + esc(suite) + "</span>" +
    meter(pct(passed, total), full ? "pass" : "accent") +
    '<span class="t-lane-count">' + num(passed) + " / " + num(total) + "</span>" +
    '<span class="t-lane-pct">' + pctLabel(passed, total) + "</span></div>"
  );
}

function blockHead(title: string, body: string, tag: string, tagKind: string, content: string): string {
  return (
    '<section class="t-block"><div class="t-block-head rv"><div><h2>' + esc(title) + "</h2><p>" + body + "</p>" +
    (tag ? '<span class="t-tag t-tag-' + tagKind + '">' + esc(tag) + "</span>" : "") +
    "</div><div>" + content + "</div></div></section>"
  );
}

function current(): string {
  const lanes = currentLanes();
  let html = "";
  for (let i = 0; i < lanes.length; i++) {
    const l = lanes[i];
    html +=
      '<div class="t-lane"><h3>' + esc(l.name) + "</h3><p>" + esc(l.what) + "</p>" +
      laneRow("Compiler suite", l.compiler.passed, l.compiler.total) +
      laneRow("Conformance suite", l.conformance.passed, l.conformance.total) +
      "</div>";
  }
  return blockHead(
    "Baseline lanes",
    "One configuration per test case, compared with the baseline file that tsc itself produced. " +
      "The emit lane skips " + num(baselineCounts().compiler.js.skipped) + " compiler and " + num(baselineCounts().conformance.js.skipped) + " conformance cases that have no .js oracle (mostly noEmit); the diagnostics lane exercises all of them. Symbols and types skip " + num(baselineCounts().compiler.symbols.skipped) + " compiler and " + num(baselineCounts().conformance.symbols.skipped) + " conformance cases without a uniquely selected oracle.",
    "Wave of " + dateLabel(latestWaveDate()),
    "now",
    html,
  );
}

function expanded(): string {
  const lanes = expandedLanes();
  let html = '<div class="t-lane"><h3>Expanded option variants</h3><p>Every stored option variant of every case: ' + num(lanes[0].identities) + ' identities.</p>';
  for (const lane of lanes) {
    html += '<div class="t-lane-row"><span class="t-lane-suite">' + esc(lane.name) + '</span>' + meter(pct(lane.passed, lane.total), "accent") +
      '<span class="t-lane-count">' + num(lane.passed) + ' / ' + num(lane.total) + '</span><span class="t-lane-pct">' + pctLabel(lane.passed, lane.total) + '</span></div>';
  }
  html += '<p>' + num(lanes[0].skipped) + ' identities without an oracle are skipped in each lane.</p></div>';
  return blockHead("Every option variant", "Many TypeScript tests run under several compiler options. This opt-in report expands each one. The failures are mostly ES5 downlevel transforms and .d.ts emit.",
    "Measured " + dateLabel(measuredDate()), "now", html);
}

function ratioRow(suite: string, label: string, percent: number): string {
  return (
    '<div class="t-lane-row"><span class="t-lane-suite">' + esc(suite) + "</span>" + meter(percent, "muted") +
    '<span class="t-lane-count">' + esc(label.indexOf("(") === -1 ? "" : label.slice(0, label.indexOf("(")).trim()) + "</span>" +
    '<span class="t-lane-pct">' + percent.toFixed(1) + "%</span></div>"
  );
}

function earlier(): string {
  const lanes = accuracyLanes();
  let html = "";
  for (let i = 0; i < lanes.length; i++) {
    const l = lanes[i];
    html +=
      '<div class="t-lane"><h3>' + esc(l.name) + "</h3><p>" + esc(l.what) + "</p>" +
      ratioRow("Compiler suite", l.compiler, l.compilerPct) +
      ratioRow("Conformance suite", l.conformance, l.conformancePct) +
      "</div>";
  }
  return blockHead(
    "Type-check accuracy",
    "Recall and precision count individual diagnostics over every option variant, " +
      "so they are stricter about coverage and looser about wording than the whole-case diagnostics lane.",
    "Measured " + dateLabel(measuredDate()),
    "now",
    html,
  );
}

function lsp(): string {
  const rows = lspRows();
  let body = "";
  let tp = 0;
  let tf = 0;
  let ts = 0;
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i];
    tp += r.passed; tf += r.failed; ts += r.skipped;
    const total = r.passed + r.failed;
    body +=
      "<tr><td>" + esc(r.op) + '</td><td class="n">' + num(r.passed) + '</td><td class="n">' + num(r.failed) +
      '</td><td class="n">' + num(r.skipped) + '</td><td class="n">' + pctLabel(r.passed, total) + "</td><td>" +
      meter(pct(r.passed, total), "accent") + "</td></tr>";
  }
  body +=
    '<tr class="total"><td>Total</td><td class="n">' + num(tp) + '</td><td class="n">' + num(tf) + '</td><td class="n">' + num(ts) +
    '</td><td class="n">' + pctLabel(tp, tp + tf) + "</td><td>" + meter(pct(tp, tp + tf), "accent") + "</td></tr>";
  const table =
    '<div class="t-table-wrap"><table class="t-table"><thead><tr><th>Operation</th><th class="n">Passed</th><th class="n">Failed</th>' +
    '<th class="n">Skipped</th><th class="n">Pass rate</th><th>Share</th></tr></thead><tbody>' + body + "</tbody></table></div>";
  return blockHead(
    "Language server",
    "The language server is tested against the TypeScript fourslash suite, the same editor-behaviour tests tsc uses. " +
      "The table covers the five supported operations. Across the full inventory, " + num(lspInventory().skipped) + " cases are skipped, including unsupported operations; skips never count as passes.",
    "Measured " + dateLabel(measuredDate()),
    "now",
    table,
  );
}

function work(): string {
  const d = currentLanes()[1];
  const cFail = d.compiler.total - d.compiler.passed;
  const fFail = d.conformance.total - d.conformance.passed;
  const html =
    '<div class="t-prose"><ul>' +
    "<li><b>Default emit matches</b> every case that has a JavaScript oracle. The remaining emit work is in the expanded variants: ES5 downlevel transforms and declaration files.</li>" +
    "<li><b>Diagnostics are the active front.</b> A whole-case pass needs every message and column to match, which is where the remaining " + num(cFail) + " compiler and " + num(fFail) + " conformance failures come from: " + num(cFail + fFail) + " mismatches in total.</li>" +
    "<li><b>Language server gaps are type inference.</b> Hover misses are contextual typing, generics, JSDoc type tags and cross-file aliases. Completion misses are auto-imports and cross-file members.</li>" +
    "<li><b>Symbols and declaration emit are early.</b></li>" +
    "</ul></div>";
  return blockHead("Where the work is", "What the failing cases have in common, lane by lane.", "", "", html);
}

function method(): string {
  const cmds = [
    "cargo build --release -p tsc_rs_harness",
    "",
    "# JavaScript emit and diagnostics (drop --baseline errors for emit)",
    "target/release/baseline-report --suite compiler --baseline errors --no-cache",
    "target/release/baseline-report --suite conformance --baseline errors --no-cache",
    "",
    "# Symbols",
    "target/release/baseline-report --suite compiler --baseline symbols --no-cache",
    "",
    "# Type-check recall and precision (run the full corpus)",
    "target/release/typecheck-report --suite compiler --json",
    "",
    "# Language server",
    "target/release/lsp-report --op all --json",
  ].join("\n");
  const html =
    '<div class="t-prose" style="margin-bottom:1.2rem"><ul>' +
    "<li>Pass rate is <code>passed / (passed + failed)</code>. Skipped cases are excluded and never counted as passes.</li>" +
    "<li>The oracle is chosen from the effective compiler options, never from whichever candidate output happens to match.</li>" +
    "<li>Results are cached by a hash of the compiler sources, so published numbers are always taken with <code>--no-cache</code>.</li>" +
    "<li>Each wave records newly passing cases, lost passes and unchanged skips before it is merged.</li>" +
    "</ul></div>" +
    codeBlock("Reproduce any row", "bash", cmds);
  return blockHead(
    "Method",
    "The test corpus is the TypeScript repository's own cases and reference baselines, vendored unmodified under <code>tests/</code>. " +
      'Wave by wave results are in <a class="t-link" href="' + REPO_BLOB + 'docs/verified-waves.md" target="_blank" rel="noopener">docs/verified-waves.md</a>.',
    "",
    "",
    html,
  );
}

export function conformanceHtml(): string {
  return (
    '<div class="t-wrap">' +
    '<header class="t-page-head"><span class="t-eyebrow">Conformance report</span>' +
    "<h1>How close is tsc-rs to tsc?</h1>" +
    "<p>tsc-rs is measured against the TypeScript project's own test suites, using the baseline files that the real compiler produced. " +
    "This page reports every lane, including the ones that are far from done.</p>" +
    '<div class="t-stamp"><span><b>Suites</b> compiler, conformance, fourslash</span><span><b>Oracle</b> tsc reference baselines</span>' +
    "<span><b>Latest wave</b> " + esc(dateLabel(latestWaveDate())) + "</span></div></header>" +
    current() + expanded() + earlier() + lsp() + work() + method() +
    '<div style="padding:1rem 0 0;display:flex;gap:.7rem;flex-wrap:wrap">' +
    button({ href: "/progress", label: "See the wave log", kind: "primary", icon: "arrow" }) +
    button({ href: "/docs/testing", label: "How the harness works", kind: "ghost" }) +
    button({ href: REPO_BLOB + "docs/compatibility-metrics.json", label: "Raw metrics snapshot", kind: "ghost", external: true }) +
    "</div></div>"
  );
}
