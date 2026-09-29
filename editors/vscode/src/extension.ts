import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import { ExportMapPanel } from "./export_map_panel";
import { TscRsPanelProvider } from "./panel";
import { TscRsSidebarProvider } from "./sidebar";

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel | undefined;
let clientLifecycle: Promise<void> = Promise.resolve();
let panelProvider: TscRsPanelProvider | undefined;
let sidebarProvider: TscRsSidebarProvider | undefined;

const RESTART_COMMAND = "tsc-rs.restartLanguageServer";

type ResolvedServer = {
  command: string;
  args: string[];
  cwd?: string;
  source: string;
};

function log(msg: string): void {
  const ts = new Date().toISOString().slice(11, 23);
  outputChannel?.appendLine(`[${ts}] ${msg}`);
}

// ---------------------------------------------------------------------------
// Competing extension detection
// ---------------------------------------------------------------------------

/** Known extensions that provide TypeScript language features and will
 *  conflict with tsc-rs hover, completions, diagnostics, etc. */
const KNOWN_COMPETING_EXTENSIONS: ReadonlyArray<{
  id: string;
  label: string;
  builtin: boolean;
}> = [
  { id: "vscode.typescript-language-features", label: "Built-in TypeScript", builtin: true },
  { id: "vscode.typescript", label: "Built-in TypeScript Basics", builtin: true },
  { id: "typescriptteam.native-preview", label: "TypeScript Native Preview (tsgo)", builtin: false },
  { id: "ms-vscode.vscode-typescript-next", label: "TypeScript Nightly", builtin: false },
  { id: "denoland.vscode-deno", label: "Deno", builtin: false },
];

/** Language IDs that tsc-rs owns. Any other extension registering providers
 *  for these languages is a potential conflict. */
const TS_LANGUAGE_IDS = new Set([
  "typescript",
  "typescriptreact",
  "javascript",
  "javascriptreact",
]);

let conflictStatusBar: vscode.StatusBarItem | undefined;

/** Detect all competing TypeScript extensions — both from the known list
 *  and dynamically by scanning activation events.
 *  Returns the list of active competing extension IDs. */
function detectCompetingExtensions(): string[] {
  const dominated = new Set(KNOWN_COMPETING_EXTENSIONS.map((e) => e.id));
  const competing: string[] = [];

  // 1. Check known list
  for (const known of KNOWN_COMPETING_EXTENSIONS) {
    const ext = vscode.extensions.getExtension(known.id);
    if (ext?.isActive) {
      competing.push(known.id);
      log(`  competing: ${known.label} (${known.id}) — active`);
    }
  }

  // 2. Scan all extensions for ones that activate on TS languages
  //    (catches unknown third-party TS plugins)
  for (const ext of vscode.extensions.all) {
    if (ext.id === "tsc-rs.tsc-rs" || ext.id === "tsc-rs") continue;
    if (dominated.has(ext.id)) continue;
    if (!ext.isActive) continue;

    const pkg = ext.packageJSON;
    if (!pkg) continue;

    // Check activation events for onLanguage:typescript etc.
    const activationEvents: string[] = pkg.activationEvents ?? [];
    const activatesOnTs = activationEvents.some((ev: string) => {
      const lang = ev.replace("onLanguage:", "");
      return TS_LANGUAGE_IDS.has(lang);
    });
    if (!activatesOnTs) continue;

    // Check if it contributes TS language features (not just themes/snippets)
    const contribs = pkg.contributes ?? {};
    const hasLangProvider =
      contribs.typescriptServerPlugins ||
      contribs.languages?.some?.((l: any) =>
        TS_LANGUAGE_IDS.has(l.id),
      );

    // Also check if main module is a language client (heuristic: depends on
    // vscode-languageclient or vscode-languageserver-protocol)
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };
    const hasLspClient =
      "vscode-languageclient" in deps ||
      "vscode-languageserver-protocol" in deps;

    if (hasLangProvider || hasLspClient) {
      competing.push(ext.id);
      log(`  competing: ${ext.packageJSON.displayName ?? ext.id} (${ext.id}) — active, provides TS features`);
    }
  }

  return competing;
}

/** Update the status bar conflict indicator. */
function updateConflictStatusBar(competing: string[]): void {
  if (competing.length === 0) {
    conflictStatusBar?.hide();
    return;
  }

  if (!conflictStatusBar) {
    conflictStatusBar = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      100,
    );
    conflictStatusBar.command = "tsc-rs.showCompetingExtensions";
  }

  const n = competing.length;
  conflictStatusBar.text = `$(warning) tsc-rs: ${n} conflict${n > 1 ? "s" : ""}`;
  conflictStatusBar.tooltip = `Competing TS extensions: ${competing.join(", ")}\nClick to disable them.`;
  conflictStatusBar.backgroundColor = new vscode.ThemeColor(
    "statusBarItem.warningBackground",
  );
  conflictStatusBar.show();
}

