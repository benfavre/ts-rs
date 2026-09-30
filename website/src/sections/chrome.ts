// Site chrome: navigation, footer, per-route metadata and structured data.
// Returned as inner HTML for the <nav> and <footer> elements the root layout
// owns (the nav must be a direct child of <body> for position: sticky).

import { SITE, REPO, REPO_BLOB, VERSION, navItems, githubIconHtml, dateLabel } from "../lib/site";
import { repoSource } from "../lib/repo.generated";
import { docsFind } from "../lib/docs-nav";
import { esc, icon } from "../lib/html";

function isActive(path: string, href: string): boolean {
  return path === href || path.startsWith(href + "/");
}

function logo(): string {
  return (
    '<a class="t-logo" href="/" aria-label="tsc-rs home"><span class="t-logo-mk" aria-hidden="true">rs</span>' +
    '<span class="t-logo-wm">tsc-rs</span></a>'
  );
}

function tools(): string {
  return (
    '<button class="t-tool t-search-btn" type="button" data-search aria-label="Search the docs">' + icon("search", 16) +
    "<span>Search docs</span><kbd>/</kbd></button>" +
    '<button class="t-tool t-theme" type="button" data-theme-toggle aria-label="Switch between light and dark theme">' +
    '<span class="ic-sun">' + icon("sun", 17) + '</span><span class="ic-moon">' + icon("moon", 17) + "</span></button>"
  );
}

export function navInnerHtml(path: string): string {
  const items = navItems();
  let links = "";
  let mobile = "";
  for (let i = 0; i < items.length; i++) {
    const cur = isActive(path, items[i].href) ? ' aria-current="page"' : "";
    links += '<a href="' + items[i].href + '"' + cur + ">" + esc(items[i].label) + "</a>";
    mobile += '<a href="' + items[i].href + '">' + esc(items[i].label) + "</a>";
  }
  return (
    '<div class="t-nav-in">' + logo() + '<span class="t-logo-v">v' + esc(VERSION) + "</span>" +
    '<div class="t-nav-links">' + links + "</div>" +
    '<div class="t-nav-right">' + tools() +
    '<a class="t-nav-gh" href="' + REPO + '" target="_blank" rel="noopener" aria-label="tsc-rs on GitHub">' + githubIconHtml(17) + "GitHub</a>" +
    '<a class="t-btn t-btn-primary t-btn-sm" href="/docs/installation">Get started</a></div>' +
    '<div class="t-nav-tools-m">' + tools() + "</div>" +
    '<button class="t-nav-toggle" id="t-nav-toggle" type="button" aria-label="Menu" aria-expanded="false" aria-controls="t-nav-mobile">' +
    icon("menu", 18) + "</button></div>" +
    '<div class="t-nav-mobile" id="t-nav-mobile"><div>' + mobile +
    '<a href="' + REPO + '" target="_blank" rel="noopener">GitHub</a>' +
    '<a class="t-btn t-btn-primary" href="/docs/installation">Get started</a></div></div>'
  );
}

function col(title: string, links: string[][]): string {
  let html = '<div class="t-foot-col"><div class="t-foot-h">' + esc(title) + "</div>";
  for (let i = 0; i < links.length; i++) {
    const ext = links[i][1].indexOf("http") === 0 ? ' target="_blank" rel="noopener"' : "";
    html += '<a href="' + links[i][1] + '"' + ext + ">" + esc(links[i][0]) + "</a>";
  }
  return html + "</div>";
}

