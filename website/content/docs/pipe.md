---
title: Transpile pipe
---
# Transpile pipe

`tsc-rs --pipe` is a persistent, transpile-only process. It reads one JSON request per line on stdin and writes one JSON response per line on stdout. A bundler or runtime keeps it alive and sends it modules, instead of paying process start-up for every file.

It does not type-check. For that, see [Type-check daemon](/docs/check-pipe).

## Start

```bash
tsc-rs --pipe
```

The process prints a banner once it is ready to accept requests:

```json
{"ready":true,"pid":1263400}
```

## Request

```json
{
  "id": "1",
  "file": "page.tsx",
  "source": "export const A = (p: { n: number }) => <b>{p.n}</b>;",
  "options": {
    "target": "es2022",
    "module": "esnext",
    "jsx": "react-jsx",
    "sourceMap": false
  }
}
```

Each request must be a single line. It is pretty-printed here for reading.

| Field | Meaning |
|---|---|
| `id` | Echoed back on the response |
| `file` | File name. The extension selects TypeScript or TSX parsing |
| `source` | Source text |
| `options` | Compile options, see below |
| `pkg_json_type` | The `type` of the nearest `package.json`. `"module"` makes a `.js` file ESM |

When `options` is absent, flat `jsx` and `module` fields on the request are accepted instead.

### Options

| Option | Meaning |
|---|---|
| `target` | ECMAScript target, for example `es2022` |
| `module` | Module system, for example `commonjs` or `esnext` |
| `jsx` | JSX mode: `react`, `react-jsx`, `react-jsxdev`, `preserve`. See [JSX](/docs/jsx) |
| `jsxImportSource` | Import source for the automatic JSX runtime |
| `jsxFactory` | Factory function for the classic JSX runtime |
| `sourceMap` | Include a source map in the response |
| `importHelpers` | Import emit helpers from `tslib` instead of inlining them |
| `removeComments` | Strip comments |
| `useDefineForClassFields` | Emit class fields with define semantics |
| `fastEmit` | Skip cosmetic formatting normalization. Same as `--fast-emit` |

## Response

```json
{
  "id": "1",
  "ok": true,
  "output": "import { jsx as _jsx } from \"react/jsx-runtime\";\nexport const A = (p) => _jsx(\"b\", { children: p.n });\n",
  "elapsed_ms": 0,
  "exports": ["A"]
}
```

| Field | Meaning |
|---|---|
| `ok` | `true` when the module compiled |
| `output` | Emitted JavaScript |
| `elapsed_ms` | Time spent on this request |
| `imports` | Static imports, each with a `specifier` and a `kind` |
| `dynamic_imports` | Specifiers of `import()` calls |
| `exports` | Exported names |
| `sourceMap` | Source map, when requested |
| `error` | Present when `ok` is `false` |

Empty lists are omitted. The import and export lists let the caller build a module graph without parsing the output again. They describe the source: a runtime import that the compiler adds, such as `react/jsx-runtime`, is not listed.

### Errors

A source file that does not parse is an error, with the position and the TypeScript code:

```json
{"id":"1","ok":false,"error":"a.ts:1:19: TS1109: Expression expected.","elapsed_ms":0}
```

## Batches

Send several modules in one request. They are compiled in parallel:

```json
{"id":"batch1","batch":[{"file":"a.ts","source":"..."},{"file":"b.ts","source":"..."}]}
```

```json
{"id":"batch1","ok":true,"results":[{"file":"a.ts","ok":true,"output":"..."},{"file":"b.ts","ok":true,"output":"..."}]}
```

## Shutdown

```json
{"id":"9","cmd":"shutdown"}
```

The process answers `{"id":"9","ok":true,"shutdown":true}` and exits. Closing stdin also ends it.

## Lifetime caps

A long-lived process should be recyclable. The pipe exits cleanly, after answering the current request, when either cap is reached, so a supervisor can start a fresh one:

| Variable | Default | Effect |
|---|---|---|
| `TSC_RS_PIPE_MAX_REQUESTS` | 5000 | Exit after this many requests. `0` disables |
| `TSC_RS_PIPE_MAX_MEMORY_MB` | 2048 | Exit when resident memory passes this value. `0` disables |

Your supervisor must treat an exit as normal and respawn.

## Example: Node

```js
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const child = spawn("tsc-rs", ["--pipe"]);
const lines = createInterface({ input: child.stdout });
const pending = new Map();
let nextId = 0;

lines.on("line", (line) => {
  const msg = JSON.parse(line);
  if (msg.ready) return;
  const resolve = pending.get(msg.id);
  pending.delete(msg.id);
  if (resolve) resolve(msg);
});

export function transpile(file, source) {
  const id = String(++nextId);
  const options = { target: "es2022", module: "esnext", jsx: "react-jsx" };
  child.stdin.write(JSON.stringify({ id, file, source, options }) + "\n");
  return new Promise((resolve) => pending.set(id, resolve));
}
```

## Who uses it

The pipe is how the [bext](https://bext.dev) engine compiles PRISM routes: a pool of `tsc-rs --pipe` workers transforms each module on first use, and the results are cached.
