# DIAGNOSTICS.md - Analysis of ts-rs Compiler Issues

## Executive Summary

This document analyzes the gaps between ts-rs (this compiler) and tsc (official TypeScript compiler). The current pass rate is **71.5%** (8,884/12,436 tests).

---

## Test Results (Updated: February 21, 2026)

| Test Suite | Total | Passed | Failed | Pass Rate |
|------------|-------|--------|--------|-----------|
| Compiler | 6,529 | 5,142 | 1,387 | 78.8% |
| Conformance | 5,907 | 3,739 | 2,168 | 63.3% |
| Skipped | 504 | - | - | - |
| **Total** | **12,436** | **8,881** | **3,555** | **71.4%** |

### Quick Subset (First 500)
- Pass rate: **94.4%**
- This shows most issues are in edge cases and error recovery

---

## Failure Categories

### Compiler Suite (1,387 failures)

| Category | Count | Description |
|----------|-------|-------------|
| CODE-DIFF | 731 | Generated JavaScript differs from expected |
| EXTRA-COMMENT | 47 | Comments duplicated in output |
| MISSING-COMMENT | 30 | Comments missing from output |
| WHITESPACE-ONLY | 35 | Minor whitespace differences |
| MISSING-LINES-AT-END | 31 | Missing lines at end of file |
| EXTRA-LINES-AT-END | 26 | Extra lines at end of file |
| WRONG-FILE-HEADER | 9 | Wrong output filenames in multi-file tests |

### Conformance Suite (2,168 failures)

Most due to **missing baseline files** (no reference output to compare against).

---

## Root Cause Analysis

### 1. Parser Error Recovery (PRIMARY ISSUE)

When invalid TypeScript is encountered, ts-rs produces different AST than tsc, leading to cascading differences.

**Example: `classMemberWithMissingIdentifier`**
```typescript
class C { 
    public {};
}
```
- **Expected output**: Class with empty block emitted outside, semicolon after
- **Actual output**: Error marker `<error>` emitted

**Root cause**: Parser creates `<error>` identifier when property name parsing fails, then emits it.

**Files involved**:
- `crates/tsc_rs_parser/src/lib.rs` - `parse_property_name()`, `parse_identifier_name()`
- `crates/tsc_rs_emitter/src/lib.rs` - `emit_class_member()`

### 2. Async/Await Downleveling

**Example: `asyncImportNestedYield`**
- Missing `__awaiter` helper in output
- Import namespace transformations incomplete

**Root cause**: `scan_needs_awaiter()` may not detect all async contexts (imports, nested functions)

**Files involved**:
- `crates/tsc_rs_emitter/src/lib.rs`:
  - `scan_needs_awaiter()` (line ~3371)
  - `source_has_async()` (line ~17910)
  - `emit_awaiter_helper()` (line ~1202)
  - `emit_awaiter_body()` (line ~1695)

### 3. Multi-File Output Filenames

**Example: `isolatedModulesReExportType`**
- Expected: `//// [exportEqualsT.js]`
- Actual: `//// [exportValue.js]`

**Root cause**: Output filename resolution uses wrong file from multi-file test case.

### 4. Comment Handling

**Example: `capturedParametersInInitializers1`**
- Comments duplicated or lost in error-recovered code
- Comments attached to wrong nodes

**Root cause**: Error recovery creates nodes with incorrect span information, affecting comment attachment.

**Files involved**:
- `crates/tsc_rs_emitter/src/lib.rs`:
  - `emit_leading_comments()` (line ~7557)
  - `emit_trailing_comments()`

### 5. Empty Block Emission

**Example: `catchClauseWithInitializer1`**
- Expected: `try {` (compact)
- Actual: `try { }` (with space)

**Root cause**: Emitter treats empty blocks differently based on source span.

---

## Specific Test Cases

### Hardest Cases (Complex Error Recovery)

| Test | Issue | Location |
|------|-------|----------|
| `ambiguousGenericAssertion1` | `<<` vs `< <` tokenization | Scanner |
| `arrowFunctionsMissingTokens` | Missing `=>` produces wrong AST | Parser |
| `asiAbstract` | ASI doesn't work with `abstract` keyword | Parser |
| `classExpressionWithDecorator1` | `@` decorator in wrong context | Parser |
| `classMemberWithMissingIdentifier` | Empty class member `{}` | Parser/Emitter |

### Medium Difficulty

