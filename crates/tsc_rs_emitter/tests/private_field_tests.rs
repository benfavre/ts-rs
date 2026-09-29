//! Private field downlevel transform tests.
//!
//! Tests that class private fields (#field) are correctly transformed to
//! WeakMap-based storage when the target is below ES2022.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

/// Helper: parse and emit with a specific target.
fn emit_with_target(source: &str, target: ScriptTarget) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        target: Some(target),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with ES2020 target (downlevels private fields).
fn emit_es2020(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ES2020)
}

/// Helper: parse and emit with ESNext target (no downleveling).
fn emit_esnext(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ESNext)
}

// ---------------------------------------------------------------
// Private field WeakMap declaration
// ---------------------------------------------------------------

#[test]
fn test_private_field_weakmap_declaration() {
    let js = emit_es2020("class Foo { #name: string; }");
    assert!(
        js.contains("var _Foo_name;"),
        "private field should emit hoisted helper declaration: {js}"
    );
    assert!(
        js.contains("_Foo_name = new WeakMap();"),
        "private field should emit WeakMap declaration: {js}"
    );
    // The private field declaration should not appear in the class body
    assert!(
        !js.contains("#name"),
        "private field declaration should be removed from class body: {js}"
    );
}

#[test]
fn test_private_field_no_downlevel_at_esnext() {
    let js = emit_esnext("class Foo { #name: string; }");
    assert!(
        !js.contains("WeakMap"),
        "private fields should NOT be downleveled at ESNext: {js}"
    );
    assert!(
        js.contains("#name"),
        "private field should be preserved at ESNext: {js}"
    );
}

#[test]
fn test_private_field_no_downlevel_at_es2022() {
    let js = emit_with_target("class Foo { #name: string; }", ScriptTarget::ES2022);
    assert!(
        !js.contains("WeakMap"),
        "private fields should NOT be downleveled at ES2022: {js}"
    );
}

// ---------------------------------------------------------------
// Private field constructor initialization
// ---------------------------------------------------------------

#[test]
fn test_private_field_constructor_init_with_value() {
    let js = emit_es2020("class Foo { #count = 0; }");
    assert!(
        js.contains("var _Foo_count;"),
        "should emit hoisted helper declaration for #count: {js}"
    );
    assert!(
        js.contains("_Foo_count = new WeakMap();"),
        "should emit WeakMap for #count: {js}"
    );
    assert!(
        js.contains("_Foo_count.set(this, 0)"),
        "should initialize private field in constructor: {js}"
    );
    assert!(
        js.contains("constructor()"),
        "should synthesize constructor: {js}"
    );
}

#[test]
fn test_private_field_constructor_init_without_value() {
    let js = emit_es2020("class Foo { #name: string; }");
    assert!(
        js.contains("_Foo_name.set(this, void 0)"),
        "should initialize uninitialized private field with void 0: {js}"
    );
}

#[test]
fn test_private_field_with_existing_constructor() {
    let source = r#"
class Foo {
    #data = 42;
    constructor(x: number) {
        console.log(x);
    }
}
"#;
    let js = emit_es2020(source);
    assert!(
        js.contains("var _Foo_data;"),
        "should emit hoisted helper declaration: {js}"
    );
    assert!(
        js.contains("_Foo_data = new WeakMap();"),
        "should emit WeakMap: {js}"
    );
    assert!(
        js.contains("_Foo_data.set(this, 42)"),
        "should set private field in existing constructor: {js}"
    );
    assert!(
        js.contains("console.log(x)"),
        "should preserve existing constructor body: {js}"
    );
}

// ---------------------------------------------------------------
// Private field access (read)
// ---------------------------------------------------------------

#[test]
fn test_private_field_get() {
    let source = r#"
class Foo {
    #name = "hello";
    getName() {
        return this.#name;
    }
}
"#;
    let js = emit_es2020(source);
    assert!(
        js.contains("__classPrivateFieldGet(this, _Foo_name, \"f\")"),
        "should transform this.#name to __classPrivateFieldGet with class-qualified helper: {js}"
    );
}

// ---------------------------------------------------------------
// Private field access (write)
// ---------------------------------------------------------------

#[test]
fn test_private_field_set() {
    let source = r#"
class Foo {
    #name = "hello";
    setName(n: string) {
        this.#name = n;
    }
}
"#;
    let js = emit_es2020(source);
    assert!(
        js.contains("__classPrivateFieldSet(this, _Foo_name, n, \"f\")"),
        "should transform this.#name = n to __classPrivateFieldSet with class-qualified helper: {js}"
    );
}

