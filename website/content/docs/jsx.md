---
title: JSX
---
# JSX

tsc-rs compiles JSX in `.tsx` and `.jsx` files. The `--jsx` flag, or `jsx` in `tsconfig.json`, selects what the output looks like: classic `React.createElement` calls, the automatic runtime, or JSX left in place for a later tool. This page shows the real output of version 0.4.1 for each mode and lists where it differs from `tsc`.

## Which files are parsed as JSX

The file extension decides, as in `tsc`:

| Extension | JSX | Notes |
|---|---|---|
| `.tsx`, `.jsx` | Parsed | Angle-bracket assertions such as `<number>value` are not available. Write `value as number` |
| `.ts`, `.js` | Not parsed | `<b>hi</b>` is a syntax error, for example TS1161 |

Generic arrow functions in a `.tsx` file need the usual hint: `<T,>(x: T) => x` or `<T extends unknown>(x: T) => x`. Both compile.

## Modes

| `--jsx` value | Output |
|---|---|
| `react` | `React.createElement(...)` calls |
| `react-jsx` | `_jsx(...)` and `_jsxs(...)` calls, with an import from `react/jsx-runtime` |
| `react-jsxdev` | `_jsxDEV(...)` calls with source positions, imported from `react/jsx-dev-runtime` |
| `preserve`, `react-native` | JSX kept as written, types removed |
| `none` | Same as `react`. See "Differences from tsc" at the end of this page |

Values are not case sensitive. `--help` does not list `react-jsxdev` or `react-native`, but both are accepted.

:::warning
When `--jsx` is absent, or its value is not one of the above, tsc-rs does not report an error. It behaves like `preserve` and writes JSX into the `.js` file. Always set the mode explicitly.
:::

The examples below compile this file, `card.tsx`:

```ts
interface CardProps {
  title: string;
  tags: string[];
}

export function Card({ title, tags }: CardProps) {
  return (
    <section className="card">
      <h2>{title}</h2>
      <>
        {tags.map((tag) => (
          <span key={tag}>{tag}</span>
        ))}
      </>
    </section>
  );
}
```

```bash
tsc-rs --transpileOnly --target es2022 --module esnext --jsx <mode> --outDir out card.tsx
```

### react

```js
export function Card({ title, tags }) {
    return (React.createElement("section", { className: "card" },
        React.createElement("h2", null, title),
        React.createElement(React.Fragment, null, tags.map((tag) => (React.createElement("span", { key: tag }, tag))))));
}
```

No import is added. `React` has to be in scope in the source file.

### react-jsx

```js
import { jsx as _jsx, jsxs as _jsxs, Fragment as _Fragment } from "react/jsx-runtime";
export function Card({ title, tags }) {
    return (_jsxs("section", { className: "card", children: [_jsx("h2", { children: title }), _jsx(_Fragment, { children: tags.map((tag) => (_jsx("span", { children: tag }, tag))) })] }));
}
```

With `--module commonjs` the import becomes `const jsx_runtime_1 = require("react/jsx-runtime");` and calls are written `(0, jsx_runtime_1.jsx)(...)`. A `key` placed after a spread, as in `<Item {...props} key="k" />`, falls back to `createElement` imported from `react`, as it does in `tsc`.

### react-jsxdev

```js
import { jsxDEV as _jsxDEV, Fragment as _Fragment } from "react/jsx-dev-runtime";
const _jsxFileName = "card.tsx";
```

Each call then carries its position, for example:

```js
_jsxDEV("h2", { children: title }, void 0, false, { fileName: _jsxFileName, lineNumber: 9, columnNumber: 7 }, this)
```

`_jsxFileName` is the path as the compiler received it. In a `-p` build that is an absolute path.

### preserve

```js
export function Card({ title, tags }) {
    return (<section className="card">
      <h2>{title}</h2>
      <>
        {tags.map((tag) => (<span key={tag}>{tag}</span>))}
      </>
    </section>);
}
```

The file is written as `card.js`, not `card.jsx`.

## Factory and import source

`--jsxFactory` replaces `React.createElement` in `react` mode. It has no effect in the automatic modes. Given `factory.tsx`:

```ts
import { h, Fragment } from "preact";

export const Badge = (props: { label: string }) => (
  <>
    <b class="badge">{props.label}</b>
  </>
);
```

```bash
tsc-rs --transpileOnly --target es2022 --module esnext --jsx react --jsxFactory h --outDir out factory.tsx
```

```js
import { h } from "preact";
export const Badge = (props) => (h(React.Fragment, null,
    h("b", { class: "badge" }, props.label)));
```

Fragments still compile to `React.Fragment`, and the now unused `Fragment` import is dropped. There is no `--jsxFragmentFactory` flag (it is rejected as an unknown flag). Set the fragment factory per file with a pragma instead.

