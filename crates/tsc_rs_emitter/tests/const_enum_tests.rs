//! Tests for const enum member value inlining.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

/// Helper: parse TypeScript source and emit JavaScript with default (CJS) options.
fn emit_ts(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions::default();
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with specific compiler options.
fn emit_ts_with(source: &str, opts: CompilerOptions) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let out = emit(&file, &opts);
    out.javascript
}

// ---------------------------------------------------------------
// Basic numeric const enum inlining
// ---------------------------------------------------------------

#[test]
fn test_const_enum_numeric_inlining() {
    let js = emit_ts(
        r#"
const enum Direction {
    Up,
    Down,
    Left,
    Right
}
let d = Direction.Up;
"#,
    );
    // The enum body should be elided
    assert!(
        !js.contains("var Direction;"),
        "const enum body should be elided: {js}"
    );
    // The usage should be inlined with a comment
    assert!(
        js.contains("0 /* Direction.Up */"),
        "Direction.Up should be inlined as 0: {js}"
    );
}

#[test]
fn test_const_enum_numeric_auto_increment() {
    let js = emit_ts(
        r#"
const enum Color {
    Red,
    Green,
    Blue
}
let r = Color.Red;
let g = Color.Green;
let b = Color.Blue;
"#,
    );
    assert!(
        js.contains("0 /* Color.Red */"),
        "Color.Red should be 0: {js}"
    );
    assert!(
        js.contains("1 /* Color.Green */"),
        "Color.Green should be 1: {js}"
    );
    assert!(
        js.contains("2 /* Color.Blue */"),
        "Color.Blue should be 2: {js}"
    );
}

#[test]
fn test_const_enum_explicit_numeric_values() {
    let js = emit_ts(
        r#"
const enum Status {
    Active = 10,
    Inactive = 20,
    Pending = 30
}
let s = Status.Inactive;
"#,
    );
    assert!(
        js.contains("20 /* Status.Inactive */"),
        "Status.Inactive should be 20: {js}"
    );
}

#[test]
fn test_const_enum_auto_increment_after_explicit() {
    let js = emit_ts(
        r#"
const enum E {
    A = 5,
    B,
    C
}
let x = E.B;
let y = E.C;
"#,
    );
    assert!(
        js.contains("6 /* E.B */"),
        "E.B should be 6 (auto-increment from 5): {js}"
    );
    assert!(js.contains("7 /* E.C */"), "E.C should be 7: {js}");
}

// ---------------------------------------------------------------
// String enum inlining
// ---------------------------------------------------------------

#[test]
fn test_const_enum_string_inlining() {
    let js = emit_ts(
        r#"
const enum Greeting {
    Hello = "hello",
    World = "world"
}
let g = Greeting.Hello;
let w = Greeting.World;
"#,
    );
    assert!(
        js.contains("\"hello\" /* Greeting.Hello */"),
        "Greeting.Hello should be inlined as \"hello\": {js}"
    );
    assert!(
        js.contains("\"world\" /* Greeting.World */"),
        "Greeting.World should be inlined as \"world\": {js}"
    );
}

// ---------------------------------------------------------------
// Cross-member references
// ---------------------------------------------------------------

#[test]
fn test_const_enum_cross_member_reference() {
    let js = emit_ts(
        r#"
const enum E {
    A = 1,
    B = A + 1,
    C = A + B
}
let a = E.A;
let b = E.B;
let c = E.C;
"#,
    );
    assert!(js.contains("1 /* E.A */"), "E.A should be 1: {js}");
    assert!(js.contains("2 /* E.B */"), "E.B should be 2 (A + 1): {js}");
    assert!(js.contains("3 /* E.C */"), "E.C should be 3 (A + B): {js}");
}

#[test]
fn test_const_enum_bitwise_operations() {
    let js = emit_ts(
        r#"
const enum Flags {
    None = 0,
    Read = 1,
    Write = 2,
    ReadWrite = Read | Write
}
let rw = Flags.ReadWrite;
"#,
    );
    assert!(
        js.contains("3 /* Flags.ReadWrite */"),
        "Flags.ReadWrite should be 3 (1 | 2): {js}"
    );
}

