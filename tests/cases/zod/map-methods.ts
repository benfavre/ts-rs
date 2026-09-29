// @expect: no-errors
// @hover got: User | undefined
// @hover entries: [string, User
// @hover values: User
// @hover keys: string
//
// Map<K,V>'s methods must propagate K/V through inference. Real LSP
// users routinely hover `users.get("x")` expecting `User | undefined`
// to surface so they can decide whether to `?.name` the result.

interface User {
  id: number;
  name: string;
}

const users = new Map<string, User>();

const got = users.get("alice");
const entries: [string, User] = [...users][0]!;
const values: User = [...users.values()][0]!;
const keys: string = [...users.keys()][0]!;

export { got, entries, values, keys };
