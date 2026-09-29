# Contributing to tsc-rs

## Prerequisites

- Rust stable toolchain
- Cargo

## Local setup

```bash
cargo build
cargo run -p tsc-rs -- --help
```

The standard-library integration tests require TypeScript declarations including
`lib.esnext.temporal.d.ts` (verified with TypeScript 6.0.3). If discovery selects
an older installation, such as the editor extension's TypeScript dependency,
select the compiler library explicitly:

```bash
TSC_RS_TYPESCRIPT_LIB_DIR=/path/to/typescript/lib cargo test --workspace
```

Keep this library selection the same in before/after compatibility measurements.

## Development workflow

1. Make targeted changes in the relevant crate(s).
2. Run focused tests first (crate or test target).
3. Run broader validation before opening a PR.

Common commands:

```bash
cargo test -p tsc_rs_parser
cargo test -p tsc_rs_harness --test integration_pipeline
cargo test
```

Fast local loop helpers are available via `make`:

```bash
make check
make test-fast
make quickwins
make case CASE=betterErrorForAccidentalCall
make case CASE=betterErrorForAccidentalCall FULL=1
```

`make case` (and the underlying `baseline-case` harness binary) is intended for
single-test baseline debugging with full diff output.

## Documentation changes

Documentation updates are welcome, especially for:

- CLI behavior and flags
- harness workflows
- supported `tsconfig` behavior
- known compatibility gaps

When changing docs, verify that command snippets are still valid by running them.

## Style and quality

- Keep changes scoped and intentional.
- Preserve existing crate boundaries and workspace structure.
- Prefer small, reviewable commits.
- Add comments only where they clarify non-obvious behavior.

## Pull request checklist

- [ ] Code builds (`cargo build`)
- [ ] Relevant tests pass (`make test-fast` is a good baseline)
- [ ] New behavior is covered by tests (if applicable)
- [ ] Docs updated for user-facing changes
- [ ] Baseline-factory submissions include focused evidence and a cache-free, zero-loss manifest comparison
