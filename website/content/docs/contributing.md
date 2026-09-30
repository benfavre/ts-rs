---
title: Contributing
---
# Contributing

tsc-rs is developed in the open at [github.com/benfavre/ts-rs](https://github.com/benfavre/ts-rs), under the MIT license.

## Prerequisites

- A stable Rust toolchain with Cargo.
- On Linux x86-64, `clang` and `lld`.

## Local setup

```bash
git clone https://github.com/benfavre/ts-rs.git
cd ts-rs
cargo build
cargo run -p tsc-rs -- --help
```

## Workflow

1. Make a targeted change in the relevant crate.
2. Run the focused tests first: the crate, or a single test target.
3. Run the broader validation before you open a pull request.

```bash
cargo test -p tsc_rs_parser
cargo test -p tsc_rs_harness --test integration_pipeline
cargo test
```

Faster loops through `make`:

```bash
make check
make test-fast
make quickwins
make case CASE=betterErrorForAccidentalCall
make case CASE=betterErrorForAccidentalCall FULL=1
```

`make help` lists every target. [Testing and the harness](/docs/testing) explains the baseline tools.

## Where help is most useful

The [conformance report](/conformance) shows where the gaps are. In practice:

- **Diagnostics.** The largest body of work. `typecheck-report --code <N>` lists the cases that leak a given error code.
- **Declaration emit and ES5 downlevel transforms.** The failing expanded variants.
- **Language server hover.** Contextual typing, generics and cross-file aliases.
- **Documentation.** CLI behaviour and flags, harness workflows, supported tsconfig behaviour and known compatibility gaps. When you change docs, run the command snippets to check they are still valid.

## Style

- Keep changes scoped and intentional.
- Preserve the existing crate boundaries and workspace structure.
- Prefer small, reviewable commits.
- Add comments only where they clarify behaviour that is not obvious.

## Pull request checklist

- The code builds with `cargo build`.
- The relevant tests pass. `make test-fast` is a good baseline.
- New behaviour is covered by tests where applicable.
- Docs are updated for user-facing changes.
- A change to compiler behaviour includes focused evidence and a cache-free, zero-loss manifest comparison.

## Licensing

The tsc-rs source is MIT licensed. The repository also contains the TypeScript test corpus and reproduces TypeScript's diagnostic messages; that material is Copyright Microsoft Corporation and licensed under Apache 2.0. See `NOTICE` in the repository.
