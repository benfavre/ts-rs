// Single source of truth for ts-rs.bext.dev: links, navigation and every
// number shown on the site.
//
// The figures come from src/lib/repo.generated.ts, which scripts/sync.ts
// rebuilds from the tsc-rs repository (README "Status at a glance" and
// docs/verified-waves.md). Nothing on the site hard-codes a pass count: rerun
// the sync when the repository moves.

export { dateLabel, pct, pctLabel, num } from "./metric-format";
import { repoVersion, readmeStatus, waveLog } from "./repo.generated";

export const SITE = "https://ts-rs.bext.dev";
export const REPO = "https://github.com/benfavre/ts-rs";
export const REPO_BLOB = "https://github.com/benfavre/ts-rs/blob/main/";
export const VERSION = repoVersion();

export interface NavItem {
  href: string;
  label: string;
}

export function navItems(): NavItem[] {
  return [
    { href: "/docs", label: "Docs" },
    { href: "/conformance", label: "Conformance" },
    { href: "/progress", label: "Progress" },
    { href: "/playground", label: "Playground" },
  ];
}

// ---------------------------------------------------------------------------
// Conformance lanes.
//
// Every lane comes from the published compatibility snapshot. The wave log
// supplies historical chart values only.

export interface Lane {
  id: string;
  name: string;
  what: string;
  measured: string;
  compiler: { passed: number; total: number };
  conformance: { passed: number; total: number };
}

export function latestWaveDate(): string {
  const waves = waveLog();
  return waves[waves.length - 1].date;
}

export function currentLanes(): Lane[] {
  const m = readmeStatus().metrics;
  return [
    ["emit", "js", "JavaScript emit", "Byte-for-byte match with the tsc .js baseline, one configuration per case."],
    ["diagnostics", "errors", "Diagnostics", "Whole-case match with the tsc .errors.txt baseline: message, position and code."],
    ["symbols", "symbols", "Symbols", "Match with the tsc .symbols baseline."],
    ["types", "types", "Types", "Match with the tsc .types baseline."],
  ].map(([id, key, name, what]) => {
    const c = m.baselines.compiler[key as keyof typeof m.baselines.compiler];
    const f = m.baselines.conformance[key as keyof typeof m.baselines.conformance];
    return { id, name, what, measured: m.measured_at,
      compiler: { passed: c.passed, total: c.passed + c.failed },
      conformance: { passed: f.passed, total: f.passed + f.failed } };
  });
}

export function expandedLanes() {
  const m = readmeStatus().metrics;
  return ["js-expanded", "declarations-expanded"].map(key => {
    const c = m.baselines.compiler[key as keyof typeof m.baselines.compiler];
    const f = m.baselines.conformance[key as keyof typeof m.baselines.conformance];
    return { name: key === "js-expanded" ? "JavaScript" : "Declarations", measured: m.measured_at,
      passed: c.passed + f.passed, total: c.passed + c.failed + f.passed + f.failed,
      skipped: c.skipped + f.skipped, identities: c.total + f.total };
  });
}

export function lspInventory() { return readmeStatus().metrics.lsp_inventory; }
export function measuredDate(): string { return readmeStatus().measured; }
export function baselineCounts() { return readmeStatus().metrics.baselines; }

export function rustTests(): { passed: number; ignored: number; measured: string } {
  const r = readmeStatus();
  return { passed: r.rustTests.passed, ignored: r.rustTests.ignored, measured: r.measured };
}

// Per-diagnostic accuracy from the same published measurement.
export interface RatioLane {
  name: string;
  what: string;
  compiler: string;
  conformance: string;
  compilerPct: number;
  conformancePct: number;
}

export function accuracyLanes(): RatioLane[] {
  const m = readmeStatus().metrics;
  return ["recall", "precision"].map(key => {
    const compilerPct = 100 * m.diagnostic_accuracy.compiler[key as "recall" | "precision"];
    const conformancePct = 100 * m.diagnostic_accuracy.conformance[key as "recall" | "precision"];
    return { name: "Type-check " + key,
      what: key === "recall" ? "Share of expected diagnostics found, matched per (file, line, code) over every option variant." : "Share of reported diagnostics that tsc also reports, same matching rule.",
      compiler: compilerPct.toFixed(1) + "%", conformance: conformancePct.toFixed(1) + "%", compilerPct, conformancePct };
  });
}

export interface LspRow {
  op: string;
  passed: number;
  failed: number;
  skipped: number;
}

export function lspRows(): LspRow[] {
  return readmeStatus().lsp;
}

// ---------------------------------------------------------------------------
// Workspace crates (README, "Workspace layout").

export interface Crate {
  name: string;
  role: string;
}

