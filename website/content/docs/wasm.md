---
title: WebAssembly
---
# WebAssembly

The `tsc_rs_wasm` crate compiles the tsc-rs parser, binder, type checker and emitter to WebAssembly. It runs in a browser, in a worker, at the edge, or anywhere else that can instantiate a Wasm module. The [playground](/playground) on this site is that build.

## API

Three functions are exported.

```ts
function compile(source: string, options?: CompileOptions): CompileResult;
function validate(source: string, options?: CompileOptions): Diagnostic[];
function version(): string;
```

- `compile` parses and emits. It returns the JavaScript and the **parse** diagnostics. It does not type-check.
- `validate` parses, binds and type-checks one file, and returns all diagnostics. It does not emit.
- `version` returns the tsc-rs version.

### CompileOptions

| Option | Type | Meaning |
|---|---|---|
| `target` | string | ECMAScript target: `es5`, `es2015` to `es2025`, `esnext`. Default `esnext` |
| `module` | string | `commonjs`, `amd`, `umd`, `system`, `es2015`, `es2020`, `es2022`, `esnext`, `node16`, `nodenext` |
| `jsx` | string | `react`, `react-jsx`, `react-jsxdev`, `preserve` |
| `sourceMap` | boolean | Return a source map |
| `declaration` | boolean | Return a `.d.ts` |
| `strict` | boolean | Enable strict mode |
| `fileName` | string | Name used for diagnostics and source maps. Default `input.ts`. Use a `.tsx` name for JSX |

### CompileResult

```ts
interface CompileResult {
  js: string;
  sourceMap?: string;
  declaration?: string;
  diagnostics: Diagnostic[];
}

interface Diagnostic {
  message: string;
  line: number;
  column: number;
  severity: string;
  code?: number;
}
```

## Usage

```js
import init, { compile, validate, version } from "./pkg/tsc-rs-wasm.js";

await init();

const result = compile("const x: number = 42;", { target: "es2020" });
console.log(result.js);

const diagnostics = validate('const x: number = "oops";', { strict: true });
console.log(diagnostics);
// [{ message: "Type 'string' is not assignable to type 'number'.", line: 1, column: 7, severity: "error", code: 2322 }]
```

`init()` fetches and instantiates `tsc-rs-wasm_bg.wasm` from next to the JavaScript file. Serve the `.wasm` file with the `application/wasm` content type.

## Build

You need [wasm-pack](https://rustwasm.github.io/wasm-pack/) and the Wasm target:

```bash
cargo install wasm-pack
rustup target add wasm32-unknown-unknown
```

The crate is not yet listed in the workspace `members`, so Cargo refuses to build it in place. Add it to the `members` array in the root `Cargo.toml`:

```toml
[workspace]
members = [
  # ...
  "crates/tsc_rs_wasm",
]
```

Then build:

```bash
cd crates/tsc_rs_wasm
wasm-pack build --target web --out-dir pkg --out-name tsc-rs-wasm --no-opt
```

`--no-opt` skips the bundled `wasm-opt` pass, which rejects the bulk-memory operations that current Rust emits. To shrink the binary, run `wasm-opt` yourself with the newer features enabled:

```bash
wasm-opt pkg/tsc-rs-wasm_bg.wasm -o pkg/tsc-rs-wasm_bg.wasm -Os --all-features
```

The result is about 4.5 MB, or about 1.6 MB with gzip.

## What it does not do

- **One file at a time.** There is no file system and no module resolution, so imports are not followed.
- **No full standard library.** Type checking uses the built-in fallback declarations, not the complete `lib.d.ts`, and no `@types` packages. Code that depends on library or framework types can report errors the command line compiler would not.
- **Not on npm.** The package is not published; build it and ship the `pkg` directory with your application.

## Content Security Policy

Instantiating WebAssembly needs `'wasm-unsafe-eval'` in the `script-src` directive of a strict Content Security Policy.
