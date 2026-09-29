# Quality and performance progress

Measured on 2026-09-06, starting from
`6faa2983337cd0dbf925baa02470aec9077b4c7d`, with Rust 1.98.0.
The goal of complete compatibility and leading performance remains open.

## Changes and Rust checks

- Recovered optional-chain targets in `for-in` and `for-of` now emit with
  assignment precedence. Nested expressions retain their parentheses.
- ES5 destructuring tests now require extracted bindings instead of expecting
  ES2015 syntax. Tests using `startsWith`, `find`, and `findIndex` explicitly
  select ES2015.
- The Temporal integration test reports its library prerequisite and no longer
  silently succeeds when no library is installed.
- The parser checks for `<` before inspecting the preceding expression for JSX
  recovery on each binary-expression iteration.
- Applied the workspace formatter to clear the existing formatting failures.

`cargo test --workspace --no-fail-fast`: **2,974 passed, 0 failed, 37 ignored**.
`make ci` and `git diff --check` also passed. Rust tests and CI used:

```bash
TSC_RS_TYPESCRIPT_LIB_DIR=/path/to/node_modules/typescript/lib \
  cargo test --workspace --no-fail-fast
TSC_RS_TYPESCRIPT_LIB_DIR=/path/to/node_modules/typescript/lib make ci
```

That installation is TypeScript 6.0.3. Automatic discovery on this checkout
selects the editor extension's TypeScript 5.9.3, which lacks Temporal. See
[CONTRIBUTING.md](../CONTRIBUTING.md) for the portable library-selection command.

## Compatibility

All reports below were cache-free. Before and after reports both used the
automatically discovered TypeScript 5.9.3 library to hold the environment fixed.
Counts are exact harness oracle comparisons, not diagnostic recall scores.

| Suite / lane | Passed | Failed | Skipped |
| --- | ---: | ---: | ---: |
| Compiler, default JS | 6,032 | 0 | 497 |
| Conformance, default JS | 5,388 | 0 | 519 |
| Compiler, errors | 3,633 | 2,896 | 0 |
| Conformance, errors | 2,785 | 3,122 | 0 |
| Compiler, expanded JS | 6,414 | 290 | 497 |
| Conformance, expanded JS | 6,340 | 759 | 519 |
| Compiler, expanded declarations | 5,730 | 974 | 497 |
| Conformance, expanded declarations | 6,283 | 816 | 519 |

`baseline-compare` passed for all eight lanes, with unchanged identities,
oracles, and skips and zero losses. `elementAccessChain.3` and
`propertyAccessChain.3` changed from failed to passed in default and expanded
conformance JS; these are the same two gains, not four distinct cases.
Other lanes were unchanged. Skipped cases are not counted as passes.

Reports used `baseline-report --no-cache --save-manifest`; expanded reports also
used `--expand-variants --manifest-schema 2`. Declaration counts follow the
harness's declaration-output lane, including cases with no declaration section.

## Parser measurements

Both binaries used `cargo build --profile perf -p tsc_rs_bench --example prof_oxc`
(fat LTO, one codegen unit, mimalloc). Runs were pinned to CPU 4 and alternated
before/after order. Confirmation used ten pairs of 30 parses for `typescript.js`
and six pairs for each other fixture (100 parses for `parser.ts`, 200 for the
smaller fixtures).

| Fixture | User instructions | User cycles | Parse time before / after |
| --- | ---: | ---: | ---: |
| `typescript.js` | -0.58% | -0.26% | 44.08 / 42.94 ms |
| `parser.ts` | -0.44% | -2.64% | 2.030 / 1.965 ms |
| `table.tsx` | -0.38% | -1.14% | 0.17 / 0.16 ms |
| `renderer.ts` | -0.53% | -1.95% | 0.34 / 0.34 ms |

Times are medians of per-block minima, not medians of individual parses.
The profiler rounds times to 0.01 ms, limiting precision for small fixtures.
Counters used `instructions:u,cycles:u,branches:u,branch-misses:u` and include
process setup and AST destruction; the reported parse timer excludes destruction.
Host load varies, so these are local measurements rather than universal speedups.

A separate six-pair comparison using the candidate binary and the same large
fixture measured **41.11 ms for tsc-rs versus 35.845 ms for oxc 0.117.0** by the
same block-minimum method. The parser is still slower than oxc on this fixture.

Session evidence (logs, manifests, benchmark samples, and binary/fixture SHA-256
hashes) is in `/tmp/ts-rs-progress-20260906/` on this host. After this first batch, the remaining correctness
work comprised 6,018 default diagnostic failures and expanded emit failures;
the performance goal requires further measured improvements and broader
comparative benchmarks.


## Interface inheritance diagnostics

The second batch adds declaration-site TS2430 checks for incompatible interface
members and index signatures. It checks optionality, nested properties,
private/protected class members, method variance, and call/construct signatures,
including contextual generic callback inference. Base members are instantiated
on demand, and recursive relations track active pairs to bound repeated work.
Twenty regression tests cover these behaviors, including expanding recursive
type arguments and namespace-qualified generic bases.

The final `cargo test --workspace --no-fail-fast` run passed **2,994 tests**,
with **0 failures and 37 ignored**. `make ci`, the format check, and
`git diff --check` passed. Rust tests and CI used the same TypeScript 6.0.3
library override documented above.

The following cache-free comparisons use the first batch above as their base:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,638 | 2,891 | 0 | 5 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,414 | 290 | 497 | 0 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,791 | 3,116 | 0 | 6 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,340 | 759 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with unchanged case identities, oracle selections,
and skip counts. Eleven previously failing diagnostic cases now match their
complete oracles: five compiler cases and six conformance cases. No upstream
cases or reference baselines were modified.

A separate expanded diagnostic scan finds 11 of the 14 expected compiler
TS2430 diagnostics and 106 of 183 conformance TS2430 diagnostics, with zero
spurious TS2430 diagnostics in either suite. These location/code matches are a
different metric from complete baseline passes. Overall diagnostic counts are:

| Suite | Matched | Missed | Spurious |
| --- | ---: | ---: | ---: |
| Compiler | 6,465 | 8,451 | 3,649 |
| Conformance | 13,208 | 15,527 | 5,454 |

This is partial TS2430 coverage. Generic constraint/shadowing details, some
recursive collection relations, and diagnostic text/related spans still need
work. The default diagnostic suites retain 6,007 failing cases, and the expanded
emit failures shown above remain open. This batch makes no new performance claim;
the earlier parser measurements do not establish whole-compiler leadership.

Evidence for this batch is in `/tmp/ts-rs-diagnostics-20260906/` on this host.
The final manifests and diagnostic reports have the suffix `v11`; intermediate
checkpoints in that directory include rejected experiments.


## Conflicting inherited properties

The third batch implements TS2320 for interfaces inheriting incompatible
properties from multiple bases. It compares property types and optional/readonly
flags, preserves private/protected declaration identity, and handles generic
signature binders, structural aliases, recursive types, and merged interface
fragments. It includes a merged class's base and allows explicit own members to
resolve otherwise conflicting inherited members.

Mapped inherited properties now retain optionality, including the difference
between omission and explicit `undefined` under `exactOptionalPropertyTypes`.
Interface fragments can satisfy a merged class's abstract-member requirements;
an unrelated fragment still leaves the missing-member error in place.
Fifteen new regression tests cover these behaviors. The final workspace run
passed **3,009 tests**, with **0 failures and 37 ignored**; `make ci`, the
format check, and `git diff --check` passed. Rust tests and CI used the same
TypeScript 6.0.3 library override documented above.

The check returns early for
fewer than two bases and instantiates member types only when names overlap.

Cache-free comparisons against the second batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 11 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,414 | 290 | 497 | 0 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 5 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,340 | 759 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed: **16 new complete diagnostic baseline passes**,
zero losses, and unchanged case identities, oracle selections, and skips. The
expanded diagnostic scans match all **27 expected TS2320 diagnostics** (18
compiler, nine conformance), with zero missing or spurious TS2320 diagnostics.
This is coverage of the current corpus, not proof of complete type-system parity.

| Suite | Matched | Missed | Spurious |
| --- | ---: | ---: | ---: |
| Compiler | 6,483 | 8,433 | 3,649 |
| Conformance | 13,219 | 15,516 | 5,452 |

Compared with the second batch, these scans add 29 correct diagnostic matches
and remove two false positives. No upstream tests or reference baselines were
modified. The default diagnostic suites still have **5,991 failing cases**;
expanded emit failures and the whole-compiler performance goal remain open.
This batch makes no new speed claim.

Evidence is in `/tmp/ts-rs-multiple-bases-20260906/` on this host. The final
manifests and reports use suffix `v7`; earlier checkpoints include experiments.


## Scanner vector classification performance

A fourth batch reduces the SSE2 ASCII identifier classifier's range checks.
Wrapping byte addition moves each valid letter/digit range to signed `-128`,
allowing one signed comparison to check both bounds. The load width and bounds
guards are unchanged, as are Unicode/escape handling and the portable scanner.
The existing exhaustive test checks every byte in every vector lane against the
scalar classifier; all 77 scanner tests passed.

A `cycles:u` profile identified identifier-tail scanning as the largest scanner
hotspot. Before/after binaries used the same `perf` profile (fat LTO, one codegen
unit) and mimalloc allocator. Measurements were pinned to CPU 4 on an AMD Ryzen
9 5900X. Eight scanner pairs alternated before/after order; each process scanned
60 times for `typescript.js`, 200 for `parser.ts`, or 100 for `cal.com.tsx`.

| Scanner fixture | Before / after (ms) | Time change | User instructions | User cycles |
| --- | ---: | ---: | ---: | ---: |
| `typescript.js` | 13.795 / 13.580 | -1.56% | -0.78% | -1.09% |
| `parser.ts` | 0.695 / 0.695 | 0.00% | -0.79% | +0.004% |
| `cal.com.tsx` | 2.155 / 2.080 | -3.48% | -0.61% | -3.44% |

Times are medians of per-process minima, rounded by the profiler to 0.01 ms
before aggregation. All eight paired scanner runs were faster on each of the
two larger fixtures; the `parser.ts` timing difference was indistinguishable
from noise. These are local measurements, not universal speedup guarantees.

Six alternating parser pairs on each of `typescript.js`, `parser.ts`,
`table.tsx`, and `renderer.ts` reduced user instructions by 0.22–0.25% and median
user cycles by 0.29–1.78%. Parser elapsed-time differences were small/noisy; this
batch does not claim a statistically established parser elapsed-time speedup.
Counter totals include process setup and AST destruction, while the parser's
timed region excludes destruction.

A fresh six-pair comparison of the candidate with oxc 0.117.0 measured:

| Parser fixture | tsc-rs (ms) | oxc (ms) |
| --- | ---: | ---: |
| `typescript.js` | 41.36 | 36.25 |
| `parser.ts` | 1.970 | 1.945 |
| `table.tsx` | 0.16 | 0.16 |
| `renderer.ts` | 0.34 | 0.34 |

Equal rounded values on the small fixtures do not prove equal performance.
The large fixture remains slower than oxc. These parser measurements also do
not establish whole-compiler performance leadership.

Binaries, source/fixture SHA-256 hashes, the CPU/profile record, raw paired
measurements, and the profile are in `/tmp/ts-rs-perf-20260906/` on this host.
`measure.py`, `measure_scan.py`, and `compare_oxc.py` record the measurement
commands and alternation order. `paired-summary.json` retains individual timing
changes and exploratory bootstrap intervals.


The final workspace run passed **3,009 tests**, with **0 failures and 37
ignored**. `make ci`, the format check, and `git diff --check` passed. Rust
checks used the same TypeScript 6.0.3 library override as the earlier batches.
All eight cache-free baseline comparisons against the third batch passed with
unchanged case identities, oracles, passes, failures, and skips. The default
diagnostic suites therefore still contain **5,991 failing cases**; this batch
improves scanner performance without changing compatibility results.


## ES5 class fields and erased members

The fifth batch extends the ES5 class IIFE transform to assignment-semantics
instance and static fields. Base constructors initialize fields after parameter
prologues; derived constructors initialize them on the object returned by
`super`, before the remaining constructor body. Static assignments follow method
and accessor installation. Generic method parameters and TypeScript visibility
modifiers are erased, and signature-only methods/accessors and declared fields
no longer prevent class lowering.

Constructor expression temporaries now share the existing function-scope
machinery. Hoisted temporaries follow directive prologues, preserving
`"use strict"`. Seven new tests cover initialization order, replacement objects
returned by a base constructor, static method availability, function and arrow
receivers, abstract/declared member erasure, local temporaries, directive
placement, and the nested-class environment boundary. The existing dependency
test now uses a static block to exercise an unsupported base; initialized fields
are supported by this batch.

`baseline-case --variant key=value,...` selects an exact expanded oracle and
rejects malformed, missing, or ambiguous selections. Eight CLI smoke checks
cover selection, declaration output, the unchanged default path, and invalid
attributes. For example:

```bash
cargo run -p tsc_rs_harness --bin baseline-case -- \
  abstractPropertyBasics --variant target=es5 --show-outputs
```

The final workspace run passed **3,016 tests**, with **0 failures and
37 ignored**. `make ci`, the format check, and `git diff --check` passed.
Rust tests and CI used the TypeScript 6.0.3 library override documented above.

Cache-free comparisons against the fourth batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,422 | 282 | 497 | 8 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,352 | 747 | 519 | 12 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed, with unchanged case identities, oracle selections,
and skip counts: **20 new complete expanded-JS passes and zero losses**. No
upstream tests or reference baselines were modified.

This remains partial ES5 support. Define-semantics fields, side-effectful computed
field names, nested class initializers, field/method `super` references, and
complex derived-constructor control flow retain their existing emission paths.
The conservative environment checks may also retain expressions containing
`class` or `super` in source text. A nested-class initializer experiment exposed
receiver-alias leakage; this batch preserves the prior path for that boundary
rather than enabling the incorrect IIFE output. Full downleveling of arrows and
other unsupported class shapes remains separate work.

The default diagnostic suites still contain **5,991 failures**. Expanded JS has
**1,029 failures**, and expanded declarations have **1,790 failures**; these are
separate lanes and can cover overlapping source cases. Skips remain excluded
from passes. This batch makes no new speed claim, and the earlier benchmark
still does not establish whole-compiler performance leadership.

Evidence is in `/tmp/ts-rs-emit-20260906/` on this host. Final manifests,
comparisons, workspace tests, and CI logs use suffix `v4`; earlier checkpoints
include rejected experiments. `verify-v4.py` records all eight report/comparison
commands, and `verified-input-sha256.json` records the source and tool binaries.


## ES5 async state machines

The sixth batch adds an explicit resumable-control-flow plan for async function
bodies. Supported bodies now emit `__generator` state machines instead of leaving
native generator syntax in ES5 output. The plan represents suspension, resume,
return/throw, conditional branches, and fallthrough label updates. It retains
ordinary `if` blocks when their arms do not suspend. Variable declarations are
hoisted into the generator closure so values survive suspension.

The entire body is planned before any output changes. The transform currently
supports `var` declarations, expression/return/throw statements, blocks and
conditionals, with direct or nested awaits and supported unary/type/binary
wrappers. A binary RHS that suspends still needs value staging and is not yet
admitted. Helper scanning follows supported async declarations/methods and skips
targets that preserve generators. Inline helpers honor `noEmitHelpers`.

Eight new executable tests cover receiver preservation, concurrent invocations,
rejection, empty completion, parameter redeclarations, generated-name/temp
separation, directive prologues, atomic rejection of unsupported plans, branch
fallthrough, comma return values, nullish expressions, and import/export
shadowing. Tests exposed and fixed wrong resume labels, source-copy restoration
of erased `await`, and references incorrectly resolving to outer module bindings.
The generator closure uses the existing function-scope/temp machinery and
restores import/export shadowing after emission.

The final workspace run passed **3,024 tests**, with **0 failures and
37 ignored**. `make ci`, the format check, and `git diff --check` passed.
Rust checks used the TypeScript 6.0.3 library override documented above.

Cache-free comparisons against the fifth batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,425 | 279 | 497 | 3 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,364 | 735 | 519 | 12 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed: **15 new complete expanded-JS passes**, zero
losses, unchanged case identities/oracle selections, and unchanged skips. Gains
include `es5-asyncFunctionIfStatements`, `generatorTransformFinalLabel`,
`asyncUseStrict_es5`, and several await-expression and function-declaration cases.
No upstream cases or reference baselines were modified.

This is a foundation for full generator lowering, not complete ES5 async
compatibility. Loops, exception regions, expression values that must be saved
across a later await, parameter lifting, imported helpers, async arrows/function
expressions, and direct generator declarations need further integration. Comment
ownership and some declaration/helper scanning boundaries also remain incomplete.
Unsupported bodies retain their previous emission path and remain eligible to
fail the exact baseline comparison.

