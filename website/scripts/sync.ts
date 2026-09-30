// Regenerate everything the site derives from other files.
//
//   bun scripts/sync.ts [path-to-ts-rs]      (default: ~/ts-rs-public)
//
// From the tsc-rs checkout:
//   src/lib/repo.generated.ts   version, published compatibility metrics, the wave log
// From this site's own docs:
//   public/search-index.json    docs search
//   public/sitemap.xml
//   public/llms.txt
//
// The parsers fail loudly: if the snapshot or the wave log changes shape, the
// script exits non-zero and leaves the generated files untouched.

import { readFileSync, writeFileSync, existsSync, renameSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";
import { docsNav, docsFlat } from "../src/lib/docs-nav";
import { renderDoc } from "../src/lib/markdown";

const SITE_DIR = resolve(import.meta.dir, "..");
const REPO = resolve(process.argv[2] || join(process.env.HOME || "", "ts-rs-public"));
const ORIGIN = "https://ts-rs.bext.dev";

function fail(msg: string): never {
  console.error("sync: " + msg);
  process.exit(1);
}

function int(s: string): number {
  const n = parseInt(s.replace(/,/g, ""), 10);
  if (!Number.isFinite(n)) fail("not a number: " + s);
  return n;
}

// ---------------------------------------------------------------------------
// Published compatibility snapshot. Never label working-tree edits as HEAD.
function git(args: string[]): string {
  return execFileSync("git", ["-C", REPO].concat(args), { encoding: "utf8" }).trim();
}
const sourceCommit = git(["rev-parse", "HEAD"]);
const published = git(["ls-remote", "https://github.com/benfavre/ts-rs.git", "refs/heads/main"]).split(/\s+/)[0];
if (sourceCommit !== published) fail("push this checkout to benfavre/ts-rs main before syncing the site");
function committed(path: string): string {
  return execFileSync("git", ["-C", REPO, "show", sourceCommit + ":" + path], { encoding: "utf8" });
}
const cargo = committed("Cargo.toml");
const version = (/\[workspace\.package\][\s\S]*?\nversion = "([^"]+)"/.exec(cargo) || [])[1];
if (!version) fail("workspace version not found in Cargo.toml");
const metrics = JSON.parse(committed("docs/compatibility-metrics.json"));
const measured = metrics.measured_at;
if (!/^\d{4}-\d{2}-\d{2}$/.test(measured)) fail("metrics: invalid measurement date");
git(["merge-base", "--is-ancestor", metrics.source_commit, sourceCommit]);
function counts(value: any, label: string) {
  for (const key of ["total", "passed", "failed", "skipped"]) {
    if (!Number.isSafeInteger(value?.[key]) || value[key] < 0) fail(label + ": invalid " + key);
  }
  if (value.total !== value.passed + value.failed + value.skipped) fail(label + ": inconsistent counts");
}
for (const suite of ["compiler", "conformance"]) {
  for (const lane of ["js", "errors", "symbols", "types", "js-expanded", "declarations-expanded"]) {
    counts(metrics.baselines?.[suite]?.[lane], suite + ":" + lane);
  }
  const a = metrics.diagnostic_accuracy?.[suite];
  if (!a || a.expected_diagnostics !== a.matched + a.false_negatives ||
      a.recall !== a.matched / a.expected_diagnostics ||
      a.precision !== a.matched / (a.matched + a.false_positives)) fail(suite + ": invalid accuracy");
}
const lspNames = [["quickinfo", "QuickInfo (hover)"], ["completions", "Completions"],
  ["gotodefinition", "Go-to-definition"], ["findallrefs", "Find-all-references"], ["signaturehelp", "Signature help"]];