// ---------------------------------------------------------------
// Helper function emission
// ---------------------------------------------------------------

#[test]
fn test_private_field_helpers_emitted() {
    let js =
        emit_es2020("class Foo { #x = 1; getX() { return this.#x; } setX(v) { this.#x = v; } }");
    assert!(
        js.contains("var __classPrivateFieldGet"),
        "should emit __classPrivateFieldGet helper: {js}"
    );
    assert!(
        js.contains("var __classPrivateFieldSet"),
        "should emit __classPrivateFieldSet helper: {js}"
    );
}

#[test]
fn test_private_field_helpers_not_emitted_when_not_needed() {
    let js = emit_es2020("class Foo { x = 1; }");
    assert!(
        !js.contains("__classPrivateFieldGet"),
        "should NOT emit private field helpers without private fields: {js}"
    );
}

// ---------------------------------------------------------------
// Multiple private fields
// ---------------------------------------------------------------

#[test]
fn test_multiple_private_fields() {
    let source = r#"
class Point {
    #x = 0;
    #y = 0;
    getX() { return this.#x; }
    getY() { return this.#y; }
}
"#;
    let js = emit_es2020(source);
    assert!(
        js.contains("var _Point_x, _Point_y;"),
        "should emit hoisted helper declarations for both fields: {js}"
    );
    assert!(
        js.contains("_Point_x = new WeakMap()"),
        "should emit WeakMap for #x: {js}"
    );
    assert!(
        js.contains("_Point_y = new WeakMap()"),
        "should emit WeakMap for #y: {js}"
    );
    assert!(
        js.contains("_Point_x.set(this, 0)"),
        "should initialize #x in constructor: {js}"
    );
    assert!(
        js.contains("_Point_y.set(this, 0)"),
        "should initialize #y in constructor: {js}"
    );
    assert!(
        js.contains("__classPrivateFieldGet(this, _Point_x, \"f\")"),
        "should transform this.#x read: {js}"
    );
    assert!(
        js.contains("__classPrivateFieldGet(this, _Point_y, \"f\")"),
        "should transform this.#y read: {js}"
    );
}

// ---------------------------------------------------------------
// Private field with extends (super call)
// ---------------------------------------------------------------

#[test]
fn test_private_field_with_extends_no_ctor() {
    let source = r#"
class Child extends Parent {
    #value = 10;
}
"#;
    let js = emit_es2020(source);
    assert!(
        js.contains("var _Child_value;"),
        "should emit hoisted helper declaration: {js}"
    );
    assert!(
        js.contains("_Child_value = new WeakMap();"),
        "should emit WeakMap: {js}"
    );
    assert!(
        js.contains("super(...arguments)"),
        "should call super in synthesized constructor: {js}"
    );
    assert!(
        js.contains("_Child_value.set(this, 10)"),
        "should set private field after super: {js}"
    );
}

#[test]
fn test_private_fields_in_anonymous_class_expressions_emit_wrapped_helpers_in_order() {
    let source = r#"
export const ClassExpression = class {
    #context = 0;
    #method() { return 42; }
    public value = 1;
};

export const ClassExpressionStatic = class {
    static #staticPrivate = "hidden";
    #instancePrivate = true;
    public exposed = "visible";
};
"#;
    let js = emit_es2020(source);
    assert!(
        js.contains(
            "var _instances, _context, _method, _a, _b, _ClassExpressionStatic_staticPrivate, _ClassExpressionStatic_instancePrivate;"
        ),
        "anonymous class-expression helpers should share the hoisted var order from TypeScript: {js}"
    );
    assert!(
        js.contains("export const ClassExpression = (_a = class {"),
        "instance-private anonymous class expression should be wrapped in a comma IIFE: {js}"
    );
    assert!(
        js.contains("_context = new WeakMap()"),
        "instance-private anonymous class expression should emit WeakMap init in the IIFE tail: {js}"
    );
    assert!(
        js.contains("_method = function _method() { return 42; }"),
        "private methods in anonymous class expressions should lower to helper functions: {js}"
    );
    assert!(
        !js.contains("__setFunctionName(_a, \"ClassExpression\")"),
        "pure instance-private anonymous class expressions should not get __setFunctionName: {js}"
    );
    assert!(
        js.contains("__setFunctionName(_b, \"ClassExpressionStatic\")"),
        "anonymous class expressions with static private fields should still get __setFunctionName: {js}"
    );
    assert!(
        js.contains("_ClassExpressionStatic_staticPrivate = { value: \"hidden\" }"),
        "static private fields in anonymous class expressions should lower to object storage in the IIFE tail: {js}"
    );
}
