//! Tests for import elision (type-only import removal).
//!
//! When imports are used only in type positions (type annotations, interfaces, etc.),
//! they should be elided from the JavaScript output. This matches TypeScript's
//! default `importsNotUsedAsValues: "remove"` behaviour.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use tsc_rs_ast::*;
use tsc_rs_emitter::{emit, emit_with_const_enum_values, ConstEnumValue};

/// Helper: parse and emit with CommonJS options.
fn emit_ts(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with ESNext module mode.
fn emit_ts_esm(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with specific options.
fn emit_ts_with(source: &str, opts: CompilerOptions) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let out = emit(&file, &opts);
    out.javascript
}

fn emit_export_pattern_module(source: &str, module: ModuleKind) -> String {
    emit_ts_with(
        source,
        CompilerOptions {
            module: Some(module),
            target: Some(ScriptTarget::ESNext),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    )
}

fn execute_system_module(javascript: &str, result_expression: &str) -> String {
    let runtime = format!(
        r#"
function __rest(source, excluded) {{
    const target = {{}};
    for (const key in source) {{
        if (Object.prototype.hasOwnProperty.call(source, key) && !excluded.includes(key)) {{
            target[key] = source[key];
        }}
    }}
    return target;
}}
const captured = Object.create(null);
const System = {{
    register(...args) {{
        const declare = args[args.length - 1];
        const registration = declare((name, value) => {{
            if (typeof name === "object") Object.assign(captured, name);
            else captured[name] = value;
            return value;
        }}, {{ id: "test" }});
        for (const setter of registration.setters) setter({{}});
        try {{
            registration.execute();
        }} catch (error) {{
            captured.__error = {{ name: error.name, message: error.message }};
        }}
    }}
}};
{javascript}
console.log(JSON.stringify({result_expression}));
"#
    );
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node must be available for System emit runtime controls");
    child
        .stdin
        .as_mut()
        .expect("node stdin")
        .write_all(runtime.as_bytes())
        .expect("write System emit runtime");
    let output = child.wait_with_output().expect("wait for node");
    assert!(
        output.status.success(),
        "System emit runtime failed:\n{}\n{javascript}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("node stdout is UTF-8")
        .trim()
        .to_string()
}

#[test]
fn test_amd_export_empty_binding_patterns_keep_rhs_evaluation() {
    for (pattern, initializer) in [("[]", "[]"), ("{}", "{}")] {
        let js = emit_export_pattern_module(
            &format!("export const {pattern} = {initializer};"),
            ModuleKind::AMD,
        );
        assert!(
            js.contains("\"use strict\";\n    var _a;\n    Object.defineProperty"),
            "AMD temp should be declared inside the factory before __esModule: {js}"
        );
        assert!(
            js.contains(&format!("    _a = {initializer};")),
            "AMD emit should evaluate the destructuring RHS once: {js}"
        );
    }
}

#[test]
fn test_system_export_patterns_keep_evaluation_and_exports() {
    for (pattern, initializer) in [("[]", "[]"), ("{}", "{}")] {
        let js = emit_export_pattern_module(
            &format!("export const {pattern} = {initializer};"),
            ModuleKind::System,
        );
        assert!(
            js.contains("    var _a;\n    var __moduleName"),
            "System temp should be scoped to the register factory: {js}"
        );
        assert!(
            js.contains(&format!("            _a = {initializer};")),
            "System emit should evaluate the destructuring RHS once: {js}"
        );
    }

    let js = emit_export_pattern_module(
        "export const { x, ...rest } = { x: 'x', y: 'y' };",
        ModuleKind::System,
    );
    assert!(
        js.contains("_a = { x: 'x', y: 'y' }, exports_1(\"x\", x = _a.x), exports_1(\"rest\", rest = __rest(_a, [\"x\"]));"),
        "System object-rest export should evaluate once and publish both bindings: {js}"
    );
}

#[test]
fn test_system_object_rest_defaults_preserve_values_and_evaluation_order() {
    let js = emit_export_pattern_module(
        r#"
export const events: string[] = [];
function fallback(label: string) {
    events.push("default:" + label);
    return label + "-fallback";
}
function rhs(label: string, value: any) {
    events.push("rhs:" + label);
    return {
        get x() { events.push("get:" + label); return value; },
        keep: label
    };
}
export const { x: present = fallback("present"), ...presentRest } = rhs("present", "value"),
    { x: missing = fallback("missing"), ...missingRest } = rhs("missing", undefined),
    { x = fallback("shorthand"), ...shorthandRest } = rhs("shorthand", undefined);
"#,
        ModuleKind::System,
    );
    assert!(
        js.contains("=== void 0 ? fallback(\"missing\")"),
        "System alias defaults must retain their initializer: {js}"
    );
    assert!(
        js.contains("=== void 0 ? fallback(\"shorthand\")"),
        "System shorthand defaults must retain their initializer: {js}"
    );
    let result = execute_system_module(
        &js,
        r#"({
            present: captured.present,
            missing: captured.missing,
            shorthand: captured.x,
            presentRest: captured.presentRest,
            missingRest: captured.missingRest,
            shorthandRest: captured.shorthandRest,
            events: captured.events
        })"#,
    );
    assert_eq!(
        result,
        r#"{"present":"value","missing":"missing-fallback","shorthand":"shorthand-fallback","presentRest":{"keep":"present"},"missingRest":{"keep":"missing"},"shorthandRest":{"keep":"shorthand"},"events":["rhs:present","get:present","rhs:missing","get:missing","default:missing","rhs:shorthand","get:shorthand","default:shorthand"]}"#
    );
}

#[test]
fn test_system_nested_static_key_rest_preserves_access_and_exclusions() {
    let js = emit_export_pattern_module(
        r#"
export const events: string[] = [];
function rhs() {
    events.push("rhs");
    return {
        get nested() { events.push("nested"); return { value: 3 }; },
        "fixed-key": 4,
        extra: 5
    };
}
export const { nested: { value }, "fixed-key": fixed, ...rest } = rhs();
"#,
        ModuleKind::System,
    );
    let result = execute_system_module(
        &js,
        "({ value: captured.value, fixed: captured.fixed, rest: captured.rest, events: captured.events })",
    );
    assert_eq!(
        result,
        r#"{"value":3,"fixed":4,"rest":{"extra":5},"events":["rhs","nested"]}"#
    );
}

#[test]
fn test_system_empty_nested_patterns_evaluate_getters_and_preserve_native_errors() {
    for (pattern, value, expected_error) in [
        ("{}", "({})", None),
        ("{}", "[]", None),
        ("{}", "null", Some("TypeError")),
        ("[]", "[]", None),
        ("[]", "({})", Some("TypeError")),
        ("[]", "null", Some("TypeError")),
    ] {
        let js = emit_export_pattern_module(
            &format!(
                r#"
export const events: string[] = [];
function rhs() {{
    events.push("rhs");
    return {{
        get nested() {{ events.push("nested"); return {value}; }},
        keep: 1
    }};
}}
export const {{ nested: {pattern}, ...rest }} = rhs();
"#
            ),
            ModuleKind::System,
        );
        let result = execute_system_module(
            &js,
            "({ events: captured.events, rest: captured.rest, error: captured.__error && captured.__error.name })",
        );
        if let Some(error) = expected_error {
            assert_eq!(
                result,
                format!(r#"{{"events":["rhs","nested"],"error":"{error}"}}"#),
                "pattern={pattern}, value={value}, js={js}"
            );
        } else {
            assert_eq!(
                result, r#"{"events":["rhs","nested"],"rest":{"keep":1}}"#,
                "pattern={pattern}, value={value}, js={js}"
            );
        }
    }
}

#[test]
fn test_system_empty_nested_patterns_propagate_getter_throws_once() {
    for pattern in ["{}", "[]"] {
        let js = emit_export_pattern_module(
            &format!(
                r#"
export const events: string[] = [];
function rhs() {{
    events.push("rhs");
    return {{
        get nested() {{ events.push("nested"); throw new Error("boom"); }},
        keep: 1
    }};
}}
export const {{ nested: {pattern}, ...rest }} = rhs();
"#
            ),
            ModuleKind::System,
        );
        let result = execute_system_module(
            &js,
            "({ events: captured.events, rest: captured.rest, error: captured.__error })",
        );
        assert_eq!(
            result, r#"{"events":["rhs","nested"],"error":{"name":"Error","message":"boom"}}"#,
            "pattern={pattern}, js={js}"
        );
    }
}

#[test]
fn test_system_unsupported_computed_default_rest_falls_back_atomically() {
    let source = r#"
const key = () => "x";
const fallback = () => 1;
const rhs = () => ({ x: undefined, y: 2 });
export const { [key()]: value = fallback(), ...rest } = rhs();
"#;
    let js = emit_export_pattern_module(source, ModuleKind::System);
    assert!(
        !js.contains("System.register("),
        "unsupported computed/default patterns must leave the specialized System path: {js}"
    );
    assert!(
        js.contains("[key()]: value = fallback()") && js.contains("...rest"),
        "fallback emit must preserve the complete pattern instead of partially lowering it: {js}"
    );
}

#[test]
fn test_object_rest_default_controls_remain_module_format_specific() {
    let source = "export const { x: alias = 1, ...rest } = source;";
    let amd = emit_export_pattern_module(source, ModuleKind::AMD);
    assert!(amd.contains("define(["));
    assert!(!amd.contains("System.register"));

    let cjs = emit_export_pattern_module(source, ModuleKind::CommonJS);
    assert!(cjs.contains("exports.alias"));
    assert!(!cjs.contains("define(["));
    assert!(!cjs.contains("System.register"));

    let esm = emit_export_pattern_module(source, ModuleKind::ESNext);
    assert!(esm.contains("export const { x: alias = 1, ...rest } = source;"));
    assert!(!esm.contains("exports.alias"));
    assert!(!esm.contains("System.register"));
}

#[test]
fn test_export_pattern_module_controls_remain_format_specific() {
    let source = "export const { x, ...rest } = { x: 'x', y: 'y' };";
    let cjs = emit_export_pattern_module(source, ModuleKind::CommonJS);
    assert!(cjs.contains("exports.x = _a.x, exports.rest = __rest(_a, [\"x\"]);"));
    assert!(!cjs.contains("define(["));
    assert!(!cjs.contains("System.register"));

    let esm = emit_export_pattern_module(source, ModuleKind::ESNext);
    assert!(esm.contains("export const { x, ...rest } = { x: 'x', y: 'y' };"));
    assert!(!esm.contains("exports.x"));
    assert!(!esm.contains("System.register"));
}

// ---------------------------------------------------------------
// Type-only import declaration (import type { ... })
// ---------------------------------------------------------------

#[test]
fn test_type_only_import_elided() {
    let js = emit_ts("import type { Foo } from './types';\nconst x = 1;");
    assert!(
        !js.contains("require(\"./types\")"),
        "type-only import should be completely elided: {js}"
    );
}

#[test]
fn test_type_only_import_elided_esm() {
    let js = emit_ts_esm("import type { Foo } from './types';\nconst x = 1;");
    assert!(
        !js.contains("import"),
        "type-only import should be completely elided in ESM: {js}"
    );
}

// ---------------------------------------------------------------
// Named imports: type-only specifiers with `import { type X }`
// ---------------------------------------------------------------

#[test]
fn test_inline_type_specifier_removed() {
    let js = emit_ts_esm("import { type Foo, bar } from './mod';\nconsole.log(bar);");
    assert!(
        !js.contains("Foo"),
        "inline type specifier Foo should be removed: {js}"
    );
    assert!(
        js.contains("bar"),
        "value specifier bar should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Import elision: import used only in type annotation
// ---------------------------------------------------------------

#[test]
fn test_import_used_only_as_type_annotation_elided_esm() {
    let js = emit_ts_esm("import { Foo } from './types';\nconst x: Foo = {} as any;");
    // Foo is only used in a type annotation, so the entire import should be elided
    assert!(
        !js.contains("import"),
        "import used only as type annotation should be elided: {js}"
    );
}

#[test]
fn test_import_used_only_as_type_annotation_elided_cjs() {
    let js = emit_ts("import { Foo } from './types';\nconst x: Foo = {} as any;");
    // Foo is only used in a type annotation, so the import should be elided
    assert!(
        !js.contains("require"),
        "CJS import used only as type annotation should be elided: {js}"
    );
}

#[test]
fn test_resolved_type_only_import_local_alias_is_elided_when_reexported() {
    let js = emit_ts_with(
        "import { Foo as Bar } from './types';\nexport { Bar };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            other: vec![(
                "__tsrsResolvedTypeOnlyImportLocals".to_string(),
                "Bar".to_string(),
            )],
            ..Default::default()
        },
    );
    assert!(
        !js.contains("require(\"./types\")"),
        "resolved type-only import alias should be elided: {js}"
    );
    assert!(
        !js.contains("exports.Bar"),
        "resolved type-only import alias should not emit a runtime export binding: {js}"
    );
}

// ---------------------------------------------------------------
// Import used in value position: should be kept
// ---------------------------------------------------------------

#[test]
fn test_import_used_as_value_kept_esm() {
    let js = emit_ts_esm("import { Foo } from './mod';\nconst x = new Foo();");
    assert!(
        js.contains("import"),
        "import used as value should be kept: {js}"
    );
    assert!(js.contains("Foo"), "Foo should appear in the output: {js}");
}

#[test]
fn test_import_used_as_value_kept_cjs() {
    let js = emit_ts("import { Foo } from './mod';\nconst x = new Foo();");
    assert!(
        js.contains("require"),
        "CJS import used as value should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Mixed: some specifiers used as value, some as type only
// ---------------------------------------------------------------

#[test]
fn test_mixed_import_partial_elision_esm() {
    let js =
        emit_ts_esm("import { Foo, Bar } from './mod';\nconst x = new Foo();\nconst y: Bar = x;");
    assert!(js.contains("Foo"), "Foo (value-used) should be kept: {js}");
    assert!(
        !js.contains("Bar"),
        "Bar (type-only) should be elided from import: {js}"
    );
}

// ---------------------------------------------------------------
// Default import elision
// ---------------------------------------------------------------

#[test]
fn test_default_import_type_only_elided_esm() {
    let js = emit_ts_esm("import Foo from './mod';\nconst x: Foo = 1 as any;");
    assert!(
        !js.contains("import"),
        "default import used only as type should be elided: {js}"
    );
}

#[test]
fn test_default_import_value_used_kept_esm() {
    let js = emit_ts_esm("import Foo from './mod';\nconst x = Foo();");
    assert!(
        js.contains("import Foo"),
        "default import used as value should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Namespace import elision
// ---------------------------------------------------------------

#[test]
fn test_namespace_import_type_only_elided_esm() {
    let js = emit_ts_esm("import * as ns from './mod';\nconst x: ns.Type = 1 as any;");
    // ns is only used in type position (ns.Type is a type reference)
    // Note: ns.Type as a type annotation is erased, so ns doesn't appear in value context
    assert!(
        !js.contains("import * as ns"),
        "namespace import used only as type should be elided: {js}"
    );
}

#[test]
fn test_namespace_import_value_used_kept_esm() {
    let js = emit_ts_esm("import * as ns from './mod';\nconst x = ns.foo();");
    assert!(
        js.contains("import * as ns"),
        "namespace import used as value should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Side-effect imports (no specifiers)
// ---------------------------------------------------------------

#[test]
fn test_side_effect_import_always_kept_esm() {
    let js = emit_ts_esm("import './side-effects';");
    assert!(
        js.contains("import './side-effects'"),
        "side-effect import should always be kept: {js}"
    );
}

#[test]
fn test_side_effect_import_always_kept_cjs() {
    let js = emit_ts("import './side-effects';");
    assert!(
        js.contains("require(\"./side-effects\")"),
        "CJS side-effect import should always be kept: {js}"
    );
}

// ---------------------------------------------------------------
// verbatimModuleSyntax: preserve all imports
// ---------------------------------------------------------------

#[test]
fn test_verbatim_module_syntax_preserves_imports_esm() {
    let opts = CompilerOptions {
        module: Some(ModuleKind::ESNext),
        verbatim_module_syntax: Some(true),
        ..Default::default()
    };
    let js = emit_ts_with(
        "import { Foo } from './mod';\nconst x: Foo = 1 as any;",
        opts,
    );
    assert!(
        js.contains("Foo"),
        "verbatimModuleSyntax should preserve type-only imports: {js}"
    );
}

// ---------------------------------------------------------------
// importsNotUsedAsValues: "preserve" (removed in TS 5.x)
// ---------------------------------------------------------------

#[test]
fn test_imports_not_used_as_values_preserve() {
    let opts = CompilerOptions {
        module: Some(ModuleKind::ESNext),
        imports_not_used_as_values: Some(ImportsNotUsedAsValues::Preserve),
        ..Default::default()
    };
    let js = emit_ts_with(
        "import { Foo } from './mod';\nconst x: Foo = 1 as any;",
        opts,
    );
    assert!(
        !js.contains("import"),
        "importsNotUsedAsValues: preserve should still elide type-only imports in TS 5.x: {js}"
    );
}

// ---------------------------------------------------------------
// Export { X } re-export counts as value usage
// ---------------------------------------------------------------

#[test]
fn test_reexport_prevents_elision_esm() {
    let js = emit_ts_esm("import { Foo } from './mod';\nexport { Foo };");
    assert!(
        js.contains("Foo"),
        "re-exported import should not be elided: {js}"
    );
}

// ---------------------------------------------------------------
// Import used in class extends (value position)
// ---------------------------------------------------------------

#[test]
fn test_import_in_class_extends_kept() {
    let js = emit_ts_esm("import { Base } from './mod';\nclass Child extends Base {}");
    assert!(
        js.contains("Base"),
        "import used in class extends should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Import used in typeof (value position)
// ---------------------------------------------------------------

#[test]
fn test_import_used_in_typeof_kept() {
    let js = emit_ts_esm("import { foo } from './mod';\nconst t = typeof foo;");
    assert!(
        js.contains("foo"),
        "import used in typeof expression should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Import used only in as/satisfies expression
// ---------------------------------------------------------------

#[test]
fn test_import_in_as_expression_value_part_kept() {
    let js = emit_ts_esm("import { Foo, bar } from './mod';\nconst x = bar as Foo;");
    assert!(
        js.contains("bar"),
        "import used as value in 'as' expression should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Import used in function call
// ---------------------------------------------------------------

#[test]
fn test_import_in_function_call_kept() {
    let js = emit_ts_esm("import { doStuff } from './mod';\ndoStuff();");
    assert!(
        js.contains("doStuff"),
        "import used in function call should be kept: {js}"
    );
}

// ---------------------------------------------------------------
// Multiple imports, some elided some not
// ---------------------------------------------------------------

#[test]
fn test_multiple_imports_mixed_elision() {
    let js = emit_ts_esm(
        "import { TypeA } from './types';\nimport { valueB } from './values';\nconst x: TypeA = valueB();",
    );
    assert!(!js.contains("TypeA"), "TypeA import should be elided: {js}");
    assert!(js.contains("valueB"), "valueB import should be kept: {js}");
}

#[test]
fn test_const_enum_member_only_import_elided_by_default() {
    let file = tsc_rs_parser::parse("index.ts", "import { Enum } from './merge';\nEnum.One;\n");
    let opts = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };
    let mut ext = HashMap::new();
    ext.insert(
        ("Enum".to_string(), "One".to_string()),
        ConstEnumValue::Number(1.0),
    );
    let js = emit_with_const_enum_values(&file, &opts, &ext).javascript;
    assert!(
        !js.contains("require(\"./merge\")"),
        "member-only const enum import should be elided by default: {js}"
    );
    assert!(
        js.contains("1 /* Enum.One */"),
        "const enum member should be inlined: {js}"
    );
}

#[test]
fn test_const_enum_member_only_import_elided_with_preserve_const_enums() {
    let file = tsc_rs_parser::parse("index.ts", "import { Enum } from './merge';\nEnum.One;\n");
    let opts = CompilerOptions {
        module: Some(ModuleKind::AMD),
        preserve_const_enums: Some(true),
        ..Default::default()
    };
    let mut ext = HashMap::new();
    ext.insert(
        ("Enum".to_string(), "One".to_string()),
        ConstEnumValue::Number(1.0),
    );
    let js = emit_with_const_enum_values(&file, &opts, &ext).javascript;
    assert!(
        !js.contains("\"./merge\""),
        "member-only const enum import should still be elided with preserveConstEnums: {js}"
    );
    assert!(
        js.contains("1 /* Enum.One */"),
        "const enum member should still be inlined: {js}"
    );
}