#[test]
fn test_const_enum_shift_operations() {
    let js = emit_ts(
        r#"
const enum Bits {
    A = 1 << 0,
    B = 1 << 1,
    C = 1 << 2
}
let a = Bits.A;
let b = Bits.B;
let c = Bits.C;
"#,
    );
    assert!(js.contains("1 /* Bits.A */"), "Bits.A should be 1: {js}");
    assert!(js.contains("2 /* Bits.B */"), "Bits.B should be 2: {js}");
    assert!(js.contains("4 /* Bits.C */"), "Bits.C should be 4: {js}");
}

// ---------------------------------------------------------------
// preserveConstEnums behavior
// ---------------------------------------------------------------

#[test]
fn test_preserve_const_enums() {
    let js = emit_ts_with(
        r#"
const enum Direction {
    Up,
    Down
}
let d = Direction.Up;
"#,
        CompilerOptions {
            preserve_const_enums: Some(true),
            ..Default::default()
        },
    );
    // With preserveConstEnums, the enum body SHOULD be emitted
    assert!(
        js.contains("var Direction;"),
        "with preserveConstEnums, enum body should be emitted: {js}"
    );
    // But values should STILL be inlined at usage sites
    assert!(
        js.contains("0 /* Direction.Up */"),
        "with preserveConstEnums, usage should still be inlined: {js}"
    );
}

#[test]
fn test_const_enum_elided_without_preserve() {
    let js = emit_ts(
        r#"
const enum E { A, B }
let x = E.A;
"#,
    );
    assert!(
        !js.contains("var E;"),
        "without preserveConstEnums, enum body should be elided: {js}"
    );
    assert!(
        !js.contains("(function"),
        "without preserveConstEnums, enum IIFE should be elided: {js}"
    );
}

// ---------------------------------------------------------------
// const enum in exported context
// ---------------------------------------------------------------

#[test]
fn test_exported_const_enum_inlining() {
    let js = emit_ts(
        r#"
export const enum Direction {
    Up,
    Down
}
let d = Direction.Up;
"#,
    );
    // Usage should still be inlined even if the enum is exported
    assert!(
        js.contains("0 /* Direction.Up */"),
        "exported const enum member should be inlined: {js}"
    );
}

// ---------------------------------------------------------------
// Regular enum should NOT be inlined
// ---------------------------------------------------------------

#[test]
fn test_regular_enum_not_inlined() {
    let js = emit_ts(
        r#"
enum Direction {
    Up,
    Down
}
let d = Direction.Up;
"#,
    );
    // Regular enums should NOT be inlined - they emit the IIFE body
    assert!(
        js.contains("var Direction;"),
        "regular enum should emit body: {js}"
    );
    // And usage should remain as property access
    assert!(
        !js.contains("/* Direction.Up */"),
        "regular enum member should NOT be inlined: {js}"
    );
}

// ---------------------------------------------------------------
// Negative values
// ---------------------------------------------------------------

#[test]
fn test_const_enum_negative_values() {
    let js = emit_ts(
        r#"
const enum Temp {
    Cold = -10,
    Freezing = -20
}
let t = Temp.Cold;
let f = Temp.Freezing;
"#,
    );
    assert!(
        js.contains("-10 /* Temp.Cold */"),
        "Temp.Cold should be -10: {js}"
    );
    assert!(
        js.contains("-20 /* Temp.Freezing */"),
        "Temp.Freezing should be -20: {js}"
    );
}

// ---------------------------------------------------------------
// Multiple const enums in same file
// ---------------------------------------------------------------

#[test]
fn test_multiple_const_enums() {
    let js = emit_ts(
        r#"
const enum A { X = 1, Y = 2 }
const enum B { X = 10, Y = 20 }
let a = A.X;
let b = B.X;
"#,
    );
    assert!(js.contains("1 /* A.X */"), "A.X should be 1: {js}");
    assert!(js.contains("10 /* B.X */"), "B.X should be 10: {js}");
}