Default diagnostics retain **5,991 failures**. Expanded JS retains **1,014
failures**, and expanded declarations retain **1,790 failures**, in separate
potentially overlapping lanes. Skips are not passes. This batch makes no speed
claim; earlier benchmarks still do not establish performance leadership.

Evidence is in `/tmp/ts-rs-generators-20260906/`. Final manifests, comparisons,
workspace/CI logs, and source/tool hashes use suffix `v7`. `verify-v7.py` records
the eight cache-free report/comparison commands. Earlier checkpoints include
rejected experiments; a superseded workspace run also encountered stale crate
metadata during concurrent rebuilding, resolved by the final uninterrupted run.

## ES5 async expression evaluation

The seventh batch extends the generator plan to preserve values and references
across awaits in binary expressions, property accesses, calls, assignments, and
array literals. Logical and conditional expressions retain lazy evaluation.
Compound assignments preserve getter/setter order, logical assignments skip
setters when short-circuited, and discarded awaits still propagate rejection.
Calls capture their receiver before suspended arguments; array and argument
prefixes retain their values and sparse-array holes. Exponentiation follows the
TypeScript lowering order through a staged `Math.pow` call.

Expression planning now lives in `generator/expressions.rs`. Generated names
avoid source identifiers, and persistent temporaries are allocated before the
state parameter. A private symbolic AST identifier connects resume expressions
to that parameter without rewriting user strings. Unsupported complete plans
still fall back atomically; the fallback regression now exercises direct eval,
whose scope semantics require separate support.

Seven new executable tests compare generated output with native async execution
and explicit expected results. The full workspace passed **3,031 tests**, with
**0 failures and 37 ignored**. The emitter suite passed **666 tests**. `make ci`,
the format check, and `git diff --check` passed. Rust checks used the documented
TypeScript 6.0.3 library override; baseline comparisons retained automatic
TypeScript 5.9.3 discovery for consistency with the base.

Cache-free comparisons against the sixth batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,429 | 275 | 497 | 4 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,372 | 727 | 519 | 8 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed: **12 new complete expanded-JS passes**, zero
losses, unchanged case identities/oracles, and unchanged skips. Gains include
`es5-asyncFunctionBinaryExpressions`, conditionals, property and element access,
and six awaited-call cases. No upstream cases or reference baselines changed.

Loops, exception regions, suspended spreads, optional calls, direct eval
extraction, constructors with suspended arguments, parameter lifting, imported
helpers, async arrows/function expressions, and direct generator declarations
still need work. Default diagnostics retain **5,991 failures**, expanded JS
retains **1,002 failures**, and expanded declarations retain **1,790 failures**
in separate, potentially overlapping lanes. This batch makes no new speed
claim; performance leadership remains unproven.

Evidence is in `/tmp/ts-rs-generator-expressions-20260906/`. Final manifests and
comparison logs use suffix `v5`; `verification-final.log` records all eight
successful report/comparison pairs. `verify-final.py` uses snapshots of the
verification executables taken after Cargo completed, avoiding a relinking race
in an earlier orchestration attempt. Workspace and CI evidence is in
`workspace-v5.log`, `rust-test-summary.json`, and `ci-v5.log`; source and tool
hashes are in `verified-input-sha256.json`.

Following the user's instruction, the accumulated verified improvements are
committed to `main`, and subsequent verified batches will be committed as work
continues. Unrelated local work is excluded from these commits.

## ES5 async loops

The eighth batch lowers suspending `while`, `do`, and `for` loops into the
generator state machine. Loop scopes distinguish native `break`/`continue`
statements from jumps to resumable states. Forward destinations resolve after
planning, including labeled exits and continues through nested native loops.
For-loop continues reach the update, do-loop continues reach the condition,
and breaks skip both. Initializers execute once, before the loop's back edge.

Loops without suspension remain native loops. Their bodies still use generator
return operations and hoist function-scoped variables. Structured bodies share
the enclosing local/temp allocation and control scopes; non-suspending `if`
arms preserve braced and unbraced forms. For-loop variable initializers retain
evaluation order when later initializers await.

Six new executable regression tests compare emitted JavaScript with native
async execution and explicit expected results. They cover awaited conditions,
initializers, updates and bodies; early exits; multiple labels; native and
suspending nested loops; variable hoisting; return; and rejection at each loop
phase. The full workspace passed **3,037 tests**, with **0 failures and
37 ignored**, including **672 emitter tests**. `make ci`, formatting, and
`git diff --check` passed, using the same library selections as the prior batch.

Cache-free comparisons against the seventh batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,433 | 271 | 497 | 4 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,372 | 727 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **four new complete expanded-JS passes**,
zero losses, and unchanged case identities, oracle selections, and skips.
The four gains are `es5-asyncFunctionWhileStatements`,
`es5-asyncFunctionDoStatements`, `es5-asyncFunctionForStatements`, and
`es5-asyncFunctionNestedLoops`. Upstream cases and reference baselines are
unchanged.

Generator lowering still needs for-in/of, block-scoped loop binding capture,
exception regions, switches, non-loop labeled blocks, and the other expression
and function-shape boundaries recorded above. Default diagnostics retain
**5,991 failures**, expanded JS retains **998 failures**, and expanded
declarations retain **1,790 failures**, in separate potentially overlapping
lanes. This correctness batch makes no new speed claim.

Evidence is in `/tmp/ts-rs-generator-loops-20260906/`. Final logs and manifests
use suffix `v1`; `verification-v1.log` records the eight successful cache-free
report/comparison pairs using snapshot verification executables.
`workspace-v1.log`, `rust-test-summary.json`, and `ci-v1.log` record Rust/CI
checks. `verified-input-sha256.json` binds the source files and tool binaries.

## Portable identifier scanning

The ninth batch fixes a cross-byte borrow in the portable eight-byte identifier
classifier. The subtract-one zero-byte mask falsely accepted `%` after `$` and
`^` after `_`, so input such as `a$%rhs` could become one identifier on non-x86
targets. The x86 SSE2 classifier did not have this bug.

The replacement uses the existing ASCII precondition: XOR with the comparison
byte, then add 127 in each lane. Each sum is at most 254, so there is no carry
between lanes; only a matching byte leaves its high bit clear. Inverting those
high bits gives an exact equality mask with fewer bitwise operations.

The new adjacent-byte regression failed before the fix on `[$, %, a, a, a, a,
a, a]`, reporting no delimiter instead of position 1. It now passes **344,064
checks** spanning all adjacent ASCII pairs, every adjacent lane, and three
identifier fill bytes. A second regression checks complete tokenization of
short and long identifiers followed by the two operators.

To exercise the actual non-x86 scanner path on this host, a WebAssembly probe
was compiled before and after with Rust 1.98.0, opt-level 3, fat LTO, and one
codegen unit. The same four tokenization cases went from **four failures to
zero** when executed in Node 25.7.0. All **79 scanner tests** pass natively.

The probe also measures scanner throughput on three unchanged fixtures. Both
WebAssembly instances ran in one Node process pinned to CPU 4 with
`--no-liftoff`, six warm-up rounds, and twelve alternating before/after pairs.
Each timed call includes scanning, allocation, and token/comment destruction;
compilation and warm-up are excluded. Each block scans the fixture 12, 60, or
40 times respectively. Token counts matched between both builds.

| Fixture | Before, ms/scan | After, ms/scan | Median paired change |
| --- | ---: | ---: | ---: |
| `typescript.js` | 30.523 | 30.284 | -0.14% |
| `parser.ts` | 1.636 | 1.614 | -1.39% |
| `cal.com.tsx` | 4.445 | 4.431 | -0.33% |

Times are medians of the twelve block averages; paired changes are medians of
the twelve within-pair percentages. A paired bootstrap interval includes zero
for `typescript.js` and `cal.com.tsx`, so these runs do not establish a speedup
there. The local `parser.ts` interval is approximately -3.38% to -0.22%.
These are WebAssembly scanner measurements, not native ARM/x86 or whole-compiler
performance claims. Performance leadership remains unproven.

The full workspace passed **3,039 tests**, with **0 failures and 37 ignored**.
`make ci`, formatting, and `git diff --check` passed. All eight cache-free native
baseline comparisons passed with zero gains, losses, or changes to skips,
case identities, and oracle selections. Native baseline counts remain those
of the eighth batch. The new passing cases establish a portability correction
beyond the existing native baseline coverage. No upstream cases or reference
baselines were modified.

Evidence is in `/tmp/ts-rs-swar-20260906/`: `red.log`, `scanner-green.log`,
`wasm-red-green.log`, the before/after WebAssembly binaries, the probe source
and Cargo manifest/lockfile, and the timing driver and samples.
`wasm-statistics.json` records the paired bootstrap calculation.
Final workspace/CI logs, manifests, and comparisons use suffix `v1`;
`verification-v1.log` records all eight successful comparisons.
`verified-input-sha256.json` binds source, tools, probe, dependencies, and
benchmark fixtures. Library selections remain consistent with prior batches.

## Async expression forms and helper discovery

The tenth batch connects supported async arrows, function expressions, and
object methods to the ES5 generator planner. Helper discovery now descends
through expression containers, function bodies, parameter initializers, binding
patterns, class members, and JSX expressions. Supported functions inside arrays,
conditionals, calls, and getters therefore request both `__awaiter` and
`__generator`. Discovery remains based on the body planner rather than emitting
a generator helper for every async construct.

Nested function and arrow definitions no longer count as suspension in their
enclosing expression plan. Their bodies are emitted in their own function
context. Concise async arrows retain TypeScript's compact callback layout while
sharing the existing function-scope machinery for locals and temporaries.
Top-level arrows without lexical-environment hazards also lower their outer
wrapper to an ordinary function.

Executable checks exposed `arguments` references resolving to the generator
callback's own arguments. Normal expression emission now honors active capture
aliases, including shorthand value references, and avoids source-copy paths
that would restore the original spelling. ES5 arrow emission restores the
enclosing alias after each arrow, keeping sibling captures separate. The older
ES2015 backend retains its existing capture-state behavior; an initial candidate
changed its parameter-evaluation oracle and was rejected before publication.

Six new runtime regressions compare generated execution with native async
execution and explicit results. They cover helper discovery, receivers and
arguments in function expressions, sibling lexical captures, delayed execution
of nested async definitions, object methods/getters, rejection, and concurrent
invocations. An older unsupported-arrow test now uses suspended direct eval,
which still requires fallback; simple async value-return arrows are covered by
the new successful execution tests.

The full workspace passed **3,045 tests**, with **0 failures and 37 ignored**,
including **678 emitter tests**. `make ci`, formatting, and `git diff --check`
passed. Library selections match the preceding batches.

Cache-free comparisons against the ninth batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,433 | 271 | 497 | 0 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,377 | 722 | 519 | 5 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **five new complete expanded-JS passes**,
zero losses, and unchanged skips, case identities, and oracles. Gains are
`asyncArrowFunction1_es5`, `asyncArrowFunction5_es5`,
`asyncArrowFunction10_es5`, `asyncUnParenthesizedArrowFunction_es5`, and
`asyncFunctionDeclarationCapturesArguments_es5`. Upstream cases and reference
baselines remain unchanged.

This does not complete ES5 arrow lowering. Lexical captures in enclosing
functions still use the existing outer arrow wrapper, so those outputs still
contain `=>` even when their resumable body no longer contains `function*`.
Capture hoisting, parameter lifting, rest/destructured parameters, imported
helpers, and the remaining generator expression/control-flow boundaries need
further work. Default diagnostics retain **5,991 failures**, expanded JS retains
**993 failures**, and expanded declarations retain **1,790 failures**, in
separate potentially overlapping lanes. This batch makes no speed claim.

Evidence is in `/tmp/ts-rs-async-expressions-20260906/`. Final cache-free
manifests/comparisons and workspace/CI logs use suffix `v2`;
`verification-v2.log` records all eight successful report/comparison pairs.
`runtime-v4.log` contains the final focused runtime run, and
`verified-input-sha256.json` binds the candidate sources and snapshot tools.
The `v1` comparison records the rejected ES2015 regression.

The user additionally requested pushes as work continues. The completed prior
batches were pushed to `origin/main` at `11aacafaa`; subsequent verified batches
are committed and pushed to `main` after their checks pass. Unrelated local work
remains excluded.

## ES5 async exception regions

The eleventh batch lowers supported async `try`/`catch`/`finally` statements
through the generator helper's exception regions and pending completion stack.
Rejected awaits enter the correct catch; finalizers can suspend and run through
normal completion, returns, throws, and labeled loop exits. A finalizer's return,
throw, rejection, break, or continue replaces the pending completion correctly.
Try statements without suspension retain native exception scopes.

Lifted catch variables receive stable file-wide names by lexical binding
identity. References, captured closures, shorthand properties, and same-named
`var` initializers retain their meaning, including collisions with imports,
source identifiers, and generated argument captures. Lexical analysis now
stops `arguments` resolution at normal functions and methods while allowing
arrows to resolve their enclosing binding. The additional catch-reference lookup
is scoped to generator emission that needs it; ordinary emission bypasses it.

Twelve new tests exercise runtime equivalence with native Node execution and
explicit expected results, plus atomic fallback checks. They cover nested
handlers and rethrows, awaited finalizers, completion replacement, mixed native
and suspended loop control, optional catch bindings, concurrent invocations,
catch shadowing, closures, shorthand properties, CommonJS imports, and the
separate `arguments` environments of functions, methods, and arrows.

The full workspace passed **3,057 tests**, with **0 failures and 37 ignored**,
including **690 emitter tests**. `make ci`, formatting, and `git diff --check`
passed. Rust tests use the same explicit TypeScript 6.0.3 library directory as
the preceding batch; baseline tools retain automatic TypeScript 5.9.3 discovery.

Cache-free comparisons against the tenth batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,434 | 270 | 497 | 1 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,377 | 722 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **one new complete expanded-JS pass**,
`es5-asyncFunctionTryStatements (target=es5)`, zero losses, and unchanged skips,
case identities, and oracles. Upstream cases and reference baselines remain
unchanged.

Captured catches inside suspended loops still require a separate environment
for each entry and retain the existing fallback. Destructured catches and
potential direct eval in lifted catch bodies also retain fallback. The existing
ES5 arrow-capture and unsupported generator-shape limitations remain. Default
diagnostics retain **5,991 failures**, expanded JS retains **992 failures**, and
expanded declarations retain **1,790 failures**, in separate potentially
overlapping lanes. Complete compatibility and the fastest-compiler goal remain
open; this batch makes no speed claim.

Evidence is in `/tmp/ts-rs-generator-exceptions-20260906/`. Final cache-free
manifests/comparisons and workspace/CI logs use suffix `v2`;
`verification-v2.log` records all eight successful report/comparison pairs.
`emitter-final.log` contains the final emitter suite, and
`verified-input-sha256.json` binds the candidate sources and snapshot tools.
`arguments-boundary-before.log` and `runtime-v3.log` preserve the failing
argument-boundary and shorthand-shadowing repros before their fixes.

## ES5 async switches

The twelfth batch lowers supported async switches into resumable dispatch and
body states. The discriminant is evaluated once before the case tests. Groups
of synchronous tests stay in native switches, and a match skips all later tests,
including their awaits and side effects. Case bodies retain source order and
fallthrough, with defaults allowed before, between, or after explicit cases.
Switches that suspend only in the discriminant retain native bodies.

Control scopes now distinguish switches from loops. Unlabeled break exits the
nearest switch or loop, continue searches outward for a loop, and labeled exits
retain their target through mixed native and suspended control flow. Exception
regions continue to run finalizers during those exits. A switch alone does not
require the per-iteration catch environment reserved for loops.

Nine new runtime tests compare emitted execution with native Node execution
and explicit expected results. They cover case-test evaluation order and
short-circuiting, empty-arm fallthrough, defaults in the middle, no-match and
empty switches, native and awaited discriminants, nested and labeled exits,
finalizers that suspend or replace completion, rejections, catch closures,
strict equality, object identity, and concurrent invocations.

The full workspace passed **3,066 tests**, with **0 failures and 37 ignored**,
including **699 emitter tests**. `make ci`, formatting, and `git diff --check`
passed. Library selections match the preceding batches.

Cache-free comparisons against the eleventh batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,435 | 269 | 497 | 1 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,377 | 722 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **one new complete expanded-JS pass**,
`es5-asyncFunctionSwitchStatements (target=es5)`, zero losses, and unchanged
skips, case identities, and oracles. Upstream cases and reference baselines
remain unchanged.

Unsupported statements and expressions inside switches retain the complete
async body's existing fallback, including the remaining lexical-declaration
and generator-shape boundaries. Default diagnostics retain **5,991 failures**,
expanded JS retains **991 failures**, and expanded declarations retain **1,790
failures**, in separate potentially overlapping lanes. Complete compatibility
and the fastest-compiler goal remain open; this batch makes no speed claim.

