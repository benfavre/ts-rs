// Playground page shell. The editor is wired by public/playground.js, which
// loads the WebAssembly build from public/playground/pkg.

import { VERSION, REPO_BLOB } from "../lib/site";
import { esc } from "../lib/html";

function options(values: string[], selected: string): string {
  let html = "";
  for (let i = 0; i < values.length; i++) {
    html += '<option value="' + esc(values[i]) + '"' + (values[i] === selected ? " selected" : "") + ">" + esc(values[i]) + "</option>";
  }
  return html;
}

export function playgroundHtml(): string {
  const targets = ["esnext", "es2022", "es2020", "es2017", "es2015", "es5"];
  const modules = ["esnext", "commonjs", "amd", "umd", "system"];
  const jsx = ["off", "react-jsx", "react", "preserve"];
  return (
    '<div class="pg">' +
    '<header class="pg-head"><div><span class="t-eyebrow">Playground</span>' +
    "<h1>Compile TypeScript in your browser.</h1>" +
    "<p>The tsc-rs parser, type checker and emitter, compiled to WebAssembly. Edit the code on the left; the JavaScript and the diagnostics update as you type. Click a diagnostic to jump to it. Nothing is sent to a server.</p></div></header>" +
    '<div class="pg-app" id="pg" data-wasm="/playground/pkg/tsc-rs-wasm.js?v=' + esc(VERSION) + '">' +
    '<div class="pg-bar">' +
    '<label class="pg-field">example <select id="pg-example">' +
    '<option value="classes" selected>classes and enums</option><option value="modern">private fields, async</option>' +
    '<option value="jsx">JSX</option><option value="errors">type errors</option></select></label>' +
    '<label class="pg-field">target <select id="pg-target">' + options(targets, "es2022") + "</select></label>" +
    '<label class="pg-field">module <select id="pg-module">' + options(modules, "esnext") + "</select></label>" +
    '<label class="pg-field">jsx <select id="pg-jsx">' + options(jsx, "off") + "</select></label>" +
    '<label class="pg-check"><input type="checkbox" id="pg-check" checked> type-check</label>' +
    '<label class="pg-check"><input type="checkbox" id="pg-strict" checked> strict</label>' +
    '<button class="pg-btn" type="button" id="pg-share">Copy link</button>' +
    '<span class="pg-status" id="pg-status" data-state="loading" role="status">Loading compiler</span>' +
    "</div>" +
    '<div class="pg-panes">' +
    '<div class="pg-pane"><div class="pg-tabs"><span class="pg-tab on">input.ts</span>' +
    '<span class="pg-tab-info" id="pg-pos"></span></div>' +
    '<div class="pg-editor" id="pg-editor"><div class="pg-gutter" id="pg-gutter" aria-hidden="true"></div>' +
    '<div class="pg-code"><pre class="pg-hl" id="pg-hl" aria-hidden="true"></pre>' +
    '<textarea class="pg-input" id="pg-input" spellcheck="false" autocapitalize="off" autocomplete="off" autocorrect="off" wrap="off" aria-label="TypeScript source"></textarea></div></div></div>' +
    '<div class="pg-pane"><div class="pg-tabs"><span class="pg-tab on">output.js</span>' +
    '<span class="pg-tab-info" id="pg-time"></span></div>' +
    '<pre class="pg-output" tabindex="0" aria-label="Compiled JavaScript"><code id="pg-output">// Loading the compiler (about 1.2 MB compressed)...</code></pre>' +
    '<div class="pg-diags" id="pg-diags" aria-live="polite"></div></div>' +
    "</div></div>" +
    '<p class="pg-foot">This build checks one file on its own, with built-in fallback declarations in place of the full <code>lib.d.ts</code> ' +
    "and no <code>@types</code> packages, so code that depends on library or framework types can report errors the command line compiler would not. " +
    'The bindings are the <a class="t-link" href="' + REPO_BLOB + 'crates/tsc_rs_wasm/src/lib.rs" target="_blank" rel="noopener">tsc_rs_wasm</a> crate; ' +
    'see <a class="t-link" href="/docs/wasm">the WebAssembly guide</a> to use it in your own page.</p>' +
    "</div>"
  );
}
