---
title: Limitations
---
# Limitations

tsc-rs is not a drop-in replacement for `tsc` yet. This page lists the differences you are most likely to meet. The [conformance report](/conformance) puts numbers on each of them.

## Type checking

- **The type checker is incomplete.** It misses some diagnostics that `tsc` reports and reports some that `tsc` does not. On the last full measurement, recall was {{metrics.accuracy.compiler.recall}} on the compiler suite and {{metrics.accuracy.conformance.recall}} on the conformance suite, and precision was {{metrics.accuracy.compiler.precision}} and {{metrics.accuracy.conformance.precision}}.
- **Messages and positions can differ.** A whole-case diagnostic match needs every message, position and code to agree with `tsc`. That holds for roughly seven in ten compiler cases and six in ten conformance cases; the [conformance report](/conformance) has the current figures.
- **Unbounded type expansion.** Some inputs trigger unbounded type expansion in the checker. The process caps its own address space (`TSC_RS_MAX_VMSIZE_MB`, 12 GB by default) so that such a case fails instead of exhausting the machine.

Treat a clean tsc-rs run as a fast first pass, not as proof that `tsc` would also be clean.

## Emit

- **Default JavaScript emit matches `tsc`** for every test case that has a JavaScript oracle.
- **Option variants are not all covered.** With every stored option variant expanded, {{metrics.expanded.js.percent}} of JavaScript variants pass. The failures are mostly ES5 downlevel transforms.
- **Declaration emit is early.** `.d.ts` output can differ from `tsc`; {{metrics.expanded.declarations.percent}} of expanded declaration variants pass.
- **No output on type errors.** When the checker reports errors, tsc-rs 0.4.1 writes no output files, where `tsc` emits by default. Use `--transpileOnly` to emit without checking.

## Project builds

These were checked by running version 0.4.1. [Projects and watch](/docs/projects) has the detail.

- **`outDir` is flat.** Source subdirectories are not recreated, so relative imports between nested files break and files with the same base name overwrite each other.
- **`-b` checks but does not emit.** Build mode orders and type-checks project references and writes no files.
- **A failed incremental build is recorded as built.** Rerunning the same command after a type error reports nothing and exits 0.
- **React projects.** With `@types/react` installed, a checked build can stop on an error reported inside the type package. Use `--transpileOnly` for emit. See [JSX](/docs/jsx).

## Language server

The language server passes {{metrics.lsp.percent}} of the `fourslash` checks it is run against. The gaps are mostly type inference: hover misses come from contextual typing, generics, JSDoc type tags and cross-file aliases, and completion misses come from auto-imports and cross-file members.

## Command line

The CLI is a subset of `tsc`. The supported flags are listed in the [CLI reference](/docs/cli) and the supported `compilerOptions` in [tsconfig support](/docs/tsconfig). An option that is not listed there is not applied.

## Symbols

Symbol baselines are early: {{metrics.symbols.compiler.percent}} of compiler cases and {{metrics.symbols.conformance.percent}} of conformance cases match the `tsc` `.symbols` baseline.

## Distribution

- tsc-rs is built from source. The npm package `@bext-stack/tsc-rs` ships a prebuilt binary for Linux x64 only, and follows its own release schedule.
- The WebAssembly crate is not published to npm; you build it yourself. See [WebAssembly](/docs/wasm).

## Reporting a difference

If tsc-rs and `tsc` disagree on your code, a small reproduction is valuable. [Open an issue](https://github.com/benfavre/ts-rs/issues) with the input, the `tsc` output and the tsc-rs output.
