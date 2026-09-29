import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

interface ServerStatus {
  status: string;
  version: string;
  uptimeSeconds: number;
  openFiles: number;
  cachedFiles: number;
  projects: number;
  projectFiles: number;
  memoryMb: number;
}

interface ProjectInfo {
  root: string;
  files: number;
  configuredFiles: number;
  exports: number;
  hasConfig: boolean;
  configPath: string | null;
}

interface DiagnosticsSummary {
  totalErrors: number;
  totalWarnings: number;
  files: Array<{
    uri: string;
    fileName: string;
    errors: number;
    warnings: number;
    suggestions: number;
  }>;
}

interface OpenFile {
  uri: string;
  version: number;
  size: number;
  lines: number;
  errors: number;
  warnings: number;
}

export class TscRsPanelProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "tsc-rs.panel";

  private _view?: vscode.WebviewView;
  private _refreshInterval?: ReturnType<typeof setInterval>;
  private _getClient: () => LanguageClient | undefined;
  private _serverState: "stopped" | "starting" | "running" = "stopped";

  constructor(
    private readonly _extensionUri: vscode.Uri,
    getClient: () => LanguageClient | undefined,
  ) {
    this._getClient = getClient;
  }

  public setServerState(state: "stopped" | "starting" | "running") {
    this._serverState = state;
    this.refresh();
  }

  public resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken,
  ) {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
    };

    webviewView.webview.onDidReceiveMessage(async (msg) => {
      switch (msg.type) {
        case "refresh":
          this.refresh();
          break;
        case "restart":
          await vscode.commands.executeCommand("tsc-rs.restartLanguageServer");
          break;
        case "openFile":
          if (msg.uri) {
            const uri = vscode.Uri.parse(msg.uri);
            await vscode.window.showTextDocument(uri);
          }
          break;
        case "copyText":
          if (msg.text) {
            await vscode.env.clipboard.writeText(msg.text);
          }
          break;
      }
    });

    webviewView.onDidDispose(() => {
      if (this._refreshInterval) {
        clearInterval(this._refreshInterval);
        this._refreshInterval = undefined;
      }
    });

    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        this.refresh();
        this._startAutoRefresh();
      } else if (this._refreshInterval) {
        clearInterval(this._refreshInterval);
        this._refreshInterval = undefined;
      }
    });

    this._startAutoRefresh();
    this.refresh();
  }

  private _startAutoRefresh() {
    if (this._refreshInterval) {
      clearInterval(this._refreshInterval);
    }
    this._refreshInterval = setInterval(() => this.refresh(), 5000);
  }

  public async refresh() {
    if (!this._view) return;

    const client = this._getClient();
    let status: ServerStatus | null = null;
    let projects: ProjectInfo[] = [];
    let diagnostics: DiagnosticsSummary | null = null;
    let openFiles: OpenFile[] = [];

    if (client && this._serverState === "running") {
      try {
        [status, projects, diagnostics, openFiles] = await Promise.all([
          client.sendRequest<ServerStatus>("tsc-rs/status"),
          client.sendRequest<ProjectInfo[]>("tsc-rs/projects"),
          client.sendRequest<DiagnosticsSummary>("tsc-rs/diagnosticsSummary"),
          client.sendRequest<OpenFile[]>("tsc-rs/openFiles"),
        ]);
      } catch {
        // Server may not support custom methods yet
      }
    }

    this._view.webview.html = this._getHtml(
      status,
      projects,
      diagnostics,
      openFiles,
    );
  }

  private _getHtml(
    status: ServerStatus | null,
    projects: ProjectInfo[],
    diagnostics: DiagnosticsSummary | null,
    openFiles: OpenFile[],
  ): string {
    const state = this._serverState;
    const stateIcon =
      state === "running" ? "●" : state === "starting" ? "◐" : "○";
    const stateColor =
      state === "running"
        ? "var(--vscode-testing-iconPassed)"
        : state === "starting"
          ? "var(--vscode-editorWarning-foreground)"
          : "var(--vscode-errorForeground)";
    const stateLabel =
      state === "running"
        ? "Running"
        : state === "starting"
          ? "Starting..."
          : "Stopped";

    const uptime = status ? formatUptime(status.uptimeSeconds) : "--";
    const memory = status ? `${status.memoryMb.toFixed(1)} MB` : "--";
    const openCount = status?.openFiles ?? 0;
    const cachedCount = status?.cachedFiles ?? 0;
    const projectFileCount = status?.projectFiles ?? 0;
    const version = status?.version ?? "?";

    const totalErrors = diagnostics?.totalErrors ?? 0;
    const totalWarnings = diagnostics?.totalWarnings ?? 0;

    const fileRows = (diagnostics?.files ?? [])
      .map(
        (f) => `
      <tr class="file-row" data-uri="${escHtml(f.uri)}">
        <td class="file-name" title="${escHtml(f.fileName)}">${escHtml(basename(f.fileName))}</td>
        <td class="count err">${f.errors || ""}</td>
        <td class="count warn">${f.warnings || ""}</td>
        <td class="count sugg">${f.suggestions || ""}</td>
      </tr>`,
      )
      .join("");

    const projectRows = projects
      .map(
        (p) => `
      <tr>
        <td class="project-root" title="${escHtml(p.root)}">${escHtml(basename(p.root))}</td>
        <td class="count">${p.files}</td>
        <td class="count">${p.configuredFiles}</td>
        <td class="count">${p.exports}</td>
      </tr>`,
      )
      .join("");

    const openFileRows = openFiles
      .map(
        (f) => `
      <tr class="file-row" data-uri="${escHtml(f.uri)}">
        <td class="file-name" title="${escHtml(f.uri)}">${escHtml(basename(f.uri))}</td>
        <td class="count">${f.lines}</td>
        <td class="count">${formatSize(f.size)}</td>
        <td class="count err">${f.errors || ""}</td>
        <td class="count warn">${f.warnings || ""}</td>
      </tr>`,
      )
      .join("");

    return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0">
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: var(--vscode-font-family);
    font-size: var(--vscode-font-size);
    color: var(--vscode-foreground);
    background: var(--vscode-panel-background);
    padding: 8px 12px;
    line-height: 1.5;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--vscode-panel-border);
  }
  .toolbar .status {
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .toolbar .spacer { flex: 1; }
  .toolbar button {
    background: var(--vscode-button-secondaryBackground);
    color: var(--vscode-button-secondaryForeground);
    border: none;
    padding: 2px 8px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 11px;
  }
  .toolbar button:hover {
    background: var(--vscode-button-secondaryHoverBackground);
  }
  .toolbar button.primary {
    background: var(--vscode-button-background);
    color: var(--vscode-button-foreground);
  }
  .toolbar button.primary:hover {
    background: var(--vscode-button-hoverBackground);
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(90px, 1fr));
    gap: 6px;
    margin-bottom: 12px;
  }
  .stat {
    background: var(--vscode-editor-background);
    border: 1px solid var(--vscode-panel-border);
    border-radius: 4px;
    padding: 6px 8px;
    text-align: center;
  }
  .stat .value {
    font-size: 16px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .stat .label {
    font-size: 10px;
    opacity: 0.7;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .stat.error .value { color: var(--vscode-errorForeground); }
  .stat.warning .value { color: var(--vscode-editorWarning-foreground); }
  .stat.success .value { color: var(--vscode-testing-iconPassed); }

  section {
    margin-bottom: 12px;
  }
  section h3 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    opacity: 0.7;
    margin-bottom: 4px;
    cursor: pointer;
    user-select: none;
  }
  section h3::before {
    content: "▸ ";
    font-size: 9px;
  }
  section h3.open::before {
    content: "▾ ";
  }
  section .section-body {
    display: none;
  }
  section h3.open + .section-body {
    display: block;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th {
    text-align: left;
    font-size: 10px;
    text-transform: uppercase;
    opacity: 0.6;
    padding: 2px 4px;
    border-bottom: 1px solid var(--vscode-panel-border);
  }
  td {
    padding: 3px 4px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 180px;
  }
  .file-row {
    cursor: pointer;
  }
  .file-row:hover {
    background: var(--vscode-list-hoverBackground);
  }
  .count { text-align: right; font-variant-numeric: tabular-nums; min-width: 28px; }
  .count.err { color: var(--vscode-errorForeground); }
  .count.warn { color: var(--vscode-editorWarning-foreground); }
  .count.sugg { color: var(--vscode-editorInfo-foreground); }

  .meta {
    font-size: 10px;
    opacity: 0.5;
    text-align: right;
    margin-top: 8px;
  }
</style>
</head>
<body>
  <div class="toolbar">
    <span class="status">
      <span style="color:${stateColor}">${stateIcon}</span>
      tsc-rs <span style="opacity:0.5;font-weight:400">v${escHtml(version)}</span>
      &mdash; ${stateLabel}
    </span>
    <span class="spacer"></span>
    <button onclick="send('refresh')">Refresh</button>
    <button class="primary" onclick="send('restart')">Restart</button>
  </div>

  <div class="stats-grid">
    <div class="stat ${totalErrors > 0 ? "error" : "success"}">
      <div class="value">${totalErrors}</div>
      <div class="label">Errors</div>
    </div>
    <div class="stat ${totalWarnings > 0 ? "warning" : ""}">
      <div class="value">${totalWarnings}</div>
      <div class="label">Warnings</div>
    </div>
    <div class="stat">
      <div class="value">${openCount}</div>
      <div class="label">Open</div>
    </div>
    <div class="stat">
      <div class="value">${projectFileCount}</div>
      <div class="label">Files</div>
    </div>
    <div class="stat">
      <div class="value">${memory}</div>
      <div class="label">Memory</div>
    </div>
    <div class="stat">
      <div class="value">${uptime}</div>
      <div class="label">Uptime</div>
    </div>
  </div>

  ${
    diagnostics && diagnostics.files.length > 0
      ? `
  <section>
    <h3 class="open" onclick="toggle(this)">Diagnostics (${diagnostics.files.length} files)</h3>
    <div class="section-body">
      <table>
        <tr><th>File</th><th class="count">Err</th><th class="count">Warn</th><th class="count">Info</th></tr>
        ${fileRows}
      </table>
    </div>
  </section>`
      : ""
  }

  ${
    openFiles.length > 0
      ? `
  <section>
    <h3 onclick="toggle(this)">Open Files (${openFiles.length})</h3>
    <div class="section-body">
      <table>
        <tr><th>File</th><th class="count">Lines</th><th class="count">Size</th><th class="count">Err</th><th class="count">Warn</th></tr>
        ${openFileRows}
      </table>
    </div>
  </section>`
      : ""
  }

  ${
    projects.length > 0
      ? `
  <section>
    <h3 onclick="toggle(this)">Projects (${projects.length})</h3>
    <div class="section-body">
      <table>
        <tr><th>Root</th><th class="count">Files</th><th class="count">Conf.</th><th class="count">Exports</th></tr>
        ${projectRows}
      </table>
    </div>
  </section>`
      : ""
  }

  <div class="meta">${cachedCount} cached &middot; ${projects.length} project${projects.length !== 1 ? "s" : ""}</div>

  <script>
    const vscode = acquireVsCodeApi();
    function send(type, data) {
      vscode.postMessage({ type, ...data });
    }
    function toggle(el) {
      el.classList.toggle("open");
    }
    document.addEventListener("click", (e) => {
      const row = e.target.closest(".file-row");
      if (row) {
        send("openFile", { uri: row.dataset.uri });
      }
    });
  </script>
</body>
</html>`;
  }
}

function escHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function basename(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || p;
}

function formatUptime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return `${h}h${m}m`;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)}K`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}M`;
}
