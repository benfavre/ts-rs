---
title: VS Code extension
---
# VS Code extension

The repository includes a VS Code extension in `editors/vscode`. It is a language client: it starts `tsc-rs --lsp` and connects the editor to it. The extension is built from source; it is not on the Marketplace.

## Prerequisites

Build or install the `tsc-rs` binary first:

```bash
cargo install --path crates/tsc_rs_cli
```

## Build and install

```bash
cd editors/vscode
npm install
npm run compile
npx vsce package
code --install-extension tsc-rs-*.vsix
```

`npm run compile` type-checks the extension and bundles it into `dist/extension.js`, so the packaged VSIX does not ship a `node_modules` tree.

Open a TypeScript or JavaScript project. The extension activates for `typescript`, `typescriptreact`, `javascript` and `javascriptreact` files.

## How the binary is found

The extension looks for the server in this order:

1. The `tsc-rs.serverPath` setting. A relative path resolves from the first workspace folder.
2. The `TSC_RS_SERVER_PATH` environment variable.
3. A workspace build: `target/debug/tsc-rs` or `target/release/tsc-rs` in the workspace, or in an ancestor Cargo workspace.
4. `tsc-rs` on `PATH`.

If the binary is not on your `PATH`, set it explicitly:

```json
{
  "tsc-rs.serverPath": "/path/to/tsc-rs"
}
```

## Settings

| Setting | Default | Description |
|---|---|---|
| `tsc-rs.serverPath` | `""` | Explicit path to the `tsc-rs` binary |
| `tsc-rs.serverArgs` | `[]` | Extra arguments appended after `--lsp` |
| `tsc-rs.preferWorkspaceBinary` | `true` | Prefer a `target/debug` or `target/release` build from the current workspace when no explicit path is set |
| `tsc-rs.disableBuiltinTypescript` | `true` | Disable VS Code's built-in TypeScript and JavaScript language features while tsc-rs is active, to prevent duplicate diagnostics |
| `tsc-rs.hover.showDebugInfo` | `false` | Show symbol id, flags, offset and resolution path in hovers |
| `tsc-rs.hover.copyButton` | `true` | Show a Copy button in hovers to copy the type signature |
| `tsc-rs.activityLogFile` | `""` | Path to a JSONL file that records all LSP activity. Empty disables logging |

## Commands

Open the command palette and type `tsc-rs`:

| Command | What it does |
|---|---|
| Restart Language Server | Stop and start the server |
| Copy Type | Copy the type at the cursor |
| Show Competing TypeScript Extensions | List other extensions that provide TypeScript features |
| Copy Debug Info (Bug Report) | Copy a bundle of diagnostic details for an issue |

The extension also restarts the server when a `tsc-rs` setting changes, watches TypeScript and JavaScript sources plus `tsconfig*.json` and `jsconfig.json` for external changes, and writes startup details and launch failures to the `tsc-rs` output channel.

## Troubleshooting

- **Nothing happens.** Open the `tsc-rs` output channel. A launch failure prints the path it tried.
- **Duplicate errors.** Another TypeScript extension is active. Run `tsc-rs: Show Competing TypeScript Extensions`.
- **A hover looks wrong.** Turn on `tsc-rs.hover.showDebugInfo`, then use `tsc-rs: Copy Debug Info (Bug Report)` and attach the result to an issue.
