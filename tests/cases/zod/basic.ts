// @expect: no-errors
// @hover sv: string
// @hover nv: number
// @hover s:  ZodString
// @hover n:  ZodNumber
//
// Smoke test: import zod and call the most basic builders. Hovering on
// the builder-call results now exposes the actual schema type — the
// namespace-import refactor + overload selection ship the call's real
// return type through `expression_types`.

import { z } from "zod";

const s = z.string();
const n = z.number();
const b = z.boolean();
const d = z.date();
const u = z.undefined();
const nu = z.null();
const a = z.any();
const un = z.unknown();

// .parse returns the schema's output type.
const sv: string = s.parse("hello");
const nv: number = n.parse(42);
const bv: boolean = b.parse(true);

// Use the values to keep the linter happy.
export { s, n, b, d, u, nu, a, un, sv, nv, bv };
