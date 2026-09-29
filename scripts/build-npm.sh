#!/usr/bin/env bash
# Build the platform-matched tsc-rs binary and stage it into the
# `npm/@bext-stack/tsc-rs-<platform>/bin/` directory so the per-platform
# package can be packed/published.
#
# Usage:
#   scripts/build-npm.sh              # detect platform, stage current host
#   scripts/build-npm.sh --all        # cross-compile (requires Zig/cross)
#
# After staging, run:
#   cd npm/@bext-stack/tsc-rs-<platform> && npm publish
#   cd npm/@bext-stack/tsc-rs && npm publish
#
# Both packages must be at the same version (see optionalDependencies
# pin in npm/@bext-stack/tsc-rs/package.json).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Detect host platform → npm package name segment.
detect_platform() {
  local os arch
  case "$(uname -s)" in
    Linux)  os=linux ;;
    Darwin) os=darwin ;;
    MINGW*|MSYS*|CYGWIN*) os=win32 ;;
    *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64)  arch=x64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
  esac
  echo "${os}-${arch}"
}

PLATFORM="$(detect_platform)"
BINARY_NAME="tsc-rs"
[ "${PLATFORM%%-*}" = "win32" ] && BINARY_NAME="tsc-rs.exe"

PKG_DIR="$ROOT/npm/@bext-stack/tsc-rs-${PLATFORM}"
if [ ! -d "$PKG_DIR" ]; then
  echo "no per-platform package for ${PLATFORM} at ${PKG_DIR}" >&2
  echo "create one based on npm/@bext-stack/tsc-rs-linux-x64/" >&2
  exit 1
fi

echo "→ Building tsc-rs (release) for ${PLATFORM}…"
cargo +nightly build --release --bin tsc-rs

mkdir -p "$PKG_DIR/bin"
cp "$ROOT/target/release/${BINARY_NAME}" "$PKG_DIR/bin/${BINARY_NAME}"
chmod 755 "$PKG_DIR/bin/${BINARY_NAME}"

echo "→ Staged $(du -h "$PKG_DIR/bin/${BINARY_NAME}" | cut -f1) at $PKG_DIR/bin/${BINARY_NAME}"
echo "→ Next: cd $PKG_DIR && npm publish --access public"
echo "       cd $ROOT/npm/@bext-stack/tsc-rs && npm publish --access public"
