#!/bin/bash
# Analyze errors baseline failures to find highest-impact fix targets.
# Usage: ./scripts/errors-analysis.sh [--suite compiler|conformance] [--sample N]
#
# Outputs a ranked list of missing error codes with test counts.

set -euo pipefail

SUITE="${1:-compiler}"
SUITE="${SUITE#--suite }"
SAMPLE="${2:-100}"
SAMPLE="${SAMPLE#--sample }"

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

echo "=== Errors Baseline Analysis: $SUITE suite ==="
echo ""

# Get current stats
cargo run -p tsc_rs_harness --bin baseline-report -- --suite "$SUITE" --baseline errors --no-cache 2>/dev/null | head -10
echo ""

# Save MISSING-LINES cases (we produce 0 errors, expected has errors)
echo "Sampling $SAMPLE MISSING-LINES-AT-END cases for expected error codes..."
cargo run -p tsc_rs_harness --bin baseline-report -- --suite "$SUITE" --baseline errors \
  --kind MISSING-LINES-AT-END --save-case-list "$TMPDIR/missing.txt" --no-cache 2>/dev/null > /dev/null

TOTAL_MISSING=$(wc -l < "$TMPDIR/missing.txt")
echo "Total MISSING-LINES-AT-END: $TOTAL_MISSING"
echo ""

# Sample cases and extract expected error codes
head -n "$SAMPLE" "$TMPDIR/missing.txt" | while read -r test; do
  cargo run -p tsc_rs_harness --bin baseline-case -- "$test" --suite "$SUITE" \
    --baseline errors --show-outputs 2>/dev/null | grep "^[a-zA-Z].*error TS" | \
    sed 's/.*error \(TS[0-9]*\): \(.*\)/\1: \2/' | head -5
done > "$TMPDIR/error_codes.txt" 2>/dev/null

# Count by error code
echo "=== Top Missing Error Codes (from $SAMPLE samples) ==="
echo ""
cut -d: -f1 "$TMPDIR/error_codes.txt" | sort | uniq -c | sort -rn | head -25
echo ""

# Count by error code + message
echo "=== Top Missing Diagnostics (code + message) ==="
echo ""
sort "$TMPDIR/error_codes.txt" | uniq -c | sort -rn | head -20
echo ""

# Also check CODE-DIFF patterns
echo "=== CODE-DIFF Analysis ==="
cargo run -p tsc_rs_harness --bin baseline-report -- --suite "$SUITE" --baseline errors \
  --kind CODE-DIFF --save-case-list "$TMPDIR/codediff.txt" --no-cache 2>/dev/null > /dev/null
TOTAL_CD=$(wc -l < "$TMPDIR/codediff.txt")
echo "Total CODE-DIFF: $TOTAL_CD"

head -n 30 "$TMPDIR/codediff.txt" | while read -r test; do
  cargo run -p tsc_rs_harness --bin baseline-case -- "$test" --suite "$SUITE" \
    --baseline errors --show-first-mismatch 2>/dev/null | grep "expected:" | head -1 | \
    sed 's/.*error \(TS[0-9]*\).*/\1/'
done > "$TMPDIR/cd_codes.txt" 2>/dev/null

echo ""
echo "Top expected codes at first mismatch:"
sort "$TMPDIR/cd_codes.txt" | uniq -c | sort -rn | head -15
