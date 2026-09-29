// Resolve the absolute path of the platform-matched tsc-rs binary.
//
// Per-platform packages (`@bext-stack/tsc-rs-linux-x64`, `@bext-stack/tsc-rs-darwin-arm64`,
// etc.) are listed under `optionalDependencies`. npm/pnpm/yarn skip the ones
// whose `os` + `cpu` fields don't match the host, so only the right one
// physically lands in node_modules.
//
// Each per-platform package ships exactly one file: `bin/tsc-rs`. We
// `require.resolve` that path and return it. If no platform package
// installed (unsupported OS/arch), throw with a helpful message.

import { createRequire } from "node:module";
import { existsSync } from "node:fs";

const require = createRequire(import.meta.url);

const PLATFORM_KEY = `${process.platform}-${process.arch}`;
const PACKAGE_FOR_PLATFORM = {
  "linux-x64": "@bext-stack/tsc-rs-linux-x64",
  "linux-arm64": "@bext-stack/tsc-rs-linux-arm64",
  "darwin-x64": "@bext-stack/tsc-rs-darwin-x64",
  "darwin-arm64": "@bext-stack/tsc-rs-darwin-arm64",
  "win32-x64": "@bext-stack/tsc-rs-win32-x64",
};

export function resolveBinaryPath() {
  // 1. If the consumer set TSCRS_PATH, honor it. Same env var bext-turbopack's
  //    `find_tscrs_binary()` reads — keeps a single override knob across the
  //    Rust + JS sides.
  if (process.env.TSCRS_PATH && existsSync(process.env.TSCRS_PATH)) {
    return process.env.TSCRS_PATH;
  }

  const pkg = PACKAGE_FOR_PLATFORM[PLATFORM_KEY];
  if (!pkg) {
    throw new Error(
      `@bext-stack/tsc-rs: unsupported platform ${PLATFORM_KEY}. ` +
        `Supported: ${Object.keys(PACKAGE_FOR_PLATFORM).join(", ")}. ` +
        `Set TSCRS_PATH to a locally-built binary as a workaround.`,
    );
  }

  let binaryPath;
  try {
    binaryPath = require.resolve(`${pkg}/bin/tsc-rs`);
  } catch (e) {
    throw new Error(
      `@bext-stack/tsc-rs: ${pkg} not found in node_modules. ` +
        `It's listed as an optionalDependency — re-run \`npm install\` (or pnpm/yarn) ` +
        `with the right package manager so optional deps are evaluated. ` +
        `Original error: ${e.message}`,
    );
  }

  if (!existsSync(binaryPath)) {
    throw new Error(`@bext-stack/tsc-rs: ${pkg} resolved to ${binaryPath} but the file is missing.`);
  }
  return binaryPath;
}
