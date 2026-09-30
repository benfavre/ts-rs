// Home page, assembled as one HTML string (see lib/html.ts for why).
//
// Every figure comes from lib/site.ts. The code samples are real: queue.js and
// the preserve-mode output were produced by tsc-rs 0.4.1 from the inputs shown.

import {
  REPO, REPO_BLOB, VERSION,
  currentLanes, rustTests, dateLabel, lspRows, pipelineCrates, supportCrates, pct, pctLabel, num,
} from "../lib/site";
import { esc, icon, button, codePane, codeBlock, sectionHead, meter } from "../lib/html";
import { latestWavesHtml } from "./progress";

function sampleInput(): string {
  return [
    "export enum Priority {",
    "  Low,",
    "  High,",
    "}",
    "",
    "interface Job<T> {",
    "  id: number;",
    "  payload: T;",
    "  priority: Priority;",
    "}",
    "",
    "export class Queue<T> {",
    "  private jobs: Job<T>[] = [];",
    "",
    "  push(payload: T, priority = Priority.Low): Job<T> {",
    "    const job: Job<T> = { id: this.jobs.length + 1, payload, priority };",
    "    this.jobs.push(job);",
    "    return job;",
    "  }",
    "",
    "  next(): Job<T> | undefined {",
    "    return this.jobs.shift();",
    "  }",
    "}",
  ].join("\n");
}

function sampleOutput(): string {
  return [
    "export var Priority;",
    "(function (Priority) {",
    '    Priority[Priority["Low"] = 0] = "Low";',
    '    Priority[Priority["High"] = 1] = "High";',
    "})(Priority || (Priority = {}));",
    "export class Queue {",
    "    jobs = [];",
    "    push(payload, priority = Priority.Low) {",
    "        const job = { id: this.jobs.length + 1, payload, priority };",
    "        this.jobs.push(job);",
    "        return job;",
    "    }",
    "    next() {",
    "        return this.jobs.shift();",
    "    }",
    "}",
  ].join("\n");
}

interface Demo {
  tab: string;
  cmd: string;
  status: string;
  ok: boolean;
  left: string[];
  right: string[];
}

// Every right-hand pane is real tsc-rs 0.4.1 output for the left-hand input.
function demos(): Demo[] {
  return [
    {
      tab: "Compile",
      cmd: "tsc-rs --strict --target es2022 --module esnext queue.ts",
      status: "0 errors",
      ok: true,
      left: ["queue.ts", "ts", sampleInput()],
      right: ["queue.js", "js", sampleOutput()],
    },
    {
      tab: "Type-check",
      cmd: "tsc-rs --noEmit --strict user.ts",
      status: "2 errors, exit 1",
      ok: false,
      left: ["user.ts", "ts", [
        "interface User {",
        "  id: number;",
        "  name: string;",
        "}",
        "",
        "function greet(user: User): string {",
        '  return "Hello, " + user.nmae;',
        "}",
        "",
        'const total: number = "42";',
      ].join("\n")],
      right: ["terminal", "text", [
        "user.ts(7,27): error TS2339: Property 'nmae' does not exist on type 'User'.",
        "user.ts(10,7): error TS2322: Type 'string' is not assignable to type 'number'.",
        "",
        "Found 2 errors in 1 file.",
      ].join("\n")],
    },
    {
      tab: "JSX",
      cmd: "tsc-rs --transpileOnly --jsx react-jsx --module esnext button.tsx",
      status: "transpile only",
      ok: true,
      left: ["button.tsx", "tsx", [
        "interface ButtonProps {",
        "  label: string;",
        '  tone?: "primary" | "ghost";',
        "  onPress(): void;",
        "}",
        "",
        'export function Button({ label, tone = "primary", onPress }: ButtonProps) {',
        "  return (",
        '    <button className={"btn btn-" + tone} onClick={onPress}>',
        "      {label}",
        "    </button>",
        "  );",
        "}",
      ].join("\n")],
      right: ["button.js", "js", [
        'import { jsx as _jsx } from "react/jsx-runtime";',
        'export function Button({ label, tone = "primary", onPress }) {',
        '    return (_jsx("button", { className: "btn btn-" + tone, onClick: onPress, children: label }));',
        "}",
      ].join("\n")],
    },
    {
      tab: "Pipe",
      cmd: "tsc-rs --pipe",
      status: "JSON lines",
      ok: true,
      left: ["stdin", "json", [
        "{",
        '  "id": "1",',
        '  "file": "button.tsx",',
        '  "source": "export const Save = () => <Button label=\\"Save\\" />;",',
        '  "options": { "module": "esnext", "jsx": "react-jsx" }',
        "}",
      ].join("\n")],
      right: ["stdout", "json", [
        "{",
        '  "id": "1",',
        '  "ok": true,',
        '  "output": "import { jsx as _jsx } from \\"react/jsx-runtime\\";\\nexport const Save = () => _jsx(Button, { label: \\"Save\\" });\\n",',
        '  "elapsed_ms": 0,',
        '  "exports": ["Save"]',
        "}",
      ].join("\n")],
    },
  ];
}

