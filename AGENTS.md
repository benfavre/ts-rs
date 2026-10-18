# Agent instructions

Read [README.md](README.md) and [docs/external-checker.md](docs/external-checker.md)
before changing compiler modes. The native parser/emitter handles transpilation;
pinned external OXC tsrs handles checking when explicitly selected. The old
vendored ts-rust/goport backend is removed. Do not set `TSC_RS_CHECKER=tsrs`
globally: that also changes native pipe invocations.

For external checking, use `tsc-rs --checker tsrs --noEmit -p <project-config>`.
Plain `tsc-rs`, native `--check-pipe` and native `--lsp` use the separate
experimental native checker, which can miss property/tRPC errors. Historical
native conformance figures do not measure external tsrs; full Microsoft
parity is unverified.

Use each package's own config. Preserve existing project wrappers, environment
checks, serialization and memory/time guards. Four checker threads, Go
assignment, lazy members and a 4 GiB soft target are the measured server
settings, not universal performance guarantees. A soft target is not an RSS
cap; virtual arena reservation is not physical memory use.

Focused warm checks suit local iteration. Explicit incremental state can help
repeated local full checks; isolate it by absolute config path and checker
version, and use a separate cache for experimental binaries/settings. Preserve
cached diagnostics and nonzero status. Keep CI, campaign baselines/gates and
cold performance comparisons incremental-disabled. Do not count changing
checkers, cache reuse or reduced scope as correctness gains. Evidence:
[docs/tsrs-lazy-performance.md](docs/tsrs-lazy-performance.md).

`check-site --json` is report-only and can exit zero with diagnostics or a
failed check. Inspect every report's `errors`, `external_errors` and `skipped`.
A missing/crashed/skipped check is not clean.

Build and test with the repository Makefile (`make ci`, `cargo test --workspace`)
as appropriate to the change. Build candidates in an isolated target directory
on hosts that might consume release binaries. Preserve unrelated work and
production wrappers. Before deploying native compiler changes, compare full
pipe responses against the baseline and retain an atomic rollback. Do not
weaken assertions, suppress errors or regenerate reference baselines to make
checks pass.
