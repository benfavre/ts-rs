---
title: Introduction
---
# Introduction

tsc-rs is a TypeScript compiler written in Rust: scanner, parser, binder, type checker, JavaScript and declaration emitter, and a language server, all in one Cargo workspace.

## What it is for

tsc-rs is a tooling-first compiler. It aims to produce the same output as `tsc`, and then goes further in the directions that tools need:

- **Standard compilation.** Compile a file or a `tsconfig.json` project to JavaScript, with source maps, declarations, watch mode and project references.
- **Preserve modes.** Keep type annotations, comments or whitespace in the emitted JavaScript for runtime type libraries, documentation tools and refactoring tools. See [Preserve modes](/docs/preserve-modes).
- **Editor support.** A language server over stdio and a VS Code extension. See [Language server](/docs/language-server).
- **Embedding.** A persistent [transpile pipe](/docs/pipe), a [type-check daemon](/docs/check-pipe) and a [WebAssembly build](/docs/wasm) for bundlers, agents and browsers.
- **Analysis.** Parser-level audits of a monorepo's import graph. See [tsc-rs analyze](/docs/analyze).

## How it is measured

tsc-rs is measured continuously against the upstream TypeScript test suites (`compiler`, `conformance` and `fourslash`), using the baselines that the real `tsc` produced as the oracle. A case passes only when the output matches.

The README snapshot of {{metrics.measured}}:

| Lane | Compiler suite | Conformance suite |
|---|---|---|
| JavaScript emit | {{metrics.js.compiler.count}} | {{metrics.js.conformance.count}} |
| Diagnostics | {{metrics.errors.compiler.count}} | {{metrics.errors.conformance.count}} |

The [conformance report](/conformance) tracks the latest wave and has every lane, the method, and the commands to reproduce each row. The [progress page](/progress) shows the change wave by wave.

## What it is not

:::warning
tsc-rs is not a drop-in replacement for `tsc` yet. Diagnostics, declaration emit and expanded option variants are not at parity, and the command line surface is a subset.
:::

If you need a type checker you can trust as the last word on a codebase, keep running `tsc`. If you want a fast compiler you can embed, extend and read, or you want to help close the gap, read on. The [Limitations](/docs/limitations) page lists the known differences.

## Where to go next

- [Installation](/docs/installation): build the binary.
- [Quick start](/docs/quick-start): compile something.
- [CLI reference](/docs/cli): every flag.
- [Playground](/playground): try the compiler without installing anything.
