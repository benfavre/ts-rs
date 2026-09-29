//! For-of downlevel emit transformation tests.
//!
//! Tests that `for-of` loops are correctly transformed to index-based
//! for loops (or iterator-based loops with __values) when the target
//! is set to ES5 or ES3.

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

/// Helper: parse and emit with ES5 target (downlevels for-of).
fn emit_es5(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ES5)
}

/// Helper: parse and emit with ESNext target (no downleveling).
fn emit_esnext(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ESNext)
}

/// Helper: parse and emit with ES5 target and downlevelIteration.
fn emit_es5_downlevel_iteration(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        down_level_iteration: Some(true),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

// ---------------------------------------------------------------
// Basic for-of downlevel (target < ES2015, no downlevelIteration)
// ---------------------------------------------------------------

#[test]
fn test_for_of_const_downlevel_es5() {
    let js = emit_es5("for (const x of arr) { console.log(x); }");
    // Should produce index-based loop
    assert!(
        js.contains(".length"),
        "for-of should be downleveled to index-based loop: {js}"
    );
    assert!(
        js.contains("var x = "),
        "should declare loop variable with var: {js}"
    );
    assert!(
        !js.contains(" of "),
        "for-of keyword should not appear in downlevel output: {js}"
    );
}

#[test]
fn test_for_of_let_downlevel_es5() {
    let js = emit_es5("for (let x of arr) { console.log(x); }");
    assert!(
        js.contains(".length"),
        "for-of with let should be downleveled: {js}"
    );
    assert!(
        js.contains("var x = "),
        "let should become var in ES5 output: {js}"
    );
    assert!(
        !js.contains(" of "),
        "for-of keyword should not appear in downlevel output: {js}"
    );
}

#[test]
fn test_for_of_preserved_esnext() {
    let js = emit_esnext("for (const x of arr) { console.log(x); }");
    assert!(
        js.contains(" of "),
        "for-of should be preserved for ESNext: {js}"
    );
    assert!(
        !js.contains(".length"),
        "no .length should appear for ESNext for-of: {js}"
    );
}

#[test]
fn test_for_of_preserved_es2015() {
    let js = emit_with_target(
        "for (const x of arr) { console.log(x); }",
        ScriptTarget::ES2015,
    );
    assert!(
        js.contains(" of "),
        "for-of should be preserved for ES2015: {js}"
    );
}

#[test]
fn test_for_of_index_var_structure() {
    let js = emit_es5("for (const x of arr) { console.log(x); }");
    // Check the index-based for structure
    assert!(js.contains("= 0"), "should initialize index to 0: {js}");
    assert!(js.contains("++"), "should have increment operator: {js}");
}

#[test]
fn test_for_of_existing_identifier_is_assignment() {
    let js = emit_es5("let x; for (x of arr) { console.log(x); }");
    assert!(
        js.contains("x = arr_1[_i];"),
        "should assign the iteration value: {js}"
    );
    assert!(
        !js.contains("var x = arr_1[_i];"),
        "assignment-form for-of must not redeclare its target: {js}"
    );
}

#[test]
fn test_for_of_recovery_target_preserves_member_expression() {
    let js = emit_es5("for (obj.value of arr) { console.log(obj.value); }");
    assert!(
        js.contains("obj.value = arr_1[_i];"),
        "recovery target should use its original source expression: {js}"
    );
    assert!(
        !js.contains("var obj.value"),
        "recovery target must not become a declaration: {js}"
    );
}

#[test]
fn test_for_of_declaration_still_declares_iteration_binding() {
    let js = emit_es5("for (const x of arr) { console.log(x); }");
    assert!(
        js.contains("var x = arr_1[_i];"),
        "declaration-form for-of should retain a declaration: {js}"
    );
}

#[test]
fn test_optional_chain_loop_targets_use_assignment_precedence() {
    for (target, access) in [
        ("obj?.a", "obj.a"),
        ("obj?.a.b", "obj.a.b"),
        ("obj?.[key]", "obj[key]"),
        ("obj?.[key].b", "obj[key].b"),
    ] {
        for (operator, iterable) in [("in", "{}"), ("of", "[]")] {
            let js = emit_with_target(
                &format!("for ({target} {operator} {iterable});"),
                ScriptTarget::ES2015,
            );
            assert_eq!(
                js,
                format!(
                    "\"use strict\";\nfor (obj === null || obj === void 0 ? void 0 : {access} {operator} {iterable})\n    ;\n"
                ),
                "recovered loop target: {target} {operator} {iterable}"
            );
        }
    }
}

#[test]
fn test_optional_chain_inside_loop_target_retains_parentheses() {
    let js = emit_with_target(
        "for (obj[key?.value] in {});\nconst result = (other?.value) + 1;",
        ScriptTarget::ES2015,
    );
    assert!(
        js.contains("for (obj[(key === null || key === void 0 ? void 0 : key.value)] in {})"),
        "element index has its own expression context: {js}"
    );
    assert!(
        js.contains("(other === null || other === void 0 ? void 0 : other.value) + 1"),
        "loop target precedence must not leak into the next statement: {js}"
    );
}

// ---------------------------------------------------------------
// Destructuring patterns in for-of
// ---------------------------------------------------------------

#[test]
fn test_for_of_array_destructuring_downlevel() {
    let js = emit_es5("for (const [a, b] of pairs) { console.log(a, b); }");
    assert!(
        js.contains(".length"),
        "destructured for-of should be downleveled: {js}"
    );
    assert!(
        js.contains("var _a = pairs_1[_i], a = _a[0], b = _a[1];"),
        "ES5 must extract each binding from the iteration value: {js}"
    );
    assert!(
        !js.contains("[a, b]"),
        "ES5 output must not retain an array binding pattern: {js}"
    );
    assert!(
        !js.contains(" of "),
        "for-of keyword should not appear: {js}"
    );
}

#[test]
fn test_for_of_object_destructuring_downlevel() {
    let js = emit_es5("for (const { name, age } of people) { console.log(name); }");
    assert!(
        js.contains(".length"),
        "destructured for-of should be downleveled: {js}"
    );
    assert!(
        js.contains("name") && js.contains("age"),
        "destructuring pattern names should appear: {js}"
    );
    assert!(
        !js.contains(" of "),
        "for-of keyword should not appear: {js}"
    );
}

// ---------------------------------------------------------------
// downlevelIteration mode (uses __values helper)
// ---------------------------------------------------------------

#[test]
fn test_for_of_downlevel_iteration_uses_values_helper() {
    let js = emit_es5_downlevel_iteration("for (const x of arr) { console.log(x); }");
    assert!(
        js.contains("__values"),
        "downlevelIteration should use __values helper: {js}"
    );
    assert!(
        js.contains(".next()"),
        "downlevelIteration should use iterator .next(): {js}"
    );
    assert!(
        js.contains(".done"),
        "downlevelIteration should check .done: {js}"
    );
    assert!(
        js.contains(".value"),
        "downlevelIteration should access .value: {js}"
    );
}

#[test]
fn test_for_of_downlevel_iteration_emits_try_catch() {
    let js = emit_es5_downlevel_iteration("for (const x of arr) { console.log(x); }");
    assert!(
        js.contains("try"),
        "downlevelIteration should emit try block: {js}"
    );
    assert!(
        js.contains("catch"),
        "downlevelIteration should emit catch block: {js}"
    );
    assert!(
        js.contains("finally"),
        "downlevelIteration should emit finally block: {js}"
    );
}

#[test]
fn test_for_of_downlevel_iteration_emits_values_helper_definition() {
    let js = emit_es5_downlevel_iteration("for (const x of arr) { console.log(x); }");
    assert!(
        js.contains("var __values = "),
        "should emit __values helper definition: {js}"
    );
    assert!(
        js.contains("Symbol.iterator"),
        "__values helper should reference Symbol.iterator: {js}"
    );
}

#[test]
fn test_for_of_no_values_helper_without_downlevel_iteration() {
    let js = emit_es5("for (const x of arr) { console.log(x); }");
    assert!(
        !js.contains("__values"),
        "__values helper should not appear without downlevelIteration: {js}"
    );
}

#[test]
fn test_for_of_downlevel_iteration_with_destructuring() {
    let js = emit_es5_downlevel_iteration("for (const [a, b] of pairs) { console.log(a, b); }");
    assert!(
        js.contains("__values"),
        "destructured for-of with downlevelIteration should use __values: {js}"
    );
    assert!(
        js.contains("var _b = __read(arr_1_1.value, 2), a = _b[0], b = _b[1];"),
        "ES5 must read and extract both iterable bindings: {js}"
    );
    assert!(
        !js.contains("[a, b]"),
        "ES5 output must not retain an array binding pattern: {js}"
    );
}

#[test]
fn test_for_of_downlevel_iteration_existing_identifier_is_assignment() {
    let js = emit_es5_downlevel_iteration("let x; for (x of arr) { console.log(x); }");
    assert!(
        js.contains("x = arr_1_1.value;"),
        "iterator transform should assign the iteration value: {js}"
    );
    assert!(
        !js.contains("var x = arr_1_1.value;"),
        "iterator assignment-form for-of must not redeclare its target: {js}"
    );
}

#[test]
fn test_for_of_downlevel_iteration_declaration_still_declares_binding() {
    let js = emit_es5_downlevel_iteration("for (const x of arr) { console.log(x); }");
    assert!(
        js.contains("var x = arr_1_1.value;"),
        "iterator declaration-form for-of should retain a declaration: {js}"
    );
}

// ---------------------------------------------------------------
// ES3 target
// ---------------------------------------------------------------

#[test]
fn test_for_of_downlevel_es3() {
    let js = emit_with_target(
        "for (const x of arr) { console.log(x); }",
        ScriptTarget::ES3,
    );
    assert!(
        js.contains(".length"),
        "for-of should be downleveled for ES3: {js}"
    );
    assert!(
        !js.contains(" of "),
        "for-of keyword should not appear in ES3 output: {js}"
    );
}

// ---------------------------------------------------------------
// Multiple for-of loops (unique variable names)
// ---------------------------------------------------------------

#[test]
fn test_multiple_for_of_unique_vars() {
    let js = emit_es5(
        "for (const x of arr1) { console.log(x); }\nfor (const y of arr2) { console.log(y); }",
    );
    assert!(
        js.contains(".length"),
        "both for-of loops should be downleveled: {js}"
    );
    // Should not have variable name conflicts
    assert!(
        !js.contains(" of "),
        "no for-of keywords should remain: {js}"
    );
}

// ---------------------------------------------------------------
// For-of with expression body (no block)
// ---------------------------------------------------------------

#[test]
fn test_for_of_expression_body() {
    let js = emit_es5("for (const x of arr) console.log(x);");
    assert!(
        js.contains(".length"),
        "for-of with expression body should be downleveled: {js}"
    );
    assert!(
        !js.contains(" of "),
        "for-of keyword should not appear: {js}"
    );
}
