// @expect: no-errors
// @hover schema: ZodObject
// @hover optional: ZodOptional
// @hover withDefault: ZodDefault
//
// Hover on the result of a chained-call assignment must reach the
// schema-specific subclass, not collapse to `ZodType` / `ZodSchema`.
// Confirms that overload selection + `this` substitution survive across
// `z.object(...).optional()` and `z.something().default(x)`.

import { z } from "zod";

const schema = z.object({ name: z.string() });
const optional = z.string().optional();
const withDefault = z.number().default(0);

export { schema, optional, withDefault };