/** Attempt to disable tsserver via workspace settings. */
async function disableTsServerSettings(): Promise<void> {
  const tsConfig = vscode.workspace.getConfiguration("typescript");
  const jsConfig = vscode.workspace.getConfiguration("javascript");
  const updates: Array<[string, string, boolean]> = [
    ["typescript", "tsserver.enable", false],
    ["typescript", "validate.enable", false],
    ["typescript", "suggest.enabled", false],
    ["javascript", "validate.enable", false],
    ["javascript", "suggest.enabled", false],
  ];
  for (const [section, key, value] of updates) {
    const cfg = section === "typescript" ? tsConfig : jsConfig;
    const current = cfg.get(key);
    if (current !== value) {
      await cfg.update(key, value, vscode.ConfigurationTarget.Workspace);
      log(`  ${section}.${key}: ${current} -> ${value}`);
    }
  }
}

/** Try to disable a single extension via VS Code command. */
async function tryDisableExtension(extId: string): Promise<boolean> {
  try {
    await vscode.commands.executeCommand(
      "workbench.extensions.disableExtension",
      extId,
    );
    log(`  ${extId}: disable command accepted.`);
    return true;
  } catch (err) {
    log(`  ${extId}: disable command failed — ${formatError(err)}`);
    return false;
  }
}

/** Full competing-extension check: detect, disable, and update status bar.
 *  Called at startup and whenever extensions change. */
async function checkCompetingExtensions(): Promise<void> {
  log("Scanning for competing TypeScript extensions...");

  const competing = detectCompetingExtensions();

  if (competing.length === 0) {
    log("  No competing TypeScript extensions detected.");
    updateConflictStatusBar([]);
    return;
  }

  log(`  Found ${competing.length} competing extension(s): ${competing.join(", ")}`);

  // Disable tsserver settings
  await disableTsServerSettings();

  // Try to disable each competing extension
  let needsReload = false;
  for (const extId of competing) {
    const disabled = await tryDisableExtension(extId);
    if (!disabled) {
      needsReload = true;
    }
  }

  // Re-check after a short delay to see what stuck
  setTimeout(() => {
    const stillActive = detectCompetingExtensions();
    updateConflictStatusBar(stillActive);

    if (stillActive.length > 0) {
      const names = stillActive.join(", ");
      log(`WARNING: ${stillActive.length} competing extension(s) still active: ${names}`);
      const msg =
        `tsc-rs: ${stillActive.length} competing TypeScript extension(s) detected (${names}). ` +
        "These may block tsc-rs hover, completions, and diagnostics.";
      void vscode.window
        .showWarningMessage(msg, "Disable & Reload", "Ignore")
        .then(async (choice) => {
          if (choice === "Disable & Reload") {
            for (const id of stillActive) {
              await tryDisableExtension(id);
            }
            await vscode.commands.executeCommand("workbench.action.reloadWindow");
          }
        });
    } else {
      log("  All competing extensions successfully disabled.");
    }
  }, 2000);
}

