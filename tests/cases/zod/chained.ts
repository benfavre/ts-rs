// @expect: no-errors
//
// Method chains preserving the schema's output type. Just exercising the
// builders — no `.parse()` here, so we don't depend on conditional types.

import { z } from "zod";

const s1 = z.string().min(1);
const s2 = z.string().min(1).max(100);
const s3 = z.string().email();
const s4 = z.string().regex(/^\d+$/);
const s5 = z.string().uuid().optional();

const n1 = z.number().int();
const n2 = z.number().int().positive();
const n3 = z.number().min(0).max(100);
const n4 = z.number().int().nullable();

const a1 = z.array(z.string());
const a2 = z.array(z.string()).min(1).max(10);
const a3 = z.array(z.object({ id: z.string() }));

export { s1, s2, s3, s4, s5, n1, n2, n3, n4, a1, a2, a3 };
