#!/bin/bash
# Quick before/after check for error baseline improvements.
# Usage: ./scripts/quick-check.sh
#
# Runs both JS (regression check) and errors baselines, prints summary.

set -euo pipefail

echo "=== JS Baseline (regression check) ==="
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --limit 100 --no-cache 2>/dev/null | head -1

echo ""
echo "=== Errors Baseline (compiler, first 500) ==="
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --baseline errors --limit 500 --no-cache 2>/dev/null | head -1

echo ""
echo "=== Errors Baseline (compiler, full) ==="
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --baseline errors --no-cache 2>/dev/null | head -1

echo ""
echo "=== Errors Baseline (conformance, full) ==="
cargo run -p tsc_rs_harness --bin baseline-report -- --suite conformance --baseline errors --no-cache 2>/dev/null | head -1
