# Lazy-member evaluation, 2026-10-18

The server's checking defaults and the Bext site bridge no longer force
`--noLazyMembers`. This retains tsrs's upstream lazy-member optimization.
The pinned executable, four checker threads, Go checker assignment, `noEmit`,
soft memory targets, physical-memory guards, timeouts and serialization stay
in place. Native transpilation is independent of this change.

## Large app project

Three interleaved before/after rounds used the existing guarded full-project
entry point. Both modes used the same pinned native tsrs and a shell proxy;
the proxy removed only `--noLazyMembers` for the optimized runs. No warm
language server or incremental build state was reused.

| Measurement | Forced eager members | Lazy members enabled |
|---|---:|---:|
| Median wall time | 18.208 s | 14.482 s |
| Median maximum RSS | 7.189 GiB | 6.391 GiB |
| Existing diagnostics | 23 | 23 |

Median wall time decreased 20.5% and median maximum RSS decreased 11.1% on
this project. Full diagnostic output, including messages, matched in all six
runs. The three before times ranged from 15.787 to 18.851 s; after times ranged
from 14.277 to 15.568 s. This is a shared-server sample, not a general checker
speedup or a diagnostic-parity result against Microsoft's compiler.

`/usr/bin/time` measured the guarded command's maximum RSS. The compiler was
execed by its launcher; the small guard process does not account for these
peaks. This is not a sum of RSS across concurrent processes. Each run retained
the 4 GiB soft target, a 10 GiB RSS guard and a 12 GiB cgroup with no swap.

## Site checking and rejected alternatives

Across 32 Bext sites, three interleaved rounds measured medians of 0.866 s
before and 0.815 s after. Full reports matched after removing only
`elapsed_ms`: 595 owned-file diagnostics and 41 imported-file diagnostics
remained visible. This does not mean these projects are clean.

The earlier exploratory 128-project generated fixture was slower with lazy
members (medians 2.341 s versus 2.581 s). Small-project performance is not
uniformly better. One-checker modes also changed the real-site diagnostic
count from 139 to 137 on a four-site sample, so they were rejected. Four
checkers remain the default.

## Reproduction and evidence

Use the same pinned tsrs executable and project configuration. Compare checks
with and without `--noLazyMembers`; keep `--checkers 4 --checkerAssignment go
--noEmit --incremental false --pretty false --maxMemory 4G` in both commands.
Use an existing project safety wrapper, run serially in a physical-memory
cgroup, alternate run order, and compare full diagnostic output before
interpreting timings. Do not use the native tsc-rs checker as the reference
for this comparison. Site checks use a 1 GiB soft target instead.

[Raw timings, hashes and conditions](benchmarks/tsrs-lazy-members-2026-10-18.json)
include all measured modes, including the rejected alternatives. The site-bridge code change is `77de548146`; the server commands use the
same four-thread lazy-member setting.
The native compatibility snapshot remains dated 2026-10-14.


## Further search: incremental full-project checks

An explicit local incremental mode was the strongest additional result in
2026-10-18 experiments. It uses the pinned checker's existing incremental
implementation; the native transpiler, default checking commands and cold
campaign baselines are unchanged.

| Full-app check | Wall time | Maximum RSS |
|---|---:|---:|
| Existing cold command, one probe | 14.354 s | 6.396 GiB |
| First incremental run | 14.021 s | 7.212 GiB |
| Unchanged incremental repeat | 3.497 s | 3.816 GiB |
| Second unchanged repeat | 2.382 s | 3.821 GiB |

A separate invalidation sequence measured another unchanged repeat at 2.343 s.
Every full-app run retained all 23 existing diagnostic messages and a nonzero
status. A previous failed check's diagnostics remained visible on cached
repeats. The first incremental check still pays the full cost and uses more
memory than the current cold command in this sample; subsequent repeats are
the benefit. The cache file was approximately 22 MB.

The real-app invalidation sequence also tested adding a clean file, introducing
three tRPC/Zod/environment errors, repeating that failed check, changing an
imported type without editing its consumer, repairing the errors and deleting
the added files. The original 23 diagnostics were preserved throughout;
new errors appeared and disappeared as expected. A separate fixture changed
`strictNullChecks` through false, true and false again against the same build
state and returned clean, TS2322 and clean results. These tests cover this
pinned version and project, not all possible cache invalidation cases.

### Guarded local recipe on the configured server

From the app monorepo root, use a cache unique to the absolute project path and
checker pin. This is an explicit local reuse command; continue using the
existing cold command for independent CI and campaign comparisons.

```bash
project_key="$(realpath apps/app/tsconfig.json | sha256sum | cut -d ' ' -f 1)"
checker_pin="$(cat /usr/local/share/tsc-rs/TSRS_VERSION)"
check_cache="${XDG_CACHE_HOME:-$HOME/.cache}/tsc-rs/$checker_pin/$project_key"
mkdir -p "$check_cache"
systemd-run --user --scope -p MemoryMax=12G -p MemorySwapMax=0 \
  choom -n 1000 -- env RAYON_NUM_THREADS=4 TSGO_MAX_PEAK_RSS_GIB=10 \
  node scripts/ts-waves/safe-tsgo.mjs \
    --noEmit -p apps/app/tsconfig.json --pretty false \
    --incremental true --tsBuildInfoFile "$check_cache/app.tsbuildinfo"
```

The guard retains its environment-contract validation, singleton, memory
headroom, RSS monitoring and timeout. Use another cache directory when trying a
custom checker binary or custom checker-history settings. Deleting only this
build-info file starts a fresh local cache. Missing binaries and compiler
crashes remain failed checks, not cached clean results.

### Other candidates

Single exploratory probes kept identical diagnostic output but did not justify
new defaults. Automatic scheduling took 34.305 s with about 4.765 GiB maximum
RSS; a timing-cost cache took 35.166 s cold and 33.399 s warm. Eight checker
threads took 13.230 s versus 14.354 s at four, but used about 8.419 GiB rather
than 6.396 GiB. Raising the soft target to 6 GiB measured 13.782 s and essentially
the same RSS as the current setting; one sample does not establish a gain.
These are tradeoffs and rejected default changes, not a statistical ranking.

The instrumented profile identified large tRPC and Prisma type materialization
as the dominant work. Its extra instrumentation was excluded from candidate
comparisons. A persisted AST/front-end cache is still an upstream design, not a
shipped feature to enable. See [upstream performance options](https://github.com/oxc-project/tsrs/blob/main/docs/USAGE.md).

[All exploratory samples and invalidation results](benchmarks/tsrs-further-search-2026-10-18.json)
are recorded, including unsuccessful candidates. Temporary cache state and
canary sources were removed after the experiments; no persistent cache was
enabled globally.
