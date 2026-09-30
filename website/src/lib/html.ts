// HTML string builders shared by every section.
//
// Sections are assembled as plain strings and handed to the page through
// dangerouslySetInnerHTML. That keeps the markup opaque to the PRISM static
// fold pass and means every value that reaches the page goes through esc().

import { iconPath } from "./site";
import { highlight } from "./highlight";

export function esc(s: string): string {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function icon(name: string, size: number): string {
  return (
    '<svg class="ic" width="' + size + '" height="' + size + '" viewBox="0 0 24 24" fill="none" stroke="currentColor" ' +
    'stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="' +
    iconPath(name) + '"/></svg>'
  );
}

export interface ButtonOpts {
  href: string;
  label: string;
  kind: string;
  icon?: string;
  external?: boolean;
}

export function button(o: ButtonOpts): string {
  const ext = o.external ? ' target="_blank" rel="noopener"' : "";
  const ic = o.icon ? icon(o.icon, 16) : "";
  return '<a class="t-btn t-btn-' + o.kind + '" href="' + esc(o.href) + '"' + ext + ">" + esc(o.label) + ic + "</a>";
}

// One dark code pane: a file tab and highlighted source.
export function codePane(file: string, lang: string, source: string): string {
  return (
    '<div class="t-pane' + (lang === "text" ? " t-pane-wrap" : "") + '"><div class="t-pane-tab"><span>' + esc(file) + "</span></div>" +
    '<pre tabindex="0"><code>' + highlight(source, lang) + "</code></pre></div>"
  );
}

// A standalone dark code block with a copy button (marketing pages).
export function codeBlock(label: string, lang: string, source: string): string {
  return (
    '<div class="t-code" data-code>' +
    '<div class="t-code-bar"><span class="t-code-label">' + esc(label) + "</span>" +
    '<button class="t-copy" type="button" data-copy aria-label="Copy code">Copy</button></div>' +
    '<pre tabindex="0"><code>' + highlight(source, lang) + "</code></pre></div>"
  );
}

export interface SectionHead {
  eyebrow: string;
  title: string;
  lead?: string;
}

export function sectionHead(h: SectionHead): string {
  return (
    '<div class="t-head rv"><span class="t-eyebrow">' + esc(h.eyebrow) + "</span>" +
    "<h2>" + esc(h.title) + "</h2>" +
    (h.lead ? "<p>" + esc(h.lead) + "</p>" : "") +
    "</div>"
  );
}

// A horizontal pass-rate meter. The width comes from a custom property so the
// stylesheet can animate it in.
export function meter(percent: number, tone: string): string {
  const p = Math.max(0, Math.min(100, percent));
  return (
    '<div class="t-meter t-meter-' + tone + '" role="img" aria-label="' + p + ' percent">' +
    '<div class="t-meter-fill" style="--v:' + p + '%"></div></div>'
  );
}
