// @expect: no-errors
// @hover first: string
// @hover second: number
// @hover name: string
// @hover age: number
// @hover rest: string
//
// Destructuring patterns — array, tuple, object — must propagate the
// element types into the bound identifiers. Without these, hover on
// `const [a, b] = pair` shows `any` for both.

const pair: [string, number] = ["alice", 30] as [string, number];
const [first, second] = pair;

const user = { name: "alice", age: 30 };
const { name, age } = user;

const arr: string[] = ["a", "b", "c"];
const [head, ...rest] = arr;

export { first, second, name, age, head, rest };
