#!/bin/bash
# Rebuild the playground's WebAssembly package from a tsc-rs checkout.
#
#   scripts/build-wasm.sh [path-to-ts-rs]     (default: ~/ts-rs-public)
#
# The tsc_rs_wasm crate is not a member of the ts-rs workspace, so it cannot be
# built in place. This script builds a scratch copy that points at the
# checkout's crates, leaving the checkout untouched, then optimizes the binary
# and installs it into public/playground/pkg.
#
# Needs: wasm-pack, the wasm32-unknown-unknown target, and wasm-opt (wasm-pack
# downloads one into ~/.cache/.wasm-pack on first use).
set -euo pipefail

REPO="$(cd "${1:-$HOME/ts-rs-public}" && pwd)"
SITE="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/Cargo.toml" | head -1)"
cp -r "$REPO/crates/tsc_rs_wasm" "$WORK/crate"
cd "$WORK/crate"
sed -i \
  -e "s|^version.workspace = true|version = \"$VERSION\"|" \
  -e 's|^edition.workspace = true|edition = "2021"|' \
  -e 's|^license.workspace = true|license = "MIT"|' \
  -e "s|path = \"\.\./|path = \"$REPO/crates/|g" Cargo.toml
printf '\n[workspace]\n\n[profile.release]\nopt-level = "s"\nlto = "thin"\ncodegen-units = 4\nstrip = true\n' >> Cargo.toml

# --no-opt: the bundled wasm-opt call rejects bulk-memory operations.
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/ts-rs-prism-wasm-target}" \
  nice -n 15 wasm-pack build --target web --out-dir "$WORK/pkg" --out-name tsc-rs-wasm --no-opt

WASM_OPT="$(command -v wasm-opt || find "$HOME/.cache/.wasm-pack" -name wasm-opt -type f | head -1)"
if [ -n "$WASM_OPT" ]; then
  "$WASM_OPT" "$WORK/pkg/tsc-rs-wasm_bg.wasm" -o "$WORK/pkg/opt.wasm" -Os --all-features
  mv "$WORK/pkg/opt.wasm" "$WORK/pkg/tsc-rs-wasm_bg.wasm"
fi

install -m 644 "$WORK/pkg/tsc-rs-wasm.js" "$SITE/public/playground/pkg/tsc-rs-wasm.js"
install -m 644 "$WORK/pkg/tsc-rs-wasm_bg.wasm" "$SITE/public/playground/pkg/tsc-rs-wasm_bg.wasm"
# Precompressed sidecars: the server picks them up when they are newer than the
# source, which saves compressing 4.5 MB on the fly.
node "$SITE/scripts/precompress.mjs" "$SITE/public/playground/pkg/tsc-rs-wasm_bg.wasm"
ls -la "$SITE/public/playground/pkg"
echo "Built tsc-rs $VERSION. Update VERSION in src/lib/site.ts if it changed (it cache-busts the package URL)."
