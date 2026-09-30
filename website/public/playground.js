// ts-rs.bext.dev playground: runs the tsc-rs WebAssembly build in the page.
// compile() parses and emits; validate() binds and type-checks one file.
// Nothing leaves the browser.
//
// The editor is a textarea over a highlighted <pre>: the textarea holds the
// text, the caret and the selection, the <pre> paints the colours. highlight.js
// is the site's server-side tokenizer (src/lib/highlight.ts) built for the
// browser with "bun run build:client".

import { highlight } from "/highlight.js?v=2";

const EXAMPLES = {
  classes: `enum Priority {
  Low,
  High,
}

interface Job<T> {
  id: number;
  payload: T;
  priority: Priority;
}

class Queue<T> {
  private jobs: Job<T>[] = [];

  push(payload: T, priority = Priority.Low): Job<T> {
    const job: Job<T> = { id: this.jobs.length + 1, payload, priority };
    this.jobs.push(job);
    return job;
  }

  next(): Job<T> | undefined {
    return this.jobs.shift();
  }
}

const queue = new Queue<string>();
queue.push("compile", Priority.High);
export const first = queue.next();
`,
  errors: `interface User {
  id: number;
  name: string;
}

function greet(user: User): string {
  return "Hello, " + user.nmae;
}

const total: number = "42";
greet({ id: 1 });
`,
  modern: `class Counter {
  static #instances = 0;
  #count = 0;

  constructor(readonly label: string) {
    Counter.#instances++;
  }

  increment(by = 1): this {
    this.#count += by;
    return this;
  }

  get value(): number {
    return this.#count;
  }
}

async function* ticks(limit: number): AsyncGenerator<number> {
  for (let i = 0; i < limit; i++) {
    yield i;
  }
}

export async function run(): Promise<number> {
  const counter = new Counter("ticks");
  for await (const tick of ticks(3)) {
    counter.increment(tick ?? 0);
  }
  return counter.value;
}
`,
  jsx: `interface ButtonProps {
  label: string;
  tone?: "primary" | "ghost";
  onPress(): void;
}

export function Button({ label, tone = "primary", onPress }: ButtonProps) {
  return (
    <button className={"btn btn-" + tone} onClick={onPress}>
      {label}
    </button>
  );
}

export const Toolbar = () => (
  <nav>
    <Button label="Save" onPress={() => {}} />
    <Button label="Cancel" tone="ghost" onPress={() => {}} />
  </nav>
);
`,
};

const $ = (id) => document.getElementById(id);
const app = $("pg");
const editor = $("pg-editor");
const input = $("pg-input");
const hl = $("pg-hl");
const gutter = $("pg-gutter");
const output = $("pg-output");
const diags = $("pg-diags");
const statusEl = $("pg-status");
const timeEl = $("pg-time");
const posEl = $("pg-pos");
const targetEl = $("pg-target");
const moduleEl = $("pg-module");
const jsxEl = $("pg-jsx");
const checkEl = $("pg-check");
const strictEl = $("pg-strict");
const exampleEl = $("pg-example");
const shareEl = $("pg-share");
const fileTab = document.querySelector(".pg-pane .pg-tab");

let wasm = null;
let errorLines = new Set();

function setStatus(state, text) {
  statusEl.dataset.state = state;
  statusEl.textContent = text;
}

