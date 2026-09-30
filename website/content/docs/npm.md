---
title: npm package
---
# npm package

`@bext-stack/tsc-rs` ships a prebuilt `tsc-rs` binary through npm, so a JavaScript project can use the compiler without a Rust toolchain.

```bash
npm install --save-dev @bext-stack/tsc-rs
```

:::note
The npm package is released separately from the source tree. A commit on `main` does not imply a new npm release, so the installed binary can be older than the numbers on this site. Check with `npx tsc-rs --version`.
:::

## Use as a CLI

```bash
npx tsc-rs --target es2022 --outDir dist src/index.ts
```

## Use from a script

The package exposes a helper that returns the path of the installed binary:

```js
import { resolveBinaryPath } from "@bext-stack/tsc-rs/lib/resolve.mjs";
import { spawn } from "node:child_process";

const tscrs = resolveBinaryPath();
spawn(tscrs, ["--noEmit", "src/index.ts"], { stdio: "inherit" });
```

From there you can start the [transpile pipe](/docs/pipe) or the [type-check daemon](/docs/check-pipe).

## Supported platforms

The binary comes from a per-platform optional dependency. npm, pnpm and yarn install only the one that matches the host.

| Platform | Architecture | Package | Status |
|---|---|---|---|
| Linux | x64 | `@bext-stack/tsc-rs-linux-x64` | Available |
| Linux | arm64 | `@bext-stack/tsc-rs-linux-arm64` | Planned |
| macOS | x64 | `@bext-stack/tsc-rs-darwin-x64` | Planned |
| macOS | arm64 | `@bext-stack/tsc-rs-darwin-arm64` | Planned |
| Windows | x64 | `@bext-stack/tsc-rs-win32-x64` | Planned |

On a platform without a prebuilt binary the shim fails with a clear error. [Build from source](/docs/installation) and use the override below.

## Override the binary

Set `TSCRS_PATH` to the absolute path of a `tsc-rs` binary you built yourself. The npm shim uses it instead of the packaged binary:

```bash
TSCRS_PATH=/path/to/ts-rs/target/release/tsc-rs npx tsc-rs --version
```

The bext server resolves the compiler the same way: `TSCRS_PATH` first, then `node_modules/.bin/tsc-rs` found by walking up from the working directory.
