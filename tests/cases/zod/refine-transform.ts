// @expect: no-errors
//
// .refine() and .transform() — used a lot for "must be a valid email",
// "string -> normalized lowercase", "uuid format", etc.

import { z } from "zod";

const Email = z.string().email().refine(
  (val) => val.includes("@"),
  { message: "must contain @" }
);

const Normalized = z.string().transform((val) => val.trim().toLowerCase());

const Slug = z.string().regex(/^[a-z0-9-]+$/).transform((s) => s.toLowerCase());

export { Email, Normalized, Slug };
