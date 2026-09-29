# tsc-rs for Visual Studio Code

TypeScript language support powered by the tsc-rs language server.

## Prerequisites

Install or build the `tsc-rs` binary:

```bash
cargo install --path crates/tsc_rs_cli
```

For local development inside this repository, the extension also auto-detects:

```bash
target/debug/tsc-rs
target/release/tsc-rs
```

If you open a nested folder such as `editors/vscode`, the extension also walks up to ancestor Cargo workspace roots and checks their `target/debug` and `target/release` builds.

## Installation

1. Build and install the extension:

   ```bash
   cd editors/vscode
   npm install
   npm run compile
   npm run package
   code --install-extension tsc-rs-0.1.0.vsix
   ```

2. Open a TypeScript or JavaScript project in VSCode.

## Configuration

| Setting            | Default    | Description                        |
| ------------------ | ---------- | ---------------------------------- |
| `tsc-rs.serverPath` | `""` | Explicit path to the `tsc-rs` binary. Relative paths resolve from the first workspace folder. |
| `tsc-rs.serverArgs` | `[]` | Extra args appended after `--lsp`. |
| `tsc-rs.preferWorkspaceBinary` | `true` | Prefer `target/debug/tsc-rs` or `target/release/tsc-rs` in the current workspace when neither `tsc-rs.serverPath` nor `TSC_RS_SERVER_PATH` is set. |

If `tsc-rs` is not on your PATH, set the full path in your VSCode settings:

```json
{
  "tsc-rs.serverPath": "/path/to/tsc-rs"
}
```

If `tsc-rs.serverPath` is unset, the extension next checks `TSC_RS_SERVER_PATH`, then workspace binary auto-detection, and finally falls back to `tsc-rs` on `PATH`.

The extension exposes `tsc-rs: Restart Language Server` from the command palette, automatically restarts when `tsc-rs` settings change, watches project files and `tsconfig*.json` / `jsconfig.json` for external changes, serializes start/stop/restart/shutdown work to avoid overlapping client lifecycles, and surfaces startup details and launch failures in the `tsc-rs` output channel. Workspace-folder add/remove changes are handled directly by the server; the extension only forces a restart there when workspace-binary auto-detection could select a different `tsc-rs` executable.

## Packaging Notes

`npm run compile` typechecks the extension and bundles it into `dist/extension.js`, so the packaged VSIX does not need to ship the runtime `node_modules` tree.
