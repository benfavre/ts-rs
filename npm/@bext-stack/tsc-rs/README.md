# @bext-stack/tsc-rs

`tsc-rs` is a TypeScript compiler in Rust. It implements the parts of `tsc`
needed by bext's PRISM dispatcher (parse, typecheck, JSX → JS emit, source
maps), plus a long-running stdin/stdout pipe API used for per-module
transforms during PRISM compile.

This package ships prebuilt binaries for major platforms via per-platform
`optionalDependencies`. npm/pnpm/yarn install only the one matching your
host's `process.platform`/`process.arch`.

## Compatibility and validation

The source compiler is still progressing toward TypeScript compatibility. The
2026-09-08 constructor-overload wave matches all 11,420 default JavaScript
baselines with an oracle (1,016 cases have no JavaScript oracle). Diagnostic
baselines pass 8,035 of 12,436 cases; 4,401 remain mismatched. Expanded option
variants and declaration output are not yet at parity.

See the [repository README](https://github.com/benfavre/ts-rs#status-at-a-glance)
and [verified waves](https://github.com/benfavre/ts-rs/blob/main/docs/verified-waves.md)
for validation and performance measurements. These figures describe the source
wave; installed npm binaries depend on the published package version. No npm
release is implied by a source commit.

## Install

```sh
npm install @bext-stack/tsc-rs
```

## Use as a CLI

```sh
npx tsc-rs --target es2022 --outDir dist src/index.ts
```

## Use programmatically

```js
import { resolveBinaryPath } from "@bext-stack/tsc-rs/lib/resolve.mjs";
import { spawn } from "node:child_process";

const tscrs = resolveBinaryPath();
const child = spawn(tscrs, ["--noEmit", "src/index.ts"], { stdio: "inherit" });
```

bext-server's `find_tscrs_binary()` walks up from CWD looking for
`./node_modules/.bin/tsc-rs` first — installing this package makes the
binary discoverable without the `TSCRS_PATH` env var or hardcoded
`$HOME/ts-rs/target/release/tsc-rs` paths.

## Override

If you have a locally-built binary you want to use instead, set
`TSCRS_PATH` to its absolute path. Both the npm shim (`bin/tsc-rs.mjs`)
and bext-server's Rust resolver respect it.

## Supported platforms

| platform | arch | package |
|---|---|---|
| linux | x64 | `@bext-stack/tsc-rs-linux-x64` |
| linux | arm64 | `@bext-stack/tsc-rs-linux-arm64` _(planned)_ |
| darwin | x64 | `@bext-stack/tsc-rs-darwin-x64` _(planned)_ |
| darwin | arm64 | `@bext-stack/tsc-rs-darwin-arm64` _(planned)_ |
| win32 | x64 | `@bext-stack/tsc-rs-win32-x64` _(planned)_ |

Other platforms throw a clear error with a `TSCRS_PATH` workaround. The
optionalDependencies list reserves the package names so the cross-platform
publish flow can drop a binary at the same name.

## License

MIT