Evidence is in `/tmp/ts-rs-generator-switches-20260906/`. Cache-free
manifests/comparisons and workspace/CI logs use suffix `v1`;
`verification-v1.log` records all eight successful report/comparison pairs.
`runtime-v1.log` contains the focused runtime checks, `emitter-final.log` the
final emitter suite, and `verified-input-sha256.json` binds the candidate
sources and snapshot tools.

## Parser profiling and rejected experiments

A follow-up performance investigation started at
`bd1e877e0610d3ecfa8901d2378cf3a5b2f74ab0`. **No compiler change from these
experiments was retained.** Lower allocation or instruction counts alone did
not establish the elapsed-time improvement sought by the performance goal.

A current `cycles:u` profile of `typescript.js` parsing and destruction showed
10.45% of samples in expression destruction, 6.93% in left-hand-side expression
parsing, and 3.59% in `mi_free`. The first experiment stored a member-access
receiver inline inside its already-boxed member node, eliminating a second
allocation while keeping the outer expression size unchanged.

| Fixture | Allocations before | Allocations with inline receiver |
| --- | ---: | ---: |
| `typescript.js` | 880,025 | 819,398 |
| `parser.ts` | 51,045 | 47,494 |
| `renderer.ts` | 7,472 | 7,080 |
| `table.tsx` | 3,799 | 3,566 |
| `cal.com.tsx` | 114,568 | 108,382 |

All **12,450 source AST fingerprints matched**, including spans and diagnostics.
The initial UTF-8-only probe could not read seven UTF-16 inputs; they were
subsequently checked successfully after harness-equivalent decoding. That
follow-up also checked the UTF-8 BOM inputs, for 851 decoded files altogether.
Only scratch copies were decoded; upstream files were unchanged. The inline
candidate passed **3,066 Rust tests**, with **0 failures and 37 ignored**, plus
`make ci` and all eight cache-free baseline comparisons with unchanged results.
These were checks of the rejected candidate, not a newly accepted implementation.

Both timing rounds used eight alternating before/after pairs per fixture, with
40, 300, 1,000, 1,000, and 100 timed parses per process respectively. The `perf`
profile uses fat LTO and one codegen unit. Parser timing used mimalloc; the
allocation-count probe used the system allocator. The first round ran on CPU 4.
A two-second independent load sample selected CPU 9 for the repeat based on
its combined load with its SMT sibling. Both rounds ran on the same AMD Ryzen 9 5900X with Rust 1.98.0.

The following are median **paired percentage changes** in process-average
parse-and-destroy time from the CPU 9 round, not ratios of aggregate medians.
The intervals are exploratory paired-bootstrap intervals from only eight pairs.
Profiler rounding and shared-host activity limit their precision.

| Fixture | Inline receiver time change | Exploratory 95% interval |
| --- | ---: | ---: |
| `typescript.js` | +2.96% | -4.51% to +21.87% |
| `parser.ts` | -0.21% | -0.85% to +1.26% |
| `renderer.ts` | +1.22% | -2.38% to +4.76% |
| `table.tsx` | 0.00% | 0.00% to +5.00% |
| `cal.com.tsx` | +0.29% | -1.62% to +1.47% |

Instruction counts decreased by 0.90–1.35%, but elapsed results were mixed.
Counters cover complete profiler processes, including warmup and destruction.
Minimum parse-only time on `cal.com.tsx` also increased in both full rounds.
The allocation savings did not justify retaining the AST representation change
without a reliable timing benefit, so every implementation edit was reverted.

A second experiment checked token kind before reading newline metadata for
postfix updates and dot-property recovery. It reduced user instructions by
0.38–0.70% in the full CPU 9 comparison. Its paired process-average timing changes
were -2.05%, +1.05%, +1.19%, +4.76%, and -1.56% on the same five fixtures; every
exploratory interval included zero. This change was also reverted. Neither
experiment establishes a compiler speedup or performance leadership.

Evidence for the layout experiment is in `/tmp/ts-rs-parser-perf-20260906/`:
`profile.txt`, allocation logs, `corpus-ast-comparison-final.json`,
`confirm.json` and `quiet-confirm.json`,
`paired-summary.json`, validation logs/manifests, source/tool hashes, and
`rejected-inline-layout.patch`. The raw failed UTF-8-only probe results remain
available separately from the final decoded comparison. The token-check trial
is in `/tmp/ts-rs-parser-guards-20260906/`, with `confirm.json`,
`paired-summary.json`, provenance, and `rejected-token-guards.patch`.
Both directories contain the exact paired measurement scripts and executable
snapshots. The compiler sources remain exactly those of the preceding verified
switch batch; only this investigation record is committed.

## ES5 async for-in

The thirteenth implementation batch lowers supported async `for-in` loops into
resumable states. It evaluates the object once, snapshots its enumerable keys,
and checks each key's continued presence before assigning the iteration target.
Awaited receivers and computed target keys are evaluated anew for each visited
key. Loop exits, labeled control flow, and awaited finalizers retain their
existing control-scope handling. Loops without awaits retain native `for-in`
bodies, with function-scoped iteration variables hoisted.

Assignment references now preserve identifier spans so lexical binding lookup
can resolve renamed catch variables. This fixes both suspended assignments and
iteration targets inside catch blocks. Enumeration temporaries avoid source
identifier collisions and follow TypeScript's allocation and hoisting order.

Ten new tests cover enumeration order and inheritance, deleted and added keys,
single evaluation of the object, awaited assignment targets, nested labels and
finalizers, variable hoisting and concurrent invocations, catch bindings,
rejections and proxy enumeration errors, empty values and boxed strings,
native loop control flow, and atomic fallback for unsupported headers.
Runtime checks compare emitted execution with native Node and explicit expected
results; the fallback check verifies preservation of the complete async body.

The full workspace passed **3,076 tests**, with **0 failures and 37 ignored**,
including **709 emitter tests**. `make ci`, formatting, and `git diff --check`
passed. Workspace/CI tests use TypeScript 6.0.3 libraries through the explicit
environment override; baseline comparisons retain automatic TypeScript 5.9.3
library discovery, matching the preceding batches.

Cache-free comparisons against the twelfth implementation batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,436 | 268 | 497 | 1 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,377 | 722 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **one new complete expanded-JS pass**,
`es5-asyncFunctionForInStatements (target=es5)`, zero losses, and unchanged
skips, case identities, and oracles. Upstream cases and reference baselines
remain unchanged.

The snapshot and membership strategy matches TypeScript's ES5 transform; it
does not promise native-JavaScript equivalence for every primitive or proxy
enumeration. In particular, the membership check can throw for primitive
strings. Lexical, destructured, and initialized iteration declarations retain
the complete async body's existing fallback. Default diagnostics retain
**5,991 failures**, expanded JS retains **990 failures**, and expanded
declarations retain **1,790 failures**, in separate potentially overlapping
lanes. Complete compatibility and performance leadership remain open; this
batch makes no speed claim.

Evidence is in `/tmp/ts-rs-generator-for-in-20260906/`. Cache-free
manifests/comparisons and workspace/CI logs use suffix `v1`;
`verification-v1.log` records all eight successful report/comparison pairs.
`runtime-v1.log` contains the ten focused checks, `case-v1.log` the exact
upstream baseline match, and `verified-input-sha256.json` binds the candidate
sources and snapshot tools.

## ES5 array spreads and async array construction

The fourteenth implementation batch lowers array spreads in ordinary ES5
expressions and supported async state machines. Array segments use
`__spreadArray`, with dense literal spreads folded directly into their
surrounding segment. Spread holes become explicit undefined values, while
ordinary array elisions remain holes. With `downLevelIteration`, nonliteral
spread operands pass through the existing `__read` iterator helper.

Helper discovery reuses the lexical analyzer's traversal and its distinction
between value expressions and assignment targets. It excludes erased
declarations, visits shorthand assignment defaults, and preserves nested rest
targets. Inline helpers, suppressed helper declarations, CJS namespace imports,
and ESM named helper imports use the existing helper infrastructure.

Async lowering stages helper arguments and completed array prefixes before
later awaits. Spread operands are evaluated in source order, so an array
already spread is copied before a later suspension can mutate it. Synthetic
helper calls retain suspension discovery through their original operands and
use TypeScript's stable-helper invocation layout.

Eleven new tests cover sparse arrays, getters and evaluation order, mutation
across suspension, nested literals, iterator protocol selection and cleanup,
Unicode strings with iterator lowering, helper modes, erased declarations,
destructuring targets and defaults, rejections, and catch bindings. Runtime
checks use native Node comparisons where semantics agree, plus explicit
expected results for TypeScript's option-dependent iterator behavior.

One existing loop-capture test now enables `downLevelIteration` for the
`[...g()]` expression used to exhaust its generator. A TypeScript 6.0.3 oracle
probe confirmed that disabling the option produces no consumed values, while
enabling it produces the test's intended `0,1`. Its assertions against loop
helper extraction and for the `0,1` runtime result are unchanged. A new test
independently verifies indexed access with the option disabled and iterator
protocol use with it enabled.

Final validation passed **3,087 workspace tests**, with **0 failures and 37
ignored**, including **720 emitter tests**. `make ci`, formatting, and
`git diff --check` passed. Workspace/CI library selection remains TypeScript
6.0.3 through the explicit environment override; baseline comparisons retain
automatic TypeScript 5.9.3 library discovery.

Cache-free comparisons against the thirteenth implementation batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,438 | 266 | 497 | 2 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,377 | 722 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **two new complete expanded-JS passes**:
`es5-asyncFunctionArrayLiterals (target=es5)` and `typedArrays-es5 (target=es5)`.
There are zero losses and unchanged skips, case identities, and oracles.
Upstream cases and reference baselines remain unchanged.

The default ES5 transform follows TypeScript's array-like spread behavior;
general iterable and Unicode-string semantics require `downLevelIteration`.
Recovery files retain the existing fallback when the prepass cannot establish
helper requirements. Call and constructor argument spreads remain separate
unfinished transforms. Default diagnostics retain **5,991 failures**, expanded
JS retains **988 failures**, and expanded declarations retain **1,790 failures**
in separate potentially overlapping lanes. Full compatibility and performance
leadership remain open; this batch makes no speed claim.

Evidence is in `/tmp/ts-rs-array-spread-20260906/`. The accepted cache-free
manifests/comparisons and workspace/CI logs use suffix `v3`;
`verification-v3.log` records all eight successful report/comparison pairs.
`generator-consumption-oracle.log` records the test-configuration probe.
`verified-input-sha256.json` binds the final candidate sources, tests, and
snapshot tools. Earlier trial logs are retained separately from the accepted
validation.

## ES5 spread calls and async argument staging

The fifteenth implementation batch lowers supported ES5 spread calls through
`apply`. Complex receiver expressions are cached once, and computed method
names that suspend retain the receiver across that suspension. Bare calls
use an undefined receiver; parenthesized method references retain their
receiver, while detached references remain detached. Spread-form `eval` keeps
its indirect-eval behavior.

Argument packing shares the array-spread implementation with packing disabled
for argument lists. A single spread can pass its array-like operand directly
when `downLevelIteration` is disabled. With that option enabled, the existing
iterator helper consumes iterable arguments. Helper discovery reuses the
binding traversal, excludes erased code, and integrates with inline helpers,
helper suppression, and tslib imports.

The async planner rewrites spread calls before staging their expressions.
Suspension discovery follows synthetic call, member, assignment, and other
expression containers so that awaits remain visible after source spans are
removed. Callees, receivers, and completed argument prefixes are evaluated
before later argument awaits. Nested synchronous calls use the emitter's
existing generator-name reservation when allocating later temporaries; review
removed an unnecessary additional traversal for those calls.

Twelve new tests cover receiver and getter order, sparse arguments,
bare and detached calls, indirect eval, iterable arguments and Unicode strings,
helper modes, erased code, mutation across awaits, awaited callees and computed
keys, rejections and finalizers, concurrent invocations, computed method names,
and temporary scoping in nested functions. Runtime checks compare emitted
execution with native Node and explicit expected results. Existing assertions and upstream
fixtures are unchanged.

Final validation passed **3,099 workspace tests**, with **0 failures and 37
ignored**, including **732 emitter tests**. `make ci`, formatting, and
`git diff --check` passed. Library selections remain unchanged: TypeScript
6.0.3 through the explicit workspace/CI environment override, and automatic
TypeScript 5.9.3 discovery for baseline comparisons.

Cache-free comparisons against the fourteenth implementation batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,439 | 265 | 497 | 1 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,377 | 722 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **one new complete expanded-JS pass**,
`es5-asyncFunctionCallExpressions (target=es5)`, zero losses, and unchanged
skips, case identities, and oracles. Upstream cases and reference baselines
remain unchanged.

Constructor argument spreads, optional-chain calls, and super/private-call
spreads remain on their existing emission paths. Recovery files retain the
existing fallback when helper discovery cannot establish a supported plan.
Default diagnostics retain **5,991 failures**, expanded JS retains **987
failures**, and expanded declarations retain **1,790 failures**, in separate
potentially overlapping lanes. Full compatibility and performance leadership
remain open; this batch makes no speed claim.

Evidence is in `/tmp/ts-rs-call-spread-20260906/`. Accepted cache-free
manifests/comparisons, workspace/CI logs, and focused runtime checks use suffix
`v3`. `verification-v3.log` records all eight successful report/comparison
pairs, and `verified-input-sha256.json` binds the final sources, tests, and
snapshot tools. Earlier trial logs remain separate from accepted validation.

## Lazy indexing for spread-comment layout checks

The sixteenth implementation batch avoids repeated whole-span searches for
comments immediately following spread markers during normal emission. A lazy,
per-emitter textual index records possible `... /*` and `... //` locations.
Queries binary-search that index and skip the existing scan only when no match
is possible. The original scan remains authoritative for possible matches,
including overlapping dot runs and truncated comment boundaries. The index
intentionally includes marker text inside strings and comments, matching the
existing formatting behavior. Fast emission does not initialize the index.

Two regression tests compare both optimized predicates against an independent
span-local reference over every UTF-8 boundary subspan of generated inputs.
They cover long dot runs, horizontal and Unicode whitespace, block/line and
unclosed comments, strings, templates, out-of-bounds ends, and lazy behavior in
fast mode. No upstream case, reference baseline, or assertion is weakened.

A new `prof_emit` benchmark example measures emission alone or parsing plus
emission with the CLI's mimalloc allocator, nanosecond timing, three warmups,
and output destruction inside the timed loop. It fingerprints JavaScript,
source maps, and declarations outside timing and reports source/output sizes
and parse diagnostic counts. Its pipeline mode includes parsing and AST
destruction, but **does not bind or type-check**. Example invocation:

```bash
cargo run -p tsc_rs_bench --profile perf --example prof_emit -- \
  crates/tsc_rs_bench/fixtures/parser.ts 30 normal emit
```

Both benchmark binaries used Rust 1.98.0, the existing `perf` profile (optimized
with fat LTO and one codegen unit), and an AMD Ryzen 9 5900X. The before binary
contains the same profiler against `b29d8de8d1c6eb40bbd32f71ffe59c81b43161c3`.
A three-pair pilot was followed by two ten-pair rounds over thirteen
configurations. Before/after process order alternated in each pair. The pilot
and first round were pinned to CPU 5; a fresh CPU/sibling load sample selected
CPU 4 for the second round and follow-up controls. No repository builds or
tests ran alongside the measurements. Shared-host
activity still caused substantial spikes. Every sample is retained, and
fingerprints, sizes, and diagnostic counts matched in every pair.

Median paired wall-time changes in the second ten-pair round (negative is
faster), with exploratory paired-bootstrap 95% percentile intervals:

| Fixture, normal emission | Time change | Interval | Process instruction change |
| --- | ---: | ---: | ---: |
| UserSettings.tsx | -4.26% | -5.83% to -1.47% | -4.62% |
| parser.ts | -2.56% | -3.97% to -0.53% | -2.76% |
| renderer.ts | -4.66% | -6.60% to -3.21% | -4.70% |
| table.tsx | -3.71% | -7.16% to +21.52% | -5.03% |
| cal.com.tsx | -4.96% | -6.49% to -2.71% | -5.05% |
| typescript.js | -3.92% | -5.90% to -2.37% | -3.46% |

The pilot and first ten-pair round also showed negative median time changes
on all their normal-emission fixtures. Parse-and-emit medians in the second
round were -2.36% for parser.ts, -4.07% for cal.com.tsx, and -3.52% for
typescript.js. Parser source-map emission measured -2.41%, and ES5 emission
-3.13%. The table, ES5, and TypeScript pipeline intervals crossed zero, so
these individual timing gains remain uncertain. Instruction counters cover
the complete process, including initial parsing/emission and warmups; they
are separate from the timed-loop wall measurements.

