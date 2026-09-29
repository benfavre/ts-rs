#!/usr/bin/env node
"use strict";

const { existsSync, mkdirSync, copyFileSync, chmodSync } = require("fs");
const { join, dirname } = require("path");

const PLATFORMS = {
  "linux-x64": "@tsc-rs/linux-x64",
  "linux-arm64": "@tsc-rs/linux-arm64",
  "darwin-x64": "@tsc-rs/darwin-x64",
  "darwin-arm64": "@tsc-rs/darwin-arm64",
  "win32-x64": "@tsc-rs/win32-x64",
};

const platform = `${process.platform}-${process.arch}`;
const pkgName = PLATFORMS[platform];

if (!pkgName) {
  console.error(
    `tsc-rs: unsupported platform ${platform}. ` +
      `Supported: ${Object.keys(PLATFORMS).join(", ")}`
  );
  process.exit(1);
}

let pkgPath;
try {
  pkgPath = dirname(require.resolve(`${pkgName}/package.json`));
} catch {
  console.error(
    `tsc-rs: could not find ${pkgName}. ` +
      `Make sure optional dependencies are installed (don't use --no-optional).`
  );
  process.exit(1);
}

const ext = process.platform === "win32" ? ".exe" : "";
const src = join(pkgPath, `tsc-rs${ext}`);
const dest = join(__dirname, "bin", `tsc-rs${ext}`);

if (!existsSync(src)) {
  console.error(`tsc-rs: binary not found at ${src}`);
  process.exit(1);
}

mkdirSync(dirname(dest), { recursive: true });
copyFileSync(src, dest);
if (process.platform !== "win32") {
  chmodSync(dest, 0o755);
}