export function activate(context: vscode.ExtensionContext): void {
  outputChannel = vscode.window.createOutputChannel("tsc-rs");
  context.subscriptions.push(outputChannel);
  context.subscriptions.push({
    dispose: () => {
      queueClientLifecycle(() => stopClient());
    },
  });

  log("tsc-rs extension activating...");
  log(`Platform: ${os.platform()} ${os.arch()}`);
  log(`VS Code: ${vscode.version}`);
  log(`Extension path: ${context.extensionPath}`);
  log(
    `Workspace folders: ${(vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath).join(", ") || "(none)"}`,
  );

  // Register "show competing extensions" command (status bar click action)
  context.subscriptions.push(
    vscode.commands.registerCommand("tsc-rs.showCompetingExtensions", async () => {
      const competing = detectCompetingExtensions();
      if (competing.length === 0) {
        void vscode.window.showInformationMessage("tsc-rs: No competing TypeScript extensions detected.");
        return;
      }
      const choice = await vscode.window.showWarningMessage(
        `tsc-rs: ${competing.length} competing extension(s): ${competing.join(", ")}`,
        "Disable & Reload",
        "Ignore",
      );
      if (choice === "Disable & Reload") {
        for (const id of competing) {
          await tryDisableExtension(id);
        }
        await vscode.commands.executeCommand("workbench.action.reloadWindow");
      }
    }),
  );

  // Detect and disable competing TypeScript extensions
  const tscRsConfig = vscode.workspace.getConfiguration("tsc-rs");
  if (tscRsConfig.get<boolean>("disableBuiltinTypescript", true)) {
    void checkCompetingExtensions();
  } else {
    log("disableBuiltinTypescript is off — competing extension detection skipped.");
  }

  // Re-check when extensions are installed, enabled, or activated
  context.subscriptions.push(
    vscode.extensions.onDidChange(() => {
      if (tscRsConfig.get<boolean>("disableBuiltinTypescript", true)) {
        log("Extension set changed, re-scanning for competing TS extensions...");
        void checkCompetingExtensions();
      }
    }),
  );
  // Clean up status bar on deactivate
  context.subscriptions.push({ dispose: () => conflictStatusBar?.dispose() });

  panelProvider = new TscRsPanelProvider(context.extensionUri, () => client);
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(
      TscRsPanelProvider.viewType,
      panelProvider,
      { webviewOptions: { retainContextWhenHidden: true } },
    ),
  );

  sidebarProvider = new TscRsSidebarProvider(context.extensionUri, () => client);
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(
      TscRsSidebarProvider.viewType,
      sidebarProvider,
      { webviewOptions: { retainContextWhenHidden: true } },
    ),
  );
  log("Panel + sidebar providers registered.");

  context.subscriptions.push(
    vscode.commands.registerCommand(RESTART_COMMAND, async () => {
      queueClientLifecycle(() => restartClient(context, "command"));
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("tsc-rs.showExportMap", () => {
      ExportMapPanel.show(context.extensionUri, () => client);
    }),
  );

  // Copy type command — copies the type signature under cursor to clipboard
  context.subscriptions.push(
    vscode.commands.registerCommand("tsc-rs.copyType", async (encodedType?: string) => {
      let textToCopy = encodedType;
      if (!textToCopy) {
        // No argument — get hover at current cursor position
        const editor = vscode.window.activeTextEditor;
        if (!editor || !client) return;
        const hover = await client.sendRequest<any>("textDocument/hover", {
          textDocument: { uri: editor.document.uri.toString() },
          position: {
            line: editor.selection.active.line,
            character: editor.selection.active.character,
          },
        });
        if (hover?.contents?.value) {
          textToCopy = hover.contents.value
            .replace(/```typescript\n?/g, "")
            .replace(/\n```/g, "")
            .split("\n---\n")[0]
            .trim();
        }
      }
      if (textToCopy) {
        await vscode.env.clipboard.writeText(textToCopy);
        vscode.window.setStatusBarMessage(`Copied: ${textToCopy.slice(0, 60)}`, 2000);
      }
    }),
  );

  // Peek Type — show type info in an info message without leaving the editor
  context.subscriptions.push(
    vscode.commands.registerCommand("tsc-rs.peekType", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || !client) {
        void vscode.window.showWarningMessage("tsc-rs: No active editor or server not running.");
        return;
      }
      const pos = editor.selection.active;
      const hover = await client.sendRequest<any>("textDocument/hover", {
        textDocument: { uri: editor.document.uri.toString() },
        position: { line: pos.line, character: pos.character },
      });
      if (!hover?.contents?.value) {
        void vscode.window.showInformationMessage("tsc-rs: No type information at cursor.");
        return;
      }
      const typeText = hover.contents.value
        .replace(/```typescript\n?/g, "")
        .replace(/\n```/g, "")
        .trim();
      void vscode.window.showInformationMessage(typeText, "Copy").then((choice) => {
        if (choice === "Copy") {
          void vscode.env.clipboard.writeText(typeText);
        }
      });
    }),
  );

  // Show Diagnostics at Cursor — show all diagnostics near the cursor
  context.subscriptions.push(
    vscode.commands.registerCommand("tsc-rs.showDiagnostics", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      const pos = editor.selection.active;
      const uri = editor.document.uri;
      const allDiags = vscode.languages.getDiagnostics(uri);
      const atCursor = allDiags.filter((d) => d.range.contains(pos));
      const nearby = atCursor.length > 0
        ? atCursor
        : allDiags.filter((d) => {
            const dist = Math.abs(d.range.start.line - pos.line);
            return dist <= 3;
          });
      if (nearby.length === 0) {
        void vscode.window.showInformationMessage("tsc-rs: No diagnostics near cursor.");
        return;
      }
      const lines = nearby.map((d) => {
        const sev = ["Error", "Warning", "Info", "Hint"][d.severity ?? 0];
        return `[${sev}] L${d.range.start.line + 1}: ${d.message}${d.source ? ` (${d.source})` : ""}`;
      });
      void vscode.window.showInformationMessage(
        lines.join("\n"),
        "Copy",
      ).then((choice) => {
        if (choice === "Copy") {
          void vscode.env.clipboard.writeText(lines.join("\n"));
        }
      });
    }),
  );

  // Copy Debug Info — gather comprehensive debug state for LLM bug reports
  context.subscriptions.push(
    vscode.commands.registerCommand("tsc-rs.copyDebugInfo", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) {
        void vscode.window.showWarningMessage("tsc-rs: No active editor.");
        return;
      }

      const doc = editor.document;
      const pos = editor.selection.active;
      const wordRange = doc.getWordRangeAtPosition(pos);
      const word = wordRange ? doc.getText(wordRange) : "";

      // Compute byte offset (UTF-8 bytes before cursor position)
      const textBefore = doc.getText(new vscode.Range(new vscode.Position(0, 0), pos));
      const byteOffset = Buffer.byteLength(textBefore, "utf8");

      // Source context: 3 lines before and after, with line numbers
      const startLine = Math.max(0, pos.line - 3);
      const endLine = Math.min(doc.lineCount - 1, pos.line + 3);
      const contextLines: string[] = [];
      for (let i = startLine; i <= endLine; i++) {
        const prefix = i === pos.line ? ">>>" : "   ";
        contextLines.push(`${prefix} ${i + 1} | ${doc.lineAt(i).text}`);
      }
      if (pos.line >= 0 && pos.line < doc.lineCount) {
        const cursorCol = pos.character;
        contextLines.push(`    ${" ".repeat(String(pos.line + 1).length)} | ${" ".repeat(cursorCol)}^ cursor (char ${cursorCol})`);
      }

      // Hover result
      let hoverText = "(server not running)";
      if (client) {
        try {
          const hover = await client.sendRequest<any>("textDocument/hover", {
            textDocument: { uri: doc.uri.toString() },
            position: { line: pos.line, character: pos.character },
          });
          if (hover?.contents?.value) {
            hoverText = hover.contents.value
              .replace(/```typescript\n?/g, "")
              .replace(/\n```/g, "")
              .trim();
          } else {
            hoverText = "(null — no hover result)";
          }
        } catch (err) {
          hoverText = `(error: ${formatError(err)})`;
        }
      }

      // Diagnostics in this file
      const allDiags = vscode.languages.getDiagnostics(doc.uri);
      const atCursor = allDiags.filter((d) => d.range.contains(pos));
      const nearCursor = allDiags.filter((d) => Math.abs(d.range.start.line - pos.line) <= 5);
      const diagLines = (atCursor.length > 0 ? atCursor : nearCursor).map((d) => {
        const sev = ["error", "warning", "info", "hint"][d.severity ?? 0];
        return `  [${sev}] L${d.range.start.line + 1}:${d.range.start.character} ${d.message}${d.source ? ` (${d.source})` : ""}`;
      });

      // Server state
      const serverRunning = client !== undefined;
      const competing = detectCompetingExtensions();

      // Selection range (if any text selected)
      const selection = editor.selection;
      const hasSelection = !selection.isEmpty;
      const selectedText = hasSelection ? doc.getText(selection) : "";

      // Build the report
      const sections: string[] = [];

      sections.push("# tsc-rs Debug Report");
      sections.push("");
      sections.push("## Location");
      sections.push(`- **File**: \`${doc.uri.fsPath}\``);
      sections.push(`- **Position**: line ${pos.line + 1}, character ${pos.character}`);
      sections.push(`- **Byte offset**: ${byteOffset}`);
      sections.push(`- **Token**: \`${word || "(none)"}\``);
      sections.push(`- **Language**: ${doc.languageId}`);
      if (hasSelection) {
        sections.push(`- **Selection**: L${selection.start.line + 1}:${selection.start.character}–L${selection.end.line + 1}:${selection.end.character}`);
        sections.push(`- **Selected text**: \`${selectedText.length > 200 ? selectedText.slice(0, 200) + "..." : selectedText}\``);
      }

      sections.push("");
      sections.push("## Source Context");
      sections.push("```typescript");
      sections.push(contextLines.join("\n"));
      sections.push("```");

      sections.push("");
      sections.push("## Hover Result");
      sections.push("```");
      sections.push(hoverText);
      sections.push("```");

      if (diagLines.length > 0) {
        sections.push("");
        sections.push("## Diagnostics");
        sections.push(diagLines.join("\n"));
      } else {
        sections.push("");
        sections.push("## Diagnostics");
        sections.push("(none near cursor)");
      }

      sections.push("");
      sections.push("## Environment");
      sections.push(`- **Server**: ${serverRunning ? "running" : "stopped"}`);
      if (competing.length > 0) {
        sections.push(`- **Competing extensions**: ${competing.join(", ")}`);
      }
      sections.push(`- **VS Code**: ${vscode.version}`);
      sections.push(`- **Platform**: ${os.platform()} ${os.arch()}`);

      // What seems wrong (heuristics)
      const issues: string[] = [];
      if (!serverRunning) {
        issues.push("Server is not running — no LSP features available");
      }
      if (hoverText === "(null — no hover result)" && word) {
        issues.push(`Hover returned null for token "${word}" — symbol may not be resolved`);
      }
      if (competing.length > 0) {
        issues.push(`${competing.length} competing TS extension(s) active — may intercept LSP requests`);
      }
      if (atCursor.length > 0) {
        const errors = atCursor.filter((d) => d.severity === vscode.DiagnosticSeverity.Error);
        if (errors.length > 0) {
          issues.push(`${errors.length} error(s) at cursor position`);
        }
      }
      if (issues.length > 0) {
        sections.push("");
        sections.push("## Detected Issues");
        for (const issue of issues) {
          sections.push(`- ${issue}`);
        }
      }

      const report = sections.join("\n");
      await vscode.env.clipboard.writeText(report);
      vscode.window.setStatusBarMessage("tsc-rs: Debug info copied to clipboard", 3000);
      log("[debug-info] Copied debug report to clipboard");
    }),
  );
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration("tsc-rs")) {
        return;
      }
      log("tsc-rs configuration changed, restarting...");
      queueClientLifecycle(() => restartClient(context, "configuration change"));
    }),
  );
  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      if (shouldRestartForWorkspaceFolderChange()) {
        log("Workspace folders changed, restarting...");
        queueClientLifecycle(() => restartClient(context, "workspace folder change"));
      } else {
        log(
          "Workspace folders changed; keeping current tsc-rs process (LSP handles updates).",
        );
      }
    }),
  );

  queueClientLifecycle(() => startClient(context));
}

