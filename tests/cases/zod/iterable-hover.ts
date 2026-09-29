// @expect: no-errors
// @hover unique: string
// @hover keys: string
// @hover pairs: [string, number
// @hover typeOf: "string" | "number"
//
// Spread of Set/Map should unwrap the element type and the typeof
// operator should return the canonical 8-label union (not plain `string`).

const arr = ["a", "b", "a", "c"];
const unique: string = [...new Set(arr)][0]!;

const map = new Map<string, number>([["x", 1], ["y", 2]]);
const keys: string = [...map.keys()][0]!;
const pairs: [string, number] = [...map][0]!;

function describe(x: unknown): "string" | "number" {
  const typeOf = typeof x;
  if (typeOf === "string") return "string";
  return "number";
}

export { unique, keys, pairs, describe };
