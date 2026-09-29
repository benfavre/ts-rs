// @expect: no-errors
// @hover key: Key
// @hover name: string
// @hover age: number
// @hover record: Record<string, number
//
// Indexed access types (`T[K]`) and `keyof` operator. Real users hover
// over `Object.keys()` results and TypedDict-style typed dictionaries.

interface User {
  name: string;
  age: number;
}

type Key = keyof User;
const key: Key = "name";

const user: User = { name: "alice", age: 30 };
const name: User["name"] = user.name;
const age: User["age"] = user.age;

const record: Record<string, number> = { x: 1 };

export { key, name, age, record };
