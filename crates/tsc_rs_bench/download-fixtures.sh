#!/usr/bin/env bash
# Download standard benchmark fixture files used by the JS tooling ecosystem.
#
# These are the same files used by oxc, swc, and biome benchmarks,
# enabling direct comparison of throughput numbers.
#
# Sources:
#   - oxc-project/bench-javascript-parser-written-in-rust (parser benchmarks)
#   - oxc-project/bench-transformer (transformer benchmarks)
#
# Usage: ./download-fixtures.sh [--force]

set -euo pipefail

FIXTURE_DIR="$(cd "$(dirname "$0")" && pwd)/fixtures"
FORCE="${1:-}"

mkdir -p "$FIXTURE_DIR"

download() {
    local name="$1"
    local url="$2"
    local dest="$FIXTURE_DIR/$name"

    if [ -f "$dest" ] && [ "$FORCE" != "--force" ]; then
        local size
        size=$(wc -c < "$dest" | tr -d ' ')
        if [ "$size" -gt 100 ]; then
            echo "  [skip] $name (already exists, use --force to re-download)"
            return
        fi
    fi

    echo "  [download] $name"
    curl -sL --fail "$url" -o "$dest"
    local size
    size=$(wc -c < "$dest" | tr -d ' ')
    local lines
    lines=$(wc -l < "$dest" | tr -d ' ')
    echo "             -> ${size} bytes, ${lines} lines"
}

echo "Downloading benchmark fixtures to $FIXTURE_DIR ..."
echo ""

# 1. TypeScript compiler bundle (~8MB / ~97K lines)
#    The canonical "large file" benchmark in JS tooling.
#    Used by: oxc parser bench, V8 web-tooling-benchmark
OXC_PARSER_BENCH="https://raw.githubusercontent.com/oxc-project/bench-javascript-parser-written-in-rust/main/files"
download "typescript.js" "${OXC_PARSER_BENCH}/typescript.js"

# 2. cal.com TSX component (~1MB / ~20K lines bundled)
#    Real-world React/TSX file.
#    Used by: oxc parser bench + transformer bench
download "cal.com.tsx" "${OXC_PARSER_BENCH}/cal.com.tsx"

# 3. TypeScript parser source (~537K / ~10K lines)
#    Real-world TypeScript from the compiler itself.
#    Used by: oxc transformer benchmarks
OXC_TRANSFORM_BENCH="https://raw.githubusercontent.com/oxc-project/bench-transformer/main/fixtures"
download "parser.ts" "${OXC_TRANSFORM_BENCH}/parser.ts"

# 4. Vue.js runtime-core renderer (~72K / ~2.5K lines)
#    Real-world TypeScript library code.
#    Used by: oxc transformer benchmarks
download "renderer.ts" "${OXC_TRANSFORM_BENCH}/renderer.ts"

# 5. AFFiNE table component (~31K / ~1.1K lines)
#    Medium TSX file with real-world patterns.
#    Used by: oxc transformer benchmarks
download "table.tsx" "${OXC_TRANSFORM_BENCH}/table.tsx"

# 6. cal.com UserSettings (~4K / ~124 lines)
#    Small TSX component — fast iteration benchmark.
#    Used by: oxc transformer benchmarks
download "UserSettings.tsx" "${OXC_TRANSFORM_BENCH}/UserSettings.tsx"

echo ""
echo "Done! Fixture summary:"
echo ""
for f in "$FIXTURE_DIR"/*.js "$FIXTURE_DIR"/*.ts "$FIXTURE_DIR"/*.tsx; do
    if [ -f "$f" ]; then
        name=$(basename "$f")
        size=$(wc -c < "$f" | tr -d ' ')
        lines=$(wc -l < "$f" | tr -d ' ')
        printf "  %-20s %10s bytes  %8s lines\n" "$name" "$size" "$lines"
    fi
done
