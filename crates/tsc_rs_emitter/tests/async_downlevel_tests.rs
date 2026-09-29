//! Async/await downlevel emit transformation tests.
//!
//! Tests that async functions are correctly transformed to __awaiter + generator
//! when targeting ES2015 or ES5, and preserved when targeting ES2017+.

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

/// Helper: parse and emit with ES2015 target (downlevels async/await).
fn emit_es2015(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ES2015)
}

/// Helper: parse and emit with ESNext target (no downleveling).
fn emit_esnext(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ESNext)
}

// ---------------------------------------------------------------
// __awaiter helper emission
// ---------------------------------------------------------------

#[test]
fn test_awaiter_helper_emitted_for_async_function() {
    let js = emit_es2015("async function foo() { await bar(); }");
    assert!(
        js.contains("var __awaiter"),
        "should emit __awaiter helper: {js}"
    );
    assert!(
        js.contains("function adopt(value)"),
        "should contain adopt function in helper: {js}"
    );
}

#[test]
fn test_awaiter_helper_not_emitted_when_no_async() {
    let js = emit_es2015("function foo() { return 1; }");
    assert!(
        !js.contains("__awaiter"),
        "should not emit __awaiter when no async functions: {js}"
    );
}

#[test]
fn test_awaiter_helper_not_emitted_for_esnext() {
    let js = emit_esnext("async function foo() { await bar(); }");
    assert!(
        !js.contains("__awaiter"),
        "should not emit __awaiter for ESNext target: {js}"
    );
}

#[test]
fn test_awaiter_helper_not_emitted_for_es2017() {
    let js = emit_with_target(
        "async function foo() { await bar(); }",
        ScriptTarget::ES2017,
    );
    assert!(
        !js.contains("__awaiter"),
        "should not emit __awaiter for ES2017 target: {js}"
    );
}

// ---------------------------------------------------------------
// Async function declaration downlevel
// ---------------------------------------------------------------

#[test]
fn test_async_function_decl_downlevel() {
    let js = emit_es2015("async function foo() { await bar(); }");
    // Should NOT have the async keyword
    assert!(
        !js.contains("async function foo"),
        "async keyword should be stripped: {js}"
    );
    // Should have a regular function declaration
    assert!(
        js.contains("function foo()"),
        "should emit regular function: {js}"
    );
    // Should wrap body in __awaiter
    assert!(
        js.contains("__awaiter(this, void 0, void 0, function* ()"),
        "should wrap body in __awaiter with generator: {js}"
    );
    // Should convert await to yield
    assert!(
        js.contains("yield bar()"),
        "should convert await to yield: {js}"
    );
    assert!(
        !js.contains("await bar()"),
        "await should not appear in output: {js}"
    );
}

#[test]
fn test_async_function_decl_preserved_esnext() {
    let js = emit_esnext("async function foo() { await bar(); }");
    assert!(
        js.contains("async function foo"),
        "async keyword should be preserved for ESNext: {js}"
    );
    assert!(
        js.contains("await bar()"),
        "await should be preserved for ESNext: {js}"
    );
    assert!(
        !js.contains("__awaiter"),
        "should not use __awaiter for ESNext: {js}"
    );
}

// ---------------------------------------------------------------
// Async function expression downlevel
// ---------------------------------------------------------------

#[test]
fn test_async_function_expr_downlevel() {
    let js = emit_es2015("const f = async function() { await x; };");
    assert!(
        !js.contains("async function"),
        "async keyword should be stripped from fn expr: {js}"
    );
    assert!(
        js.contains("__awaiter"),
        "should use __awaiter in fn expr: {js}"
    );
    assert!(
        js.contains("yield x"),
        "await should become yield in fn expr: {js}"
    );
}

// ---------------------------------------------------------------
// Async arrow function downlevel
// ---------------------------------------------------------------

#[test]
fn test_async_arrow_block_body_downlevel() {
    let js = emit_es2015("const f = async () => { await x; };");
    assert!(
        !js.contains("async"),
        "async keyword should be stripped from arrow: {js}"
    );
    assert!(
        js.contains("__awaiter"),
        "should use __awaiter for arrow: {js}"
    );
    assert!(
        js.contains("yield x"),
        "await should become yield in arrow: {js}"
    );
    // TypeScript uses arrow syntax with __awaiter for downleveling
    assert!(
        js.contains("=> __awaiter"),
        "should use arrow syntax with __awaiter: {js}"
    );
}

#[test]
fn test_async_arrow_expr_body_downlevel() {
    let js = emit_es2015("const f = async () => await x;");
    assert!(
        !js.contains("async"),
        "async keyword should be stripped from arrow expr: {js}"
    );
    assert!(
        js.contains("__awaiter"),
        "should use __awaiter for arrow expr: {js}"
    );
    assert!(
        js.contains("yield x"),
        "await should become yield in arrow expr body: {js}"
    );
}

#[test]
fn test_async_arrow_preserved_esnext() {
    let js = emit_esnext("const f = async () => await x;");
    assert!(
        js.contains("async"),
        "async should be preserved for ESNext: {js}"
    );
    assert!(
        js.contains("=>"),
        "arrow syntax should be preserved for ESNext: {js}"
    );
}

