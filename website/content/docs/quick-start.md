---
title: Quick start
---
# Quick start

This page compiles a file, then a project, then type-checks without writing anything. It assumes `tsc-rs` is on your `PATH`; see [Installation](/docs/installation).

## Compile one file

Create `queue.ts`:

```ts
export enum Priority {
  Low,
  High,
}

interface Job<T> {
  id: number;
  payload: T;
  priority: Priority;
}

export class Queue<T> {
  private jobs: Job<T>[] = [];

  push(payload: T, priority = Priority.Low): Job<T> {
    const job: Job<T> = { id: this.jobs.length + 1, payload, priority };
    this.jobs.push(job);
    return job;
  }

  next(): Job<T> | undefined {
    return this.jobs.shift();
  }
}
```

Compile it:

```bash
tsc-rs --strict --target es2022 --module esnext --outDir out queue.ts
```

`out/queue.js` now contains:

```js
export var Priority;
(function (Priority) {
    Priority[Priority["Low"] = 0] = "Low";
    Priority[Priority["High"] = 1] = "High";
})(Priority || (Priority = {}));
export class Queue {
    jobs = [];
    push(payload, priority = Priority.Low) {
        const job = { id: this.jobs.length + 1, payload, priority };
        this.jobs.push(job);
        return job;
    }
    next() {
        return this.jobs.shift();
    }
}
```

## Compile a project

Generate a starter `tsconfig.json`, then compile everything it includes:

```bash
tsc-rs --init
tsc-rs -p tsconfig.json
```

Add `-w` to recompile on change:

```bash
tsc-rs -w -p tsconfig.json
```

:::warning
With `outDir`, tsc-rs 0.4.1 writes every file directly into that directory and does not recreate source subdirectories. For nested sources, leave `outDir` unset so each `.js` file lands next to its source. [Projects and watch](/docs/projects) covers this and the other project build differences.
:::

See [tsconfig support](/docs/tsconfig) for the options tsc-rs applies.

## Type-check only

```bash
tsc-rs --noEmit --strict src/index.ts
```

Diagnostics use the `tsc` format and codes, and the process exits with status 1. For a file with two mistakes:

```bash
user.ts(7,27): error TS2339: Property 'nmae' does not exist on type 'User'.
user.ts(10,7): error TS2322: Type 'string' is not assignable to type 'number'.

Found 2 errors in 1 file.
```

When the checker reports errors, tsc-rs writes no output files. This differs from `tsc`, which emits by default.

:::warning
The type checker is incomplete. It misses some errors that `tsc` reports and reports some that `tsc` does not. The [conformance report](/conformance) quantifies both.
:::

## Transpile without checking

When you only need JavaScript, skip the checker:

```bash
tsc-rs --transpileOnly --outDir out src/index.ts
```

Add `--fast-emit` to also skip cosmetic formatting normalization. The output is valid JavaScript but is not formatted the way `tsc` would format it. This is the mode bundlers and runtimes want.

## Next

- [CLI reference](/docs/cli)
- [Preserve modes](/docs/preserve-modes)
- [Language server](/docs/language-server)
