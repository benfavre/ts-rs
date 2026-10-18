---
title: FAQ
---
# FAQ

Short answers to the questions that come up first. Each one links to the page with the detail.

## Is tsc-rs a drop-in replacement for tsc?

No. The published native snapshot of {{metrics.measured}} matched the JavaScript cases with an oracle in their default tested configuration. Expanded option variants, native diagnostics, declaration emit and the CLI are not at parity. External tsrs checking is a separate integration. The [conformance report](/conformance) has the numbers and [Limitations](/docs/limitations) lists the differences you are most likely to meet.

## What is it good for today?

- **Transpiling.** Native TypeScript and TSX to JavaScript, evaluated against the dated baseline configurations. This path runs in production inside the [bext](https://bext.dev) engine.
- **Embedding.** A [transpile pipe](/docs/pipe), a [type-check daemon](/docs/check-pipe), a [WebAssembly build](/docs/wasm) and [Rust crates](/docs/library) you can link.
- **Fast feedback.** A first-pass type check and a language server that start quickly. Treat the result as advisory and keep `tsc` as the final word.
- **Tooling.** [Preserve modes](/docs/preserve-modes) that keep type syntax in the output, and [import graph audits](/docs/analyze).

## How is it different from SWC, oxc or esbuild?

The native tsc-rs workspace includes a parser, emitter, experimental checker and language server. OXC also publishes tsrs, which current source builds can use as an external checker. Native transpilation and preservation modes remain in tsc-rs; selecting tsrs never selects upstream transpilation. See [External project checking](/docs/external-checker).

If all you need is fast transpilation, those tools are mature choices. tsc-rs is for cases where you want the types understood, not only erased, in Rust.

## What about Microsoft's native port of TypeScript?

Microsoft's native TypeScript compiler is a separate implementation. Its diagnostic results remain an independent reference; the tsrs integration does not establish full-project parity with it.

tsc-rs is an independent implementation with a different focus: a Rust codebase that tools can link as a library, modes that `tsc` does not have (preserve modes, the JSON pipes), and a compiler small enough to ship as WebAssembly.

## Which TypeScript version does it follow?

The test oracle is the TypeScript repository's own baselines, vendored under `tests/`. The standard-library tests and recent diagnostic work are verified against TypeScript 6.0.3.

## Does it write output when there are type errors?

No. When the checker reports errors, tsc-rs 0.4.1 writes no output files, where `tsc` emits by default. Use `--transpileOnly` to emit without type checking.

## tsc-rs reports an error that tsc does not. Is my code wrong?

Probably not. The type checker is incomplete and has false positives: on the last full measurement its precision was {{metrics.accuracy.compiler.precision}} on the compiler suite and {{metrics.accuracy.conformance.precision}} on the conformance suite. If `tsc` accepts the code, trust `tsc`, and please [open an issue](https://github.com/benfavre/ts-rs/issues) with a small reproduction.

## Are there prebuilt binaries?

Yes. The [0.4.2 GitHub release](https://github.com/benfavre/ts-rs/releases/tag/v0.4.2) provides Linux x64/ARM64, macOS Intel/Apple Silicon, and Windows x64 binaries with SHA256 checksums. The [npm package](/docs/npm) ships Linux x64.

## Which platforms does it build on?

The native compiler release workflow targets Linux x64 and arm64, macOS x64 and arm64, and Windows x64. On Linux x64 it links with `clang` and `lld`. The optional tsrs package has a narrower platform set; see [External project checking](/docs/external-checker).

## How fast is it?

On `typescript.js`, a 7.8 MB fixture, one same-session comparison measured tsc-rs parsing in 44.5 ms against 35.2 ms for oxc, and parsing plus semantic analysis in 96.0 ms against 97.9 ms. Those historical single-fixture numbers do not predict project checking performance. Benchmark `--fast-emit` and structured emit on your own inputs. The benchmarks are in `crates/tsc_rs_bench`; see [Testing and the harness](/docs/testing) to run them.

## Can I try it without installing anything?

Yes. The [playground](/playground) runs the compiler in your browser as WebAssembly.

## Why is the repository called ts-rs when the tool is tsc-rs?

The repository is `benfavre/ts-rs`. The binary, the crates (`tsc_rs_*`) and the npm package are all named `tsc-rs`.

## What is the license?

MIT. The repository also vendors the TypeScript test corpus and reproduces TypeScript's diagnostic messages; that material is Copyright Microsoft Corporation and licensed under Apache 2.0. See `NOTICE` in the repository.

## How do I report a bug or a difference from tsc?

[Open an issue](https://github.com/benfavre/ts-rs/issues) with the input, the `tsc` output and the tsc-rs output. A case that reduces to a few lines is the most useful kind. If you want to fix it yourself, start with [Contributing](/docs/contributing).
