import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

interface ExportGraphNode {
  id: string;
  name: string;
  rel?: string;
  dir?: string;
  external: boolean;
  exports?: number;
  edgesIn?: number;
  project?: string;
}

interface ExportGraphEdge {
  source: string;
  target: string;
  kind: "import" | "reexport" | "reexport-all";
}

interface ExportGraphPayload {
  nodes: ExportGraphNode[];
  edges: ExportGraphEdge[];
}

/** Language IDs tsc-rs owns; used to pick which open docs to report on. */
const TS_LANGUAGE_IDS = new Set([
  "typescript",
  "typescriptreact",
  "javascript",
  "javascriptreact",
]);

/** A flattened symbol with the position to hover at. */
interface RawSymbol {
  name: string;
  kind: string;
  position: vscode.Position;
}

/** LSP SymbolKind number -> readable name. */
const SYMBOL_KIND_NAMES: Record<number, string> = {
  1: "File", 2: "Module", 3: "Namespace", 4: "Package", 5: "Class",
  6: "Method", 7: "Property", 8: "Field", 9: "Constructor", 10: "Enum",
  11: "Interface", 12: "Function", 13: "Variable", 14: "Constant",
  15: "String", 16: "Number", 17: "Boolean", 18: "Array", 19: "Object",
  20: "Key", 21: "Null", 22: "EnumMember", 23: "Struct", 24: "Event",
  25: "Operator", 26: "TypeParameter",
};

function kindName(kind: unknown): string {
  return (typeof kind === "number" && SYMBOL_KIND_NAMES[kind]) || "Symbol";
}

/**
 * Flatten an LSP documentSymbol response into a flat list, handling both the
 * hierarchical `DocumentSymbol[]` shape (with `selectionRange`/`children`) and
 * the flat `SymbolInformation[]` shape (with `location`).
 */
function flattenSymbols(raw: unknown): RawSymbol[] {
  if (!Array.isArray(raw)) return [];
  const out: RawSymbol[] = [];
  const visit = (node: any): void => {
    if (!node || typeof node !== "object") return;
    // DocumentSymbol: has selectionRange/range; SymbolInformation: has location.
    const range = node.selectionRange ?? node.range ?? node.location?.range;
    const start = range?.start;
    if (start && typeof start.line === "number") {
      out.push({
        name: String(node.name ?? "?"),
        kind: kindName(node.kind),
        position: new vscode.Position(start.line, start.character ?? 0),
      });
    }
    if (Array.isArray(node.children)) {
      for (const child of node.children) visit(child);
    }
  };
  for (const node of raw) visit(node);
  return out;
}

function errMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** Manage the singleton "Export Map" webview panel. */
export class ExportMapPanel {
  public static readonly viewType = "tsc-rs.exportMap";

  private static current: ExportMapPanel | undefined;

  private readonly panel: vscode.WebviewPanel;
  private readonly disposables: vscode.Disposable[] = [];
  private latestPayload?: ExportGraphPayload;

