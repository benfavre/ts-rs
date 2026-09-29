#!/bin/bash
# Build the WASM package for npm distribution.
# Requires: cargo install wasm-pack
#
# Output goes to ./pkg/ — ready for `npm publish`.

set -euo pipefail

cd "$(dirname "$0")"

echo "Building tsc-rs WASM package..."
wasm-pack build \
  --target web \
  --out-dir pkg \
  --out-name tsc-rs-wasm \
  --scope tsc-rs

echo ""
echo "Build complete. Package at: $(pwd)/pkg/"
echo ""
echo "To test locally:"
echo "  cd pkg && npm pack"
echo ""
echo "To publish (when ready):"
echo "  cd pkg && npm publish --access public"