`--jsxImportSource` changes the package the automatic runtime is imported from, in `react-jsx` and `react-jsxdev`:

```bash
tsc-rs --transpileOnly --target es2022 --module esnext --jsx react-jsx --jsxImportSource preact --outDir out factory.tsx
```

```js
import { jsx as _jsx, Fragment as _Fragment } from "preact/jsx-runtime";
```

### Pragmas

| Pragma | Effect |
|---|---|
| `/** @jsx h */` | Factory for this file |
| `/** @jsxFrag Fragment */` | Fragment factory for this file |
| `/** @jsxImportSource preact */` | Automatic runtime from `preact/jsx-runtime`. Also switches a `react` mode file to the automatic runtime |
| `/** @jsxRuntime automatic */` | Switches a `react` mode file to the automatic runtime |
| `/** @jsxRuntime classic */` | Not applied. The file stays on the automatic runtime under `react-jsx` |

## In tsconfig.json

`compilerOptions.jsx` is read and takes the same values as the flag. A `--jsx` flag on the command line overrides it.

```json
{
  "compilerOptions": {
    "target": "es2022",
    "module": "esnext",
    "jsx": "react-jsx",
    "outDir": "dist"
  },
  "include": ["src/**/*.tsx"]
}
```

:::warning
In 0.4.1, `jsxFactory`, `jsxFragmentFactory`, `jsxImportSource` and `reactNamespace` are ignored when they are set in `tsconfig.json`. A config with `"jsxImportSource": "preact"` still imports from `react/jsx-runtime`. Pass `--jsxFactory` and `--jsxImportSource` on the command line, or use a pragma.
:::

## Type checking

JSX is type-checked unless you pass `--transpileOnly`, and a checker error means no output is written. Two cases are worth knowing:

- With no `JSX` namespace in the program, every element reports TS7026 (`JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.`). tsc-rs reports it without `--strict` too.
- With `@types/react` 19.2.17 installed, a project build stops on a TS2314 error reported inside `@types/react/index.d.ts`. `tsc` 5.9 reports nothing there. `skipLibCheck` does not suppress it.

Until the second point is fixed, compile React projects with `--transpileOnly` and keep `tsc --noEmit` for type checking. See [Limitations](/docs/limitations).

## In the transpile pipe

[`tsc-rs --pipe`](/docs/pipe) takes the same settings per request, in `options`:

| Option | Default | Notes |
|---|---|---|
| `jsx` | `react` | Same values as `--jsx`. An unknown value leaves JSX in the output |
| `jsxFactory` | `React.createElement` | `react` mode only |
| `jsxImportSource` | `react` | Automatic modes only |

```json
{"id":"1","file":"a.tsx","source":"export const A = (p: { n: number }) => <b>{p.n}</b>;","options":{"module":"esnext","jsx":"react-jsx","jsxImportSource":"preact"}}
```

```json
{"id":"1","ok":true,"output":"import { jsx as _jsx } from \"preact/jsx-runtime\";\nexport const A = (p) => _jsx(\"b\", { children: p.n });\n","elapsed_ms":0,"exports":["A"]}
```

- The `file` extension selects the parser. `a.ts` or `a.js` with JSX in `source` returns `ok: false`. A request without `file` is parsed as TSX.
- A top-level `jsx` field on the request is used when `options.jsx` is absent.
- The `imports` list describes the source. The runtime import that tsc-rs adds (`react/jsx-runtime`) is not in it, so a bundler has to account for that module itself.
- `jsxFragmentFactory` is not an option; fragments use `React.Fragment` in `react` mode.

## Differences from tsc

Measured against `tsc` 5.9.3 on the examples above, and re-checked with a build of the public `main` branch on 30 September 2026:

| Area | tsc | tsc-rs 0.4.1 |
|---|---|---|
| No `--jsx` on a `.tsx` file | TS17004 | No error, JSX kept in the `.js` output |
| `--jsx none` | TS17004 | Compiles as `react` |
| Unknown `--jsx` value | Rejected | Accepted, behaves like `preserve` |
| `preserve` output file | `card.jsx` | `card.js` |
| `react-jsx` with no runtime module installed | TS2792 | No error |
| `react-jsx` import list | `jsx, Fragment, jsxs` | `jsx, jsxs, Fragment`. Same bindings, different order |
| `react-jsxdev` positions | Position before the whitespace that precedes a tag | Position of the tag itself, so `lineNumber` and `columnNumber` can differ |
| `@jsxRuntime classic` pragma | Honoured | Ignored |
| JSX options in `tsconfig.json` | All read | Only `jsx` |

`react` and `preserve` output for `card.tsx` is otherwise byte-identical to `tsc`.