export function pipelineCrates(): Crate[] {
  return [
    { name: "tsc_rs_scanner", role: "Lexer" },
    { name: "tsc_rs_parser", role: "Parser with tsc-compatible error recovery" },
    { name: "tsc_rs_symbols", role: "Binder and symbol table" },
    { name: "tsc_rs_types", role: "Type checker: inference, narrowing, relations" },
    { name: "tsc_rs_emitter", role: "JavaScript, source map and declaration emit" },
  ];
}

export function supportCrates(): Crate[] {
  return [
    { name: "tsc_rs_ast", role: "AST, spans, diagnostics, compiler options" },
    { name: "tsc_rs_resolver", role: "Module resolution" },
    { name: "tsc_rs_project", role: "tsconfig, project graph, references, .tsbuildinfo" },
    { name: "tsc_rs_incremental", role: "Incremental build metadata and change detection" },
    { name: "tsc_rs_query", role: "Query facade used by tooling" },
    { name: "tsc_rs_server", role: "LSP server" },
    { name: "tsc_rs_cli", role: "The tsc-rs binary" },
    { name: "tsc_rs_harness", role: "Baseline harness and reports" },
    { name: "tsc_rs_bench", role: "Scanner, parser and emitter benchmarks" },
    { name: "tsc_rs_wasm", role: "WebAssembly build of the compiler" },
    { name: "tsc_rs_analyze", role: "Static analysis for TypeScript monorepos" },
  ];
}

// ---------------------------------------------------------------------------
// Icons: Feather-style stroke paths on a 24x24 grid.

export function iconPath(name: string): string {
  switch (name) {
    case "arrow": return "M5 12h14 M13 6l6 6-6 6";
    case "code": return "m16 18 6-6-6-6 M8 6l-6 6 6 6";
    case "check": return "M20 6 9 17l-5-5";
    case "shield": return "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z M9 12l2 2 4-4";
    case "terminal": return "m4 17 6-6-6-6 M12 19h8";
    case "cursor": return "M4 4l7.07 17 2.51-7.39L21 11.07z";
    case "layers": return "m12 2 10 5-10 5L2 7l10-5z M2 17l10 5 10-5 M2 12l10 5 10-5";
    case "zap": return "M13 2 3 14h9l-1 8 10-12h-9z";
    case "search": return "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35";
    case "box": return "M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z M3.27 6.96 12 12.01l8.73-5.05 M12 22.08V12";
    case "file": return "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M8 13h8 M8 17h5";
    case "git": return "M6 3v12 M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M18 9a9 9 0 0 1-9 9";
    case "play": return "M6 4l14 8-14 8z";
    case "book": return "M4 19.5A2.5 2.5 0 0 1 6.5 17H20 M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z";
    case "cpu": return "M6 6h12v12H6z M9 9h6v6H9z M9 2v2 M15 2v2 M9 20v2 M15 20v2 M2 9h2 M2 15h2 M20 9h2 M20 15h2";
    case "menu": return "M4 7h16 M4 12h16 M4 17h16";
    case "sun": return "M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10z M12 1v2 M12 21v2 M4.22 4.22l1.42 1.42 M18.36 18.36l1.42 1.42 M1 12h2 M21 12h2 M4.22 19.78l1.42-1.42 M18.36 5.64l1.42-1.42";
    case "moon": return "M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z";
    case "trend": return "M23 6l-9.5 9.5-5-5L1 18 M17 6h6v6";
    case "external": return "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6 M15 3h6v6 M10 14 21 3";
    default: return "";
  }
}

export function githubIconHtml(size: number): string {
  return (
    '<svg width="' + size + '" height="' + size + '" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">' +
    '<path d="M12 .5C5.7.5.5 5.7.5 12c0 5.1 3.3 9.4 7.9 10.9.6.1.8-.3.8-.6v-2c-3.2.7-3.9-1.5-3.9-1.5-.5-1.3-1.3-1.7-1.3-1.7-1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.7 1.3 3.4 1 .1-.8.4-1.3.7-1.6-2.6-.3-5.3-1.3-5.3-5.8 0-1.3.5-2.3 1.2-3.1-.1-.3-.5-1.6.1-3.2 0 0 1-.3 3.3 1.2a11.4 11.4 0 0 1 6 0C16.9 4.7 18 5 18 5c.6 1.6.2 2.9.1 3.2.8.8 1.2 1.8 1.2 3.1 0 4.5-2.7 5.5-5.3 5.8.4.4.8 1.1.8 2.3v3.3c0 .3.2.7.8.6 4.6-1.5 7.9-5.8 7.9-10.9C23.5 5.7 18.3.5 12 .5z"/></svg>'
  );
}
