//! Downlevel emit transformation tests.
//!
//! Tests that modern JavaScript syntax is correctly transformed to older
//! equivalents when the target is set to an earlier ECMAScript version.

use std::io::Write;
use std::process::{Command, Stdio};

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

/// Helper: parse and emit with ES5 target (downlevels everything).
fn emit_es5(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ES5)
}

/// Helper: parse and emit with ESNext target (no downleveling).
fn emit_esnext(source: &str) -> String {
    emit_with_target(source, ScriptTarget::ESNext)
}

fn execute_with_node(javascript: &str) -> String {
    let mut child = Command::new("node")
        .args(["-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node must be available for emitted JavaScript controls");
    child
        .stdin
        .as_mut()
        .expect("node stdin")
        .write_all(javascript.as_bytes())
        .expect("write emitted JavaScript to node");
    let output = child.wait_with_output().expect("wait for node");
    assert!(
        output.status.success(),
        "node failed:\n{}\n{javascript}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("node stdout is UTF-8")
}

#[test]
fn test_es5_lexical_shadow_uses_binding_identity() {
    let js = emit_es5("let x=1; { let x=2; console.log(x); } console.log(x);");
    assert!(
        js.contains("var x = 1;"),
        "outer binding was not lowered: {js}"
    );
    assert!(
        js.contains("var x_1 = 2;") && js.contains("console.log(x_1);"),
        "inner binding/reference were not renamed together: {js}"
    );
    assert!(
        js.ends_with("console.log(x);\n"),
        "outer reference changed binding: {js}"
    );
}

#[test]
fn test_es5_bare_block_enum_has_one_local_declaration() {
    let js = emit_es5("{ enum B {} }");
    assert_eq!(js.matches("var B = void 0;").count(), 1, "{js}");
    assert!(
        !js.contains("let B"),
        "duplicate lexical enum declaration: {js}"
    );
}

#[test]
fn test_es5_lexical_rename_avoids_user_suffixes() {
    let js = emit_es5("let x=1; let x_1=3; { let x=2; console.log(x,x_1); }");
    assert!(
        js.contains("var x_2 = 2;") && js.contains("console.log(x_2, x_1);"),
        "generated rename collided with a user binding: {js}"
    );
}

#[test]
fn test_es5_import_bindings_participate_in_lexical_shadow_renames() {
    for source in [
        "import {value} from './dep'; {let value='inner'; console.log(value)} console.log(value);",
        "import value from './dep'; {let value='inner'; console.log(value)} console.log(value);",
        "import * as value from './dep'; {let value='inner'; console.log(value)} console.log(value);",
        "import value = require('./dep'); {let value='inner'; console.log(value)} console.log(value);",
    ] {
        let js = emit_es5(source);
        assert!(
            js.contains("var value_1 = 'inner';") && js.contains("console.log(value_1);"),
            "import binding did not reserve the emitted name: {js}"
        );
    }
}

#[test]
fn test_es5_type_only_module_bindings_do_not_pollute_structural_lexical_planning() {
    for source in [
        "import type {value} from './dep'; {let value=1;console.log(value)}",
        "import {type value} from './dep'; {let value=1;console.log(value)}",
        "import /*a*/ type /*b*/ value = dep.value; {let value=1;console.log(value)}",
        "export type {value}; {let value=1;console.log(value)}",
        "export {type value}; {let value=1;console.log(value)}",
    ] {
        let js = emit_es5(source);
        assert!(js.contains("var value = 1;"), "{js}");
        assert!(!js.contains("value_1"), "{js}");
        assert!(!js.contains("var _loop_"), "{js}");
    }

    for source in [
        "import type {value} from './dep';var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "import type value from './dep';var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "import type * as value from './dep';var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "import /*a*/ type /*b*/ value = dep.value;var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "import {type value} from './dep';var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "export type {value};var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "export type * from './dep';var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "export type * as value from './dep';var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
        "export {type value};var fs=[];for(let value=0;value<2;value++)fs.push(()=>value);",
    ] {
        let js = emit_es5(source);
        assert!(js.contains("var _loop_"), "{js}");
        assert!(!js.contains("value_1"), "{js}");
    }
}

#[test]
fn test_es5_ambient_namespace_does_not_reserve_runtime_binding_name() {
    let js = emit_es5("declare namespace N {} {let N=2;console.log(N)}");
    assert!(js.contains("var N = 2;"), "{js}");
    assert!(
        !js.contains("N_1"),
        "ambient namespace reserved a runtime name: {js}"
    );
}

#[test]
fn test_es5_unsupported_analysis_bails_out_before_structural_renames() {
    for source in [
        "var x=0; with({x:1}) { let x=2; console.log(x); }",
        "let x=0; { function x(){} let y=1; console.log(y); }",
    ] {
        let js = emit_es5(source);
        assert!(
            !js.contains("_loop_"),
            "unsupported file gained a helper: {js}"
        );
        assert!(
            !js.contains("x_1"),
            "unsupported file received a partial rename: {js}"
        );
    }
}

#[test]
fn test_es5_namespace_binding_participates_in_lexical_shadow_rename() {
    let js = emit_es5("namespace N{export const x=1}{let N=2;console.log(N)}console.log(N.x)");
    assert!(
        js.contains("var N_1 = 2;")
            && js.contains("console.log(N_1);")
            && js.contains("console.log(N.x);"),
        "namespace binding was not resolved structurally: {js}"
    );
}

#[test]
fn test_es5_class_and_function_namespace_merges_keep_inner_shadows_distinct() {
    for (source, renamed, outer) in [
        (
            "class C{} namespace C{export const v=1}{let C=2;console.log(C)}console.log(C.v)",
            "C_1",
            "C.v",
        ),
        (
            "function F(){} namespace F{export const v=1}{let F=2;console.log(F)}console.log(F.v)",
            "F_1",
            "F.v",
        ),
    ] {
        let js = emit_es5(source);
        assert!(js.contains(&format!("var {renamed} = 2;")), "{js}");
        assert!(js.contains(&format!("console.log({renamed});")), "{js}");
        assert!(js.contains(&format!("console.log({outer});")), "{js}");
    }
}

#[test]
fn test_es5_uncaptured_loop_has_no_helper() {
    let js = emit_es5("var s=0; for(let i=0;i<2;i++) s+=i; console.log(s);");
    assert!(js.contains("for (var i = 0; i < 2; i++)"), "{js}");
    assert!(
        !js.contains("_loop_"),
        "uncaptured loop gained a helper: {js}"
    );
}

#[test]
fn test_es2015_lexical_output_is_not_transformed() {
    let js = emit_with_target(
        "let x=1; { let x=2; } var fs=[]; for(let i=0;i<2;i++) fs.push(()=>i);",
        ScriptTarget::ES2015,
    );
    assert_eq!(js.matches("let x").count(), 2, "{js}");
    assert!(js.contains("for (let i = 0; i < 2; i++)"), "{js}");
    assert!(js.contains("() => i"), "{js}");
    assert!(
        !js.contains("_loop_"),
        "ES2015 output was transformed: {js}"
    );
}

// ---------------------------------------------------------------
// Optional chaining downlevel (target < ES2020)
// ---------------------------------------------------------------

#[test]
fn test_optional_chaining_member_downlevel() {
    let js = emit_es5("const x = a?.b;");
    assert!(
        js.contains("=== null") && js.contains("=== void 0") && js.contains("void 0 :"),
        "optional chaining should be downleveled: {js}"
    );
    assert!(
        !js.contains("?."),
        "?. should not appear in downlevel output: {js}"
    );
}

#[test]
fn test_optional_chaining_member_preserved_esnext() {
    let js = emit_esnext("const x = a?.b;");
    assert!(
        js.contains("?."),
        "optional chaining should be preserved for ESNext: {js}"
    );
    assert!(
        !js.contains("=== null"),
        "no null check should appear for ESNext: {js}"
    );
}

#[test]
fn test_optional_chaining_preserved_es2020() {
    let js = emit_with_target("const x = a?.b;", ScriptTarget::ES2020);
    assert!(
        js.contains("?."),
        "optional chaining should be preserved for ES2020: {js}"
    );
}

#[test]
fn test_optional_method_call_downlevel() {
    let js = emit_es5("const x = a?.b();");
    assert!(
        js.contains("=== null") && js.contains("=== void 0"),
        "optional call should be downleveled: {js}"
    );
    assert!(
        !js.contains("?."),
        "?. should not appear in downlevel output: {js}"
    );
}

#[test]
fn test_optional_elem_access_downlevel() {
    let js = emit_es5("const x = a?.[0];");
    assert!(
        js.contains("=== null") && js.contains("=== void 0"),
        "optional element access should be downleveled: {js}"
    );
    assert!(!js.contains("?."), "?. should not appear: {js}");
}

#[test]
fn test_optional_chaining_produces_conditional() {
    let js = emit_es5("const x = obj?.prop;");
    // Should produce: obj === null || obj === void 0 ? void 0 : obj.prop
    assert!(js.contains("=== null"), "should check null: {js}");
    assert!(js.contains("=== void 0"), "should check undefined: {js}");
    assert!(
        js.contains("void 0 :"),
        "should have void 0 alternate: {js}"
    );
    assert!(js.contains(".prop"), "should access .prop: {js}");
}

#[test]
fn test_optional_call_with_args_downlevel() {
    let js = emit_es5("const x = a?.b(1, 2);");
    assert!(
        js.contains("=== null") && js.contains("1, 2"),
        "optional call args should be preserved: {js}"
    );
}

#[test]
fn test_optional_chaining_nested_downlevel() {
    let js = emit_es5("const x = a?.b?.c;");
    // Both levels of optional chaining should be downleveled
    assert!(!js.contains("?."), "no ?. in nested downlevel: {js}");
    // Should contain multiple null checks
    let null_count = js.matches("=== null").count();
    assert!(
        null_count >= 2,
        "nested optional chaining should have multiple null checks ({null_count}): {js}"
    );
}

// ---------------------------------------------------------------
// Nullish coalescing downlevel (target < ES2020)
// ---------------------------------------------------------------

#[test]
fn test_nullish_coalescing_downlevel() {
    let js = emit_es5("const x = a ?? b;");
    assert!(
        js.contains("!== null") && js.contains("!== void 0"),
        "nullish coalescing should be downleveled: {js}"
    );
    assert!(
        !js.contains("??"),
        "?? should not appear in downlevel output: {js}"
    );
}

#[test]
fn test_nullish_coalescing_preserved_esnext() {
    let js = emit_esnext("const x = a ?? b;");
    assert!(js.contains("??"), "?? should be preserved for ESNext: {js}");
}

#[test]
fn test_nullish_coalescing_preserved_es2020() {
    let js = emit_with_target("const x = a ?? b;", ScriptTarget::ES2020);
    assert!(js.contains("??"), "?? should be preserved for ES2020: {js}");
}

#[test]
fn test_nullish_coalescing_uses_temp_var() {
    // Complex expression (not a simple identifier) uses temp var
    let js = emit_es5("const x = a.b ?? c;");
    assert!(
        js.contains("_a"),
        "nullish coalescing should use temp var for complex expr: {js}"
    );
    // Simple identifier does NOT use temp var (matches TypeScript behavior)
    let js2 = emit_es5("const x = a ?? b;");
    assert!(
        !js2.contains("_a"),
        "nullish coalescing should not use temp var for simple ident: {js2}"
    );
}

#[test]
fn test_nullish_coalescing_conditional_form() {
    let js = emit_es5("const x = a ?? b;");
    // Should produce: (_a = a) !== null && _a !== void 0 ? _a : b
    assert!(js.contains("!== null"), "should check not-null: {js}");
    assert!(
        js.contains("!== void 0"),
        "should check not-undefined: {js}"
    );
}

// ---------------------------------------------------------------
// Logical assignment downlevel (target < ES2021)
// ---------------------------------------------------------------

#[test]
fn test_nullish_coalescing_assign_downlevel() {
    let js = emit_es5("x ??= y;");
    assert!(
        js.contains("!== null") && js.contains("!== void 0"),
        "??= should be downleveled: {js}"
    );
    assert!(
        !js.contains("??="),
        "??= should not appear in downlevel: {js}"
    );
    // Should contain an assignment
    assert!(js.contains("x = y"), "should contain assignment: {js}");
}

#[test]
fn test_logical_and_assign_downlevel() {
    let js = emit_es5("x &&= y;");
    assert!(
        js.contains("&&") && js.contains("x = y"),
        "&&= should be downleveled to && and assignment: {js}"
    );
    assert!(!js.contains("&&="), "&&= should not appear: {js}");
}

#[test]
fn test_logical_or_assign_downlevel() {
    let js = emit_es5("x ||= y;");
    assert!(
        js.contains("||") && js.contains("x = y"),
        "||= should be downleveled to || and assignment: {js}"
    );
    assert!(!js.contains("||="), "||= should not appear: {js}");
}

#[test]
fn test_parenthesized_logical_assignment_members_match_typescript() {
    let source = "(a.value) &&= x;\n(b.value) ||= y;\n(c.value) ??= z;";
    assert_eq!(
        emit_with_target(source, ScriptTarget::ES2015),
        concat!(
            "\"use strict\";\n",
            "var _a;\n",
            "a.value && (a.value = x);\n",
            "b.value || (b.value = y);\n",
            "(_a = c.value) !== null && _a !== void 0 ? _a : (c.value = z);\n",
        )
    );
    assert_eq!(
        emit_with_target(source, ScriptTarget::ES2020),
        concat!(
            "\"use strict\";\n",
            "a.value && (a.value = x);\n",
            "b.value || (b.value = y);\n",
            "c.value ?? (c.value = z);\n",
        )
    );
}

#[test]
fn test_logical_assignment_arrow_rhs_and_comma_controls_match_typescript() {
    let source = concat!(
        "function arrows(f?: (a: number) => number) {\n",
        "  f ??= (a => a); f ||= (a => a); f &&= (a => a);\n",
        "}\n",
        "function commas(f?: (a: number) => number) {\n",
        "  f ??= (f.toString(), (a => a));\n",
        "  f ||= (f.toString(), (a => a));\n",
        "  f &&= (f.toString(), (a => a));\n",
        "}",
    );
    let es2015 = concat!(
        "\"use strict\";\n",
        "function arrows(f) {\n",
        "    f !== null && f !== void 0 ? f : (f = a => a);\n",
        "    f || (f = a => a);\n",
        "    f && (f = a => a);\n",
        "}\n",
        "function commas(f) {\n",
        "    f !== null && f !== void 0 ? f : (f = (f.toString(), (a => a)));\n",
        "    f || (f = (f.toString(), (a => a)));\n",
        "    f && (f = (f.toString(), (a => a)));\n",
        "}\n",
    );
    let es2020 = es2015.replace("f !== null && f !== void 0 ? f : (f =", "f ?? (f =");
    assert_eq!(emit_with_target(source, ScriptTarget::ES2015), es2015);
    assert_eq!(emit_with_target(source, ScriptTarget::ES2020), es2020);
}

#[test]
fn test_nested_logical_assignment_rhs_matches_typescript_before_member_access() {
    let source = concat!(
        "function all(a: number[] | undefined, b: number[] | undefined) {\n",
        "  (a ||= (b ||= [])).push(1);\n",
        "  (a ??= (b ??= [])).push(2);\n",
        "  (a &&= (b &&= [])).push(3);\n",
        "}",
    );
    assert_eq!(
        emit_with_target(source, ScriptTarget::ES2015),
        concat!(
            "\"use strict\";\n",
            "function all(a, b) {\n",
            "    (a || (a = b || (b = []))).push(1);\n",
            "    (a !== null && a !== void 0 ? a : (a = b !== null && b !== void 0 ? b : (b = []))).push(2);\n",
            "    (a && (a = b && (b = []))).push(3);\n",
            "}\n",
        )
    );
    assert_eq!(
        emit_with_target(source, ScriptTarget::ES2020),
        concat!(
            "\"use strict\";\n",
            "function all(a, b) {\n",
            "    (a || (a = b || (b = []))).push(1);\n",
            "    (a ?? (a = b ?? (b = []))).push(2);\n",
            "    (a && (a = b && (b = []))).push(3);\n",
            "}\n",
        )
    );
}

#[test]
fn test_logical_assignment_parenthesis_normalization_fails_closed() {
    for (source, unsafe_normalized) in [
        ("(obj[key]) ||= rhs;", "obj[key] || (obj[key] = rhs)"),
        (
            "(get().value) ||= rhs;",
            "get().value || (get().value = rhs)",
        ),
        (
            "(obj.inner.value) ||= rhs;",
            "obj.inner.value || (obj.inner.value = rhs)",
        ),
        ("(obj?.value) ||= rhs;", "obj?.value || (obj?.value = rhs)"),
        ("(this.value) ||= rhs;", "this.value || (this.value = rhs)"),
        (
            "(super.value) ||= rhs;",
            "super.value || (super.value = rhs)",
        ),
        (
            "((obj as any).value) ||= rhs;",
            "obj.value || (obj.value = rhs)",
        ),
        ("((x!)) ||= rhs;", "x || (x = rhs)"),
        (
            "(/* target */ obj.value) ||= rhs;",
            "obj.value || (obj.value = rhs)",
        ),
    ] {
        let js = emit_with_target(source, ScriptTarget::ES2020);
        assert!(
            !js.contains(unsafe_normalized),
            "unsafe target was normalized as a stable direct member for {source:?}: {js}"
        );
    }

    let nested = emit_with_target(
        "x ||= ((get().value) ||= []); f ||= ((a => a));",
        ScriptTarget::ES2020,
    );
    assert!(
        nested.contains("x = ((get().value) || ((get().value) = []))"),
        "unstable nested assignment lost its grouping: {nested}"
    );
    assert!(
        nested.contains("f = ((a => a))"),
        "more than one arrow parenthesis was removed: {nested}"
    );
}

#[test]
fn test_logical_assignment_parenthesis_rules_preserve_es2021_syntax() {
    let source = concat!(
        "(a.value) ||= (b.value ||= []);\n",
        "(a.value) ??= (x => x);\n",
        "(a.value) &&= (x.toString(), (y => y));",
    );
    let js = emit_with_target(source, ScriptTarget::ES2021);
    assert!(
        js.contains("(a.value) ||= (b.value ||= [])")
            && js.contains("(a.value) ??= (x => x)")
            && js.contains("(a.value) &&= (x.toString(), (y => y))"),
        "ES2021 logical assignments should be preserved: {js}"
    );
}

#[test]
fn test_logical_assignment_parenthesis_runtime_short_circuits_and_nests() {
    let source = r#"
let reads = 0, writes = 0, rhsCalls = 0, stored;
const obj = {
    get value() { reads++; return stored; },
    set value(value) { writes++; stored = value; }
};
function rhs(value) { rhsCalls++; return value; }
stored = 0; (obj.value) ||= rhs(2); (obj.value) ||= rhs(3);
stored = 0; (obj.value) &&= rhs(3);
stored = 3; (obj.value) &&= rhs(4);
stored = 5; (obj.value) ??= rhs(5);
stored = undefined; (obj.value) ??= rhs(6);

let calls = 0;
function build(value) { calls++; return [value]; }
let a, b; (a ||= (b ||= build(1))).push(2); (a ||= (b ||= build(9))).push(3);
let c, d; (c ??= (d ??= build(3))).push(4); (c ??= (d ??= build(9))).push(5);
let e = [0], f = [9]; (e &&= (f &&= build(5))).push(6);
console.log(reads, writes, rhsCalls, stored, calls, a.join(), c.join(), e.join());
"#;
    for target in [ScriptTarget::ES2015, ScriptTarget::ES2020] {
        let js = emit_with_target(source, target);
        assert_eq!(
            execute_with_node(&js),
            "6 3 3 6 3 1,2,3 3,4,5 5,6\n",
            "{target:?}: {js}"
        );
    }
}

#[test]
fn test_logical_assignment_preserved_esnext() {
    let js = emit_esnext("x ??= y;");
    assert!(
        js.contains("??="),
        "??= should be preserved for ESNext: {js}"
    );
}

#[test]
fn test_logical_assignment_preserved_es2021() {
    let js = emit_with_target("x ??= y;", ScriptTarget::ES2021);
    assert!(
        js.contains("??="),
        "??= should be preserved for ES2021: {js}"
    );
}

#[test]
fn test_logical_and_assign_preserved_es2021() {
    let js = emit_with_target("x &&= y;", ScriptTarget::ES2021);
    assert!(
        js.contains("&&="),
        "&&= should be preserved for ES2021: {js}"
    );
}

#[test]
fn test_logical_or_assign_preserved_es2021() {
    let js = emit_with_target("x ||= y;", ScriptTarget::ES2021);
    assert!(
        js.contains("||="),
        "||= should be preserved for ES2021: {js}"
    );
}

#[test]
fn test_es2020_nullish_assignment_uses_partial_lowering_for_stable_targets() {
    assert_eq!(
        emit_with_target("let x; x ??= 1;", ScriptTarget::ES2020),
        "\"use strict\";\nlet x;\nx ?? (x = 1);\n"
    );
    assert_eq!(
        emit_with_target("let obj; (obj.value) ??= 1;", ScriptTarget::ES2020),
        "\"use strict\";\nlet obj;\nobj.value ?? (obj.value = 1);\n"
    );
    assert_eq!(
        emit_with_target("a ??= b ??= c;", ScriptTarget::ES2020),
        "\"use strict\";\na ?? (a = b ?? (b = c));\n"
    );
    assert_eq!(
        emit_with_target("a ??= obj?.value ?? fallback;", ScriptTarget::ES2020),
        "\"use strict\";\na ?? (a = obj?.value ?? fallback);\n"
    );
    assert_eq!(
        emit_with_target(
            "function f(a?: number[], b?: number[]) { (a ??= (b ??= [])).push(1); }",
            ScriptTarget::ES2020,
        ),
        concat!(
            "\"use strict\";\n",
            "function f(a, b) { (a ?? (a = b ?? (b = []))).push(1); }\n",
        )
    );
}

#[test]
fn test_es2020_nullish_assignment_partial_lowering_target_controls() {
    let es2015 = emit_with_target("x ??= y;", ScriptTarget::ES2015);
    assert!(es2015.contains("x !== null && x !== void 0"), "{es2015}");
    assert!(!es2015.contains("x ?? (x = y)"), "{es2015}");

    assert_eq!(
        emit_with_target("x ??= y; x &&= y; x ||= y;", ScriptTarget::ES2020),
        "\"use strict\";\nx ?? (x = y);\nx && (x = y);\nx || (x = y);\n"
    );
    assert_eq!(
        emit_with_target("x ??= y; x &&= y; x ||= y;", ScriptTarget::ES2021),
        "\"use strict\";\nx ??= y;\nx &&= y;\nx ||= y;\n"
    );
}

#[test]
fn test_es2020_nullish_assignment_partial_lowering_expression_precedence() {
    assert_eq!(
        emit_with_target(
            concat!(
                "(x ??= y).p; f(x ??= y); ",
                "(x ??= y) ? a : b; (x ??= y) + z; ",
                "(x ??= y) || z; true ? x ??= y : z; x ??= y ||= z;",
            ),
            ScriptTarget::ES2020,
        ),
        concat!(
            "\"use strict\";\n",
            "(x ?? (x = y)).p;\n",
            "f(x ?? (x = y));\n",
            "(x ?? (x = y)) ? a : b;\n",
            "(x ?? (x = y)) + z;\n",
            "(x ?? (x = y)) || z;\n",
            "true ? x ?? (x = y) : z;\n",
            "x ?? (x = y || (y = z));\n",
        )
    );
}

#[test]
fn test_es2020_nullish_assignment_partial_lowering_fails_closed() {
    for source in [
        "obj[key] ??= value;",
        "getObj().value ??= value;",
        "obj.inner.value ??= value;",
        "obj?.value ??= value;",
        "this.value ??= value;",
        "super.value ??= value;",
        "(obj as any).value ??= value;",
        "(x!) ??= value;",
        "(x satisfies unknown) ??= value;",
        "#value ??= value;",
        "this.#value ??= value;",
        "(/* target */ x) ??= value;",
        "obj /* target */.value ??= value;",
        "x ??= /* rhs */ value;",
        "x ??= ;",
    ] {
        let js = emit_with_target(source, ScriptTarget::ES2020);
        assert!(
            !js.contains(" ?? ("),
            "unsafe target used partial lowering for {source:?}: {js}"
        );
    }
}

#[test]
fn test_es2020_nullish_assignment_direct_member_runtime_reads_once() {
    let js = emit_with_target(
        r#"
let reads = 0, writes = 0, rhsCalls = 0;
let stored;
const obj = {
    get value() { reads++; return stored; },
    set value(value) { writes++; stored = value; }
};
function rhs() { rhsCalls++; return 42; }
obj.value ??= rhs();
obj.value ??= rhs();
console.log(reads, writes, rhsCalls, stored);
"#,
        ScriptTarget::ES2020,
    );
    assert!(js.contains("obj.value ?? (obj.value = rhs())"), "{js}");
    assert_eq!(execute_with_node(&js), "2 1 1 42\n", "{js}");
}

// ---------------------------------------------------------------
// Object spread downlevel (target < ES2018)
// ---------------------------------------------------------------

#[test]
fn test_object_spread_downlevel() {
    let js = emit_es5("const x = {...a, b: 1};");
    assert!(
        js.contains("Object.assign("),
        "object spread should be downleveled to Object.assign: {js}"
    );
    assert!(!js.contains("..."), "spread should not appear: {js}");
}

#[test]
fn test_object_spread_preserved_esnext() {
    let js = emit_esnext("const x = {...a, b: 1};");
    assert!(
        js.contains("..."),
        "spread should be preserved for ESNext: {js}"
    );
    assert!(
        !js.contains("Object.assign"),
        "Object.assign should not appear for ESNext: {js}"
    );
}

#[test]
fn test_object_spread_preserved_es2018() {
    let js = emit_with_target("const x = {...a, b: 1};", ScriptTarget::ES2018);
    assert!(
        js.contains("..."),
        "spread should be preserved for ES2018: {js}"
    );
}

#[test]
fn test_object_spread_starts_with_empty_obj() {
    let js = emit_es5("const x = {...a};");
    // When spread is first, should start with {} target
    assert!(
        js.contains("Object.assign({}"),
        "should start with empty obj: {js}"
    );
}

#[test]
fn test_object_spread_multiple_spreads() {
    let js = emit_es5("const x = {...a, ...b};");
    assert!(
        js.contains("Object.assign("),
        "multiple spreads should use Object.assign: {js}"
    );
}

// ---------------------------------------------------------------
// Mixed features / edge cases
// ---------------------------------------------------------------

#[test]
fn test_no_downlevel_when_target_is_high() {
    // When target is ESNext, all modern syntax should be preserved
    let js = emit_esnext("const x = a?.b ?? c;");
    assert!(js.contains("?."), "?. preserved: {js}");
    assert!(js.contains("??"), "?? preserved: {js}");
}

#[test]
fn test_regular_member_not_affected() {
    // Non-optional member access should not be changed
    let js = emit_es5("const x = a.b;");
    assert!(
        js.contains("a.b"),
        "regular member access should be unchanged: {js}"
    );
    assert!(
        !js.contains("=== null"),
        "no null check for regular member: {js}"
    );
}

#[test]
fn test_regular_binary_not_affected() {
    // Non-nullish-coalescing binary ops should not be changed
    let js = emit_es5("const x = a || b;");
    assert!(js.contains("||"), "regular || should be preserved: {js}");
    assert!(
        !js.contains("!== null"),
        "no null check for regular ||: {js}"
    );
}

#[test]
fn test_regular_assign_not_affected() {
    let js = emit_es5("x += 1;");
    assert!(js.contains("+="), "+= should be preserved: {js}");
}

#[test]
fn test_es2019_target_downlevels_optional_chaining() {
    let js = emit_with_target("const x = a?.b;", ScriptTarget::ES2019);
    assert!(
        !js.contains("?."),
        "?. should be downleveled for ES2019: {js}"
    );
}

#[test]
fn test_es2020_target_downlevels_logical_assignment() {
    let js = emit_with_target("x ??= y;", ScriptTarget::ES2020);
    assert!(
        !js.contains("??="),
        "??= should be downleveled for ES2020: {js}"
    );
}

#[test]
fn test_es5_lowers_constructor_only_classes_and_extends() {
    let js = emit_es5(
        "class Base { constructor(value: number) { this.value = value; } }\n\
         class Derived extends Base {}",
    );
    assert!(js.contains("var __extends = "), "missing helper: {js}");
    assert!(
        js.contains("var Base = /** @class */ (function () {")
            && js.contains("function Base(value) {")
            && js.contains("var Derived = /** @class */ (function (_super) {")
            && js.contains("__extends(Derived, _super);")
            && js.contains("return _super !== null && _super.apply(this, arguments) || this;"),
        "classes should use the ES5 IIFE transform: {js}"
    );
    assert!(!js.contains("class Base"), "native class leaked: {js}");
}

#[test]
fn test_es5_lowers_nested_super_calls_through_receiver_capture() {
    let js = emit_es5(
        "class Base { constructor(fail: boolean) { if (fail) throw new Error(); } }\n\
         class Derived extends Base { constructor() { try { super(true); } catch (e) { super(false); } } }",
    );
    assert!(
        js.contains("var _this = this;"),
        "receiver was not captured: {js}"
    );
    assert_eq!(
        js.matches("_this = _super.call(this").count(),
        2,
        "each nested super call must update the receiver: {js}"
    );
    assert!(
        js.contains("return _this;"),
        "captured receiver not returned: {js}"
    );
}

#[test]
fn test_es2015_preserves_constructor_only_class_syntax() {
    let js = emit_with_target("class Derived extends Base {}", ScriptTarget::ES2015);
    assert!(
        js.contains("class Derived extends Base"),
        "class was lowered: {js}"
    );
    assert!(
        !js.contains("var __extends"),
        "helper should not be emitted: {js}"
    );
}
