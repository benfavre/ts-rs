---
title: npm package
---
# npm package

`@bext-stack/tsc-rs` ships a prebuilt `tsc-rs` binary through npm, so a JavaScript project can use the compiler without a Rust toolchain.

```bash
npm install --save-dev @bext-stack/tsc-rs@0.4.2
```

:::note
The published npm package currently ships tsc-rs 0.4.2 on Linux x64. Check with `npx tsc-rs --version`. Source commits and package releases remain separate.
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

On a platform without an npm binary the shim fails with a clear error. Download the matching compiler from the [0.4.2 GitHub release](https://github.com/benfavre/ts-rs/releases/tag/v0.4.2), or [build from source](/docs/installation), and use the override below.

## Override the binary

Set `TSCRS_PATH` to the absolute path of a `tsc-rs` binary you built yourself. The npm shim uses it instead of the packaged binary:

```bash
TSCRS_PATH=/path/to/ts-rs/target/release/tsc-rs npx tsc-rs --version
```

The bext server resolves the compiler the same way: `TSCRS_PATH` first, then `node_modules/.bin/tsc-rs` found by walking up from the working directory.
