#!/bin/bash
# Bump version across all npm packages and Cargo workspace.
# Usage: ./npm/bump-version.sh 0.2.0

set -euo pipefail

VERSION="${1:?Usage: bump-version.sh <version>}"

cd "$(dirname "$0")"

# Update all npm package.json files
for pkg in tsc-rs tsc-rs-linux-x64 tsc-rs-linux-arm64 tsc-rs-darwin-x64 tsc-rs-darwin-arm64 tsc-rs-win32-x64; do
  jq --arg v "$VERSION" '.version = $v' "$pkg/package.json" > "$pkg/package.json.tmp"
  mv "$pkg/package.json.tmp" "$pkg/package.json"
done

# Update optionalDependencies versions in root package
jq --arg v "$VERSION" '
  .optionalDependencies |= with_entries(.value = $v)
' tsc-rs/package.json > tsc-rs/package.json.tmp
mv tsc-rs/package.json.tmp tsc-rs/package.json

echo "Updated all npm packages to v${VERSION}"
echo "Don't forget to also update workspace version in Cargo.toml"
