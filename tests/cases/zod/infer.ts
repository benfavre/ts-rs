// @expect-error: 2322
// @hover User: name: string
//
// `z.infer<typeof Schema>` is THE central zod idiom in this repo
// (every TRPC router uses it). It walks `Schema._zod.output` via a
// conditional type. The negative case below assigns a wrong primitive
// to the inferred alias — that mismatch must surface.
//
// Hover shows the inferred object shape after mapped-type evaluation
// in display mode (see `simplify_type_for_display`).

import { z } from "zod";

const UserSchema = z.object({
  name: z.string(),
  age: z.number(),
});

type User = z.infer<typeof UserSchema>;

// Should error: { name: string; age: number } not assignable to number.
const wrong: number = { name: "alice", age: 30 } as User;

export { wrong };
