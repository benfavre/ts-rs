---
title: Language server
---
# Language server

tsc-rs includes a Language Server Protocol server. Start it over stdio from any LSP-capable editor:

```bash
tsc-rs --lsp
```

## Features

| Feature | Status |
|---|---|
| Diagnostics on open and edit | Working |
| Hover, go-to-definition, find references (single file and cross-file) | Working |
| Completions: scope, members, module paths, auto-import statements, JSX, JSDoc | Working |
| Signature help | Working |
| Rename, document symbols, workspace symbol search | Working |
| Code actions (remove unused variable) | Working |
| Formatting (through the emitter) | Working |
| Incremental text sync, per-file parse, bind and check cache | Working |
| tsconfig and jsconfig project awareness, multi-root workspaces, watched-file refresh | Working |
| Built-in `lib.d.ts` fallback stubs | Working |

"Working" means the feature is implemented. It does not mean every answer matches `tsc`; the next section measures that.

## Accuracy

The server is tested against TypeScript's `fourslash` suite. Measured {{metrics.measured}}:

{{metrics.lsp.table}}

The full inventory skips {{metrics.lsp.skipped}} cases, including unsupported operations. Skips are excluded from pass rates.

Navigation is the strongest area. Hover is the weakest: the misses come from contextual typing, generics, JSDoc type tags and cross-file aliases.

## Editor setup

### VS Code

Use the extension in `editors/vscode`. See [VS Code extension](/docs/vscode).

### Neovim

With Neovim 0.11 or later, register the server with the built-in LSP client:

```lua
-- ~/.config/nvim/lsp/tsc_rs.lua
return {
  cmd = { "tsc-rs", "--lsp" },
  filetypes = { "typescript", "typescriptreact", "javascript", "javascriptreact" },
  root_markers = { "tsconfig.json", "jsconfig.json", ".git" },
}
```

Then enable it from your `init.lua` with `vim.lsp.enable("tsc_rs")`.

### Helix

Add the server to `languages.toml`:

```toml
[language-server.tsc-rs]
command = "tsc-rs"
args = ["--lsp"]

[[language]]
name = "typescript"
language-servers = ["tsc-rs"]
```

### Any other editor

Point the editor's LSP client at the command `tsc-rs --lsp`, communicating over stdin and stdout, for TypeScript and JavaScript files.

:::note
Run one TypeScript language server per buffer. If your editor also starts `tsserver` or `typescript-language-server`, you will see duplicate diagnostics.
:::

## Measuring it yourself

```bash
cargo run -p tsc_rs_harness --bin lsp-report -- --op quickinfo
cargo run -p tsc_rs_harness --bin lsp-case -- <testName> --show-outputs
```

See [Testing and the harness](/docs/testing).
