---
title: Architecture
---
# Architecture

tsc-rs is a Cargo workspace. The compiler is a plain pipeline of five crates, and the tooling crates are built on top of those five.

## The pipeline

| Stage | Crate | Role |
|---|---|---|
| 1 | `tsc_rs_scanner` | Lexer: source text to tokens |
| 2 | `tsc_rs_parser` | Parser with `tsc`-compatible error recovery: tokens to AST |
| 3 | `tsc_rs_symbols` | Binder and symbol table, including declaration merging and overloads |
| 4 | `tsc_rs_types` | Type checker: inference, narrowing, relations, diagnostics |
| 5 | `tsc_rs_emitter` | JavaScript, source map and declaration emit |

All five share `tsc_rs_ast`, which defines the AST, spans, diagnostics and compiler options.

Emit does not depend on the checker. A transpile-only run is scanner, parser, emitter; type checking adds the binder and the checker in between.

## Using the crates as a library

The WebAssembly bindings are a compact example of driving the pipeline by hand. Transpile:

```rust
let source_file = tsc_rs_parser::parse("input.ts", source);
let options = tsc_rs_ast::CompilerOptions::default();
let output = tsc_rs_emitter::emit(&source_file, &options);

println!("{}", output.javascript);
```

Type-check one file:

```rust
let source_file = tsc_rs_parser::parse("input.ts", source);
let symbols = tsc_rs_symbols::Binder::new().bind(&source_file);
let checked = tsc_rs_types::TypeChecker::new()
    .check_with_options(&source_file, &symbols, &options);

for diagnostic in source_file.diagnostics.iter().chain(checked.diagnostics.iter()) {
    // message, span and code
}
```

The crates are not published to crates.io. Depend on them through a git dependency:

```toml
[dependencies]
tsc_rs_parser = { git = "https://github.com/benfavre/ts-rs.git" }
tsc_rs_ast = { git = "https://github.com/benfavre/ts-rs.git" }
tsc_rs_emitter = { git = "https://github.com/benfavre/ts-rs.git" }
```

Pin a `rev` for reproducible builds. The internal APIs are not stable between commits.

## Projects and resolution

| Crate | Role |
|---|---|
| `tsc_rs_resolver` | Module resolution |
| `tsc_rs_project` | tsconfig loading, project graph, project references, `.tsbuildinfo` |
| `tsc_rs_incremental` | Incremental build metadata and change detection |

These turn a `tsconfig.json` into an ordered set of files with resolved imports, and decide what needs to be rebuilt.

## Tooling

| Crate | Role |
|---|---|
| `tsc_rs_cli` | The `tsc-rs` binary: compile, watch, build, pipe modes, analyze |
| `tsc_rs_server` | The LSP server behind `tsc-rs --lsp` |
| `tsc_rs_query` | A query facade over parser, binder and checker, used by tooling |
| `tsc_rs_analyze` | Static analysis for TypeScript monorepos |
| `tsc_rs_wasm` | WebAssembly build of the compiler |
| `editors/vscode` | VS Code extension (language client) |

## Measurement

| Crate | Role |
|---|---|
| `tsc_rs_harness` | Baseline harness: `baseline-report`, `baseline-case`, `typecheck-report`, `lsp-report`, `lsp-case`, `borrow-plan`, `baseline-compare` |
| `tsc_rs_bench` | Scanner, parser and emitter benchmarks and profiling examples |

See [Testing and the harness](/docs/testing).

## Experimental

`tsc_rs_constraints` and `tsc_rs_control_flow` hold experimental constraint-graph and control-flow-graph primitives. They belong to a longer-term design that turns the checker's implicit reasoning into explicit, queryable data. The design document is [docs/ARCHITECTURE.md](https://github.com/benfavre/ts-rs/blob/main/docs/ARCHITECTURE.md) in the repository; it describes intent, and parts of it predate the current code.

## Design choices

- **Same output as `tsc`.** The emitter is written to reproduce `tsc` formatting byte for byte, because the test oracle is `tsc`'s own baseline files.
- **Error recovery matters.** The parser recovers the way `tsc` does, so diagnostics on broken code can be compared too.
- **Diagnostics reuse `tsc` wording.** Messages and codes reproduce TypeScript's, so tools that parse `tsc` output keep working.
- **Parallel by default.** Files are processed across a Rayon thread pool sized to the physical cores; see `--jobs` in the [CLI reference](/docs/cli).
