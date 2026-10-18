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
