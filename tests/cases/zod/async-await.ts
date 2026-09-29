// @expect: no-errors
// @hover awaited: User
// @hover pending: Promise<User>
// @hover mapped: User
//
// async/await unwrap: `await fetchUser()` strips `Promise<User>` to `User`.
// Without proper Promise<T> awareness, the bound variable hovers as
// `Promise<User>` and every downstream property access breaks.

interface User {
  id: number;
  name: string;
}

async function fetchUser(): Promise<User> {
  return { id: 1, name: "alice" };
}

const pending = fetchUser();

async function main(): Promise<User> {
  const awaited = await fetchUser();
  // Inside async function: `.then` returns `Promise<User>`; chaining
  // `await` would unwrap again. Direct member access also works on
  // the awaited binding.
  const mapped: User = awaited;
  return mapped;
}

export { main, pending };