function demoWindow(): string {
  const list = demos();
  let tabs = "";
  let panels = "";
  for (let i = 0; i < list.length; i++) {
    const d = list[i];
    tabs +=
      '<button class="t-win-tab" type="button" role="tab" id="demo-tab-' + i + '" aria-controls="demo-panel-' + i + '" aria-selected="' +
      (i === 0 ? "true" : "false") + '"' + (i === 0 ? "" : ' tabindex="-1"') + ">" + esc(d.tab) + "</button>";
    panels +=
      '<div class="t-win-panel" role="tabpanel" id="demo-panel-' + i + '" aria-labelledby="demo-tab-' + i + '"' + (i === 0 ? "" : " hidden") + ">" +
      '<div class="t-win-cmdline"><span class="t-win-cmd">$ <b>' + esc(d.cmd) + "</b></span>" +
      '<span class="t-win-ok' + (d.ok ? "" : " t-win-bad") + '">' + esc(d.status) + "</span></div>" +
      '<div class="t-win-panes">' + codePane(d.left[0], d.left[1], d.left[2]) + codePane(d.right[0], d.right[1], d.right[2]) + "</div></div>";
  }
  return (
    '<div class="t-win rv" data-tabs><div class="t-win-bar"><i></i><i></i><i></i>' +
    '<div class="t-win-tabs" role="tablist" aria-label="Examples">' + tabs + "</div></div>" + panels + "</div>"
  );
}

function hero(): string {
  return (
    '<section class="t-hero"><div class="t-hero-grid" aria-hidden="true"></div><div class="t-wrap">' +
    '<div class="t-hero-copy">' +
    '<span class="t-pill"><span class="t-pill-dot"></span><b>v' + esc(VERSION) + "</b> &middot; open source, MIT licensed</span>" +
    "<h1>A TypeScript compiler, written in <em>Rust</em>.</h1>" +
    '<p class="t-hero-lead">Scanner, parser, binder, type checker, emitter and language server in one Cargo workspace. ' +
    "Every change is measured against the upstream TypeScript test suites, with the real tsc baselines as the oracle.</p>" +
    '<div class="t-hero-actions">' +
    button({ href: "/docs/installation", label: "Get started", kind: "primary", icon: "arrow" }) +
    button({ href: "/playground", label: "Try it in the browser", kind: "ghost" }) +
    "</div>" +
    '<div class="t-hero-meta">' +
    "<span>" + icon("check", 15) + "100% of default JavaScript emit baselines</span>" +
    "<span>" + icon("check", 15) + "LSP server and VS Code extension</span>" +
    "<span>" + icon("check", 15) + "Runs in the browser as WebAssembly</span>" +
    "</div></div>" +
    demoWindow() +
    "</div></section>"
  );
}

function scoreCell(value: string, unit: string, label: string, sub: string, percent: number, tone: string): string {
  return (
    '<div class="t-score-cell"><div class="t-score-n">' + esc(value) + (unit ? "<span>" + esc(unit) + "</span>" : "") + "</div>" +
    '<div class="t-score-l">' + esc(label) + "</div>" +
    '<div class="t-score-s">' + esc(sub) + "</div>" + meter(percent, tone) + "</div>"
  );
}

