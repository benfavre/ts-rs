// Docs chrome: sidebar, landing page, pager and the not-found panel.

import { REPO } from "../lib/site";
import { docsNav, docsFlat } from "../lib/docs-nav";
import { esc, icon, button } from "../lib/html";

// Rendered open so the navigation is visible on desktop and without JS;
// public/site.js collapses it on small screens.
export function docsSidebarHtml(path: string): string {
  const nav = docsNav();
  let html = '<details class="d-disc" open><summary>Documentation</summary><nav aria-label="Documentation">';
  for (let i = 0; i < nav.length; i++) {
    html += '<div class="d-nav-sec"><div class="d-nav-h">' + esc(nav[i].title) + "</div><ul>";
    for (let j = 0; j < nav[i].items.length; j++) {
      const it = nav[i].items[j];
      const cur = it.href === path ? ' aria-current="page"' : "";
      html += '<li><a href="' + it.href + '"' + cur + ">" + esc(it.title) + "</a></li>";
    }
    html += "</ul></div>";
  }
  return html + "</nav></details>";
}

export function docsPagerHtml(path: string): string {
  const flat = docsFlat();
  let idx = -1;
  for (let i = 0; i < flat.length; i++) {
    if (flat[i].href === path) idx = i;
  }
  if (idx === -1) return "";
  const prev = idx > 0 ? flat[idx - 1] : null;
  const next = idx < flat.length - 1 ? flat[idx + 1] : null;
  if (!prev && !next) return "";
  return (
    '<nav class="d-pager" aria-label="Page navigation">' +
    (prev ? '<a class="prev" href="' + prev.href + '" rel="prev"><span class="dir">Previous</span><span class="ttl">' + esc(prev.title) + "</span></a>" : "") +
    (next ? '<a class="next" href="' + next.href + '" rel="next"><span class="dir">Next</span><span class="ttl">' + esc(next.title) + "</span></a>" : "") +
    "</nav>"
  );
}

export function docsIndexHtml(): string {
  const nav = docsNav();
  let cards = "";
  for (let i = 0; i < nav.length; i++) {
    let links = "";
    for (let j = 0; j < nav[i].items.length; j++) {
      links += '<li><a href="' + nav[i].items[j].href + '">' + esc(nav[i].items[j].title) + "</a></li>";
    }
    cards +=
      '<div class="t-card"><span class="t-card-ic">' + icon(nav[i].icon, 19) + "</span><h2>" + esc(nav[i].title) + "</h2>" +
      "<p>" + esc(nav[i].blurb) + '</p><ul class="d-cardlinks">' + links + "</ul></div>";
  }
  return (
    '<div class="d-hero"><span class="t-eyebrow">Documentation</span><h1>tsc-rs documentation</h1>' +
    "<p>Build the compiler, point it at a project, wire it into an editor, or embed it in your own tooling. " +
    "Every page here is written against the source in the repository.</p>" +
    '<div class="d-hero-actions">' +
    button({ href: "/docs/installation", label: "Install tsc-rs", kind: "primary", icon: "arrow" }) +
    button({ href: "/docs/cli", label: "CLI reference", kind: "ghost" }) +
    "</div></div>" +
    '<div class="d-grid">' + cards + "</div>"
  );
}

export function docNotFoundHtml(slug: string): string {
  return (
    '<div class="d-nf"><h1>Page not found</h1><p>There is no documentation page at <code>/docs/' + esc(slug) +
    "</code>. It may have moved or been renamed. Use the sidebar to find what you need.</p>" +
    button({ href: "/docs", label: "Back to the docs home", kind: "ghost" }) + "</div>"
  );
}

export function docSourceHtml(slug: string): string {
  return (
    '<p class="doc-src">Found a mistake on this page? <a href="' + REPO + "/issues/new?title=" +
    encodeURIComponent("docs: " + slug) + '" target="_blank" rel="noopener">Open an issue on GitHub</a>.</p>'
  );
}