function toBase64Url(text) {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  bytes.forEach((b) => { bin += String.fromCharCode(b); });
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(data) {
  const bin = atob(data.replace(/-/g, "+").replace(/_/g, "/"));
  const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

function readHash() {
  const m = /^#src=([A-Za-z0-9_-]+)$/.exec(location.hash);
  if (!m) return null;
  try { return fromBase64Url(m[1]); } catch (e) { return null; }
}

// Tokens never nest, so a token that spans lines (a block comment, a template
// string) is closed and reopened at each newline. Every line can then be its
// own block.
function splitLines(html) {
  return html
    .replace(/<span class="(hl-[a-z]+)">([^<]*)<\/span>/g, (m, cls, inner) =>
      inner.indexOf("\n") === -1 ? m : '<span class="' + cls + '">' + inner.split("\n").join('</span>\n<span class="' + cls + '">') + "</span>")
    .split("\n");
}

function paint() {
  const lang = jsxEl.value === "off" ? "ts" : "tsx";
  const lines = splitLines(highlight(input.value, lang));
  let code = "";
  let nums = "";
  for (let i = 0; i < lines.length; i++) {
    const bad = errorLines.has(i + 1);
    code += '<span class="l' + (bad ? " e" : "") + '">' + (lines[i] || " ") + "</span>";
    nums += "<span" + (bad ? ' class="e"' : "") + ">" + (i + 1) + "</span>";
  }
  hl.innerHTML = code;
  gutter.innerHTML = nums;
  syncScroll();
}

function syncScroll() {
  hl.scrollTop = input.scrollTop;
  hl.scrollLeft = input.scrollLeft;
  gutter.scrollTop = input.scrollTop;
}

function showPosition() {
  const before = input.value.slice(0, input.selectionStart);
  const line = before.split("\n").length;
  const col = before.length - before.lastIndexOf("\n");
  posEl.textContent = "Ln " + line + ", Col " + col;
}

function jumpTo(line, column) {
  const lines = input.value.split("\n");
  let offset = 0;
  for (let i = 0; i < line - 1 && i < lines.length; i++) offset += lines[i].length + 1;
  const start = offset + Math.max(0, column - 1);
  const lineEnd = offset + (lines[line - 1] || "").length;
  // Select to the end of the word so the target is visible.
  const rest = input.value.slice(start, lineEnd);
  const word = /^[A-Za-z0-9_$]+/.exec(rest);
  input.focus();
  input.setSelectionRange(start, start + (word ? word[0].length : Math.min(1, rest.length)));
  const lineHeight = parseFloat(getComputedStyle(input).lineHeight) || 23;
  const top = (line - 1) * lineHeight;
  if (top < input.scrollTop || top > input.scrollTop + input.clientHeight - lineHeight * 2) {
    input.scrollTop = Math.max(0, top - input.clientHeight / 3);
  }
  syncScroll();
  showPosition();
}

function showDiagnostics(list, checked) {
  diags.textContent = "";
  errorLines = new Set(list.map((d) => d.line));
  if (!list.length) {
    const ok = document.createElement("div");
    ok.className = "pg-diag-none";
    ok.textContent = checked ? "No errors." : "No syntax errors (type checking is off).";
    diags.appendChild(ok);
    return;
  }
  list.forEach((d) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "pg-diag";
    const pos = document.createElement("span");
    pos.className = "pg-diag-pos";
    pos.textContent = d.line + ":" + d.column;
    const code = document.createElement("span");
    code.className = "pg-diag-code";
    code.textContent = d.code ? "TS" + d.code : "error";
    const msg = document.createElement("span");
    msg.textContent = d.message;
    row.append(pos, code, msg);
    row.addEventListener("click", () => jumpTo(d.line, d.column));
    diags.appendChild(row);
  });
}

function run() {
  if (!wasm) { paint(); return; }
  const source = input.value;
  const jsx = jsxEl.value;
  const opts = {
    target: targetEl.value,
    module: moduleEl.value,
    fileName: jsx === "off" ? "input.ts" : "input.tsx",
    strict: strictEl.checked,
  };
  if (jsx !== "off") opts.jsx = jsx;
  fileTab.textContent = opts.fileName;
  const t0 = performance.now();
  try {
    const result = wasm.compile(source, opts);
    let list = result.diagnostics || [];
    if (checkEl.checked) list = wasm.validate(source, opts) || [];
    const ms = performance.now() - t0;
    output.innerHTML = highlight(result.js || "", "js");
    timeEl.textContent = ms.toFixed(1) + " ms";
    showDiagnostics(list, checkEl.checked);
  } catch (err) {
    output.textContent = "// The compiler threw: " + (err && err.message ? err.message : String(err));
    timeEl.textContent = "";
    showDiagnostics([], false);
  }
  paint();
}

let timer = 0;
function schedule() {
  clearTimeout(timer);
  timer = setTimeout(run, 180);
}

input.addEventListener("input", () => { paint(); showPosition(); schedule(); });
input.addEventListener("scroll", syncScroll);
["keyup", "click", "focus"].forEach((ev) => input.addEventListener(ev, showPosition));
[targetEl, moduleEl, jsxEl, checkEl, strictEl].forEach((el) => el.addEventListener("change", run));

exampleEl.addEventListener("change", () => {
  const key = exampleEl.value;
  if (!EXAMPLES[key]) return;
  input.value = EXAMPLES[key];
  input.scrollTop = 0;
  jsxEl.value = key === "jsx" ? "react-jsx" : "off";
  // The JSX sample needs React's types, which the single-file checker has not loaded.
  checkEl.checked = key !== "jsx";
  history.replaceState(null, "", location.pathname);
  run();
});

shareEl.addEventListener("click", () => {
  const url = location.origin + location.pathname + "#src=" + toBase64Url(input.value);
  history.replaceState(null, "", url);
  if (navigator.clipboard) {
    navigator.clipboard.writeText(url).then(() => {
      const label = shareEl.textContent;
      shareEl.textContent = "Link copied";
      setTimeout(() => { shareEl.textContent = label; }, 1400);
    });
  }
});

// Tab inserts two spaces instead of leaving the editor. Escape then Tab leaves.
let escaped = false;
input.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { escaped = true; return; }
  if (e.key !== "Tab" || e.shiftKey || e.altKey || e.ctrlKey || e.metaKey || escaped) { escaped = false; return; }
  e.preventDefault();
  const start = input.selectionStart;
  const end = input.selectionEnd;
  input.value = input.value.slice(0, start) + "  " + input.value.slice(end);
  input.selectionStart = input.selectionEnd = start + 2;
  paint();
  schedule();
});

input.value = readHash() || EXAMPLES.classes;
editor.classList.add("hl-on");
paint();
showPosition();

(async function boot() {
  try {
    const mod = await import(app.dataset.wasm);
    await mod.default();
    wasm = mod;
    setStatus("ready", "tsc-rs " + mod.version() + " ready");
    run();
  } catch (err) {
    setStatus("error", "Could not load the compiler");
    output.textContent = "// WebAssembly failed to load: " + (err && err.message ? err.message : String(err));
  }
})();