  public static show(
    extensionUri: vscode.Uri,
    getClient: () => LanguageClient | undefined,
  ): void {
    const column = vscode.window.activeTextEditor?.viewColumn ?? vscode.ViewColumn.One;
    if (ExportMapPanel.current) {
      ExportMapPanel.current.panel.reveal(column, true);
      void ExportMapPanel.current.refresh(getClient);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      ExportMapPanel.viewType,
      "tsc-rs: Export Map",
      { viewColumn: column, preserveFocus: false },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(extensionUri, "dist")],
      },
    );

    ExportMapPanel.current = new ExportMapPanel(panel, extensionUri, getClient);
  }

  private constructor(
    panel: vscode.WebviewPanel,
    private readonly extensionUri: vscode.Uri,
    private readonly getClient: () => LanguageClient | undefined,
  ) {
    this.panel = panel;
    this.panel.webview.html = this.renderHtml();

    this.disposables.push(
      this.panel.onDidDispose(() => this.dispose()),
      this.panel.webview.onDidReceiveMessage((msg) => this.onMessage(msg)),
    );
  }

  private async onMessage(msg: any): Promise<void> {
    switch (msg?.type) {
      case "ready":
        await this.refresh(this.getClient);
        break;
      case "refresh":
        await this.refresh(this.getClient);
        break;
      case "copyInfo":
        await this.copyAllInfo();
        break;
      case "openFile":
        if (typeof msg.path === "string") {
          const uri = vscode.Uri.file(msg.path);
          await vscode.window.showTextDocument(uri, { preserveFocus: false });
        }
        break;
    }
  }

  private async refresh(
    getClient: () => LanguageClient | undefined,
  ): Promise<void> {
    const client = getClient();
    if (!client) {
      this.post({ type: "error", message: "tsc-rs language server is not running." });
      return;
    }
    try {
      const payload = await client.sendRequest<ExportGraphPayload>(
        "tsc-rs/exportGraph",
      );
      this.latestPayload = payload;
      this.post({ type: "graph", payload });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this.post({ type: "error", message });
    }
  }

  /**
   * Gather type info (via documentSymbol + hover), diagnostics, and offsets
   * for every open TS/JS document, and write a Markdown report to the
   * clipboard. Intended to be pasted back to an LLM for self-improving tsc-rs.
   */
  private async copyAllInfo(): Promise<void> {
    const client = this.getClient();

    // Collect URIs of files open in tabs (across all tab groups), de-duplicated
    // and preserving tab order. Falls back to nothing if no text tabs are open.
    const tabUris = new Map<string, vscode.Uri>();
    for (const group of vscode.window.tabGroups.all) {
      for (const tab of group.tabs) {
        const input = tab.input;
        if (input instanceof vscode.TabInputText) {
          tabUris.set(input.uri.toString(), input.uri);
        }
      }
    }

    // Match open tabs to loaded text documents that tsc-rs owns.
    const byUri = new Map(
      vscode.workspace.textDocuments.map((d) => [d.uri.toString(), d]),
    );
    const docs: vscode.TextDocument[] = [];
    for (const [key, uri] of tabUris) {
      const doc = byUri.get(key);
      if (
        doc &&
        TS_LANGUAGE_IDS.has(doc.languageId) &&
        (uri.scheme === "file" || uri.scheme === "untitled")
      ) {
        docs.push(doc);
      }
    }

    if (docs.length === 0) {
      this.post({ type: "copyInfoDone", ok: false });
      void vscode.window.showWarningMessage(
        "tsc-rs: No open TypeScript/JavaScript files to copy info from.",
      );
      return;
    }

    const out: string[] = [];
    out.push("# tsc-rs Open-Files Report");
    out.push("");
    out.push(`- **Generated**: ${new Date().toISOString()}`);
    out.push(`- **Open files**: ${docs.length}`);
    out.push(`- **Server**: ${client ? "running" : "stopped"}`);
    out.push(`- **VS Code**: ${vscode.version}`);
    out.push("");
    out.push(
      "> Offsets are 0-based. `char` = UTF-16 code-unit offset (VS Code), " +
        "`byte` = UTF-8 byte offset (what the tsc-rs server sees).",
    );

    let totalSymbols = 0;
    let totalDiagnostics = 0;

    for (const doc of docs) {
      const byteSize = Buffer.byteLength(doc.getText(), "utf8");
      out.push("");
      out.push("---");
      out.push("");
      out.push(`## ${doc.uri.fsPath}`);
      out.push("");
      out.push(
        `- language: \`${doc.languageId}\` · version: ${doc.version} · ` +
          `lines: ${doc.lineCount} · bytes: ${byteSize}`,
      );

      // --- Diagnostics ---
      const diags = [...vscode.languages.getDiagnostics(doc.uri)].sort(
        (a, b) => a.range.start.compareTo(b.range.start),
      );
      totalDiagnostics += diags.length;
      out.push("");
      out.push(`### Diagnostics (${diags.length})`);
      if (diags.length === 0) {
        out.push("(none)");
      } else {
        for (const d of diags) {
          const sev = ["error", "warning", "info", "hint"][d.severity ?? 0];
          const start = d.range.start;
          const code =
            typeof d.code === "object" && d.code
              ? (d.code as { value: string | number }).value
              : d.code;
          const offsets = this.offsetsFor(doc, start);
          out.push(
            `- [${sev}] L${start.line + 1}:${start.character + 1} ` +
              `(char ${offsets.char}, byte ${offsets.byte})` +
              `${code != null ? ` TS${code}` : ""}` +
              `${d.source ? ` (${d.source})` : ""}: ${d.message.replace(/\n+/g, " ")}`,
          );
        }
      }

      // --- Type info (document symbols + hover) ---
      out.push("");
      out.push("### Symbols & Types");
      if (!client) {
        out.push("(server not running — no type info)");
        continue;
      }

      let symbols: RawSymbol[] = [];
      try {
        const raw = await client.sendRequest<unknown>(
          "textDocument/documentSymbol",
          { textDocument: { uri: doc.uri.toString() } },
        );
        symbols = flattenSymbols(raw);
      } catch (err) {
        out.push(`(documentSymbol failed: ${errMessage(err)})`);
      }

      if (symbols.length === 0) {
        out.push("(no symbols)");
        continue;
      }

      const SYMBOL_CAP = 300;
      const capped = symbols.slice(0, SYMBOL_CAP);
      for (const sym of capped) {
        const pos = sym.position;
        const offsets = this.offsetsFor(doc, pos);
        const hover = await this.hoverText(client, doc.uri.toString(), pos);
        totalSymbols += 1;
        const loc = `L${pos.line + 1}:${pos.character + 1} (char ${offsets.char}, byte ${offsets.byte})`;
        out.push("");
        out.push(`- **${sym.name}** \`${sym.kind}\` — ${loc}`);
        if (hover) {
          out.push("  ```typescript");
          for (const line of hover.split("\n")) {
            out.push(`  ${line}`);
          }
          out.push("  ```");
        } else {
          out.push("  (no hover result)");
        }
      }
      if (symbols.length > SYMBOL_CAP) {
        out.push("");
        out.push(
          `_… ${symbols.length - SYMBOL_CAP} more symbols omitted (cap ${SYMBOL_CAP})._`,
        );
      }
    }

    out.splice(
      6,
      0,
      `- **Symbols captured**: ${totalSymbols} · **Diagnostics**: ${totalDiagnostics}`,
    );

    const report = out.join("\n");
    await vscode.env.clipboard.writeText(report);
    this.post({ type: "copyInfoDone", ok: true });
    vscode.window.setStatusBarMessage(
      `tsc-rs: Copied info for ${docs.length} file(s) (${totalSymbols} symbols, ${totalDiagnostics} diagnostics)`,
      4000,
    );
  }

  /** UTF-16 (char) and UTF-8 (byte) offsets of a position within a document. */
  private offsetsFor(
    doc: vscode.TextDocument,
    pos: vscode.Position,
  ): { char: number; byte: number } {
    const char = doc.offsetAt(pos);
    const textBefore = doc.getText(
      new vscode.Range(new vscode.Position(0, 0), pos),
    );
    return { char, byte: Buffer.byteLength(textBefore, "utf8") };
  }

  /** Fetch hover text at a position, stripped of markdown code fences. */
  private async hoverText(
    client: LanguageClient,
    uri: string,
    pos: vscode.Position,
  ): Promise<string | undefined> {
    try {
      const hover = await client.sendRequest<{
        contents?: { value?: string } | string | Array<{ value?: string } | string>;
      }>("textDocument/hover", {
        textDocument: { uri },
        position: { line: pos.line, character: pos.character },
      });
      const raw = hover?.contents;
      if (raw == null) return undefined;
      let text: string;
      if (typeof raw === "string") {
        text = raw;
      } else if (Array.isArray(raw)) {
        text = raw
          .map((x) => (typeof x === "string" ? x : x?.value ?? ""))
          .join("\n");
      } else {
        text = raw.value ?? "";
      }
      const clean = text
        .replace(/```\w*\n?/g, "")
        .replace(/\n```/g, "")
        .split("\n---\n")[0]
        .trim();
      return clean || undefined;
    } catch {
      return undefined;
    }
  }

  private post(msg: unknown): void {
    void this.panel.webview.postMessage(msg);
  }

  private renderHtml(): string {
    const scriptUri = this.panel.webview.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "dist", "webview", "export_map.js"),
    );
    const nonce = makeNonce();
    const csp = [
      `default-src 'none'`,
      `style-src ${this.panel.webview.cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}'`,
      `font-src ${this.panel.webview.cspSource}`,
      `img-src ${this.panel.webview.cspSource} data:`,
    ].join("; ");

    return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <title>tsc-rs: Export Map</title>
  <style>
    html, body { margin: 0; height: 100%; overflow: hidden; background: var(--vscode-editor-background); color: var(--vscode-editor-foreground); font-family: var(--vscode-font-family); font-size: var(--vscode-font-size); }
    #toolbar { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-bottom: 1px solid var(--vscode-panel-border); background: var(--vscode-sideBar-background); position: relative; z-index: 1; }
    #toolbar input[type="search"] { flex: 0 0 240px; padding: 3px 6px; background: var(--vscode-input-background); color: var(--vscode-input-foreground); border: 1px solid var(--vscode-input-border, transparent); border-radius: 2px; outline: none; }
    #toolbar label { display: inline-flex; align-items: center; gap: 4px; user-select: none; }
    #toolbar button { padding: 3px 10px; background: var(--vscode-button-background); color: var(--vscode-button-foreground); border: none; border-radius: 2px; cursor: pointer; }
    #toolbar button:hover { background: var(--vscode-button-hoverBackground); }
    #status { margin-left: auto; opacity: 0.7; font-size: 11px; }
    #graph { width: 100%; height: calc(100vh - 36px); display: block; cursor: grab; }
    #graph:active { cursor: grabbing; }
    .node circle { stroke: var(--vscode-editor-background); stroke-width: 1.2px; }
    .node.module circle { fill: var(--vscode-charts-blue, #4ea1ff); }
    .node.leaf circle { fill: var(--vscode-charts-green, #6acc6a); }
    .node.external circle { fill: var(--vscode-charts-orange, #d18030); opacity: 0.85; }
    .node text { fill: var(--vscode-editor-foreground); font-size: 10px; pointer-events: none; paint-order: stroke; stroke: var(--vscode-editor-background); stroke-width: 3px; }
    .node.dim { opacity: 0.15; }
    .node { cursor: pointer; }
    .edge { stroke: var(--vscode-editorLineNumber-foreground, #888); stroke-opacity: 0.45; stroke-width: 1px; fill: none; }
    .edge.reexport { stroke-dasharray: 4 2; }
    .edge.reexport-all { stroke-dasharray: 1 3; }
    .edge.dim { stroke-opacity: 0.05; }
  </style>
</head>
<body>
  <div id="toolbar">
    <input id="search" type="search" placeholder="Filter by file name..." />
    <label><input id="toggle-external" type="checkbox" checked /> External packages</label>
    <button id="refresh">Refresh</button>
    <button id="copy-info" title="Copy type info, diagnostics and offsets for every open file to the clipboard">Copy Info</button>
    <span id="status">Loading...</span>
  </div>
  <svg id="graph" xmlns="http://www.w3.org/2000/svg">
    <defs>
      <marker id="arrow" viewBox="0 -5 10 10" refX="14" refY="0" markerWidth="6" markerHeight="6" orient="auto">
        <path d="M0,-5L10,0L0,5" fill="var(--vscode-editorLineNumber-foreground, #888)" opacity="0.7"></path>
      </marker>
    </defs>
  </svg>
  <script nonce="${nonce}">
    document.getElementById('refresh').addEventListener('click', () => {
      acquireVsCodeApiHolder.postMessage({ type: 'refresh' });
    });
    (() => {
      const copyBtn = document.getElementById('copy-info');
      copyBtn.addEventListener('click', () => {
        copyBtn.disabled = true;
        copyBtn.textContent = 'Collecting...';
        acquireVsCodeApiHolder.postMessage({ type: 'copyInfo' });
      });
      window.addEventListener('message', (event) => {
        const msg = event.data;
        if (msg && msg.type === 'copyInfoDone') {
          copyBtn.disabled = false;
          copyBtn.textContent = msg.ok ? 'Copied ✓' : 'Copy Info';
          if (msg.ok) {
            setTimeout(() => { copyBtn.textContent = 'Copy Info'; }, 2500);
          }
        }
      });
    })();
    // Bridge: webview bundle calls acquireVsCodeApi() once; expose its handle
    // here so the toolbar refresh button can also post messages.
    const acquireVsCodeApiHolder = (() => {
      let api;
      const original = window.acquireVsCodeApi;
      window.acquireVsCodeApi = () => {
        if (!api) api = original();
        return api;
      };
      return { postMessage: (m) => (api ?? window.acquireVsCodeApi()).postMessage(m) };
    })();
  </script>
  <script nonce="${nonce}" src="${scriptUri}"></script>
</body>
</html>`;
  }

  private dispose(): void {
    ExportMapPanel.current = undefined;
    this.panel.dispose();
    while (this.disposables.length) {
      const d = this.disposables.pop();
      try {
        d?.dispose();
      } catch {
        // ignore
      }
    }
  }
}

function makeNonce(): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let out = "";
  for (let i = 0; i < 32; i++) {
    out += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return out;
}
