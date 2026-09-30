---
title: Testing and the harness
---
# Testing and the harness

tsc-rs is developed against an oracle: the TypeScript project's own test cases and the baseline files that `tsc` produced for them. The repository vendors that corpus, unmodified, under `tests/`. The harness runs tsc-rs on each case and compares the result with the baseline.

The published results are on the [conformance page](/conformance).

## The suites

| Suite | What it contains |
|---|---|
| `compiler` | Compiler behaviour tests |
| `conformance` | Language specification tests |
| `fourslash` | Editor behaviour tests, used for the language server |

## The lanes

Each case can be compared on several kinds of output:

| Lane | Baseline | A pass means |
|---|---|---|
| `js` | `.js` | Emitted JavaScript matches byte for byte |
| `errors` | `.errors.txt` | Every diagnostic matches: message, position and code |
| `symbols` | `.symbols` | The symbol dump matches |
| `declarations` | `.d.ts` | Emitted declarations match |

## Unit and integration tests

```bash
cargo test            # the whole workspace
make check            # cargo check and format check
make test-fast        # a quick regression set
make ci               # the local CI command set
```

The standard-library tests need TypeScript's declaration files, including `lib.esnext.temporal.d.ts` (verified with TypeScript 6.0.3). If discovery picks an older installation, select the library directory explicitly:

```bash
TSC_RS_TYPESCRIPT_LIB_DIR=/path/to/typescript/lib cargo test --workspace
```

## One case

Debug a single test. This takes about a second:

```bash
# Emit: first mismatch with context
cargo run -p tsc_rs_harness --bin baseline-case -- <testName> \
  --suite compiler --show-first-mismatch --context-lines 5

# Diagnostics: full expected and actual output
cargo run -p tsc_rs_harness --bin baseline-case -- <testName> \
  --suite compiler --baseline errors --show-outputs
```

`make case CASE=<testName>` is a shortcut, and `FULL=1` prints the full diff.

## A whole suite

```bash
# Bucketed failures. Cached after the first run
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --top 20

# Rerun the failures plus a sample. About 0.3 seconds
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --incremental
```

`--baseline js|errors|symbols|declarations` selects the lane.

## Type-check accuracy

`typecheck-report` counts individual diagnostics instead of whole cases, and shows where recall leaks per TypeScript error code:

```bash
cargo run --release -p tsc_rs_harness --bin typecheck-report -- --suite compiler
cargo run --release -p tsc_rs_harness --bin typecheck-report -- --code 2304 --show-cases 20
```

Run it on the full corpus. `--limit` samples alphabetically and is biased.

## Language server

```bash
cargo run -p tsc_rs_harness --bin lsp-report -- --op quickinfo
cargo run -p tsc_rs_harness --bin lsp-case -- <testName> --show-outputs
```

## Rules the harness follows

- **Caching.** `baseline-report` caches classified results in `.harness-cache/`, keyed on a hash of the compiler crate sources. Use `--no-cache` for a fresh run. Published numbers are always taken with `--no-cache`.
- **Oracle selection.** The oracle is chosen from the effective compiler options, never from whichever candidate output happens to match.
- **Skips are not passes.** A case without an oracle is skipped and excluded from the pass rate.
- **Variants.** `--expand-variants --manifest-schema 2 --save-manifest <file>` inventories every stored option variant with a stable identity.
- **Memory.** Some type-check cases trigger unbounded type expansion. If a multi-threaded `typecheck-report` run is killed for memory, rerun with `RAYON_NUM_THREADS=1`.

## Comparing two builds

A change is accepted on evidence: which cases newly pass, which passes were lost, and whether skips changed. Save a manifest before and after, then compare:

```bash
make manifest SUITE=compiler BASELINE=errors MANIFEST=/tmp/before.json
# apply the change, rebuild
make manifest SUITE=compiler BASELINE=errors MANIFEST=/tmp/after.json
make compare BASE_MANIFEST=/tmp/before.json CANDIDATE_MANIFEST=/tmp/after.json
```

Each merged wave is recorded, with its validation and timing, in [docs/verified-waves.md](https://github.com/benfavre/ts-rs/blob/main/docs/verified-waves.md).

## Benchmarks

```bash
make bench            # all benchmarks, release mode
make bench-parser     # parser only
make bench-vs-oxc     # parser and pipeline, head to head with oxc
```

The benchmarks live in `crates/tsc_rs_bench`.