Fast-mode controls do not establish an improvement. Parser fast emission
ranged from -0.03% to +1.49% across the first three rounds, including +1.43%
in the second ten-pair round. A further twenty-pair check with longer timed
loops measured +0.80% (-1.39% to +3.43%), with effectively unchanged
instruction counts. A small fast-mode slowdown therefore remains possible.
The TypeScript fast control measured +0.03% in the second ten-pair round.
A ten-pair check of the existing commentsAfterSpread.ts input measured -0.18%
(-2.73% to +0.81%) and +0.65% process instructions. The extra index work can
cost instructions when many spans still require the original scan.

These results support retaining the optimization for normal emission, with
the fast-mode and comment-heavy limitations above. They establish neither a
universal speedup nor a ranking against other compilers.

Final validation passed **3,101 workspace tests**, with **0 failures and 37
ignored**, including **732 emitter integration tests** and the two new unit
tests. `make ci`, formatting, `git diff --check`, and the profiler command
above passed. Workspace/CI use the explicit TypeScript 6.0.3 library override;
baseline tools retain automatic TypeScript 5.9.3 discovery.

All eight cache-free comparisons against the fifteenth implementation batch
passed with **zero losses, zero new passes, and unchanged skips, case
identities, and oracles**. This is a performance improvement; compatibility
counts are unchanged:

| Suite / lane | Passed | Failed | Skipped |
| --- | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 |
| Compiler, js | 6,032 | 0 | 497 |
| Compiler, js-expanded | 6,439 | 265 | 497 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 |
| Conformance, errors | 2,796 | 3,111 | 0 |
| Conformance, js | 5,388 | 0 | 519 |
| Conformance, js-expanded | 6,377 | 722 | 519 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 |

Full compatibility remains open: **5,991 diagnostic failures**, **987 expanded
JS failures**, and **1,790 expanded declaration failures** remain in separate,
potentially overlapping lanes. Performance leadership also remains unproven.

Evidence is in `/tmp/ts-rs-emitter-perf-20260906/`. `measure.py`,
`measure-fast-control.py`, `measure-comment-heavy.py`, raw JSON/log files,
and `summary.json` retain all paired samples and the deterministic 20,000-draw
bootstrap summaries. `benchmark-provenance.json` records the base SHA,
toolchain, CPU, source diff, new sources, and fixture hashes. The profiler
snapshots and all candidate sources are bound by
`verified-input-sha256.json`, which also binds the baseline snapshot tools.
The cache-free manifests, comparison logs, workspace log, and CI log use
suffix `v3`; `verification-v3.log` records all eight successful comparisons.

## ES5 constructor spreads and async construction

The seventeenth implementation batch lowers ES5 constructor spreads through a
bound constructor and the existing argument-spread helpers. Nontrivial
constructors, including property getters, computed references, and
parenthesized expressions, are evaluated once. The packed arguments begin
with the bind receiver; a lone constructor spread still requires packing.
Dense literal spreads avoid unnecessary helpers. Calls and constructors share
the existing invocation metadata map, keeping helper discovery in the binding
walk without adding another per-emitter map.

The async planner now handles `new` expressions with awaited constructors,
receivers, computed keys, and arguments. Spread construction is rewritten
before suspension planning, so completed argument prefixes and constructor
identity survive later awaits. Non-spread construction with suspending
arguments also captures the constructor, including identifier callees whose
bindings can change while suspended. Its synthetic undefined bind receiver
can remain unevaluated until the first real argument resumes, avoiding an
otherwise unnecessary `[void 0]` temporary array and concatenation.

Synthetic state and catch-binding resolution now traverses constructor
callees and arguments. This prevents unresolved internal names in emitted
construction expressions and preserves shadowed catch constructors.

Fourteen new tests cover constructor getters and evaluation order, sparse
arguments, returned objects and functions, bound and native constructors,
private and optional constructor references, helper suppression/imports,
dense spreads, iterable and array-like options, awaited constructors and
computed keys, mutation across suspension, nested construction, concurrent
invocations, rejections and finalizers, iterator cleanup, catch bindings,
temporary scopes, erased code, modern targets, and multiple argument awaits.
Runtime tests compare with native Node where semantics agree and use explicit
expectations for TypeScript's option-dependent iteration and cleanup behavior.

One baseline limitation is deliberate: non-spread construction with a
suspending argument retains an identifier constructor rather than re-reading
its binding after the await. A local TypeScript 6.0.3 transpilation probe
produced `[false,99]` after an awaited callback reassigned the constructor,
while native JavaScript produced `[true,1]`. The new runtime test preserves
the original constructor. Existing baseline oracles remain unchanged and
continue to report unmatched output as failures; the async new-expression
case therefore remains open rather than counting this difference as a pass.


Final validation passed **3,115 workspace tests**, with **0 failures and 37
ignored**, including **746 emitter integration tests**. `make ci`, formatting,
and `git diff --check` passed. Workspace/CI retain the explicit TypeScript
6.0.3 library override; baseline tools retain automatic TypeScript 5.9.3
discovery.

Cache-free comparisons against the sixteenth implementation batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,439 | 265 | 497 | 0 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,378 | 721 | 519 | 1 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **one new complete expanded-JS pass**,
`newWithSpreadES5 (target=es5)`, zero losses, and unchanged skips, case
identities, and oracles. Upstream test cases and reference baselines remain
unchanged.

Full compatibility remains open: **5,991 diagnostic failures**, **986 expanded
JS failures**, and **1,790 expanded declaration failures** remain in separate,
potentially overlapping lanes. This batch makes no compiler speed claim;
performance leadership remains unproven.

Evidence is in `/tmp/ts-rs-new-spread-20260906/`. Final cache-free manifests,
comparison logs, workspace/CI logs, and focused runtime checks use suffix
`v5`. `verification-v5.log` records all eight successful comparisons;
`verified-input-sha256.json` binds the final sources, tests, and snapshot tools.
The constructor-identity oracle script, emitted output, and log record the
local TypeScript probe. Earlier trial logs are retained separately from final
validation.

## Resumable async object literals

The eighteenth implementation batch builds object literals across ES5 async
suspension points. It retains the initial property prefix as an object literal,
then stages later property values and computed-key values in source order.
Pending definitions execute before a following property suspends, while the
completed object is returned only after every definition succeeds. Computed
key caches use the object transform's temporary order and declaration group.

Ordinary and async methods retain their function scopes, parameters, `this`,
and `arguments`. Accessor-bearing objects use property descriptors so getters
and setters merge and later data properties replace accessors. Object spreads
reuse the emitter's existing `Object.assign` strategy. Suspension discovery
now includes computed method/accessor names, and synthetic binding resolution
traverses object property values and computed names without entering method
bodies.

Generated object sequences retain a zero-width source origin for layout;
they cannot copy their original await expressions through source preservation.
Continuation indentation accounts for inline generator labels. Synthetic
string literals without a source span now emit their escaped contents, making
generated descriptor keys valid JavaScript while quoted source keys retain
their original spelling.

Eleven new runtime tests cover values across multiple awaits, computed keys,
nested literals and concurrent invocations, accessor merging and replacement,
methods and nested async functions, spreads with getters, rejected values and
finalizers, catch bindings and shorthand properties, quoted/numeric keys,
symbols, escaped keys, computed accessor names, conditional branches, and
constructor arguments. Their emitted runtime results match native Node on
these inputs. Upstream test cases and reference baselines are unchanged.


Final validation passed **3,126 workspace tests**, with **0 failures and 37
ignored**, including **757 emitter integration tests**. `make ci`, formatting,
and `git diff --check` passed. Workspace/CI use the explicit TypeScript 6.0.3
library override; baseline tools retain automatic TypeScript 5.9.3 discovery.

Cache-free comparisons against the seventeenth implementation batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,441 | 263 | 497 | 2 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,378 | 721 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **two new complete expanded-JS passes**:
`es5-asyncFunctionObjectLiterals (target=es5)` and
`es5-asyncFunctionLongObjectLiteral (target=es5)`. There are zero losses and
unchanged skips, case identities, and oracles.

Full compatibility remains open: **5,991 diagnostic failures**, **984 expanded
JS failures**, and **1,790 expanded declaration failures** remain in separate,
potentially overlapping lanes. Existing unsupported async shapes keep their
atomic fallback. This batch makes no compiler speed claim; performance
leadership remains unproven.

Evidence is in `/tmp/ts-rs-async-objects-20260906/`. Final cache-free manifests,
comparison logs, workspace/CI logs, baseline-case probes, and runtime tests use
suffix `v3`. `verification-v3.log` records all eight successful comparisons,
and `verified-input-sha256.json` binds the final sources, tests, and snapshot
tools. Earlier trial logs remain separate from final validation.

## Standalone and labeled blocks in ES5 async bodies

The nineteenth implementation batch preserves standalone blocks that do not
suspend in the async operation plan. Nested returns and native labeled breaks
retain their block structure. Blocks that form the body of an already lowered
loop or conditional are flattened at that structural boundary, while further
standalone blocks inside those bodies remain intact.

Labeled blocks with suspension now have explicit exit targets. Their breaks
can cross awaits and suspending finalizers. A label-only control scope is
excluded from unlabeled break/continue lookup, so enclosing loops and switches
retain their control semantics. Jump resolution and synthetic/catch binding
resolution traverse the new structured block operations.

Seven runtime tests compare emitted execution with native Node for nested
returns before/after await, multiple labels, breaks across suspension,
unlabeled loop control through labeled blocks, switch breaks and outer-loop
continues, suspending finalizers, catch redeclarations, and native finalizers.
Upstream test cases and reference baselines remain unchanged.


Final validation passed **3,133 workspace tests**, with **0 failures and 37
ignored**, including **764 emitter integration tests**. `make ci`, formatting,
and `git diff --check` passed. Workspace/CI retain the explicit TypeScript
6.0.3 library override; baseline tools retain automatic TypeScript 5.9.3
discovery.

Cache-free comparisons against the eighteenth implementation batch:

| Suite / lane | Passed | Failed | Skipped | New passes | Losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Compiler, errors | 3,649 | 2,880 | 0 | 0 | 0 |
| Compiler, js | 6,032 | 0 | 497 | 0 | 0 |
| Compiler, js-expanded | 6,442 | 262 | 497 | 1 | 0 |
| Compiler, declarations-expanded | 5,730 | 974 | 497 | 0 | 0 |
| Conformance, errors | 2,796 | 3,111 | 0 | 0 | 0 |
| Conformance, js | 5,388 | 0 | 519 | 0 | 0 |
| Conformance, js-expanded | 6,378 | 721 | 519 | 0 | 0 |
| Conformance, declarations-expanded | 6,283 | 816 | 519 | 0 | 0 |

All eight comparisons passed with **one new complete expanded-JS pass**,
`es5-asyncFunctionReturnStatements (target=es5)`, zero losses, and unchanged
skips, case identities, and oracles. The initial trial caught six regressions
from retaining structural control-body braces; the final version restores
all six and retains standalone blocks at their proper positions.

Full compatibility remains open: **5,991 diagnostic failures**, **983 expanded
JS failures**, and **1,790 expanded declaration failures** remain in separate,
potentially overlapping lanes. Unsupported async shapes retain the existing
atomic fallback. This batch makes no compiler speed claim; performance
leadership remains unproven.

Evidence is in `/tmp/ts-rs-async-blocks-20260906/`. Accepted cache-free
manifests, comparison logs, focused baseline/runtime checks, and workspace/CI
logs use suffix `v2`. `verification-v2.log` records all eight successful
comparisons; `verified-input-sha256.json` binds the final sources, tests, and
snapshot tools. The rejected `v1` evidence remains separate from final
validation.

## Rejected streaming recovery-check performance trial

A follow-up performance trial replaced the source-emission variable-recovery
check's allocated line vector with a streaming predicate. It could stop at an
already terminated first line or the first incompatible continuation line.
An exhaustive comparison over **33,930** generated line/comment/whitespace
combinations matched the original predicate. The trial's unit test passed,
and all benchmark output fingerprints, sizes, and diagnostic counts matched
within each before/after pair.

The before binary was built against
`34f3c516c9f3c000b3b08250ef788902313e39cb`, using the existing `prof_emit`
example, Rust 1.98.0, mimalloc, the optimized `perf` profile with fat LTO, and
an AMD Ryzen 9 5900X. A current CPU profile placed source-line emission at
10.56% self samples and the brace/unified normalizers at 5.15%/4.82%; these
are whole-function costs, not costs attributed solely to recovery checking.

The experiment retained **220 process measurements**: a three-pair pilot over
ten configurations, followed by ten pairs over six normal-emission fixtures
and two fast-mode controls. Each pair alternated process order. CPU/sibling
load sampling selected CPU 1 for the pilot and CPU 7 for confirmation. No
repository builds or tests ran concurrently with timing. Shared-host spikes
remain in the recorded data. Timings cover emission or parse-and-emit,
excluding binding and type checking; hardware counters cover entire processes.

Confirmation medians and exploratory paired-bootstrap 95% percentile intervals
(negative means faster), with 20,000 deterministic resamples:

| Fixture / mode | Paired time change | Interval | Process instruction change |
| --- | ---: | ---: | ---: |
| UserSettings.tsx, normal | -0.53% | -1.32% to +0.04% | -0.25% |
| parser.ts, normal | +0.02% | -0.82% to +0.73% | -0.05% |
| renderer.ts, normal | -0.17% | -1.17% to +1.48% | -0.03% |
| table.tsx, normal | -0.07% | -0.69% to +1.29% | -0.14% |
| cal.com.tsx, normal | +0.12% | -1.96% to +4.63% | -0.14% |
| typescript.js, normal | -4.25% | -15.67% to +6.90% | -0.06% |
| parser.ts, fast | -0.13% | -12.27% to +3.46% | -0.04% |
| typescript.js, fast | +1.33% | -3.53% to +6.27% | -0.19% |

Every confirmation timing interval crossed zero. The pilot's larger apparent
gains did not repeat, and whole-process median instruction savings reached only
about 0.25%. The trial is **rejected**: neither its implementation nor its temporary
regression test is retained. The compiler source was restored to the exact
verified HEAD blob. No new full compatibility gate was needed for this
documentation-only result; the nineteenth batch's counts remain current.

Evidence is in `/tmp/ts-rs-recovery-perf-20260906/` (the experiment began before
UTC midnight). `candidate.patch` and `candidate-emit-stmt.rs` preserve the
rejected code, `benchmark-input-sha256.json` binds the measured sources and
binaries, and `benchmark-provenance.json` records the toolchain, CPU, fixtures,
and base SHA. Measurement scripts, raw JSON/log files, CPU samples, profile,
`summary.json`, and `decision.json` retain the reproducible evidence and
rejection decision. The normalizers' per-call state allocations are a separate
candidate for subsequent measurement.

## Twentieth implementation batch: remove unused normalizer state

`LineState` allocated a root ternary counter on every construction, pushed and
popped counters for parentheses/brackets, and incremented counters at question
marks. No shared normalizer read those counters to make a formatting decision.
Its for-header vector also remained empty: no caller ever populated it.
Removing both fields and their updates leaves the separate, output-relevant
ternary and for-header tracking in `normalize_brace_spacing` intact.

The retained change adds three lines and removes 31. Existing assertions remain
unchanged. All 7,740 generated inputs matched the original brace, unified, and
combined normalizers, including nesting to 128 levels, templates, comments,
regexes, ternaries, and malformed leading closers. A separate instrumented
allocator check of the combined chain found:

| Input shape | Before allocations / reallocations | After allocations / reallocations |
| --- | ---: | ---: |
| Simple variable initializer | 4 / 0 | 3 / 0 |
| Conditional with a call | 4 / 2 | 3 / 1 |
| Object containing an array and conditional call | 7 / 5 | 6 / 4 |
| Template interpolation containing a conditional call | 6 / 2 | 5 / 1 |

These are four direct normalizer-chain probes using an instrumented system
allocator, not whole-compiler allocation counts. Paired timing binaries use
mimalloc and the optimized `perf` profile with fat LTO, Rust 1.98.0, on an AMD
Ryzen 9 5900X. The before binary is the previously measured binary from
`34f3c516c9f3c000b3b08250ef788902313e39cb`; a clean Git comparison confirms
that `crates`, `Cargo.toml`, and `Cargo.lock` did not change between that commit
and the current base `cb8018f105a21bed237771a6c6b2a8a52be72510`.

The retained candidate has **440 process measurements**: three pilot pairs
across ten configurations, ten confirmation pairs across thirteen
configurations, then twenty longer pairs on three configurations to recheck
the small-fixture result and controls. Each pair alternated process order.
CPU/sibling load sampling selected CPU 3, 8, and 7 respectively. No repository
builds or tests ran during timings; all shared-host noise/outliers remain in
the data. Output fingerprints, sizes, and diagnostic counts matched in every
pair. The timed operations exclude binding/type checking, and instruction
counters cover entire processes.

