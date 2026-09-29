// @expect: no-errors
// @hover sBuilder: zod
// @hover stringSchema: ZodString
// @hover parsed: string
//
// Hover on the namespace-imported `z` exposes its type signature.
// `z.string()` carries through overload selection to ZodString, and
// `.parse(...)` propagates the inferred output type.

import * as z from "zod";

const sBuilder = z;
const stringSchema = z.string();
const parsed = stringSchema.parse("hi");

export { sBuilder, stringSchema, parsed };
