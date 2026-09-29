// @expect: no-errors
// @hover step: Step
// @hover greeting: "hi"
// @hover all: Step[]
//
// Test that string literals coexist with their literal-union aliases.
// `let x: Step = 'a'` is the canonical "narrow literal preserved by
// annotation" pattern — without contextual typing, `'a'` widens to
// `string` and the assignment fails (TS2322 string vs Step).

type Step = 'a' | 'b' | 'c';

let step: Step = 'a';
step = 'b';

const greeting = 'hi' as const;
const all: Step[] = ['a', 'b', 'c'];

export { step, greeting, all };