function queueClientLifecycle(action: () => Promise<void>): void {
  clientLifecycle = clientLifecycle.then(action, action).catch((error) => {
    const message = `tsc-rs LSP lifecycle error: ${formatError(error)}`;
    log(message);
    outputChannel?.show(true);
    void vscode.window.showErrorMessage(message);
  });
}

async function startClient(context: vscode.ExtensionContext): Promise<void> {
  const resolved = resolveServerBinary();
  if (!resolved) {
    log("No tsc-rs binary found. Server will not start.");
    panelProvider?.setServerState("stopped");
    sidebarProvider?.setServerState("stopped");
    return;
  }

  panelProvider?.setServerState("starting");
  sidebarProvider?.setServerState("starting");
  log(
    `Starting LSP server (source: ${resolved.source}): ${resolved.command} ${resolved.args.join(" ")}`,
  );
  if (resolved.cwd) {
    log(`  Working directory: ${resolved.cwd}`);
  }

  const serverOptions: ServerOptions = {
    command: resolved.command,
    args: resolved.args,
    transport: TransportKind.stdio,
    options: resolved.cwd ? { cwd: resolved.cwd } : undefined,
  };

  const languages = [
    "typescript",
    "typescriptreact",
    "javascript",
    "javascriptreact",
  ] as const;
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      ...languages.map((l) => ({ scheme: "file" as const, language: l })),
      ...languages.map((l) => ({ scheme: "untitled" as const, language: l })),
    ],
    synchronize: {
      fileEvents: [
        vscode.workspace.createFileSystemWatcher(
          "**/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
        ),
        vscode.workspace.createFileSystemWatcher(
          "**/{tsconfig*.json,jsconfig.json}",
        ),
      ],
    },
    outputChannel,
    traceOutputChannel: outputChannel,
    middleware: {
      provideHover: async (document, position, token, next) => {
        const start = performance.now();
        const result = await next(document, position, token);
        const elapsed = (performance.now() - start).toFixed(1);
        const range = document.getWordRangeAtPosition(position);
        const word = range ? document.getText(range) : "";
        const hasResult = result?.contents != null;
        const file = basename(document.uri.fsPath);
        log(`[client] hover: ${file}:${position.line}:${position.character} "${word}" -> ${hasResult ? "resolved" : "null"} (${elapsed}ms)`);
        let hoverClean = "";
        if (result && hasResult) {
          const c = result.contents;
          let text: string;
          if (typeof c === "string") {
            text = c;
          } else if (c instanceof vscode.MarkdownString) {
            text = c.value;
          } else if ("value" in c) {
            text = (c as any).value;
          } else if (Array.isArray(c)) {
            text = c.map((x: any) => typeof x === "string" ? x : x?.value ?? "").join("\n");
          } else {
            text = JSON.stringify(c);
          }

          // Strip markdown code fences for compact logging
          const clean = text.replace(/```\w*\n?/g, "").replace(/\n```/g, "").trim();
          hoverClean = clean;
          log(`  [client] hover content: ${clean.slice(0, 200)}`);

          // Inject copy button and debug info into hover markdown
          const config = vscode.workspace.getConfiguration("tsc-rs");
          const showCopy = config.get<boolean>("hover.copyButton", true);
          const showDebug = config.get<boolean>("hover.showDebugInfo", false);

          if (showCopy || showDebug) {
            // Extract the type signature (first code block content)
            const typeSignature = text
              .replace(/```typescript\n?/g, "")
              .replace(/\n```/g, "")
              .split("\n---\n")[0]
              .trim();

            let extra = "";

            if (showCopy && typeSignature) {
              const encoded = encodeURIComponent(typeSignature);
              extra += `\n\n[Copy type](command:tsc-rs.copyType?${JSON.stringify(encoded)})`;
            }

            if (showDebug) {
              extra += `\n\n---\n\n`;
              extra += `<small>`;
              extra += `file: \`${basename(document.uri.fsPath)}\` `;
              extra += `pos: \`${position.line}:${position.character}\` `;
              extra += `time: \`${elapsed}ms\``;
              extra += `</small>`;
            }

            if (extra) {
              const md = new vscode.MarkdownString(text + extra);
              md.isTrusted = true; // Required for command URIs
              md.supportHtml = true;
              return new vscode.Hover(md, result.range);
            }
          }
        }
        sidebarProvider?.logRequest("hover", `${file}:${position.line}:${position.character}`, elapsed, hasResult ? word : "null", {
          word, resolved: hasResult, ...(hoverClean ? { hoverContent: hoverClean.slice(0, 300) } : {}),
        });
        return result;
      },
      provideDefinition: async (document, position, token, next) => {
        const start = performance.now();
        const result = await next(document, position, token);
        const elapsed = (performance.now() - start).toFixed(1);
        const range = document.getWordRangeAtPosition(position);
        const word = range ? document.getText(range) : "";
        const locations = Array.isArray(result) ? result : result ? [result] : [];
        const count = locations.length;
        const file = basename(document.uri.fsPath);
        if (count > 0) {
          const targets = locations.slice(0, 3).map((loc: any) => {
            const u = loc.uri || loc.targetUri;
            const r = loc.range || loc.targetRange;
            const f = u ? basename(u.toString()) : "?";
            const l = r?.start?.line ?? "?";
            return `${f}:${l}`;
          }).join(", ");
          log(`[client] definition: ${file}:${position.line}:${position.character} "${word}" -> ${targets} (${elapsed}ms)`);
          sidebarProvider?.logRequest("definition", `${file}:${position.line}:${position.character}`, elapsed, targets, {
            word, locationCount: count, targets,
          });
        } else {
          log(`[client] definition: ${file}:${position.line}:${position.character} "${word}" -> not found (${elapsed}ms)`);
          sidebarProvider?.logRequest("definition", `${file}:${position.line}:${position.character}`, elapsed, "not found", {
            word, locationCount: 0,
          });
        }
        return result;
      },
      provideCompletionItem: async (document, position, context, token, next) => {
        const start = performance.now();
        const result = await next(document, position, context, token);
        const elapsed = (performance.now() - start).toFixed(1);
        const itemList = Array.isArray(result) ? result : result?.items ?? [];
        const count = itemList.length;
        const file = basename(document.uri.fsPath);
        const sample = itemList.slice(0, 5).map((i: any) => i.label || i).join(", ");
        log(`[client] completion: ${file}:${position.line}:${position.character} -> ${count} items${count > 0 ? ` [${sample}${count > 5 ? "..." : ""}]` : ""} (${elapsed}ms)`);
        sidebarProvider?.logRequest("completion", `${file}:${position.line}:${position.character}`, elapsed, `${count} items`, {
          itemCount: count, sampleLabels: itemList.slice(0, 10).map((i: any) => i.label || String(i)),
        });
        return result;
      },
      provideReferences: async (document, position, context, token, next) => {
        const start = performance.now();
        const result = await next(document, position, context, token);
        const elapsed = (performance.now() - start).toFixed(1);
        const range = document.getWordRangeAtPosition(position);
        const word = range ? document.getText(range) : "";
        const refs = Array.isArray(result) ? result : [];
        const count = refs.length;
        if (count > 0) {
          const files = new Set(refs.map((r: any) => basename((r.uri || "").toString())));
          log(
            `[client] references: ${basename(document.uri.fsPath)}:${position.line}:${position.character} ` +
            `"${word}" -> ${count} refs in ${files.size} file(s) (${elapsed}ms)`,
          );
        } else {
          log(
            `[client] references: ${basename(document.uri.fsPath)}:${position.line}:${position.character} ` +
            `"${word}" -> 0 refs (${elapsed}ms)`,
          );
        }
        return result;
      },
      provideSignatureHelp: async (document, position, context, token, next) => {
        const start = performance.now();
        const result = await next(document, position, context, token);
        const elapsed = (performance.now() - start).toFixed(1);
        if (result) {
          const sigs = result.signatures?.length ?? 0;
          const active = result.activeSignature ?? 0;
          const label = result.signatures?.[active]?.label ?? "";
          log(
            `[client] signatureHelp: ${basename(document.uri.fsPath)}:${position.line}:${position.character} ` +
            `-> ${sigs} sig(s), active="${label.slice(0, 60)}" (${elapsed}ms)`,
          );
        } else {
          log(
            `[client] signatureHelp: ${basename(document.uri.fsPath)}:${position.line}:${position.character} ` +
            `-> null (${elapsed}ms)`,
          );
        }
        return result;
      },
      provideDocumentSymbols: async (document, token, next) => {
        const start = performance.now();
        const result = await next(document, token);
        const elapsed = (performance.now() - start).toFixed(1);
        const symbols = Array.isArray(result) ? result : [];
        const names = symbols.slice(0, 5).map((s: any) => s.name || "?").join(", ");
        log(
          `[client] documentSymbols: ${basename(document.uri.fsPath)} ` +
          `-> ${symbols.length} symbols${symbols.length > 0 ? ` [${names}${symbols.length > 5 ? "..." : ""}]` : ""} (${elapsed}ms)`,
        );
        return result;
      },
      provideRenameEdits: async (document, position, newName, token, next) => {
        const start = performance.now();
        const result = await next(document, position, newName, token);
        const elapsed = (performance.now() - start).toFixed(1);
        const entries = result?.entries() ?? [];
        const fileCount = entries.length;
        const editCount = entries.reduce((sum: number, [, edits]: any) => sum + edits.length, 0);
        log(
          `[client] rename: ${basename(document.uri.fsPath)}:${position.line}:${position.character} ` +
          `-> "${newName}" ${editCount} edits across ${fileCount} files (${elapsed}ms)`,
        );
        return result;
      },
      provideCodeActions: async (document, range, context, token, next) => {
        const start = performance.now();
        const result = await next(document, range, context, token);
        const elapsed = (performance.now() - start).toFixed(1);
        const actions = Array.isArray(result) ? result : [];
        if (actions.length > 0) {
          const titles = actions.slice(0, 3).map((a: any) => a.title || "?").join(", ");
          log(
            `[client] codeAction: ${basename(document.uri.fsPath)} ` +
            `-> ${actions.length} [${titles}] (${elapsed}ms)`,
          );
        }
        // Don't log empty code action responses — too noisy
        return result;
      },
    },
  };
  log(
    `Document selector: ${languages.join(", ")} (file + untitled schemes)`,
  );

  const nextClient = new LanguageClient(
    "tsc-rs",
    "tsc-rs TypeScript",
    serverOptions,
    clientOptions,
  );
  client = nextClient;
  try {
    await nextClient.start();
    log("LSP server started successfully.");
    log(`  Server PID: ${(nextClient as any)._serverProcess?.pid ?? "unknown"}`);
    panelProvider?.setServerState("running");
    sidebarProvider?.setServerState("running");
  } catch (error) {
    if (client === nextClient) {
      client = undefined;
    }
    panelProvider?.setServerState("stopped");
    sidebarProvider?.setServerState("stopped");
    const message = `Failed to start LSP: ${formatError(error)}`;
    log(message);
    outputChannel?.show(true);
    void vscode.window.showErrorMessage(`tsc-rs: ${message}`);
  }
}

