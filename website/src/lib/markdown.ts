// Markdown to HTML for the docs: vendored marked, plus callouts, server-side
// syntax highlighting, copy buttons and heading anchors.
//
// The PRISM bundler does not resolve the bare "marked" specifier, but it does
// bundle a relative import, hence src/vendor/marked.esm.js. The parse function
// is looked up across module shapes and falls back to an escaped <pre> so a
// parser problem degrades to readable text instead of an error page.
import { expandMetricTokens } from "./metrics";
import * as markedMod from "../vendor/marked.esm.js";
import { langCfg, highlightCode, hlUnesc } from "./highlight";

function mdParse(src: string): string {
  const m: any = markedMod as any;
  const fn =
    (m && m.marked && typeof m.marked.parse === "function" && m.marked.parse) ||
    (m && typeof m.parse === "function" && m.parse) ||
    (m && m.default && typeof m.default.parse === "function" && m.default.parse) ||
    (m && typeof m.default === "function" && m.default) ||
    (m && typeof m.marked === "function" && m.marked) ||
    null;
  if (typeof fn === "function") return String(fn(String(src)));
  return "<pre>" + String(src).replace(/&/g, "&amp;").replace(/</g, "&lt;") + "</pre>";
}

// Base64url of the UTF-8 bytes of a string, written out because the render
// isolate is not guaranteed to have btoa. Matches what the playground decodes.
function base64Url(text: string): string {
  const abc = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
  const bytes: number[] = [];
  for (let i = 0; i < text.length; i++) {
    let c = text.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length) {
      c = 0x10000 + ((c - 0xd800) << 10) + (text.charCodeAt(i + 1) - 0xdc00);
      i++;
    }
    if (c < 0x80) bytes.push(c);
    else if (c < 0x800) bytes.push(0xc0 | (c >> 6), 0x80 | (c & 63));
    else if (c < 0x10000) bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    else bytes.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
  }
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const b0 = bytes[i];
    const b1 = i + 1 < bytes.length ? bytes[i + 1] : -1;
    const b2 = i + 2 < bytes.length ? bytes[i + 2] : -1;
    out += abc[b0 >> 2] + abc[((b0 & 3) << 4) | (b1 === -1 ? 0 : b1 >> 4)];
    if (b1 !== -1) out += abc[((b1 & 15) << 2) | (b2 === -1 ? 0 : b2 >> 6)];
    if (b2 !== -1) out += abc[b2 & 63];
  }
  return out;
}

// Wrap every fenced block: highlight it and add a copy button. TypeScript
// samples of a few lines or more also get a link that opens them in the playground.
function highlightBlocks(html: string): string {
  return html.replace(
    /<pre><code(?:\s+class="language-([A-Za-z0-9_+-]+)")?>([\s\S]*?)<\/code><\/pre>/g,
    (_m: string, lang: string | undefined, body: string) => {
      const cfg = langCfg(lang);
      const source = hlUnesc(body);
      const inner = cfg ? highlightCode(source, cfg) : body;
      const runnable = lang === "ts" && source.split("\n").length >= 4 && source.length < 4000;
      return (
        '<div class="doc-code' + (runnable ? " has-run" : "") + '" data-code>' +
        (runnable ? '<a class="t-copy doc-run" href="/playground#src=' + base64Url(source) + '">Open in playground</a>' : "") +
        '<button class="t-copy" type="button" data-copy aria-label="Copy code">Copy</button>' +
        '<pre tabindex="0"><code>' + inner + "</code></pre></div>"
      );
    },
  );
}

function headingText(inner: string): string {
  return hlUnesc(inner.replace(/<[^>]*>/g, "")).trim();
}

function slugify(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

function attrEsc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/"/g, "&quot;");
}

// Give h2 and h3 an id, a permalink and a data-title (read by the TOC script).
function addHeadingAnchors(html: string): string {
  const used: Record<string, boolean> = {};
  return html.replace(/<h([23])>([\s\S]*?)<\/h\1>/g, (_m: string, lvl: string, inner: string) => {
    const text = headingText(inner);
    const base = slugify(text) || "section";
    let id = base;
    let k = 2;
    while (used[id]) { id = base + "-" + k; k++; }
    used[id] = true;
    return (
      "<h" + lvl + ' id="' + id + '" data-title="' + attrEsc(text) + '">' + inner +
      '<a class="hanchor" href="#' + id + '" aria-label="Link to this section">#</a></h' + lvl + ">"
    );
  });
}

function stripFrontmatter(c: string): string {
  if (!c.startsWith("---")) return c;
  const e = c.indexOf("\n---", 3);
  return e === -1 ? c : c.slice(e + 4).trim();
}

function calloutLabel(variant: string): string {
  switch (variant) {
    case "tip": return "Tip";
    case "info": return "Info";
    case "warning": return "Warning";
    case "caution": return "Caution";
    default: return "Note";
  }
}

// Admonitions: a line of three colons and a kind, the body, then three colons.
function renderCallouts(c: string): string {
  return c.replace(/^:::(\w+)[^\n]*\n([\s\S]*?)\n:::[ \t]*$/gm, (_m: string, variant: string, body: string) => {
    const raw = String(variant).toLowerCase();
    const v = raw === "tip" || raw === "info" || raw === "warning" || raw === "caution" ? raw : "note";
    return (
      '<div class="callout callout-' + v + '"><div class="callout-label">' + calloutLabel(v) +
      '</div><div class="callout-body">' + mdParse(String(body).trim()) + "</div></div>"
    );
  });
}

// External links open in a new tab; internal ones are left alone.
function externalLinks(html: string): string {
  return html.replace(/<a href="(https?:\/\/[^"]+)"/g, '<a href="$1" target="_blank" rel="noopener"');
}

export function frontmatterValue(raw: string, key: string): string {
  if (!raw.startsWith("---")) return "";
  const end = raw.indexOf("\n---", 3);
  if (end === -1) return "";
  const lines = raw.slice(3, end).split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const at = line.indexOf(":");
    if (at === -1) continue;
    if (line.slice(0, at).trim() === key) {
      return line.slice(at + 1).trim().replace(/^"(.*)"$/, "$1");
    }
  }
  return "";
}

export function renderDoc(raw: string, metrics: any): string {
  raw = expandMetricTokens(raw, metrics);
  let s = stripFrontmatter(raw);
  s = renderCallouts(s);
  let html = mdParse(s);
  html = highlightBlocks(html);
  html = addHeadingAnchors(html);
  html = externalLinks(html);
  return html;
}
