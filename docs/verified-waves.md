# Verified compiler waves

Work proceeds in small commits with focused tests, workspace checks, cache-free
baseline comparisons, and release performance measurements. Missing oracles and
ignored tests are reported separately from passes. Upstream cases and reference
baselines remain unchanged.

## 2026-09-08: missing binding-pattern argument notes

Commit `04b90c6db` adds TS6211 related information for omitted destructured
parameters in functions, methods, and constructors. Three regression tests cover
named parameters, object/array patterns, inherited constructors, supplied
arguments, and defaults.

Four complete baseline comparisons against `c4d2b3833` show one compiler-error
gain and no regressions or skip changes. Compiler diagnostic passes increase
from 4,538 to 4,539.

Alternating release-fast runs over the first 1,000 compiler error cases used one
Rayon thread and no result cache. After one warmup pair, three measured pairs
gave median wall times of 14.70 seconds before and 15.05 seconds after; CPU time
was 14.78 versus 15.07 seconds. Median peak RSS was 49,524 versus 54,424 KiB.
These measurements establish a comparison point; they do not demonstrate a
performance improvement.

## 2026-09-08: restore the workspace test gate

The next wave restores test fixtures for added type metadata and updates stale
expectations for literal types, diagnostic codes, declaration output, and enum
properties. Checks against the installed TypeScript 6.0.3 compiler distinguish
outdated assertions from implementation bugs.

Implementation repairs preserve non-generic ambient namespace function
signatures and qualified enum names, align eager import-alias evidence with
on-demand resolution, check overload defaults despite TS2371, recognize readonly
writes through literal-typed indexes, and display optional signature-help
parameters without redundant outer `undefined` unions. Generic namespace
callback inference remains a separate work item.

Validation completed with TypeScript 6.0.3 available from the workspace's
ancestor `node_modules/typescript` installation:

- `cargo test --workspace --no-fail-fast`: 3,287 passed, zero failed, 37 ignored
  across 115 test targets, including documentation tests.
- `make ci`: formatting, workspace check, project/query tests, server check,
  773 emitter regressions, and the small compiler baseline subset pass.
- `cargo clippy --workspace -- -W clippy::all`: succeeds with existing warnings.
- Four cache-free baseline comparisons against `04b90c6db`: zero regressions,
  no case/oracle changes, and no additional skips.

Three alternating release-fast performance pairs used the same 1,000 compiler
cases, one Rayon thread, and no result cache. Median wall time was 15.08 seconds
before this wave and 15.13 seconds after (+0.3%); CPU time was 15.07 versus
15.21 seconds (+0.9%). Median peak RSS fell from 56,008 to 55,444 KiB (-1.0%).
The wall-time samples overlap (14.69–15.64 seconds before, 14.94–15.47 after),
so these runs establish no clear speed change.

The four non-expanded baseline matrices retain these exact results:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,539 | 1,990 | 0 |
| Conformance | Diagnostics | 3,377 | 2,530 | 0 |

The remaining 4,520 diagnostic mismatches are future repair work. JavaScript
skips represent missing oracles and are not counted as passes. These matrices
do not include expanded emit variants or declaration baselines.

## 2026-09-08: function completion diagnostics

Add TS2355, TS2366, TS2534, and endpoint TS7030 checks for functions, arrows,
methods, and getters. Shared flow analysis accounts for reachable returns,
loops and labels, switch fallthrough, and try/catch/finally. Statement checking
records explicit never-call and exhaustive-switch evidence while lexical
bindings are available. Completion diagnostics use constant-time deduplication
and skip body analysis when the return type permits implicit completion.

Async and generator annotations are unwrapped before checking completion.
Unresolved return types avoid secondary missing-return errors. Readonly scalar
class properties retain literal types, allowing exhaustive discriminant switches.
Ten regression tests cover function forms, strictness, source spans, nested
scopes and shadowing, inferred versus explicitly typed never calls, enum and
readonly-class switches, and async/generator returns. Semantics and spans were
checked against the installed TypeScript 6.0.3 implementation and local probes.

Four cache-free comparisons against `04bef3571` yield 18 new diagnostic passes
(15 compiler and three conformance), zero lost passes, and unchanged skips and
case/oracle inventories:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,554 | 1,975 | 0 |
| Conformance | Diagnostics | 3,380 | 2,527 | 0 |

There are still 4,502 diagnostic mismatches in these matrices. Expanded emit
variants, declaration matrices, and remaining semantic flow cases need further
waves; this result does not establish full TypeScript compatibility.

`cargo test --workspace --no-fail-fast` passes: 3,297 passed, zero failed,
37 ignored across 116 targets, including documentation tests.

`make ci` passes, and `cargo clippy --workspace -- -W clippy::all` succeeds
with existing warnings.

Release-fast performance used the first 1,000 compiler diagnostic cases, one
Rayon thread, no cache, a warmup pair, and three measured alternating pairs.
No builds or test suites ran during measurement. Median wall time was
14.60 seconds before and 14.49 after (-0.8%); CPU time was 14.68 versus 14.56
seconds (-0.8%). Median peak RSS was 51,364 versus 54,232 KiB (+5.6%). Wall-time
ranges overlap (14.45–14.66 before, 14.40–14.74 after), as do memory ranges
(48,604–58,932 versus 48,208–63,808 KiB). These samples show stable elapsed
performance and variable memory use, not a demonstrated speed improvement.

## 2026-09-08: implicit return types in signatures

Add TS7013 for construct signatures and TS7020 for call signatures without
return annotations under `noImplicitAny`. Named method signatures now use the
same annotation traversals, covering inline and nested type literals as well as
interfaces. TS7010 preserves quoted and computed name spelling and underlines
the entire type-member signature. A single streaming token scan includes an
explicit separator and intervening trivia without scanning the rest of the
file. Constant-time deduplication prevents repeated annotation visits from
reporting duplicate errors.

Six regression tests cover generic and nested signatures, parameter and return
annotations, type-parameter constraints/defaults, class fields, source spans,
comments/separators, disabled checking, and explicit return types. Named
bodyless function declarations retain their name-only spans. Local TypeScript
6.0.3 probes establish expected messages and spans.

Four cache-free comparisons against `f3e1be127` yield ten new diagnostic passes
(nine compiler and one conformance), no lost passes, and unchanged case/oracle
inventories and skips:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,563 | 1,966 | 0 |
| Conformance | Diagnostics | 3,381 | 2,526 | 0 |

The remaining 4,492 diagnostic mismatches, expanded emit variants, and
declaration matrices still require further work.

`cargo test --workspace --no-fail-fast` passes: 3,303 passed, zero failed,
37 ignored across 117 targets, including documentation tests.

`make ci` passes and `cargo clippy --workspace -- -W clippy::all` succeeds
with existing warnings.