async function restartClient(
  context: vscode.ExtensionContext,
  reason: string,
): Promise<void> {
  log(`Restarting LSP (reason: ${reason})...`);
  await stopClient();
  await startClient(context);
}

async function stopClient(): Promise<void> {
  if (!client) {
    log("Stop requested but no client running.");
    return;
  }
  log("Stopping LSP server...");
  const runningClient = client;
  client = undefined;
  try {
    await runningClient.stop();
    log("LSP server stopped.");
  } catch (error) {
    log(`Error stopping LSP: ${formatError(error)}`);
  }
  panelProvider?.setServerState("stopped");
    sidebarProvider?.setServerState("stopped");
}

function formatError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function resolveServerBinary(): ResolvedServer | undefined {
  const config = vscode.workspace.getConfiguration("tsc-rs");
  const workspaceCwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const extraArgs = config.get<string[]>("serverArgs", []);
  const preferWorkspaceBinary = config.get<boolean>(
    "preferWorkspaceBinary",
    true,
  );

  // 1. Explicit serverPath setting
  const configuredRaw = config.get<string>("serverPath", "").trim();
  if (configuredRaw) {
    const configuredPath = resolveBinaryPath(
      expandHome(configuredRaw),
      workspaceCwd,
    );
    log(`Resolved tsc-rs.serverPath: "${configuredRaw}" -> "${configuredPath}"`);
    if (!validateBinaryPath(configuredPath, "tsc-rs.serverPath")) {
      return undefined;
    }
    return {
      command: configuredPath,
      args: ["--lsp", ...extraArgs],
      cwd: workspaceCwd,
      source: "tsc-rs.serverPath",
    };
  }

  // 2. TSC_RS_SERVER_PATH env var
  const envRaw = process.env.TSC_RS_SERVER_PATH ?? "";
  if (envRaw) {
    const envPath = resolveBinaryPath(expandHome(envRaw), workspaceCwd);
    log(`Resolved TSC_RS_SERVER_PATH: "${envRaw}" -> "${envPath}"`);
    if (!validateBinaryPath(envPath, "TSC_RS_SERVER_PATH")) {
      return undefined;
    }
    return {
      command: envPath,
      args: ["--lsp", ...extraArgs],
      cwd: workspaceCwd,
      source: "TSC_RS_SERVER_PATH",
    };
  }

  // 3. Workspace Cargo target binary
  if (preferWorkspaceBinary) {
    const workspaceBinary = findWorkspaceBinary();
    if (workspaceBinary) {
      log(`Found workspace binary: ${workspaceBinary}`);
      return {
        command: workspaceBinary,
        args: ["--lsp", ...extraArgs],
        cwd: path.dirname(path.dirname(path.dirname(workspaceBinary))),
        source: "workspace target binary",
      };
    }
    log("No workspace Cargo binary found.");
  }

  // 4. Fallback to PATH
  log('Falling back to "tsc-rs" on PATH.');
  return {
    command: "tsc-rs",
    args: ["--lsp", ...extraArgs],
    cwd: workspaceCwd,
    source: "PATH",
  };
}