function scoreboard(): string {
  const lanes = currentLanes();
  const emit = lanes[0];
  const diag = lanes[1];
  const emitPass = emit.compiler.passed + emit.conformance.passed;
  const emitTotal = emit.compiler.total + emit.conformance.total;
  const diagPass = diag.compiler.passed + diag.conformance.passed;
  const diagTotal = diag.compiler.total + diag.conformance.total;
  let lspPass = 0;
  let lspTotal = 0;
  const rows = lspRows();
  const tests = rustTests();
  for (let i = 0; i < rows.length; i++) {
    lspPass += rows[i].passed;
    lspTotal += rows[i].passed + rows[i].failed;
  }
  return (
    '<section class="t-sec" style="padding-top:2rem"><div class="t-wrap">' +
    sectionHead({
      eyebrow: "Status at a glance",
      title: "Measured, not claimed.",
      lead: "tsc-rs runs the TypeScript project's own compiler, conformance and fourslash suites. A case passes only when the output matches what tsc produced. Skipped cases are never counted as passes.",
    }) +
    '<div class="t-score rv">' +
    scoreCell(pctLabel(emitPass, emitTotal).replace("%", ""), "%", "JavaScript emit",
      num(emitPass) + " of " + num(emitTotal) + " baselines match the tsc .js output byte for byte.", pct(emitPass, emitTotal), "pass") +
    scoreCell(pctLabel(diagPass, diagTotal).replace("%", ""), "%", "Diagnostics",
      num(diagPass) + " of " + num(diagTotal) + " cases match every message, position and code.", pct(diagPass, diagTotal), "accent") +
    scoreCell(pctLabel(lspPass, lspTotal).replace("%", ""), "%", "Language server",
      num(lspPass) + " of " + num(lspTotal) + " fourslash checks across five operations.", pct(lspPass, lspTotal), "accent") +
    scoreCell(num(tests.passed), "", "Rust tests passing",
      "Zero failures and " + tests.ignored + " ignored across the workspace, including documentation tests.", 100, "muted") +
    "</div>" +
    '<p class="t-note">Emit and diagnostics as of the wave of ' + esc(dateLabel(emit.measured)) + ", measured with the cache disabled. Language server and Rust tests were verified in the same published snapshot. " +
    '<a href="/conformance">Read the full conformance report</a></p>' +
    '<div class="nw-wrap rv">' + latestWavesHtml() + '<a class="nw-all" href="/progress">All waves' + icon("arrow", 15) + "</a></div>" +
    "</div></section>"
  );
}

function card(ic: string, title: string, body: string, cmd: string): string {
  return (
    '<div class="t-card"><span class="t-card-ic">' + icon(ic, 19) + "</span><h3>" + esc(title) + "</h3><p>" + esc(body) + "</p>" +
    '<div class="t-card-cmd"><span>$ </span>' + esc(cmd) + "</div></div>"
  );
}

function toolchain(): string {
  return (
    '<section class="t-sec t-sec-tint"><div class="t-wrap">' +
    sectionHead({
      eyebrow: "One workspace",
      title: "The whole toolchain, not just a transpiler.",
      lead: "Most Rust TypeScript tools stop at stripping types. tsc-rs implements the compiler end to end, then adds the modes that tooling needs.",
    }) +
    '<div class="t-cards rv">' +
    card("code", "Compiler and emitter",
      "JavaScript, source map and declaration emit for ES3 through ESNext, with CommonJS, ESM, AMD, UMD and SystemJS module output.",
      "tsc-rs -p tsconfig.json") +
    card("shield", "Type checker",
      "Inference, narrowing, relations and diagnostics that reproduce tsc's messages, positions and codes. Incomplete, and measured case by case.",
      "tsc-rs --noEmit --strict") +
    card("cursor", "Language server",
      "Diagnostics, hover, go-to-definition, references, completions, signature help, rename and formatting over stdio, plus a VS Code extension.",
      "tsc-rs --lsp") +
    card("layers", "Preserve modes",
      "Keep type annotations, comments or the original layout in the emitted JavaScript for runtime type libraries and refactoring tools.",
      "tsc-rs --preserveTypeAnnotations") +
    card("zap", "Pipe and daemon modes",
      "A persistent transpile pipe and a type-check daemon that speak newline-delimited JSON, built for bundlers, editors and agents.",
      "tsc-rs --check-pipe -p tsconfig.json") +
    card("search", "Monorepo analysis",
      "Parser-level import graph audits: find unreachable or type-only externals and the files with the widest dependency fan-out.",
      "tsc-rs analyze dep-graph --entry src") +
    "</div></div></section>"
  );
}