const lsp = lspNames.map(([key, op]) => {
  const r = metrics.lsp[key]; counts(r, "LSP " + key);
  return { op, passed: r.passed, failed: r.failed, skipped: r.skipped };
});
counts(metrics.lsp_inventory, "LSP inventory");
for (const key of ["passed", "failed"] as const) {
  if (lsp.reduce((sum, row) => sum + row[key], 0) !== metrics.lsp_inventory[key]) fail("LSP operation totals disagree with the inventory");
}
counts(metrics.rust_tests, "Rust tests");
const lane = (name: string) => Object.fromEntries(["compiler", "conformance"].map(suite => {
  const r = metrics.baselines[suite][name]; return [suite, [r.passed, r.passed + r.failed]];
}));
const emit = lane("js"), diagnostics = lane("errors");
const commit = sourceCommit.slice(0, 9);
const commitDate = git(["log", "-1", "--format=%cs"]);
const syncedOn = new Date().toISOString().slice(0, 10);
// Validate all content before replacing any generated output.
const outputs = new Map<string, string>();
function output(path: string, content: string): void { outputs.set(path, content); }

// ---------------------------------------------------------------------------
// docs/verified-waves.md: one entry per "## YYYY-MM-DD: title" section

interface Wave {
  date: string;
  title: string;
  anchor: string;
  summary: string;
  codes: string[];
  compilerDiag: number | null;
  conformanceDiag: number | null;
  compilerJs: number | null;
  conformanceJs: number | null;
}

