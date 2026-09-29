// @expect: no-errors
// @hover takeUser: function
// @hover greeting: string
// @hover summed: number
//
// Function-parameter hover via the `name` token on the call site,
// plus inferred return-type propagation. Closures over generics
// (`reducer`) must thread `T` through the parameter typing.

function takeUser(u: { name: string }): string {
  return `hello ${u.name}`;
}

const greeting = takeUser({ name: "alice" });

function reducer<T>(arr: T[], seed: T, fn: (acc: T, cur: T) => T): T {
  return arr.reduce(fn, seed);
}

const summed = reducer([1, 2, 3], 0, (a, b) => a + b);

export { takeUser, greeting, summed };
