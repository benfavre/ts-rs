// @expect: no-errors
// @hover doubled: number
// @hover names: string
// @hover total: number
// @hover first: User
//
// Array method-chain hover. Real-world bread-and-butter LSP query: when
// I hover the bound variable after `.map(...).filter(...).reduce(...)`,
// I want to see the propagated element type, not `any` and not a raw
// callback signature.

interface User {
  id: number;
  name: string;
}

const users: User[] = [
  { id: 1, name: "alice" },
  { id: 2, name: "bob" },
];

// `.map(u => u.name)` projects to string[]; index access gives string.
const names: string = users.map((u) => u.name)[0]!;

// `.map(u => u.id * 2)` projects to number[]; `.filter(...).reduce(...)`
// also lands at number. Test the post-reduce variable.
const doubled: number = users.map((u) => u.id).filter((x) => x > 0)[0]!;
const total: number = [1, 2, 3].reduce((a, b) => a + b, 0);

// `.find(...)` returns `User | undefined`; non-null assertion → User.
const first: User = users.find((u) => u.id === 1)!;

export { names, doubled, total, first };
