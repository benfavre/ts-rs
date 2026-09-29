#!/bin/bash
# Find all tests expecting a specific error code and test them.
# Usage: ./scripts/test-error-code.sh TS2451 [--suite compiler]
#
# Useful for measuring impact of implementing a new error code.

set -euo pipefail

CODE="${1:?Usage: test-error-code.sh TSxxxx [--suite compiler]}"
SUITE="${2:-compiler}"

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

echo "=== Finding tests expecting $CODE in $SUITE suite ==="

# Find .errors.txt baselines containing this code
BASELINE_DIR="tests/baselines/reference"
grep -rl "error $CODE:" "$BASELINE_DIR" --include="*.errors.txt" 2>/dev/null | \
  sed "s|$BASELINE_DIR/||;s|\.errors\.txt$||;s|(.*)||" | sort -u > "$TMPDIR/all_tests.txt"

TOTAL=$(wc -l < "$TMPDIR/all_tests.txt")
echo "Found $TOTAL baselines containing $CODE"

if [ "$TOTAL" -eq 0 ]; then
  echo "No tests found."
  exit 0
fi

# Find single-code tests (only this error code in the baseline)
while read -r test; do
  baseline="$BASELINE_DIR/${test}.errors.txt"
  # Try parameterized baselines too
  if [ ! -f "$baseline" ]; then
    baseline=$(ls "$BASELINE_DIR/${test}"*.errors.txt 2>/dev/null | head -1)
  fi
  [ -z "$baseline" ] && continue
  codes=$(grep "error TS" "$baseline" | sed 's/.*error \(TS[0-9]*\).*/\1/' | sort -u)
  if [ "$(echo "$codes" | wc -l)" -eq 1 ] && [ "$codes" = "$CODE" ]; then
    echo "$test"
  fi
done < "$TMPDIR/all_tests.txt" > "$TMPDIR/single_code.txt"

SINGLE=$(wc -l < "$TMPDIR/single_code.txt")
echo "Of those, $SINGLE are single-code tests (implementing $CODE alone would pass them)"
echo ""

# Test a sample
SAMPLE_SIZE=10
if [ "$SINGLE" -gt 0 ]; then
  echo "=== Testing $SAMPLE_SIZE single-code cases ==="
  PASS=0
  FAIL=0
  head -n "$SAMPLE_SIZE" "$TMPDIR/single_code.txt" | while read -r test; do
    result=$(cargo run -p tsc_rs_harness --bin baseline-case -- "$test" --suite "$SUITE" --baseline errors 2>/dev/null | head -1)
    status=$(echo "$result" | grep -o "passed=[a-z]*" | cut -d= -f2)
    echo "  $test: $status"
  done
fi