const log = committed("docs/verified-waves.md");
const parts = log.split(/^## (\d{4}-\d{2}-\d{2}): (.+)$/m);
if (parts.length < 4) fail("verified-waves.md: no wave headings found");

function ghAnchor(heading: string): string {
  return heading.toLowerCase().replace(/[^a-z0-9 _-]/g, "").replace(/ /g, "-");
}

function ascii(s: string): string {
  return s
    .replace(/[\u2018\u2019]/g, "'").replace(/[\u201c\u201d]/g, '"')
    .replace(/\u2014/g, ", ").replace(/\u2013/g, "-").replace(/\u2026/g, "...")
    .replace(/[^\x00-\x7F]/g, "");
}

const waves: Wave[] = [];
const last = { compilerDiag: null as number | null, conformanceDiag: null as number | null, compilerJs: null as number | null, conformanceJs: null as number | null };

for (let i = 1; i + 2 < parts.length; i += 3) {
  const date = parts[i];
  const title = parts[i + 1].trim();
  const body = parts[i + 2] || "";

  const para = body.trim().split(/\n\s*\n/)[0] || "";
  const summary = ascii(
    para
      .replace(/^\s*[-*] +/gm, "")
      .replace(/\*\*/g, "")
      .replace(/\s+/g, " ")
      .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
      .trim(),
  );

  // Result tables: "| Suite | Baseline | ... | After |" or "... | Passed | Failed | Skipped |".
  const lines = body.split("\n");
  let col = -1;
  let diagnosticOnly = false;
  for (const line of lines) {
    if (!line.startsWith("|")) { col = -1; continue; }
    const cells = line.split("|").slice(1, -1).map((c) => c.trim());
    if (cells[0] === "Suite" && (cells[1] === "Baseline" || cells[1] === "Diagnostic passes before")) {
      diagnosticOnly = cells[1] === "Diagnostic passes before";
      col = cells.indexOf("After");
      if (col === -1) col = cells.indexOf("Passed");
      continue;
    }
    if (col === -1 || /^[-: ]+$/.test(cells[0])) continue;
    const suite = cells[0];
    const baseline = diagnosticOnly ? "Diagnostics" : cells[1];
    const value = cells[col];
    if (!value || !/^[\d,]+$/.test(value)) continue;
    const n = int(value);
    if (suite === "Compiler" && baseline === "Diagnostics") last.compilerDiag = n;
    if (suite === "Conformance" && baseline === "Diagnostics") last.conformanceDiag = n;
    if (suite === "Compiler" && baseline === "JavaScript") last.compilerJs = n;
    if (suite === "Conformance" && baseline === "JavaScript") last.conformanceJs = n;
  }
  // The earliest waves state the gain in prose only.
  const prose = /Compiler diagnostic passes increase\s+from [\d,]+ to ([\d,]+)/.exec(body);
  if (prose && col === -1) last.compilerDiag = int(prose[1]);

  const codes = Array.from(new Set((title.match(/TS\d{4,5}/g) || [])));
  waves.push({
    date,
    title: ascii(title.replace(/`/g, "")),
    anchor: ghAnchor(date + ": " + title),
    summary,
    codes,
    compilerDiag: last.compilerDiag,
    conformanceDiag: last.conformanceDiag,
    compilerJs: last.compilerJs,
    conformanceJs: last.conformanceJs,
  });
}
if (waves.length < 5) fail("verified-waves.md: only " + waves.length + " waves parsed");
const latest = waves[waves.length - 1];
if (latest.compilerDiag === null || latest.conformanceDiag === null) fail("verified-waves.md: no diagnostics totals found");
if (latest.compilerDiag !== diagnostics.compiler[0] || latest.conformanceDiag !== diagnostics.conformance[0])
  fail("latest wave and published diagnostic snapshot disagree; check the parser or refresh the snapshot");

const generated =
  "// GENERATED by scripts/sync.ts from the tsc-rs repository. Do not edit.\n" +
  "// Source: published docs/compatibility-metrics.json and docs/verified-waves.md.\n\n" +
  "export interface Wave {\n  date: string;\n  title: string;\n  anchor: string;\n  summary: string;\n  codes: string[];\n" +
  "  compilerDiag: number | null;\n  conformanceDiag: number | null;\n  compilerJs: number | null;\n  conformanceJs: number | null;\n}\n\n" +
  "export function repoVersion(): string {\n  return " + JSON.stringify(version) + ";\n}\n\n" +
  "export function repoSource(): { commit: string; commitDate: string; syncedOn: string } {\n  return " +
  JSON.stringify({ commit, commitDate, syncedOn }) + ";\n}\n\n" +
  "export function readmeStatus() {\n  return " +
  JSON.stringify({
    measured,
    emit,
    diagnostics,
    lsp,
    rustTests: { passed: metrics.rust_tests.passed, ignored: metrics.rust_tests.skipped },
    metrics,
    totals: { compiler: diagnostics.compiler[1], conformance: diagnostics.conformance[1] },
  }, null, 2).replace(/\n/g, "\n  ") +
  ";\n}\n\n" +
  "export function waveLog(): Wave[] {\n  return " + JSON.stringify(waves, null, 2).replace(/\n/g, "\n  ") + ";\n}\n";

if (/[^\x00-\x7F]/.test(generated)) fail("generated module is not ASCII");
output(join(SITE_DIR, "src/lib/repo.generated.ts"), generated);
console.log("repo.generated.ts: v" + version + ", snapshot measured " + measured + ", " + waves.length + " waves, latest " +
  latest.date + " diagnostics " + latest.compilerDiag + " / " + latest.conformanceDiag);

// ---------------------------------------------------------------------------
// Docs: search index, sitemap, llms.txt

function strip(html: string): string {
  return html
    .replace(/<button[\s\S]*?<\/button>/g, " ")
    .replace(/<a class="t-copy doc-run"[\s\S]*?<\/a>/g, " ")
    .replace(/<a class="hanchor"[\s\S]*?<\/a>/g, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();
}

interface Entry { t: string; h: string; u: string; x: string; }
const entries: Entry[] = [];
const flat = docsFlat();
for (const item of flat) {
  const file = join(SITE_DIR, "content", item.href + ".md");
  if (!existsSync(file)) fail("docs-nav lists " + item.href + " but " + file + " does not exist");
  const html = renderDoc(readFileSync(file, "utf8"), metrics);
  // Split at h2/h3 so each section is its own result.
  const chunks = html.split(/(?=<h[23] id=")/);
  for (const chunk of chunks) {
    const m = /^<h[23] id="([^"]+)" data-title="([^"]*)"/.exec(chunk);
    const text = strip(chunk);
    if (!text) continue;
    entries.push({
      t: item.title,
      h: m ? strip(m[2]) : "",
      u: item.href + (m ? "#" + m[1] : ""),
      x: text.slice(0, 1200),
    });
  }
}
// Site pages and the wave log are searchable too (an error code finds its wave).
entries.push({ t: "Site", h: "Conformance report", u: "/conformance", x: "Conformance report: JavaScript emit, diagnostics, symbols, declarations and language server pass rates against the TypeScript compiler, conformance and fourslash suites. Method and commands to reproduce." });
entries.push({ t: "Site", h: "Playground", u: "/playground", x: "Playground: compile and type-check TypeScript in the browser with the tsc-rs WebAssembly build." });
entries.push({ t: "Site", h: "Progress", u: "/progress", x: "Progress: the verified wave log and a chart of diagnostic cases gained per wave." });
for (let i = waves.length - 1; i >= 0; i--) {
  const w = waves[i];
  entries.push({ t: "Wave " + w.date, h: w.title, u: "/progress#wave-" + i, x: (w.codes.join(" ") + " " + w.summary.replace(/`/g, "")).trim().slice(0, 600) });
}
output(join(SITE_DIR, "public/search-index.json"), JSON.stringify(entries));
console.log("search-index.json: " + entries.length + " sections from " + flat.length + " pages");

const pages: [string, string, string][] = [["/", "weekly", "1.0"], ["/docs", "weekly", "0.9"], ["/conformance", "weekly", "0.9"], ["/progress", "weekly", "0.8"], ["/playground", "monthly", "0.8"]];
for (const item of flat) pages.push([item.href, "monthly", "0.7"]);
output(
  join(SITE_DIR, "public/sitemap.xml"),
  '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' +
    pages.map((p) => "  <url><loc>" + ORIGIN + p[0] + "</loc><changefreq>" + p[1] + "</changefreq><priority>" + p[2] + "</priority></url>").join("\n") +
    "\n</urlset>\n",
);
console.log("sitemap.xml: " + pages.length + " urls");

// Atom feed of the wave log, newest first.
function xml(t: string): string {
  return t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
let feed =
  '<?xml version="1.0" encoding="utf-8"?>\n<feed xmlns="http://www.w3.org/2005/Atom">\n' +
  "  <title>tsc-rs verified waves</title>\n  <subtitle>Each verified change to the tsc-rs TypeScript compiler, with the test cases it gained.</subtitle>\n" +
  '  <link href="' + ORIGIN + '/feed.xml" rel="self"/>\n  <link href="' + ORIGIN + '/progress"/>\n' +
  "  <id>" + ORIGIN + "/progress</id>\n  <updated>" + latest.date + "T00:00:00Z</updated>\n  <author><name>tsc-rs</name></author>\n";
for (let i = waves.length - 1; i >= Math.max(0, waves.length - 40); i--) {
  const w = waves[i];
  // Waves of one day keep their order through the seconds field.
  const stamp = w.date + "T00:" + String(Math.floor(i / 60)).padStart(2, "0") + ":" + String(i % 60).padStart(2, "0") + "Z";
  feed +=
    "  <entry>\n    <title>" + xml(w.title) + "</title>\n    <id>" + ORIGIN + "/progress#wave-" + i + "</id>\n" +
    '    <link href="' + ORIGIN + "/progress#wave-" + i + '"/>\n    <updated>' + stamp + "</updated>\n" +
    "    <summary>" + xml(w.summary.replace(/`/g, "")) + "</summary>\n  </entry>\n";
}
feed += "</feed>\n";
output(join(SITE_DIR, "public/feed.xml"), feed);
console.log("feed.xml written");

let llms =
  "# tsc-rs\n\n> A TypeScript compiler, type checker and language server written in Rust. Measured against the upstream TypeScript test suites with the real tsc baselines as the oracle. Not a drop-in tsc replacement yet.\n\n" +
  "Source: https://github.com/benfavre/ts-rs (MIT). Version " + version + ".\n\n" +
  "## Site\n\n- [Conformance report](" + ORIGIN + "/conformance): pass rates per lane, method, commands to reproduce\n" +
  "- [Progress](" + ORIGIN + "/progress): the verified wave log\n- [Playground](" + ORIGIN + "/playground): the compiler as WebAssembly in the browser\n";
for (const section of docsNav()) {
  llms += "\n## " + section.title + "\n\n";
  for (const item of section.items) llms += "- [" + item.title + "](" + ORIGIN + item.href + "): " + item.desc + "\n";
}
output(join(SITE_DIR, "public/llms.txt"), llms);
console.log("llms.txt written");

for (const [path, content] of outputs) {
  const temporary = path + ".sync-tmp";
  writeFileSync(temporary, content);
  renameSync(temporary, path);
}
console.log("Published source " + sourceCommit + "; measurement " + metrics.source_commit);
