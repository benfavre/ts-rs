// @expect-error: 2322
//
// safeParse returns `{ success: true; data: T } | { success: false; error: ZodError }`.
// Narrowing on `result.success` should make `result.data` available as the
// inferred type. The negative case below claims `data` is a number when the
// schema produces strings — should error.

import { z } from "zod";

const s = z.string();
const result = s.safeParse("hello");

if (result.success) {
  // result.data: string
  const x: number = result.data;
  void x;
}

export { result };
