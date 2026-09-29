# Changelog

## 0.1.0 (Unreleased)

Initial public release of tsc-rs, a TypeScript-to-JavaScript compiler written in Rust.

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