function pipeline(): string {
  const stages = pipelineCrates();
  let cells = "";
  for (let i = 0; i < stages.length; i++) {
    cells +=
      '<div class="t-stage"><span class="t-stage-n">0' + (i + 1) + "</span><code>" + esc(stages[i].name) + "</code><p>" +
      esc(stages[i].role) + "</p></div>";
  }
  const support = supportCrates();
  let items = "";
  for (let i = 0; i < support.length; i++) {
    items += "<li><code>" + esc(support[i].name) + "</code><span>" + esc(support[i].role) + "</span></li>";
  }
  return (
    '<section class="t-sec"><div class="t-wrap">' +
    sectionHead({
      eyebrow: "Architecture",
      title: "Five stages, one crate each.",
      lead: "The compiler is a plain pipeline. Each stage is its own crate with its own tests, and the tooling crates build on the same five.",
    }) +
    '<div class="rv"><div class="t-pipe">' + cells + "</div>" +
    '<div class="t-pipe-io"><span>.ts .tsx .js</span><span>.js .d.ts .js.map</span></div>' +
    '<ul class="t-crates">' + items + "</ul></div>" +
    '<p class="t-note"><a href="/docs/architecture">How the crates fit together</a></p>' +
    "</div></section>"
  );
}

function bar(name: string, ms: number, us: boolean): string {
  return (
    '<div class="t-bar' + (us ? " t-bar-us" : "") + '"><span class="t-bar-nm">' + esc(name) + "</span>" +
    '<div class="t-bar-track"><div class="t-bar-fill" style="--v:' + ms + '%"></div></div>' +
    '<span class="t-bar-val">' + ms.toFixed(1) + " ms</span></div>"
  );
}

function performance(): string {
  return (
    '<section class="t-sec t-sec-tint"><div class="t-wrap">' +
    sectionHead({
      eyebrow: "Performance",
      title: "In the same league as the fastest native parsers.",
      lead: "Historical same-session comparison on typescript.js, a 7.8 MB fixture. Lower is better. The benchmarks live in crates/tsc_rs_bench so you can run them yourself.",
    }) +
    '<div class="t-bench rv">' +
    '<div class="t-bench-box"><h3>Parse</h3><p>Source text to AST.</p>' + bar("tsc-rs", 44.5, true) + bar("oxc", 35.2, false) + "</div>" +
    '<div class="t-bench-box"><h3>Parse and semantic analysis</h3><p>AST plus binding and symbol resolution.</p>' + bar("tsc-rs", 96.0, true) + bar("oxc", 97.9, false) + "</div>" +
    "</div>" +
    '<div class="t-facts rv">' +
    '<div class="t-fact"><b>2 to 4x</b><span>Transpile-only emit with <code class="t-inline">--fast-emit</code> skips type checking and formatting normalization.</span></div>' +
    '<div class="t-fact"><b>Every file</b><span>tsc-rs parses faster than SWC on every file in the benchmark set.</span></div>' +
    '<div class="t-fact"><b>48 MB</b><span>Peak memory while checking 1,000 compiler diagnostic cases on one thread.</span></div>' +
    "</div>" +
    '<p class="t-note">oxc parses faster; tsc-rs is ahead once semantic analysis is included. Method and raw timings are in the ' +
    '<a href="' + REPO_BLOB + 'docs/verified-waves.md" target="_blank" rel="noopener">verified waves log</a>.</p>' +
    "</div></section>"
  );
}