function shouldRestartForWorkspaceFolderChange(): boolean {
  const config = vscode.workspace.getConfiguration("tsc-rs");
  const workspaceCwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const configuredPath = resolveBinaryPath(
    expandHome(config.get<string>("serverPath", "").trim()),
    workspaceCwd,
  );
  if (configuredPath) {
    return false;
  }
  const envPath = resolveBinaryPath(
    expandHome(process.env.TSC_RS_SERVER_PATH ?? ""),
    workspaceCwd,
  );
  if (envPath) {
    return false;
  }
  return config.get<boolean>("preferWorkspaceBinary", true);
}

function findWorkspaceBinary(): string | undefined {
  const exeName = process.platform === "win32" ? "tsc-rs.exe" : "tsc-rs";
  const seenRoots = new Set<string>();
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const root of findCargoWorkspaceRoots(folder.uri.fsPath)) {
      if (seenRoots.has(root)) {
        continue;
      }
      seenRoots.add(root);
      const candidates = [
        path.join(root, "target", "debug", exeName),
        path.join(root, "target", "release", exeName),
      ];
      for (const candidate of candidates) {
        if (fs.existsSync(candidate)) {
          return candidate;
        }
      }
    }
  }
  return undefined;
}

function findCargoWorkspaceRoots(startDir: string): string[] {
  const roots: string[] = [];
  let current = path.resolve(startDir);
  while (true) {
    if (fs.existsSync(path.join(current, "Cargo.toml"))) {
      roots.push(current);
    }
    const parent = path.dirname(current);
    if (parent === current) {
      break;
    }
    current = parent;
  }
  return roots;
}

function expandHome(value: string): string {
  if (value === "~") {
    return os.homedir();
  }
  if (!value.startsWith("~/") && !value.startsWith("~\\")) {
    return value;
  }
  return path.join(os.homedir(), value.slice(2));
}

function resolveBinaryPath(value: string, workspaceCwd?: string): string {
  if (!looksLikePath(value)) {
    return value;
  }
  if (path.isAbsolute(value)) {
    return value;
  }
  return path.resolve(workspaceCwd ?? process.cwd(), value);
}

function validateBinaryPath(value: string, source: string): boolean {
  if (!looksLikePath(value) || fs.existsSync(value)) {
    return true;
  }
  const message = `${source} does not exist: ${value}`;
  log(message);
  void vscode.window.showErrorMessage(`tsc-rs: ${message}`);
  return false;
}

function looksLikePath(value: string): boolean {
  return value.includes("/") || value.includes("\\");
}

function basename(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || p;
}

export function deactivate(): Thenable<void> | undefined {
  log("tsc-rs extension deactivating...");
  return clientLifecycle.then(
    () => stopClient(),
    () => stopClient(),
  );
}
