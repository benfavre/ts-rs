// Docs information architecture: the sidebar, the landing cards, the pager
// order and the page titles all read from this one list.
//
// Each href maps to content/docs/<slug>.md, which the catch-all route reads at
// render time. To add a page: write the Markdown file, add an entry here, and
// add the URL to public/sitemap.xml.

export interface DocsNavItem {
  title: string;
  href: string;
  desc: string;
}

export interface DocsNavSection {
  title: string;
  blurb: string;
  icon: string;
  items: DocsNavItem[];
}

export function docsNav(): DocsNavSection[] {
  return [
    {
      title: "Getting started",
      blurb: "What tsc-rs is, how to build it, and a first compile.",
      icon: "play",
      items: [
        { title: "Introduction", href: "/docs/introduction", desc: "What tsc-rs is, what it is for, and how far along it is." },
        { title: "Installation", href: "/docs/installation", desc: "Build tsc-rs from source with Cargo, or install the npm package." },
        { title: "Quick start", href: "/docs/quick-start", desc: "Compile a file, a project, and type-check without emitting." },
        { title: "FAQ", href: "/docs/faq", desc: "Short answers: what tsc-rs is good for today and how it relates to tsc, SWC and oxc." },
      ],
    },
    {
      title: "Compiler",
      blurb: "The command line, projects, JSX and the preserve modes.",
      icon: "terminal",
      items: [
        { title: "CLI reference", href: "/docs/cli", desc: "Every tsc-rs command, flag and environment variable." },
        { title: "tsconfig support", href: "/docs/tsconfig", desc: "The compilerOptions that tsc-rs reads and applies." },
        { title: "Projects and watch", href: "/docs/projects", desc: "Project builds, output layout, incremental builds, project references and watch mode." },
        { title: "JSX", href: "/docs/jsx", desc: "JSX modes, factories and import sources, with real output and the differences from tsc." },
        { title: "Preserve modes", href: "/docs/preserve-modes", desc: "Keep type annotations, comments or layout in the emitted JavaScript." },
        { title: "Limitations", href: "/docs/limitations", desc: "Where tsc-rs differs from tsc today." },
      ],
    },
    {
      title: "Editor",
      blurb: "The language server and the VS Code extension.",
      icon: "cursor",
      items: [
        { title: "Language server", href: "/docs/language-server", desc: "Run tsc-rs as an LSP server from any editor." },
        { title: "VS Code extension", href: "/docs/vscode", desc: "Build, install and configure the tsc-rs extension." },
      ],
    },
    {
      title: "Embedding",
      blurb: "Drive tsc-rs from a bundler, an agent, a browser, Node or Rust.",
      icon: "box",
      items: [
        { title: "Transpile pipe", href: "/docs/pipe", desc: "A persistent transpile-only process that speaks JSON lines." },
        { title: "Type-check daemon", href: "/docs/check-pipe", desc: "A long-running type checker for editors and agents." },
        { title: "WebAssembly", href: "/docs/wasm", desc: "Compile and type-check TypeScript in the browser or at the edge." },
        { title: "Rust library API", href: "/docs/library", desc: "Drive the parser, binder, checker and emitter, or the query engine, from Rust." },
        { title: "npm package", href: "/docs/npm", desc: "Install the prebuilt binary through npm." },
      ],
    },
    {
      title: "Analysis",
      blurb: "Parser-level audits for TypeScript monorepos.",
      icon: "search",
      items: [
        { title: "tsc-rs analyze", href: "/docs/analyze", desc: "Audit externals and dependency fan-out from the import graph." },
      ],
    },
    {
      title: "Project",
      blurb: "How the workspace is organised and how to work on it.",
      icon: "git",
      items: [
        { title: "Architecture", href: "/docs/architecture", desc: "The crates and how a file moves through them." },
        { title: "Testing and the harness", href: "/docs/testing", desc: "How tsc-rs is measured against the TypeScript test suites." },
        { title: "Contributing", href: "/docs/contributing", desc: "Local setup, workflow and the pull request checklist." },
      ],
    },
  ];
}

export function docsFlat(): DocsNavItem[] {
  const out: DocsNavItem[] = [];
  const nav = docsNav();
  for (let i = 0; i < nav.length; i++) {
    for (let j = 0; j < nav[i].items.length; j++) out.push(nav[i].items[j]);
  }
  return out;
}

export function docsFind(path: string): DocsNavItem | null {
  const flat = docsFlat();
  for (let i = 0; i < flat.length; i++) {
    if (flat[i].href === path) return flat[i];
  }
  return null;
}
