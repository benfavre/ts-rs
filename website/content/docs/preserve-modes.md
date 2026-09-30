---
title: Preserve modes
---
# Preserve modes

`tsc` removes all type information when it emits JavaScript. tsc-rs can keep some of it in the output, for tools that run after the compiler: runtime type libraries, documentation generators and refactoring tools.

There are three independent modes.

## Type annotations

`--preserveTypeAnnotations` keeps `as` expressions, `satisfies` expressions and angle-bracket assertions in the emitted code.

Input, `cast.ts`:

```ts
const raw = JSON.parse("{}") as { port: number };
const config = { port: raw.port, host: "localhost" } satisfies Record<string, string | number>;
export const port = <number>config.port;
```

Default output:

```bash
tsc-rs --target es2022 --module esnext cast.ts
```

```js
const raw = JSON.parse("{}");
const config = { port: raw.port, host: "localhost" };
export const port = config.port;
```

With the flag:

```bash
tsc-rs --target es2022 --module esnext --preserveTypeAnnotations cast.ts
```

```ts
const raw = JSON.parse("{}") as { port: number };
const config = { port: raw.port, host: "localhost" } satisfies Record<string, string | number>;
export const port = <number>config.port;
```

:::warning
Output that keeps type syntax is not runnable JavaScript. It is meant as input for another tool, not for a JavaScript engine.
:::

## Comments

`--preserveComments` keeps comments even when `removeComments` is set. It lets a project keep `removeComments` in a shared config and still produce commented output for a documentation or analysis pass.

`removeComments` is a `tsconfig.json` option; there is no `--removeComments` command line flag. With this config:

```json
{
  "compilerOptions": {
    "target": "es2022",
    "module": "esnext",
    "removeComments": true
  },
  "include": ["*.ts"]
}
```

`tsc-rs -p tsconfig.json` strips the comments:

```js
export const a = 1;
export function f() { }
```

and `tsc-rs -p tsconfig.json --preserveComments` keeps them:

```js
// leading note
export const a = 1; // trailing
/** doc */
export function f() { }
```

Setting `"preserveComments": true` in the config has the same effect as the flag.

## Whitespace

`--preserveWhitespace` keeps the original layout of the source where it can. This mode is partial: statements that the compiler has to transform are still re-printed.

```bash
tsc-rs --preserveWhitespace file.ts
```

## In tsconfig.json

All three are also accepted as `compilerOptions`: `preserveTypeAnnotations`, `preserveComments` and `preserveWhitespace`. See [tsconfig support](/docs/tsconfig).

## Related: fast emit

`--fast-emit` is a different trade. It produces plain JavaScript, but copies statements that need no transform verbatim from the source instead of normalizing their formatting the way `tsc` would. Combined with `--transpileOnly` it runs two to four times faster than the structured path, and is intended for consumers that feed the result straight to a JavaScript engine.
