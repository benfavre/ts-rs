// @expect: no-errors
// @hover thenResult: Promise<number>
// @hover catchResult: Promise<number>
// @hover finalResult: Promise
//
// `.then(fn)` chains should propagate the inferred return type through
// the Promise. Failing this case caused real `await foo().then(map)`
// patterns to surface as `Promise<any>` in hover.

const base: Promise<number> = Promise.resolve(42);
const thenResult = base.then((x) => x * 2);
const catchResult = base.catch((e) => 0);
const finalResult = base.finally(() => {});

export { thenResult, catchResult, finalResult };
