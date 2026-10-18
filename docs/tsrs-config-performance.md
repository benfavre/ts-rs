# tsrs configuration-session measurement, 2026-10-18

This measures configuration startup overhead for `check-site`, not the native
parser or checker throughput on large applications.

Before: `67647c0b94` starts one tsrs configuration API process per site.
After: `fcc2ea21b1` reuses one API process per preparation worker, then kills
and reaps it before checking starts. Each site still gets its own checker
process. Sessions do not survive a CLI invocation; project configs use
absolute paths and a neutral API working directory.

| 128 generated projects, 4 workers | Before | After |
|---|---:|---:|
| Configuration API starts | 128 | 4 |
| Median wall time, 5 interleaved rounds | 2.261 s | 2.176 s |

The median decreased 3.8%. The timing ranges overlap (before 2.229-2.490 s,
after 2.154-2.394 s) on this shared server, so this is a modest observed
change, not evidence of a general speedup. Complete JSON reports matched
across every timed and instrumented invocation after removing only
`elapsed_ms`. Every seventh project deliberately has a type error; configs
also exercise project-relative import aliases.

Process counts come from separate proxy-instrumented invocations; timed runs
launch native tsrs directly. No project cache or warmed checker is reused
between CLI invocations. No RSS reduction or full-project diagnostic parity
is established by this fixture.

## Reproduce

Build each revision with `cargo build --profile release-fast --bin tsc-rs`
and retain both executables. Install the pinned checker with
`python3 scripts/install-tsrs.py`, then run from the optimized checkout:

```bash
systemd-run --user --scope -p MemoryMax=12G -p MemorySwapMax=0 \
  choom -n 1000 -- env CARGO_BUILD_JOBS=4 RAYON_NUM_THREADS=4 \
  python3 scripts/bench-site-check.py \
    --before /path/to/before/tsc-rs --after /path/to/after/tsc-rs \
    --projects 128 --jobs 4 --rounds 5 --out /tmp/tsrs-config.json
```

The builds and measurement used the same memory/concurrency limits. Raw
rounds, binary SHA-256 hashes, full commit IDs, compiler version, checker pin
and machine details are in
[the recorded result](benchmarks/tsrs-config-2026-10-18.json).

Regression tests check configuration isolation after an invalid config, JSX
and inherited paths, and synchronous API-process cleanup for both single-site
and batched checks. Native conformance metrics remain the separate historical
snapshot dated 2026-10-14.