Confirmation medians and exploratory paired-bootstrap 95% percentile intervals
(20,000 deterministic resamples; negative means faster):

| Fixture / mode / phase | Paired time change | Interval | Process instruction change |
| --- | ---: | ---: | ---: |
| UserSettings.tsx, normal, emit | +1.74% | +0.30% to +3.62% | -0.76% |
| parser.ts, normal, emit | -1.27% | -2.47% to +1.48% | -0.46% |
| renderer.ts, normal, emit | -1.16% | -1.89% to -0.58% | -0.58% |
| table.tsx, normal, emit | -0.79% | -1.83% to -0.27% | -0.79% |
| cal.com.tsx, normal, emit | -2.17% | -4.09% to +2.70% | -0.67% |
| typescript.js, normal, emit | -0.90% | -6.79% to +0.12% | -0.48% |
| parser.ts, normal, pipeline | -0.56% | -1.35% to +0.55% | -0.43% |
| parser.ts, maps, emit | -1.59% | -2.38% to -0.65% | -0.45% |
| parser.ts, es5, emit | -1.73% | -2.69% to -1.34% | -0.20% |
| parser.ts, fast, emit | +0.61% | -0.40% to +1.24% | +0.01% |
| typescript.js, fast, emit | +0.04% | -3.15% to +1.42% | +0.01% |
| cal.com.tsx, normal, pipeline | -1.08% | -3.31% to +8.46% | -0.60% |
| typescript.js, normal, pipeline | -2.24% | -3.34% to +1.22% | -0.46% |

The twenty-pair recheck used 5,000 iterations per process for UserSettings,
200 for renderer, and 100 for parser fast mode:

| Fixture / mode | Paired time change | Interval | Process instruction change |
| --- | ---: | ---: | ---: |
| UserSettings.tsx, normal | +0.17% | -0.38% to +1.44% | -0.76% |
| renderer.ts, normal | -1.24% | -1.48% to -1.05% | -0.58% |
| parser.ts, fast | +0.51% | +0.17% to +1.60% | +0.01% |

The initial UserSettings slowdown did not repeat convincingly; renderer's
improvement did repeat. Parser fast mode has a small measured cost, about
**0.5%** in the recheck, despite essentially unchanged process instructions.
This tradeoff remains explicit: retain the simpler implementation for its
proven allocation reduction and repeated renderer elapsed gain, without
claiming every workload is faster. Broad pipeline gains remain uncertain.

Validation: all **3,133 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. Workspace/CI used the installed TypeScript 6.0.3 library
override, while the baseline tools retained automatic TypeScript 5.9.3 library
discovery. Eight cache-free comparisons against the nineteenth batch all
reported zero gains, zero losses, and no skip changes. The candidate retained
all case/oracle identities. This performance change does not improve baseline
pass counts:

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,649 / 2,880 / 0 | 2,796 / 3,111 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

The goal remains open: 5,991 diagnostic failures, 983 expanded-JavaScript
failures, and 1,790 expanded-declaration failures remain in separate,
potentially overlapping lanes. Compiler performance leadership is unproven.

Evidence is in `/tmp/ts-rs-normalizer-state-perf-20260907/`: raw paired
measurements and scripts, allocation instrumentation/results, differential
checks against the saved original normalizers, benchmark provenance and
hashes, eight cache-free manifests/comparisons, and workspace/CI logs.
`verified-input-sha256.json` and `benchmark-input-sha256.json` were rechecked
after verification. All eight candidate manifests are identical to their
accepted base manifests, including individual records.

A preceding inline-stack implementation was **rejected**. It matched 7,740
generated normalization inputs and passed a vector-equivalence test covering
repeated spilling, mutation, cloning, and reuse through depth 128. Its 320
paired process measurements reduced instructions modestly, but every
confirmation timing interval crossed zero. The implementation and temporary
test were removed before the retained candidate was built. Raw measurements,
the candidate patch/source, CPU selection, and rejection decision are preserved
separately in `/tmp/ts-rs-normalizer-stack-perf-20260907/`; that trial's data is
not mixed into the retained candidate's results.

## Twenty-first implementation batch: reserved type declaration names

Type-bearing declarations now reject reserved built-in type names through their
existing checker paths: TS2414 for classes (including class expressions),
TS2427 for interfaces, TS2457 for aliases, TS2431 for enums, and TS2368 for
generic, mapped, and inferred type parameters. The check decodes escaped
identifier spellings and reports only the name span, excluding type-parameter
modifiers, comments, constraints, and defaults. Repeated contextual checking
does not duplicate the same diagnostic.

Mapped-type lookahead now recognizes contextual keyword tokens, matching the
identifier parser. Previously `{ [object in string]: 1 }` was misparsed as a
computed property, preventing type-parameter checking. Legal contextual
parameter names such as `intrinsic` remain valid. Existing formatting and
reference oracles were preserved.

Six new Rust tests cover fifty declaration/name combinations, allowed value and
contextual names, nested/exported/ambient declarations and class expressions,
eleven generic-signature/mapped/infer occurrences, modifiers and Unicode
escapes, and mapped-type parsing with eight contextual tokens. Local
TypeScript 6.0.3 source and diagnostic probes informed the rule and spans;
repository baselines remain the acceptance oracle.

The cache-free diagnostic comparisons gained **14 complete passes**: nine
compiler and five conformance cases. Compiler gains are `ClassDeclaration24`,
`InterfaceDeclaration8`, `enumWithPrimitiveName`, `primitiveTypeAsClassName`,
`primitiveTypeAsInterfaceName`, `primitiveTypeAsInterfaceNameGeneric`,
`typeNamedUndefined1`, `typeNamedUndefined2`, and `undefinedTypeAssignment1`.
Conformance gains are `classWithPredefinedTypesAsNames`, `symbolType20`,
`parserClassDeclaration24`, `parserInterfaceDeclaration8`, and
`objectTypesWithPredefinedTypesAsName`.

All eight compiler/conformance matrices have zero losses and no skip changes,
with identical case and oracle identities. JavaScript and declaration pass
counts are unchanged. The resulting totals are:

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,658 / 2,871 / 0 | 2,801 / 3,106 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

All **3,139 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. Workspace/CI used the installed TypeScript 6.0.3 library
override; baseline reports retained automatic TypeScript 5.9.3 discovery.
Final source/test/tool hashes were rechecked after the gates.

The goal remains open: **5,977 diagnostic failures**, **983 expanded-JavaScript
failures**, and **1,790 expanded-declaration failures** remain in separate,
potentially overlapping lanes. Invalid-keyword recovery such as `type void`
and the target-resolution-dependent TS2438 import-alias rule need separate
follow-up work. This batch makes no speed claim.

Evidence is in `/tmp/ts-rs-reserved-types-20260907/`. Final focused checks,
snapshot tools, eight cache-free manifests/comparisons, and workspace/CI logs
use suffix `v4`; `verification-v4.log` records all successful gates.
`verified-input-sha256-v4.json` binds the final source/test inputs and tools.
Earlier build/test attempts remain separate from final verification.

## Twenty-second implementation batch: declaration-name error recovery

Expression-statement recovery now reports the declaration-specific invalid or
missing name after `interface`, `type`, `namespace`, and `module` when the next
token cannot be separated by automatic semicolon insertion. It preserves the
following tokens for ordinary expression recovery. This matches cases such as
`interface void {}` and `type void = I;`, where the keyword is an expression,
not a successfully parsed declaration. The generic expression fallback now
uses TS1109's canonical `Expression expected.` message.

The checker no longer assumes that unbound `type` or `namespace` expressions
are globals, or that the spelling `interface` is intrinsically a type. Real
value bindings still resolve normally, and an actual interface named
`interface` still produces TS2693 when used as a value. Seven new Rust tests
cover name recovery, exact diagnostic spans/messages, following references,
value/type bindings, namespace recovery, and semicolon/line-break boundaries.
Local TypeScript 6.0.3 diagnostic probes informed these rules; existing
repository oracles remain unchanged.

The eight cache-free comparisons against the twenty-first batch gained
**12 complete conformance diagnostic passes**, with zero losses and no new
skips in any matrix. The gains are `classExtendingPrimitive2`,
`logicalNotOperatorInvalidOperations`, `plusOperatorInvalidOperations`,
`interfacesWithPredefinedTypesAsNames`, `parserErrorRecovery_LeftShift1`,
`parserGreaterThanTokenAmbiguity12`, `parserGreaterThanTokenAmbiguity13`,
`parserGreaterThanTokenAmbiguity14`, `parserGreaterThanTokenAmbiguity17`,
`parserGreaterThanTokenAmbiguity18`, `parserGreaterThanTokenAmbiguity19`, and
`reservedNamesInAliases`.

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,658 / 2,871 / 0 | 2,813 / 3,094 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

All **3,146 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. Workspace/CI used the installed TypeScript 6.0.3 library
override; baseline tools retained automatic TypeScript 5.9.3 discovery.
Final source/test/tool hashes, case/oracle identities, and skip counts were
rechecked after verification.

The goal remains open: **5,965 diagnostic failures**, **983 expanded-JavaScript
failures**, and **1,790 expanded-declaration failures** remain in separate,
potentially overlapping lanes. This batch makes no speed claim. Import-alias
name diagnostics still require their separate target-resolution rule.

Evidence is in `/tmp/ts-rs-declaration-recovery-20260907/`. Final checker tests,
tool snapshots, eight cache-free manifests/comparisons, and workspace/CI logs
use suffix `v3`; the unchanged focused parser test passed in `parser-v2.log`.
`verification-v3.log` records all eight successful comparisons, and
`verified-input-sha256-v3.json` binds final source/test inputs and tools.

## Twenty-third implementation batch: parser messages and missing delimiters

Parser diagnostics now use source token spellings and TypeScript wording instead
of internal enum/debug descriptions. `TokenKind::fixed_text` covers all 143
fixed keyword/punctuation spellings; literals, identifiers, trivia, and EOF
remain explicitly without a fixed spelling. Missing-token messages and the
identifier/class-member/type-member/declaration/string-literal error paths now
report their canonical messages.

Missing closing delimiters in blocks, array/object literals, and supported
statement headers carry TS1007 related information pointing to the actual
opening token. Nested missing delimiters retain the first diagnostic and its
innermost opening token. Mismatched array closers remain available to the
surrounding expression, while the missing `]` is reported. Missing identifiers
at EOF are anchored before trailing trivia, matching TypeScript's insertion
point. Same-line literals after variable declarators report the missing comma
without being consumed as binding names.

The error-baseline formatter now preserves zero-length spans instead of drawing
an invented one-character squiggle. Tests separately prove that a zero-width
diagnostic and its message remain visible and that nonempty diagnostics retain
their squiggles. No comparison assertion, skip rule, source test case, or
reference baseline was relaxed or replaced.

Six new Rust tests cover punctuation spellings and nonfixed tokens, opening
locations and nested missing delimiters, EOF trivia positions, expected-token
messages, and zero/nonzero diagnostic markers. The existing exhaustive keyword
lookup test additionally checks reverse spellings. Error construction for
matching delimiters stays in a cold function, and valid opening/closing tokens
are consumed with one check. This structure makes no measured speed claim.

Eight cache-free comparisons gained **21 complete diagnostic passes**: nine
compiler and twelve conformance cases, with zero losses and no skip changes.
The final `v5` manifests are identical to the earlier `v4` candidate's manifests,
confirming that the cold-path refactor preserved all recorded outcomes.

Compiler gains are `enumWithParenthesizedInitializer1`, `exportInFunction`,
`incompleteDottedExpressionAtEOF`, `missingCloseBrace`,
`missingCloseBraceInObjectLiteral`, `missingCloseBracketInArray`, `parse1`,
`taggedTemplatesWithIncompleteTemplateExpressions1`, and `validRegexp`.
Conformance gains are `classWithPredefinedTypesAsNames2`, `decoratorOnAwait`,
`invalidTaggedTemplateEscapeSequences`, `binaryIntegerLiteralError`,
`octalIntegerLiteralError`, `parserAccessibilityAfterStatic6`,
`parserErrorRecovery_ParameterList2`, `parser512084`, `TupleType4`,
`scannerS7.8.3_A6.1_T1`, `scannerS7.8.4_A7.1_T4`, and
`objectTypesWithPredefinedTypesAsName2`.

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,667 / 2,862 / 0 | 2,825 / 3,082 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

All **3,152 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. Workspace/CI used the installed TypeScript 6.0.3 library
override; baseline tools retained automatic TypeScript 5.9.3 discovery.
Final source/test/tool hashes, case/oracle identities, and skip counts were
rechecked after verification.

The goal remains open: **5,944 diagnostic failures**, **983 expanded-JavaScript
failures**, and **1,790 expanded-declaration failures** remain in separate,
potentially overlapping lanes. Compiler performance leadership is unproven.

Evidence is in `/tmp/ts-rs-parser-messages-20260907/`. Final snapshot tools,
eight cache-free manifests/comparisons, and workspace/CI logs use suffix `v5`;
`verification-v5.log` records all successful gates and
`verified-input-sha256-v5.json` binds the source/test inputs and tools. The
focused scanner, parser, and formatter logs and intermediate snapshots remain
separate from final validation.


## Twenty-fourth implementation batch: missing types and recovery boundaries

Missing type annotations now report TS1110 (`Type expected.`) and insert a
zero-width type node without consuming the following token. Parameter lists,
tuples, type arguments, object types, and declarations retain their delimiters
and recover without the errors caused by losing those delimiters. Qualified
type names and JSX name recovery use TS1003; missing object-property colons and
JSX tag terminators use TS1005 with source token spellings. The type-member and
object-member recovery guards use TS1131 and TS1136 respectively.

Type references accept keyword names, including `const` in angle-bracket const
assertions, and JSDoc's `*` type is parsed without treating it as a missing type.
The parser distinguishes array suffixes from indexed accesses by checking
whether an index type can start. Private names left by invalid index recovery
remain available as recovered variable bindings. An unmatched closing delimiter
followed by `=` recovers the right-hand expression as a statement; its span
includes the consumed semicolon so trailing comments remain attached.

The first broad recovery candidate exposed six JavaScript regressions. Fixing
the keyword/JSDoc parsing paths and the private-binding, unmatched-delimiter,
and statement-span recovery paths restored all six. Eight focused parser tests
cover diagnostic codes, messages, locations, retained delimiters, accepted type
forms, recovered bindings, and expression-statement spans. TypeScript 5.9.3
probes confirm the recorded diagnostic expectations and agree with the earlier
6.0.3 probes.

The final eight cache-free comparisons against `272950122` gain **six complete
conformance diagnostic passes**, with zero passed-case losses and unchanged
skips, case identities, and oracles: `ArrowFunction1`, `parserX_ArrowFunction1`,
`TupleType6`, `parserComputedPropertyName1`, `parserShorthandPropertyAssignment3`,
and `parserShorthandPropertyAssignment4`.

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,667 / 2,862 / 0 | 2,831 / 3,076 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

All **3,160 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. Workspace/CI used the installed TypeScript 6.0.3 library
override; baseline tools used automatic TypeScript 5.9.3 discovery. Final
source/test/tool hashes and case/oracle identities were rechecked. No benchmark
speedup is claimed for this correctness batch. The goal remains open, with
**5,938 diagnostic**, **983 expanded-JavaScript**, and **1,790 expanded-declaration**
failures in separate, potentially overlapping lanes.

Evidence is in `/tmp/ts-rs-type-recovery-20260907/`. Final snapshot tools,
eight manifests/comparisons, parser/workspace/CI logs, verification script,
and input hashes use suffix `v9`. `manifest-audit-v9.txt` records totals and
gains; `typescript-5.9.3-probes.json` records the reference parser probes.
Intermediate failing candidates remain distinct from the final verification.


## Rejected primitive-type allocation trials and precise parser measurements

The parser benchmark now emits machine-readable nanosecond timings and complete
AST/diagnostic fingerprints on stdout while retaining its human-readable timing
summary on stderr. Zero iterations are rejected. Fingerprinting happens after
the timed work, so its temporary allocations cannot perturb the measured
parses. The retained change is confined to the benchmark example and this record;
compiler source is restored byte-for-byte to `b9298888d`.

Three candidates targeted temporary strings in ten primitive-type parsing
paths: borrowing token text, deferring the text read until a predicate appears,
and adding an inline predicate check with a separate cold parsing helper.
Instrumented `System` allocation counts decreased for each candidate. The parser
fixture used 1,047 fewer allocations per parse (51,044 to 49,997), the calendar
fixture used 937 fewer (114,559 to 113,622), and a generated 10,000-annotation
stress fixture used 10,000 fewer (15,019 to 5,019). The plain-JavaScript control
was unchanged at 879,996. These are counting-allocator observations; timings
used the CLI's mimalloc allocator.

