import * as fs from "fs";
import * as path from "path";
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

interface DiagSummary {
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

interface ActivityEntry {
  time: string;
  timestamp: string;
  method: string;
  file: string;
  elapsed: string;
  result: string;
}

export class TscRsSidebarProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "tsc-rs.sidebar";

  private _view?: vscode.WebviewView;
  private _refreshInterval?: ReturnType<typeof setInterval>;
  private _getClient: () => LanguageClient | undefined;
  private _serverState: "stopped" | "starting" | "running" = "stopped";
  private _activeTab = "overview";
  private _requestLog: ActivityEntry[] = [];
  private _logStream: fs.WriteStream | null = null;
  private _logFilePath = "";
  private _sessionId = Date.now().toString(36);

  constructor(
    private readonly _extensionUri: vscode.Uri,
    getClient: () => LanguageClient | undefined,
  ) {
    this._getClient = getClient;
    this._setupLogFile();
    // Re-setup on config change
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration("tsc-rs.activityLogFile")) {
        this._setupLogFile();
      }
    });
  }

  private _setupLogFile() {
    // Close existing stream
    if (this._logStream) {
      this._logStream.end();
      this._logStream = null;
    }
    const config = vscode.workspace.getConfiguration("tsc-rs");
    const logPath = config.get<string>("activityLogFile", "").trim();
    if (!logPath) {
      this._logFilePath = "";
      return;
    }
    // Resolve ~ and relative paths
    const resolved = logPath.startsWith("~/")
      ? path.join(process.env.HOME || "", logPath.slice(2))
      : path.isAbsolute(logPath) ? logPath
      : path.join(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || "", logPath);
    try {
      const dir = path.dirname(resolved);
      if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
      this._logStream = fs.createWriteStream(resolved, { flags: "a" });
      this._logFilePath = resolved;
      // Write session header
      this._logStream.write(JSON.stringify({
        _event: "session_start",
        timestamp: new Date().toISOString(),
        sessionId: this._sessionId,
        vscodeVersion: vscode.version,
        extensionVersion: "0.2.0",
      }) + "\n");
    } catch {
      this._logFilePath = "";
    }
  }

  public setServerState(state: "stopped" | "starting" | "running") {
    this._serverState = state;
    this._writeLogEntry({
      _event: "server_state",
      timestamp: new Date().toISOString(),
      state,
    });
    this.refresh();
  }

  public logRequest(
    method: string,
    file: string,
    elapsed: string,
    result: string,
    detail?: Record<string, unknown>,
  ) {
    const timestamp = new Date().toISOString();
    const time = timestamp.slice(11, 19);
    const entry: ActivityEntry = { time, timestamp, method, file, elapsed, result };
    this._requestLog.unshift(entry);
    if (this._requestLog.length > 200) this._requestLog.length = 200;

    // Write to log file
    this._writeLogEntry({
      _event: "lsp_request",
      timestamp,
      sessionId: this._sessionId,
      method,
      file,
      elapsedMs: parseFloat(elapsed),
      result,
      ...(detail || {}),
    });
  }

  private _writeLogEntry(entry: Record<string, unknown>) {
    if (this._logStream) {
      try {
        this._logStream.write(JSON.stringify(entry) + "\n");
      } catch { /* ignore write errors */ }
    }
  }

  public resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken,
  ) {
    this._view = webviewView;
    webviewView.webview.options = { enableScripts: true };

    webviewView.webview.onDidReceiveMessage(async (msg) => {
      switch (msg.type) {
        case "refresh": this.refresh(); break;
        case "restart": await vscode.commands.executeCommand("tsc-rs.restartLanguageServer"); break;
        case "setTab": this._activeTab = msg.tab; this.refresh(); break;
        case "openFile":
          if (msg.uri) await vscode.window.showTextDocument(vscode.Uri.parse(msg.uri));
          break;
        case "openSettings":
          await vscode.commands.executeCommand("workbench.action.openSettings", "tsc-rs");
          break;
        case "copyText":
          if (msg.text) await vscode.env.clipboard.writeText(msg.text);
          break;
        case "copyActivity": {
          const lines = this._requestLog.map(r =>
            `${r.timestamp}\t${r.method}\t${r.file}\t${r.elapsed}ms\t${r.result}`
          ).join("\n");
          await vscode.env.clipboard.writeText(lines);
          vscode.window.setStatusBarMessage(`Copied ${this._requestLog.length} activity entries`, 2000);
          break;
        }
        case "clearActivity":
          this._requestLog.length = 0;
          this.refresh();
          break;
        case "openLogFile":
          if (this._logFilePath) {
            const doc = await vscode.workspace.openTextDocument(this._logFilePath);
            await vscode.window.showTextDocument(doc);
          } else {
            vscode.window.showInformationMessage(
              "No log file configured. Set tsc-rs.activityLogFile in settings.",
            );
          }
          break;
      }
    });

    webviewView.onDidDispose(() => {
      if (this._refreshInterval) clearInterval(this._refreshInterval);
    });
    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) { this.refresh(); this._startAutoRefresh(); }
      else if (this._refreshInterval) { clearInterval(this._refreshInterval); this._refreshInterval = undefined; }
    });

    this._startAutoRefresh();
    this.refresh();
  }

  private _startAutoRefresh() {
    if (this._refreshInterval) clearInterval(this._refreshInterval);
    this._refreshInterval = setInterval(() => this.refresh(), 5000);
  }

  public async refresh() {
    if (!this._view) return;
    const client = this._getClient();
    let status: ServerStatus | null = null;
    let projects: ProjectInfo[] = [];
    let diagnostics: DiagSummary | null = null;
    let openFiles: OpenFile[] = [];

    if (client && this._serverState === "running") {
      try {
        [status, projects, diagnostics, openFiles] = await Promise.all([
          client.sendRequest<ServerStatus>("tsc-rs/status"),
          client.sendRequest<ProjectInfo[]>("tsc-rs/projects"),
          client.sendRequest<DiagSummary>("tsc-rs/diagnosticsSummary"),
          client.sendRequest<OpenFile[]>("tsc-rs/openFiles"),
        ]);
      } catch { /* server may not support */ }
    }

    this._view.webview.html = this._getHtml(status, projects, diagnostics, openFiles);
  }

  private _getHtml(
    status: ServerStatus | null,
    projects: ProjectInfo[],
    diagnostics: DiagSummary | null,
    openFiles: OpenFile[],
  ): string {
    const state = this._serverState;
    const tab = this._activeTab;
    const stateColor = state === "running" ? "var(--vscode-testing-iconPassed)"
      : state === "starting" ? "var(--vscode-editorWarning-foreground)"
      : "var(--vscode-errorForeground)";
    const stateLabel = state === "running" ? "Running" : state === "starting" ? "Starting..." : "Stopped";
    const version = status?.version ?? "?";
    const uptime = status ? fmtUptime(status.uptimeSeconds) : "--";
    const memory = status ? `${status.memoryMb.toFixed(1)} MB` : "--";
    const totalErrors = diagnostics?.totalErrors ?? 0;
    const totalWarnings = diagnostics?.totalWarnings ?? 0;

    const tabs = [
      { id: "overview", label: "Overview" },
      { id: "diagnostics", label: `Diagnostics${totalErrors > 0 ? ` (${totalErrors})` : ""}` },
      { id: "projects", label: `Projects (${projects.length})` },
      { id: "activity", label: "Activity" },
    ];

    const tabBar = tabs.map(t =>
      `<button class="tab${tab === t.id ? " active" : ""}" onclick="send('setTab',{tab:'${t.id}'})">${esc(t.label)}</button>`
    ).join("");

    let body = "";

    if (tab === "overview") {
      body = this._renderOverview(status, projects, diagnostics, openFiles, stateColor, stateLabel, version, uptime, memory, totalErrors, totalWarnings);
    } else if (tab === "diagnostics") {
      body = this._renderDiagnostics(diagnostics, openFiles);
    } else if (tab === "projects") {
      body = this._renderProjects(projects);
    } else if (tab === "activity") {
      body = this._renderActivity();
    }

    return `<!DOCTYPE html>
<html lang="en"><head>
<meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:var(--vscode-font-family);font-size:var(--vscode-font-size);color:var(--vscode-foreground);background:var(--vscode-sideBar-background);line-height:1.5}
.header{padding:10px 12px 0;display:flex;align-items:center;gap:8px}
.header .logo{font-weight:700;font-size:14px;display:flex;align-items:center;gap:6px}
.header .dot{width:8px;height:8px;border-radius:50%;display:inline-block}
.header .spacer{flex:1}
.header button{background:var(--vscode-button-secondaryBackground);color:var(--vscode-button-secondaryForeground);border:none;padding:2px 8px;border-radius:3px;cursor:pointer;font-size:11px}
.header button:hover{background:var(--vscode-button-secondaryHoverBackground)}
.tabs{display:flex;border-bottom:1px solid var(--vscode-panel-border);padding:0 8px;margin-top:8px;overflow-x:auto}
.tab{background:none;border:none;color:var(--vscode-foreground);opacity:0.6;padding:6px 10px;cursor:pointer;font-size:11px;border-bottom:2px solid transparent;white-space:nowrap}
.tab:hover{opacity:0.9}
.tab.active{opacity:1;border-bottom-color:var(--vscode-focusBorder);font-weight:600}
.content{padding:10px 12px;overflow-y:auto}
.stats{display:grid;grid-template-columns:1fr 1fr;gap:6px;margin-bottom:12px}
.stat{background:var(--vscode-editor-background);border:1px solid var(--vscode-panel-border);border-radius:4px;padding:8px 10px;text-align:center}
.stat .val{font-size:18px;font-weight:700;font-variant-numeric:tabular-nums}
.stat .lbl{font-size:10px;opacity:0.6;text-transform:uppercase;letter-spacing:0.5px}
.stat.err .val{color:var(--vscode-errorForeground)}
.stat.warn .val{color:var(--vscode-editorWarning-foreground)}
.stat.ok .val{color:var(--vscode-testing-iconPassed)}
section{margin-bottom:14px}
section h3{font-size:11px;text-transform:uppercase;letter-spacing:0.8px;opacity:0.6;margin-bottom:4px;padding-bottom:4px;border-bottom:1px solid var(--vscode-panel-border)}
table{width:100%;border-collapse:collapse;font-size:12px}
th{text-align:left;font-size:10px;text-transform:uppercase;opacity:0.5;padding:2px 4px;border-bottom:1px solid var(--vscode-panel-border)}
td{padding:4px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:200px}
tr.clickable{cursor:pointer}
tr.clickable:hover{background:var(--vscode-list-hoverBackground)}
.count{text-align:right;font-variant-numeric:tabular-nums;min-width:30px}
.count.err{color:var(--vscode-errorForeground)}
.count.warn{color:var(--vscode-editorWarning-foreground)}
.count.info{color:var(--vscode-editorInfo-foreground)}
.badge{display:inline-block;padding:1px 6px;border-radius:8px;font-size:10px;font-weight:600}
.badge.green{background:rgba(50,200,100,0.15);color:var(--vscode-testing-iconPassed)}
.badge.red{background:rgba(255,80,80,0.15);color:var(--vscode-errorForeground)}
.badge.yellow{background:rgba(255,200,50,0.15);color:var(--vscode-editorWarning-foreground)}
.badge.blue{background:rgba(50,120,255,0.15);color:var(--vscode-focusBorder)}
.project-card{background:var(--vscode-editor-background);border:1px solid var(--vscode-panel-border);border-radius:6px;padding:10px;margin-bottom:8px}
.project-card h4{font-size:12px;font-weight:600;margin-bottom:6px;display:flex;align-items:center;gap:6px}
.project-card .meta{display:flex;gap:12px;font-size:11px;opacity:0.7}
.project-card .meta span{display:flex;align-items:center;gap:3px}
.activity-row{font-size:11px;padding:3px 0;border-bottom:1px solid var(--vscode-panel-border);display:flex;gap:6px}
.activity-row .time{opacity:0.4;font-variant-numeric:tabular-nums;min-width:60px}
.activity-row .method{font-weight:600;min-width:70px}
.activity-row .file{opacity:0.7;flex:1;overflow:hidden;text-overflow:ellipsis}
.activity-row .elapsed{opacity:0.5;font-variant-numeric:tabular-nums}
.activity-row .result{opacity:0.6;max-width:120px;overflow:hidden;text-overflow:ellipsis}
.empty{text-align:center;padding:20px;opacity:0.4;font-style:italic}
.info-row{display:flex;justify-content:space-between;padding:3px 0;font-size:12px}
.info-row .key{opacity:0.6}
.info-row .value{font-weight:500;font-variant-numeric:tabular-nums}
.server-info{background:var(--vscode-editor-background);border:1px solid var(--vscode-panel-border);border-radius:6px;padding:10px;margin-bottom:12px}
a.action{color:var(--vscode-textLink-foreground);text-decoration:none;cursor:pointer;font-size:11px}
a.action:hover{text-decoration:underline}
</style>
</head><body>
<div class="header">
  <span class="logo"><span class="dot" style="background:${stateColor}"></span>tsc-rs <span style="opacity:0.4;font-weight:400">v${esc(version)}</span></span>
  <span class="spacer"></span>
  <button onclick="send('refresh')">Refresh</button>
  <button onclick="send('restart')">Restart</button>
</div>
<div class="tabs">${tabBar}</div>
<div class="content">${body}</div>
<script>
const vscode=acquireVsCodeApi();
function send(type,data){vscode.postMessage({type,...(data||{})});}
document.addEventListener("click",e=>{
  const row=e.target.closest("tr.clickable");
  if(row&&row.dataset.uri)send("openFile",{uri:row.dataset.uri});
});
</script>
</body></html>`;
  }

  private _renderOverview(
    status: ServerStatus | null, projects: ProjectInfo[], diagnostics: DiagSummary | null,
    openFiles: OpenFile[], stateColor: string, stateLabel: string,
    version: string, uptime: string, memory: string,
    totalErrors: number, totalWarnings: number,
  ): string {
    const openCount = status?.openFiles ?? 0;
    const projFiles = status?.projectFiles ?? 0;
    const cached = status?.cachedFiles ?? 0;

    return `
<div class="server-info">
  <div class="info-row"><span class="key">Status</span><span class="value" style="color:${stateColor}">${stateLabel}</span></div>
  <div class="info-row"><span class="key">Version</span><span class="value">${esc(version)}</span></div>
  <div class="info-row"><span class="key">Uptime</span><span class="value">${uptime}</span></div>
  <div class="info-row"><span class="key">Memory</span><span class="value">${memory}</span></div>
  <div class="info-row"><span class="key">PID</span><span class="value">${status ? "active" : "--"}</span></div>
</div>

<div class="stats">
  <div class="stat ${totalErrors > 0 ? "err" : "ok"}"><div class="val">${totalErrors}</div><div class="lbl">Errors</div></div>
  <div class="stat ${totalWarnings > 0 ? "warn" : ""}"><div class="val">${totalWarnings}</div><div class="lbl">Warnings</div></div>
  <div class="stat"><div class="val">${openCount}</div><div class="lbl">Open Files</div></div>
  <div class="stat"><div class="val">${projFiles}</div><div class="lbl">Project Files</div></div>
</div>

<section>
  <h3>Projects</h3>
  ${projects.length === 0 ? '<div class="empty">No projects loaded</div>' :
    projects.map(p => `
    <div class="project-card">
      <h4>${esc(basename(p.root))} ${p.hasConfig ? '<span class="badge blue">tsconfig</span>' : '<span class="badge yellow">inferred</span>'}</h4>
      <div class="meta">
        <span>${p.files} files</span>
        <span>${p.configuredFiles} configured</span>
        <span>${p.exports} exports</span>
      </div>
      ${p.configPath ? `<div style="font-size:10px;opacity:0.4;margin-top:4px">${esc(p.configPath)}</div>` : ""}
    </div>`).join("")}
</section>

${openFiles.length > 0 ? `
<section>
  <h3>Open Files (${openFiles.length})</h3>
  <table>
    <tr><th>File</th><th class="count">Lines</th><th class="count">Err</th><th class="count">Warn</th></tr>
    ${openFiles.map(f => `
    <tr class="clickable" data-uri="${esc(f.uri)}">
      <td title="${esc(f.uri)}">${esc(basename(f.uri))}</td>
      <td class="count">${f.lines}</td>
      <td class="count err">${f.errors || ""}</td>
      <td class="count warn">${f.warnings || ""}</td>
    </tr>`).join("")}
  </table>
</section>` : ""}

<div style="text-align:center;margin-top:12px">
  <a class="action" onclick="send('openSettings')">Extension Settings</a>
</div>`;
  }

  private _renderDiagnostics(diagnostics: DiagSummary | null, openFiles: OpenFile[]): string {
    const files = diagnostics?.files ?? [];
    const totalE = diagnostics?.totalErrors ?? 0;
    const totalW = diagnostics?.totalWarnings ?? 0;

    if (files.length === 0) {
      return `
<div class="stats">
  <div class="stat ok"><div class="val">0</div><div class="lbl">Errors</div></div>
  <div class="stat"><div class="val">0</div><div class="lbl">Warnings</div></div>
</div>
<div class="empty">No diagnostics. All clean!</div>`;
    }

    return `
<div class="stats">
  <div class="stat ${totalE > 0 ? "err" : "ok"}"><div class="val">${totalE}</div><div class="lbl">Errors</div></div>
  <div class="stat ${totalW > 0 ? "warn" : ""}"><div class="val">${totalW}</div><div class="lbl">Warnings</div></div>
</div>

<section>
  <h3>Files with Issues (${files.length})</h3>
  <table>
    <tr><th>File</th><th class="count">Errors</th><th class="count">Warnings</th><th class="count">Info</th></tr>
    ${files.map(f => `
    <tr class="clickable" data-uri="${esc(f.uri)}">
      <td title="${esc(f.fileName)}">${esc(basename(f.fileName))}</td>
      <td class="count err">${f.errors || ""}</td>
      <td class="count warn">${f.warnings || ""}</td>
      <td class="count info">${f.suggestions || ""}</td>
    </tr>`).join("")}
  </table>
</section>`;
  }

  private _renderProjects(projects: ProjectInfo[]): string {
    if (projects.length === 0) {
      return '<div class="empty">No projects loaded. Open a TypeScript file to start.</div>';
    }

    return projects.map(p => `
<div class="project-card">
  <h4>${esc(basename(p.root))} ${p.hasConfig ? '<span class="badge blue">tsconfig</span>' : '<span class="badge yellow">inferred</span>'}</h4>
  <div class="info-row"><span class="key">Root</span><span class="value" title="${esc(p.root)}">${esc(p.root)}</span></div>
  <div class="info-row"><span class="key">Config</span><span class="value">${p.configPath ? esc(basename(p.configPath)) : "none"}</span></div>
  <div class="info-row"><span class="key">Total Files</span><span class="value">${p.files}</span></div>
  <div class="info-row"><span class="key">Configured Files</span><span class="value">${p.configuredFiles}</span></div>
  <div class="info-row"><span class="key">Export Map</span><span class="value">${p.exports} modules</span></div>
</div>`).join("");
  }

  private _renderActivity(): string {
    const logActive = !!this._logFilePath;

    const toolbar = `
<div style="display:flex;gap:6px;margin-bottom:10px;flex-wrap:wrap;align-items:center">
  <button onclick="send('copyActivity')" style="background:var(--vscode-button-secondaryBackground);color:var(--vscode-button-secondaryForeground);border:none;padding:2px 8px;border-radius:3px;cursor:pointer;font-size:11px">Copy All</button>
  <button onclick="send('clearActivity')" style="background:var(--vscode-button-secondaryBackground);color:var(--vscode-button-secondaryForeground);border:none;padding:2px 8px;border-radius:3px;cursor:pointer;font-size:11px">Clear</button>
  <button onclick="send('openLogFile')" style="background:${logActive ? "var(--vscode-button-background)" : "var(--vscode-button-secondaryBackground)"};color:${logActive ? "var(--vscode-button-foreground)" : "var(--vscode-button-secondaryForeground)"};border:none;padding:2px 8px;border-radius:3px;cursor:pointer;font-size:11px">${logActive ? "Open Log" : "No Log File"}</button>
  <span style="flex:1"></span>
  <span style="font-size:10px;opacity:0.4">${this._requestLog.length} entries${logActive ? " | logging to file" : ""}</span>
</div>`;

    if (this._requestLog.length === 0) {
      return `${toolbar}<div class="empty">No activity yet. Interact with a TypeScript file.</div>`;
    }

    // Compute stats
    const methods: Record<string, { count: number; totalMs: number }> = {};
    for (const r of this._requestLog) {
      const m = r.method;
      if (!methods[m]) methods[m] = { count: 0, totalMs: 0 };
      methods[m].count++;
      methods[m].totalMs += parseFloat(r.elapsed) || 0;
    }
    const statsHtml = Object.entries(methods)
      .sort((a, b) => b[1].count - a[1].count)
      .map(([m, s]) => `<span style="font-size:10px;opacity:0.6">${esc(m)}: ${s.count}x (avg ${(s.totalMs / s.count).toFixed(1)}ms)</span>`)
      .join(" &middot; ");

    const rows = this._requestLog.slice(0, 80).map((r, i) => {
      const methodColor = r.method === "hover" ? "var(--vscode-editorInfo-foreground)"
        : r.method === "definition" ? "var(--vscode-textLink-foreground)"
        : r.method === "completion" ? "var(--vscode-testing-iconPassed)"
        : "var(--vscode-foreground)";
      const isNull = r.result === "null" || r.result === "not found";
      return `
<div class="activity-row" title="${esc(r.timestamp)} ${esc(r.method)} ${esc(r.file)} -> ${esc(r.result)}">
  <span class="time">${esc(r.time)}</span>
  <span class="method" style="color:${methodColor}">${esc(r.method)}</span>
  <span class="file">${esc(r.file)}</span>
  <span class="elapsed">${esc(r.elapsed)}ms</span>
  <span class="result" style="${isNull ? "opacity:0.3" : ""}">${esc(r.result.slice(0, 30))}</span>
</div>`;
    }).join("");

    return `${toolbar}
<div style="margin-bottom:8px">${statsHtml}</div>
<section>
  <h3>Recent Requests</h3>
  ${rows}
</section>`;
  }
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function basename(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || p;
}

function fmtUptime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return `${h}h ${m}m`;
}