// ---------------------------------------------------------------
// Async class method downlevel
// ---------------------------------------------------------------

#[test]
fn test_async_method_downlevel() {
    let js = emit_es2015(
        "class Foo {
    async bar() { await baz(); }
}",
    );
    assert!(
        !js.contains("async bar"),
        "async keyword should be stripped from method: {js}"
    );
    assert!(
        js.contains("__awaiter"),
        "should use __awaiter for method: {js}"
    );
    assert!(
        js.contains("yield baz()"),
        "await should become yield in method: {js}"
    );
}

#[test]
fn test_async_static_method_downlevel() {
    let js = emit_es2015(
        "class Foo {
    static async bar() { await baz(); }
}",
    );
    assert!(
        js.contains("static bar"),
        "static keyword should be preserved: {js}"
    );
    assert!(
        !js.contains("async"),
        "async keyword should be stripped: {js}"
    );
    assert!(js.contains("__awaiter"), "should use __awaiter: {js}");
}

// ---------------------------------------------------------------
// Async object method downlevel
// ---------------------------------------------------------------

#[test]
fn test_async_object_method_downlevel() {
    let js = emit_es2015(
        "const obj = {
    async foo() { await x; }
};",
    );
    assert!(
        !js.contains("async foo"),
        "async keyword should be stripped from object method: {js}"
    );
    assert!(
        js.contains("__awaiter"),
        "should use __awaiter for object method: {js}"
    );
    assert!(
        js.contains("yield x"),
        "await should become yield in object method: {js}"
    );
}

// ---------------------------------------------------------------
// Await-to-yield conversion in various statement types
// ---------------------------------------------------------------

#[test]
fn test_await_in_return_statement() {
    let js = emit_es2015("async function foo() { return await bar(); }");
    assert!(
        js.contains("return yield bar()"),
        "return await should become return yield: {js}"
    );
}

#[test]
fn test_await_in_if_condition() {
    let js = emit_es2015("async function foo() { if (await check()) { } }");
    assert!(
        js.contains("yield check()"),
        "await in if condition should become yield: {js}"
    );
}

#[test]
fn test_await_in_variable_initializer() {
    let js = emit_es2015("async function foo() { const x = await bar(); }");
    assert!(
        js.contains("yield bar()"),
        "await in var init should become yield: {js}"
    );
}

#[test]
fn test_await_in_try_catch() {
    let js = emit_es2015(
        "async function foo() {
    try {
        await bar();
    } catch (e) {
        await baz();
    }
}",
    );
    assert!(
        js.contains("yield bar()"),
        "await in try block should become yield: {js}"
    );
    assert!(
        js.contains("yield baz()"),
        "await in catch block should become yield: {js}"
    );
}

#[test]
fn test_multiple_awaits() {
    let js = emit_es2015("async function foo() { const a = await x; const b = await y; }");
    assert!(
        js.contains("yield x"),
        "first await should become yield: {js}"
    );
    assert!(
        js.contains("yield y"),
        "second await should become yield: {js}"
    );
}

// ---------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------

#[test]
fn test_nested_async_not_converted() {
    // A nested async arrow inside an async function should get its own __awaiter
    // but the outer function's downlevel should not touch the inner's await
    let js = emit_es2015(
        "async function outer() {
    const inner = async () => await nested();
    await inner();
}",
    );
    // Both awaits should be converted to yield, but in separate scopes
    assert!(
        js.contains("yield inner()"),
        "outer await should become yield: {js}"
    );
    assert!(js.contains("__awaiter"), "should contain __awaiter: {js}");
}

#[test]
fn test_es2016_needs_downlevel() {
    let js = emit_with_target(
        "async function foo() { await bar(); }",
        ScriptTarget::ES2016,
    );
    assert!(
        js.contains("__awaiter"),
        "ES2016 should downlevel async: {js}"
    );
    assert!(
        !js.contains("async function"),
        "async keyword should be stripped for ES2016: {js}"
    );
}

#[test]
fn test_es5_needs_downlevel() {
    let js = emit_with_target("async function foo() { await bar(); }", ScriptTarget::ES5);
    assert!(js.contains("__awaiter"), "ES5 should downlevel async: {js}");
}

#[test]
fn test_async_function_with_params() {
    let js = emit_es2015("async function foo(a: number, b: string) { return await bar(a, b); }");
    assert!(
        js.contains("function foo(a, b)"),
        "params should be preserved without type annotations: {js}"
    );
    assert!(js.contains("__awaiter"), "should use __awaiter: {js}");
    assert!(
        js.contains("yield bar(a, b)"),
        "await should become yield: {js}"
    );
}

#[test]
fn test_async_arrow_with_params() {
    let js = emit_es2015("const f = async (x: number) => await x;");
    // TypeScript uses arrow syntax with __awaiter for downleveling
    assert!(
        js.contains("(x)"),
        "arrow should become => __awaiter arrow with __awaiter: {js}"
    );
    assert!(js.contains("yield x"), "await should become yield: {js}");
}
