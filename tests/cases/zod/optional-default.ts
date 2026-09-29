// @expect: no-errors
//
// .optional() and .default() — the two most commonly-used modifiers in
// the repo. Verifying these chain correctly without errors.

import { z } from "zod";

const Schema = z.object({
  name: z.string(),
  age: z.number().optional(),
  role: z.enum(["admin", "user", "guest"]).default("user"),
  metadata: z.record(z.string(), z.unknown()).optional(),
});

export { Schema };
