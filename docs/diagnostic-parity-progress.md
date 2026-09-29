# Diagnostic parity progress

Last updated: 2026-07-27

This document records the current type-checker diagnostic baseline, the
verification used for the latest parity wave, and the concrete follow-up work.
Counts are location-aware comparisons against the TypeScript 6.0.3 error
baselines.

## Current score

| Suite | Revision | Matched | False negatives | False positives | Recall | Precision |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Conformance | corrected `main` baseline | 12,459 | 16,276 | 5,810 | 43.3583% | 68.1975% |
| Conformance | current | 13,102 | 15,633 | 5,454 | 45.5960% | 70.6079% |
| Compiler | measurable `main` baseline | 6,226 | 8,690 | 3,633 | 41.7404% | 63.1504% |
| Compiler | current | 6,454 | 8,462 | 3,648 | 43.2690% | 63.8883% |

The conformance delta is `+643` matches, `-643` false negatives, and `-356`
false positives. The compiler delta is `+228` matches, `-228` false negatives,
and `+15` false positives; the larger true-positive gain still improves both
compiler recall and precision.

The compiler baseline needs one performance-only fix to be measurable.
Unpatched `main` exhausts memory on source case 4,010,
`longObjectInstantiationChain1.ts`: a bounded run succeeds through case 4,009,
then attempts a 1 GiB allocation on case 4,010. The uncapped process reached
approximately 46 GiB RSS and filled 32 GiB of swap. Applying
`989c17990` (`preserve lazy object merge instantiations`) reduces the same
4,010-source prefix to about 58 MiB RSS. That commit's complete conformance
report is byte-identical to `main`, so it is used only as the diagnostic-neutral
compiler measurement vehicle.

## Included work

- Decorator recovery and validity diagnostics: TS1206, TS1249, and TS1433.
- Lazy object merge instantiation, including the compiler-suite memory
  pathology above.
- Update-operand diagnostics (TS2356), including generic, enum, readonly, and
  precedence behavior.
- Binary-operator compatibility diagnostics (TS2365).
- Missing newer-library global diagnostics and lexical shadowing (TS2583).
- Qualified namespace ownership, generic arity, assertion, implements, and
  collision handling.
- Virtual-file module extension substitution, ambient wildcard modules, and
  exact source-file identity (TS2307/TS2792).
- A bounded expanded-baseline harness so parallel workers stream results in
  deterministic case order.

## Verification

- Conformance RAYON=1 and RAYON=4 JSON are byte-identical:
  `cd39b435fe6e5d241a63bd6859b5423f9c1d6465201c4b75fd8d16e432214382`.
- Compiler baseline RAYON=1 and RAYON=4 JSON are byte-identical:
  `ac52a8ea5b309b6b44cd67c987d5f53a0502ff95265422acc2326982c19f15d2`.
- Compiler current RAYON=1 and RAYON=4 JSON are byte-identical:
  `7cad5ec42ae43074bc3a84eabfdc46ad92dd7cd55271afcf7a92c4e62b2c7158`.
- Full `tsc_rs_parser`, `tsc_rs_types`, and `tsc_rs_harness` tests pass.
- All focused regression suites for this wave pass.
- The 1,078-file frozen application scan is byte-identical to the baseline:
  33 diagnostics, 5,833 normalized bytes, empty stderr, SHA-256
  `80982d416e7862614e4bc2fe36c9d2d80a176c1db2e458f8607929b43431f90b`.

## Known compiler-suite precision debt

The current compiler aggregate improves precision, but five diagnostic codes
gain false positives and require a focused cleanup pass:

- TS2304: `+39`, dominated by privacy-class and named-import fixtures.
- TS2322: `+29` net, dominated by privacy, dynamic-name, and getter fixtures.
- TS2365: `+3`; one JSX recovery wrong-code result and two genuine Temporal
  bigint arithmetic false positives.
- TS2576: `+1` recovery cascade in a broken generic recursive namespace case.
- TS2739: `+1` structural assignment false positive in `dynamicNames`.

False-positive reductions in TS2307, TS2345, TS2416, TS2693, and TS2792 offset
58 of those additions, leaving the compiler suite at `+15` net. The exact
additions are documented here so the aggregate improvement does not conceal
the remaining regressions.

## Next high-impact waves

1. **Virtual package manifests (TS2307).** Parse virtual `package.json` files
   once into immutable shared state, then implement tri-state
   `Resolved`/`Blocked`/`NoManifest` handling for `exports`, `imports`, package
   self-name, wildcards, `null`, and ordered import/require conditions. The
   measured target is `+8` matches and `-887` false positives. Keep
   `typesVersions`, package `main`, Classic resolution, and additional syntax
   forms out of this first wave.
2. **Interface heritage compatibility (TS2430).** Add a declaration-site,
   direct-override relation rather than tightening global assignability. The
   exact 183 expected diagnostics split into 35 ordinary-property, 8 indexer,
   10 private/protected nominal, 68 call-property, and 62
   constructor-property cases. Land the 53 ordinary/index/nominal cases first;
   add the 130 signature-property cases only with correct variance, arity,
   overload, and generic alpha-equivalence behavior.
3. **Compiler precision cleanup.** Remove the five per-code additions listed
   above before broadening qualified-namespace or binary-operator behavior.

Each future wave should keep the same gates: focused TypeScript-oracle
regressions, full parser/types/harness tests, deterministic RAYON=1/4 reports,
exhaustive per-code delta review, and a zero-addition frozen-application scan.