| Test | Issue | Location |
|------|-------|----------|
| `asyncFunctionTempVariableScoping` | Async helper not emitted | Emitter |
| `commentsemitComments` | Comment positions wrong | Emitter |
| `commonJsIsolatedModules` | Multi-file emit order | Emitter |

---

## Code Architecture

### Parser Flow
```
Source → Scanner → Parser → AST → Binder → TypeChecker → Emitter → JS
```

### Error Recovery Points
1. **Scanner**: May tokenize `<<` as single token
2. **Parser**: Creates error nodes with `<error>` identifiers
3. **Emitter**: Emits error nodes differently than tsc

### Key Functions

**Parser** (`crates/tsc_rs_parser/src/lib.rs`):
- `parse_property_name()` - Line ~4390 - Handles property names
- `parse_identifier_name()` - Line ~271 - Parses identifiers
- `parse_class_member()` - Line ~946 - Class member parsing
- `parse_member_modifiers()` - Line ~4491 - Access modifiers

**Emitter** (`crates/tsc_rs_emitter/src/lib.rs`):
- `emit_class_member()` - Line ~9543 - Class member emission
- `emit_prop_name()` - Line ~11599 - Property name emission
- `emit_leading_comments()` - Line ~7557 - Comment handling
- `scan_needs_awaiter()` - Line ~3371 - Async detection

---

## Recommendations

### High Priority

1. **Fix error recovery in parser**
   - Instead of creating `<error>` nodes, skip to next valid token
   - Match tsc's error recovery behavior

2. **Fix async helper detection**
   - Scan all contexts including imports, exports
   - Add more test cases for async edge cases

3. **Fix multi-file output**
   - Debug filename resolution in harness

### Medium Priority

4. **Fix comment attachment**
   - Ensure spans are correct for error nodes
   - Test comment preservation

5. **Fix empty block emission**
   - Match tsc's `{ }` vs multi-line block behavior

### Low Priority

6. **Generate missing baselines**
   - Many conformance tests lack reference files
   - Could generate via tsc for comparison

---

## Preserve Mode: Alternative Compiler Behavior

ts-rs supports **preserve modes** that maintain more information from the original TypeScript source, enabling richer tooling capabilities.

### Available Options

These options can be set in `CompilerOptions`:

| Option | Effect |
|--------|--------|
| `preserve_type_annotations` | Keep `as Type`, `<Type>`, `satisfies Type` in output |
| `preserve_comments` | Keep all comments regardless of `remove_comments` |
| `preserve_whitespace` | Try to maintain original formatting (not yet implemented) |

### Usage

```rust
let mut options = CompilerOptions::default();
options.preserve_type_annotations = Some(true);
options.preserve_comments = Some(true);

let output = tsc_rs_emitter::emit(&source_file, &options);
// Output will contain type annotations and comments
```

### Benefits

1. **TypeScript-aware tooling**: Keep types for IDE support, refactoring
2. **Documentation extraction**: Preserve JSDoc comments
3. **Code generation**: Maintain original type information
4. **Different use cases**: 
   - LSP servers that need rich AST
   - Code formatters with original source awareness
   - Type-preserving transforms

### Files Modified

- `crates/tsc_rs_ast/src/lib.rs` - Added `preserve_type_annotations`, `preserve_comments` to `CompilerOptions`
- `crates/tsc_rs_emitter/src/lib.rs` - Added logic to preserve types and comments

---

## Commands for Testing

```bash
# Run all baseline tests
cargo test -p tsc_rs_harness --test baseline_tests -- --ignored

# Run specific test case
cargo run -p tsc_rs_harness --bin baseline-case -- <name> --suite compiler

# Generate report
cargo run -p tsc_rs_harness --bin baseline-report -- --suite compiler --top 20

# Quick analysis
cargo test -p tsc_rs_harness --test baseline_tests find_quick_wins -- --ignored --nocapture
```

---

## Appendix: Test Categories

### Categories that could improve with targeted fixes

| Category | Count | Potential Fix |
|----------|-------|---------------|
| CODE-DIFF (known issues) | ~50 | Parser error recovery |
| WHITESPACE-ONLY | 39 | Spacing in emitter |
| EXTRA-COMMENT | 56 | Comment attachment |
| MISSING-COMMENT | 46 | Comment attachment |

### Categories requiring major work

| Category | Count | Requires |
|----------|-------|----------|
| CODE-DIFF (complex) | ~800 | Full parser rewrite |
| WRONG-FILE-HEADER | 24 | Multi-file emit fix |
| MISSING-LINES-AT-END | 77 | Various emit fixes |

---

*Last updated: Feb 2026*