function preserve(): string {
  const input = [
    'const raw = JSON.parse("{}") as { port: number };',
    'const config = { port: raw.port, host: "localhost" } satisfies Record<string, string | number>;',
    "export const port = <number>config.port;",
  ].join("\n");
  const plain = [
    'const raw = JSON.parse("{}");',
    'const config = { port: raw.port, host: "localhost" };',
    "export const port = config.port;",
  ].join("\n");
  return (
    '<section class="t-sec"><div class="t-wrap"><div class="t-split">' +
    '<div><div class="t-head rv"><span class="t-eyebrow">Tooling first</span>' +
    "<h2>Keep the types when you need them.</h2>" +
    "<p>tsc throws all type information away. tsc-rs can carry some of it into the output, so downstream tools see what the author wrote.</p></div>" +
    '<ul class="t-list rv">' +
    "<li>" + icon("check", 16) + "<span><b>Type annotations.</b> Keep <code>as</code>, <code>satisfies</code> and angle-bracket assertions in the emitted JavaScript.</span></li>" +
    "<li>" + icon("check", 16) + "<span><b>Comments.</b> Keep comments even when <code>removeComments</code> is set.</span></li>" +
    "<li>" + icon("check", 16) + "<span><b>Whitespace.</b> Keep the original layout (partial).</span></li>" +
    "</ul></div>" +
    '<div class="rv">' +
    codeBlock("tsc-rs --preserveTypeAnnotations cast.ts", "ts", input) +
    codeBlock("tsc-rs cast.ts", "js", plain) +
    "</div></div></div></section>"
  );
}

function production(): string {
  return (
    '<section class="t-sec t-sec-tint"><div class="t-wrap">' +
    sectionHead({
      eyebrow: "In production",
      title: "The TypeScript front end of bext.",
      lead: "tsc-rs is not a lab project. It compiles every page and module served by the bext engine, including the one you are reading.",
    }) +
    '<div class="t-prod rv">' +
    '<div class="t-prod-cell"><h3>PRISM compiles with it</h3><p>The bext PRISM renderer uses the tsc-rs parser, AST and emitter crates to turn TSX routes into the JavaScript it runs in V8.</p></div>' +
    '<div class="t-prod-cell"><h3>One long-lived process</h3><p>The persistent pipe transforms modules one JSON line at a time and reports imports, dynamic imports and exports with each result.</p></div>' +
    '<div class="t-prod-cell"><h3>Errors fail the build</h3><p>Parser diagnostics are reported even in transpile-only mode, so a broken source file stops the compile instead of shipping.</p></div>' +
    "</div></div></section>"
  );
}

function limits(): string {
  return (
    '<section class="t-sec"><div class="t-wrap">' +
    sectionHead({
      eyebrow: "Limitations",
      title: "What it is not, yet.",
      lead: "The tables on this site exist so you can judge the gaps for yourself. These are the ones that matter most today.",
    }) +
    '<div class="t-limits rv">' +
    '<div class="t-limit"><h3>Not a drop-in tsc replacement</h3><p>Diagnostics, declaration emit and expanded option variants are not at parity, and the CLI surface is a subset.</p></div>' +
    '<div class="t-limit"><h3>The type checker is incomplete</h3><p>Hover types and some diagnostics differ from tsc. Expect missing errors and some false ones on advanced code.</p></div>' +
    '<div class="t-limit"><h3>Declarations and symbols are early</h3><p>.d.ts output and the symbol baselines are the least mature lanes of the project.</p></div>' +
    "</div></div></section>"
  );
}

function band(): string {
  return (
    '<section class="t-wrap"><div class="t-band rv">' +
    "<h2>Build it, point it at your project, read the numbers.</h2>" +
    "<p>Install from source in one command, or compile a snippet in the playground without installing anything.</p>" +
    '<div class="t-band-actions">' +
    button({ href: "/docs/installation", label: "Install tsc-rs", kind: "light", icon: "arrow" }) +
    button({ href: REPO, label: "View on GitHub", kind: "ondark", external: true }) +
    "</div></div></section>"
  );
}

export function homeHtml(): string {
  return hero() + scoreboard() + toolchain() + pipeline() + performance() + preserve() + production() + limits() + band();
}
