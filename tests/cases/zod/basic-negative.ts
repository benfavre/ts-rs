// @expect-error: 2322
// @expect-error: 2322
//
// Negative: parsing a string-schema MUST return string. Assigning that
// result to `number` should produce TS2322. Closed when the namespace
// import refactor + overload-as-intersection wiring landed — `z.string()`
// now picks the correct overload (ZodString) and `s.parse()`'s
// `core.output<this>` resolves through the inheritance chain.

import { z } from "zod";

const s = z.string();
const n = z.number();

const wrong1: number = s.parse("hello"); // string → number
const wrong2: string = n.parse(42);       // number → string

export { wrong1, wrong2 };