Measurements used Rust 1.98.0, the `perf` profile (optimization level 3, fat LTO,
one codegen unit), a Ryzen 9 5900X, CPU affinity selected from sampled load,
and alternating before/after order. The six local benchmark fixtures were supplemented
by the generated type-heavy stress case. Parse+emit measurements do not include
binding or type checking. Hardware counters cover each complete process;
reported parse times cover the timed loop, including AST destruction.

There were **660 process measurements in 330 complete pairs**: 72 for the direct
borrow pilot, 324 for its stopped confirmation, 72 for each of the two later
pilots, and 120 for a final recheck. All paired AST/diagnostic fingerprints or
emission hashes matched, and fixture hashes were checked. Twenty additional
keyword-predicate probes, including escaped spellings, matched the original
parser's AST/diagnostic fingerprints for the deferred and cold variants.

The first harness version fingerprinted before warmup. Its 20-pair direct-borrow
confirmation showed the parser fixture **2.29% slower** (95% paired-bootstrap
interval **+1.69% to +2.85%**) and the calendar fixture **1.52% slower**
(**+0.34% to +2.64%**); remaining pipeline measurements were stopped after these
regressions were established. The deferred and cold candidates were exploratory
three-pair pilots and were not promoted to full confirmation.

Moving fingerprinting after timing preserved the original warmup conditions.
The final direct-borrow recheck used 20 pairs per workload:

| Parse workload | Median paired time change | 95% bootstrap interval |
| --- | ---: | ---: |
| TypeScript parser fixture | +0.93% | +0.22% to +1.79% |
| Calendar fixture | -0.75% | -3.19% to -0.03% |
| Type-heavy stress fixture | -2.18% | -3.40% to -1.38% |

Positive values mean slower. The repeated parser-fixture slowdown ruled out
accepting the direct-borrow change. None of the three compiler candidates was
retained. These trials establish allocation savings and workload-dependent
timing tradeoffs, not a compiler speedup or performance ranking.

The retained benchmark built successfully with the `perf` profile. Smoke checks
validated its JSON output, preserved stderr summary, and zero-iteration
rejection; `make ci` passed with the TypeScript 6.0.3 library override. Compiler
source, upstream cases, and reference baselines remain unchanged, so this batch
claims no new baseline passes. The broader correctness/performance goal remains
open.

Evidence is in `/tmp/ts-rs-keyword-borrow-perf-20260907/`: versioned benchmark
sources and binaries, input hashes, allocation reports, raw measurements and
summaries, CPU-load samples, predicate probes, `decision.json`, and the retained
build/CI/smoke results. `measurement-integrity.txt` records the complete pair
checks. The rejected compiler sources remain only as experiment artifacts.


## Twenty-fifth implementation batch: JSX identifier scanning and lookup

JSX identifier rescanning now treats contiguous hyphens and identifier
continuation characters as one name. Trailing/repeated hyphens, digits after
hyphens, uppercase hyphenated names, combining characters, and both components
of namespaced names follow the same lexical rule. Trivia terminates a name.
When a JavaScript token straddles the JSX name boundary (such as the numeric
part of `a-1.2`), the parser retains the consumed prefix and rescans the suffix.

JSX tag/attribute names and member-name suffixes report TS17021 over the complete
source token when they contain Unicode escapes. The original spelling remains
in the AST for emission. Type checking resolves decoded component/member names;
hyphenated and namespaced tags are classified as intrinsic elements, including
uppercase spellings. Missing-name/property diagnostics retain the original
escaped spelling, and property diagnostics and hover entries use the source
token's actual location. Attribute and child expressions continue to be checked.

Twelve focused tests cover scanner token boundaries and cooked values, parser
name/diagnostic spans, escaped component resolution, missing expressions,
intrinsic typing requirements, and escaped-property diagnostic/hover locations.
Local TypeScript 5.9.3 scanner/parser/checker behavior was inspected and parser
and semantic probes were recorded. The first baseline candidate exposed the
checker’s literal lookup of escaped spellings; resolving those names recovered
the complete `unicodeEscapesInJsxtags` diagnostic baseline. Further probes
ensured that lookup decoding did not replace source spellings in diagnostics.

All **3,172 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. The final eight cache-free comparisons gain **one complete
conformance diagnostic pass**, with zero passed-case losses, unchanged skips,
and unchanged case/oracle identities. No upstream case or reference baseline
was modified. The comparison base is the verified compiler state at `b9298888d`,
which is unchanged by the benchmark-only successor `77ee1468e`.

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,667 / 2,862 / 0 | 2,832 / 3,075 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

Workspace/CI used the installed TypeScript 6.0.3 library override; baseline tools
used automatic TypeScript 5.9.3 discovery. Final source/test/tool hashes,
manifest identities, and skip counts were rechecked. This correctness batch
claims no measured speedup. The goal remains open with **5,937 diagnostic**,
**983 expanded-JavaScript**, and **1,790 expanded-declaration** failures in
separate, potentially overlapping lanes.

Evidence is in `/tmp/ts-rs-jsx-names-20260907/`. Final snapshot tools, eight
manifests/comparisons, checker/workspace/CI logs, verification script, and input
hashes use suffix `v7`. `manifest-audit-v7.txt` records all totals and the gain;
`typescript-probes.json`, `typescript-semantic-probes.json`, and
`typescript-missing-name-probes.json` record reference behavior. Focused scanner
and parser results are in `scanner-v2.log` and `parser-v6.log`, respectively;
those sources are also covered by the final workspace run.

## Twenty-sixth implementation batch: JSX `this` tags and property diagnostics

JSX tag parsing now represents bare `this` and the root of `this.Component`
with the ordinary `This` expression node. Escaped spellings keep TS17021 while
resolving to the same expression. Names extended with hyphens or a namespace
colon remain JSX identifiers. This removes false unresolved-name diagnostics
and gives emitted JSX the instance value, including inside downleveled arrows.

Missing instance and static properties accessed through `this` now underline
the property token instead of the whole member expression. The new JSX negative
test exposed this pre-existing location error; the correction applies to normal
member expressions as well. TypeScript 5.9.3 parser, emission, and diagnostic
probes confirm both behaviors.

Four new regression tests cover eight tag spellings, valid instance component
lookup, missing static/instance property diagnostic spans in JSX and ordinary
expressions, and runtime identity for bare/member/paired JSX tags inside nested
arrows targeting ES5 and ES2015. An existing parser test now strictly expects
the `This` node instead of the old identifier representation. The runtime test executes emitted JavaScript
with Node and compares the actual tag values to the instance and its component.

All **3,176 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. Eight cache-free comparisons against `59bdbb6e7` gain
**ten complete diagnostic passes** (six compiler, four conformance), with zero
passed-case losses, unchanged skips, and unchanged case/oracle identities.
The parser-only trial gained the three `tsxDynamicTagName5/8/9` cases; the
property-location correction additionally gains `autoLift2`, `autolift4`,
`detachedCommentAtStartOfFunctionBody1/2`,
`invokingNonGenericMethodWithTypeArguments1`, `propertyOrdering2`, and
`typeOfThisInInstanceMember`. No upstream cases or reference baselines changed.

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,673 / 2,856 / 0 | 2,836 / 3,071 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

Workspace/CI used the installed TypeScript 6.0.3 library override; baseline
tools used automatic TypeScript 5.9.3 discovery. Source/test/tool hashes were
rechecked after verification. This correctness batch claims no measured
speedup. The goal remains open with **5,927 diagnostic**, **983 expanded-JavaScript**,
and **1,790 expanded-declaration** failures in separate, potentially overlapping
lanes.

Evidence is in `/tmp/ts-rs-jsx-this-20260907/`: final snapshot tools, eight
manifests/comparisons and verification script use suffix `v2`. Final workspace/CI
logs, audit script, and source/test/tool hashes use `v3`; only the existing AST
assertion changed after the `v2` baseline run. `manifest-audit-v3.txt` records
totals and all ten gains;
`typescript-probes.json` records reference parsing, emission, and diagnostic
locations. Focused parser/runtime logs use `v1`; those unchanged sources are
also covered by the final workspace run.

## Twenty-seventh implementation batch: JSX expressions and text grammar

JSX attribute values, spread attributes, and child expression containers now
parse complete expressions, retaining comma operands instead of producing
cascading brace/name recovery errors. Type checking reports TS18007 for a direct
comma expression in an ordinary attribute or child container. Parenthesized
comma expressions remain allowed, and spread attributes retain their separate
grammar. All operands still receive ordinary expression checking, including
TS2695 for an unused left operand.

The emitter groups retained comma expressions when they supply one property,
argument, array element, or spread operand. This keeps transformed JavaScript
syntactically valid, prevents a single child from becoming multiple children,
and ensures a spread copies only its final operand. Classic React, automatic
JSX, development JSX, extracted keys, and the createElement fallback are
covered at ES5 and ES2018 targets. Key extraction retains TypeScript's evaluation
order, which differs from classic emission when a key precedes a spread.

JSX text reports TS1381 for raw `}` and TS1382 for raw `>`, with exact byte spans.
Entity escapes, quoted attribute strings, and expression literals remain valid.
Splitting an opening delimiter from `>>` now retains the consumed prefix in the
token stream so the delimiter is not recaptured as text. Text skipped by the
JavaScript batch scanner as a comment is preserved even across lines. Text
checks run when each segment is captured, preserving diagnostic order across
nested elements.

Seven new regression tests cover comma ASTs and continuation, expression
grammar distinctions, text preservation and diagnostic positions, nested error
order, and 18 emitted-JavaScript runtime executions. TypeScript 5.9.3 probes
record parsing and semantic diagnostics plus 12 reference runtime executions
for spread/key behavior. No existing assertions were weakened.

All **3,183 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. The final eight cache-free comparisons against `02f984517`
gain **two complete conformance diagnostic passes** (`jsxParsingError1` and
`jsxParsingError3`), with zero passed-case losses, unchanged skips, and unchanged
case/oracle identities. No upstream cases or reference baselines changed.

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,673 / 2,856 / 0 | 2,838 / 3,069 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

Workspace/CI used the installed TypeScript 6.0.3 library override; baseline
tools used automatic TypeScript 5.9.3 discovery. Source/test/tool hashes were
rechecked after verification. This correctness batch claims no measured
speedup. The goal remains open with **5,925 diagnostic**, **983 expanded-JavaScript**,
and **1,790 expanded-declaration** failures in separate, potentially overlapping
lanes.

Evidence is in `/tmp/ts-rs-jsx-containers-20260907/`. Final snapshot tools,
eight manifests/comparisons, workspace/CI logs, verification/audit scripts, and
input hashes use suffix `v5`; `manifest-audit-v5.txt` records all totals and gains.
`typescript-probes.js` and `.json` contain reference probes and results. The
focused emitter/runtime logs use `v3`; those unchanged sources are also covered
by the final workspace run. Earlier trials record the text-boundary correction
and the runtime test that exposed missing comma parentheses.

## Twenty-eighth implementation batch: JSX attribute grammar and recovery

Missing or unbraced JSX attribute initializers now report TS1145 while retaining
the offending token for normal recovery. Empty expression containers, including
comment-only containers, remain distinct from boolean attributes: an omitted
expression retains the full brace span for TS17000 and leaves following
attributes intact.

The checker reports the first empty-container or duplicate-attribute grammar
error per opening element, with TS17000/TS17001 source ranges. Namespaced
duplicate ranges include trivia around the colon and exclude the initializer.
Nested elements and every attribute expression still receive type checking.
The existing file-level expression grammar eligibility flag now also covers
JSX: parse errors suppress JSX grammar diagnostics, including TS18007, while
ordinary expression diagnostics remain active. Regular-expression eligibility
is unchanged.

Empty containers emit boolean `true` in classic, automatic, and development JSX
runtimes, including extracted keys and the createElement fallback. Object-spread
flattening now inserts separators only after an actual property, preventing a
leading comma after an empty spread. Recovery-based attribute splitting runs
only in files with parse errors and only recognizes delimiters outside an
attribute's value and comments. Nested JSX and strings containing `/>` therefore
retain following outer attributes.

Seven new regression tests cover parser continuation and empty-container spans,
attribute grammar precedence, nested/value checks, source-file parse-error
eligibility, and 24 JavaScript runtime executions. TypeScript 5.9.3 probes record
syntax/semantic diagnostics and matching results for all 24 runtime executions.
The first full candidate exposed a compiler JavaScript regression where the old
recovery heuristic discarded outer attributes after a nested value. The final
candidate restores that pass and adds explicit nested-value/string/comment
coverage, including a file with an unrelated parse error.

All **3,190 Rust tests passed**, with **37 ignored** and no failures;
`make ci` passed. Eight cache-free comparisons against `be4e48345` gain
**one complete conformance diagnostic pass** (`jsxAttributeInitializer`), with
zero passed-case losses, unchanged skips, and unchanged case/oracle identities.
No upstream cases, reference baselines, or existing assertions changed.

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,673 / 2,856 / 0 | 2,839 / 3,068 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

Workspace/CI used the installed TypeScript 6.0.3 library override; baseline
tools used automatic TypeScript 5.9.3 discovery. Source/test/tool hashes were
rechecked after verification. This correctness batch claims no measured
speedup. The goal remains open with **5,924 diagnostic**, **983 expanded-JavaScript**,
and **1,790 expanded-declaration** failures in separate, potentially overlapping
lanes. The empty-attribute compiler diagnostic case still needs global-`this`
and closing-tag checking; this batch does not claim that complete case as a gain.

Evidence is in `/tmp/ts-rs-jsx-attributes-20260907/`. Final snapshot tools,
eight manifests/comparisons, workspace/CI logs, verification/audit scripts, and
input hashes use suffix `v5`; `manifest-audit-v5.txt` records totals and the gain.
`typescript-probes.json` records attribute grammar/emit probes, and
`reference-runtime.js`/`.json` record the 24 reference runtime executions plus
namespace-span and parse-error eligibility probes. Focused parser, checker,
and empty-container runtime logs use `v2`, `v3`, and `v4`, respectively; all
retained sources are covered by the final workspace run.
## Main fix waves, September 7, 2026: unclosed JSX diagnostics

Work continues sequentially on `main`, using the shared `release-fast` build
for focused tests and full cache-free comparisons before each fix commit.
Each retained wave must gain complete baseline passes with no passed-case
losses, new skips, or changed upstream inputs/oracles.

The first wave reports TS17008 on an unclosed JSX element's opening name and
TS17014 on an unclosed fragment's opening range. Nested elements retain their
parsed children and each opening receives its own diagnostic. The shared EOF
insertion point receives one TS1005 (`'</' expected.`), replacing the separate
`<`, `/`, and `>` errors. Three regression tests cover qualified/namespaced
names, trivia, nesting, fragments, and valid closing tags.

Eight fresh comparisons against the starting working tree at `6bb2dae04`
gain **one complete compiler diagnostic pass**, `errorSpanForUnclosedJsxTag`:

| Lane | Compiler passed / failed / skipped | Conformance passed / failed / skipped |
| --- | ---: | ---: |
| Diagnostics | 3,674 / 2,855 / 0 | 2,839 / 3,068 / 0 |
| Default JavaScript | 6,032 / 0 / 497 | 5,388 / 0 / 519 |
| Expanded JavaScript | 6,442 / 262 / 497 | 6,378 / 721 / 519 |
| Expanded declarations | 5,730 / 974 / 497 | 6,283 / 816 / 519 |

All eight lanes have zero passed-case losses and unchanged skips. The fresh
base also matches the previous committed batch's eight manifests. Upstream
case/oracle hashes and the pre-existing emitter/profiling edits are unchanged.
The latter edits are outside this commit.

Validation: 310 parser tests, 769 emitter tests, the small compiler subset,
workspace check, and workspace formatting all pass. Cargo commands use
`--profile release-fast` to reuse compilation artifacts. Evidence, input
hashes, snapshot tools, manifests, comparisons, and logs are under
`/tmp/ts-rs-main-waves-20260907/`, with `base` and `wave1` suffixes.

### Wave 2: JSX closing-tag ownership and recovery emission

Closing names now receive TS17002 when they do not match the opening name.
Name comparison ignores trivia and decodes identifier escapes. An inner
element that encounters its parent's closing tag reports TS17008 and leaves
that delimiter for the parent, preserving following statements. A named
closing tag on a fragment reports TS17015 while retaining expression recovery.

The AST records missing closing tags explicitly. Preserve-mode emission uses
that information to insert `</>` before a retained parent close and count EOF
closes without guessing from tag-name strings. This also handles qualified
names and distinguishes orphan closing delimiters from actual openings.
Three parser tests and one emitter test cover these behaviors, with six
emission fixtures and TypeScript probes for names, spans, and recovery.

Eight cache-free comparisons against wave 1 add **one conformance diagnostic
pass**, `jsxParsingError2`: **2,839 → 2,840**. Compiler diagnostics remain
**3,674**; all JavaScript/declaration counts and skips remain unchanged, with
zero passed-case losses. The initial candidate's two JavaScript regressions
were repaired before retaining the wave.

