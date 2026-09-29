// @expect: no-errors
// @def myValue -> def-targets.ts:14:7
// @def helper  -> def-targets.ts:10:10
//
// Smoke test for go-to-definition. Both targets are local to this
// file. The harness scans for `const <ident>` / `function <ident>`
// and asks the LSP for the def at that offset; a working def should
// land at the same identifier (selfref OK on declaration sites).

function helper(): number {
  return 1;
}

const myValue = helper();

export { myValue, helper };
