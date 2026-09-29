// @expect: no-errors
// @hover circleArea: number
// @hover circle: Shape
// @hover narrowed: number
//
// Discriminated-union narrowing: after `if (s.kind === "circle") { ... }`,
// hover inside the branch must see only the matching arm — `s.radius`
// would be a TS2339 if narrowing didn't kick in.

type Shape =
  | { kind: "circle"; radius: number }
  | { kind: "square"; side: number };

function area(s: Shape): number {
  if (s.kind === "circle") {
    // Inside the narrowed branch, `s.radius` is number.
    const narrowed: number = s.radius;
    return Math.PI * narrowed * narrowed;
  }
  return s.side * s.side;
}

const circle: Shape = { kind: "circle", radius: 4 };
const circleArea: number = area(circle);

export { area, circle, circleArea };