Validation: **313 parser tests**, **770 emitter tests**, the small compiler
subset, workspace check, and formatting pass using `release-fast`. Final
snapshot tools and eight manifests use `wave2-final` under
`/tmp/ts-rs-main-waves-20260907/`; tests/checks also use `wave2-final` logs.
Upstream cases/oracles and the pre-existing working-tree edits are unchanged.

### Wave 3: self-closing JSX intrinsic diagnostic spans

TS7026 now underlines the full self-closing JSX element, including attributes
and delimiters. Attribute-expression errors retain their own spans. Two new
tests cover intrinsic, hyphenated, namespaced, and multiline tags, option
eligibility, and an unresolved attribute expression.

Eight cache-free comparisons against wave 2 recover **14 complete diagnostic
cases**: compiler passes **3,674 → 3,681** and conformance passes
**2,840 → 2,847**. JavaScript/declaration counts and skips are unchanged, with
zero passed-case losses. These are full reference-output matches, not merely
matching diagnostic codes.

Validation: **15 focused JSX checker tests**, **770 emitter tests**, the small
compiler subset, workspace check, and formatting pass. Snapshot tools, full
manifests, comparisons, checks, and input hashes use the `wave3` suffix under
`/tmp/ts-rs-main-waves-20260907/`. Upstream cases/oracles and pre-existing edits
remain unchanged.

### Wave 4: unresolved side-effect import diagnostic codes

Unresolved side-effect imports now report TS2882 under every module-resolution
kind. Previously that diagnostic was selected only under Classic resolution;
Node and Bundler resolution incorrectly reported TS2307. Bound imports retain
their existing resolution-dependent diagnostic. Existing resolution tests now
assert the complete diagnostic-code list, including the corrected code, and a
new test covers five resolution modes, the message, and the module-name span.

Eight cache-free comparisons against wave 3 recover **four compiler diagnostic
cases**, increasing passes **3,681 → 3,685**. Conformance remains **2,847**.
JavaScript/declaration counts and skips are unchanged, with zero passed-case
losses. Validation passes: **four module-resolution tests**, **770 emitter
tests**, the small compiler subset, workspace check, and formatting.

Evidence and source hashes use the `wave4` suffix under
`/tmp/ts-rs-main-waves-20260907/`. Upstream cases/oracles and pre-existing edits
are unchanged. Across waves 1–4, full diagnostic passes increased by **20**:
compiler **3,673 → 3,685**, conformance **2,839 → 2,847**.

### Wave 5: paired JSX tag spans and closing-name checks

Paired JSX elements retain the full opening span and the parsed closing name
and span. The checker reports intrinsic-element errors on each full tag and
resolves closing component names in the surrounding scope. Missing qualified
properties underline the property token on both tags; recovered missing closes
do not invent a second reference. Three new checker tests cover paired/nested
intrinsics, missing components and properties, instance members, and recovery.

Eight cache-free comparisons against wave 4 recover **10 complete diagnostic
cases**: compiler passes **3,685 → 3,691**, conformance **2,847 → 2,851**.
All JavaScript/declaration counts and skips remain unchanged, with zero
passed-case losses. Validation passes: **18 focused JSX checker tests**,
**313 parser tests**, **770 emitter tests**, the small compiler subset,
workspace check, and formatting.

Evidence and hashes use the `wave5` suffix under
`/tmp/ts-rs-main-waves-20260907/`. Upstream cases/oracles and pre-existing edits
are unchanged. Waves 1–5 have gained **30 complete diagnostic passes** in total.
The campaign remains open: **5,894 diagnostic**, **983 expanded-JavaScript**,
and **1,790 expanded-declaration** failures remain in separate overlapping lanes.

### Wave 6: multiline diagnostic annotation ordering

Error-baseline annotations now retain diagnostic source order across every
covered line, including blank continuation rows. A multiline diagnostic ends
before later diagnostics on its final line. Its message and related information
use the same rendering path as a single-line diagnostic. One exact-output
regression test covers a multiline JSX error followed by an EOF error.

Eight cache-free comparisons against wave 5 recover **two complete diagnostic
cases**: `reachabilityChecks1` and `tsxFragmentErrors`. Compiler passes increase
**3,691 → 3,692** and conformance passes **2,851 → 2,852**. All other lane
counts and skips are unchanged, with zero passed-case losses.

Validation passes: **90 harness library tests**, **770 emitter tests**, the
small compiler subset, workspace check, and formatting. Evidence and hashes
use `wave6` under `/tmp/ts-rs-main-waves-20260907/`. Upstream test/oracle inputs
and unrelated working-tree edits are unchanged.

### Wave 7: root-first error-baseline source sections

Diagnostic annotations now follow the upstream compiler runner's root/other
file partition for fixtures without tsconfig. The last input is printed first
when its text contains the runner's require/reference signals or the fixture
sets noImplicitReferences. Header sorting and compilation remain unchanged.
Two regression tests cover section ordering, retained diagnostics, textual
signals, output options, metadata, and tsconfig exclusions.

Eight cache-free comparisons against wave 6 recover **87 complete diagnostic
cases**: compiler passes **3,692 → 3,758**, conformance **2,852 → 2,873**.
All JavaScript/declaration counts and skips remain unchanged, with zero
passed-case losses. Validation passes: **92 harness library tests**, **770
emitter tests**, the small compiler subset, workspace check, and formatting.

Evidence and hashes use `wave7` under `/tmp/ts-rs-main-waves-20260907/`.
Upstream cases/oracles and unrelated edits are unchanged. Waves 1–7 have
recovered **119 complete diagnostic cases** in total.

### Wave 8: excess-property checks for empty structural targets

Fresh object literals can now be assigned to structural targets with no
properties, call/construct signatures, or index signatures. This covers empty
classes, inherited empty classes, `{}`, array elements, and later assignments.
The regression test also requires excess-property errors for nonempty,
callable, and constructable targets.

Eight cache-free comparisons against wave 7 recover **two complete diagnostic
cases**, `indexerA` and `classWithEmptyBody`: compiler passes **3,758 → 3,759**,
conformance **2,873 → 2,874**. All other lane counts and skips are unchanged,
with zero passed-case losses. Validation passes: **542 checker library tests**,
**770 emitter tests**, the small compiler subset, workspace check, and formatting.
The checker library suite required `TSC_RS_TYPESCRIPT_LIB_DIR` pointing to the
installed `/tmp/hygiene-frozen-tdg9_l0n/node_modules/typescript/lib` because the
default installation lacks the Temporal declaration file. Baseline comparisons
kept their original library environment.

Evidence and hashes use `wave8` under `/tmp/ts-rs-main-waves-20260907/`.
Upstream cases/oracles and unrelated edits are unchanged. Cumulative diagnostic
gain across waves 1–8: **121 complete cases**.

### Wave 9: reject unsupported outFile module formats

Option validation reports TS6082 for JavaScript bundles using an explicit
module format other than AMD, System, or None. Declaration-only output is
exempt; noEmit does not suppress configuration errors. Tests cover every
module kind, the exact diagnostic, and the option key used for config spans.

Eight cache-free comparisons against wave 8 recover **six compiler diagnostic
cases**, increasing passes **3,759 → 3,765**. Conformance remains **2,874**;
JavaScript/declaration counts and skips are unchanged, with zero passed-case
losses. Validation passes: **543 checker library tests** using the Temporal
library path recorded in wave 8, **770 emitter tests**, the small compiler
subset, workspace check, and formatting.

Evidence and hashes use `wave9-options` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream cases/oracles and unrelated edits
are unchanged. An earlier excess-property candidate had no complete-case gain
and was saved as an unapplied patch. Cumulative diagnostic gain: **127 cases**.

### Wave 10: validate source-map option combinations

Option validation reports TS5053 when inlineSourceMap conflicts with sourceMap
or mapRoot, and TS5069 when mapRoot lacks both sourceMap and declarationMap.
Tests assert full diagnostic lists, messages, config option keys, enabled and
disabled flags, valid companions, and an empty mapRoot.

Eight cache-free comparisons against wave 9 recover **three compiler diagnostic
cases**, increasing passes **3,765 → 3,768**. Conformance remains **2,874**;
all JavaScript/declaration counts and skips remain unchanged, with zero
passed-case losses. Validation passes: **544 checker library tests** with the
Temporal library path from wave 8, **770 emitter tests**, the small compiler
subset, workspace check, and formatting.

Evidence and hashes use `wave10` under `/tmp/ts-rs-main-waves-20260907/`.
Upstream cases/oracles and unrelated edits remain unchanged. Waves 1–10 have
recovered **130 complete diagnostic cases**. The remaining failures are
**5,794 diagnostic**, **983 expanded-JavaScript**, and **1,790 expanded-declaration**
cases in separate overlapping lanes; the campaign remains open.

### Wave 11: declaration-option prerequisites

Option validation reports TS5069 when declarationDir, declarationMap,
emitDeclarationOnly, or isolatedDeclarations is enabled without declaration
or composite. Config parsing retains declarationDir/declarationMap with
directive precedence, allowing errors to point at the actual tsconfig key.
Tests check complete diagnostics, disabled controls, noEmit, and both valid
companions. Unrelated option tests now enable their declaration prerequisite.

Eight cache-free comparisons against wave 10 recover **five compiler diagnostic
cases**: passes **3,768 → 3,773**. Conformance remains **2,874**. All other
lane counts and skips are unchanged, with zero passed-case losses.
Validation passes: **545 checker tests** with the Temporal library path from
wave 8, **92 harness tests**, **770 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave11` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits are
unchanged. Cumulative diagnostic gain: **135 complete cases**.

### Wave 12: ES5 setter parameter lowering

Class setter functions reuse the established default/rest parameter transform
when lowering to ES5 property descriptors. Eligibility checks reject parameter
shapes the transform cannot handle. A runtime regression checks default
side effects, receiver state, getter/setter pairing, and rest argument copying.

Eight cache-free comparisons against wave 11 recover **four expanded JavaScript
variants**: compiler **6,442 → 6,444**, conformance **6,378 → 6,380**.
Diagnostic, default JavaScript, declaration, and skip counts are unchanged,
with zero passed-case losses. Validation passes: **771 emitter tests**, the
small compiler subset, workspace check, and formatting. Evidence/hashes use
`wave12` under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated
edits are unchanged. Cumulative gains: **135 diagnostic cases** and **four
expanded JavaScript variants** across the separate lanes.

### Wave 13: ES5 literal member names

ES5 class lowering emits string and numeric method names as bracket accesses
and supports literal accessor names. Getter/setter pairing uses the actual
property key, including escaped strings, hexadecimal values, separators, and
exponent notation. The emitted key spelling is preserved. A runtime regression
checks method calls, receiver state, and exactly one descriptor per accessor pair.

Eight cache-free comparisons against wave 12 recover **five expanded JavaScript
variants**: compiler **6,444 → 6,445**, conformance **6,380 → 6,384**.
All other lane counts and skips are unchanged, with zero passed-case losses.
Validation passes: **772 emitter tests**, the small compiler subset, workspace
check, and formatting. Final evidence/hashes use `wave13-final` under
`/tmp/ts-rs-main-waves-20260907/`; the preliminary `wave13` probe is superseded.
Upstream inputs and unrelated edits are unchanged. Cumulative gains:
**135 diagnostic cases** and **nine expanded JavaScript variants**.

### Wave 14: preserve ES5 constructor comments

ES5 class lowering moves each constructor's own leading comments with its
function and appends its trailing comment after the closing brace. Adjacent
erased-field comments remain omitted. A regression checks leading, body, and
trailing comments occur once, their positions, and removeComments behavior.

Eight cache-free comparisons against wave 13 recover **two expanded JavaScript
variants**: compiler **6,445 → 6,446**, conformance **6,384 → 6,385**.
All other lane counts and skips are unchanged, with zero passed-case losses.
Validation passes: **773 emitter tests**, the small compiler subset, workspace
check, and formatting. Evidence/hashes use `wave14` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits are
unchanged. Cumulative gains: **135 diagnostic cases** and **11 expanded
JavaScript variants** in separate lanes.

### Wave 15: enum constants in declarations

Declaration emission evaluates numeric/string enum constants, arithmetic,
member references, namespace-qualified references, and implicit numeric values.
Runtime-only initializers are omitted; ambient enums preserve their distinct
implicit-value rules. Focused tests cover full output, scope ownership,
non-finite values, and bitwise arithmetic. Untouched mixed line endings in the
existing declaration emitter are preserved.

Eight cache-free comparisons against wave 14 recover **six declaration
baselines**: compiler **5,730 → 5,732**, conformance **6,283 → 6,287**.
All other lane counts and skips are unchanged, with zero passed-case losses.
Validation passes: **44 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Final evidence/hashes use
`wave15-final` under `/tmp/ts-rs-main-waves-20260907/`; the earlier `wave15`
probe is superseded. Upstream inputs and unrelated edits remain unchanged.
Cumulative gains: **135 diagnostic cases**, **11 expanded JavaScript variants**,
and **six declaration variants**, counted in separate overlapping lanes.

### Wave 16: literal const declarations

Const declarations preserve direct numeric, string, boolean, template, and
bigint literal values. Numeric spellings normalize to declaration form; bigint
conversion retains arbitrary precision. Explicit annotations and widened
mutable declarations keep their existing types. Tests assert complete output,
Unicode escapes, signed literals, annotations, and large bigint values.

Eight cache-free comparisons against wave 15 recover **one compiler declaration
baseline**, increasing passes **5,732 → 5,733**. All other lane counts and skips
are unchanged, with zero passed-case losses. Validation passes: **46 declaration
tests**, **773 emitter tests**, the small compiler subset, workspace check,
and formatting. Evidence/hashes use `wave16` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative gains: **135 diagnostics**, **11 expanded JavaScript**,
and **seven declaration variants** in separate lanes.

### Wave 17: class property declaration serialization

Readonly fields preserve direct literal values; definite-assignment assertions
are omitted. Constructor parameter properties become fields with the appropriate
visibility, optionality, readonly modifier, and type. Constructor parameters
lose property modifiers, private constructors omit their parameter lists, and
trailing defaults are optional. Tests cover complete output for ordinary,
readonly, private/protected, defaulted, and definite-assignment properties.

Eight cache-free comparisons against wave 16 recover **six declaration
baselines**: compiler **5,733 → 5,736**, conformance **6,287 → 6,290**.
All other lane counts and skips remain unchanged, with zero passed-case losses.
Validation passes: **48 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Final evidence/hashes use
`wave17-complete` under `/tmp/ts-rs-main-waves-20260907/`; earlier `wave17`
and `wave17-final` probes are superseded. Upstream inputs and unrelated edits
are unchanged. Cumulative gains: **135 diagnostic cases**, **11 expanded
JavaScript variants**, and **13 declaration variants** in separate lanes.

### Wave 18: namespace declaration visibility and prefixes

Namespace declaration output retains public declarations and private types
needed by their signatures. Unused implementation declarations are omitted;
ambient namespaces retain implicit members. Export markers preserve private
boundaries where necessary, redundant nested declare/export prefixes disappear,
and dotted namespace names emit once. A complete-output test covers these
visibility cases and nested enum declarations.

Eight cache-free comparisons against wave 17 recover **34 declaration cases**:
compiler **5,736 → 5,769**, conformance **6,290 → 6,291**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**49 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave18` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative declaration gain is **47**, alongside **135 diagnostic**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 19: declaration function and method return signatures

Declaration signatures omit implementation-only async and generator markers.
Functions and methods without value-bearing returns infer void, or Promise<void>
when async. Return scanning covers nested control-flow statements and excludes
nested function scopes; explicit return annotations remain intact. Two tests
cover complete signature output and value returns across control-flow forms.

Eight cache-free comparisons against wave 18 recover **47 declaration cases**:
compiler **5,769 → 5,804**, conformance **6,291 → 6,303**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**51 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave19` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative declaration gain is **94**, alongside **135 diagnostic**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 20: public overload signatures

Function, method, and constructor declarations omit overload implementation
signatures. Signature dependency collection likewise omits implementation-only
types while retaining constructor parameter-property types. Static and instance
methods stay separate, nested namespaces use their own overload sets, and
anonymous default function declarations preserve their separating space.
The old overload test now checks the correct complete public output; new tests
cover scope, default exports, parameter properties, and quoted method names.

Eight cache-free comparisons against wave 19 recover **13 declaration cases**:
compiler **5,804 → 5,811**, conformance **6,303 → 6,309**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**53 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave20` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative declaration gain is **107**, alongside **135 diagnostic**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 21: implicit declaration types and private overloads

Untyped declaration members, bodyless function/method returns, and ordinary
uninitialized parameters emit explicit any annotations; untyped rest parameters
emit any[]. Private method overloads collapse to one stripped member per static
or instance scope. Complete-output tests cover interfaces, nested type literals,
ambient functions, abstract members, rest parameters, and private overloads.