export function footerInnerHtml(): string {
  return (
    '<div class="t-foot-top"><div class="t-foot-brand">' + logo() +
    "<p>A TypeScript compiler, type checker and language server written in Rust. MIT licensed and measured in the open.</p></div>" +
    col("Documentation", [
      ["Installation", "/docs/installation"],
      ["CLI reference", "/docs/cli"],
      ["Language server", "/docs/language-server"],
      ["WebAssembly", "/docs/wasm"],
    ]) +
    col("Project", [
      ["Conformance report", "/conformance"],
      ["Progress", "/progress"],
      ["Playground", "/playground"],
      ["GitHub", REPO],
      ["Changelog", REPO_BLOB + "CHANGELOG.md"],
      ["License", REPO_BLOB + "LICENSE"],
    ]) +
    col("bext", [
      ["bext engine", "https://bext.dev"],
      ["bext docs", "https://docs.bext.dev"],
      ["bext lite", "https://lite.bext.dev"],
    ]) +
    "</div>" +
    '<div class="t-foot-bottom"><div><span>Figures read from commit <a href="' + REPO + "/commit/" + esc(repoSource().commit) +
    '" target="_blank" rel="noopener"><code>' + esc(repoSource().commit) + "</code></a> of " + esc(dateLabel(repoSource().commitDate)) +
    '. <a href="/feed.xml">Wave feed</a></span></div>' +
    '<div><span>&copy; 2025-2026 Benjamin Favre and tsc-rs contributors. MIT License.</span>' +
    "<span>tsc-rs is an independent project. TypeScript is a trademark of Microsoft Corporation.</span></div></div>"
  );
}

export interface PageMeta {
  title: string;
  desc: string;
}

export function pageMeta(path: string): PageMeta {
  if (path === "/") {
    return {
      title: "tsc-rs: a TypeScript compiler written in Rust",
      desc: "Scanner, parser, binder, type checker, emitter and language server in one Cargo workspace, measured against the upstream TypeScript test suites.",
    };
  }
  if (path === "/conformance") {
    return {
      title: "Conformance report | tsc-rs",
      desc: "How close tsc-rs is to tsc: JavaScript emit, diagnostics, symbols and language server pass rates against the TypeScript test suites.",
    };
  }
  if (path === "/progress") {
    return {
      title: "Progress | tsc-rs",
      desc: "The verified wave log of tsc-rs: which diagnostics each wave added and how many TypeScript test cases newly pass.",
    };
  }
  if (path === "/playground") {
    return {
      title: "Playground | tsc-rs",
      desc: "Compile and type-check TypeScript in your browser with the tsc-rs WebAssembly build. Nothing is sent to a server.",
    };
  }
  if (path === "/docs") {
    return {
      title: "Documentation | tsc-rs",
      desc: "Install tsc-rs, compile a project, run the language server, and embed the compiler through the pipe, the daemon or WebAssembly.",
    };
  }
  const doc = docsFind(path);
  if (doc) return { title: doc.title + " | tsc-rs docs", desc: doc.desc };
  return { title: "Page not found | tsc-rs", desc: "A TypeScript compiler written in Rust." };
}

export function ogImage(meta: PageMeta): string {
  return (
    SITE + "/__bext/api/og?title=" + encodeURIComponent(meta.title.replace(" | tsc-rs docs", "").replace(" | tsc-rs", "")) +
    "&description=" + encodeURIComponent("tsc-rs, a TypeScript compiler written in Rust") +
    "&site_name=ts-rs.bext.dev&accent=2563c9"
  );
}

export function jsonLd(): string {
  const data = {
    "@context": "https://schema.org",
    "@type": "SoftwareSourceCode",
    name: "tsc-rs",
    description: "A TypeScript compiler, type checker and language server written in Rust.",
    url: SITE,
    codeRepository: REPO,
    programmingLanguage: "Rust",
    license: "https://opensource.org/licenses/MIT",
    version: VERSION,
    author: { "@type": "Person", name: "Benjamin Favre" },
  };
  return JSON.stringify(data).replace(/</g, "\\u003c");
}

export function notFoundHtml(): string {
  return (
    '<div class="t-wrap nf"><div class="nf-code" aria-hidden="true">404</div><h1>This page does not exist.</h1>' +
    "<p>The address may be mistyped, or the page may have moved. The docs search can usually find it.</p>" +
    '<div class="nf-actions"><a class="t-btn t-btn-primary" href="/">Back to the home page</a>' +
    '<a class="t-btn t-btn-ghost" href="/docs">Browse the docs</a>' +
    '<button class="t-btn t-btn-ghost" type="button" data-search>Search</button></div></div>'
  );
}

// Runs in <head> before first paint: marks JS as available and applies a stored
// theme choice so the page never flashes the wrong theme.
export function headScript(): string {
  return (
    "(function(){var d=document.documentElement;d.classList.add('js');" +
    "try{var t=localStorage.getItem('theme');if(t==='dark'||t==='light')d.setAttribute('data-theme',t)}catch(e){}})()"
  );
}
