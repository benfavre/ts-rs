# zod compatibility test suite

Fixtures here exercise idiomatic zod usage so we can catch regressions in
how `tsc-rs` checks zod schemas, inferred types, and chained method calls.

The current regression run passes all **30 type-check fixture expectations**
and all **66 LSP assertions**. These totals include the explicit xfail
contracts described below; an unexpected improvement is an XPASS and still
fails the suite until its stale xfail marker is removed.

The cases run against **real zod** (v4) — the test discovers it under
`tests/cases/zod/node_modules/zod`. That path is a symlink to the zod
install in `/path/to/app/node_modules/zod` (set up manually because
this repo doesn't have its own node_modules).

```bash
# One-time setup (gitignored):
ln -s /path/to/app/node_modules/zod \
      ./tests/cases/zod/node_modules/zod
```

If the symlink is missing the tests are **skipped** (not failed), so CI
without a zod install still goes green.

## Layout

```
tests/cases/zod/
├── tsconfig.json                  # checked into git
├── node_modules/zod -> ...        # gitignored symlink
├── basic.ts                       # one case per file
├── infer.ts
├── objects.ts
├── ...
└── README.md                      # this file
```

## Header annotation

Each fixture starts with a comment block declaring what we expect:

```ts
// @expect: no-errors                      // case passes with zero diagnostics
// @expect-error: 2322 at line 12          // any TS2322 at line 12
// @expect-error: 2322                     // any TS2322, position doesn't matter
// @xfail: <reason>                        // test is allowed to fail today
```

Multiple `@expect-error` lines combine — the test fails if any expected
error is missing OR if extra errors appear that weren't declared.

The `@xfail` marker inverts the result: a fixture marked `@xfail` that
*still* passes is reported as XPASS (regression toward correctness),
which makes the test suite fail and prompts you to remove the marker.

### LSP assertions

Inline in the same fixture, alongside the diagnostic expectations:

```ts
// @hover  sv: string                       // hover on `sv` decl contains "string"
// @xhover s:  ZodString                    // expected today, but xfail
// @def    foo -> bar.ts:5:7                // go-to-def(foo) lands at bar.ts:5:7
// @xdef   ...                              // same, but xfail
```

Identifiers are located by scanning for `const`/`let`/`var`/`function`/
`type`/`interface` followed by the name — first match wins.

## Runners

- Type-check side: `crates/tsc_rs_harness/tests/zod_compat.rs`
- LSP side:        `crates/tsc_rs_harness/tests/zod_lsp.rs`

Each is its own `cargo test` binary so you can run them separately:

```bash
cargo test --release -p tsc_rs_harness --test zod_compat -- --nocapture
cargo test --release -p tsc_rs_harness --test zod_lsp     -- --nocapture
```

If the zod symlink is missing, both tests *skip* (not fail) with a
message telling you how to create it.

## Known gaps documented as xfail

| Fixture | What's expected | What's blocking |
|---|---|---|
| `basic-negative.ts` | `s.parse("x")` returns `string` | `core.output<this>` is conditional + indexed type, returns `unknown`/`any` |
| `infer.ts` | `z.infer<typeof S>` materializes the inferred shape | Same — `infer` is `core.output` re-exported |
| `safeParse.ts` | `result.success === true` narrows to `{ data: T }` | Discriminated union narrowing through inferred property type |
| `basic.ts` (LSP) | hover on `const s = z.string()` shows `ZodString` | Builder return type goes through generics, falls back to `any` |
| `infer.ts` (LSP)  | hover on `User` shows the inferred object shape | Same root cause as the type-check side |

When any of these checker features lands, the corresponding xfail
markers should start firing XPASS and need to be removed.
