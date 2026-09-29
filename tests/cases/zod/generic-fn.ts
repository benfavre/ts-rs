// @expect: no-errors
// @hover idResult: number
// @hover wrapResult: value: string
// @hover pairResult: [string, number
//
// Generic function call inference. Hover on the bound variable must
// reflect the inferred return type, not the declared `T` placeholder.
// Real TS would surface `number`, `{ value: string }`, `[string, number]`.

function id<T>(x: T): T {
  return x;
}

function wrap<T>(value: T): { value: T } {
  return { value };
}

function pair<A, B>(a: A, b: B): [A, B] {
  return [a, b];
}

const idResult = id(42);
const wrapResult = wrap("hello");
const pairResult = pair("x", 7);

export { idResult, wrapResult, pairResult };