Hosted CI is currently unavailable: the GitHub Actions run for the preceding
`f3e1be127` commit ([run 34240833065](https://github.com/benfavre/ts-rs/actions/runs/34240833065))
failed before any job steps started. GitHub's check annotation reports failed
account payments or an insufficient spending limit. Local test results above
do not establish a successful hosted run; restoring Actions requires resolving
that account-level restriction.

Release-fast comparison used the first 1,000 compiler diagnostic cases, one
Rayon thread, no cache, a warmup pair, and three measured alternating pairs.
No builds or tests ran during measurement. Median wall time was 14.97 seconds
before and 14.70 after (-1.8%); CPU time was 15.04 versus 14.77 seconds (-1.8%).
Median peak RSS was 56,284 versus 54,328 KiB (-3.5%). Wall-time ranges overlap
(14.68–15.10 before, 14.55–15.33 after); these measurements show no clear
performance regression or demonstrated speed improvement.

## 2026-09-08: strictness defaults and option precedence

Explicit `noImplicitAny`, `strictNullChecks`, `strictFunctionTypes`, and
`strictPropertyInitialization` settings now override `strict` in either
direction. Omitted settings inherit TypeScript 6's strict-by-default behavior.
Property initialization and local definite-assignment checks use the effective
null-check setting, including explicit opt-ins when `strict` is false.

Strict-mode binding, label, `with`, and reserved-word checks now apply the
effective `alwaysStrict` setting. Explicit source directives and external
modules retain their automatic strictness. Declaration files are exempt from
`alwaysStrict`, and ambient declarations are exempt from generic reserved-word
checks. The common binding-name path rejects ordinary names before inspecting
file settings to avoid unnecessary work.

Six regression tests cover option combinations, variance, nullability,
initialization, directives, and ambient contexts. Existing keyword-recovery and
nested-directive fixtures explicitly select non-strict grammar; their original
assertions remain intact. The rules were verified against the installed
TypeScript 6.0.3 option resolver and compiler diagnostics.

Four cache-free comparisons against `f6f35016f` yield 16 newly passing cases
(ten compiler and six conformance), no lost passes, unchanged skips, and
unchanged case/oracle inventories:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,573 | 1,956 | 0 |
| Conformance | Diagnostics | 3,387 | 2,520 | 0 |

There are still 4,476 diagnostic mismatches in these non-expanded matrices;
expanded emit variants and declaration matrices also remain to be verified.
GitHub Actions remains blocked before job execution by the account
billing/spending-limit restriction, confirmed for `f6f35016f` in
[run 34242165388](https://github.com/benfavre/ts-rs/actions/runs/34242165388).

`cargo test --workspace --no-fail-fast` passes: 3,309 passed, zero failed,
37 ignored across 118 targets, including documentation tests.

`make ci` passes, and `cargo clippy --workspace -- -W clippy::all` succeeds
with existing warnings. Hosted CI is not included in those local results.

Release-fast timing used the first 1,000 compiler diagnostic cases, one Rayon
thread, no cache, one warmup pair, and three measured alternating pairs. An
initial run overlapped a compiler build in another workspace and was retained
separately, excluded from the reported comparison. The repeated run sampled
compiler processes every five seconds and detected none.

Median wall time was 14.67 seconds before and 14.76 after (+0.6%); CPU time was
14.76 versus 14.83 seconds (+0.5%). Median peak RSS was 49,168 versus 49,828 KiB
(+1.3%). Wall-time ranges overlap (14.58–14.70 before, 14.69–15.30 after).
These samples show a small median cost and timing variability, not a speed
improvement.

## 2026-09-08: static inheritance compatibility

Class declarations and expressions now report TS2417 when their own static
members conflict with the nearest inherited declaration. The check preserves
source declaration order, private/protected visibility, optionality, getter
read types, and callable overloads without their implementation signatures.
Static generic methods retain their constraints and defaults. Method parameter
bivariance applies separately to each overload pair; function-valued properties
use the configured function variance rule. Heritage traversal detects cycles.

The shared diagnostic explanation handles target overload failures, constrained
type parameters, and return-type paths with arguments. Signature parameter and
arity failures take precedence over return-type explanations. Thirty-nine tests
check exact diagnostic messages and spans against TypeScript 6.0.3, including
valid inheritance, class expressions, aliases, qualified bases, accessors,
private identifiers, generics, and overloaded method variance.

Four cache-free comparisons against `4ba3249ee` yield 11 newly passing cases
(six compiler and five conformance), zero lost passes, unchanged skips, and
unchanged case/oracle inventories:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,579 | 1,950 | 0 |
| Conformance | Diagnostics | 3,392 | 2,515 | 0 |

There are still 4,465 diagnostic mismatches in these non-expanded matrices;
expanded emit variants and declaration matrices remain to be verified.

`cargo test --workspace --no-fail-fast` passes: 3,348 passed, zero failed,
37 ignored across 119 targets, including documentation tests. `make ci` passes,
and `cargo clippy --workspace -- -W clippy::all` succeeds with existing warnings.

GitHub Actions still cannot start jobs because of the account billing/spending
limit, confirmed for `4ba3249ee` in
[run 34244694056](https://github.com/benfavre/ts-rs/actions/runs/34244694056).
Hosted CI is not included in these local results.

Release-fast timing used the first 1,000 compiler diagnostic cases, one Rayon
thread, no cache, one warmup pair, and three measured alternating pairs.
No builds or tests ran during measurement; sampling compiler processes every
five seconds detected none. Median wall time was 14.73 seconds before and
14.88 after (+1.0%); CPU time was 14.82 versus 14.96 seconds (+0.9%). Median
peak RSS was 49,356 versus 49,372 KiB (+0.03%). Wall-time ranges overlap
(14.56–15.00 before, 14.64–15.07 after). The added checks have a small measured
cost in this sample; this is not a demonstrated speed improvement.

## 2026-09-08: jump targets and duplicate labels

`break` and `continue` now resolve their enclosing loop, switch, or label during
the normal statement traversal. Invalid jumps report TS1104, TS1105, TS1107,
TS1115, or TS1116 as appropriate. Chained labels preserve their iteration target,
and duplicate enclosing labels report TS1114 with the original source spelling.
Escaped label names use their decoded identity for matching.

Jump validation tracks syntactic function boundaries independently of namespace
lowering and recognizes object accessors and class static blocks. Syntax errors
suppress jump grammar checks. Nested ambient blocks report their first illegal
statement before diagnosing subsequent jumps. Forty-seven regression tests
cover messages and spans, including valid control flow and JavaScript files.

Four cache-free comparisons against `2098a482d` yield 34 newly passing cases
(13 compiler and 21 conformance), zero lost passes, unchanged skips, and unchanged
case/oracle inventories:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,592 | 1,937 | 0 |
| Conformance | Diagnostics | 3,413 | 2,494 | 0 |

There are still 4,431 diagnostic mismatches in these non-expanded matrices;
expanded emit variants and declaration matrices remain to be verified.

`cargo test --workspace --no-fail-fast` passes: 3,395 passed, zero failed,
37 ignored across 120 targets, including documentation tests. `make ci` passes,
and `cargo clippy --workspace -- -W clippy::all` succeeds with existing warnings.

GitHub Actions still cannot start jobs because of the account billing/spending
limit, confirmed for `2098a482d` in
[run 34247633370](https://github.com/benfavre/ts-rs/actions/runs/34247633370).
Hosted CI is not included in these local results.

Release-fast timing used the first 1,000 compiler diagnostic cases, one Rayon
thread, no cache, one warmup pair, and three measured alternating pairs.
No builds or tests ran during measurement; sampling compiler processes every
five seconds detected none. Median wall time was 14.54 seconds before and
14.51 after (-0.2%); CPU time was 14.60 versus 14.58 seconds (-0.1%). Median
peak RSS was 55,200 versus 55,276 KiB (+0.14%). Wall-time ranges overlap
(14.53–14.65 before, 14.48–14.61 after). These samples show no clear performance
regression or demonstrated speed improvement.

## 2026-09-08: circular module aliases

A shared program index now detects TS2303 cycles through import-equals, named
and default imports, re-exports, ambient modules, and UMD namespace aliases.
Dependency-first file order and declaration identity determine which alias
receives the error. Export-assignment files have their own module scope in the
index, preserving valid namespace-backed UMD declarations.

The index resolves aliases once, caches module targets, and groups work by
source file. Programs without aliases take a fast path. Per-file checkers share
the resulting diagnostics without rebuilding the graph. Existing local alias
resolution avoids duplicating a diagnostic already reported by the index.

Thirty-six regression tests cover valid module cycles, invalid alias cycles,
star re-exports, namespace aliases, escaped identifiers, runtime extensions,
and the standalone checker entry point. Oracle-backed expectations verify
message text, offsets, and source spans against TypeScript 6.0.3.

Four cache-free comparisons against `b777a01db` yield seven newly passing
compiler cases, zero lost passes, unchanged skips, and unchanged case/oracle
inventories:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,599 | 1,930 | 0 |
| Conformance | Diagnostics | 3,413 | 2,494 | 0 |

There are still 4,424 diagnostic mismatches in these non-expanded matrices;
expanded emit variants and declaration matrices remain to be verified.

`cargo test --workspace --no-fail-fast` passes: 3,431 passed, zero failed,
37 ignored across 121 targets, including documentation tests. `make ci` passes,
and `cargo clippy --workspace -- -W clippy::all` succeeds with existing warnings.

GitHub Actions still cannot start jobs because of the account billing/spending
limit, confirmed for `b777a01db` in
[run 34249844444](https://github.com/benfavre/ts-rs/actions/runs/34249844444).
Hosted CI is not included in these local results.

Release-fast timing used the first 1,000 compiler diagnostic cases, one Rayon
thread, no cache, one warmup pair, and three measured alternating pairs.
No builds or tests ran during measurement; sampling compiler processes every
five seconds detected none. Median wall time was 14.57 seconds before and
14.71 after (+1.0%); CPU time was 14.65 versus 14.76 seconds (+0.8%). Median
peak RSS was 48,180 versus 49,992 KiB (+3.8%). Measured wall-time ranges were
14.49–14.64 before and 14.65–14.71 after. The alias index adds a small measured
runtime and memory cost in this sample; this is not a speed improvement.

## 2026-09-08: instance member compatibility

Instance inheritance and implementation checks now share the signature relation
used for static members. They check overload sets, required parameters, generic
constraints, instantiated base arguments, and method versus function-property
variance. Inherited method metadata survives intermediate classes; a property
redeclaration replaces that metadata. Alias intersections and optional interface
members participate in implementation checks.

Method signatures retain generic constraints and defaults. Parameters with default
initializers infer their types and remain required when a later parameter is
required. An unannotated method declaration without a body returns `any`.
Class parameters remain opaque during heritage comparisons, with method-local
parameters preserving their own scope. Conflicting interface bases retain the
first member while the heritage checker reports the conflict separately.

Shared assignment diagnostics now indent nested reasons consistently, explain
array element failures and incompatible generic constraints, and preserve method
variance when explaining failures. Overload errors point at each affected member
declaration. Forty-two regression tests verify messages, offsets, and spans
against TypeScript 6.0.3, including valid recursive generic inheritance.

Four cache-free comparisons against `7a123c95b` yield 13 newly passing cases
(10 compiler, three conformance), zero lost passes, unchanged skips, and unchanged
case/oracle inventories:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,609 | 1,920 | 0 |
| Conformance | Diagnostics | 3,416 | 2,491 | 0 |

There are still 4,411 diagnostic mismatches in these non-expanded matrices;
expanded emit variants and declaration matrices remain to be verified.

`cargo test --workspace --no-fail-fast` passes: 3,473 passed, zero failed,
37 ignored across 122 targets, including documentation tests.
`make ci` passes, and `cargo clippy --workspace -- -W clippy::all` succeeds with
existing warnings.

GitHub Actions could not start jobs for `7a123c95b` because of the account
billing/spending limit, confirmed in
[run 34254050063](https://github.com/benfavre/ts-rs/actions/runs/34254050063).
Hosted CI is not included in these local results.

Release-fast timing used the first 1,000 compiler diagnostic cases, one Rayon
thread, no cache, one warmup pair, and three measured alternating pairs.
No builds or tests ran during measurement; sampling compiler processes every
five seconds detected none. Median wall time was 15.40 seconds before and
15.73 after (+2.1%); CPU time was 15.31 versus 15.83 seconds (+3.4%). Median
peak RSS was 51,900 versus 55,072 KiB (+6.1%). Wall-time ranges overlap
(15.25–17.72 before, 15.63–17.95 after), with both binaries slowing in the
last pair. The broader checks add a modest measured runtime and memory cost
in this sample.

## 2026-09-08: public constructor overloads

Class constructor relations, construction expressions, and `super(...)` calls
now use public overload declarations, excluding the implementation signature.
Ambient overload declarations remain separate signatures. Inherited signatures
substitute each base's type arguments and dependent defaults while retaining
the derived class as the constructed result. Parameter initializers infer their
types and retain required positions before later required parameters.

Constructor failures select the relevant overload, report arity gaps and ranges,
and share literal widening and nested argument elaboration with function calls.
Resolved single signatures supply argument context, including inherited generic
defaults and explicit type arguments. Direct data-property generic classes explain
covariant argument mismatches without a redundant property path.

Constructor lookup walks inheritance iteratively and substitutes each heritage
edge once. Construction reuses the signatures already resolved for argument
checking, avoiding a full class-metadata clone and a second parameter-only lookup.
Forty regression tests verify diagnostic codes, messages, offsets, and spans
against TypeScript 6.0.3. A separate 4,096-class stress test checks inherited
constructor parameters and the derived return type without recursive stack use. Existing contextual-typing regressions also verify that
inherited signatures through cyclic heritage cannot supply argument context,
while a directly declared constructor can still supply its own context.

Four cache-free comparisons against `f9d5802a9` yield ten newly passing cases
(eight compiler, two conformance), zero lost passes, unchanged skips, and unchanged
case/oracle inventories:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,617 | 1,912 | 0 |
| Conformance | Diagnostics | 3,418 | 2,489 | 0 |

There are still 4,401 diagnostic mismatches in these non-expanded matrices;
expanded emit variants and declaration matrices remain to be verified.

`cargo test --workspace --no-fail-fast` passes: 3,514 passed, zero failed,
37 ignored across 123 targets, including documentation tests.

`make ci` passes, and `cargo clippy --workspace -- -W clippy::all` succeeds with
existing warnings.

GitHub Actions could not start jobs for the preceding main commit `f9d5802a9`
because of the account billing/spending limit, confirmed in
[run 34256260418](https://github.com/benfavre/ts-rs/actions/runs/34256260418).
Hosted CI is not included in these local results.

Release-fast timing compared this wave with `f9d5802a9` on the first 1,000
compiler diagnostic cases, one Rayon thread, no cache, one warmup pair, and
three uncontaminated measured pairs with both execution orders. No builds or
tests were intentionally run alongside the benchmark. A process monitor sampled
Cargo/rustc every five seconds and detected an unrelated Cargo process during
the original final pair; that pair was retained in the raw evidence but excluded
from the summary and replaced. The replacement detected no compiler processes.

Median wall time was 14.55 seconds before and 14.74 after (+1.3%); CPU time
was 14.62 versus 14.81 seconds (+1.3%). Median peak RSS was 48,232 versus
48,732 KiB (+1.0%). Wall-time ranges overlap (14.50–14.72 before,
14.49–14.87 after). This sample shows a small measured cost for the broader
constructor checks, rather than a speedup.


## 2026-09-26: constructor diagnostic spans without rescanning

Starting from `cb92e8bc0`, constructors retain the parser's keyword span in the
AST. Overload compatibility uses that span instead of allocating a token stream
for the entire constructor body. Missing-implementation diagnostics use the same
endpoint. Both diagnostics include accessibility modifiers and intervening
comments through the constructor keyword, matching TypeScript 6.0.3. A regression
tests all three accessibility modifiers, unmodified constructors, Unicode
comments, and the implementation's related diagnostic. The earlier protected
constructor assertion is corrected to the upstream span.

Four cache-free compiler/conformance comparisons, covering default JavaScript
and diagnostics, have unchanged identities, oracles, skips, and pass counts:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | JavaScript | 6,032 | 0 | 497 |
| Conformance | JavaScript | 5,388 | 0 | 519 |
| Compiler | Diagnostics | 4,617 | 1,912 | 0 |
| Conformance | Diagnostics | 3,418 | 2,489 | 0 |

There are zero lost passes and no newly passing complete baseline cases in this
batch. The focused regression verifies the corrected diagnostics; 4,401 complete
diagnostic mismatches remain. Expanded variants, declarations, and LSP baselines
were not rerun for this batch. No upstream test cases or reference oracles changed.

The final sequential `cargo test --workspace --no-fail-fast` run passes:
**3,523 passed, zero failed, 37 ignored**, including documentation tests.
`make ci` passes. Existing formatting failures in the recent split-angle-token
regressions were cleared with rustfmt, without changing their assertions.
Tests select the TypeScript 6.0.3 library with
`TSC_RS_TYPESCRIPT_LIB_DIR=/path/to/node_modules/typescript/lib`;
before/after baseline reports retain the same automatic library discovery.

The new `profile_check` example measures checking a pre-parsed, bound source file,
including checker creation and result destruction, without loading standard
libraries. Both versions were built using `--profile perf` (fat LTO, one codegen
unit, mimalloc). Six alternating before/after pairs followed a discarded warmup
pair, pinned to CPU 4. Times are medians of the six per-run medians. Counters
include process startup and the single parse/bind; timers cover repeated checks.
No Cargo, rustc, or rustdoc processes were observed during measurement.

| Synthetic constructor body | Checks per run | Before / after median | Change | User instructions |
| --- | ---: | ---: | ---: | ---: |
| Four property updates | 1,000 | 0.06615 / 0.06623 ms | +0.1% | -0.6% |
| 2,000 property updates | 50 | 2.95427 / 2.82534 ms | -4.4% | -3.2% |
| 2,000 comments | 1,000 | 0.10777 / 0.08143 ms | -24.4% | -39.8% |

Each fixture is a class with `value = 0`, an overload
`protected constructor(x: number);`, and an implementation with the same parameter.
A property update is `this.value += 1;`; a comment contains ten repetitions of
`constructor text `. Each update/comment occupies its own line. Both versions
produce zero diagnostics. These synthetic measurements isolate the removed work;
they do not establish whole-compiler performance leadership.

Run the reusable profiler on any source file with:

```sh
cargo run --profile perf -p tsc_rs_harness --example profile_check -- file.ts 1000
```

A separate broader sample used the first 1,000 compiler diagnostic cases,
cache disabled, one Rayon thread, and CPU 4. Both harness binaries used
`release-fast`, with one warmup pair and three alternating measured pairs.
Median wall time was 14.27 seconds before and 14.22 after (-0.4%); median CPU
time was 14.25 versus 14.21 seconds. Wall-time ranges were 14.25–14.39 and
14.18–14.85 seconds, respectively, so the timings do not establish a broader
speedup. Median peak RSS was 51,980 versus 49,824 KiB. All eight runs returned
the same 790 passes, 210 failures, and zero skips. No Cargo, rustc, or rustdoc
processes were observed. This alphabetical sample is not the whole corpus.

Logs, four before/after manifests, TypeScript API span evidence, benchmark samples,
process observations, and binary hashes are retained on this host under
`/tmp/ts-rs-progress-20260926/`.


## 2026-09-27: declaration notes for anonymous types and unconstrained type parameters

Starting from `b72e8c656`, two related-information notes that tsc attaches to
relation errors are now produced:

- **TS2728** ("'x' is declared here.") for a missing property whose target is an
  anonymous object type. Type literals record their member name spans when
  resolved, and an un-annotated variable initialized by an object literal
  records its property spans (for both the inferred and the widened type). The
  first declaration of a structurally identical type wins.
- **TS2208** ("This type parameter might need an `extends X` constraint.") when
  a relation line's source is a type parameter declared without a constraint,
  following `reportRelationError`. Relation text does not identify which
  declaration a name refers to, so the note is added only when the file has
  exactly one declaration of that name; same-name cases (for example
  `genericSpecializations1`, `assignmentStricterConstraints`) remain open.

Cache-free comparisons against a `b72e8c656` worktree:

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,617 | 4,630 | 0 |
| Conformance | Diagnostics | 3,418 | 3,421 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

`make ci` passes. `tests/relation_related_notes.rs` covers both notes and the
constrained-source exclusion.


## 2026-09-27: overload implementation notes and single-arity candidates

Overload sets now remember their bodied implementation (top-level functions
and class methods, keyed by the callable signatures). When every overload
rejects a call that the implementation would accept, the error carries
tsc's TS2793 note at the implementation's name. When exactly one overload has
a compatible argument count, the call reports that candidate's TS2345, as
tsc does, instead of TS2769.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,630 | 4,634 | 0 |
| Conformance | Diagnostics | 3,421 | 3,421 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

`make ci` passes; `tests/relation_related_notes.rs` covers both behaviors and
an implementation that would also reject the call. Still missing: the note
for a single overload with a generic implementation
(`overloadErrorMatchesImplementationElaboaration`) and for constructor overloads.


## 2026-09-27: strict-mode parameter names and missing `super()` calls

- **TS1100** now covers parameters named `eval` or `arguments` in strict code,
  in every signature (declarations, expressions, arrows, and type-position
  signatures), matching tsc's `bindParameter`; ambient signatures are exempt.
  Class members keep TS1210; modules (TS1215) are not yet covered.
- **TS2377**: a bodied constructor of a derived class must contain a
  `super(...)` call directly in its body; calls inside nested functions or
  arrows do not count (tsc's `findFirstSuperCall`). A heritage expression that
  is not a plain name is skipped, since it could be `null`.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,634 | 4,644 | 0 |
| Conformance | Diagnostics | 3,421 | 3,436 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

`make ci` passes; `tests/strict_and_super_rules.rs` covers both rules.


## 2026-09-27: unreachable-code ranges follow tsc

TS7027 now follows tsc's `checkSourceElementUnreachable`:

- Statements that are not potentially executable end a reported range instead
  of being absorbed into it. These are function, interface and type-alias
  declarations, and `var` statements with no initializers. `let x;` is
  executable.
- Enums and namespaces count only when they emit code: a non-const enum, any
  enum under preserveConstEnums/isolatedModules, or an instantiated namespace.
- Namespace bodies are checked, and nothing inside reported code is reported
  again (tsc's `withinUnreachableCode`).
- Function bodies inside expressions are checked: arrows, function
  expressions, object-literal methods and accessors, and class members.
- A `switch` ends the flow when some clause always runs and every clause
  exits without `break`. A clause always runs when there is a `default`, or
  when the cases of a `typeof x` switch name all eight typeof results.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,644 | 4,648 | 0 |
| Conformance | Diagnostics | 3,436 | 3,436 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

`make ci` passes; `tests/unreachable_code.rs` covers the new rules.


## 2026-09-27: switch-case comparability (TS2678)

`case` expressions are now related to the switch expression's type, as in
tsc's `checkSwitchStatement`. The rule reuses the TS2367 no-overlap logic,
which was moved into `comparison_no_overlap`. That logic is now decided per
union member, so `string` and `number | "hello"` overlap through `"hello"`.
A class constructor (`typeof C`) never overlaps a primitive or literal type.
The check skips `null`/`undefined`/`any` discriminants and intersections,
because our intersections are not reduced (`string & number` is `never` in
tsc).

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,648 | 4,652 | 0 |
| Conformance | Diagnostics | 3,436 | 3,442 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: `super` property access outside a method (TS2660)

`super.x` must have a class member, or an object-literal method or accessor,
as its nearest non-arrow container. This follows tsc's `getSuperContainer`.
Function scopes now carry a marker saying whether `super` properties are
legal there. Class members, property initializers, static blocks and
object-literal methods allow them. Function declarations and expressions do
not. Arrows carry no marker, so they defer to their container.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,652 | 4,656 | 0 |
| Conformance | Diagnostics | 3,442 | 3,442 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

Across the full compiler corpus (`typecheck-report`), this session's new
rules report no false positives for TS2660, TS2678 and TS2377. TS1100 and
TS7027 have one each, and both predate the session (TS7027 fell from three).
Overall compiler recall is 55.8% and precision 83.9%.


## 2026-09-27: emit-helper diagnostics under importHelpers (TS2354/TS2343)

The new `external_helpers.rs` ports tsc's `checkExternalEmitHelpers`. Syntax
that lowers to a tslib helper records a request, using tsc's target and option
gates, its helper bits and its error location. This covers class `extends`,
object spread/rest, legacy and ES decorators (including `__setFunctionName`
and `__propKey`), metadata, `__param`, async functions and generators,
`yield*`, `for await`, down-levelled iteration, tagged templates,
private-field get/set/in, `using`, and module interop
(`import *`, `{ default }`, `export *`).

- **Request order** follows tsc's check order: statements first, then
  deferred function and arrow expressions, then lazily checked function
  declarations.
- **TS2354**: when the file's 'tslib' cannot be found, it is reported once, at
  the first request.
- **TS2343**: otherwise each helper missing from tslib's exports is reported
  once per program, at its first request. Requested helpers are shared across
  the per-file checkers, like tsc's symbol links.

The harness registers where 'tslib' resolves for each file. An ambient
`declare module "tslib"` takes precedence, as in tsc. Classic resolution
searches ancestor directories. CommonJS-format files use `require` conditions.

A separate fix applies to object destructuring assignments
(`({ C = 1 } = {})`). These are now checked property by property: a missing
property is TS2339 unless it has a default. Previously we reported a false
TS2741 against the whole pattern.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,656 | 4,671 | 0 |
| Conformance | Diagnostics | 3,442 | 3,488 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

Full-corpus `typecheck-report`:
- **TS2343** matches all 180 expected errors (compiler and conformance) with
  no false positives.
- **TS2354** has no false positives (17 of 19 found).
- **Overall**: compiler recall 56.1% / precision 84.4%; conformance 62.2% /
  86.5%.


## 2026-09-27: namespace used as a type (TS2709)

An identifier type reference that names one of the file's namespaces is now
TS2709 when that name has no type meaning. A class, interface, alias, enum,
type parameter, import or global type of the same name counts as a type
meaning, as does a type declaration in an enclosing block or function scope.
Namespaces reached through cross-file import aliases are not handled yet
(`noCrashOnImportShadowing`, `staticInstanceResolution5`,
`moduleInTypePosition1`). A reference recovered from `extends Foo?.Bar` is
skipped, because tsc reports only TS2499 there. There are no TS2709 false
positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,671 | 4,674 | 0 |
| Conformance | Diagnostics | 3,488 | 3,488 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: unused renamings in bodyless signatures (TS2842)

`{ p: name }` in a parameter of a signature with no body is now TS2842 when
`name` is unused. This covers function and constructor types, call,
construct and method signatures, overloads, and `declare` functions. A
`typeof name` or `name is T` in the signature counts as a use. An unannotated
parameter also gets tsc's related TS2843, placed at the end of the parameter.

The check runs on the existing unused-type-parameter walk, which now takes a
`bodyless` flag per signature. Without `noUnusedParameters`, and for ambient
declarations, the walk reports only renamings. Declaration files are skipped,
as in tsc. There are no TS2842 false positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,674 | 4,679 | 0 |
| Conformance | Diagnostics | 3,488 | 3,490 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: directive options located in the test's tsconfig

When a test has a `tsconfig.json`, a deprecated option set only by a `// @`
directive is now reported at the tsconfig's `"compilerOptions"` key, as tsc's
harness does. Previously it was reported as a global error. This change is
harness-only.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,679 | 4,684 | 0 |
| Conformance | Diagnostics | 3,490 | 3,490 | 0 |


## 2026-09-27: import-equals aliases that collide with a `var` (TS2440)

The binder binds `import x = <entity>` as a function-scoped variable, so it
merged silently with `var x`. The new `import_alias_conflicts.rs` follows
tsc's `checkAliasSymbol`. Such a pair is TS2440, reported at the import
statement, when the alias target has a value meaning: a `var`, function,
class or regular enum, an instantiated namespace, or any `require`. Targets
are resolved through the file's own namespaces. Exported aliases are also
compared with exported `var`s in merged blocks of the same namespace.
Unresolvable targets are skipped. The TS2440 false positives that remain in
the full-corpus report come from existing binder rules (ES imports against
classes and namespaces), not from this pass.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,684 | 4,688 | 0 |
| Conformance | Diagnostics | 3,490 | 3,490 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: merge conflict markers (TS1185) and value-less shorthands (TS18004)

- **TS1185**: the scanner already skipped merge conflict markers as trivia
  but reported nothing. It now records each marker's start on its cold path.
  `parse` reports "Merge conflict marker encountered." at the seven marker
  characters. This code does not count as a syntax error, so semantic
  checking continues, as in tsc.
- **TS18004**: a shorthand property (`{ b }`, or `{ b = 1 }` in a
  destructuring target) whose name resolves to nothing is resolved like a
  bare identifier. tsc's TS2304/TS2552 for that name is reported as TS18004.
  Guards cover the shapes where tsc reports something else: reserved words
  and recovery shorthands that tsc parses as `name: <missing>`,
  `{ a = 1 }` outside a destructuring target (TS1312 only), and type-only
  imports (TS1361). There are no TS18004 or TS1185 false positives in either
  full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,688 | 4,695 | 0 |
| Conformance | Diagnostics | 3,490 | 3,491 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: built-in global redeclarations (TS2397) and TDZ gaps (TS2448)

- **TS2397**: in a script file, any top-level declaration named `globalThis`
  is reported at its name. So is a variable, function or namespace (not a
  type) named `undefined`.
- **TS2448**, three changes:
  - A `let`/`const` loop binding of `for…in/of` is now ready only after the
    iterated expression, so `for (let v of v)` is an error.
  - A destructured name is initialized only after its own default, so
    `let [x2 = x2] = []` is an error, while `[p, q = p]` is still fine.
  - Under `outFile`, a script's top-level (non-deferred) use of a block-scoped
    binding declared in a later script file is reported, with a related
    location in that file. The harness supplies program order. JS
    declaration files are not modeled yet (`jsFileCompilationLetDeclarationOrder2`).
- An ambient `declare const` no longer has a temporal dead zone.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,695 | 4,702 | 0 |
| Conformance | Diagnostics | 3,491 | 3,494 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: recursive interface bases (TS2310) and type-only namespaces as values (TS2708)

- **TS2310**: each interface whose `extends` graph (from `interface_info`)
  leads back to itself is reported at its name, printed with its type
  parameters (`I1<T>`). Class cycles (TS2506) are not included.
- **TS2708**: a value-position identifier that resolves at root scope to one
  of the file's namespaces with no value meaning is now TS2708, with an error
  type so member access adds nothing. Such a namespace has every block
  uninstantiated and no same-named var, function, class, enum, alias or
  import. Namespaces holding only const enums or import aliases count as
  values, as in tsc's ConstEnumOnly state. The check skips
  `extends M.I` (tsc's cannot-extend-an-interface case) and
  `export default M` (an alias of all meanings). It applies only in module
  files or single-file programs, since script namespaces merge across files.

There are no TS2310 or TS2708 false positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,702 | 4,707 | 0 |
| Conformance | Diagnostics | 3,494 | 3,494 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: alias root hidden by a local declaration (TS2437)

In a namespace block, `import X = A.b` whose target has a value meaning is now
TS2437 at `A` when `A` resolves, for value or namespace meaning, to a local
non-namespace declaration of that block (`var`, function or class). This
follows tsc's `checkImportEqualsDeclaration`. There are no TS2437 false
positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,707 | 4,711 | 0 |
| Conformance | Diagnostics | 3,494 | 3,494 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: import assignments under ES module kinds (TS1202)

With `module` set to es2015, es2020, es2022 or esnext, a non-type-only,
non-ambient `import x = require(...)` is now TS1202 on the whole statement,
as in tsc's `checkImportEqualsDeclaration`, whatever the file extension. It
is a grammar error, so it is skipped in files with syntax errors. There are
no TS1202 false positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,711 | 4,715 | 0 |
| Conformance | Diagnostics | 3,494 | 3,495 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: JSX factory namespace in scope (TS2874) and factory option validation (TS5067/TS5059)

- **TS2874**: under `jsx: react`, each opening or self-closing tag needs the
  factory namespace as a value in scope, as in tsc's `markJsxAliasReferenced`.
  The namespace comes from a file `@jsx` pragma, else a valid `jsxFactory`,
  else `reactNamespace` as written, else `React`. It is resolved directly
  rather than through the identifier path, because that path deliberately
  tolerates common browser and React globals. Otherwise TS2874 is reported at
  the tag name, or TS2552 when a spelling suggestion exists. The check is
  skipped for files that reference libraries by `/// <reference>`, contain
  `declare global`, or opt into the automatic runtime by pragma.
- **TS5067 / TS5059**: invalid `jsxFactory` and `reactNamespace` values are
  reported. The raw option values are now kept, because an invalid
  `jsxFactory` was dropped at parse time.

There are no TS2874 false positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,715 | 4,722 | 0 |
| Conformance | Diagnostics | 3,495 | 3,496 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: value used as a type (TS2749)

An identifier type reference to a name that the file declares only as a
top-level value (`var`, `let`, `const` or function) is now TS2749. It
applies when there is no same-named class, interface, alias, enum,
namespace, import or global type, and no type parameter or nested type
declaration shadows the name. Qualified references (`A.B`) are not covered
yet. There are no TS2749 false positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,722 | 4,723 | 0 |
| Conformance | Diagnostics | 3,496 | 3,496 | 0 |


## 2026-09-27: bare `return;` checked as `undefined` (TS2322)

As in tsc's `checkReturnStatement`, a bare `return;` in a non-generator,
non-constructor function whose declared return type excludes `undefined`
(and is not `void`, `any` or `unknown`) is now TS2322, "Type 'undefined' is
not assignable…", at the `return` keyword under strictNullChecks. It adds no
TS2322 false positives on bare-return lines in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,723 | 4,726 | 0 |
| Conformance | Diagnostics | 3,496 | 3,496 | 0 |


## 2026-09-27: computed property name key types (TS2464)

TS2464 now follows tsc's `checkComputedPropertyName`: the key's type must be
string-, number- or symbol-like as a whole. Every union member must qualify,
an intersection needs one qualifying member, and a type parameter is judged
by its constraint (an unconstrained one fails). Types we cannot classify are
accepted. Three exemptions match tsc: destructuring keys, `[await]` with no
operand (tsc's error type), and the malformed mapped-type shape `[K in T]: V`
on property signatures and class properties (tsc reports TS7061 there). There
are no TS2464 false positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,726 | 4,726 | 0 |
| Conformance | Diagnostics | 3,496 | 3,504 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: private names are lexically scoped (TS18013)

`x.#p` is now TS18013 when no lexically enclosing class declares `#p` but the
receiver's class declares it. For instances, a base class that declares it
also counts, since a `Derived` instance carries `Base`'s `#p`. For
constructors it does not, because static private names aren't inherited;
tsc reports TS2339 there. Non-static `#` accessors are now recorded as
private instance members. There are no TS18013 false positives in either
full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,726 | 4,726 | 0 |
| Conformance | Diagnostics | 3,504 | 3,513 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: `await` as a binding name (TS1262 / TS1359)

The new `await_bindings.rs` reports binding names spelled `await` in await
contexts, as tsc's parser does:

- **TS1262**: at the top level of a module. This covers vars (including
  destructuring), function and class names, default, named and namespace
  imports, and `import x = …`.
- **TS1359**: inside an async function. This covers parameters, body
  bindings, and an async function expression's own name.

Nested non-async functions reset the context. Only `import`, `export` and
`import x = require(...)` make a file a module; the entity form
`import x = a.b` does not. Files with syntax errors are skipped. There are no
TS1262 or TS1359 false positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,726 | 4,726 | 0 |
| Conformance | Diagnostics | 3,513 | 3,534 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |


## 2026-09-27: multiple default exports (TS2528)

TS2528 follows tsc's binder, which declares one `default` symbol. A new
default declaration conflicts when its exclusions meet the symbol's flags:

- `export default <expr>` conflicts with anything.
- A function or class conflicts with an existing `<expr>` default.
- Two classes conflict, and so do two alias defaults (`export default name`).
- Functions merge with functions and classes, and an alias default does not
  conflict with a class. Those combinations are left to other diagnostics.

Each conflicting declaration is reported once, at its name (or at `export`
for an expression). It carries tsc's related notes: "Another export default
is here." or "and here." on earlier declarations, and "The first export
default is here." on the new one. There are no TS2528 false positives in
either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,726 | 4,726 | 0 |
| Conformance | Diagnostics | 3,534 | 3,539 | 0 |


## 2026-09-27: property/accessor override kinds (TS2610 / TS2611)

As in tsc's `checkKindsOfPropertyMemberOverrides`, overriding a base accessor
with an instance property is now TS2610. Overriding a base property with an
accessor is TS2611. The nearest base class declaring the member is found
through the file's own class declarations. Ambiguous names and bases from
other files are skipped. Private members, abstract base members, methods and
auto-`accessor` fields (in either direction) are exempt, as in tsc. The error
is on the derived member's first declaration. `ClassInfo::accessor_props`
turned out to hold only `accessor` fields, so base kinds come from the AST.
There are no false positives for either code in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,726 | 4,728 | 0 |
| Conformance | Diagnostics | 3,539 | 3,550 | 0 |


## 2026-09-27: `super` in non-derived classes (TS2335) and computed keys (TS2466)

- **TS2335**: class members now mark their scope with whether their class has
  `extends`, taken from the AST (`enclosing_class_derived`), because nested
  and local classes don't resolve through `class_info` by name. `super.x`
  whose nearest container is a member of a non-derived class is TS2335. So
  is `super()` directly in such a class's constructor; elsewhere a super call
  is TS2337.
- **TS2466**: `super` in a computed property name with no enclosing `super`
  container. When an enclosing method exists, tsc resolves through it, as in
  a class expression inside a method.

There are no false positives for either code in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,728 | 4,731 | 0 |
| Conformance | Diagnostics | 3,550 | 3,559 | 0 |


## 2026-09-27: comma-operator unused left side (TS2695) follows tsc exactly

The side-effect test now mirrors tsc's `isSideEffectFree`. Identifiers,
literals, templates, functions, arrows, classes, array and object literals,
`typeof`, `!x` / `+x` / `-x` / `~x`, non-null and JSX count as side-effect
free. So do non-assigning binaries and conditionals when their parts are.
Member access, `this` and `void` do not. The earlier list treated member
access and `this` as side-effect free.

The comma is left-associative, so the left side of each comma is the whole
chain before it. It is reported, spanning the chain, only when every operand
in it is side-effect free (`void D, A, C` reports nothing).

A tagged-template tag counts as a callee, so `(0, x.fn)``` is an indirect
call. Files with parse diagnostics and commas recovered from sibling JSX
roots (tsc reports TS2657 there) are skipped. There are no TS2695 false
positives in either full corpus.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,731 | 4,732 | 0 |
| Conformance | Diagnostics | 3,559 | 3,566 | 0 |

## 2026-09-30: cross-file types, checker waves 92-100, language server rounds

Five rounds of checker and language-server work between 2026-09-29 and
2026-09-30, merged as `c0949af47` through `f21e5c7e1`. Every batch was gated on
failing-case lists (not totals): no compiler or conformance diagnostics case
regressed, JavaScript emit stayed at zero failures, and no language-server
operation lost a passing case.

Checker highlights: multi-file tests now see real imported types (cross-file
injection, on by default; `TSC_RS_HARNESS_CROSS_FILE=0` disables it), dotted
namespace bodies are checked, unannotated method returns are inferred,
iteration diagnostics (TS2488, TS2495, TS2461, TS2548, TS2549),
`strictBindCallApply` for `call`/`apply`, and an allowlist of lib interfaces
(`TemplateStringsArray`, `IArguments`, `ArrayLike`, ...) is visible to the
checker. The new `call`/`apply` rule adds no spurious TS2345 in either suite.

Language server: generic call-site instantiation in hovers, JSDoc types in
JavaScript, namespace qualification, find-all-references through
export/import aliases, go-to-definition by name no longer mixes per-file symbol
ids, contextual literal completions, and signature help inside contextually
typed parameter lists. Fourslash runs strict, like the TypeScript 6 language
service.

| Suite | Baseline | Before | After |
|---|---|---:|---:|
| Compiler | Diagnostics | 4,732 | 4,779 |
| Conformance | Diagnostics | 3,566 | 3,621 |
| Compiler | JavaScript | 6,032 | 6,032 |
| Conformance | JavaScript | 5,388 | 5,388 |

Language server (fourslash) before and after: QuickInfo 250 to 298,
completions 839 to 859, go-to-definition 172 to 191, find-all-references 305
to 329, signature help 121 to 123.

## 2026-09-30: regular-expression flags and UTF-16 diagnostic formatting

The checker now validates trailing regex flags and flags inside modifier
groups: unknown flags (TS1499), duplicates (TS1500), target availability
(TS1501), conflicting Unicode modes (TS1502), and flags that cannot be
toggled in a subpattern (TS1509). Diagnostic precedence follows TypeScript,
including repeated Unicode conflicts and duplicates across enabled/disabled
modifier lists. Escapes, character classes, lookarounds, and named captures
do not become modifier groups.

The scanner and parser consume identifier parts as flags and stop at Unicode
trivia. Misplaced shebangs retain TS18026, preventing recovered regexes from
receiving grammar errors after a syntax error. The errors-baseline formatter
now converts byte spans to UTF-16 columns and underline widths.

All 119 expected flag diagnostics across expanded compiler/conformance
variants match, with zero false positives for these codes. Compiler diagnostic
matches rise from 8,679 to 8,783 (including five TS18026 diagnostics), while
false positives remain 1,517. The two newly passing whole cases are
`regularExpressionWithNonBMPFlags` and `jsxEsprimaFbTestSuite`.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,779 | 4,780 | 0 |
| Conformance | Diagnostics | 3,621 | 3,622 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

All four comparisons use cache-free manifests. Skips are unchanged: none
for diagnostics, 497 for compiler JavaScript, and 519 for conformance
JavaScript. No upstream cases or reference baselines changed.

Validation: 1,606 scanner/parser/checker tests and 96 harness tests passed;
`make ci` passed. New coverage includes target boundaries, diagnostic
precedence, modifier groups, Unicode flag spans/trivia, misplaced shebang
recovery, `noCheck`, and an exact upstream errors-baseline comparison.

## 2026-09-30: static constructor property conflicts (TS2699 / TS2300)

Static `prototype` members now report TS2699; `name`, `length`, `caller`, and
`arguments` do so when `useDefineForClassFields` is disabled. Computed literal
names, methods, accessors, class expressions, and default exports preserve
their names and spans. Ambient classes are exempt from TS2699. Methods and
accessors named `prototype` additionally conflict with the class's implicit
prototype symbol (TS2300), including ambient declarations.

Every expected TS2699 matches in both expanded suites, with no false
positives. Total diagnostic matches increase by 172: compiler 8,783 to 8,786,
conformance 18,401 to 18,570. False-positive totals remain 1,517 and 2,785.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,780 | 4,781 | 0 |
| Conformance | Diagnostics | 3,622 | 3,623 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

Cache-free comparisons show no changed skip counts. Newly passing cases:
`staticPrototypeProperty` and `propertyNamedPrototype`. All 1,201 checker
tests, including eight new regression tests, and `make ci` passed.

## 2026-09-30: namespace default exports and export assignments

Internal namespaces now reject default exports (TS1319) and `export =`
assignments (TS1063). Default declarations underline their `default` modifier
and retain body checking. Invalid export expressions underline the whole
statement and stop before value checking, avoiding cascading missing-name
errors. Ambient external modules and global augmentations remain valid;
ambient and dotted internal namespaces follow the same restrictions.

All 39 expected TS1319/TS1063 diagnostics match across expanded variants,
with no false positives for these codes. Conformance false positives decrease
from 2,785 to 2,782. Newly passing cases are `exportDefaultClassInNamespace`,
`exportDefaultFunctionInNamespace`, `staticPropertyNameConflicts`,
`parserExportAssignment5`, and `parserExportAssignment9`.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,781 | 4,783 | 0 |
| Conformance | Diagnostics | 3,623 | 3,626 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

All four cache-free comparisons preserve skip counts. All 1,207 checker
tests, including six new regressions, and `make ci` passed.

## 2026-09-30: reserved syntax in `.mts` and `.cts` files

Angle-bracket assertions now report TS7059 in `.mts` and `.cts` files.
Single-parameter generic arrows report TS7060 unless a trailing comma or
explicit constraint disambiguates them. Defaults, `const` modifiers, and
commas inside comments do not exempt an arrow. Contextual arrows follow the
same rule; ordinary `.ts` files and `as` assertions remain valid.

All 64 expected TS7059 and 32 expected TS7060 diagnostics match across the
expanded conformance variants, with no false positives. Total conformance
matches increase from 18,605 to 18,701; false positives remain 2,782.
`nodeModulesForbidenSyntax` now passes its complete errors baseline.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,783 | 4,783 | 0 |
| Conformance | Diagnostics | 3,626 | 3,627 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

All four cache-free comparisons preserve skip counts, cases, and oracles.
All 1,214 checker tests, including seven new regressions, and `make ci` passed.

## 2026-09-30: Node import resolution by occurrence and version

ESM extension diagnostics now distinguish each import occurrence from
`require` uses of the same specifier. Dynamic imports use ESM resolution in
CommonJS files, including template literals and directory paths. Single-file
programs register resolution facts, type-only imports participate, and `.tsx`
suggestions honor JSX preservation. Relative `./` resolution retains the
virtual program's root directory. TS1471 and TS1479 apply to Node16/Node18;
Node20/NodeNext permit synchronous loading of ESM.

Conformance diagnostic matches increase from 18,701 to 19,159 (+458), while
false positives decrease from 2,782 to 2,582 (-200). Compiler precision and
recall are unchanged. Every expected TS2834 matches with no false positives;
167 of 169 expected TS2835 match with no false positives. The two remaining
TS2835 misses involve explicit Node module resolution with an incompatible
module setting (`extensionLoadingPriority`). No diagnostic code loses a match
or gains a false positive.

| Suite | Baseline | Before | After | Lost passes |
| --- | --- | ---: | ---: | ---: |
| Compiler | Diagnostics | 4,783 | 4,783 | 0 |
| Conformance | Diagnostics | 3,627 | 3,632 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 |

Newly passing cases: `moduleResolutionWithoutExtension3`,
`moduleResolutionWithoutExtension5`, `moduleResolutionWithoutExtension8`,
`nodeModulesAllowJs1`, and `nodeModules1`. All four cache-free comparisons
preserve cases, oracles, and skip counts. All 1,214 checker tests, 100 harness
unit tests (four new regressions), and `make ci` pass. The pre-change full
workspace run passed 3,599 tests with 37 ignored; ignored tests are not passes.

## 2026-09-30: expanded inventory for the 100% goal

The goal includes expanded emit variants, declarations, and LSP, beyond the
default compiler and conformance matrices. Cache-free schema-2 reports and a
complete `lsp-report --op all` run establish the following remaining work:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler, expanded | JavaScript | 6,446 | 258 | 497 |
| Conformance, expanded | JavaScript | 6,385 | 714 | 519 |
| Compiler, expanded | Declarations | 5,834 | 870 | 497 |
| Conformance, expanded | Declarations | 6,312 | 787 | 519 |
| LSP, all operations | LSP | 1,800 | 563 | 3,957 |

These are inventory measurements, not claims that this diagnostic wave
improves emit or LSP behavior. Skips remain separate from passes. The default
diagnostic matrices still have 4,021 failing cases; expanded JavaScript has
972 and declarations have 1,657. The 100% goal remains open.

## 2026-09-30: ES5 bodyless accessors and empty class elements

Concrete bodyless getters and setters now emit empty descriptor functions
under ES5. Abstract and ambient bodyless accessors remain erased. Getter/setter
pairs retain one descriptor, static descriptors stay on the constructor, and
computed keys execute once in source order. Empty class elements emit their
standalone semicolon in the lowered class body.

The expanded `abstractPropertyNegative(target=es5)` JavaScript baseline now
passes, increasing expanded compiler JavaScript passes from 6,446 to 6,447
and reducing its failures from 258 to 257. All eight cache-free comparisons
(the four default diagnostic/JavaScript matrices plus four expanded
JavaScript/declaration matrices) show zero losses and unchanged skips, cases,
and oracles. All 250 focused ES5 tests and `make ci`, including all 776 emitter
tests, pass. Three new regressions check runtime descriptor behavior,
computed-key evaluation, abstract erasure, and empty-element ordering.

The remaining supported symbol and type lanes were also measured without
the result cache. These are inventory measurements, not gains from the ES5
change:

| Suite | Baseline | Passed | Failed | Skipped |
| --- | --- | ---: | ---: | ---: |
| Compiler | Symbols | 185 | 6,249 | 95 |
| Conformance | Symbols | 293 | 5,324 | 290 |
| Compiler | Types | 108 | 6,326 | 95 |
| Conformance | Types | 63 | 5,554 | 290 |

The 100% goal includes these lanes. Missing or ambiguous oracles stay separate
from passes, and upstream cases and reference baselines remain unchanged.

## 2026-09-30: declaration positions and symbol/type annotation layout

The binder now records declaration starts including leading trivia separately
from identifier navigation spans. Export wrappers, declaration modifiers,
comments, and rest parameters retain their full starts. Symbol locations count
UTF-16 columns and all ECMAScript line terminators. Symbol and type baselines
share the source/annotation spacing used by the pinned TypeScript harness;
type annotation underlines also count UTF-16 code units.

| Suite | Baseline | Before | After | Gain | Lost passes |
| --- | --- | ---: | ---: | ---: | ---: |
| Compiler | Symbols | 185 | 1,155 | 970 | 0 |
| Conformance | Symbols | 293 | 1,207 | 914 | 0 |
| Compiler | Types | 108 | 184 | 76 | 0 |
| Conformance | Types | 63 | 128 | 65 | 0 |

The four cache-free symbol/type comparisons add 2,025 whole-case passes.
Compiler symbol/type skips remain 95 each; conformance skips remain 290 each.
The four default JavaScript/diagnostic matrices are unchanged. All eight
comparisons preserve cases and oracles and lose no passes. The full workspace
passes 3,609 tests with 37 ignored, and `make ci` passes. Three new regression
tests check independently verified declaration positions, navigation spans,
annotation spacing, Unicode columns, and line endings. The final symbol tests
and all four symbol/type reports also pass after adding assertions against
invalid or unordered annotation locations.

The first conformance symbol report exited with SIGSEGV while eight reports
ran concurrently. The same report completed under the debugger and in five
ordinary reruns with identical manifests; the final report also completed.
The initial crash remains unexplained and is not claimed fixed. Evidence is
in `/tmp/ts-rs-symbol-positions-20260930/` on the verification host.

## 2026-09-30: forward references and lexical shadowing

Identifier references now resolve after all declarations have been collected,
retaining the scope of each occurrence. References before a function, class,
variable, or type declaration can therefore navigate to that declaration, and
later local bindings shadow outer names for earlier uses. Class/interface
members do not capture bare lexical references; their type parameters remain
visible. Bindings in sibling and nested scopes stay isolated.

Compiler symbol passes increase from 1,155 to 1,176 (+21), and conformance
symbol passes increase from 1,207 to 1,210 (+3). The other six default
JavaScript, diagnostic, and type matrices are unchanged. All eight cache-free
comparisons preserve cases, oracles, and skips and lose no passes. Four new
tests cover forward value/type references, local shadowing, scope isolation,
and member names. Shadowing and member lookup were independently checked with
TypeScript 6.0.3.

The full workspace passes 3,613 tests with 37 ignored, and `make ci` passes.
An earlier workspace
run encountered a doctest dependency-linking error after source edits during
testing; the fresh run against the final source passes including doctests.
The complete LSP run retains 1,800 passes, 563 failures, and 3,957 skips; its
complete failure set matches a rerun of the prior inventory executable.
Evidence is in `/tmp/ts-rs-forward-symbols-20260930/` on the verification host.

## 2026-09-30: member ownership and precise navigation spans

Symbol baselines qualify class, interface, and enum members with their owners
and use the occurrence's recorded lexical scope to shorten namespace paths.
Quoted member names retain their source labels and bracketed display names;
quoted import specifiers retain the local binding label. Declaration lists
follow TypeScript's five-location limit and report the remaining count.

Interface and enum member navigation spans now cover the name while retaining
the full declaration start. The binder visits enum initializers, recording
references to other enum members. JSX intrinsic attribute navigation reads the
parsed property annotation instead of assuming the navigation span includes
the entire type. This also prevents a nested property with the same name from
capturing the attribute's definition.

| Suite | Baseline | Before | After | Gain | Lost passes |
| --- | --- | ---: | ---: | ---: | ---: |
| Compiler | Symbols | 1,176 | 1,709 | 533 | 0 |
| Conformance | Symbols | 1,210 | 1,534 | 324 | 0 |

The symbol comparisons add 857 whole-case passes, with compiler skips still
95 and conformance skips still 290. All eight cache-free compiler/conformance
symbol, type, diagnostic, and JavaScript comparisons preserve cases, oracles,
and skips and lose no passes; the other six matrices are unchanged.
Five new regressions cover owner names,
enum initializer references, name spans, overload lists, namespace scope
selection, import labels, and JSX attribute lookup. All 3,618 workspace tests
pass with 37 ignored, and `make ci` passes.
The upstream `tsxGoToDefinitionIntrinsics` case passes
all three markers after adapting navigation to the precise member spans.
The complete LSP comparison retains 1,800 passes, 563 failures, and 3,957
skips with an identical failure set.
Evidence is in `/tmp/ts-rs-symbol-owners-20260930/` on the verification host.

## 2026-09-30: protected member access and class object rest

The checker reports TS2445 for protected members accessed outside an eligible
class and TS2446 for instance receivers that do not derive from the accessing
subclass. It accounts for static members, nested lexical classes/functions,
explicit and contextual `this` parameters, generic constraints, public
replacements, and distinct getter/setter visibility. Assignments and updates
select setter visibility. Class ancestry walks detect cycles instead of
stopping after sixteen classes. First-declaration visibility prevents a later
duplicate parameter property from making an earlier public field protected.

Class object-rest bindings copy public data members, excluding private and
protected members, methods, and accessors. Generic bindings retain an `Omit`
source type; property lookup respects the source class's public keys. This
removes incorrect accessibility errors on the copied object while reporting
missing properties, including 32 previously missed TS2339 diagnostics.

| Suite | Diagnostic passes before | After | Gain | Lost passes |
| --- | ---: | ---: | ---: | ---: |
| Compiler | 4,783 | 4,785 | 2 | 0 |
| Conformance | 3,632 | 3,639 | 7 | 0 |

Cache-free comparisons preserve every prior pass, case, oracle, and skip
across the eight default compiler/conformance diagnostic, JavaScript, symbol,
and type matrices, plus all four expanded JavaScript/declaration matrices.
The other ten matrices are unchanged. Separate precision reports increase
matched diagnostics from 8,790 to 8,871 for compiler and from 19,159 to 19,236
for conformance: 158 additional matches, with no lost matches or new spurious
diagnostics in any code category. Compiler false positives fall from 1,517 to
1,513; conformance remains 2,582. All 35 expected TS2446 diagnostics match;
TS2445 matches 91 of 109, with remaining gaps including interface heritage,
JSDoc visibility, mixins, destructuring, and class/namespace merges.

Thirteen new regressions cover access contexts, generic messages, static
members, getter/setter writes, bracket access, deep inheritance, duplicate
visibility, and class rest. All 3,631 workspace tests pass with 37 ignored,
and `make ci` passes. The complete LSP comparison retains 1,800 passes,
563 failures, and 3,957 skips with an identical failing-case set (bucket labels
can vary when a case has multiple failure kinds). Evidence is in
`/tmp/ts-rs-protected-access-20260930/`, using the `final-` manifests and logs.

## 2026-09-30: public compatibility snapshot

Public `main` at `7643258e9` contains all ten verified September 30 compiler,
checker, and symbol batches. Fresh reports from a release build of the public
checkout match all twelve default/expanded baseline manifests and both
precision reports of the preceding protected-access verification. The full
LSP report retains the same complete failing-case set, and the five supported
operations were rerun individually.

The public workspace passes 3,374 Rust tests with zero failures and 37 ignored;
`make ci` passes. The public workspace has fewer crates than the internal
workspace used in the earlier wave logs, so its Rust count is reported
separately. `docs/compatibility-metrics.json` now records this public snapshot,
including source commit, report hashes, skip counts, and reproduction commands.
The former snapshot remains at `docs/compatibility-metrics-2026-09-07.json`.

Publish code and metrics to `benfavre/ts-rs` on `main` before synchronizing
https://ts-rs.bext.dev/. The site must consume committed published data and
must not present working-tree changes as measurements from the public commit.

## 2026-09-30: protected visibility through interface heritage

Interfaces extending classes retain the declaring class's protected visibility.
Member lookup follows interface heritage in source order, respects public
redeclarations, and terminates on cycles. Receiver checks also recognize
interfaces extending the enclosing class, including an explicit interface-typed
`this` parameter. Five regressions verify messages and spans against TypeScript
6.0.3, multiple inheritance, legal access contexts, and cyclic heritage.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,785 | 4,785 |
| Conformance | 3,639 | 3,640 |

All twelve cache-free default and expanded baseline comparisons retain every
previous pass, case, oracle, and skip count. One conformance diagnostic case
now passes. Conformance diagnostic matches increase from 19,236 to 19,239;
false positives remain 2,582. Compiler precision results are unchanged. No
code category loses matched diagnostics or gains spurious diagnostics.

All 3,636 internal workspace tests pass with 37 ignored, and `make ci` passes.
The complete LSP inventory retains 1,800 passes, 563 failures, and 3,957 skips,
with an identical failing-case set. Evidence is in
`/tmp/ts-rs-interface-access-20260930/`; the separate `namespace-trial/` records
a rejected candidate and is not the accepted result.

## 2026-10-01: public verification of interface visibility

Public `main` at `72469af31` passes all twelve cache-free baseline comparisons
against the verified interface-heritage batch. Both diagnostic-accuracy reports
match exactly. The complete LSP inventory and all five supported operations
retain their previous counts and complete failing-case sets.

The public workspace passes 3,379 Rust tests with zero failures and 37 ignored;
`make ci` passes. The structured metrics and README now report 4,785 compiler
and 3,640 conformance diagnostic passes, with 19,239 conformance diagnostic
matches and 2,582 false positives. The preceding public snapshot is retained
at `docs/compatibility-metrics-2026-09-30-7643258e9.json`.

Evidence is in `/tmp/ts-rs-public-interface-20260930/`. Publish this code and
snapshot to public `main` before synchronizing the live website.

## 2026-10-01: preserve merged class constructors and check enum relations

Class/namespace merges retain the constructor identity alongside namespace
exports. Protected static access now uses that identity, constructor aliases
remain constructable, and structural assignments see both the class statics
and the namespace members. `typeof` relations inspect merged intersections
without discarding the original diagnostic display name. Enum relations check
union and intersection members while preserving coverage of a whole enum by
multiple union alternatives.

Seven oracle-backed regressions cover protected access and aliases, ambient
merge order, construct signatures, combined static properties, property/index
constraints, and enum union coverage. TypeScript 6.0.3 verifies their semantic
diagnostics, messages, and spans under matching options.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,785 | 4,785 |
| Conformance | 3,640 | 3,641 |

All twelve cache-free default and expanded baseline comparisons retain every
previous pass, case, oracle, and skip count. One conformance diagnostic case
now passes. Matched diagnostics increase from 8,871 to 8,874 for compiler and
from 19,239 to 19,246 for conformance. Compiler false positives decrease from
1,513 to 1,509; conformance remains 2,582. No diagnostic code loses matches or
gains spurious diagnostics. All 3,643 internal workspace tests pass with 37
ignored, and `make ci` passes.

The complete LSP inventory gains `cloduleTypeOf1`: 1,801 passes, 562 failures,
and 3,957 skips, with no new failing cases. Evidence is in
`/tmp/ts-rs-clodule-20261001/`; the final reports are separate from the
`before-enum/` intermediate candidate.

## 2026-10-01: public verification of merged constructors and enum relations

Public `main` at `42a1feda8c` passes all twelve cache-free comparisons against
the verified merged-constructor and enum batch. Both diagnostic-accuracy
reports match exactly. The full LSP inventory and each supported operation
have no new failing cases; QuickInfo gains `cloduleTypeOf1`, reaching 299
passes and 230 failures. Overall LSP totals are 1,801 passed, 562 failed, and
3,957 skipped.

All 3,386 public Rust tests pass with 37 ignored, and `make ci` passes.
The structured snapshot and README report 4,785 compiler and 3,641 conformance
diagnostic passes, 28,120 matched diagnostics overall, and four fewer spurious
compiler diagnostics. The preceding public snapshot is retained at
`docs/compatibility-metrics-2026-10-01-72469af31.json`.

Evidence is in `/tmp/ts-rs-public-clodule-20261001/`. Publish this code and
snapshot to public `main` before updating the live website.

## 2026-10-01: visibility checks for destructuring bindings

Object destructuring now checks private and protected member visibility at the
property being read, including renamed bindings, defaults, computed keys,
nested patterns, function parameters, and for-of declarations. Checks run in
the actual lexical access context, preserving legal reads inside classes and
subclasses. Generic constraints retain their class visibility, while object
rest continues to copy only public data properties.

Seven regressions verify diagnostic messages and byte spans against TypeScript
6.0.3.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,785 | 4,786 |
| Conformance | 3,641 | 3,642 |

Accuracy reports match 14 additional diagnostics
(four compiler, ten conformance), with no lost matches or new false positives
in any diagnostic code category.

All twelve cache-free default and expanded comparisons preserve every prior
pass, case, oracle, and skip count; the other ten matrices are unchanged.
All 3,650 internal workspace tests pass with 37 ignored, and `make ci` passes.
The complete LSP inventory remains at 1,801 passes, 562 failures, and 3,957
skips, with the identical complete failing-case set. Evidence is in
`/tmp/ts-rs-destructuring-access-20261001/` on the verification host.

## 2026-10-01: public verification of destructuring visibility

Public `main` at `13486afcb7` matches all twelve cache-free baseline manifests
and both diagnostic-accuracy reports from the verified destructuring batch.
The complete LSP inventory and each supported operation retain their prior
counts and complete failing-case sets: 1,801 passes, 562 failures, and 3,957
skips overall.

All 3,393 public Rust tests pass with 37 ignored, and `make ci` passes.
The structured snapshot and README report 4,786 compiler and 3,642 conformance
diagnostic passes, with 14 additional matching diagnostics and unchanged
false-positive counts. The preceding public snapshot is retained at
`docs/compatibility-metrics-2026-10-01-42a1feda8.json`.

Evidence is in `/tmp/ts-rs-public-destructuring-access-20261001/`. Publish this
code and snapshot to public `main` before updating the live website.

## 2026-10-02: exact type-expression baseline spans

Type baselines now retain complete expression spans and syntax order when
expressions share a start position. Declaration names preserve their metadata,
and malformed for-loop bindings retain parser recovery without fabricating
expression text. Hover consumers retain their existing position map.

| Lane | Compiler passes before | After | Conformance passes before | After |
|---|---:|---:|---:|---:|
| Types | 184 | 601 | 128 | 531 |

The twelve cache-free baseline comparisons gain 820 whole type-baseline
passes, with zero lost passes and unchanged cases, oracles, and skip counts.
All diagnostic, JavaScript, expanded declaration, symbol, accuracy, and LSP
results remain unchanged. No upstream tests or reference baselines changed.

The implementation is internal `1c32e8bf8` and public `8470e4a298`. Public
reports reproduce every internal matrix. All 3,662 internal and 3,405 public
Rust tests pass, with 37 ignored in each workspace; both `make ci` checks pass.
Measurements were collected on October 1; this entry publishes those results
on October 2. Evidence is in `/tmp/ts-rs-type-baselines-20261001/` and
`/tmp/ts-rs-public-type-baselines-20261001/`.

## 2026-10-02: declaration files in symbol and type baselines

Symbol and type reports now include declaration-file source inputs, matching
the upstream oracle. They retain declarations alongside implementation files
and continue to omit JSON inputs. Three regressions cover declaration symbol
positions, type annotations, module extensions, and mixed input files.

| Lane | Compiler passes before | After | Conformance passes before | After |
|---|---:|---:|---:|---:|
| Symbols | 1,709 | 1,722 | 1,534 | 1,568 |
| Types | 601 | 603 | 531 | 557 |

All twelve cache-free comparisons retain every previous pass and all case,
oracle, and skip identities. The wave gains 75 complete baseline passes:
47 symbol cases and 28 type cases. Diagnostics, diagnostic accuracy,
JavaScript, and expanded declarations are unchanged. Upstream test cases and
reference baselines remain unchanged.

The implementation is internal `ca5c94c01` and public `59cbf453d7`. All public
baseline matrices and diagnostic accuracy reports reproduce the internal
results. All 3,665 internal and 3,408 public Rust tests pass with 37 ignored
in each workspace, and both `make ci` runs pass. LSP counts and complete
failing-case sets are unchanged across the inventory and supported operations.
Evidence is in `/tmp/ts-rs-declaration-baselines-20261002/` and
`/tmp/ts-rs-public-declaration-baselines-20261002/`.

The preceding 820-pass type-expression wave was published at `57f6b9d9cc`.
All 26 live routes, the search index, and desktop/mobile rendering verified
that build and its updated metrics; evidence is in
`/tmp/ts-rs-site-types-20261002/`.

## 2026-10-09: checker correctness and speed sync

Public `main` takes 54 internal commits of checker work: classes, interfaces
and type aliases bind per file, so same-named declarations in different
modules no longer shadow each other; tRPC procedures and Prisma client calls
resolve end to end; `@ts-ignore` and `@ts-expect-error` suppress diagnostics;
expression nesting has its own depth budget. Checker speed work replaces
deep-copied project-wide tables with frozen shared bases plus per-file
overlays and copy-on-write object types, and one-shot runs cache filesystem
probes.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,786 | 4,793 |
| Conformance | 3,642 | 3,646 |

| Lane | Compiler before | After | Conformance before | After |
|---|---:|---:|---:|---:|
| Types | 603 | 604 | 557 | 558 |

Diagnostic accuracy matches 6 more compiler and 8 more conformance
diagnostics, with 22 and 48 fewer false positives. QuickInfo gains two
passes (301), so the supported LSP operations reach 1,803 passes and 560
failures. JavaScript emit stays at 100% in both suites; symbols and the
expanded JavaScript and declaration lanes are unchanged.

The measured commit is public `cb31e5f25c`. All twelve baseline matrices
were run cache-free. All 3,429 public Rust tests pass with 37 ignored, and
`make ci` passes. Harness project tests now pin `"types": []`, so typings
installed above the temporary directory cannot enter their programs. The
preceding snapshot is retained at
`docs/compatibility-metrics-2026-10-02-59cbf453d.json`.

## 2026-10-10: tsc source maps, project baselines and ES5 classes

Source maps now carry tsc's mappings for every printed node rather than
one per statement. Our emitter copies most text from the source, so the
mappings are recovered after emission: source and output tokens are
aligned, each node maps the output range of its surviving tokens, and the
CommonJS/AMD module transform is covered by rules for the statements it
synthesizes. The generator coalesces mappings exactly as tsc's
SourceMapGenerator does. The harness now runs TypeScript's project
baselines (outFile bundles with their source maps, tsc's output naming,
root and reference checks); they pass 293 of 316 internally and are not
yet part of the published lanes.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,793 | 4,796 |
| Conformance | 3,646 | 3,646 |

| Lane | Compiler before | After | Conformance before | After |
|---|---:|---:|---:|---:|
| Expanded JavaScript | 6,447 | 6,460 | 6,385 | 6,387 |
| Expanded declarations | 5,834 | 5,843 | 6,312 | 6,313 |

New diagnostics: TS1147 (import aliases of modules inside namespaces, with
tsc's lazy resolution of the alias), TS1540 (namespaces declared with the
`module` keyword), TS5051 (`sourceRoot` without a source map) and TS5053
(`declarationDir` with `outFile`). Declaration files keep the quote style
of module specifiers, and AMD/System `outFile` declaration bundles wrap
each module in `declare module "<id>"`. ES5 output lowers classes with
parameter properties, constructor overloads and multi-line commented
accessor bodies. Diagnostic accuracy matches 39 more compiler
diagnostics with 14 fewer false positives.

The measured commit is public `f64b8ea63d`. All twelve baseline matrices
were run cache-free; JavaScript emit stays at 100% in both suites; symbol,
type and LSP results are unchanged. All 3,429 public Rust tests pass with
37 ignored, and `make ci` passes. The preceding snapshot is retained at
`docs/compatibility-metrics-2026-10-09-cb31e5f25.json`.

## 2026-10-10: ES5 lowering and diagnostic ordering

ES5 output now flattens destructuring declarations and parameters as tsc's
flattenDestructuringBinding does (temps, direct reads for single-element
patterns, `=== void 0` defaults), lowers classes with legacy decorators
(applied inside the class IIFE), ES module default-exported classes and
members reading `super` (`_super.prototype.m.call(this)`), and lowers
arrows that read `this` through a `var _this = this` capture. Async bodies
with `let`/`const` declarations take the ES5 generator transform.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,796 | 4,805 |
| Conformance | 3,646 | 3,650 |

| Lane | Compiler before | After | Conformance before | After |
|---|---:|---:|---:|---:|
| Expanded JavaScript | 6,460 | 6,494 | 6,387 | 6,444 |

Errors baselines now order diagnostics as tsc's compareDiagnostics does:
a shorter span first at one position, and every file-less diagnostic in one
global list sorted by code. TS1011 has tsc's empty span, and a truthiness
guard narrows a static member read through its class name.

The measured commit is public `76ae18ea29`; checker lanes were measured
before a depth limit on ES5-only span collection that does not affect
them, and the emitter lanes, Rust tests and `make ci` were run on the
commit itself. JavaScript emit stays at 100% in both suites. All 3,429
public Rust tests pass with 37 ignored, and `make ci` passes. The
preceding snapshot is retained at
`docs/compatibility-metrics-2026-10-10-f64b8ea63.json`.

## 2026-10-10: scanner errors, loop grammar and assignment checks

The scanner now reports invalid characters (TS1127), unterminated strings
(TS1002) and out-of-range or unterminated extended Unicode escapes
(TS1198/TS1199), withdrawing them where a regex or JSX rescan absorbs the
text; a file containing a U+FFFD replacement character is reported as
binary (TS1490) and the rest is trivia, as tsc's scanner does. Loops get
tsc's declaration grammar (TS1188/TS1091, TS1190/TS1189, TS2483/TS2404),
and `for (target of xs)` checks each element against the target's
declared type. Import attributes are checked against `--module` (TS2823)
and type-only declarations (TS2857); truncated attribute clauses report
`'{'`/`'}'` expected. An object relates to a discriminated union when every
combination of its discriminant values does; disjoint primitive
intersections are `never`; write-only private properties are unused;
missing `/// <reference path>` targets report TS6053.

The errors lane now matches `target=es6` variant oracles to `es2015` runs.
Cases that had passed only against a missing oracle were fixed, not lost.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,805 | 4,829 |
| Conformance | 3,650 | 3,730 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 85.85% | 86.05% | 59.82% | 60.15% |
| Conformance | 88.38% | 88.63% | 67.04% | 67.98% |

JavaScript emit stays at 100% in both suites, and the expanded JavaScript
and declaration lanes are unchanged. All 3,429 public Rust tests pass with
37 ignored, and `make ci` passes. QuickInfo is carried from the preceding
state because one case needs more than this run's 24 GB memory cap; the
full LSP inventory is carried for the same reason. The preceding snapshot
is retained at `docs/compatibility-metrics-2026-10-10-76ae18ea2.json`.

## 2026-10-10: JSDoc types in checked JavaScript

Checked JavaScript now reads its JSDoc types. `@type`, `@param` (either
order, destructured parameters by position, dotted members), `@returns`
(type predicates included), `@typedef` (object typedefs from `@property`),
`@callback`, `@template` (constraints, defaults, `const`), `@satisfies`
and parenthesized casts become annotations on a checker-only copy of the
file, parsed at their own offsets so diagnostics point into the comment.
JavaScript therefore gets implicit-any parameter errors and unresolved
JSDoc type names, as tsc reports them; JavaScript interfaces report only
TS8006, and exporting a type-only name is TS18043.

`satisfies` now elaborates nested literals and checks excess properties on
computed shapes (`Record`, `Partial`) named as written. Parameter
initializers are checked against their annotation. A namespace import's
missing member is TS2339 on `typeof import("…")`. A negated `typeof` guard
can narrow to `never`. The quickinfo suite's memory use fell from more than
24 GB to 1 GB (alias bodies were display-expanded without a bound).

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,829 | 4,840 |
| Conformance | 3,730 | 3,786 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.05% | 86.07% | 60.15% | 60.54% |
| Conformance | 88.63% | 89.11% | 67.98% | 68.70% |

QuickInfo passes rise from 301 to 303, and the full LSP inventory is
measured again (1,805 passed). JavaScript emit stays at 100% in both
suites. All 3,429 public Rust tests pass with 37 ignored, and `make ci`
passes. The preceding snapshot is retained at
`docs/compatibility-metrics-2026-10-10-d6428aa94.json`.

## 2026-10-10: type-only aliases and missing-semicolon parse errors

A name that reaches its declaration through `import type`, `export type`
or `export type *` is now an error where it is used as a value: TS1361 or
TS1362, with a note at the first type-only declaration on the way from
the use. The chain is followed across files and through star re-exports
(a value star wins over a type-only one), and a type-only alias of a
declaration without a value meaning does not shadow a value of the same
name. `export =`, `export default`, ambient declarations and computed names
of interfaces, type literals and abstract or `declare` members may still
name such an alias. A `type` modifier inside `import type { }` or
`export type { }` is TS2206 or TS2207.

The parser reports `';' expected` after an expression statement that
continues on the same line. A bare word there is reported the way tsc
reports it instead: TS1435 with a keyword suggestion, TS1434, or TS1440
for `const`/`let`/`var`. A type reference's type arguments must start on
the same line, so a generic call signature on the next line of a type
literal no longer misparses. A decorator after a property name is TS1436.
In the checker, a nested class's private name that shadows an outer
class's is TS18014; static blocks report TS2729 for statics declared
later; with `[[Define]]` class fields, a field initializer that reads a
parameter property is TS2729.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,840 | 4,858 |
| Conformance | 3,786 | 3,853 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.07% | 86.62% | 60.54% | 61.03% |
| Conformance | 89.11% | 89.45% | 68.70% | 69.34% |

JavaScript emit stays at 100% in both suites, and the LSP lanes are
unchanged. All 3,429 public Rust tests pass with 37 ignored, and
`make ci` passes. The preceding snapshot is retained at
`docs/compatibility-metrics-2026-10-10-3f776ee67.json`.

## 2026-10-10: parser recovery parity and real-world false positives

The parser follows tsc's recovery more closely. An invalid character
absorbs a parse error at its own position and spans the character. A
missing parameter name before `)` or `,` consumes nothing, and a missing
`(` before `}` or `;` leaves an empty parameter list. An object literal
member followed by `;` on a new line abandons the literal. `export =
undefined` and re-exports of type-only imports are no longer reported as
unknown names.

A smoke test over 41,000 real-world `.ts`/`.tsx` files found syntax errors
that tsc does not report, and all of them are fixed. An apostrophe in JSX
text no longer leaves an unterminated-string or invalid-character error.
A regular expression may start with `>` or contain `</`.
`import("m").default` is a valid import type. A typed arrow parameter
inside a conditional may name a qualified type. The same files now
transpile with no diagnostics.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,858 | 4,865 |
| Conformance | 3,853 | 3,865 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.62% | 86.73% | 61.03% | 61.03% |
| Conformance | 89.45% | 89.51% | 69.34% | 69.33% |

JavaScript emit stays at 100% in both suites, and the LSP lanes are
unchanged. All 3,430 public Rust tests pass with 37 ignored, and
`make ci` passes. The preceding snapshot is retained at
`docs/compatibility-metrics-2026-10-10-06a23c635.json`.

## 2026-10-12: namespace imports, control flow and decorator checking

The checker now validates more namespace and import aliases, ambient module
augmentations, inherited private members, and property spelling suggestions.
Control-flow checks cover enum members, constant comparisons and unknown
receivers. Decorator checks resolve callable signatures, validate argument
counts and overloads, and report missing global types and parameter locations.
Parser recovery and JavaScript emission fixes are included in the same
public snapshot. Upstream test cases and reference baselines are unchanged.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,865 | 4,936 |
| Conformance | 3,865 | 3,999 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.73% | 86.99% | 61.03% | 63.44% |
| Conformance | 89.51% | 90.06% | 69.33% | 71.43% |

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all LSP operations. JavaScript
emit still matches every available default baseline in both suites. Skips are
excluded from pass rates. The LSP totals remain unchanged.

All 3,759 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `17d96efab`. The preceding
snapshot, measured on 2026-10-10, is retained at
`docs/compatibility-metrics-2026-10-12-86b97a639.json`.

## 2026-10-12: decorator arguments, return types and constructor inference

Legacy decorators now validate argument types and return types, preserving
private static member identity when comparing class constructors. Generic
inference uses class prototypes and parameterless callback return candidates
while preserving union arguments and contextual callback inference.
Bare construct signatures retain an untyped prototype. Upstream test cases
and reference baselines are unchanged.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,936 | 4,939 |
| Conformance | 3,999 | 4,004 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.99% | 87.03% | 63.44% | 63.58% |
| Conformance | 90.06% | 90.06% | 71.43% | 71.48% |

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all LSP operations. JavaScript
emit still matches every available default baseline in both suites. Skips are
excluded from pass rates. LSP totals are refreshed in the same snapshot.

All 3,786 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `1eb8300a4`. The preceding
snapshot, measured on 2026-10-12, is retained at
`docs/compatibility-metrics-2026-10-12-17d96efab.json`.

## 2026-10-12: array inference, destructuring and implicit-any diagnostics

Constructor inference now respects variance. For-of bindings and assignments
infer and narrow from iterable element types, and index-access checks validate
key types. Arrays preserve literal and tuple element shapes, including empty
arrays and omitted elements. Implicit-any diagnostics follow inferred nullable
values through properties, function returns and nested destructuring.
Upstream test cases and reference baselines are unchanged.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,939 | 4,951 |
| Conformance | 4,004 | 4,022 |

| Type baseline | Passes before | After |
|---|---:|---:|
| Compiler | 619 | 638 |
| Conformance | 560 | 613 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 87.03% | 86.91% | 63.58% | 64.12% |
| Conformance | 90.06% | 89.98% | 71.48% | 71.77% |

Whole-case baselines gain 30 diagnostic matches and 72 type matches, with
no passed-to-failed cases or new skips across all twelve measured lanes.
Diagnostic recall improves; precision decreases slightly as false positives
rise from 1,413 to 1,440 in compiler and 2,266 to 2,297 in conformance.
The full LSP inventory gains two passes (QuickInfo and completions).

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all LSP operations. JavaScript
emit still matches every available default baseline in both suites. Skips are
excluded from pass rates. LSP totals are refreshed in the same snapshot.

All 4,247 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `7750e4c1d`. The preceding
snapshot, measured on 2026-10-12, is retained at
`docs/compatibility-metrics-2026-10-12-1eb8300a4.json`.

## 2026-10-12: inherited indices and element-access diagnostics

Element access follows inherited array and index signatures, including generic
base types and unchecked reads. Missing indices produce the appropriate static
member and get/set suggestions. Fresh object reads preserve undefined results
and widen fresh property literals; enum and unique-symbol keys retain their
correct diagnostics. Duplicate index signatures use the first declaration
for reads and report TS2374; explicit intersections combine indexed values.
Object spreads report definite overwrites, while logical
AND expressions retain the appropriate falsy values. Boolean flow and function
parameter scopes preserve the information needed for return inference.
Upstream test cases and reference baselines are unchanged.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,951 | 4,960 |
| Conformance | 4,022 | 4,030 |

| Type baseline | Passes before | After |
|---|---:|---:|
| Compiler | 638 | 640 |
| Conformance | 613 | 613 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.91% | 86.92% | 64.12% | 64.44% |
| Conformance | 89.98% | 89.99% | 71.77% | 72.49% |

False-positive diagnostics change from 1,440 to 1,446 in compiler
and from 2,297 to 2,316 in conformance. The accuracy report includes all option variants.

QuickInfo passes increase from 304 to 306. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 17 diagnostic matches and 2 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,382 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `e7658c63d`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-7750e4c1d.json`.

## 2026-10-12: tuple index types and diagnostics

Tuple reads resolve fixed, optional and rest elements, numeric key unions,
empty tuples and length properties. Indexed-access aliases keep computed
results, and invalid indices report the corresponding tuple diagnostics.
Readonly declarations retain their write checks and diagnostic spelling.
Merged type/value exports select the value independently of declaration order.
The batch adds 95 tests checked against TypeScript 6.0.3, including diagnostic
messages and source spans. Upstream cases and reference baselines are unchanged.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,960 | 4,961 |
| Conformance | 4,030 | 4,035 |

| Type baseline | Passes before | After |
|---|---:|---:|
| Compiler | 640 | 640 |
| Conformance | 613 | 614 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.92% | 86.91% | 64.44% | 64.45% |
| Conformance | 89.99% | 90.02% | 72.49% | 72.59% |

False-positive diagnostics change from 1,446 to 1,448 in compiler
and from 2,316 to 2,313 in conformance. The accuracy report includes all option variants.

QuickInfo retains 306 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain six diagnostic matches and one type match.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,477 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `eb41dbc28`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-e7658c63d.json`.

The first optimized conformance-type report exited with SIGSEGV. The same
binary then completed under the debugger, in three standalone reruns and
eight reruns under parallel report load, all with identical case results.
The initial crash did not reproduce; its cause remains unresolved.

## 2026-10-12: tuple destructuring types and diagnostics

Tuple destructuring projects fixed, optional and rest elements through aliases
and generic constraints. Nested patterns retain tuple labels in bounds errors.
Defaults respect explicit annotations, and signature inference includes
destructured parameters and local bindings. Array assignment checks evaluate
the source before writing individual targets, preserve assertion reads, and
record empty and spread target expressions for type inspection.
The batch adds 62 regression tests checked against TypeScript 6.0.3, including
diagnostic messages, source spans and expression types. Upstream cases and
reference baselines are unchanged.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,961 | 4,968 |
| Conformance | 4,035 | 4,040 |

| Type baseline | Passes before | After |
|---|---:|---:|
| Compiler | 640 | 641 |
| Conformance | 614 | 614 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 86.91% | 88.21% | 64.45% | 64.58% |
| Conformance | 90.02% | 90.07% | 72.59% | 72.71% |

False-positive diagnostics change from 1,448 to 1,287 in compiler
and from 2,313 to 2,303 in conformance. The accuracy report includes all option variants.

QuickInfo retains 306 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 12 diagnostic matches and 1 type match.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,539 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `c0d733123`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-eb41dbc28.json`.

## 2026-10-12: readonly tuple types and write diagnostics

Readonly tuple annotations retain their type through aliases, generic constraints,
properties and function returns. Writes to fixed slots report readonly-property
errors; dynamic and rest indices report readonly-index-signature errors while
preserving bounds and assignment diagnostics. Delete, update and destructuring
assignments use the same rules. Valid reads, spreads, optional tuple lengths and
const-tuple aliases retain their element types.
The batch adds 62 regression tests checked against TypeScript 6.0.3, including
diagnostic messages, source spans and expression types. Upstream cases and
reference baselines are unchanged.

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,968 | 4,969 |
| Conformance | 4,040 | 4,041 |

| Type baseline | Passes before | After |
|---|---:|---:|
| Compiler | 641 | 641 |
| Conformance | 614 | 614 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.21% | 88.23% | 64.58% | 64.60% |
| Conformance | 90.07% | 90.08% | 72.71% | 72.73% |

False-positive diagnostics change from 1,287 to 1,285 in compiler
and from 2,303 to 2,302 in conformance. The accuracy report includes all option variants.

QuickInfo retains 306 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 2 diagnostic matches and 0 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,601 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `46722ed85`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-c0d733123.json`.

## 2026-10-12: exact type baselines and readonly container inference

Exact type baselines now retain assignment targets, destructured binding names,
class member names and setter types. Destructuring containers remain available
for hover without being emitted as expression nodes. Defaulted properties of
any-typed objects retain their any type. Const tuples honor mutable constraints
and nested, union and return contexts.
Array and tuple mutability survives aliases, generic inference, members,
index access and assignments. Readonly sources reject mutable array and tuple
destinations with TypeScript-compatible error details. Const assertions retain
nested tuple literals without freezing referenced mutable arrays. Readonly
property modifiers remain visible in structural type displays. Missing method
calls also diagnose callbacks that have no contextual parameter signature.
The batch adds 101 readonly-container cases checked against TypeScript 6.0.3,
including diagnostic messages, source spans and expression types, plus six
exact-span and upstream-baseline regression tests. Upstream cases and
reference baselines are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 641 | 721 |
| Conformance | 614 | 737 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,969 | 4,971 |
| Conformance | 4,041 | 4,044 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.23% | 88.26% | 64.60% | 64.70% |
| Conformance | 90.08% | 90.12% | 72.73% | 72.76% |

False-positive diagnostics change from 1,285 to 1,283 in compiler
and from 2,302 to 2,293 in conformance. The accuracy report includes all option variants.

QuickInfo passes change from 306 to 307. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 5 diagnostic matches and 203 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,708 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `5999d5373`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-46722ed85.json`.

## 2026-10-12: contextual literal subsystem transplant

Contextual literal classification and widening now use algorithms adapted
from pingdotgg/ts-rust commit 22ae6fa87, preserving the native checker API.
The transplant includes recursive literal-kind classification, contextual
widening and unique-symbol widening. Native adapters resolve aliases, enum
member kinds and generic constraint chains. Copyright notices, upstream
commit references and MIT/Apache-2.0 license texts accompany the code.
Alias declaration names retain their semantic types without exposing the
expanded-body hover marker. Class property names use the computed member
type, including readonly literals and symbol identities.
Array initialization retains the resulting literal types instead of widening
them a second time. Generic array diagnostics identify the failing element
and explain its constraint; unique-symbol arrays display their typeof type.
Thirty-one cases compare exact expression types, diagnostic text and source
spans against TypeScript 6.0.3; two further regressions cover declaration
spans and hover preservation. The unchanged upstream baselines determine
whole-case compatibility; the donor compiler is not used as an oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 721 | 740 |
| Conformance | 737 | 742 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,971 | 4,971 |
| Conformance | 4,044 | 4,044 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.26% | 88.26% | 64.70% | 64.70% |
| Conformance | 90.12% | 90.12% | 72.76% | 72.76% |

False-positive diagnostics change from 1,283 to 1,283 in compiler
and from 2,293 to 2,293 in conformance. The accuracy report includes all option variants.

QuickInfo retains 307 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 0 diagnostic matches and 24 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,741 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `620040ffc`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-5999d5373.json`.

## 2026-10-12: property signature type inspection

Property names in interfaces and nested type literals now have exact
semantic type spans. This follows the declaration-name coverage of the
upstream type writer and includes identifier, quoted, numeric and computed
names. Optional properties include undefined under strict null checking;
scoped scalar aliases resolve to their underlying type for display.
The existing scoped type traversal supplies namespace and generic context.
Collection remains opt-in and does not change hover output.
Synthesized JSDoc members remain outside the source declaration walk.
Rejected interface implements clauses recover their heritage list before
parsing the body, reporting TS1176 without creating fake member names.
Seven new regressions cover these behaviors; semantic span results were
checked against TypeScript 6.0.3.
The unchanged upstream baselines remain the whole-case compatibility oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 740 | 813 |
| Conformance | 742 | 782 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,971 | 4,972 |
| Conformance | 4,044 | 4,046 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.26% | 88.30% | 64.70% | 64.70% |
| Conformance | 90.12% | 90.15% | 72.76% | 72.76% |

False-positive diagnostics change from 1,283 to 1,279 in compiler
and from 2,293 to 2,285 in conformance. The accuracy report includes all option variants.

QuickInfo retains 307 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 3 diagnostic matches and 113 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,748 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `b7a2dbb4e`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-620040ffc.json`.

## 2026-10-12: source-aware signature type printing

Signature node-building from pingdotgg/ts-rust is adapted to the native
checker. Source parameter names and unchanged type annotations retain their
provenance through assignments and generic substitutions. Changed types
are synthesized. The exact type printer records UTF-16 synthetic-node
underlining for declarations, callbacks, overloads, methods and constructors.
Destructuring names omit runtime initializers; constraints preserve source
spelling while generic defaults use semantic serialization.
The metadata does not affect type equality or hashing. Collection remains
opt-in and existing hover formatting is preserved. Fourteen new regression
checks cover the adaptation, including original TypeScript 6.0.3 writer
outputs and non-BMP parameter names.
The unchanged upstream baselines remain the whole-case compatibility oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 813 | 1,065 |
| Conformance | 782 | 981 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,972 | 4,972 |
| Conformance | 4,046 | 4,046 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.30% | 88.30% | 64.70% | 64.70% |
| Conformance | 90.15% | 90.15% | 72.76% | 72.76% |

False-positive diagnostics change from 1,279 to 1,279 in compiler
and from 2,285 to 2,285 in conformance. The accuracy report includes all option variants.

QuickInfo retains 307 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 0 diagnostic matches and 451 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,762 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `4f6150c3e`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-12-b7a2dbb4e.json`.

## 2026-10-13: source-aware object member type printing

Property and accessor node-building from pingdotgg/ts-rust is adapted to
the native checker. Unchanged member annotations retain source provenance
through object spreads, interface inheritance and generic substitutions.
An explicit member replacement discards the inherited declaration; a later
spread restores its own provenance. Changed types are synthesized.
Quoted property names preserve their source quote style. Reused strings
normalize quotes and escapes; readonly index signatures remain synthetic.
The setter checker also preserves a paired getter’s read type instead of
replacing it with any. The getSpreadSymbol adaptation retains a setter-only
property’s declared type while object spread copies undefined. Accessor kind
survives generic substitution and inheritance; data properties replace it.
Display metadata does not affect structural equality
or hashing. Collection remains opt-in and hover formatting is preserved.
Thirty-eight new regression checks cover original TypeScript 6.0.3 writer
outputs, strict optional fields, spreads, substitutions and type identity.
The unchanged upstream baselines remain the whole-case compatibility oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,065 | 1,092 |
| Conformance | 981 | 1,006 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,972 | 4,973 |
| Conformance | 4,046 | 4,047 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.30% | 88.31% | 64.70% | 64.70% |
| Conformance | 90.15% | 90.15% | 72.76% | 72.77% |

False-positive diagnostics change from 1,279 to 1,277 in compiler
and from 2,285 to 2,284 in conformance. The accuracy report includes all option variants.

QuickInfo passes change from 307 to 308. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 2 diagnostic matches and 52 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,800 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `afdac9284`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-4f6150c3e.json`.

## 2026-10-13: interface and type-literal method type collection

Method declaration names now receive their own type-baseline entries.
Collection reuses the native resolver and source-aware signature printer
inside the existing scoped declaration traversal. It covers generic and
optional methods, predicates, literal and computed names, nested type
literals, and merged interfaces. Source annotations retain their masks.
Overload declaration identity is retained independently of structural
signature deduplication: two equal signatures from distinct declarations
both print, while re-injecting the same file and span adds no overload.
Eighteen new regression tests use original TypeScript 6.0.3 writer text
and synthetic-node masks. Every test also checks that span collection
preserves ordinary diagnostics and hover output. Collection remains opt-in.
The unchanged upstream baselines remain the whole-case compatibility oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,092 | 1,120 |
| Conformance | 1,006 | 1,027 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,973 | 4,973 |
| Conformance | 4,047 | 4,047 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.31% | 88.31% | 64.70% | 64.70% |
| Conformance | 90.15% | 90.15% | 72.77% | 72.77% |

False-positive diagnostics change from 1,277 to 1,277 in compiler
and from 2,284 to 2,284 in conformance. The accuracy report includes all option variants.

QuickInfo retains 308 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 0 diagnostic matches and 49 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,818 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `8c504ed53`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-afdac9284.json`.

## 2026-10-13: class receiver return inference

Unannotated class methods and getters now resolve returns against the
complete instance or static receiver shape. Later-declared members,
inherited generic fields, constructor parameter properties and getter
chains participate in inference. Lazy resolution follows the donor
signature resolution stack and detects direct and indirect cycles.
Overload declarations remain authoritative. Arrows keep lexical receivers;
ordinary functions use their own receiver while preserving captured aliases.
Explicit receiver annotations and async returns retain their native types.
Forty-six regression tests cover TypeScript 6.0.3 type text, synthetic-node
masks, namespace isolation and inherited setter diagnostics. Declaration
span tests also check ordinary diagnostics and hover output. Donor
attribution and licenses are retained. Native scope restoration keeps
sibling methods independent; inherited setters retain generic write types.
The unchanged upstream baselines remain the whole-case compatibility oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,120 | 1,127 |
| Conformance | 1,027 | 1,033 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,973 | 4,977 |
| Conformance | 4,047 | 4,048 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.31% | 88.42% | 64.70% | 64.74% |
| Conformance | 90.15% | 90.17% | 72.77% | 72.79% |

False-positive diagnostics change from 1,277 to 1,265 in compiler
and from 2,284 to 2,279 in conformance. The accuracy report includes all option variants.

QuickInfo retains 308 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 5 diagnostic matches and 13 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,864 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `b4c0639af`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-8c504ed53.json`.

## 2026-10-13: class and namespace declaration types

Class members now contribute type-baseline entries in namespaces, nested
scopes, class expressions and anonymous default classes. The shared recorder
projects native class shapes, preserving overloads, optional members and
separate method/accessor declaration types in invalid programs.
Namespace declaration meaning adapts the donor module-instance classifier
and export-alias resolver. Native scope lookup preserves aliases and qualifies
shadowed namespace names. Empty and type-only namespaces stay omitted.
Sixty-seven regression tests compare original TypeScript 6.0.3 type text
and synthetic-node masks and verify unchanged diagnostics and hover output.
Donor attribution and licenses are retained. Unchanged upstream baselines
remain the whole-case compatibility oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,127 | 1,248 |
| Conformance | 1,033 | 1,078 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,977 | 4,977 |
| Conformance | 4,048 | 4,048 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.42% | 88.42% | 64.74% | 64.74% |
| Conformance | 90.17% | 90.17% | 72.79% | 72.79% |

False-positive diagnostics change from 1,265 to 1,265 in compiler
and from 2,279 to 2,279 in conformance. The accuracy report includes all option variants.

QuickInfo retains 308 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 0 diagnostic matches and 166 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,931 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `8d97319d6`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-b4c0639af.json`.

## 2026-10-13: namespace value types

Namespace values retain their qualified declaration identity and print
TypeScript type queries through native lexical scopes. Local aliases,
nested namespaces and shadowing signature parameters participate in name
selection. This adapts the donor symbolToTypeNode value-symbol path while
keeping native types and ordinary hover output authoritative.
Native inference resolves namespace member returns and namespace function
exports. Import aliases and member names now contribute type entries;
global declaration names distinguish scripts from module augmentations.
Function declaration entries preserve their original annotations and
overload signatures even when a variable has the same name.
Template delimiter matching, readonly tuple spreads and suffix inference
preserve precise generic constraints. Array-map callbacks retain the
expected return context through both concise and block bodies.
Namespace completions resolve lexical bindings before selecting exports;
three server tests cover function members, declaration merging and shadowing.
18 additional tests compare complete diagnostic codes and source spans.
32 regression tests compare original TypeScript 6.0.3 type text and
synthetic-node masks and assert unchanged diagnostics and hover when
collecting spans. Donor attribution and licenses are retained. Unchanged
upstream baselines remain the whole-case compatibility oracle.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,248 | 1,402 |
| Conformance | 1,078 | 1,131 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,977 | 4,979 |
| Conformance | 4,048 | 4,048 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.42% | 88.48% | 64.74% | 64.78% |
| Conformance | 90.17% | 90.32% | 72.79% | 72.82% |

False-positive diagnostics change from 1,265 to 1,258 in compiler
and from 2,279 to 2,244 in conformance. The accuracy report includes all option variants.

QuickInfo retains 308 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 2 diagnostic matches and 207 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 4,984 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `09f151bbc`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-8d97319d6.json`.

## 2026-10-13: declaration-based library member types

The native checker instantiates member signatures from the active TypeScript
library declarations. The transplant adapts ts-rust member resolution and
failed-overload recovery while retaining the native type representation.
Overload groups, inherited iterator methods, readonly arrays and receiver
type parameters preserve their declaration metadata. Substituted caller
functions keep their own source provenance. Numeric index signatures supply
context for array literals, including tuple elements passed to concat.
Callback overload selection and explicit receiver inference preserve the
correct generic result. Boolean callback literals retain contextual types.
Parameter names inside function type annotations now contribute type entries.
Generic overload retries consider checked callback return types and anchor
failures at the return expression. Resolved object unions retain declaration identity
and display the original declaration type.
70 regression tests compare original TypeScript 6.0.3 type text, synthetic
masks and complete diagnostic codes and spans. They also verify unchanged
diagnostics and hover with span collection enabled. Donor attribution and
licenses are retained. Upstream cases and reference baselines are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,402 | 1,485 |
| Conformance | 1,131 | 1,169 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,979 | 4,987 |
| Conformance | 4,048 | 4,050 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.48% | 88.74% | 64.78% | 64.97% |
| Conformance | 90.32% | 90.32% | 72.82% | 72.90% |

False-positive diagnostics change from 1,258 to 1,230 in compiler
and from 2,244 to 2,244 in conformance. The accuracy report includes all option variants.

QuickInfo passes change from 308 to 311. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 10 diagnostic matches and 121 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 5,054 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `3849422e2`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-09f151bbc.json`.

## 2026-10-13: declaration-based primitive member types

The native checker resolves string, number, boolean, bigint and symbol
methods through their standard-library interfaces. Primitive and literal
receivers use ts-rust apparent-type mapping adapted to the native types.
Method overloads retain declaration order. Optional parameters preserve
their annotations, and non-strict null handling preserves source text while
using the resolved call result. Context-sensitive callbacks retain a callable
shape during overload selection. Rest callbacks preserve array rest types.
Calls to declaration-resolved primitive
methods now check their required and maximum argument counts. Unique-symbol
type nodes omit internal symbol identities.
62 regression tests compare original TypeScript 6.0.3 type text, available
synthetic-node masks and complete diagnostic codes and spans. They also
verify unchanged diagnostics and hover with span collection enabled. Donor
attribution and licenses are retained. Upstream cases and reference baselines
are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,485 | 1,503 |
| Conformance | 1,169 | 1,173 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,987 | 4,988 |
| Conformance | 4,050 | 4,050 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.74% | 88.77% | 64.97% | 64.97% |
| Conformance | 90.32% | 90.34% | 72.90% | 72.90% |

False-positive diagnostics change from 1,230 to 1,226 in compiler
and from 2,244 to 2,239 in conformance. The accuracy report includes all option variants.

QuickInfo retains 311 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 1 diagnostic matches and 22 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 5,116 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `973f8c8d8`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-3849422e2.json`.

## 2026-10-13: declaration-based library index and property types

The native checker resolves library properties and index signatures from
their declarations, following ts-rust member resolution and indexed-access
ordering. Receiver arguments instantiate inherited members, and omitted
library arguments use declaration defaults. Known properties take precedence
over index signatures; unchecked reads add undefined only in strict mode.
Readonly library writes retain TypeScript property/index recovery behavior.
Indexed aliases of primitive results and index-signature parameter names
now have their semantic types in the type-baseline output.
136 regression tests compare original TypeScript 6.0.3 type text, available
synthetic-node masks and complete diagnostic codes and spans. They also
verify unchanged diagnostics and hover with span collection enabled. Donor
attribution and licenses are retained. Upstream cases and reference baselines
are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,503 | 1,540 |
| Conformance | 1,173 | 1,199 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,988 | 4,988 |
| Conformance | 4,050 | 4,051 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.77% | 88.77% | 64.97% | 65.00% |
| Conformance | 90.34% | 90.34% | 72.90% | 72.91% |

False-positive diagnostics change from 1,226 to 1,226 in compiler
and from 2,239 to 2,239 in conformance. The accuracy report includes all option variants.

QuickInfo retains 311 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 1 diagnostic matches and 63 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 5,252 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `4ce09d34c`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-973f8c8d8.json`.

## 2026-10-13: lexical generic scopes and nested parameter identities

The native checker resolves generic parameters before same-named library
types and namespace members. Cached library declarations enter their own
resolution environment. Nested generic function expressions retain distinct
declaration identities, so captured outer parameters survive inference and
call-site substitution even when an inner parameter has the same name.
Source-aware printing uses scoped names and annotation reuse adapted from
ts-rust. Optional function-expression parameters include undefined under
strict null checking, and signature/alias declarations receive type spans.
104 regression tests compare original TypeScript 6.0.3 type text, available
synthetic-node masks and complete diagnostic codes and spans. They cover
nested constraints, defaults, captures, instantiation, invalid returns and
Unicode offsets, and verify diagnostics and hover parity with span tracking.
Readonly checks distinguish union and intersection property mutability,
including getter-only and paired getter/setter declarations. Class accessor
writes through intersections combine setter input types separately from reads.
Donor attribution and licenses are retained. Upstream cases and reference
baselines are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,540 | 1,688 |
| Conformance | 1,199 | 1,272 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,988 | 4,990 |
| Conformance | 4,051 | 4,051 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.77% | 88.84% | 65.00% | 65.08% |
| Conformance | 90.34% | 90.35% | 72.91% | 72.91% |

False-positive diagnostics change from 1,226 to 1,219 in compiler
and from 2,239 to 2,237 in conformance. The accuracy report includes all option variants.

QuickInfo retains 311 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 2 diagnostic matches and 221 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 5,356 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `c69bf94aa`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-4ce09d34c.json`.

## 2026-10-13: class heritage types and constructor queries

Class heritage queries instantiate native constructor signatures, including
generic arguments, dependent defaults, declaration-library constructors,
qualified class names and aliases. The base expression displays its instance
type while nested expression names retain their constructor types.
Named class expressions preserve source names without changing their nominal
identities. Anonymous mixin displays retain the generic base intersection.
Type-query declarations and shorthand properties retain complete type spans.
Invalid constructor return types, unknown bases and nongeneric class arguments
receive the corresponding TypeScript diagnostics in source order.
88 regression tests compare original TypeScript 6.0.3 type text, available
synthetic-node masks and complete diagnostic codes and spans in strict and
non-strict modes. Diagnostics and ordinary hover results agree with type-span
tracking enabled and disabled. Donor attribution and licenses are retained.
Upstream cases and reference baselines are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,688 | 1,862 |
| Conformance | 1,272 | 1,390 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,990 | 4,994 |
| Conformance | 4,051 | 4,053 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.84% | 88.89% | 65.08% | 65.11% |
| Conformance | 90.35% | 90.37% | 72.91% | 72.93% |

False-positive diagnostics change from 1,219 to 1,214 in compiler
and from 2,237 to 2,233 in conformance. The accuracy report includes all option variants.

QuickInfo retains 311 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 6 diagnostic matches and 292 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 5,444 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `63ae6da5d`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-c69bf94aa.json`.

## 2026-10-13: heritage checking without library files

Heritage validation recognizes the native built-in instance registry when
TypeScript declaration libraries are unavailable. User aliases still resolve
first, so an alias named Object with a primitive body remains an invalid base.
An isolated subprocess regression test covers built-in bases, declaration
files, interface values inside base expressions and a shadowing primitive
alias, with type-span tracking enabled and disabled.
All four JavaScript/diagnostic matrices also run without library files to
match the GitHub baseline jobs. They restore the three cases rejected by
the previous CI run while retaining the existing verified type gains.
Upstream cases and reference baselines are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,862 | 1,862 |
| Conformance | 1,390 | 1,390 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,994 | 4,994 |
| Conformance | 4,053 | 4,053 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.89% | 88.89% | 65.11% | 65.11% |
| Conformance | 90.37% | 90.37% | 72.93% | 72.93% |

False-positive diagnostics change from 1,214 to 1,214 in compiler
and from 2,233 to 2,233 in conformance. The accuracy report includes all option variants.

QuickInfo retains 311 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 0 diagnostic matches and 0 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 5,445 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `c7c7197be`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-63ae6da5d.json`.

## 2026-10-13: enum object and member type queries

The native checker distinguishes enum values from enum object types,
canonicalizes members with equal constant values, and retains enum identities
through aliases, namespace merges and lexical scopes. Member access, reverse
indexes, object spreads and type-query rendering share the same enum tables.
The implementation adapts enum symbol queries from pingdotgg/ts-rust; donor
attribution is retained in the type checker NOTICE. The native checker remains
the implementation used by the public compiler.
Sixty-six focused tests compare type text, available node masks and diagnostics
with the original TypeScript 6.0.3 oracle, with span tracking enabled and disabled.
A JavaScript regression also checks enum expando declarations and readonly members.
Upstream cases and reference baselines are unchanged.

| Suite | Type passes before | After |
|---|---:|---:|
| Compiler | 1,862 | 1,971 |
| Conformance | 1,390 | 1,471 |

| Suite | Diagnostic passes before | After |
|---|---:|---:|
| Compiler | 4,994 | 4,995 |
| Conformance | 4,053 | 4,053 |

| Diagnostic accuracy | Precision before | After | Recall before | After |
|---|---:|---:|---:|---:|
| Compiler | 88.89% | 88.92% | 65.11% | 65.18% |
| Conformance | 90.37% | 90.37% | 72.93% | 72.93% |

False-positive diagnostics change from 1,214 to 1,212 in compiler
and from 2,233 to 2,233 in conformance. The accuracy report includes all option variants.

QuickInfo retains 311 passes. Comparing every supported
LSP failure set finds no newly failing cases or additional skips.

Whole-case baselines gain 1 diagnostic matches and 190 type matches.
All twelve measured baseline lanes have zero passed-to-failed cases and no
new skips. The public JavaScript, diagnostic and type results match the six
private verification manifests exactly.

Cache-free reports cover JavaScript, diagnostics, symbols, types, expanded
JavaScript/declarations, diagnostic accuracy and all supported LSP operations.
Skips are excluded from pass rates. Remaining failures remain visible in the
published snapshot; this batch does not establish full TypeScript compatibility.

All 5,512 public Rust tests pass, with 37 ignored, and `make ci` passes.
The measured compiler commit is `543ff7588`. The preceding
snapshot is retained at `docs/compatibility-metrics-2026-10-13-c7c7197be.json`.

## 2026-10-13: complete go-to-definition coverage

Go-to-definition passes all 213 TypeScript fourslash cases, with zero failures
and zero skips. The two previously skipped cases now execute range-based
requests for quoted import and export names. Upstream tests and reference
baselines are unchanged.

The harness now compares every returned declaration at its exact file and
position. Earlier measurements checked the first target and allowed nearby
positions or same-name matches. Omitted source lines and definition metadata
are now parsed accurately, and mixed-operation baselines stay separated.

The server follows selected overloads, inherited constructors, merged symbols,
generic and mapped property origins, contextual callbacks and module aliases.
Keyword and JSDoc navigation also work through the editor protocol, which
returns every definition location. Sixty-three original tests compare complete
location spans with TypeScript 6.0.3. Two editor-protocol tests and four harness
regressions bring the added regression coverage to 69 passing tests.

| LSP operation | Passed before | Passed now | Failed now | Skipped now |
|---|---:|---:|---:|---:|
| QuickInfo | 311 | 311 | 218 | 10 |
| Completions | 860 | 860 | 271 | 0 |
| Go-to-definition | 191 | 213 | 0 | 0 |
| Find-all-references | 329 | 329 | 18 | 0 |
| Signature help | 123 | 123 | 22 | 7 |

Every compiler and conformance baseline lane retains all previous passes.
Cache-free reports cover JavaScript, diagnostics, symbols, Types, expanded
JavaScript/declarations and diagnostic accuracy. All twelve public comparisons
have zero losses or new skips; the six JavaScript, diagnostic and Types reports
match the private manifests. No diagnostic code loses matches or gains false
positives. Other supported LSP operations retain all previous passing cases.

All 5,581 public Rust tests pass, with zero failures and 37 ignored.
Formatting, `make ci` and workspace Clippy pass. Skips are excluded from pass
rates; remaining failures in other compiler and LSP operations remain visible.

Measured public source: `a7f80796d`. The preceding snapshot is
retained at `docs/compatibility-metrics-2026-10-13-543ff7588.json`.

## 2026-10-13: constructor references and exact reference measurement

Find-all-references now compares every returned file and complete span with the
TypeScript fourslash baseline. The old harness accepted partial counts and
included duplicated definition displays. Its published 329/347 (94.8%) result
was not an exact-location result and is not comparable with this snapshot.
With the same stricter harness, the preceding server passes 143/347;
this batch passes 151/347, with 196 failures and zero skips.
There are 8 newly passing cases and no lost passes under this comparison.
Repeated requests and text insertions are replayed; explanatory definition
copies are excluded, omitted source lines are restored, and malformed or
incomplete comparisons cannot become passes.

Constructor references now distinguish constructor declarations and construction
sites from ordinary class-name uses. Overloads, inherited constructors, access
modifiers, shadowed classes and imports have independent TypeScript 6.0.3 oracle
regressions. Upstream test cases and baseline files are unchanged.

Go-to-definition remains 213/213 with no skips. The other supported LSP operations
retain their previous passing cases and skip counts. All twelve public compiler
and conformance comparisons have zero losses or new skips; the six JavaScript,
diagnostic and Types manifests match the private measurements. Diagnostic
accuracy retains matches without additional false positives for every code.

All 5,600 public Rust tests pass, with zero failures and
37 ignored. Formatting, `make ci` and workspace Clippy pass.
Measured public source: `cdcf42764`. The previous snapshot remains
at `docs/compatibility-metrics-2026-10-13-a7f80796d.json`.

## 2026-10-13: compatibility rounds 1-10

Ten verified rounds improve parser diagnostics, conditional type constraints,
JavaScript resource disposal and declaration emission. Each round preserves
previous passes, oracle identities and skip counts across all twelve compiler
and conformance matrices. Public measurements match the private results.

| Round | Change | Newly passing comparisons |
|---|---|---:|
| 1 | [preserve internal namespace import aliases](https://github.com/benfavre/ts-rs/commit/11820b47e2ea768c4cdd4b170b9ea8e51b561f72) | 17 |
| 2 | [preserve resource scopes around derived constructors](https://github.com/benfavre/ts-rs/commit/4ccb1b7ff6318c0b567e6916406966cbdb099348) | 1 |
| 3 | [diagnose incomplete if statement recovery](https://github.com/benfavre/ts-rs/commit/7cd2c5e4c85c58589029ce6059d38662bb6b75f2) | 4 |
| 4 | [report duplicate function implementations](https://github.com/benfavre/ts-rs/commit/cb0f80d4ece56ae4dc5e0163908cc60d9bcd3f20) | 3 |
| 5 | [diagnose decimal exponents without digits](https://github.com/benfavre/ts-rs/commit/277bfedc2c18344f7ee9247b36f7fbc2ec4ca53e) | 4 |
| 6 | [separate declaration baseline groups once](https://github.com/benfavre/ts-rs/commit/e4548505e819e79736231f8929b0a18a88f3f7af) (measurement correction) | 67 |
| 7 | [check primitive conditional constraints](https://github.com/benfavre/ts-rs/commit/d18afe729f3d00dc65104362a2b707325609e78c) | 1 |
| 8 | [diagnose declared export assignment modifiers](https://github.com/benfavre/ts-rs/commit/6a938ef1e71c4142be21eab7868973bb70f6afb3) | 2 |
| 9 | [respect line breaks before conditional extends](https://github.com/benfavre/ts-rs/commit/3f532aa25b0b3901aaa92fe0b9c8c6bc17cb912a) | 4 |
| 10 | [omit empty destructuring from declarations](https://github.com/benfavre/ts-rs/commit/75023f14b9056b8d1ce6a07eff6ed2156f5a3bbb) | 3 |

The declaration section separator correction accounts for 67 additional passing
variant comparisons (41 compiler and 26 conformance). It changes report assembly;
emitted declaration payload bytes remain identical. These are measurement
corrections, not 67 new compiler behaviors. Other gains change compiler behavior.
Variant comparisons are not counts of distinct source programs.

| Suite | Baseline | Before | After | Failed | Skipped |
|---|---|---:|---:|---:|---:|
| Compiler | Diagnostics | 4,995 | 5,002 | 1,527 | 0 |
| Compiler | JavaScript | 6,032 | 6,032 | 0 | 497 |
| Compiler | Types | 1,971 | 1,972 | 4,462 | 95 |
| Compiler | Symbols | 1,731 | 1,732 | 4,702 | 95 |
| Compiler | Expanded JavaScript | 6,494 | 6,494 | 210 | 497 |
| Compiler | Expanded declarations | 5,844 | 5,904 | 800 | 497 |
| Conformance | Diagnostics | 4,053 | 4,061 | 1,846 | 0 |
| Conformance | JavaScript | 5,388 | 5,388 | 0 | 519 |
| Conformance | Types | 1,471 | 1,471 | 4,146 | 290 |
| Conformance | Symbols | 1,590 | 1,590 | 4,027 | 290 |
| Conformance | Expanded JavaScript | 6,444 | 6,445 | 654 | 519 |
| Conformance | Expanded declarations | 6,314 | 6,342 | 757 | 519 |

Diagnostic accuracy checks preserve matches and do not add false positives for
any TS code. The five supported language-server operations preserve their
passing cases and skip counts. The full inventory, including unsupported
operations, is retained in the metrics snapshot.

All 5,656 public Rust tests pass, with zero failures and
37 ignored. Formatting, `make ci` and workspace Clippy pass.
Pass rates exclude skips.
Remaining diagnostic, emit, type, symbol and LSP failures remain visible.

Measured public source: `75023f14b`. The preceding snapshot remains at
`docs/compatibility-metrics-2026-10-13-cdcf42764.json`.
