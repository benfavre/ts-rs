// @expect: no-errors
//
// Discriminated unions, enums, literal unions — the patterns the repo
// uses for things like `status: 'pending' | 'active'`.

import { z } from "zod";

const StatusEnum = z.enum(["pending", "active", "archived"]);

const Tag = z.union([
  z.literal("urgent"),
  z.literal("normal"),
  z.literal("low"),
]);

const Outcome = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("ok"), value: z.number() }),
  z.object({ kind: z.literal("err"), reason: z.string() }),
]);

export { StatusEnum, Tag, Outcome };
