# tsc-rs

`tsc-rs` is a TypeScript compiler written in Rust: scanner, parser, binder,
type checker, JavaScript/declaration emitter, and a Language Server, all in one
Cargo workspace. It is measured continuously against the upstream TypeScript
test suites (`compiler`, `conformance`, `fourslash`) using the real `tsc`
baselines as the oracle.

It is a tooling-first compiler. Beyond standard `tsc`-compatible output it can
keep type annotations, comments, and whitespace in the emitted JavaScript for
downstream tools, and it ships an LSP server with a VS Code extension.

## Status at a glance

JavaScript and diagnostic baselines below were measured on 2026-09-08 for the
[public constructor overload wave](docs/verified-waves.md#2026-09-08-public-constructor-overloads),
with cache disabled. Pass rates are `passed / (passed + failed)`; skipped
cases are never counted as passes.

| Lane | Compiler suite | Conformance suite | What it measures |
|---|---:|---:|---|
| JavaScript emit | 6,032 / 6,032 (100%) | 5,388 / 5,388 (100%) | Byte-for-byte match with the `tsc` `.js` baseline (one configuration per case) |
| Diagnostics | 4,617 / 6,529 (70.7%) | 3,418 / 5,907 (57.9%) | Whole-case match with the `tsc` `.errors.txt` baseline (message, position, code) |

The following measurements are retained from the earlier `65efe57e8` README
snapshot and have not been rerun for this wave:

| Lane | Compiler suite | Conformance suite | What it measures |
|---|---:|---:|---|
| Type-check recall / precision | 53.8% / 83.3% | 60.5% / 86.3% | Per-diagnostic `(file, line, code)` match over every option variant |
| Symbols | 185 / 6,434 (2.9%) | 293 / 5,617 (5.2%) | Match with the `tsc` `.symbols` baseline |

Skips per lane: JavaScript emit skips 497 compiler and 519 conformance cases
that have no `.js` oracle (mostly `noEmit`); the diagnostics lane exercises all
of them instead. The earlier symbols measurement skips 95 and 290 cases
without a uniquely selected oracle.

Last recorded language-server results against `fourslash` (not rerun in this wave):

| Operation | Passed | Failed | Skipped | Pass rate |
|---|---:|---:|---:|---:|
| QuickInfo (hover) | 250 | 279 | 10 | 47.3% |
| Completions | 839 | 292 | 0 | 74.2% |
| Go-to-definition | 172 | 39 | 2 | 81.5% |
| Find-all-references | 305 | 42 | 0 | 87.9% |
| Signature help | 120 | 25 | 7 | 82.8% |
| **Total** | **1,686** | **677** | **19** | **71.3%** |

Where the work is:

- **Default emit matches** every case with a JavaScript oracle.
  The opt-in report that expands every stored option variant (14,819
  identities, measured 2026-09-07 at `aba3839d5`) passes 93.0% of JavaScript
  variants and 88.0% of declaration-projection variants; the failures are
  mostly ES5 downlevel transforms and `.d.ts` emit.
- **Diagnostics are the active front.** Whole-case passes require every
  message and column to match, which is where the
  remaining 1,912 compiler and 2,489 conformance failures come from
  (4,401 diagnostic mismatches in total).
- **LSP gaps are type inference**: hover misses are contextual typing,
  generics, JSDoc type tags, and cross-file aliases; completion misses are
  auto-imports and cross-file members.
- **Symbols and declaration emit** are early.

Reproduce any row:

```bash
cargo build --release -p tsc_rs_harness
# JavaScript emit and diagnostics (drop --baseline errors for emit)
target/release/baseline-report --suite compiler --baseline errors --no-cache
target/release/baseline-report --suite conformance --baseline errors --no-cache
# Symbols
target/release/baseline-report --suite compiler --baseline symbols --no-cache
# Type-check recall / precision (run the full corpus; --limit samples alphabetically and is biased)
target/release/typecheck-report --suite compiler --json
# Language server
target/release/lsp-report --op all --json
```

[docs/verified-waves.md](docs/verified-waves.md) records the latest wave results
and validation. `docs/compatibility-metrics.json` retains the historical
2026-09-07 snapshot at `aba3839d5`, including expanded-variant measurements;
those lanes have not been rerun for the current wave.

Local validation for this wave: **3,514 Rust tests passed**, zero failed, and
37 ignored; `make ci` and workspace Clippy succeeded (existing warnings).
GitHub Actions jobs on the preceding main commit could not start because of
the account billing/spending limit, so these are local validation results.

## Installation

```bash
cargo install --path crates/tsc_rs_cli      # installs `tsc-rs`
# or
cargo build --release                        # target/release/tsc-rs
```

Pre-built Linux, macOS, and Windows binaries are on the
[GitHub Releases](https://github.com/benfavre/ts-rs/releases) page.

## Usage

```bash
tsc-rs file.ts                  # compile one file
tsc-rs -p tsconfig.json         # compile a project
tsc-rs -w -p tsconfig.json      # watch mode
tsc-rs -b tsconfig.json         # build project references
tsc-rs --init                   # write a starter tsconfig.json
tsc-rs --lsp                    # language server over stdio
tsc-rs --help
```

### Preserve modes

`tsc` strips all type information. `tsc-rs` can keep some of it in the output
for runtime type libraries, documentation tools, and refactoring tools:

```bash
tsc-rs --preserveTypeAnnotations file.ts   # keep `as`, `satisfies`, `<Type>`
tsc-rs --preserveComments --removeComments file.ts
tsc-rs --preserveWhitespace file.ts        # keep original layout (partial)
```

### tsconfig support

`tsc-rs` applies the common `compilerOptions`:

- target and modules: `target`, `module`, `moduleResolution`, `jsx`
- emit: `noEmit`, `declaration`, `sourceMap`, `inlineSourceMap`, `outDir`, `outFile`
- strictness: `strict`, `noImplicitAny`, `noImplicitReturns`, `noUnusedLocals`,
  `noUnusedParameters`, `strictNullChecks`, `strictFunctionTypes`
- interop and features: `esModuleInterop`, `allowSyntheticDefaultImports`,
  `allowJs`, `checkJs`, `resolveJsonModule`, `experimentalDecorators`,
  `emitDecoratorMetadata`
- incremental and projects: `incremental`, `composite`, `tsBuildInfoFile`,
  project references (`-b`), `.tsbuildinfo`
- paths: `baseUrl`, `paths`, `lib`, `rootDir`, plus `files` / `include` /
  `exclude` and `extends`
- preserve modes: `preserveTypeAnnotations`, `preserveComments`, `preserveWhitespace`

Dependent-option and module-resolution conflicts are reported with the `tsc`
codes (TS5052, TS5095, TS5109, TS5110, and others).

## Language server

Start it with `tsc-rs --lsp` from any LSP-capable editor. Supported:

| Feature | Status |
|---|---|
| Diagnostics on open and edit | Working |
| Hover, go-to-definition, find references (single and cross-file) | Working |
| Completions: scope, members, module paths, auto-import statements, JSX, JSDoc | Working |
| Signature help | Working |
| Rename, document symbols, workspace symbol search | Working |
| Code actions (remove unused variable) | Working |
| Formatting (via the emitter) | Working |
| Incremental text sync, per-file parse/bind/check cache | Working |
| tsconfig / jsconfig project awareness, multi-root workspaces, watched-file refresh | Working |
| Built-in `lib.d.ts` fallback stubs | Working |

### VS Code extension

```bash
cd editors/vscode
npm install && npm run compile && npx vsce package
code --install-extension tsc-rs-0.1.0.vsix
```

The extension auto-detects `target/debug/tsc-rs` or `target/release/tsc-rs` in
the workspace or an ancestor Cargo workspace, then `TSC_RS_SERVER_PATH`, then
`tsc-rs` on `PATH`. Override with `tsc-rs.serverPath` and `tsc-rs.serverArgs`;
restart with `tsc-rs: Restart Language Server`. It watches TS/JS sources and
`tsconfig*.json` / `jsconfig.json` and logs to the `tsc-rs` output channel.

## Workspace layout

| Crate | Role |
|---|---|
| `tsc_rs_ast` | AST, spans, diagnostics, compiler options |
| `tsc_rs_scanner` | Lexer |
| `tsc_rs_parser` | Parser with `tsc`-compatible error recovery |
| `tsc_rs_symbols` | Binder and symbol table (declaration merging, overloads) |
| `tsc_rs_types` | Type checker: inference, narrowing, relations, diagnostics |
| `tsc_rs_resolver` | Module resolution |
| `tsc_rs_emitter` | JavaScript, source map, and declaration emit |
| `tsc_rs_project` | tsconfig loading, project graph, project references, `.tsbuildinfo` |
| `tsc_rs_incremental` | Incremental build metadata and change detection |
| `tsc_rs_query` | Query facade over parser/binder/checker used by tooling |
| `tsc_rs_server` | LSP server |
| `tsc_rs_cli` | The `tsc-rs` binary |
| `tsc_rs_harness` | Baseline harness: `baseline-report`, `baseline-case`, `typecheck-report`, `lsp-report`, `lsp-case`, `borrow-plan`, `baseline-compare` |
| `tsc_rs_bench` | Scanner, parser, and emitter benchmarks and profiling examples |
| `tsc_rs_wasm` | WebAssembly build of the compiler |
| `tsc_rs_analyze` | Static analysis tools for TypeScript monorepos |
| `tsc_rs_constraints`, `tsc_rs_control_flow` | Experimental constraint-graph and control-flow-graph primitives |
| `editors/vscode` | VS Code extension (language client) |

## Development

Everyday commands (see `make help` for more):

```bash
cargo test                                   # workspace unit and integration tests
make check                                   # cargo check + fmt check

# Emit: one case, first mismatch with context (~1 s)
cargo run -p tsc_rs_harness --bin baseline-case -- <testName> --suite compiler --show-first-mismatch --context-lines 5
# Diagnostics: one case, full expected/actual
cargo run -p tsc_rs_harness --bin baseline-case -- <testName> --suite compiler --baseline errors --show-outputs

# Whole suite, bucketed failures (cached after the first run)
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --top 20
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --incremental   # rerun failures + a sample, ~0.3 s

# Type-check accuracy: where recall leaks, per TS code
cargo run --release -p tsc_rs_harness --bin typecheck-report -- --suite compiler
cargo run --release -p tsc_rs_harness --bin typecheck-report -- --code 2304 --show-cases 20

# Language server
cargo run -p tsc_rs_harness --bin lsp-report -- --op quickinfo
cargo run -p tsc_rs_harness --bin lsp-case -- <testName> --show-outputs
```

Harness notes:

- `baseline-report` caches classified results in `.harness-cache/`, keyed on a
  hash of the compiler crate sources; use `--no-cache` for a fresh run.
- `--baseline js|errors|symbols|declarations` selects the oracle lane;
  `--expand-variants --manifest-schema 2 --save-manifest <file>` inventories
  every stored option variant with a stable identity.
- Oracles are selected from the effective compiler options, never from
  whichever candidate output happens to match. Missing oracles are skips,
  never passes.
- Some type-check cases trigger unbounded type expansion; if a
  multi-threaded `typecheck-report` run is OOM-killed, rerun with
  `RAYON_NUM_THREADS=1`.

## Performance

The latest constructor wave was compared with `f9d5802a9` using 1,000 compiler
diagnostic cases, release-fast builds, one thread, no cache, and three measured
pairs after warmup. Median wall time was 14.55 → 14.74 seconds (+1.3%), CPU
14.62 → 14.81 seconds (+1.3%), and peak RSS 48,232 → 48,732 KiB (+1.0%).
Timing ranges overlap. One pair with detected background Cargo activity was
replaced; the [wave log](docs/verified-waves.md#2026-09-08-public-constructor-overloads)
records the method and limitations.

The parser is competitive with the fastest native TypeScript parsers. On the
7.8 MB `typescript.js` fixture, in one same-session comparison, `tsc-rs` parsed
in 44.5 ms against 35.2 ms for oxc, and parse plus semantic analysis was
96.0 ms against 97.9 ms. `tsc-rs` beats SWC on every benchmark file. Benchmarks live in
`crates/tsc_rs_bench`.

Transpile-only emit (`--fast-emit`) skips type checking and runs two to four
times faster than the structured path.

## Limitations and non-goals

- Not a drop-in `tsc` replacement yet: diagnostics, declaration emit, and
  expanded option variants are not at parity, and the CLI surface is a subset.
- The type checker is incomplete; hover types and some diagnostics differ from
  `tsc`, as the tables above quantify.
- `.d.ts` emit and symbol baselines are early.

## More documentation

- [CONTRIBUTING.md](CONTRIBUTING.md): local workflow
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): architecture
- [docs/QUERY_API_REFERENCE.md](docs/QUERY_API_REFERENCE.md): query API
- [docs/DIAGNOSTICS.md](docs/DIAGNOSTICS.md): diagnostics
- [docs/ANALYZE.md](docs/ANALYZE.md): monorepo analysis tools
- [docs/verified-waves.md](docs/verified-waves.md): verified changes, regression checks, and measured performance
- [docs/compatibility-metrics.json](docs/compatibility-metrics.json): historical metrics snapshot at `aba3839d5`
