// @expect: no-errors
// @hover greet: string
// @hover counter: Counter
// @hover hooked: number
//
// Class methods + return-type inference. Replaces the canonical
// `useState`-like dance with a tiny `Counter` class to keep the
// fixture dependency-free; the user-facing hover behaviour is the
// same shape.

class Counter {
  private count: number = 0;
  increment(): number {
    this.count += 1;
    return this.count;
  }
  greet(): string {
    return `count is ${this.count}`;
  }
}

const counter = new Counter();
const hooked: number = counter.increment();
const greet: string = counter.greet();

export { counter, hooked, greet };
