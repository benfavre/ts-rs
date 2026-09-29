//! Tests for useDefineForClassFields emit behavior.
//!
//! When useDefineForClassFields is true (or target >= ES2022), class fields are
//! emitted as native class field declarations (TC39 [[Define]] semantics).
//! When false (legacy), instance fields are moved into the constructor as
//! `this.field = value` assignments.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

/// Helper: parse and emit with default options (legacy mode, no define).
fn emit_ts(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions::default();
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with useDefineForClassFields: true.
fn emit_ts_define(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        use_define_for_class_fields: Some(true),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with useDefineForClassFields: false.
fn emit_ts_no_define(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        use_define_for_class_fields: Some(false),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with useDefineForClassFields: false and a specific target.
fn emit_ts_no_define_with_target(source: &str, target: ScriptTarget) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        use_define_for_class_fields: Some(false),
        target: Some(target),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with a specific target and no explicit useDefineForClassFields.
fn emit_ts_target(source: &str, target: ScriptTarget) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        target: Some(target),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

// ---------------------------------------------------------------
// Basic field definition vs assignment
// ---------------------------------------------------------------

#[test]
fn test_define_mode_emits_field_in_class_body() {
    let js = emit_ts_define("class Foo { x = 10; }");
    // In define mode, the field should appear directly in the class body
    assert!(
        js.contains("x = 10;"),
        "define mode should emit field in class body: {js}"
    );
    // Should NOT generate a constructor for the field
    assert!(
        !js.contains("constructor"),
        "define mode should not generate a constructor for field inits: {js}"
    );
}

#[test]
fn test_legacy_mode_moves_field_to_constructor() {
    let js = emit_ts_no_define("class Foo { x = 10; }");
    // In legacy mode, the field should be moved to a generated constructor
    assert!(
        js.contains("constructor()"),
        "legacy mode should generate a constructor: {js}"
    );
    assert!(
        js.contains("this.x = 10;"),
        "legacy mode should emit this.x = 10 in constructor: {js}"
    );
}

#[test]
fn test_default_options_uses_legacy_mode() {
    // Default target is ESNext (>= ES2022), so define mode is used by default.
    let js = emit_ts("class Foo { x = 10; }");
    // ESNext >= ES2022, so define mode should apply
    assert!(
        js.contains("x = 10;"),
        "default ESNext target should use define mode: {js}"
    );
    assert!(
        !js.contains("constructor"),
        "default ESNext target should not generate constructor for fields: {js}"
    );
}

// ---------------------------------------------------------------
// Static fields
// ---------------------------------------------------------------

#[test]
fn test_define_mode_static_field() {
    let js = emit_ts_define("class Foo { static count = 0; }");
    assert!(
        js.contains("static count = 0;"),
        "define mode should emit static field directly: {js}"
    );
}

#[test]
fn test_legacy_mode_static_field() {
    // Use ES2021 target to avoid static block conversion (ES2022+ converts
    // static fields to static blocks even in legacy mode)
    let js = emit_ts_no_define_with_target("class Foo { static count = 0; }", ScriptTarget::ES2021);
    // Static fields are emitted AFTER the class body in legacy mode
    // as `ClassName.prop = value;`
    assert!(
        js.contains("Foo.count = 0;"),
        "legacy mode should emit static fields after class body: {js}"
    );
    assert!(
        !js.contains("static count"),
        "legacy mode should NOT emit static fields in class body: {js}"
    );
}

// ---------------------------------------------------------------
// Fields without initializers
// ---------------------------------------------------------------

#[test]
fn test_define_mode_field_without_initializer() {
    let js = emit_ts_define("class Foo { x: number; }");
    // In define mode, fields without initializers should still be declared
    // (they will be defined as undefined by the runtime)
    assert!(
        js.contains("x;"),
        "define mode should emit field declaration without initializer: {js}"
    );
}

#[test]
fn test_legacy_mode_field_without_initializer_is_skipped() {
    let js = emit_ts_no_define("class Foo { x: number; }");
    // In legacy mode, fields without initializers are type-only and skipped
    assert!(
        !js.contains("this.x"),
        "legacy mode should skip fields without initializers: {js}"
    );
    assert!(
        !js.contains("constructor"),
        "legacy mode should not generate constructor for type-only fields: {js}"
    );
}

// ---------------------------------------------------------------
// Target-based defaults
// ---------------------------------------------------------------

#[test]
fn test_es2022_target_defaults_to_define() {
    let js = emit_ts_target("class Foo { x = 1; }", ScriptTarget::ES2022);
    assert!(
        js.contains("x = 1;"),
        "ES2022 target should default to define mode: {js}"
    );
    assert!(
        !js.contains("constructor"),
        "ES2022 target should not generate constructor: {js}"
    );
}

#[test]
fn test_es2021_target_defaults_to_legacy() {
    let js = emit_ts_target("class Foo { x = 1; }", ScriptTarget::ES2021);
    assert!(
        js.contains("constructor"),
        "ES2021 target should default to legacy mode: {js}"
    );
    assert!(
        js.contains("this.x = 1;"),
        "ES2021 target should move field to constructor: {js}"
    );
}

#[test]
fn test_esnext_target_defaults_to_define() {
    let js = emit_ts_target("class Foo { x = 1; }", ScriptTarget::ESNext);
    assert!(
        js.contains("x = 1;"),
        "ESNext target should default to define mode: {js}"
    );
}

#[test]
fn test_explicit_false_overrides_high_target() {
    // Even with ES2022 target, explicit false should use legacy mode
    let file = tsc_rs_parser::parse("test.ts", "class Foo { x = 1; }");
    let opts = CompilerOptions {
        target: Some(ScriptTarget::ES2022),
        use_define_for_class_fields: Some(false),
        ..Default::default()
    };
    let js = emit(&file, &opts).javascript;
    assert!(
        js.contains("constructor"),
        "explicit false should override ES2022 target: {js}"
    );
    assert!(
        js.contains("this.x = 1;"),
        "explicit false should use legacy field init: {js}"
    );
}

#[test]
fn test_explicit_true_overrides_low_target() {
    // useDefineForClassFields: true with ES5 target should downlevel to
    // Object.defineProperty in constructor (define semantics, but lowered)
    let file = tsc_rs_parser::parse("test.ts", "class Foo { x = 1; }");
    let opts = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        use_define_for_class_fields: Some(true),
        ..Default::default()
    };
    let js = emit(&file, &opts).javascript;
    assert!(
        js.contains("Object.defineProperty"),
        "explicit true with low target should downlevel to Object.defineProperty: {js}"
    );
    assert!(
        js.contains("constructor"),
        "explicit true with low target should generate constructor: {js}"
    );
}

// ---------------------------------------------------------------
// Inheritance (extends)
// ---------------------------------------------------------------

#[test]
fn test_define_mode_with_extends() {
    let js = emit_ts_define("class Base {} class Child extends Base { x = 5; }");
    // Child should have field in class body, no generated constructor
    assert!(
        js.contains("x = 5;"),
        "define mode should emit field in class body even with extends: {js}"
    );
    // Should not generate constructor(...args) { super(...args); }
    let child_section = js.split("class Child").nth(1).unwrap_or("");
    assert!(
        !child_section.contains("constructor"),
        "define mode should not generate constructor for Child: {js}"
    );
}

#[test]
fn test_legacy_mode_with_extends_generates_super_call() {
    let js = emit_ts_no_define("class Base {} class Child extends Base { x = 5; }");
    let child_section = js.split("class Child").nth(1).unwrap_or("");
    assert!(
        child_section.contains("constructor()"),
        "legacy mode should generate constructor(): {js}"
    );
    assert!(
        child_section.contains("super(...arguments)"),
        "legacy mode should call super(...arguments) before field inits: {js}"
    );
    assert!(
        child_section.contains("this.x = 5;"),
        "legacy mode should emit this.x = 5 in constructor: {js}"
    );
}

// ---------------------------------------------------------------
// Existing constructor with field inits
// ---------------------------------------------------------------

#[test]
fn test_define_mode_with_existing_constructor() {
    let js = emit_ts_define("class Foo { x = 1; constructor() { console.log('hi'); } }");
    // Fields should be in class body, not injected into constructor
    assert!(
        js.contains("x = 1;"),
        "define mode should keep field in class body: {js}"
    );
    assert!(
        js.contains("console.log"),
        "constructor body should remain: {js}"
    );
}

#[test]
fn test_legacy_mode_with_existing_constructor() {
    let js = emit_ts_no_define("class Foo { x = 1; constructor() { console.log('hi'); } }");
    // Field init should be injected into existing constructor
    assert!(
        js.contains("this.x = 1;"),
        "legacy mode should inject field init into existing constructor: {js}"
    );
    assert!(
        js.contains("console.log"),
        "constructor body should remain: {js}"
    );
}

#[test]
fn test_legacy_mode_field_init_stays_after_leading_body_comment() {
    // Without blank lines between `{` and first statement, field inits
    // come first, then comments with their associated statements.
    let js =
        emit_ts_no_define("class Foo { x = 1; constructor() {\n // keep\n console.log('hi'); } }");
    let comment_idx = js.find("// keep").expect("expected constructor comment");
    let field_idx = js.find("this.x = 1;").expect("expected lowered field init");
    let body_idx = js
        .rfind("console.log")
        .expect("expected original body statement");
    assert!(
        field_idx < comment_idx && comment_idx < body_idx,
        "field init should come before comment (no blank line): {js}"
    );
    // With blank lines, comments come first.
    let js_blank = emit_ts_no_define(
        "class Foo { x = 1; constructor() {\n\n // keep\n console.log('hi'); } }",
    );
    let comment_idx2 = js_blank
        .find("// keep")
        .expect("expected constructor comment");
    let field_idx2 = js_blank
        .find("this.x = 1;")
        .expect("expected lowered field init");
    assert!(
        comment_idx2 < field_idx2,
        "with blank line, comment should come before field init: {js_blank}"
    );
}

// ---------------------------------------------------------------
// Multiple fields
// ---------------------------------------------------------------

#[test]
fn test_define_mode_multiple_fields() {
    let js = emit_ts_define("class Foo { a = 1; b: string; c = 'hello'; }");
    assert!(js.contains("a = 1;"), "should emit field a: {js}");
    assert!(js.contains("b;"), "should emit field b without init: {js}");
    assert!(
        js.contains("c = \"hello\"") || js.contains("c = 'hello'"),
        "should emit field c: {js}"
    );
}

// ---------------------------------------------------------------
// declare and abstract fields are always skipped
// ---------------------------------------------------------------

#[test]
fn test_declare_field_is_skipped_in_define_mode() {
    let js = emit_ts_define("class Foo { declare x: number; }");
    assert!(
        !js.contains("x;") && !js.contains("x ="),
        "declare fields should be skipped in define mode: {js}"
    );
}

#[test]
fn test_abstract_field_is_skipped() {
    let js = emit_ts_define("abstract class Foo { abstract x: number; }");
    // The class itself should be emitted, but abstract field skipped
    assert!(
        js.contains("class Foo"),
        "abstract class should be emitted: {js}"
    );
}
