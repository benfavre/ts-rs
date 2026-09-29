#!/usr/bin/env node
// `tsc-rs` shim — resolves the platform-matched binary and execs it with
// the same args + stdio. Mirrors the pattern esbuild/swc/biome use.

import { spawn } from "node:child_process";
import { resolveBinaryPath } from "../lib/resolve.mjs";

let binaryPath;
try {
  binaryPath = resolveBinaryPath();
} catch (e) {
  console.error(e.message);
  process.exit(1);
}

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  // Don't change CWD — tsc-rs reads paths relative to the caller's CWD.
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 0);
  }
});

child.on("error", (e) => {
  console.error(`tsc-rs: failed to spawn ${binaryPath}: ${e.message}`);
  process.exit(1);
});
