---
title: External project checking
---
# External project checking

Current source builds can use the separately installed native
[oxc-project/tsrs](https://github.com/oxc-project/tsrs) executable for checking.
JavaScript transpilation remains in the native tsc-rs parser and emitter.
There is no Node adapter or transfer of a project AST between the processes.

The existing v0.4.2 release artifacts and npm package predate this integration.
Build from the current repository to use these commands:

```bash
python3 scripts/install-tsrs.py
cargo build --release --bin tsc-rs
target/release/tsc-rs --checker tsrs -p tsconfig.json --pretty false
```

`TSRS_VERSION` pins the exact native package. The installer verifies its npm
SHA-512 integrity and installs the executable, standard-library declarations
and license notices together under `~/.local/share/tsc-rs/tsrs/<version>/`.
No npm runtime is needed for installation or checking.

The pinned packages cover Linux x64/ARM64 with GNU or musl, and macOS ARM64.
This is a narrower platform set than the native compiler. On another platform,
provide a compatible native executable with `TSRS_BINARY`; the installer does
not build tsrs from source. A system installation can use
`--prefix /usr/local/share/tsc-rs/tsrs` for users with an isolated HOME.

## Choosing the checker

| Command | Implementation |
|---|---|
| `tsc-rs --pipe`, `--transpileOnly` | Native parser and emitter |
| `tsc-rs --noEmit`, `--check-pipe`, `--lsp` | Experimental native checker or language server |
| `tsc-rs --checker tsrs -p tsconfig.json` | External native tsrs project checker |
| `tsc-rs --checker tsrs --lsp --stdio` | External tsrs language server |
| `tsc-rs check-site <directory> --json` | Bext configuration bridge and external tsrs checker |

External CLI checking always forces `noEmit`, even if a caller passes
`--noEmit false`. Native transpilation works without installing tsrs. Neither
the external checker nor its configuration API runs on the transpile pipe path.
On Unix, the checker replaces the launcher process so PID-based memory monitors
and signals apply to the real checker.

External checking returns the upstream status: 0 for a completed clean check,
nonzero for diagnostics or failure. A crash or missing executable is not a clean
result. The retired `--checker goport` backend fails explicitly.

Do not export `TSC_RS_CHECKER=tsrs` globally: it also changes native compiler
invocations such as `--pipe`. Select it per checking command. Existing campaign
baselines must be captured again when changing checkers; differences between
checkers must not be counted as type-repair improvements.

## Memory and concurrency

tsrs reserves a 32 GiB virtual address arena. That is not its physical RSS.
External checking bypasses tsc-rs's native 12 GiB address-space guard; retain
physical-memory limits, a timeout and serialization for large projects.

For example, pass `--checkers 4 --maxMemory 4G --incremental false` to tune a
project check. `--maxMemory` is a soft target, not a hard RSS ceiling. These
are CLI checker options; LSP startup uses the language server's own settings.

## Bext site reports

```bash
target/release/tsc-rs check-site /path/to/site --json --jobs 4
```

This command preserves inherited tsconfig paths and supplies Bext framework JSX
types and export aliases. It checks each site in its own process with four
checker threads and a soft 1 GiB target. Native emit is not involved.

Site checking is report-only: it exits 0 even when a site has errors or its
check failed. Consumers must inspect `errors`, `external_errors` and `skipped`.
A `skipped` report means checking did not complete. Invalid CLI usage exits 2.

## Measurement boundaries

The repository's native conformance, recall and LSP tables measure the native
implementation at the stated historical commit. They do not measure tsrs.
The two implementations follow different TypeScript snapshots. No complete
upstream diagnostic-parity claim is made for the external checker.

Configuration parsing and checking stay separate from transpilation. A
reproducible small-project benchmark is provided by
`scripts/bench-site-check.py`; compare two built revisions with `--before`,
`--after` and `--out`. It checks complete report equality and measures API
process counts separately from timed runs. It does not represent a large
monorepo or establish an across-project speedup.


The [2026-10-18 small-project measurement](https://github.com/benfavre/ts-rs/blob/main/docs/tsrs-config-performance.md)
reduced configuration API starts from 128 to 4 through worker-session reuse.
Median wall time over five interleaved rounds was 2.261 s before and 2.176 s
after (3.8% lower). Timing ranges overlap; reports matched. Each site still
launches its own checker. Sessions are reaped before checking starts and do
not persist across CLI invocations.
