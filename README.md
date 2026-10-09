<div align="center">

# tsc-rs

**A TypeScript compiler, type checker and language server, written in Rust.**

[Website](https://ts-rs.bext.dev/) ·
[Documentation](https://ts-rs.bext.dev/docs) ·
[Playground](https://ts-rs.bext.dev/playground) ·
[Conformance report](https://ts-rs.bext.dev/conformance) ·
[Progress](https://ts-rs.bext.dev/progress) ·
[Sponsor](#sponsors)

[![CI](https://github.com/benfavre/ts-rs/actions/workflows/rust-ci.yml/badge.svg)](https://github.com/benfavre/ts-rs/actions/workflows/rust-ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![npm](https://img.shields.io/npm/v/@bext-stack/tsc-rs?label=npm)](https://www.npmjs.com/package/@bext-stack/tsc-rs)

</div>

`tsc-rs` implements the TypeScript compiler end to end: scanner, parser,
binder, type checker, JavaScript/declaration emitter and a language server, in
one Cargo workspace. Every change is measured against the upstream TypeScript
test suites (`compiler`, `conformance`, `fourslash`), with the real `tsc`
baselines as the oracle.

- **Emit matches `tsc` byte for byte** on all 11,420 JavaScript baselines.
- **Diagnostics reproduce `tsc`'s messages, positions and codes**; two thirds
  of the upstream cases match in full, and the rest is the active front.
- **Language server and VS Code extension**: hover, go-to-definition,
  references, completions, signature help, rename, formatting.
- **Tooling modes**: a persistent transpile pipe, a type-check daemon, preserve
  modes that keep types and comments in the output, and a WebAssembly build.
- **In production**: it is the TypeScript front end of the
  [bext](https://bext.dev) engine.

You can try it without installing anything in the
[browser playground](https://ts-rs.bext.dev/playground).

## A quick look

```console
$ cat user.ts
interface User {
  id: number;
  name: string;
}

function greet(user: User): string {
  return "Hello, " + user.nmae;
}

const total: number = "42";

$ tsc-rs --noEmit --strict user.ts
user.ts(7,27): error TS2339: Property 'nmae' does not exist on type 'User'.
user.ts(10,7): error TS2322: Type 'string' is not assignable to type 'number'.

Found 2 errors in 1 file.
```

## Status at a glance

All results below were measured on 2026-10-09 at `cb31e5f25`, with the
result cache disabled. Pass rates are `passed / (passed + failed)`; skipped
cases are never counted as passes.

| Lane | Compiler suite | Conformance suite | What it measures |
|---|---:|---:|---|
| JavaScript emit | 6,032 / 6,032 (100%) | 5,388 / 5,388 (100%) | Byte-for-byte match with the `tsc` `.js` baseline (one configuration per case) |
| Diagnostics | 4,793 / 6,529 (73.4%) | 3,646 / 5,907 (61.7%) | Whole-case match with the `tsc` `.errors.txt` baseline (message, position, code) |
| Symbols | 1,722 / 6,434 (26.8%) | 1,568 / 5,617 (27.9%) | Match with the `tsc` `.symbols` baseline |
| Types | 604 / 6,434 (9.4%) | 558 / 5,617 (9.9%) | Match with the `tsc` `.types` baseline |
| Type-check recall / precision | 59.6% / 85.7% | 67.0% / 88.4% | Per-diagnostic `(file, line, code)` match over every option variant |

JavaScript emit skips 497 compiler and 519 conformance cases without a `.js`
oracle (mostly `noEmit`); diagnostics exercises all of them. Symbols and types
skip 95 compiler and 290 conformance cases without a uniquely selected oracle.

Public workspace validation: **3,429 Rust tests passed**, zero failed, and
37 ignored; `make ci` passes.

Language server, against the `fourslash` suite:

| Operation | Passed | Failed | Skipped | Pass rate |
|---|---:|---:|---:|---:|
| QuickInfo (hover) | 301 | 228 | 10 | 56.9% |
| Completions | 859 | 272 | 0 | 76.0% |
| Go-to-definition | 191 | 20 | 2 | 90.5% |
| Find-all-references | 329 | 18 | 0 | 94.8% |
| Signature help | 123 | 22 | 7 | 84.8% |
| **Supported operations** | **1,803** | **560** | **19** | **76.3%** |

The full inventory contains 6,320 cases: 1,803 pass, 560 fail, and 3,957 are
skipped, including unsupported operations.

Expanded option variants (14,819 identities, 1,016 skips per lane):

| Lane | Passed | Failed | Pass rate |
|---|---:|---:|---:|
| JavaScript | 12,832 | 971 | 93.0% |
| Declarations | 12,146 | 1,657 | 88.0% |

Remaining diagnostic mismatches: 1,736 compiler cases and 2,261 conformance
cases. Expanded emit failures are mainly ES5 transforms and declarations;
LSP gaps include contextual typing, generics, JSDoc and cross-file information.
Symbol and type baselines still have substantial gaps.

[docs/compatibility-metrics.json](docs/compatibility-metrics.json) contains the
current counts, measurement commit, report hashes and reproduction commands.
The [September 7 snapshot](docs/compatibility-metrics-2026-09-07.json) remains
available as historical evidence.

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

The [conformance report](https://ts-rs.bext.dev/conformance) and
[progress page](https://ts-rs.bext.dev/progress) track these numbers over time.
[docs/verified-waves.md](docs/verified-waves.md) records each change with its
regression checks. [website/](website/) contains the site source and its
publishing procedure: push code and metrics to public `main` before syncing
the site from that published snapshot.

## Installation

Build from source with a stable Rust toolchain:

```bash
git clone https://github.com/benfavre/ts-rs.git
cd ts-rs
cargo install --path crates/tsc_rs_cli      # installs `tsc-rs` into ~/.cargo/bin
# or
cargo build --release                        # target/release/tsc-rs
```

On Linux x86-64 the workspace links with `clang` and `lld` (see
`.cargo/config.toml`), so install both first, for example
`sudo apt install clang lld`. The repository vendors the TypeScript test corpus
under `tests/`, so the clone is large; that corpus is what makes the numbers
above reproducible.

Version 0.4.2 is available as [GitHub release downloads](https://github.com/benfavre/ts-rs/releases/tag/v0.4.2)
for Linux x64/ARM64, macOS Intel/Apple Silicon, and Windows x64. Each release
includes SHA256 checksums and the VS Code extension.

The npm package ships the Linux x64 compiler:

```bash
npm install --save-dev @bext-stack/tsc-rs@0.4.2
npx tsc-rs --version
```

See the [installation guide](https://ts-rs.bext.dev/docs/installation) for
details.

## Usage

```bash
tsc-rs file.ts                  # compile one file
tsc-rs -p tsconfig.json         # compile a project
tsc-rs -w -p tsconfig.json      # watch mode
tsc-rs -b tsconfig.json         # build project references
tsc-rs --noEmit --strict file.ts  # type-check only
tsc-rs --init                   # write a starter tsconfig.json
tsc-rs --lsp                    # language server over stdio
tsc-rs --help
```

Full flag list: [CLI reference](https://ts-rs.bext.dev/docs/cli).

### Transpile only

`--transpileOnly` skips type checking and only parses and emits. Parser errors
are still reported, so a broken file fails the build. `--fast-emit` also skips
formatting normalization: the output is valid JavaScript but not laid out the
way `tsc` would, and it runs two to four times faster.

```bash
tsc-rs --transpileOnly --jsx react-jsx --module esnext button.tsx
```

### Pipe and daemon modes

For bundlers, editors and agents, `tsc-rs` can stay alive and answer
newline-delimited JSON requests on stdin:

```console
$ echo '{"id":"1","file":"b.tsx","source":"export const Save = () => <Button label=\"Save\" />;","options":{"module":"esnext","jsx":"react-jsx"}}' | tsc-rs --pipe
{"ready":true,"pid":12345}
{"id":"1","ok":true,"output":"import { jsx as _jsx } from \"react/jsx-runtime\";\nexport const Save = () => _jsx(Button, { label: \"Save\" });\n","elapsed_ms":0,"exports":["Save"]}
```

`tsc-rs --check-pipe -p tsconfig.json` is the type-checking equivalent. See
[Transpile pipe](https://ts-rs.bext.dev/docs/pipe) and
[Type-check daemon](https://ts-rs.bext.dev/docs/check-pipe).

### Preserve modes

`tsc` strips all type information. `tsc-rs` can keep some of it in the output
for runtime type libraries, documentation tools and refactoring tools:

```bash
tsc-rs --preserveTypeAnnotations file.ts   # keep `as`, `satisfies`, `<Type>`
tsc-rs --preserveComments --removeComments file.ts
tsc-rs --preserveWhitespace file.ts        # keep original layout (partial)
```

### Monorepo analysis

```bash
tsc-rs analyze dep-graph      # dependency fan-out from entry files
tsc-rs analyze externals      # audit serverExternalPackages against the import graph
```

See [docs/ANALYZE.md](docs/ANALYZE.md).

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

Start it with `tsc-rs --lsp` from any LSP-capable editor. It supports:

- diagnostics on open and edit
- hover, go-to-definition and find references, within a file and across files
- completions: scope, members, module paths, auto-import statements, JSX, JSDoc
- signature help
- rename, document symbols and workspace symbol search
- code actions (remove unused variable) and formatting (via the emitter)
- incremental text sync with a per-file parse/bind/check cache
- tsconfig / jsconfig project awareness, multi-root workspaces and
  watched-file refresh

How closely each operation matches `tsc` is in the table above.

### VS Code extension

```bash
cd editors/vscode
npm install && npm run compile && npx vsce package
code --install-extension tsc-rs-0.4.2.vsix
```

The extension auto-detects `target/debug/tsc-rs` or `target/release/tsc-rs` in
the workspace or an ancestor Cargo workspace, then `TSC_RS_SERVER_PATH`, then
`tsc-rs` on `PATH`. Override with `tsc-rs.serverPath` and `tsc-rs.serverArgs`;
restart with `tsc-rs: Restart Language Server`. It watches TS/JS sources and
`tsconfig*.json` / `jsconfig.json` and logs to the `tsc-rs` output channel.

## Workspace layout

The compiler is a plain pipeline, one crate per stage:

```
.ts/.tsx ─▶ scanner ─▶ parser ─▶ symbols ─▶ types ─▶ emitter ─▶ .js / .d.ts / .js.map
```

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

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) describes how the crates fit
together.

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
- The standard-library tests need the TypeScript 6.0.3 `lib.*.d.ts` files; see
  [CONTRIBUTING.md](CONTRIBUTING.md) for `TSC_RS_TYPESCRIPT_LIB_DIR`.

## Performance

The parser is competitive with the fastest native TypeScript parsers. In one
same-session comparison on the 7.8 MB `typescript.js` fixture:

| | tsc-rs | oxc |
|---|---:|---:|
| Parse | 44.5 ms | 35.2 ms |
| Parse and semantic analysis | 96.0 ms | 97.9 ms |

oxc parses faster; `tsc-rs` is ahead once binding and symbol resolution are
included, and it parses faster than SWC on every file in the benchmark set.
The benchmarks live in `crates/tsc_rs_bench` (`make bench`, `make bench-vs-oxc`).

Checker changes are measured before and after on 1,000 compiler diagnostic
cases (one thread, no cache); the method and raw timings are in
[docs/verified-waves.md](docs/verified-waves.md).

## Limitations

- Not a drop-in `tsc` replacement yet: diagnostics, declaration emit and
  expanded option variants are not at parity, and the CLI surface is a subset.
- The type checker is incomplete. Expect missing errors and some false ones on
  advanced code; hover types can differ from `tsc`. The tables above quantify
  this.
- `.d.ts` emit and the symbol baselines are the least mature lanes.

## Sponsors

`tsc-rs` is sponsored by:

- [Webdesign29](https://www.webdesign29.net/), a web and mobile agency based in
  Brest, France.
- [Inklura](https://www.inklura.fr/), all-in-one business management software.

We are looking for more sponsors. Sponsorship pays for the time spent closing
the gaps in the tables above: type-checker parity, declaration emit and the
language server. If your company depends on fast TypeScript tooling, you can
sponsor the project through
[GitHub Sponsors](https://github.com/sponsors/benfavre), or
[open an issue](https://github.com/benfavre/ts-rs/issues/new) to talk about
other arrangements.

## More documentation

The [website](https://ts-rs.bext.dev/docs) has the guides: quick start, CLI
reference, tsconfig support, JSX, the language server, the pipe and daemon
protocols, WebAssembly, the Rust library API and the npm package.

In this repository:

- [CONTRIBUTING.md](CONTRIBUTING.md): local workflow
- [CHANGELOG.md](CHANGELOG.md): release notes
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): architecture
- [docs/QUERY_API_REFERENCE.md](docs/QUERY_API_REFERENCE.md): query API
- [docs/DIAGNOSTICS.md](docs/DIAGNOSTICS.md): diagnostics
- [docs/ANALYZE.md](docs/ANALYZE.md): monorepo analysis tools
- [docs/verified-waves.md](docs/verified-waves.md): verified changes, regression checks and measured performance
- [docs/compatibility-metrics.json](docs/compatibility-metrics.json): current verified compatibility snapshot

## License

MIT; see [LICENSE](LICENSE). The vendored TypeScript test corpus and the ports
listed in [NOTICE](NOTICE) are under Apache-2.0
([LICENSE-APACHE-2.0](LICENSE-APACHE-2.0)).

`tsc-rs` is an independent project. TypeScript is a trademark of Microsoft
Corporation.