Eight cache-free comparisons against wave 20 recover **16 declaration cases**:
compiler **5,811 → 5,824**, conformance **6,309 → 6,312**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**55 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave21` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative declaration gain is **123**, alongside **135 diagnostic**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 22: initialized parameter declarations

Functions, methods, and constructors share parameter serialization. Trailing
defaults become optional, initializer types are retained, and required defaults
include undefined under strictNullChecks. Declaration emission now receives the
compiler options, including explicit strictNullChecks overrides. Tests cover
required/trailing defaults, rest parameters, constructors, type assertions, and
literal const assertions. Undefined detection currently handles explicit type
syntax; resolving aliases remains part of the broader declaration type work.

Eight cache-free comparisons against wave 21 recover **six compiler declaration
cases**, increasing passes **5,824 → 5,830**. Other lane counts and skips are
unchanged, with zero passed-case losses. Validation passes:
**58 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave22` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative declaration gain is **129**, alongside **135 diagnostic**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 23: single-return declaration inference

Function and method bodies consisting of one return infer literal or parameter
types using the parameter's own scope. Generic parameter types remain local to
the signature, optional parameters retain undefined under strictNullChecks, and
simple async returns emit Promise types. Complex bodies and thenable resolution
remain on the existing inference path. Complete-output tests cover parameter
shadowing, generics, methods, literals, async returns, and optional parameters.

Eight cache-free comparisons against wave 22 recover **four compiler declaration
cases**, increasing passes **5,830 → 5,834**. Other lane counts and skips are
unchanged, with zero passed-case losses. Validation passes:
**60 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave23` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative declaration gain is **133**, alongside **135 diagnostic**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 24: invalid const class member diagnostics

Class declarations and expressions report TS1248 for const member modifiers,
with spans on the member name (or constructor header). Existing recovery keeps
the member structure intact. Tests cover properties, methods, accessors,
constructors, and computed names, while preserving legal const member names,
const generic parameters, and const enums.

Eight cache-free comparisons against wave 23 recover **two compiler diagnostic
cases**, increasing passes **3,773 → 3,775**. Other lane counts and skips are
unchanged, with zero passed-case losses. Validation passes:
**273 parser tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave24`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **137**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 25: defaults in bodyless signatures

Bodyless function, method, constructor, accessor, and type signatures report
TS2371 for parameter defaults, including defaults inside binding patterns.
Index signatures consume their invalid initializer and report TS1020 alongside
TS2371, preserving the surrounding declaration. Tests verify diagnostic spans
and ensure implementation defaults and arrow functions remain valid.

Eight cache-free comparisons against wave 24 recover **eight diagnostic cases**:
compiler **3,775 → 3,780**, conformance **2,874 → 2,877**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**276 parser tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave25`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **145**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 26: property signature initializer recovery

Interface and type-literal properties consume invalid initializers and report
TS1246 or TS1247 at the initializer expression. This prevents spurious identifier
and member-recovery errors and retains following members. A regression test
covers nested interface/type-literal contexts and verifies the recovered AST.

Eight cache-free comparisons against wave 25 recover **two compiler diagnostic
cases**, increasing passes **3,780 → 3,782**. Other lane counts and skips are
unchanged, with zero passed-case losses. Validation passes:
**277 parser tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave26`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **147**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 27: rest and optional parameter grammar

Parameter lists report TS1014, TS1047, TS1048, TS1015, or TS1016 for invalid
rest placement, optional/defaulted rest parameters, optional parameters with
defaults, and required parameters after optional ones. Checks report the first
applicable list error with precise token/name spans. Tests cover declarations,
arrow functions, function types, error precedence, and valid parameter ordering.

Eight cache-free comparisons against wave 26 recover **14 diagnostic cases**:
compiler **3,782 → 3,788**, conformance **2,877 → 2,885**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**279 parser tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave27`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **161**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 28: accessor signatures and setter returns

Shared accessor validation covers class/object accessors and interface/type
signatures: arity, this parameters, optional setter parameters, return
annotations, and accessor type parameters. Ordinary parameter-list errors take
precedence, and setter rest/default errors no longer accumulate incorrectly.
Setter return-value checks traverse the setter's control flow and skip nested
function/class scopes. Four regression tests verify spans and valid controls.

Eight cache-free comparisons against wave 27 recover **11 diagnostic cases**:
compiler **3,788 → 3,791**, conformance **2,885 → 2,893**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**545 checker unit tests**, **four accessor regression tests**, **60 declaration
tests**, **773 emitter tests**, the small compiler subset, workspace check, and
formatting. Checker unit tests use the installed complete TypeScript library
snapshot; baseline audit inputs remain unchanged. Evidence/hashes use `wave28`
under `/tmp/ts-rs-main-waves-20260907/`. Unrelated edits are preserved.
Cumulative gains: **172 diagnostics**, **133 declarations**, and **11 expanded
JavaScript** variants in separate overlapping lanes.

### Wave 29: accessor pair modifiers

Getter/setter pairs must agree on abstractness, and the getter cannot be less
accessible than its setter. Pairs are checked once within their static or
instance scope using resolved property names; unresolved computed names never
form an accidental pair. Class accessors named constructor report TS1341.
Four more regression tests cover visibility combinations, literal names,
abstractness, constructor names, and unresolved computed names.

Eight cache-free comparisons against wave 28 recover **two diagnostic cases**:
compiler **3,791 → 3,792**, conformance **2,893 → 2,894**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**545 checker unit tests**, **eight accessor regression tests**, **60 declaration
tests**, **773 emitter tests**, the small compiler subset, workspace check, and
formatting. Evidence/hashes use `wave29` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **174**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 30: getter missing-return reachability

Getters report TS2378 when their body can fall through without a reachable
explicit return. Syntactic flow tracks branches, loops, switch fallthrough,
labels, breaks/continues, and try/catch/finally while excluding nested function
scopes. Ambient signatures are excluded. This follows TypeScript binder
behavior, including its treatment of never-returning calls and boolean syntax.
All **26 control-flow examples** matched the installed TypeScript compiler;
regression tests apply the examples to both class and object getters.

Eight cache-free comparisons against wave 29 recover **14 diagnostic cases**:
compiler **3,792 → 3,793**, conformance **2,894 → 2,907**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**545 checker unit tests**, **10 accessor regression tests**, **60 declaration
tests**, **773 emitter tests**, the small compiler subset, workspace check, and
formatting. Evidence/hashes use `wave30` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **188**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 31: class member modifier grammar

Class members validate modifier order, duplicates, incompatible combinations,
and permitted declaration kinds with first-error precedence and token spans.
Modifier words remain valid member names. Repeated static modifier recovery
reports TS1434 and suppresses cascading modifier grammar errors when parsing
fails. Abstract constructor validation now lives in the parser, removing a
duplicate ambient checker diagnostic. Regression tests cover modifier contexts,
valid names, syntax recovery, and combined parser/checker diagnostics.

Eight cache-free comparisons against wave 30 recover **45 diagnostic cases**:
compiler **3,793 → 3,797**, conformance **2,907 → 2,948**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**281 parser tests**, **545 checker unit tests**, **one modifier integration
test**, **60 declaration tests**, **773 emitter tests**, the small compiler
subset, workspace check, and formatting. Evidence/hashes use `wave31` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **233**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 32: ambient class property initializers

Ambient class properties report TS1039 on forbidden initializers, including
declare properties, declaration files, namespaces, and annotated readonly
properties. Readonly properties without annotations retain value inference.
Modifier errors take precedence. Decorated properties are checked after
option-dependent decorator validation: standard decorators on declare properties
report TS1206, while valid legacy decorators permit the initializer check.

Eight cache-free comparisons against wave 31 recover **three diagnostic cases**:
compiler **3,797 → 3,799**, conformance **2,948 → 2,949**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**282 parser tests**, **545 checker unit tests**, **two modifier integration
tests**, **60 declaration tests**, **773 emitter tests**, the small compiler
subset, workspace check, and formatting. Evidence/hashes use `wave32` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **236**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 33: constructor type grammar

Constructor type parameters report TS1092, empty parameter lists additionally
report TS1098, and constructor return annotations report TS1093. Type parameter
errors take precedence over return annotations, while independent modifier
errors remain visible. Existing AST recovery is preserved. Span regressions
cover comments, trailing commas, nested generic closers, and ambient signatures.

Eight cache-free comparisons against wave 32 recover **three diagnostic cases**:
compiler remains **3,799**, conformance **2,949 → 2,952**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**283 parser tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave33`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **239**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 34: generic argument list grammar

Shared type-argument parsing reports TS1099 for empty lists and TS1009 for
trailing commas across calls, constructors, type references, instantiations,
and tagged templates. Nested lists track their own commas even when generic
closers share a token. Speculative rollback and syntax-error precedence retain
existing recovery behavior; declaration type parameter trailing commas remain
valid. Regression tests cover nested lists, comments, and comparison syntax.

Eight cache-free comparisons against wave 33 recover **three diagnostic cases**:
compiler **3,799 → 3,802**, conformance remains **2,952**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**284 parser tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave34`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **242**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 35: index signature parameter parsing and grammar

Classes, interfaces, and type literals share index-signature parsing with full
parameter lists. Lookahead distinguishes index signatures from computed names,
including empty lists, rest parameters, modifiers, optional names, and commas.
Validation reports index-specific parameter count, trailing comma, rest,
accessibility, optionality, initializer, and missing parameter type errors with
TypeScript's precedence. Readonly index signatures retain their modifier in the
AST. Regression tests exercise all three declaration contexts and computed-name
controls; semantic key-type validation remains separate.

Eight cache-free comparisons against wave 34 recover **12 diagnostic cases**:
compiler **3,802 → 3,809**, conformance **2,952 → 2,957**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**286 parser tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave35`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **254**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 36: index key types and result annotations

Shared checker validation distinguishes valid index domains from literal or
generic keys (TS1337) and other invalid types (TS1268). It follows aliases,
normalizes unions/intersections, handles template patterns, enum members,
unique symbols, and keyof domains, and reports missing result annotations
(TS1021) after key validation. The previous named-type heuristic is removed.
Class and nested type-annotation checks share validation and avoid duplicate
diagnostics. Parser syntax-error classification is reused to preserve grammar
precedence independently of other reported grammar errors.

Eight cache-free comparisons against wave 35 recover **six diagnostic cases**:
compiler **3,809 → 3,811**, conformance **2,957 → 2,961**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**286 parser tests**, **545 checker unit tests**, **two index grammar regression
tests**, **60 declaration tests**, **773 emitter tests**, the small compiler
subset, workspace check, and formatting. Evidence/hashes use `wave36` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **260**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 37: overload implementation compatibility

Functions, methods, and constructors share TS2394 implementation compatibility
checks, reporting the first incompatible overload with TS2750 related information
at its implementation. Checks cover erased generic parameters, required and rest
arguments, explicit this parameters, return types, and strict function variance.
Methods and constructors retain parameter bivariance; callback parameters apply
their separate variance rules. Namespace and nested function scopes are covered.
The previous constructor-only check is replaced by the shared implementation.

Eight cache-free comparisons against wave 36 recover **15 diagnostic cases**:
compiler **3,811 → 3,821**, conformance **2,961 → 2,966**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**545 checker unit tests**, **four overload integration tests**, **60 declaration
tests**, **773 emitter tests**, the small compiler subset, workspace check, and
formatting. Evidence/hashes use `wave37` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **275**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 38: class property and index signature constraints

TS2411 validation covers instance and static fields, methods, accessors, and
class expressions. Class metadata retains index signatures for inheritance,
including substituted generic arguments and defaults. Own members are checked
against inherited signatures, and local signatures constrain inherited members.
Checks distinguish string, numeric, symbol, and template keys, preserve quoted
property spelling, and account for optional properties under strict null checks.
Private identifiers remain outside index access; ordinary private members are
checked. Numeric names follow canonical JavaScript spelling, including exponent
forms, NaN, and infinities.

Eight cache-free comparisons against wave 37 recover **16 diagnostic cases**:
compiler **3,821 → 3,824**, conformance **2,966 → 2,979**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**545 checker unit tests**, **three index constraint integration tests**,
**60 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave38` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **291**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 39: interface property and index signature constraints

Interface properties and methods are checked against their own index signatures
and those inherited from classes or interfaces. Metadata retains every index
domain through declaration merging and substitutes generic arguments along
heritage chains. Duplicate property declarations report at the first location,
and inherited-property identity errors retain precedence. Index diagnostics use
single-line nested types and TypeScript's full method-signature spans.

Eight cache-free comparisons against wave 38 recover **eight diagnostic cases**:
compiler **3,824 → 3,828**, conformance **2,979 → 2,983**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**545 checker unit tests**, **five index constraint integration tests**,
**60 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave39` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **299**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 40: compatibility between index signatures

TS2413 checks index value types when key domains overlap through assignability,
including number-to-string indices, generic bases, merged interfaces, and
static signatures. Errors point to the narrower local index, the other local
index, or the interface that combines incompatible inherited domains. Merged
index locations retain their first declaration; existing base errors are not
repeated. Static index signatures correctly remain local to their class.

Structural comparisons now recognize boxed primitive members such as string's
length property. The incorrect blanket acceptance of empty object sources is
removed, so required properties are checked by normal structural comparison.
These changes preserve the existing merged-interface cases while recovering
index conflicts and an additional contextual typing case.

Eight cache-free comparisons against wave 39 recover **six diagnostic cases**:
compiler **3,828 → 3,832**, conformance **2,983 → 2,985**. Other lane counts and
skips are unchanged, with zero passed-case losses. Validation passes:
**545 checker unit tests**, **nine index constraint integration tests**,
**60 declaration tests**, **773 emitter tests**, the small compiler subset,
workspace check, and formatting. Evidence/hashes use `wave40` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **305**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 41: inherited properties and interface index constraints

Interfaces check inherited properties against newly combined index domains.
A local index reports at its declaration; otherwise the interface that first
combines the incompatible bases reports the error. Overrides take precedence,
and a conflict already contained in one base is not repeated in descendants.
Inherited symbol keys retain their domain in both class and interface checks.
Diagnostics at a shared location use TypeScript's message ordering.

Eight cache-free comparisons against wave 40 recover **one compiler diagnostic
case**, raising compiler passes **3,832 → 3,833**; conformance remains **2,985**.
Other lane counts and skips are unchanged, with zero passed-case losses.
Validation passes: **545 checker unit tests**, **12 index constraint integration
tests**, **60 declaration tests**, **773 emitter tests**, the small compiler
subset, workspace check, and formatting. Evidence/hashes use `wave41` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **306**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.

### Wave 42: well-known symbol index constraints

Own class and interface computed members retain the symbol key domain for
well-known global `Symbol` properties, including members checked against an
inherited index. Ordinary string keys, compatible value types, and local or
module-level bindings named `Symbol` keep their existing behavior. Focused
positive and negative cases were checked against the upstream TypeScript API.

Eight cache-free comparisons against wave 41 recover **one conformance
diagnostic case**, raising conformance passes **2,985 → 2,986**; compiler remains
**3,833**. Other lane counts and skips are unchanged, with zero passed-case
losses. Validation passes: **545 checker unit tests**, **13 index constraint
integration tests**, **60 declaration tests**, **773 emitter tests**, the small
compiler subset, workspace check, and formatting. Evidence/hashes use `wave42`
under `/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits
remain unchanged. Cumulative diagnostic gain is **307**, alongside **133
declaration** and **11 expanded JavaScript** gains in separate overlapping lanes.

The preceding documentation commit refreshed README tables from clean committed
revision `aba3839d5`: ten baseline reports, two full diagnostic-accuracy reports,
and five LSP operation reports. `docs/compatibility-metrics.json` retains that
explicitly versioned snapshot, commands, and manifest hashes.

### Wave 43: inherited computed-member diagnostic origins

Class metadata preserves the spelling and source location of computed fields,
methods, and accessors on both instance and static sides. Inherited-member index
errors now use that spelling and add TS2728 pointing to the original member.
External declaration injection retains the correct source text while collecting
metadata, so related locations and names also work across files. Ordinary
inherited members keep their existing diagnostics without a computed-member note.

Eight cache-free comparisons against wave 42 recover **two conformance diagnostic
cases**, raising conformance passes **2,986 → 2,988**; compiler remains **3,833**.
Other lane counts and skips are unchanged, with zero passed-case losses.
Validation passes: **545 checker unit tests**, **15 index constraint integration
tests**, **60 declaration tests**, **773 emitter tests**, the small compiler
subset, workspace check, and formatting. Evidence/hashes use `wave43` under
`/tmp/ts-rs-main-waves-20260907/`. Upstream inputs and unrelated edits remain
unchanged. Cumulative diagnostic gain is **309**, alongside **133 declaration**
and **11 expanded JavaScript** gains in separate overlapping lanes.
