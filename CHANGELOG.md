# Changelog

Day-to-day changes are recorded per wave in
[`docs/verified-waves.md`](docs/verified-waves.md) and on the
[progress page](https://ts-rs.bext.dev/progress). This file summarizes
versions.

## 0.4.2 (2026-10-04)

Public distribution update based on the October 2 source snapshot:

- GitHub release binaries for Linux x64/ARM64, macOS Intel/Apple Silicon,
  and Windows x64, with SHA256 checksums and the VS Code extension.
- Linux x64 npm packages `@bext-stack/tsc-rs` and
  `@bext-stack/tsc-rs-linux-x64` updated to 0.4.2.
- Browser playground WebAssembly package rebuilt at 0.4.2.
- Release workflow uses a native Linux ARM runner and supports rebuilding an
  existing tag without moving it.

This release updates version and distribution metadata. Compatibility figures
remain the measured October 2 snapshot in `docs/compatibility-metrics.json`.

## 0.4.1

First version published as a public repository (2026-09-29). Figures measured
on 2026-09-30 at `c3c3940e9`, cache disabled:

- **JavaScript emit**: 11,420 of 11,420 `tsc` `.js` baselines match byte for
  byte across the compiler and conformance suites (1,016 cases have no `.js`
  oracle and are skipped).
- **Diagnostics**: 8,361 of 12,436 cases match every message, position and
  code (compiler 4,752 / 6,529, conformance 3,609 / 5,907).
- **Language server**: 1,747 of 2,363 `fourslash` checks pass across hover,
  completions, go-to-definition, find-all-references and signature help.
- **Transpile modes**: `--transpileOnly`, `--fast-emit`, the persistent
  `--pipe` transpile pipe and the `--check-pipe` type-check daemon.
- **Monorepo analysis**: `tsc-rs analyze dep-graph` and `analyze externals`.
- **WebAssembly build** (`crates/tsc_rs_wasm`) and a browser
  [playground](https://ts-rs.bext.dev/playground).
- The emit gaps listed under 0.1.0 below (class decorators, `__esDecorate`,
  private field compound assignment) are closed: every default emit baseline
  passes.

Known limitations: declaration (`.d.ts`) emit and symbol baselines are early,
the type checker is incomplete, and the CLI surface is a subset of `tsc`.

## 0.1.0

Historical notes from the first internal milestone, kept for reference. The
figures and limitations below describe that milestone, not the current tree.

### Highlights

- **91% baseline parity** with the TypeScript compiler on the official test suite (5,934 / 6,529 compiler tests passing, 4,620 / 5,907 conformance tests passing)
- **Diagnostic parity wave** improves location-aware conformance recall/precision
  to 45.5960%/70.6079% and compiler recall/precision to
  43.2690%/63.8883%; verification and remaining precision work are recorded in
  [`docs/diagnostic-parity-progress.md`](docs/diagnostic-parity-progress.md)
- **Full LSP server** with diagnostics, hover, go-to-definition, find references, completions, rename, and more
- **VSCode extension** included (`editors/vscode/`)
- **Preserve mode** options for keeping type annotations, comments, and whitespace in emitted JavaScript
- **Project references** (`-b` flag) and incremental build support via `.tsbuildinfo`
- **Source map** generation

### Features

- TypeScript-to-JavaScript emit for ES3 through ESNext targets
- CommonJS, ESM, AMD, UMD, and SystemJS module output
- Enum constant folding with cross-enum evaluation
- String enum constant folding (including NumLit and template literals)
- `react-jsx` / `react-jsxdev` JSX transform
- `__setFunctionName` helper for class expressions with static fields
- Legacy decorator emit (`experimentalDecorators`)
- Private field downleveling via WeakMap
- Async/await and generator downleveling
- Rest/spread parameter transforms
- `import` / `export` elision for type-only imports
- `--preserveTypeAnnotations`, `--preserveComments` output modes

### Known Limitations

- **Declaration emit (`.d.ts`)** is incomplete; `.d.ts` output may differ from `tsc`
- **Class-level `__decorate`** (legacy decorators on classes) is not yet implemented
- **Type checker** is partial; some diagnostic messages may differ from `tsc`
- ~91 compiler test failures and ~768 conformance test failures remain, primarily in: parser error recovery, missing `__esDecorate` transform, private field compound assignment, and comment/whitespace differences
