//! Comprehensive tests for the TypeScript-to-JavaScript emitter.
//!
//! These tests parse TypeScript source with the parser, then emit JavaScript
//! and verify the output matches expected patterns.

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use tsc_rs_ast::*;
use tsc_rs_emitter::{
    collect_type_only_export_names, collect_value_export_names, emit, emit_with_const_enum_values,
    emit_with_cross_file_consts, emit_with_global_type_only, ConstEnumValue,
};

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

fn emit_ts_with_const_enum_values(
    source: &str,
    opts: CompilerOptions,
    values: &HashMap<(String, String), ConstEnumValue>,
) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let out = emit_with_const_enum_values(&file, &opts, values);
    out.javascript
}

/// Helper: parse and emit with specific file name + options.
fn emit_ts_file_with(file_name: &str, source: &str, opts: CompilerOptions) -> String {
    let file = tsc_rs_parser::parse(file_name, source);
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

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DecodedSourceMapMapping {
    generated_line: u32,
    generated_column: u32,
    original_line: u32,
    original_column: u32,
}

fn decode_source_map_mappings(source_map: &str) -> Vec<DecodedSourceMapMapping> {
    fn base64_value(byte: u8) -> i64 {
        match byte {
            b'A'..=b'Z' => i64::from(byte - b'A'),
            b'a'..=b'z' => i64::from(byte - b'a' + 26),
            b'0'..=b'9' => i64::from(byte - b'0' + 52),
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 VLQ byte: {byte}"),
        }
    }

    fn decode_segment(segment: &str) -> Vec<i64> {
        let mut values = Vec::new();
        let mut value = 0i64;
        let mut shift = 0u32;
        for byte in segment.bytes() {
            let digit = base64_value(byte);
            value |= (digit & 31) << shift;
            if digit & 32 != 0 {
                shift += 5;
                continue;
            }
            let signed = if value & 1 != 0 {
                -(value >> 1)
            } else {
                value >> 1
            };
            values.push(signed);
            value = 0;
            shift = 0;
        }
        assert_eq!(shift, 0, "unterminated VLQ segment: {segment}");
        values
    }

    let encoded = source_map
        .split_once("\"mappings\":\"")
        .and_then(|(_, tail)| tail.split_once('"'))
        .map(|(mappings, _)| mappings)
        .expect("source map mappings field");
    let mut decoded = Vec::new();
    let mut previous_source = 0i64;
    let mut previous_original_line = 0i64;
    let mut previous_original_column = 0i64;
    for (generated_line, line) in encoded.split(';').enumerate() {
        let mut previous_generated_column = 0i64;
        for segment in line.split(',').filter(|segment| !segment.is_empty()) {
            let values = decode_segment(segment);
            previous_generated_column += values[0];
            if values.len() < 4 {
                continue;
            }
            previous_source += values[1];
            previous_original_line += values[2];
            previous_original_column += values[3];
            assert!(previous_source >= 0, "negative source index");
            decoded.push(DecodedSourceMapMapping {
                generated_line: generated_line as u32,
                generated_column: previous_generated_column as u32,
                original_line: previous_original_line as u32,
                original_column: previous_original_column as u32,
            });
        }
    }
    decoded
}

fn byte_line_column(text: &str, offset: usize) -> (u32, u32) {
    let before = &text[..offset];
    let line = before.bytes().filter(|&byte| byte == b'\n').count() as u32;
    let column = before
        .as_bytes()
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(before.len(), |newline| before.len() - newline - 1) as u32;
    (line, column)
}

fn assert_source_map_columns_within_output(javascript: &str, mappings: &[DecodedSourceMapMapping]) {
    let lines: Vec<&str> = javascript.split('\n').collect();
    for mapping in mappings {
        let line = lines
            .get(mapping.generated_line as usize)
            .unwrap_or_else(|| panic!("mapping beyond generated output: {mapping:?}"));
        assert!(
            mapping.generated_column <= line.len() as u32,
            "mapping beyond generated line end ({}): {mapping:?}",
            line.len()
        );
    }
}

fn decoded_source_map_positions(source_map: &str) -> Vec<(u32, u32, u32, u32)> {
    let mappings = source_map
        .split_once("\"mappings\":\"")
        .and_then(|(_, tail)| tail.split_once('"'))
        .map(|(mappings, _)| mappings)
        .expect("source map mappings field");
    let mut state = [0i64; 5];
    let mut positions = Vec::new();
    for (generated_line, line) in mappings.split(';').enumerate() {
        state[0] = 0;
        for segment in line.split(',').filter(|segment| !segment.is_empty()) {
            let mut values = Vec::new();
            let mut value = 0u64;
            let mut shift = 0u32;
            for byte in segment.bytes() {
                let digit = match byte {
                    b'A'..=b'Z' => byte - b'A',
                    b'a'..=b'z' => byte - b'a' + 26,
                    b'0'..=b'9' => byte - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    _ => panic!("invalid VLQ byte {byte}"),
                };
                value |= u64::from(digit & 31) << shift;
                if digit & 32 != 0 {
                    shift += 5;
                    continue;
                }
                let decoded = if value & 1 != 0 {
                    -((value >> 1) as i64)
                } else {
                    (value >> 1) as i64
                };
                values.push(decoded);
                value = 0;
                shift = 0;
            }
            for (idx, delta) in values.into_iter().enumerate() {
                state[idx] += delta;
            }
            if segment.len() >= 4 {
                positions.push((
                    generated_line as u32,
                    state[0] as u32,
                    state[2] as u32,
                    state[3] as u32,
                ));
            }
        }
    }
    positions
}

fn run_node_from_stdin(args: &[&str], javascript: &str) -> Output {
    let mut child = Command::new("node")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node must be available for emitted-JavaScript controls");
    child
        .stdin
        .take()
        .expect("node stdin")
        .write_all(javascript.as_bytes())
        .expect("write emitted JavaScript to node");
    child.wait_with_output().expect("wait for node")
}

fn assert_node_syntax(javascript: &str) {
    let output = run_node_from_stdin(&["--check", "-"], javascript);
    assert!(
        output.status.success(),
        "node rejected emitted JavaScript:\n{}\n{javascript}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn execute_with_node(javascript: &str) -> String {
    let output = run_node_from_stdin(&["-"], javascript);
    assert!(
        output.status.success(),
        "node failed to execute emitted JavaScript:\n{}\n{javascript}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("node stdout is UTF-8")
        .trim()
        .to_string()
}

fn execute_module_with_node(javascript: &str) -> String {
    let output = run_node_from_stdin(&["--input-type=module", "-"], javascript);
    assert!(
        output.status.success(),
        "node failed to execute emitted JavaScript module:\n{}\n{javascript}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("node stdout is UTF-8")
        .trim()
        .to_string()
}

#[test]
fn es5_exported_empty_binding_patterns_match_typescript_module_shapes() {
    let cases = [
        (
            ModuleKind::CommonJS,
            "\"use strict\";\nvar _a;\nObject.defineProperty(exports, \"__esModule\", { value: true });\nexports._b = _a = VALUE;\n",
        ),
        (
            ModuleKind::AMD,
            "define([\"require\", \"exports\"], function (require, exports) {\n    \"use strict\";\n    var _a;\n    Object.defineProperty(exports, \"__esModule\", { value: true });\n    exports._b = _a = VALUE;\n});\n",
        ),
        (
            ModuleKind::System,
            "System.register([], function (exports_1, context_1) {\n    \"use strict\";\n    var _a, _b;\n    var __moduleName = context_1 && context_1.id;\n    return {\n        setters: [],\n        execute: function () {\n            exports_1(\"_b\", _b = _a = VALUE);\n        }\n    };\n});\n",
        ),
        (ModuleKind::ES2015, "var _a;\nexport var _b = _a = VALUE;\n"),
        (ModuleKind::ESNext, "var _a;\nexport var _b = _a = VALUE;\n"),
    ];

    for (source, value) in [
        ("export const [] = [];", "[]"),
        ("export const {} = {};", "{}"),
    ] {
        for (module, expected) in &cases {
            let javascript = emit_ts_with(
                source,
                CompilerOptions {
                    target: Some(ScriptTarget::ES5),
                    module: Some(*module),
                    ..Default::default()
                },
            );
            assert_eq!(
                javascript,
                expected.replace("VALUE", value),
                "source={source:?}, module={module:?}"
            );
        }
    }
}

#[test]
fn es5_exported_empty_binding_pattern_evaluates_rhs_once() {
    let javascript = emit_ts_with(
        "let calls = 0; function make() { calls++; return { value: 7 }; } export const {} = make(); console.log(calls, exports[Object.keys(exports)[0]].value);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert_eq!(execute_with_node(&javascript), "1 7");
}

#[test]
fn es5_exported_empty_bindings_defer_names_after_all_ordinary_temps() {
    let source =
        "export const {} = {}; export const [] = []; const k = \"x\"; const obj = { [k]: 1 };";
    let cases = [
        (
            ModuleKind::CommonJS,
            concat!(
                "\"use strict\";\n",
                "var _a, _b, _c;\n",
                "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "exports._d = _a = {};\n",
                "exports._e = _b = [];\n",
                "var k = \"x\";\n",
                "var obj = (_c = {}, _c[k] = 1, _c);\n",
            ),
        ),
        (
            ModuleKind::AMD,
            concat!(
                "define([\"require\", \"exports\"], function (require, exports) {\n",
                "    \"use strict\";\n",
                "    var _a, _b, _c;\n",
                "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "    exports._d = _a = {};\n",
                "    exports._e = _b = [];\n",
                "    var k = \"x\";\n",
                "    var obj = (_c = {}, _c[k] = 1, _c);\n",
                "});\n",
            ),
        ),
        (
            ModuleKind::System,
            concat!(
                "System.register([], function (exports_1, context_1) {\n",
                "    \"use strict\";\n",
                "    var _a, _b, _c, _d, _e, k, obj;\n",
                "    var __moduleName = context_1 && context_1.id;\n",
                "    return {\n",
                "        setters: [],\n",
                "        execute: function () {\n",
                "            exports_1(\"_d\", _d = _a = {});\n",
                "            exports_1(\"_e\", _e = _b = []);\n",
                "            k = \"x\";\n",
                "            obj = (_c = {}, _c[k] = 1, _c);\n",
                "        }\n",
                "    };\n",
                "});\n",
            ),
        ),
        (
            ModuleKind::ES2015,
            concat!(
                "var _a, _b, _c;\n",
                "export var _d = _a = {};\n",
                "export var _e = _b = [];\n",
                "var k = \"x\";\n",
                "var obj = (_c = {}, _c[k] = 1, _c);\n",
            ),
        ),
        (
            ModuleKind::ESNext,
            concat!(
                "var _a, _b, _c;\n",
                "export var _d = _a = {};\n",
                "export var _e = _b = [];\n",
                "var k = \"x\";\n",
                "var obj = (_c = {}, _c[k] = 1, _c);\n",
            ),
        ),
    ];
    for (module, expected) in cases {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                ..Default::default()
            },
        );
        assert_eq!(javascript, expected, "module={module:?}");
    }

    let runtime = emit_ts_with(
        "let calls = 0; function rhs() { calls++; return {}; } export const {} = rhs(); export const [] = (calls++, []); console.log(calls);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert_eq!(execute_with_node(&runtime), "2");
}

#[test]
fn es5_exported_empty_binding_placeholder_cannot_capture_source_text() {
    let marker = "__tsrs_deferred_export_name_0__";
    let javascript = emit_ts_with(
        &format!("const marker = \"{marker}\"; export const {{}} = {{}}; console.log(marker);"),
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(javascript.contains(&format!("var marker = \"{marker}\";")));
    assert_eq!(execute_with_node(&javascript), marker);
}

#[test]
fn es5_exported_empty_binding_sentinel_cannot_capture_escaped_markers() {
    let source = concat!(
        r#"const a = "\x5f_tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"const b = "\u005f_tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"const c = "\u{5f}_tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"const d = "\x5f\u005ftsrs_deferred_export_name_0__";"#,
        "\n",
        r#"const e = "\\u005f_tsrs_deferred_export_name_0__";"#,
        "\n",
        "export const {} = {};\n",
        r#"const f = `\u005f_tsrs_deferred_export_name_0__`;"#,
        "\n",
        r#"const g = "\0__tsrs_deferred_export_name_0__\0";"#,
        "\n",
        "console.log(a, b, c, d, e, f, g);",
    );
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        r#"var a = "\x5f_tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"var b = "\u005f_tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"var c = "__tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"var d = "\x5f\u005ftsrs_deferred_export_name_0__";"#,
        "\n",
        r#"var e = "\\u005f_tsrs_deferred_export_name_0__";"#,
        "\n",
        "exports._b = _a = {};\n",
        r#"var f = "__tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"var g = "\0__tsrs_deferred_export_name_0__\0";"#,
        "\n",
        "console.log(a, b, c, d, e, f, g);\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, a, b, c, d, e, _b, f, g;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        r#"            a = "\x5f_tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"            b = "\u005f_tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"            c = "__tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"            d = "\x5f\u005ftsrs_deferred_export_name_0__";"#,
        "\n",
        r#"            e = "\\u005f_tsrs_deferred_export_name_0__";"#,
        "\n",
        "            exports_1(\"_b\", _b = _a = {});\n",
        r#"            f = "__tsrs_deferred_export_name_0__";"#,
        "\n",
        r#"            g = "\0__tsrs_deferred_export_name_0__\0";"#,
        "\n",
        "            console.log(a, b, c, d, e, f, g);\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let runtime_expected = concat!(
        "__tsrs_deferred_export_name_0__ ",
        "__tsrs_deferred_export_name_0__ ",
        "__tsrs_deferred_export_name_0__ ",
        "__tsrs_deferred_export_name_0__ ",
        r#"\u005f_tsrs_deferred_export_name_0__ "#,
        "__tsrs_deferred_export_name_0__ ",
        "\0__tsrs_deferred_export_name_0__\0",
    );

    for (module, expected) in [
        (ModuleKind::CommonJS, commonjs_expected),
        (ModuleKind::System, system_expected),
    ] {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                ..Default::default()
            },
        );
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            javascript
        };
        assert_eq!(execute_with_node(&executable), runtime_expected);
    }

    for (module, rhs_anchor, template_anchor) in [
        (ModuleKind::CommonJS, (8, 18, 5, 18), (9, 9, 6, 10)),
        (ModuleKind::System, (12, 38, 5, 18), (13, 16, 6, 10)),
    ] {
        let file = tsc_rs_parser::parse("probe.ts", source);
        let emitted = emit(
            &file,
            &CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                source_map: Some(true),
                ..Default::default()
            },
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        assert!(mappings.contains(&rhs_anchor), "{module:?}: {mappings:?}");
        assert!(
            mappings.contains(&template_anchor),
            "{module:?}: {mappings:?}"
        );
    }
}

#[test]
fn es5_exported_empty_binding_rejects_canonical_cooked_nul_sentinel() {
    let source = concat!(
        "function rhs() { return {}; }\n",
        "const g = \"\0__tsrs_deferred_export_name_\\x30__\0\\u{61}\";\n",
        "export const {} = rhs();\n",
        "console.log(JSON.stringify(g));",
    );
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "function rhs() { return {}; }\n",
        "var g = \"\0__tsrs_deferred_export_name_0__\0a\";\n",
        "exports._b = _a = rhs();\n",
        "console.log(JSON.stringify(g));\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, g, _b;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    function rhs() { return {}; }\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            g = \"\0__tsrs_deferred_export_name_0__\0a\";\n",
        "            exports_1(\"_b\", _b = _a = rhs());\n",
        "            console.log(JSON.stringify(g));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let runtime_expected = r#""\u0000__tsrs_deferred_export_name_0__\u0000a""#;

    for (module, expected, rhs_anchor) in [
        (ModuleKind::CommonJS, commonjs_expected, (5, 18, 2, 18)),
        (ModuleKind::System, system_expected, (9, 38, 2, 18)),
    ] {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                ..Default::default()
            },
        );
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            javascript
        };
        assert_eq!(execute_with_node(&executable), runtime_expected);

        let file = tsc_rs_parser::parse("probe.ts", source);
        let emitted = emit(
            &file,
            &CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                source_map: Some(true),
                ..Default::default()
            },
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        assert!(mappings.contains(&rhs_anchor), "{module:?}: {mappings:?}");
    }
}

#[test]
fn es5_exported_empty_binding_rejects_jsx_entity_cooked_nul_sentinel() {
    let source = concat!(
        "const el = <div x='&#0;__tsrs_deferred_export_name_0__&#0;' />;\n",
        "export const {} = {};\n",
        "console.log(JSON.stringify(el.props.x));",
    );
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "var el = React.createElement(\"div\", { x: '\0__tsrs_deferred_export_name_0__\0' });\n",
        "exports._b = _a = {};\n",
        "console.log(JSON.stringify(el.props.x));\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, el, _b;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            el = React.createElement(\"div\", { x: '\0__tsrs_deferred_export_name_0__\0' });\n",
        "            exports_1(\"_b\", _b = _a = {});\n",
        "            console.log(JSON.stringify(el.props.x));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let runtime_expected = r#""\u0000__tsrs_deferred_export_name_0__\u0000""#;

    for (module, expected, rhs_anchor) in [
        (ModuleKind::CommonJS, commonjs_expected, (4, 18, 1, 18)),
        (ModuleKind::System, system_expected, (8, 38, 1, 18)),
    ] {
        let javascript = emit_ts_file_with(
            "probe.tsx",
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                jsx: Some(JsxEmit::React),
                ..Default::default()
            },
        );
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var React = {{ createElement: function (_, props) {{ return {{ props: props }}; }} }};\nvar System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            format!(
                "var React = {{ createElement: function (_, props) {{ return {{ props: props }}; }} }};\n{javascript}"
            )
        };
        assert_eq!(execute_with_node(&executable), runtime_expected);

        let file = tsc_rs_parser::parse("probe.tsx", source);
        let emitted = emit(
            &file,
            &CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                jsx: Some(JsxEmit::React),
                source_map: Some(true),
                ..Default::default()
            },
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        assert!(mappings.contains(&rhs_anchor), "{module:?}: {mappings:?}");
    }
}

#[test]
fn es5_exported_empty_binding_rejects_external_const_enum_nul_sentinel() {
    let source = concat!(
        "import { Enum } from './enum';\n",
        "export const {} = {};\n",
        "console.log(JSON.stringify(Enum.Value));",
    );
    let values = HashMap::from([(
        ("Enum".to_string(), "Value".to_string()),
        ConstEnumValue::String("\0__tsrs_deferred_export_name_0__\0".to_string()),
    )]);
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "exports._b = _a = {};\n",
        "console.log(JSON.stringify(\"\0__tsrs_deferred_export_name_0__\0\" /* Enum.Value */));\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, _b;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            exports_1(\"_b\", _b = _a = {});\n",
        "            console.log(JSON.stringify(\"\0__tsrs_deferred_export_name_0__\0\" /* Enum.Value */));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let runtime_expected = r#""\u0000__tsrs_deferred_export_name_0__\u0000""#;

    for (module, expected, rhs_anchor) in [
        (ModuleKind::CommonJS, commonjs_expected, (3, 18, 1, 18)),
        (ModuleKind::System, system_expected, (7, 38, 1, 18)),
    ] {
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(module),
            ..Default::default()
        };
        let javascript = emit_ts_with_const_enum_values(source, options.clone(), &values);
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            javascript
        };
        assert_eq!(execute_with_node(&executable), runtime_expected);

        let file = tsc_rs_parser::parse("probe.ts", source);
        let emitted = emit_with_const_enum_values(
            &file,
            &CompilerOptions {
                source_map: Some(true),
                ..options
            },
            &values,
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        assert!(mappings.contains(&rhs_anchor), "{module:?}: {mappings:?}");
    }
}

#[test]
fn es5_exported_empty_binding_rejects_external_file_string_const_nul_sentinel() {
    let source = concat!(
        "enum E { Value = Imported }\n",
        "export const {} = {};\n",
        "console.log(JSON.stringify(E.Value));",
    );
    let external_strings = HashMap::from([(
        "Imported".to_string(),
        "\0__tsrs_deferred_export_name_0__\0".to_string(),
    )]);
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "var E;\n",
        "(function (E) {\n",
        "    E[\"Value\"] = \"\0__tsrs_deferred_export_name_0__\0\";\n",
        "})(E || (E = {}));\n",
        "exports._b = _a = {};\n",
        "console.log(JSON.stringify(E.Value));\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, E, _b;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            (function (E) {\n",
        "                E[\"Value\"] = \"\0__tsrs_deferred_export_name_0__\0\";\n",
        "            })(E || (E = {}));\n",
        "            exports_1(\"_b\", _b = _a = {});\n",
        "            console.log(JSON.stringify(E.Value));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let runtime_expected = r#""\u0000__tsrs_deferred_export_name_0__\u0000""#;

    for (module, expected, rhs_anchor) in [
        (ModuleKind::CommonJS, commonjs_expected, (7, 18, 1, 18)),
        (ModuleKind::System, system_expected, (10, 38, 1, 18)),
    ] {
        let file = tsc_rs_parser::parse("probe.ts", source);
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(module),
            ..Default::default()
        };
        let javascript = emit_with_cross_file_consts(
            &file,
            &options,
            &HashMap::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &external_strings,
            &HashMap::new(),
            &HashMap::new(),
        )
        .javascript;
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            javascript
        };
        assert_eq!(execute_with_node(&executable), runtime_expected);

        let emitted = emit_with_cross_file_consts(
            &file,
            &CompilerOptions {
                source_map: Some(true),
                ..options
            },
            &HashMap::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &external_strings,
            &HashMap::new(),
            &HashMap::new(),
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        assert!(mappings.contains(&rhs_anchor), "{module:?}: {mappings:?}");
    }
}

#[test]
fn es5_exported_empty_binding_rejects_split_local_string_enum_fold_sentinel() {
    let source = concat!(
        "enum E { V = \"\0__tsrs_deferred_export_\" + \"name_0__\0\" }\n",
        "export const {} = {};\n",
        "console.log(JSON.stringify(E.V));",
    );
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "var E;\n",
        "(function (E) {\n",
        "    E[\"V\"] = \"\0__tsrs_deferred_export_name_0__\0\";\n",
        "})(E || (E = {}));\n",
        "exports._b = _a = {};\n",
        "console.log(JSON.stringify(E.V));\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, E, _b;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            (function (E) {\n",
        "                E[\"V\"] = \"\0__tsrs_deferred_export_name_0__\0\";\n",
        "            })(E || (E = {}));\n",
        "            exports_1(\"_b\", _b = _a = {});\n",
        "            console.log(JSON.stringify(E.V));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let runtime_expected = r#""\u0000__tsrs_deferred_export_name_0__\u0000""#;

    for (module, expected, rhs_anchor) in [
        (ModuleKind::CommonJS, commonjs_expected, (7, 18, 1, 18)),
        (ModuleKind::System, system_expected, (10, 38, 1, 18)),
    ] {
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(module),
            ..Default::default()
        };
        let file = tsc_rs_parser::parse("probe.ts", source);
        let javascript = emit(&file, &options).javascript;
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            javascript
        };
        assert_eq!(execute_with_node(&executable), runtime_expected);

        let emitted = emit(
            &file,
            &CompilerOptions {
                source_map: Some(true),
                ..options
            },
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        assert!(mappings.contains(&rhs_anchor), "{module:?}: {mappings:?}");
    }
}

#[test]
fn es5_exported_empty_binding_preserves_later_split_string_enum_fold_sentinel() {
    let source = concat!(
        "export const {} = {};\n",
        "enum E { V = \"\0__tsrs_deferred_export_\" + \"name_0__\0\" }\n",
        "console.log(JSON.stringify(E.V));",
    );
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "exports._b = _a = {};\n",
        "var E;\n",
        "(function (E) {\n",
        "    E[\"V\"] = \"\0__tsrs_deferred_export_name_0__\0\";\n",
        "})(E || (E = {}));\n",
        "console.log(JSON.stringify(E.V));\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, _b, E;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            exports_1(\"_b\", _b = _a = {});\n",
        "            (function (E) {\n",
        "                E[\"V\"] = \"\0__tsrs_deferred_export_name_0__\0\";\n",
        "            })(E || (E = {}));\n",
        "            console.log(JSON.stringify(E.V));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let runtime_expected = r#""\u0000__tsrs_deferred_export_name_0__\u0000""#;

    for (module, expected, rhs_anchor) in [
        (ModuleKind::CommonJS, commonjs_expected, (3, 18, 0, 18)),
        (ModuleKind::System, system_expected, (7, 38, 0, 18)),
    ] {
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(module),
            ..Default::default()
        };
        let file = tsc_rs_parser::parse("probe.ts", source);
        let javascript = emit(&file, &options).javascript;
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            javascript
        };
        assert_eq!(execute_with_node(&executable), runtime_expected);

        let emitted = emit(
            &file,
            &CompilerOptions {
                source_map: Some(true),
                ..options
            },
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        assert!(mappings.contains(&rhs_anchor), "{module:?}: {mappings:?}");
    }
}

#[test]
fn es5_exported_empty_binding_rejects_external_jsx_import_source_sentinel() {
    let source = concat!(
        "export const {} = {};\n",
        "const el = <div id=\"ok\" />;\n",
        "console.log(el.type, el.props.id);",
    );
    let import_source = "\0__tsrs_deferred_export_name_0__\0";
    let expected = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "var jsx_runtime_1 = require(\"\0__tsrs_deferred_export_name_0__\0/jsx-runtime\");\n",
        "exports._b = _a = {};\n",
        "var el = (0, jsx_runtime_1.jsx)(\"div\", { id: \"ok\" });\n",
        "console.log(el.type, el.props.id);\n",
    );
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::CommonJS),
        jsx: Some(JsxEmit::ReactJSX),
        jsx_import_source: Some(import_source.to_string()),
        ..Default::default()
    };
    let file = tsc_rs_parser::parse("probe.tsx", source);
    let javascript = emit(&file, &options).javascript;
    assert_eq!(javascript, expected);
    let executable = format!(
        "function require(path) {{ console.log(JSON.stringify(path)); return {{ jsx: function (type, props) {{ return {{ type: type, props: props }}; }} }}; }}\n{javascript}"
    );
    assert_eq!(
        execute_with_node(&executable),
        concat!(
            r#""\u0000__tsrs_deferred_export_name_0__\u0000/jsx-runtime""#,
            "\ndiv ok"
        )
    );

    let emitted = emit(
        &file,
        &CompilerOptions {
            source_map: Some(true),
            ..options
        },
    );
    let mappings =
        decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
    assert!(mappings.contains(&(4, 18, 0, 18)), "{mappings:?}");
}

#[test]
fn es5_exported_empty_binding_sentinel_survives_later_helper_rewrites() {
    let marker = "__tsrs_deferred_export_name_0__";
    let source =
        "function dec(...args: any[]) {} export const {} = {}; const key = \"m\"; class C { @dec [key]() {} } const marker = `\\u005f_tsrs_deferred_export_name_0__`; console.log(marker);";
    let commonjs_expected = concat!(
        "\"use strict\";\n",
        "var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {\n",
        "    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;\n",
        "    if (typeof Reflect === \"object\" && typeof Reflect.decorate === \"function\") r = Reflect.decorate(decorators, target, key, desc);\n",
        "    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;\n",
        "    return c > 3 && r && Object.defineProperty(target, key, r), r;\n",
        "};\n",
        "var _a;\n",
        "var _b;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "function dec() {\n",
        "    var args = [];\n",
        "    for (var _i = 0; _i < arguments.length; _i++) {\n",
        "        args[_i] = arguments[_i];\n",
        "    }\n",
        "}\n",
        "exports._c = _a = {};\n",
        "var key = \"m\";\n",
        "class C {\n",
        "    [(_b = key)]() { }\n",
        "}\n",
        "__decorate([\n",
        "    dec\n",
        "], C.prototype, _b, null);\n",
        "var marker = \"__tsrs_deferred_export_name_0__\";\n",
        "console.log(marker);\n",
    );
    let system_expected = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {\n",
        "        var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;\n",
        "        if (typeof Reflect === \"object\" && typeof Reflect.decorate === \"function\") r = Reflect.decorate(decorators, target, key, desc);\n",
        "        else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;\n",
        "        return c > 3 && r && Object.defineProperty(target, key, r), r;\n",
        "    };\n",
        "    var _a, _b, _c, key, C, marker;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    function dec() {\n",
        "        var args = [];\n",
        "        for (var _i = 0; _i < arguments.length; _i++) {\n",
        "            args[_i] = arguments[_i];\n",
        "        }\n",
        "    }\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            exports_1(\"_c\", _c = _a = {});\n",
        "            key = \"m\";\n",
        "            C = class C {\n",
        "                [(_b = key)]() { }\n",
        "            };\n",
        "            marker = \"__tsrs_deferred_export_name_0__\";\n",
        "            console.log(marker);\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    for (module, expected) in [
        (ModuleKind::CommonJS, commonjs_expected),
        (ModuleKind::System, system_expected),
    ] {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                experimental_decorators: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(javascript, expected, "module={module:?}");
        let executable = if module == ModuleKind::System {
            format!(
                "var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            javascript
        };
        assert_eq!(execute_with_node(&executable), marker);
    }
}

#[test]
fn es5_exported_empty_binding_source_maps_follow_final_generated_positions() {
    let source = "function rhs(){ return {}; } export const {} = rhs(); console.log('after');";
    let cases = [
        (ModuleKind::CommonJS, (3, 0), (4, 18)),
        (ModuleKind::AMD, (4, 0), (5, 22)),
        (ModuleKind::ESNext, (1, 0), (2, 21)),
        (ModuleKind::System, (4, 4), (8, 38)),
    ];
    for (module, function_anchor, rhs_anchor) in cases {
        let file = tsc_rs_parser::parse("probe.ts", source);
        let emitted = emit(
            &file,
            &CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                source_map: Some(true),
                ..Default::default()
            },
        );
        let mappings =
            decoded_source_map_positions(emitted.source_map.as_deref().expect("source map output"));
        if module != ModuleKind::System {
            assert!(
                mappings.contains(&(function_anchor.0, function_anchor.1, 0, 0)),
                "missing function anchor for {module:?}: {mappings:?}\n{}",
                emitted.javascript
            );
        }
        assert!(
            mappings.contains(&(rhs_anchor.0, rhs_anchor.1, 0, 47)),
            "missing RHS anchor for {module:?}: {mappings:?}\n{}",
            emitted.javascript
        );
        for (line, column, _, _) in mappings {
            let generated_line = emitted
                .javascript
                .lines()
                .nth(line as usize)
                .expect("mapped generated line");
            assert!(
                column as usize <= generated_line.len(),
                "mapping beyond EOL for {module:?}: {line}:{column} > {}\n{}",
                generated_line.len(),
                emitted.javascript
            );
        }
    }
}

#[test]
fn es5_exported_empty_binding_pattern_uses_collision_safe_synthetic_names() {
    let javascript = emit_ts_with(
        "const _a = 1, _b = 2; export const {} = {};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert_eq!(
        javascript,
        "\"use strict\";\nvar _c;\nObject.defineProperty(exports, \"__esModule\", { value: true });\nvar _a = 1, _b = 2;\nexports._d = _c = {};\n"
    );
}

#[test]
fn es5_exported_object_rest_matches_all_five_module_oracles_and_source_maps() {
    let source = "export const { x, ...rest } = { x: 'x', y: 'y' };";
    let commonjs = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "exports.rest = exports.x = void 0;\n",
        "exports.x = (_a = { x: 'x', y: 'y' }, _a).x, exports.rest = __rest(_a, [\"x\"]);\n",
    );
    let amd = concat!(
        "define([\"require\", \"exports\"], function (require, exports) {\n",
        "    \"use strict\";\n",
        "    var _a;\n",
        "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "    exports.rest = exports.x = void 0;\n",
        "    exports.x = (_a = { x: 'x', y: 'y' }, _a).x, exports.rest = __rest(_a, [\"x\"]);\n",
        "});\n",
    );
    let system = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, x, rest;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            exports_1(\"x\", x = (_a = { x: 'x', y: 'y' }, _a).x), exports_1(\"rest\", rest = __rest(_a, [\"x\"]));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let esm = concat!(
        "var _a;\n",
        "export var x = (_a = { x: 'x', y: 'y' }, _a).x, rest = __rest(_a, [\"x\"]);\n",
    );

    for (module, expected) in [
        (ModuleKind::CommonJS, commonjs),
        (ModuleKind::AMD, amd),
        (ModuleKind::System, system),
        (ModuleKind::ES2015, esm),
        (ModuleKind::ESNext, esm),
    ] {
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(module),
            no_emit_helpers: Some(true),
            ..Default::default()
        };
        assert_eq!(
            emit_ts_with(source, options.clone()),
            expected,
            "{module:?}"
        );

        let file = tsc_rs_parser::parse("test.ts", source);
        let emitted = emit(
            &file,
            &CompilerOptions {
                source_map: Some(true),
                ..options
            },
        );
        let mappings =
            decode_source_map_mappings(emitted.source_map.as_deref().expect("source map output"));
        assert_source_map_columns_within_output(&emitted.javascript, &mappings);
        let generated_rhs = emitted
            .javascript
            .find("{ x: 'x', y: 'y' }")
            .expect("generated RHS");
        let original_rhs = source.rfind("{ x: 'x', y: 'y' }").expect("source RHS");
        let (generated_line, generated_column) =
            byte_line_column(&emitted.javascript, generated_rhs);
        let (original_line, original_column) = byte_line_column(source, original_rhs);
        assert!(
            mappings.iter().any(|mapping| {
                mapping.generated_line == generated_line
                    && mapping.generated_column == generated_column
                    && mapping.original_line == original_line
                    && mapping.original_column == original_column
            }),
            "missing final RHS mapping for {module:?}: {mappings:?}\n{}",
            emitted.javascript
        );
    }
}

#[test]
fn esnext_exported_object_rest_controls_remain_byte_identical() {
    let source = "export const { x, ...rest } = { x: 'x', y: 'y' };";
    let commonjs = concat!(
        "\"use strict\";\n",
        "var _a;\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "exports.rest = exports.x = void 0;\n",
        "_a = { x: 'x', y: 'y' }, exports.x = _a.x, exports.rest = __rest(_a, [\"x\"]);\n",
    );
    let amd = concat!(
        "define([\"require\", \"exports\"], function (require, exports) {\n",
        "    \"use strict\";\n",
        "    var _a;\n",
        "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "    exports.rest = exports.x = void 0;\n",
        "    _a = { x: 'x', y: 'y' }, exports.x = _a.x, exports.rest = __rest(_a, [\"x\"]);\n",
        "});\n",
    );
    let system = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var _a, x, rest;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            _a = { x: 'x', y: 'y' }, exports_1(\"x\", x = _a.x), exports_1(\"rest\", rest = __rest(_a, [\"x\"]));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    let esm = "export const { x, ...rest } = { x: 'x', y: 'y' };\n";

    for (module, expected) in [
        (ModuleKind::CommonJS, commonjs),
        (ModuleKind::AMD, amd),
        (ModuleKind::System, system),
        (ModuleKind::ES2015, esm),
        (ModuleKind::ESNext, esm),
    ] {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ESNext),
                module: Some(module),
                no_emit_helpers: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(javascript, expected, "{module:?}");
    }
}

#[test]
fn es5_exported_object_rest_evaluates_once_and_preserves_getter_order() {
    let source = concat!(
        "let calls = 0, log = [];\n",
        "function make() { calls++; return Object.defineProperties({ y: 'y' }, { x: { enumerable: true, get: function () { log.push('x'); return 'x'; } }, z: { enumerable: true, get: function () { log.push('z'); return 'z'; } } }); }\n",
        "export const { x, ...rest } = make();\n",
        "console.log(calls, x, rest.y, rest.z, log.join(','));",
    );
    let rest_helper = concat!(
        "var __rest = function (source, excluded) { var target = {}; for (var key in source) ",
        "if (Object.prototype.hasOwnProperty.call(source, key) && excluded.indexOf(key) < 0) ",
        "target[key] = source[key]; return target; };\n",
    );

    for module in [ModuleKind::CommonJS, ModuleKind::System] {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                no_emit_helpers: Some(true),
                ..Default::default()
            },
        );
        let executable = if module == ModuleKind::System {
            format!(
                "{rest_helper}var System = {{ register: function (_, factory) {{ var module = factory(function () {{}}, {{ id: 'test' }}); module.setters.forEach(function (setter) {{ setter({{}}); }}); module.execute(); }} }};\n{javascript}"
            )
        } else {
            format!("{rest_helper}{javascript}")
        };
        assert_eq!(execute_with_node(&executable), "1 x y z x,z", "{module:?}");
    }
}

#[test]
fn es5_exported_object_rest_temps_avoid_a_and_b_collisions() {
    let source = concat!(
        "const _a = 'a', _b = 'b';\n",
        "export const { x, ...rest } = { x: 'x', y: 'y' };",
    );
    for module in [ModuleKind::CommonJS, ModuleKind::System] {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                no_emit_helpers: Some(true),
                ..Default::default()
            },
        );
        assert!(
            javascript.contains("(_c = { x: 'x', y: 'y' }, _c).x"),
            "{module:?}:\n{javascript}"
        );
        assert!(
            !javascript.contains("(_a = { x:"),
            "{module:?}:\n{javascript}"
        );
        assert!(
            !javascript.contains("(_b = { x:"),
            "{module:?}:\n{javascript}"
        );
    }
}

#[test]
fn es5_exported_empty_and_object_rest_keep_ordinary_temp_order_in_both_source_orders() {
    for (source, empty_assignment, object_assignment) in [
        (
            "export const {} = {}; export const { x, ...rest } = { x: 'x', y: 'y' };",
            "_c = _a = {}",
            "(_b = { x: 'x', y: 'y' }, _b).x",
        ),
        (
            "export const { x, ...rest } = { x: 'x', y: 'y' }; export const {} = {};",
            "_c = _b = {}",
            "(_a = { x: 'x', y: 'y' }, _a).x",
        ),
    ] {
        for module in [ModuleKind::CommonJS, ModuleKind::System] {
            let javascript = emit_ts_with(
                source,
                CompilerOptions {
                    target: Some(ScriptTarget::ES5),
                    module: Some(module),
                    no_emit_helpers: Some(true),
                    ..Default::default()
                },
            );
            assert!(
                javascript.contains(empty_assignment),
                "{module:?}:\n{javascript}"
            );
            assert!(
                javascript.contains(object_assignment),
                "{module:?}:\n{javascript}"
            );
            assert!(
                !javascript.contains("__tsrs_deferred_export_name_"),
                "unresolved deferred export marker for {module:?}:\n{javascript}"
            );
            if module == ModuleKind::CommonJS {
                assert!(javascript.contains("var _a, _b;"), "{javascript}");
            } else if source.starts_with("export const {}") {
                assert!(
                    javascript.contains("var _a, _b, _c, x, rest;"),
                    "{javascript}"
                );
            } else {
                assert!(
                    javascript.contains("var _a, _b, x, rest, _c;"),
                    "{javascript}"
                );
            }
        }
    }
}

#[test]
fn es5_exported_object_rest_bridge_rejects_broader_or_unsafe_shapes() {
    let narrow_marker = "exports.x = (";
    let guarded_sources = [
        "export let { x, ...rest } = value;",
        "export const { x: value, ...rest } = source;",
        "export const { x = 1, ...rest } = source;",
        "export const { ['x']: x, ...rest } = source;",
        "export const { outer: { x }, ...rest } = source;",
        "export const { ...rest } = source;",
        "export const { x, ...rest } = source, other = 1;",
        "export const { x, /* keep */ ...rest } = source;",
        r"export const { \u0078, ...rest } = source;",
    ];
    for source in guarded_sources {
        let javascript = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(ModuleKind::CommonJS),
                no_emit_helpers: Some(true),
                ..Default::default()
            },
        );
        assert!(
            !javascript.contains(narrow_marker),
            "narrow bridge unexpectedly accepted {source:?}:\n{javascript}"
        );
    }

    for options in [
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            no_emit_helpers: Some(true),
            declaration: Some(true),
            ..Default::default()
        },
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            no_emit_helpers: Some(true),
            out_file: Some("bundle.js".into()),
            ..Default::default()
        },
    ] {
        let javascript = emit_ts_with("export const { x, ...rest } = { x: 'x', y: 'y' };", options);
        assert!(!javascript.contains(narrow_marker), "{javascript}");
    }

    let javascript_input = emit_ts_file_with(
        "test.js",
        "export const { x, ...rest } = { x: 'x', y: 'y' };",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        !javascript_input.contains(narrow_marker),
        "{javascript_input}"
    );
}

#[test]
fn exported_empty_binding_bridge_rejects_broader_patterns_and_unsafe_contexts() {
    let es5_commonjs = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };
    for source in [
        "const {} = {};",
        "export const { p: {} } = value;",
        "export const [,] = [];",
        "export const {} = {}, [] = [];",
        "export const { p = 1 } = value;",
        "export const { ...rest } = value;",
        "export const { /* keep */ } = {};",
        "export const {} = ;",
    ] {
        let javascript = emit_ts_with(source, es5_commonjs.clone());
        assert!(
            !javascript.contains("exports._b = _a ="),
            "narrow bridge unexpectedly accepted {source:?}:\n{javascript}"
        );
    }

    let esnext_target = emit_ts_with(
        "export const {} = {};",
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(!esnext_target.contains("exports._b = _a ="));

    let declaration_emit = emit_ts_with(
        "export const {} = {};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            declaration: Some(true),
            ..Default::default()
        },
    );
    assert!(!declaration_emit.contains("exports._b = _a ="));

    let bundled = emit_ts_with(
        "export const {} = {};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            out_file: Some("bundle.js".into()),
            ..Default::default()
        },
    );
    assert!(!bundled.contains("exports._b = _a ="));

    let javascript_input = emit_ts_file_with("test.js", "export const {} = {};", es5_commonjs);
    assert!(!javascript_input.contains("exports._b = _a ="));
}

#[test]
fn es5_lexical_capture_runtime_matches_typescript_controls() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let cases = [
        (
            "var fs=[]; for(let i=0;i<2;i++){ fs.push(function(){return i;}); } console.log(fs[0](),fs[1]());",
            "0 1",
        ),
        (
            "var fs=[]; for(let k in {a:1,b:2}){ fs.push(function(){return k;}); } console.log(fs[0](),fs[1]());",
            "a b",
        ),
        (
            "var fs=[]; for(let v of [1,2]){ fs.push(function(){return v;}); } console.log(fs[0](),fs[1]());",
            "1 2",
        ),
        (
            "var fs=[]; for(let i=0;i<4;i++){ fs.push(()=>i); i+=1; } console.log(fs.map(f=>f()).join(','));",
            "1,3",
        ),
        (
            "var fs=[]; for(let i=0,j=10;i<2;i++,j++) fs.push(()=>[i,j]); console.log(JSON.stringify(fs.map(f=>f())));",
            "[[0,10],[1,11]]",
        ),
        (
            "var fs=[]; for(let i=0,j=10;i<2;i++,j++){ let i=100; fs.push(()=>[i,j]); } console.log(JSON.stringify(fs.map(f=>f())));",
            "[[100,10],[100,11]]",
        ),
        (
            "var fs=[]; for(let i=0;i<2;i++){ let x=i*10; fs.push(()=>x); } console.log(fs.map(f=>f()).join(','));",
            "0,10",
        ),
        (
            "function f(){var fs=[];for(let i=0;i<5;i++){if(i===1)continue;if(i===4)break;fs.push(()=>i);}return fs.map(g=>g()).join(',')} console.log(f());",
            "0,2,3",
        ),
        (
            "var fs=[]; for(let i=0;i<4;i++){ fs.push(()=>i); if(i===0){i+=1;continue;} i+=1; } console.log(fs.map(f=>f()).join(','));",
            "1,3",
        ),
        (
            "var fs=[]; for(let i=0;i<4;i++){ fs.push(()=>i); i+=1; if(i===3)break; } console.log(fs.map(f=>f()).join(','));",
            "1,3",
        ),
        (
            "function f(){var fs=[];for(let i=0;i<3;i++){fs.push(()=>i);if(i===1)return fs.map(g=>g()).join(',')}return 'bad'} console.log(f());",
            "0,1",
        ),
        (
            "var fs=[]; outer: for(let i=0;i<3;i++){ for(let j=0;j<2;j++){ fs.push(()=>[i,j]); if(j===0) continue outer; } } console.log(JSON.stringify(fs.map(f=>f())));",
            "[[0,0],[1,0],[2,0]]",
        ),
        (
            "var fs=[]; label: for(let i=0;i<3;i++){fs.push(()=>i);if(i<2)continue label;}console.log(fs.map(f=>f()).join(','));",
            "0,1,2",
        ),
        (
            "var fs=[];block:{for(let i=0;i<2;i++){fs.push(()=>i);break block;}}console.log(fs.map(f=>f()).join(','));",
            "0",
        ),
        (
            "var fs=[];block:{for(let i=0;i<2;i++){for(let j=0;j<2;j++){fs.push(()=>[i,j]);break block;}}}console.log(JSON.stringify(fs.map(f=>f())));",
            "[[0,0]]",
        ),
        (
            "var fs=[];block:{for(let i=0;i<2;i++){for(let j=0;j<2;j++){fs.push(()=>[i,j]);i++;break block;}}}console.log(JSON.stringify(fs.map(f=>f())));",
            "[[1,0]]",
        ),
        (
            "var fs=[]; outer: for(let i=0;i<3;i++){ for(let j=0;j<2;j++){ fs.push(()=>[i,j]); if(i===1&&j===0) break outer; } } console.log(JSON.stringify(fs.map(f=>f())));",
            "[[0,0],[0,1],[1,0]]",
        ),
        (
            "function C(){ this.v=7; var fs=[]; for(let i=0;i<1;i++) fs.push(()=>[this.v,i]); return fs[0](); } console.log(C.call({}).join('|'));",
            "7|0",
        ),
        (
            "function C(){ var fs=[]; for(let i=0;i<1;i++) fs.push(function(x){return [this.v,arguments[0],i]}); return fs[0].call({v:8},9); } console.log(C().join('|'));",
            "8|9|0",
        ),
        (
            "var _loop_1=41,state_1=42,out_i_1=43,fs=[]; for(let i=0;i<2;i++){fs.push(()=>i);i++;if(i===1)break;} console.log(_loop_1,state_1,out_i_1,fs.map(f=>f()).join(','));",
            "41 42 43 1",
        ),
        (
            "var seen=[]; for(let i=0;i<4;i++){(()=>i++)();seen.push(()=>i)} console.log(seen.map(f=>f()).join(','));",
            "1,3",
        ),
        (
            "var seen=[]; for(let i=0;i<4;i++){(function(){i++}).call(null);seen.push(()=>i)} console.log(seen.map(f=>f()).join(','));",
            "1,3",
        ),
        (
            "var seen=[]; for(let i=0;i<4;i++){(()=>{i++}).apply(null,[]);seen.push(()=>i)} console.log(seen.map(f=>f()).join(','));",
            "1,3",
        ),
        (
            "var fs=[];for(let i=0;i<4;i++){let inc=()=>i++;fs.push(()=>i);inc();}console.log(fs.map(f=>f()).join(','));",
            "1,3",
        ),
        (
            "var fs=[],n=0;for(let i=0;i<4;i++){n++;let inc=()=>i++;let alias=inc,alias2=alias;fs.push(()=>i);alias2();}console.log(fs.map(f=>f()).join(',')+'|'+n);",
            "1,3|2",
        ),
        (
            "function callNow(f){f()}var fs=[],n=0;for(let i=0;i<4;i++){n++;fs.push(()=>i);callNow(()=>i++);}console.log(fs.map(f=>f()).join(',')+'|'+n);",
            "1,3|2",
        ),
        (
            "var \\u005floop_1=41,fs=[];for(let i=0;i<2;i++)fs.push(()=>i);console.log(_loop_1,fs.map(f=>f()).join(','));",
            "41 0,1",
        ),
        (
            "var fs=[]; for(let [i,j]=[0,10];i<2;i++,j++) fs.push(()=>[i,j]); console.log(JSON.stringify(fs.map(f=>f())));",
            "[[0,10],[1,11]]",
        ),
        (
            "var fs=[]; for(let [v] of [[1],[2]]) fs.push(()=>v); console.log(fs.map(f=>f()).join(','));",
            "1,2",
        ),
        (
            "var fs=[];for(let i=0;i<2;i++)fs.push((x=i,...rest)=>{return [i,x,rest.length].join(':')});console.log(fs.map(f=>f(undefined,1)).join('|'));",
            "0:0:1|1:1:1",
        ),
        (
            "var reads=0,fs=[];var source={get x(){reads++;return void 0},y:2,z:3};for(let {x=1,...rest}=source;x<2;x++)fs.push(()=>[x,rest.y,rest.z]);console.log(JSON.stringify(fs.map(f=>f()))+'|'+reads);",
            "[[1,2,3]]|1",
        ),
    ];
    for (source, expected) in cases {
        let javascript = emit_ts_with(source, options.clone());
        assert_eq!(execute_with_node(&javascript), expected, "{javascript}");
    }
}

#[test]
fn es5_classic_for_header_closures_preserve_native_iteration_environments() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let cases = [
        (
            "var fs=[];for(let i=0;i<3&&(fs.push(()=>i),true);i++){}console.log(fs.map(f=>f()).join(','));",
            "0,1,2",
        ),
        (
            "var fs=[];for(let i=0;i<2;i++,fs.push(()=>i)){}console.log(fs.map(f=>f()).join(','));",
            "1,2",
        ),
        (
            "var fs=[];for(let i=0,capture=fs.push(()=>i);i<2;i++){}console.log(fs.map(f=>f()).join(','));",
            "0",
        ),
    ];
    for (source, expected) in cases {
        let javascript = emit_ts_with(source, options.clone());
        assert!(javascript.contains("for (let "), "{javascript}");
        assert!(!javascript.contains("var _loop_"), "{javascript}");
        assert_eq!(execute_with_node(&javascript), expected, "{javascript}");
    }
}

#[test]
fn es5_type_only_imports_do_not_disable_captured_loop_semantics() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };
    for source in [
        "import type {T} from './dep';var fs=[];for(let i=0;i<2;i++)fs.push(()=>i);console.log(fs.map(f=>f()).join(','));",
        "import {type T} from './dep';var fs=[];for(let i=0;i<2;i++)fs.push(()=>i);console.log(fs.map(f=>f()).join(','));",
    ] {
        let javascript = emit_ts_with(source, options.clone());
        assert!(javascript.contains("var _loop_"), "{javascript}");
        assert_eq!(execute_with_node(&javascript), "0,1", "{javascript}");
    }
}

#[test]
fn es5_lexical_loop_helpers_capture_new_target_without_crossing_normal_functions() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let lexical = emit_ts_with(
        "function F(){var fs=[];for(let i=0;i<1;i++)fs.push(()=>[i,new.target===F]);return fs[0]();}console.log(JSON.stringify(new F()),JSON.stringify(F()));",
        options.clone(),
    );
    assert!(lexical.contains("_newTarget"), "{lexical}");
    assert_eq!(
        execute_with_node(&lexical),
        "[0,true] [0,false]",
        "{lexical}"
    );

    let nested = emit_ts_with(
        "function F(){var fs=[];for(let i=0;i<1;i++)fs.push(()=>[i,new.target===F,function G(){return [new.target===G]}]);var pair=fs[0]();return [pair[0],pair[1],pair[2](),new pair[2]()];}console.log(JSON.stringify(F()));",
        options,
    );
    assert_eq!(
        execute_with_node(&nested),
        "[0,false,[false],[true]]",
        "{nested}"
    );

    let nested_loops = emit_ts_with(
        "function F(){var fs=[];for(let i=0;i<1;i++){for(let j=0;j<1;j++)fs.push(()=>[i,j,new.target===F]);}return fs[0]();}console.log(JSON.stringify(new F()),JSON.stringify(F()));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert_eq!(
        execute_with_node(&nested_loops),
        "[0,0,true] [0,0,false]",
        "{nested_loops}"
    );
}

#[test]
fn es5_lexical_super_keeps_captured_loops_in_the_method_environment() {
    let javascript = emit_ts_with(
        r#"class A {
            m(){return "A"} get p(){return "P"}
            static m(){return "S"} static get p(){return "Q"}
        }
        class B extends A {
            run(){var fs=[];for(let i=0;i<1;i++)fs.push(()=>super["m"]()+super.p+i);return fs[0]()}
            static run(){var fs=[];for(let i=0;i<1;i++)fs.push(()=>super.m()+super.p+i);return fs[0]()}
        }
        console.log(new B().run(),B.run());"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(!javascript.contains("var _loop_"), "{javascript}");
    assert!(javascript.contains("for (let i = 0;"), "{javascript}");
    assert_eq!(execute_with_node(&javascript), "AP0 SQ0", "{javascript}");
}

#[test]
fn es5_captured_for_of_preserves_downlevel_iterator_close() {
    let source = "var log=[],fs=[]; var iterable={ [Symbol.iterator]:function(){var i=0;return {next:function(){log.push('next'+i);return {value:i,done:i++>2}},return:function(){log.push('close');return {done:true}}}}}; for(let v of iterable){fs.push(()=>v);if(v===1)break;} console.log(fs.map(f=>f()).join(',')+'|'+log.join(','));";
    let javascript = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert!(javascript.contains("__values"), "{javascript}");
    assert_eq!(
        execute_with_node(&javascript),
        "0,1|next0,next1,close",
        "{javascript}"
    );
}

#[test]
fn es5_captured_for_of_closes_iterator_when_helper_or_next_throws() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        down_level_iteration: Some(true),
        ..Default::default()
    };
    let helper_throw = emit_ts_with(
        "var log=[];var iterable={ [Symbol.iterator]:function(){return {next:function(){log.push('next');return {value:1,done:false}},return:function(){log.push('close');return {done:true}}}}};try{for(let v of iterable){(()=>v);throw Error('body')}}catch(e){log.push(e.message)}console.log(log.join(','));",
        options.clone(),
    );
    assert_eq!(
        execute_with_node(&helper_throw),
        "next,close,body",
        "{helper_throw}"
    );

    let next_throw = emit_ts_with(
        "var log=[],fs=[];var iterable={ [Symbol.iterator]:function(){var n=0;return {next:function(){log.push('next'+n);if(n++)throw Error('next-error');return {value:1,done:false}},return:function(){log.push('close');return {done:true}}}}};try{for(let v of iterable){fs.push(()=>v)}}catch(e){log.push(e.message)}console.log(log.join(','));",
        options,
    );
    assert_eq!(
        execute_with_node(&next_throw),
        "next0,next1,close,next-error",
        "{next_throw}"
    );
}

#[test]
fn es5_deferred_loop_mutation_uses_conservative_copy_out() {
    let javascript = emit_ts_with(
        "var fs=[];for(let i=0;i<2;i++)fs.push(()=>i++);console.log(fs.map(f=>f()).join(','));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(javascript.contains("out_i_"), "{javascript}");
    assert_eq!(execute_with_node(&javascript), "0,1", "{javascript}");
}

#[test]
fn es5_nested_destructuring_caches_rhs_properties_defaults_and_rests() {
    let javascript = emit_ts_with(
        "var calls=0,reads=0,fs=[];function get(){calls++;return {get a(){reads++;return [void 0,2,3]}}}for(let {a:[x=1,...rest]}=get();x<2;x++)fs.push(()=>[x,rest.join('-')]);console.log(JSON.stringify(fs.map(f=>f()))+'|'+calls+'|'+reads);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert_eq!(
        execute_with_node(&javascript),
        "[[1,\"2-3\"]]|1|1",
        "{javascript}"
    );
}

#[test]
fn es5_suspending_loops_never_extract_yield_or_await_into_plain_helpers() {
    let generator = emit_ts_with(
        "var fs=[]; function* g(){for(let i=0;i<2;i++){fs.push(()=>i);yield i;}} [...g()]; console.log(fs.map(f=>f()).join(','));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            // Consuming a generator with ES5 array spread requires iterable
            // lowering; the loop-safety assertions below remain independent.
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert!(!generator.contains("var _loop_"), "{generator}");
    assert_eq!(execute_with_node(&generator), "0,1", "{generator}");

    let asynchronous = emit_ts_with(
        "async function f(){var fs=[];for(let i=0;i<2;i++){await Promise.resolve();fs.push(()=>i)}return fs.map(g=>g()).join(',')} f().then(console.log);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(!asynchronous.contains("var _loop_"), "{asynchronous}");
    assert_eq!(execute_with_node(&asynchronous), "0,1", "{asynchronous}");
}

#[test]
fn es5_downlevel_iteration_array_patterns_preserve_iterable_semantics() {
    let javascript = emit_ts_with(
        "var fs=[];for(let [a,b] of [new Set([1,3])])fs.push(()=>a+':'+b);for(let [c] of [new Set([4])])fs.push(()=>c);console.log(fs.map(f=>f()).join('|'));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert!(javascript.contains("__values"), "{javascript}");
    assert!(javascript.contains("__read"), "{javascript}");
    assert!(!javascript.contains("let ["), "{javascript}");
    assert!(!javascript.contains(".value[0]"), "{javascript}");
    assert_eq!(execute_with_node(&javascript), "1:3|4", "{javascript}");
}

#[test]
fn es5_downlevel_iteration_array_patterns_respect_import_helpers() {
    let javascript = emit_ts_with(
        "export {}; var fs=[]; for(let [x] of new Set([[1]])) fs.push(()=>x);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            down_level_iteration: Some(true),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        javascript.contains("var tslib_1 = require(\"tslib\");"),
        "{javascript}"
    );
    assert!(javascript.contains("tslib_1.__values("), "{javascript}");
    assert!(javascript.contains("tslib_1.__read("), "{javascript}");
    assert!(!javascript.contains("var __read ="), "{javascript}");
}

#[test]
fn es5_labeled_suspending_destructured_for_of_keeps_iterator_close() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        down_level_iteration: Some(true),
        ..Default::default()
    };
    let javascript = emit_ts_with(
        "var fs=[],log=[];var customIterable={[Symbol.iterator]:function(){var values=[[1,2],[3,4],[5,6]],i=0;return{next:function(){log.push('next'+i);return i<values.length?{value:values[i++],done:false}:{done:true}},return:function(){log.push('close');return{done:true}}}}};function* g(){label:for(let [a,b] of customIterable){fs.push(()=>a+':'+b);yield a;if(a===1)continue label;if(a===3)break label;}}[...g()];console.log(fs.map(f=>f()).join('|')+';'+log.join(','));",
        options.clone(),
    );
    assert!(javascript.contains("__values"), "{javascript}");
    assert_node_syntax(&javascript);
    assert_eq!(
        execute_with_node(&javascript),
        "1:2|3:4;next0,next1,close",
        "{javascript}"
    );

    let asynchronous = emit_ts_with(
        "var fs=[],log=[];var customIterable={[Symbol.iterator]:function(){var values=[[1,2],[3,4],[5,6]],i=0;return{next:function(){log.push('next'+i);return i<values.length?{value:values[i++],done:false}:{done:true}},return:function(){log.push('close');return{done:true}}}}};async function g(){label:for(let [a,b] of customIterable){fs.push(()=>a+':'+b);await 0;if(a===1)continue label;if(a===3)break label;}}g().then(()=>console.log(fs.map(f=>f()).join('|')+';'+log.join(',')));",
        options,
    );
    assert!(asynchronous.contains("__values"), "{asynchronous}");
    assert_node_syntax(&asynchronous);
    assert_eq!(
        execute_with_node(&asynchronous),
        "1:2|3:4;next0,next1,close",
        "{asynchronous}"
    );
}

#[test]
fn es5_unicode_escaped_helper_name_reservations_are_normalized() {
    let javascript = emit_ts_with(
        "var \\u005floop_1=41,fs=[];for(let i=0;i<2;i++)fs.push(()=>i);console.log(_loop_1,fs.map(f=>f()).join(','));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(javascript.contains("var _loop_2"), "{javascript}");
    assert!(
        !javascript.contains("var _loop_1 = function"),
        "{javascript}"
    );
    assert_eq!(execute_with_node(&javascript), "41 0,1", "{javascript}");
}

#[test]
fn test_es5_computed_object_literal_uses_ordered_assignment_sequence() {
    let js = emit_ts_with(
        "declare var key: PropertyKey; var value = { before: 1, [key]: 2, after: 3 };",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var _a;\nvar value = (_a = { before: 1 }, _a[key] = 2, _a.after = 3, _a);"),
        "expected TypeScript's parenthesized computed-object assignment sequence: {js}"
    );
}

#[test]
fn test_es5_computed_object_accessors_use_ordered_property_descriptors() {
    let js = emit_ts_with(
        "var suffix = 'x'; var sink = 0; function key() { return 'value'; } var object = {\n    get [`pre${suffix}`]() { return 1; },\n    set [key()](value) { sink = value; }\n};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "Object.defineProperty(_a, \"pre\".concat(suffix), {\n        get: function () { return 1; },\n        enumerable: false,\n        configurable: true\n    })"
        ),
        "computed getter must use its raw, downleveled key expression: {js}"
    );
    assert!(
        js.contains(
            "Object.defineProperty(_a, key(), {\n        set: function (value) { sink = value; },\n        enumerable: false,\n        configurable: true\n    })"
        ),
        "computed setter must be emitted as its own descriptor in source order: {js}"
    );
    assert_eq!(js.matches("key()").count(), 2, "{js}");

    let cjs = emit_ts_with(
        "import { value } from './dep'; declare function use(value: unknown): void; var object = { set ['x'](value) { use(value); } };",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        cjs.contains("set: function (value) { use(value); }"),
        "a setter parameter must shadow a same-named CJS import: {cjs}"
    );
    assert!(!cjs.contains("use(dep_1.value)"), "{cjs}");
}

#[test]
fn test_es5_computed_object_accessors_preserve_key_and_receiver_semantics() {
    let js = emit_ts_with(
        "var log = []; function key(name) { log.push('key-' + name); return 'value'; } var object = {\n    get [key('get')]() { log.push(this === object ? 'get-this' : 'bad-get-this'); log.push('get-args-' + arguments.length); return 1; },\n    set [key('set')](value) { log.push(this === object ? 'set-this' : 'bad-set-this'); log.push('set-' + value + '-args-' + arguments.length); }\n}; var value = object.value; object.value = 2; console.log(log.join(','));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert_eq!(
        execute_with_node(&js),
        "key-get,key-set,get-this,get-args-0,set-this,set-2-args-1",
        "{js}"
    );
}

#[test]
fn test_computed_object_accessor_downlevel_keeps_comment_and_target_fallbacks() {
    let source = "var key = 'value'; var object = { /* keep */ get [key]() { return 1; } };";
    let es5 = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(es5.contains("get [key]()"), "{es5}");
    assert!(!es5.contains("Object.defineProperty"), "{es5}");

    let es2015 = emit_ts_with(
        "var key = 'value'; var object = { get [key]() { return 1; } };",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(es2015.contains("get [key]()"), "{es2015}");
    assert!(!es2015.contains("Object.defineProperty"), "{es2015}");
}

#[test]
fn test_es5_multiline_computed_object_uses_structural_sequence_layout() {
    let js = emit_ts_with(
        "var key = 'x'; var value = {\n    [key]: 1,\n    after: 2\n};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var value = (_a = {},\n    _a[key] = 1,\n    _a.after = 2,\n    _a);"),
        "multiline assignments must use TypeScript's continuation indentation: {js}"
    );
}

#[test]
fn test_es5_multiline_computed_object_prefix_is_indented_inside_call() {
    let js = emit_ts_with(
        "function take(value) { return value; } var value = take({\n    plain: 1,\n    zero: () => { },\n    ['key']: 2\n});",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "take((_a = {\n        plain: 1,\n        zero: function () { }\n    },\n    _a['key'] = 2,\n    _a))"
        ),
        "the retained prefix must nest one level inside the call sequence: {js}"
    );
    assert_eq!(
        execute_with_node(&format!("{js}\nconsole.log(value.key);")),
        "2"
    );
}

#[test]
fn test_es5_multiline_computed_methods_and_arrows_preserve_runtime_order() {
    let js = emit_ts_with(
        "var log = []; function key(name) { log.push('key-' + name); return name; } var object = {\n    [key('method')](value) { log.push(this === object ? 'this' : 'bad-this'); log.push(arguments[0] === value ? 'args' : 'bad-args'); },\n    [key('arrow')]: value => { log.push('arrow-' + value); }\n}; object.method(1); object.arrow(2); console.log(log.join(','));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("_a[key('method')] = function (value)")
            && js.contains("_a[key('arrow')] = function (value)"),
        "both supported member forms must become ordinary ES5 functions: {js}"
    );
    assert_eq!(
        execute_with_node(&js),
        "key-method,key-arrow,this,args,arrow-2"
    );
}

#[test]
fn test_es5_multiline_computed_object_temps_follow_function_and_namespace_scopes() {
    let js = emit_ts_with(
        "function local() {\n    var value = {\n        ['x']: 1\n    };\n    return value.x;\n}\nnamespace Scope {\n    var value = {\n        ['y']: 2\n    };\n    export var result = value.y;\n    export function nested() {\n        var inside = {\n            ['z']: 3\n        };\n        return inside.z;\n    }\n}\nconsole.log(local() + Scope.result + Scope.nested());",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function local() {\n    var _a;")
            && js.contains("(function (Scope) {\n    var _a;")
            && js.contains("function nested() {\n        var _a;"),
        "each generated temp must be declared in its structural function scope: {js}"
    );
    assert_eq!(execute_with_node(&js), "6");
}

#[test]
fn test_es5_multiline_computed_return_omits_only_direct_sequence_wrapper() {
    let js = emit_ts_with(
        "var log = []; function key(name) { log.push('key-' + name); return name; } function value(name) { log.push('value-' + name); return name; } function take(value) { return value; } function local() { var _a = 'occupied'; return {\n    [key('local')]: value('local')\n}; } var nested = take({\n    [key('call')]: value('call')\n}); var result = local(); console.log(log.join(',') + ':' + result.local + ':' + nested.call);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "function local() {\n    var _b;\n    var _a = 'occupied';\n    return _b = {},\n        _b[key('local')] = value('local'),\n        _b;\n}"
        ),
        "a direct return sequence must not have redundant parentheses: {js}"
    );
    assert!(!js.contains("return (_b = {}"), "{js}");
    assert!(
        js.contains("var nested = take((_b = {},\n    _b[key('call')] = value('call'),\n    _b));"),
        "a call argument still requires its structural sequence wrapper: {js}"
    );
    assert_eq!(
        execute_with_node(&js),
        "key-call,value-call,key-local,value-local:local:call"
    );
}

#[test]
fn test_es5_multiline_computed_temp_counter_resets_after_prior_top_level_scope() {
    let js = emit_ts_with(
        "var log = []; function key(name) { log.push('key-' + name); return name; } var _a = 'occupied'; var outer = {\n    [key('outer')]: 1\n}; namespace Scope { export var nested = {\n    [key('namespace')]: 2\n}; export function local() { var _a = 'local-occupied'; return {\n    [key('local')]: 3\n}; } } var local = Scope.local(); console.log(log.join(',') + ':' + (outer.outer + Scope.nested.namespace + local.local));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert_eq!(
        js.matches("var _b;").count(),
        3,
        "top-level, namespace, and nested function scopes must each allocate _b: {js}"
    );
    assert!(
        js.contains(
            "(function (Scope) {\n    var _b;\n    Scope.nested = (_b = {},\n        _b[key('namespace')] = 2,\n        _b);"
        ),
        "the namespace must restart allocation after the prior top-level temp: {js}"
    );
    assert!(
        js.contains(
            "function local() {\n        var _b;\n        var _a = 'local-occupied';\n        return _b = {},\n            _b[key('local')] = 3,\n            _b;\n    }"
        ),
        "the nested function must allocate independently without colliding with _a: {js}"
    );
    assert!(
        !js.contains("var _c;"),
        "no outer counter may leak inward: {js}"
    );
    assert_eq!(
        execute_with_node(&js),
        "key-outer,key-namespace,key-local:6"
    );
}

#[test]
fn test_es5_multiline_computed_object_keeps_unsafe_forms_on_native_fallback() {
    let commented = emit_ts_with(
        "var key = 'x'; var value = {\n    // owned comment\n    [key]: 1\n};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(commented.contains("[key]: 1"), "{commented}");
    assert!(!commented.contains("_a[key] = 1"), "{commented}");

    let accessor = emit_ts_with(
        "var key = 'x'; var value = {\n    get plain() { return 1; },\n    [key]: 2\n};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(accessor.contains("get plain()"), "{accessor}");
    assert!(!accessor.contains("Object.defineProperty"), "{accessor}");
    assert!(!accessor.contains("_a[key] = 2"), "{accessor}");

    let parameter = emit_ts_with(
        "var key = 'x'; function take(value = {\n    [key]: 1\n}) { return value; }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        parameter.contains("value = {\n        [key]: 1\n    }"),
        "{parameter}"
    );
    assert!(!parameter.contains("_a[key] = 1"), "{parameter}");

    let destructuring = emit_ts_with(
        "var key = 'x', target, source; ({\n    [key]: target\n} = source);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(destructuring.contains("[key]: target"), "{destructuring}");
    assert!(
        !destructuring.contains("_a[key] = target"),
        "{destructuring}"
    );

    let lexical_arrow = emit_ts_with(
        "var key = 'x'; var value = {\n    [key]: () => this\n};",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        lexical_arrow.contains("[key]: () => this"),
        "{lexical_arrow}"
    );
    assert!(!lexical_arrow.contains("_a[key]"), "{lexical_arrow}");
}

#[test]
fn test_es5_computed_object_method_preserves_order_this_arguments_and_once_only_keys() {
    let js = emit_ts_with(
        "var log = []; function key(x) { log.push('k' + x); return x; } function value(x) { log.push('v' + x); return x; } var object = { first: value('a'), [key('m')](x) { log.push(this === object ? 'this' : 'bad-this'); log.push(arguments[0] === x ? 'args' : 'bad-args'); return x; }, last: value('z') }; object.m(4); console.log(log.join(','));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("_a.m") || js.contains("_a[key('m')]"),
        "computed method must be assigned on the constructed receiver: {js}"
    );
    assert!(
        js.contains("= function (x)"),
        "ES5 output must use an ordinary function for the method: {js}"
    );
    assert_eq!(execute_with_node(&js), "va,km,vz,this,args");
}

#[test]
fn test_es5_computed_object_nested_temps_are_collision_safe_and_outer_first() {
    let js = emit_ts_with(
        "declare var key: PropertyKey; var _a = 1; var value = { [key]: { [key]: _a } };",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var _b, _c;"),
        "source `_a` must reserve the first generated name: {js}"
    );
    assert!(
        js.contains("(_b = {}, _b[key] = (_c = {}, _c[key] = _a, _c), _b)"),
        "outer construction must be captured before its nested value: {js}"
    );
}

#[test]
fn test_es5_computed_object_does_not_rewrite_destructuring_assignment_lhs() {
    let js = emit_ts_with(
        "var key = 'x', target, source = { nested: { x: 1 } }; ({ nested: { [key]: target } } = source);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("({ nested: { [key]: target } } = source);"),
        "destructuring target must remain a target, not become a value sequence: {js}"
    );
    assert!(
        !js.contains("= {},"),
        "unexpected object construction: {js}"
    );
}

#[test]
fn test_es5_computed_object_in_parenthesized_member_assignment_target_is_rewritten() {
    let js = emit_ts_with(
        "var k = 'x', calls = 0; function get(value) { calls++; return { y: value }; } (get({ [k]: 1 }).y) = 2; console.log(calls);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("get((_a = {}, _a[k] = 1, _a)).y) = 2"),
        "an object value nested in an ordinary assignment target must be lowered: {js}"
    );
    assert_eq!(execute_with_node(&js), "1");
}

#[test]
fn test_es5_computed_object_explicit_parens_reuse_sequence_wrapper_exactly_once() {
    let js = emit_ts_with(
        "var k = 'x'; var one = ({ [k]: 1 }); var two = (({ [k]: 2 })); var three = ((({ [k]: 3 }))); var member = ({ ['member']: 4 }).member; var element = ({ [k]: 5 })[k]; var called = ({ [k]: function () { return 6; } })[k](); console.log(one.x + two.x + three.x + member + element + called);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var one = (_a = {}, _a[k] = 1, _a);"),
        "one explicit paren must be consumed by the sequence wrapper: {js}"
    );
    assert!(
        js.contains("var two = ((_b = {}, _b[k] = 2, _b));"),
        "two explicit parens must produce exactly two wrappers: {js}"
    );
    assert!(
        js.contains("var three = (((_c = {}, _c[k] = 3, _c)));"),
        "three explicit parens must produce exactly three wrappers: {js}"
    );
    assert!(
        js.contains("var member = (_d = {}, _d['member'] = 4, _d).member;")
            && js.contains("var element = (_e = {}, _e[k] = 5, _e)[k];")
            && js.contains("var called = (_f = {}, _f[k] = function () { return 6; }, _f)[k]();"),
        "member, element, and call continuations must each retain one wrapper: {js}"
    );
    assert_eq!(execute_with_node(&js), "21");
}

#[test]
fn test_es5_computed_object_type_layer_release_audit_uses_wave4_fallback() {
    let js = emit_ts_with(
        concat!(
            "var k = 'x';",
            "var p1 = ({ [k]: 1 }); var p2 = (({ [k]: 2 })); var p3 = ((({ [k]: 3 })));",
            "var a1 = ({ [k]: 4 } as any).x; var s1 = ({ [k]: 5 } satisfies any).x; var n1 = ({ [k]: 6 }!).x; var t1 = (<any>{ [k]: 7 }).x;",
            "var an = (({ [k]: 8 } as any)!).x; var na = (({ [k]: 9 }!) as any).x; var sn = (({ [k]: 10 } satisfies any)!).x; var ns = (({ [k]: 11 }!) satisfies any).x;",
            "var tn = ((<any>{ [k]: 12 })!).x; var nt = (<any>({ [k]: 13 }!)).x; var as_ = (({ [k]: 14 } as any) satisfies any).x; var sa = (({ [k]: 15 } satisfies any) as any).x;",
            "var at = (<any>({ [k]: 16 } as any)).x; var ta = ((<any>{ [k]: 17 }) as any).x;",
            "var pa = (({ [k]: 18 }) as any).x; var ap = (({ [k]: 19 } as any)).x; var ps = (({ [k]: 20 }) satisfies any).x; var sp = (({ [k]: 21 } satisfies any)).x;",
            "var pn = (({ [k]: 22 })!).x; var np = (({ [k]: 23 }!)).x; var pt = (<any>(({ [k]: 24 }))).x; var tp = (((<any>{ [k]: 25 }))).x;",
            "var x1 = ((({ [k]: 26 } as any)! satisfies any) as any).x; var x2 = ((({ [k]: 27 } satisfies any) as any)! as any)[k];",
            "var x3 = ((<any>(({ [k]: 28 } as any)!)) satisfies any).x; var x4 = ((((<any>{ [k]: 29 })!) as any)).x;",
            "var x5 = (((({ [k]: 30 }) satisfies any)!) as any) || null; var x6 = (<any>((({ [k]: 31 }!)) satisfies any)) || null;",
            "function rPlain() { return (({ [k]: 32 })); } function rAs() { return ({ [k]: 33 } as any); } function rNonNull() { return ({ [k]: 34 }!); }",
            "function rNestedA() { return ((({ [k]: 35 } as any)! satisfies any) as any); } function rNestedB() { return (((({ [k]: 36 } satisfies any)) as any)! as any); }",
            "var keyed = { [(k as any)]: 37 }; var inst = ({ [k]: 38 })<any>;",
            "console.log([p1.x,p2.x,p3.x,a1,s1,n1,t1,an,na,sn,ns,tn,nt,as_,sa,at,ta,pa,ap,ps,sp,pn,np,pt,tp,x1,x2,x3,x4,x5.x,x6.x,rPlain().x,rAs().x,rNonNull().x,rNestedA().x,rNestedB().x,keyed.x,inst.x].join(','));",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var p1 = (_a = {}, _a[k] = 1, _a);")
            && js.contains("var p2 = ((_b = {}, _b[k] = 2, _b));")
            && js.contains("var p3 = (((_c = {}, _c[k] = 3, _c)));")
            && js.contains("function rPlain() { var _a; return ((_a = {}, _a[k] = 32, _a)); }")
            && js.contains("var keyed = (_d = {}, _d[k] = 37, _d);"),
        "plain parens and a type assertion confined to the computed key must remain eligible: {js}"
    );
    let wave4_fallback = [
        "var a1 = { [k]: 4 }.x;",
        "var s1 = { [k]: 5 }.x;",
        "var n1 = ({ [k]: 6 }).x;",
        "var t1 = { [k]: 7 }.x;",
        "var an = ({ [k]: 8 }).x;",
        "var na = ({ [k]: 9 }).x;",
        "var sn = ({ [k]: 10 }).x;",
        "var ns = ({ [k]: 11 }).x;",
        "var tn = ({ [k]: 12 }).x;",
        "var nt = ({ [k]: 13 }).x;",
        "var as_ = { [k]: 14 }.x;",
        "var sa = { [k]: 15 }.x;",
        "var at = { [k]: 16 }.x;",
        "var ta = { [k]: 17 }.x;",
        "var pa = ({ [k]: 18 }).x;",
        "var ap = { [k]: 19 }.x;",
        "var ps = ({ [k]: 20 }).x;",
        "var sp = { [k]: 21 }.x;",
        "var pn = (({ [k]: 22 })).x;",
        "var np = ({ [k]: 23 }).x;",
        "var pt = (({ [k]: 24 })).x;",
        "var tp = { [k]: 25 }.x;",
        "var x1 = { [k]: 26 }.x;",
        "var x2 = { [k]: 27 }[k];",
        "var x3 = ({ [k]: 28 }).x;",
        "var x4 = ({ [k]: 29 }).x;",
        "var x5 = (({ [k]: 30 })) || null;",
        "var x6 = ({ [k]: 31 }) || null;",
        "function rAs() { return ({ [k]: 33 }); }",
        "function rNonNull() { return ({ [k]: 34 }); }",
        "function rNestedA() { return ({ [k]: 35 }); }",
        "function rNestedB() { return ({ [k]: 36 }); }",
        "var inst = (({ [k]: 38 }));",
    ];
    for expected in wave4_fallback {
        assert!(
            js.contains(expected),
            "missing Wave4 fallback `{expected}`: {js}"
        );
    }
    assert_eq!(
        execute_with_node(&js),
        "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38"
    );
}

#[test]
fn test_es5_computed_object_bare_type_layer_roots_use_wave4_fallback() {
    let js = emit_ts_with(
        concat!(
            "var k = 'x';",
            "var asRoot = { [k]: 1 } as any;",
            "var satisfiesRoot = { [k]: 2 } satisfies any;",
            "var nonNullRoot = { [k]: 3 }!;",
            "var assertionRoot = <any>{ [k]: 4 };",
            "var nestedRoot = (({ [k]: 5 } as any)!) satisfies any;",
            "var instantiationRoot = ({ [k]: 6 })<any>;",
            "console.log([asRoot.x,satisfiesRoot.x,nonNullRoot.x,assertionRoot.x,nestedRoot.x,instantiationRoot.x].join(','));",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    for expected in [
        "var asRoot = { [k]: 1 };",
        "var satisfiesRoot = { [k]: 2 };",
        "var nonNullRoot = { [k]: 3 };",
        "var assertionRoot = { [k]: 4 };",
        "var nestedRoot = (({ [k]: 5 }));",
        "var instantiationRoot = (({ [k]: 6 }));",
    ] {
        assert!(
            js.contains(expected),
            "missing exact Wave4 root fallback `{expected}`: {js}"
        );
    }
    assert!(
        !js.contains(" = {},"),
        "type-layer roots allocated a transform temp: {js}"
    );
    assert_eq!(execute_with_node(&js), "1,2,3,4,5,6");
}

#[test]
fn test_es5_computed_object_runtime_boundaries_keep_independent_candidates() {
    let js = emit_ts_with(
        concat!(
            "var k = 'x'; function take(value) { return value; }",
            "var conditional = true ? { [k]: 1 } as any : null;",
            "var binary = { [k]: 2 } as any || null;",
            "var called = take({ [k]: 3 } as any);",
            "var array = [{ [k]: 4 } as any];",
            "var arrow = () => <any>{ [k]: 5 };",
            "console.log([conditional.x,binary.x,called.x,array[0].x,arrow().x].join(','));",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    for expected in [
        "var conditional = true ? (_a = {}, _a[k] = 1, _a) : null;",
        "var binary = (_b = {}, _b[k] = 2, _b) || null;",
        "var called = take((_c = {}, _c[k] = 3, _c));",
        "var array = [(_d = {}, _d[k] = 4, _d)];",
        "var arrow = function () { var _a; return (_a = {}, _a[k] = 5, _a); };",
    ] {
        assert!(
            js.contains(expected),
            "runtime boundary blocked independent candidate `{expected}`: {js}"
        );
    }
    assert_eq!(execute_with_node(&js), "1,2,3,4,5");
}

#[test]
fn test_es5_computed_object_paren_comments_keep_existing_emission_path() {
    let js = emit_ts_with(
        "var k = 'x'; var value = (/* before */ { [k]: 1 } /* after */);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("/* before */") && js.contains("/* after */"),
        "paren-owned comments must not be consumed with the wrapper: {js}"
    );
    assert!(
        js.contains("var value = ( /* before */(_a = {}, _a[k] = 1, _a) /* after */);"),
        "comment-bearing parens must stay on the existing conservative path: {js}"
    );
}

#[test]
fn test_es5_computed_object_in_destructuring_computed_key_is_rewritten() {
    let js = emit_ts_with(
        "var k = 'x', target, source = { '[object Object]': 7 }; ({ [String({ [k]: 1 })]: target } = source); console.log(target);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("{ [String((_a = {}, _a[k] = 1, _a))]: target } = source"),
        "a value expression inside a destructuring computed key must be lowered: {js}"
    );
    assert_eq!(execute_with_node(&js), "7");
}

#[test]
fn test_es5_computed_object_in_destructuring_default_is_rewritten() {
    let js = emit_ts_with(
        "var k = 'x', target, source = {}; ({ target = { [k]: 9 } } = source); console.log(target.x);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("target = (_a = {}, _a[k] = 9, _a)"),
        "a destructuring default is a value expression and must be lowered: {js}"
    );
    assert_eq!(execute_with_node(&js), "9");
}

#[test]
fn test_es5_computed_object_parameter_default_avoids_reentrant_file_temp() {
    let js = emit_ts_with(
        "var entered = 0; function f(value = { [entered++ === 0 ? (f(), 'outer') : 'inner']: entered }) { return value; } console.log(JSON.stringify(f()));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("value = { [entered++ === 0 ? (f(), 'outer') : 'inner']: entered }"),
        "computed objects in parameter defaults must remain native until their temp can be function-scoped: {js}"
    );
    assert!(
        !js.contains("var _a;") && !js.contains("(_a = {}"),
        "a parameter default must not allocate a reentrant file-scope temp: {js}"
    );
    assert_eq!(execute_with_node(&js), "{\"outer\":2}");
}

#[test]
fn test_es5_computed_object_rest_parameter_default_avoids_transform_temp() {
    let js = emit_ts_with(
        "var k = 'x'; function f({ ...rest } = { [k]: 3 }) { return rest[k]; } console.log(f());",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function f(_a = { [k]: 3 })"),
        "rest-transformed parameter defaults must share the computed-object parameter gate: {js}"
    );
    assert!(
        !js.contains("_a = (_") && !js.contains("= {}, _"),
        "a rest-parameter default must not allocate a computed-object temp: {js}"
    );
    assert_eq!(execute_with_node(&js), "3");
}

#[test]
fn test_es5_computed_object_recovery_clone_preserves_destructuring_pattern() {
    let js = emit_ts_with(
        "var k = 'x', target, source = { nested: { x: 4 } }; ({ nested: { [k]: target }, bad(); } = source); console.log(target);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("({ nested: { [k]: target }, } = source);"),
        "recovery-cloned nested patterns must retain destructuring identity: {js}"
    );
    assert!(
        !js.contains("nested: (_a = {}"),
        "a nested recovery pattern must not be value-lowered: {js}"
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "4");
}

#[test]
fn test_es5_computed_object_synthetic_same_span_default_is_value_lowered() {
    fn force_object_literals_to_same_span(expr: &mut Expr, shared: &mut Option<Span>) {
        match &mut expr.kind {
            ExprKind::ObjectLit(props) => {
                if let Some(span) = *shared {
                    expr.span = span;
                } else {
                    *shared = Some(expr.span);
                }
                for prop in props {
                    match prop {
                        ObjLitProp::Property(property) => {
                            force_object_literals_to_same_span(&mut property.value, shared);
                        }
                        ObjLitProp::ShorthandDefault(_, default, _)
                        | ObjLitProp::Spread(default, _) => {
                            force_object_literals_to_same_span(default, shared);
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::Assign(assign) => {
                force_object_literals_to_same_span(&mut assign.left, shared);
                force_object_literals_to_same_span(&mut assign.right, shared);
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                force_object_literals_to_same_span(inner, shared);
            }
            _ => {}
        }
    }

    let source = "var k = 'x', target, source = {}; ({ target = { [k]: 5 } } = source); console.log(target.x);";
    let mut file = tsc_rs_parser::parse("synthetic.ts", source);
    let StmtKind::Expr(assignment) = &mut file.statements[1].kind else {
        panic!("expected assignment expression statement");
    };
    force_object_literals_to_same_span(assignment, &mut None);
    let js = emit(
        &file,
        &CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    )
    .javascript;
    assert!(
        js.contains("target = (_a = {}, _a[k] = 5, _a)"),
        "structural context must distinguish same-span pattern and default value nodes: {js}"
    );
    assert_eq!(execute_with_node(&js), "5");
}

#[test]
fn test_es5_computed_object_nested_function_in_parameter_default_uses_local_temp() {
    let js = emit_ts_with(
        "var k = 'x'; function outer(value = function inner() { return { [k]: 6 }; }) { return value(); } console.log(outer().x);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function inner() { var _a; return _a = {}, _a[k] = 6, _a; }"),
        "a nested function body must leave its enclosing parameter-initializer context: {js}"
    );
    assert_eq!(execute_with_node(&js), "6");
}

#[test]
fn test_es2015_computed_object_literal_remains_native() {
    let js = emit_ts_with(
        "var key = 'x'; var value = { before: 1, [key]: 2 };",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::None),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var value = { before: 1, [key]: 2 };") && !js.contains("var _a;"),
        "ES2015 supports computed object names natively: {js}"
    );
}

// ---------------------------------------------------------------
// Type stripping tests
// ---------------------------------------------------------------

#[test]
fn test_strip_interface_declaration() {
    let js = emit_ts("interface Foo { bar: string; baz: number; }");
    assert!(
        !js.contains("interface"),
        "interface keyword should be stripped: {js}"
    );
}

#[test]
fn test_strip_type_alias() {
    let js = emit_ts("type StringOrNumber = string | number;");
    assert!(!js.contains("type "), "type alias should be stripped: {js}");
    assert!(
        !js.contains("StringOrNumber"),
        "type alias name should not appear: {js}"
    );
}

#[test]
fn test_malformed_import_type_assert_options_recovery_emits_tail_fragments() {
    let js = emit_ts_with(
        "export type LocalInterface =\n\
             & import(\"pkg\", { assert: {1234, \"resolution-mode\": \"require\"} }).RequireInterface\n\
             & import(\"pkg\", { assert: {1234, \"resolution-mode\": \"import\"} }).ImportInterface;\n\
         export const a = (null as any as import(\"pkg\", { assert: {1234, \"resolution-mode\": \"require\"} }).RequireInterface);\n\
         export const b = (null as any as import(\"pkg\", { assert: {1234, \"resolution-mode\": \"import\"} }).ImportInterface);\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "1234, \"resolution-mode\";\n\"require\";\nRequireInterface\n    & import(\"pkg\", { assert: { 1234: , \"resolution-mode\": \"import\" } }).ImportInterface;"
        ),
        "expected malformed import type alias recovery tail: {js}"
    );
    assert!(
        js.contains(
            "exports.a = null;\n1234, \"resolution-mode\";\n\"require\";\nRequireInterface;\n;"
        ),
        "expected malformed import type cast recovery tail for `a`: {js}"
    );
    assert!(
        js.contains(
            "exports.b = null;\n1234, \"resolution-mode\";\n\"import\";\nImportInterface;\n;"
        ),
        "expected malformed import type cast recovery tail for `b`: {js}"
    );
}

#[test]
fn test_malformed_import_type_with_options_recovery_emits_tail_fragments() {
    let js = emit_ts_with(
        "export type LocalInterface =\n\
             & import(\"pkg\", { with: {1234, \"resolution-mode\": \"require\"} }).RequireInterface\n\
             & import(\"pkg\", { with: {1234, \"resolution-mode\": \"import\"} }).ImportInterface;\n\
         export const a = (null as any as import(\"pkg\", { with: {1234, \"resolution-mode\": \"require\"} }).RequireInterface);\n\
         export const b = (null as any as import(\"pkg\", { with: {1234, \"resolution-mode\": \"import\"} }).ImportInterface);\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "1234, \"resolution-mode\";\n\"require\";\nRequireInterface\n    & import(\"pkg\", { with: { 1234: , \"resolution-mode\": \"import\" } }).ImportInterface;"
        ),
        "expected malformed import attributes type alias recovery tail: {js}"
    );
    assert!(
        js.contains(
            "exports.a = null;\n1234, \"resolution-mode\";\n\"require\";\nRequireInterface;\n;"
        ),
        "expected malformed import attributes cast recovery tail for `a`: {js}"
    );
    assert!(
        js.contains(
            "exports.b = null;\n1234, \"resolution-mode\";\n\"import\";\nImportInterface;\n;"
        ),
        "expected malformed import attributes cast recovery tail for `b`: {js}"
    );
}

#[test]
fn test_import_type_missing_options_wrapper_recovery_emits_tail_fragments() {
    let js = emit_ts_with(
        "export type LocalInterface =\n\
             & import(\"pkg\", {\"resolution-mode\": \"require\"}).RequireInterface\n\
             & import(\"pkg\", {\"resolution-mode\": \"import\"}).ImportInterface;\n\
         export const a = (null as any as import(\"pkg\", {\"resolution-mode\": \"require\"}).RequireInterface);\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "\"resolution-mode\";\n\"require\";\nRequireInterface\n    & import(\"pkg\", { \"resolution-mode\": \"import\" }).ImportInterface;"
        ),
        "expected missing import-options wrapper recovery tail: {js}"
    );
    assert!(
        js.contains("exports.a = null;\n\"resolution-mode\";\n\"require\";\nRequireInterface;\n;"),
        "expected missing import-options wrapper cast recovery tail: {js}"
    );
}

#[test]
fn test_import_type_array_options_recovery_drops_stray_close_paren() {
    let js = emit_ts_with(
        "export type LocalInterface =\n\
             & import(\"pkg\", [ {\"resolution-mode\": \"require\"} ]).RequireInterface\n\
             & import(\"pkg\", [ {\"resolution-mode\": \"import\"} ]).ImportInterface;\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(js.contains("\nRequireInterface\n"), "{js}");
    assert!(!js.contains("\n).RequireInterface"), "{js}");
}

#[test]
fn test_import_type_alias_options_recovery_rebuilds_exported_qualifiers() {
    let js = emit_ts_with(
        "type Asserts1 = { assert: {\"resolution-mode\": \"require\"} };\n\
         type Asserts2 = { assert: {\"resolution-mode\": \"import\"} };\n\
         export type LocalInterface =\n\
             & import(\"pkg\", Asserts1).RequireInterface\n\
             & import(\"pkg\", Asserts2).ImportInterface;\n\
         export const a = (null as any as import(\"pkg\", Asserts1).RequireInterface);\n\
         export const b = (null as any as import(\"pkg\", Asserts2).ImportInterface);\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "exports.RequireInterface\n    & import(\"pkg\", exports.Asserts2).ImportInterface;"
        ),
        "{js}"
    );
    assert!(!js.contains("\n).RequireInterface;"), "{js}");
    assert!(!js.contains("\n).ImportInterface;"), "{js}");
}

#[test]
fn test_jsx_fragment_whitespace_closing_tag_preserves_trailing_comment() {
    for jsx in [JsxEmit::Preserve, JsxEmit::React] {
        let file = tsc_rs_parser::parse_with_jsx(
            "file.tsx",
            "<    ></   >; // lots of whitespace\n",
            true,
        );
        let js = emit(
            &file,
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                jsx: Some(jsx),
                ..Default::default()
            },
        )
        .javascript;
        assert!(js.contains("; // lots of whitespace"), "{js}");
    }
}

#[test]
fn test_jsx_immediate_spread_attribute_value_recovers_empty_and_boolean_props() {
    let js = emit_ts_file_with(
        "a.tsx",
        "const X: any = null;\nconst a: any = null;\n<X a={...a} />;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        js.contains("React.createElement(X, { a: , a: true });"),
        "{js}"
    );
}

#[test]
fn test_jsx_spread_props_follow_object_spread_target_and_proto_semantics() {
    let source = "declare const React: any;\n\
                  declare const a: any;\n\
                  declare const __proto__: string;\n\
                  <div className=\"x\" {...a}>child</div>;\n\
                  <div {...{ ...a, ...{ p: 1 } }} />;\n\
                  <div {...{ __proto__: null, dir: \"rtl\" }} />;\n\
                  <div {...{ [__proto__]: null }} />;\n";

    let es2015 = emit_ts_file_with(
        "test.tsx",
        source,
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        es2015.contains("Object.assign({ className: \"x\" }, a)"),
        "{es2015}"
    );

    let es2018 = emit_ts_file_with(
        "test.tsx",
        source,
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        es2018.contains("React.createElement(\"div\", { className: \"x\", ...a }, \"child\")"),
        "{es2018}"
    );
    assert!(
        es2018.contains("React.createElement(\"div\", { ...a, ...{ p: 1 } })"),
        "{es2018}"
    );
    assert!(
        es2018.contains("React.createElement(\"div\", { ...{ __proto__: null, dir: \"rtl\" } })"),
        "{es2018}"
    );
    assert!(
        es2018.contains("React.createElement(\"div\", { [__proto__]: null })"),
        "{es2018}"
    );
    assert!(!es2018.contains("Object.assign"), "{es2018}");
}

#[test]
fn test_react_jsx_native_spread_props_keep_children_and_extract_key() {
    let js = emit_ts_file_with(
        "test.tsx",
        "declare const a: any;\n<div key=\"k\" className=\"x\" {...a}>child</div>;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSX),
            module: Some(ModuleKind::ESNext),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        js.contains("_jsx(\"div\", { className: \"x\", ...a, children: \"child\" }, \"k\")"),
        "{js}"
    );
    assert!(!js.contains("Object.assign"), "{js}");
}

#[test]
fn test_jsx_native_spread_keeps_copy_data_properties_boundary_and_comments() {
    let js = emit_ts_file_with(
        "test.tsx",
        "declare const React: any;\n\
         declare const foo: any;\n\
         <div {...{ /* accessor */ get x() { return 1; }, set y(v: any) {}, m() { return 2; } }} />;\n\
         <div {...foo /* trailing spread */} />;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        js.contains("{ ...{ /* accessor */ get x()"),
        "accessors must stay behind a real spread boundary: {js}"
    );
    assert!(js.contains("set y(v)"), "{js}");
    assert!(js.contains("m()"), "{js}");
    assert!(js.contains("...foo /* trailing spread */"), "{js}");
}

#[test]
fn test_automatic_jsx_spreads_keep_copy_data_properties_boundary_and_comments() {
    let source = "<div {.../* leading spread */ { /* inner object */ get x() { return 1; }, set y(v: any) {}, m() { return 2; } } /* trailing spread */} />;\n\
                  <div {...{ ...{ get nested() { return 3; }, nestedMethod() { return this; } } }} />;\n";
    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        for target in [ScriptTarget::ES2017, ScriptTarget::ES2018] {
            let js = emit_ts_file_with(
                "test.tsx",
                source,
                CompilerOptions {
                    jsx: Some(jsx),
                    module: Some(ModuleKind::CommonJS),
                    target: Some(target),
                    ..Default::default()
                },
            );
            assert!(
                js.contains("/* leading spread */"),
                "{jsx:?} {target:?}: {js}"
            );
            assert!(
                js.contains("/* inner object */"),
                "{jsx:?} {target:?}: {js}"
            );
            assert!(
                js.contains("/* trailing spread */"),
                "{jsx:?} {target:?}: {js}"
            );
            assert!(js.contains("get x()"), "{jsx:?} {target:?}: {js}");
            assert!(js.contains("set y(v)"), "{jsx:?} {target:?}: {js}");
            assert!(js.contains("m()"), "{jsx:?} {target:?}: {js}");
            assert!(js.contains("get nested()"), "{jsx:?} {target:?}: {js}");
            assert!(js.contains("nestedMethod()"), "{jsx:?} {target:?}: {js}");
            if target == ScriptTarget::ES2017 {
                assert!(js.contains("Object.assign"), "{jsx:?}: {js}");
            } else {
                assert!(!js.contains("Object.assign"), "{jsx:?}: {js}");
                assert!(js.contains("... /* leading spread */"), "{jsx:?}: {js}");
            }
        }
    }
}

#[test]
fn test_downlevel_jsx_first_unsafe_spread_uses_fresh_assign_target() {
    let source = "<div {...{ get x() { return 1; } }} />;\n<div {...{ __proto__: null }} />;\n";
    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        let js = emit_ts_file_with(
            "test.tsx",
            source,
            CompilerOptions {
                jsx: Some(jsx),
                module: Some(ModuleKind::CommonJS),
                target: Some(ScriptTarget::ES2017),
                ..Default::default()
            },
        );
        assert!(
            js.contains("Object.assign({}, { get x()"),
            "the accessor source must not become the assign target for {jsx:?}: {js}"
        );
        assert!(
            js.contains("Object.assign({}, { __proto__: null })"),
            "the null-prototype source must not become the assign target for {jsx:?}: {js}"
        );
    }
}

#[test]
fn test_react_jsx_key_after_spread_uses_target_aware_fallback() {
    let source = "declare const ownProto: any;\n<div {...ownProto} key=\"k\" />;\n";
    let es2017 = emit_ts_file_with(
        "test.tsx",
        source,
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSX),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2017),
            ..Default::default()
        },
    );
    assert!(
        es2017.contains("Object.assign({}, ownProto, { key: \"k\" })"),
        "{es2017}"
    );

    let es2018 = emit_ts_file_with(
        "test.tsx",
        source,
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSX),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        es2018.contains("createElement)(\"div\", { ...ownProto, key: \"k\" })"),
        "{es2018}"
    );
    assert!(!es2018.contains("Object.assign"), "{es2018}");
}

#[test]
fn test_react_jsxdev_key_before_spread_has_complete_argument_list() {
    let js = emit_ts_file_with(
        "test.tsx",
        "declare const a: any;\ndeclare function f(x: number): string;\n<div key={f(1)} {...a} />;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSXDev),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(js.contains("{ ...a }, f(1), false, { fileName:"), "{js}");
    assert!(
        js.contains("const _jsxFileName = \"test.tsx\";"),
        "jsxDEV source metadata must reference a declared file name: {js}"
    );
    assert!(!js.contains(", , false"), "jsxDEV output must parse: {js}");
}

#[test]
fn test_react_jsxdev_filename_is_declared_and_executable() {
    let js = emit_ts_file_with(
        "/src/a\"b.tsx",
        "<div />;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSXDev),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const _jsxFileName = \"/src/a\\\"b.tsx\";"),
        "the exact source file name must be escaped into the declaration: {js}"
    );
    let runtime = format!("require = () => ({{ jsxDEV: (...args) => args }});\n{js}");
    let output = run_node_from_stdin(&[], &runtime);
    assert!(
        output.status.success(),
        "jsxDEV output must execute without an undeclared filename: {}\n{js}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_react_jsxdev_filename_binding_avoids_source_collisions() {
    for source in [
        "const _jsxFileName = 123;\n<div />;\n",
        "function f(_jsxFileName: any) { return <div />; }\nf(123);\n",
        "const \\u005FjsxFileName = 123;\n<div />;\n",
    ] {
        let js = emit_ts_file_with(
            "test.tsx",
            source,
            CompilerOptions {
                jsx: Some(JsxEmit::ReactJSXDev),
                module: Some(ModuleKind::CommonJS),
                target: Some(ScriptTarget::ES2018),
                ..Default::default()
            },
        );
        assert!(js.contains("const _jsxFileName_1 = \"test.tsx\";"), "{js}");
        assert!(js.contains("fileName: _jsxFileName_1"), "{js}");
        let runtime = format!("require = () => ({{ jsxDEV: (...args) => args }});\n{js}");
        let output = run_node_from_stdin(&[], &runtime);
        assert!(
            output.status.success(),
            "collision-safe JSXDEV output must execute: {}\n{js}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn test_react_jsxdev_filename_collision_ignores_plain_jsx_text() {
    let text = emit_ts_file_with(
        "test.tsx",
        "<div>_jsxFileName</div>;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSXDev),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        text.contains("const _jsxFileName = \"test.tsx\";"),
        "plain JSX text is not an identifier collision: {text}"
    );

    let attribute = emit_ts_file_with(
        "test.tsx",
        "<div _jsxFileName />;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSXDev),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        attribute.contains("const _jsxFileName_1 = \"test.tsx\";"),
        "JSX attribute names participate in TypeScript's collision scan: {attribute}"
    );
}

#[test]
fn test_react_jsxdev_filename_and_source_locations_match_typescript_units() {
    let at_zero = emit_ts_file_with(
        "test.tsx",
        "<div />;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSXDev),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        at_zero.contains("lineNumber: 1, columnNumber: 1"),
        "a real JSX span at byte zero still has source coordinates: {at_zero}"
    );

    let unicode = emit_ts_file_with(
        "C:\\x\\emoji-😀.tsx",
        "const x=\"😀\"; <div/>;\n",
        CompilerOptions {
            jsx: Some(JsxEmit::ReactJSXDev),
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    assert!(
        unicode.contains("const _jsxFileName = \"C:/x/emoji-\\uD83D\\uDE00.tsx\";"),
        "{unicode}"
    );
    assert!(
        unicode.contains("lineNumber: 1, columnNumber: 15"),
        "columns use UTF-16 code units like TypeScript: {unicode}"
    );

    for separator in ["\r", "\r\n", "\u{2028}", "\u{2029}"] {
        let source = format!("const x = 1;{separator}<div />;\n");
        let js = emit_ts_file_with(
            "test.tsx",
            &source,
            CompilerOptions {
                jsx: Some(JsxEmit::ReactJSXDev),
                module: Some(ModuleKind::CommonJS),
                target: Some(ScriptTarget::ES2018),
                ..Default::default()
            },
        );
        assert!(
            js.contains("lineNumber: 2, columnNumber: 1"),
            "all ECMAScript line terminators use TypeScript coordinates: {js}"
        );
    }

    for (control, escaped) in [
        ('\u{0008}', "\\b"),
        ('\u{000C}', "\\f"),
        ('\u{000B}', "\\v"),
        ('\0', "\\0"),
        ('\u{007F}', "\u{007F}"),
    ] {
        let file_name = format!("control-{control}.tsx");
        let js = emit_ts_file_with(
            &file_name,
            "<div />;\n",
            CompilerOptions {
                jsx: Some(JsxEmit::ReactJSXDev),
                module: Some(ModuleKind::CommonJS),
                target: Some(ScriptTarget::ES2018),
                ..Default::default()
            },
        );
        assert!(
            js.contains(&format!("const _jsxFileName = \"control-{escaped}.tsx\";")),
            "control-character file names need unambiguous JS escapes: {js}"
        );
    }
}

#[test]
fn test_automatic_jsx_empty_inline_spread_preserves_comment() {
    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        for target in [ScriptTarget::ES2017, ScriptTarget::ES2018] {
            let js = emit_ts_file_with(
                "test.tsx",
                "<div {...{ /* empty */ }} />;\n",
                CompilerOptions {
                    jsx: Some(jsx),
                    module: Some(ModuleKind::CommonJS),
                    target: Some(target),
                    ..Default::default()
                },
            );
            assert!(
                js.contains("/* empty */"),
                "{jsx:?} {target:?} must retain the empty spread comment: {js}"
            );

            let line_comment_js = emit_ts_file_with(
                "test.tsx",
                "<div {...{ // inner\n }} />;\n",
                CompilerOptions {
                    jsx: Some(jsx),
                    module: Some(ModuleKind::CommonJS),
                    target: Some(target),
                    ..Default::default()
                },
            );
            assert!(line_comment_js.contains("// inner"), "{line_comment_js}");
            let checked = run_node_from_stdin(&["--check"], &line_comment_js);
            assert!(
                checked.status.success(),
                "line-comment preservation must leave valid JavaScript: {}\n{line_comment_js}",
                String::from_utf8_lossy(&checked.stderr)
            );
        }
    }
}

#[test]
fn test_automatic_jsx_boolean_key_is_extracted() {
    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        let js = emit_ts_file_with(
            "test.tsx",
            "declare const a: any;\n<div key />;\n<div key {...a} />;\n",
            CompilerOptions {
                jsx: Some(jsx),
                module: Some(ModuleKind::CommonJS),
                target: Some(ScriptTarget::ES2018),
                ..Default::default()
            },
        );
        assert!(js.contains("\"div\", {}, true"), "{jsx:?}: {js}");
        assert!(js.contains("\"div\", { ...a }, true"), "{jsx:?}: {js}");
        assert!(!js.contains("key: true"), "{jsx:?}: {js}");
    }
}

#[test]
fn test_automatic_jsx_fallback_only_omits_runtime_module() {
    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        let js = emit_ts_file_with(
            "test.tsx",
            "declare const a: any;\n<div {...a} key=\"k\" />;\n",
            CompilerOptions {
                jsx: Some(jsx),
                module: Some(ModuleKind::CommonJS),
                target: Some(ScriptTarget::ES2018),
                ..Default::default()
            },
        );
        assert!(js.contains("require(\"react\")"), "{jsx:?}: {js}");
        assert!(!js.contains("jsx-runtime"), "{jsx:?}: {js}");
        assert!(!js.contains("jsx-dev-runtime"), "{jsx:?}: {js}");
        assert!(!js.contains("_jsxFileName"), "{jsx:?}: {js}");
    }
}

#[test]
fn test_removed_jsx_spread_comment_does_not_force_boundary() {
    for jsx in [JsxEmit::React, JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        let js = emit_ts_file_with(
            "test.tsx",
            "declare const React: any;\n<div {...{/* removed */ a: 1}} />;\n",
            CompilerOptions {
                jsx: Some(jsx),
                module: Some(ModuleKind::CommonJS),
                target: Some(ScriptTarget::ES2018),
                remove_comments: Some(true),
                ..Default::default()
            },
        );
        assert!(!js.contains("removed"), "{jsx:?}: {js}");
        assert!(!js.contains("...{ a: 1 }"), "{jsx:?}: {js}");
        assert!(js.contains("{ a: 1 }"), "{jsx:?}: {js}");
    }
}

#[test]
fn test_react_jsx_duplicate_key_recovery_preserves_later_evaluation() {
    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        for target in [ScriptTarget::ES2017, ScriptTarget::ES2018] {
            for spread in ["...a", "...{ x: 1 }"] {
                let source = format!(
                    "declare const a: any;\ndeclare function f(x: number): string;\n<div key={{f(1)}} key={{f(2)}} {{{spread}}} />;\n"
                );
                let js = emit_ts_file_with(
                    "test.tsx",
                    &source,
                    CompilerOptions {
                        jsx: Some(jsx),
                        module: Some(ModuleKind::CommonJS),
                        target: Some(target),
                        ..Default::default()
                    },
                );
                let later = js.find("key: f(2)").unwrap_or_else(|| {
                    panic!("later key must remain in props for {jsx:?} {target:?}: {js}")
                });
                let extracted = js.rfind("f(1)").unwrap_or_else(|| {
                    panic!("first key must remain the extracted key for {jsx:?} {target:?}: {js}")
                });
                assert!(later < extracted, "TS-compatible evaluation order: {js}");
            }

            let no_spread = emit_ts_file_with(
                "test.tsx",
                "declare function f(x: number): string;\n<div key={f(1)} key={f(2)} />;\n",
                CompilerOptions {
                    jsx: Some(jsx),
                    module: Some(ModuleKind::CommonJS),
                    target: Some(target),
                    ..Default::default()
                },
            );
            assert!(no_spread.contains("{ key: f(2) }, f(1)"), "{no_spread}");
        }
    }
}

#[test]
fn test_fragment_comment_recovery_ignores_non_jsx_expression_span() {
    let js = emit_ts_file_with(
        "file.ts",
        "var r = < <T>(x: T) => T>f; // valid\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(js.contains("// valid"), "{js}");
}

#[test]
fn test_jsx_unclosed_expression_container_does_not_consume_following_statement() {
    let js = emit_ts_file_with(
        "file.tsx",
        "function foo() {\n    var x = <div>  { </div>\n}\nvar y = { a: 1 };\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(js.contains("var x = <div>  {} </div>;"), "{js}");
    assert!(js.contains("}\nvar y = { a: 1 };"), "{js}");
}

#[test]
fn test_jsx_missing_child_close_is_emitted_before_the_parent_close() {
    for (source, expected) in [
        (
            "const x = <div><span></div>; const after = 1;",
            "const x = <div><span></></div>;\nconst after = 1;\n",
        ),
        (
            "const x = <Foo.Bar><span></Foo.Bar>;",
            "const x = <Foo.Bar><span></></Foo.Bar>;\n",
        ),
        (
            "const x = <div><span>\n  </div>;",
            "const x = <div><span>\n  </></div>;\n",
        ),
        (
            "const x = <div><span><i/></div>;",
            "const x = <div><span><i /></></div>;\n",
        ),
        (
            "const x = <div><span></span></div>;",
            "const x = <div><span></span></div>;\n",
        ),
        ("</>;", " > ;\n"),
    ] {
        let actual = emit_ts_file_with(
            "test.tsx",
            source,
            CompilerOptions {
                jsx: Some(JsxEmit::Preserve),
                target: Some(ScriptTarget::ES2015),
                always_strict: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(actual, expected, "{source}");
    }
}

#[test]
fn test_adjacent_jsx_elements_emit_recovered_comma() {
    let js = emit_ts_file_with(
        "file.tsx",
        "<div></div>\n<span></span>;\nvar x = <div></div><span></span>;\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(
        js.contains("<div></div>\n    ,\n        <span></span>;"),
        "{js}"
    );
    assert!(js.contains("var x = <div></div>, <span></span>;"), "{js}");

    let js = emit_ts_file_with(
        "file.tsx",
        "var x = <div></div><span></span>;\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "var x = (React.createElement(\"div\", null), React.createElement(\"span\", null));"
        ),
        "{js}"
    );
}

#[test]
fn test_multiline_if_or_operand_aligns_with_other_continuations() {
    let js = emit_ts(
        r#"function f() {
    if (a &&
        b &&
        (c && d) ||
            (e && f))
    {
    }
}
"#,
    );
    assert!(js.contains("\n        (e && f)) {"), "{js}");
    assert!(!js.contains("\n            (e && f)) {"), "{js}");
}

#[test]
fn test_multiline_if_trailing_comment_places_block_on_next_line() {
    let js = emit_ts(
        r#"function f() {
  if (a &&
      b ) // reason
  {
    return true;
  }
}
"#,
    );
    assert!(js.contains("        b) // reason\n     {"), "{js}");
}

#[test]
fn test_comment_between_try_close_and_catch_stays_on_own_line() {
    let js = emit_ts_file_with(
        "a.js",
        "try {\n  console.log();\n}\n// @ts-ignore\ncatch (/** @type {number} */ err) {\n  console.log(err);\n}\n",
        CompilerOptions {
            allow_js: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("    console.log();\n}\n// @ts-ignore\ncatch ( /** @type {number} */err) {"),
        "{js}"
    );
}

#[test]
fn test_import_type_defer_conflict_reparses_remaining_clause_as_statements() {
    let js = emit_ts_with(
        "import type defer * as ns1 from \"./a\";\n",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert_eq!(js, "\"use strict\";\n * as;\nns1;\nfrom;\n\"./a\";\n");
}

#[test]
fn test_excessive_type_as_import_chain_is_fully_consumed() {
    let js = emit_ts_with(
        "import { type as as as as } from \"./mod.js\";\n",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert_eq!(js, "export {};\n");
}

#[test]
fn test_function_local_binding_does_not_retain_shadowed_import() {
    let js = emit_ts_with(
        "import a from \"./a\" with { type: \"json\" };\n\
         export async function f() {\n\
           const a = import(\"./a\", { with: { type: \"json\" } });\n\
           a;\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(!js.contains("import a from"), "{js}");
    assert!(js.contains("const a = import("), "{js}");
}

#[test]
fn test_generator_object_method_recovery_recombines_split_var_cases() {
    let opts = CompilerOptions {
        target: ScriptTarget::parse("es6"),
        ..Default::default()
    };

    let js = emit_ts_with("var v = { *() { } }", opts.clone());
    assert_eq!(js.trim(), "\"use strict\";\nvar v = { *() { } };");

    let js = emit_ts_with("var v = { *{ } }", opts.clone());
    assert_eq!(js.trim(), "\"use strict\";\nvar v = { *() { } };");

    let js = emit_ts_with("var v = { *<T>() { } }", opts.clone());
    assert_eq!(js.trim(), "\"use strict\";\nvar v = { *() { } };");

    let js = emit_ts_with("var v = { * }", opts);
    assert_eq!(js.trim(), "\"use strict\";\nvar v = {};");
}

#[test]
fn test_single_multiline_object_method_keeps_inline_object_opener() {
    let js = emit_ts_with(
        "var v = { * foo() {\n    yield(foo);\n  }\n}\n",
        CompilerOptions {
            target: ScriptTarget::parse("es6"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var v = { *foo() {\n        yield (foo);\n    }\n};"),
        "expected single multiline object method to stay inline with the object opener: {js}"
    );
    assert!(
        !js.contains("var v = {\n    *foo()"),
        "object literal opener should not be moved to its own line for this recovery-preserving shape: {js}"
    );
}

#[test]
fn test_class_missing_name_generator_method_recovery_emits_from_source() {
    let js = emit_ts_with(
        "class C {\n   *() { }\n}\n",
        CompilerOptions {
            target: ScriptTarget::parse("es6"),
            ..Default::default()
        },
    );
    assert_eq!(js.trim(), "\"use strict\";\nclass C {\n    *() { }\n}");
}

#[test]
fn test_if_invalid_character_mul_recovery_splits_tail_stmt() {
    let js = emit_ts_with(
        "class C {\n  foo() {\n    if (a) \u{00AC} * bar;\n    return bar;\n  }\n}\n",
        CompilerOptions {
            target: ScriptTarget::parse("es6"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("if (a)\n            ;\n         * bar;\n        return bar;"),
        "expected invalid-character `if` recovery to split the `* bar;` tail into its own statement: {js}"
    );
}

#[test]
fn test_constructor_with_incomplete_type_annotation_namespace_recovery_matches_reference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let case_path = root.join("cases/compiler/constructorWithIncompleteTypeAnnotation.ts");
    let reference_path =
        root.join("baselines/reference/constructorWithIncompleteTypeAnnotation.js");
    let source = std::fs::read_to_string(&case_path).expect("failed to read compiler case");
    let source = format!(
        "{}\n",
        source
            .lines()
            .filter(|line| !line.trim_start().starts_with("// @"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let file = tsc_rs_parser::parse(
        case_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("constructorWithIncompleteTypeAnnotation.ts"),
        &source,
    );
    let js = emit(
        &file,
        &CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            ..Default::default()
        },
    )
    .javascript;
    let reference =
        std::fs::read_to_string(&reference_path).expect("failed to read reference baseline");
    let (_, expected_js) = reference
        .split_once("//// [constructorWithIncompleteTypeAnnotation.js]")
        .expect("missing JS baseline section");

    assert_eq!(
        normalize_newlines(js.trim_end()),
        normalize_newlines(expected_js.trim()),
        "full emit should match the compiler reference baseline"
    );
}

#[test]
fn test_malformed_strict_eq_assignment_recovery_recombines_following_tokens() {
    let js = emit_ts_with(
        "export = } x = ( y = z ==== 'function') {\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(
        js.contains("x = (y = z === ) = 'function';\n{\n}\n"),
        "expected malformed strict-equality assignment recovery to merge following tokens: {js}"
    );
    assert!(
        !js.contains("x = (y = z === =);"),
        "standalone malformed assignment placeholder should not remain: {js}"
    );
    assert!(
        !js.contains("'function';\n{ }"),
        "string literal and block should be folded into the recovery assignment: {js}"
    );
}

#[test]
fn test_invalid_in_lhs_assignment_expr_stmt_recovery_splits_into_two_statements() {
    let js = emit_ts_with(
        "class Foo {\n\
         #field = 1;\n\
         invalidLHS(v: any) {\n\
             'prop' in v = 10;\n\
             #field in v = 10;\n\
         }\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2020),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);

    assert!(
        js.contains("'prop' in v;\n        10;\n"),
        "plain `in` assignment recovery should split into two statements: {js}"
    );
    assert!(
        js.contains("__classPrivateFieldIn(_Foo_field, v);\n        10;\n"),
        "private brand-check assignment recovery should preserve downlevel emit on the left side: {js}"
    );
    assert!(
        !js.contains("'prop' in v = 10;"),
        "invalid assignment should not survive recovery unchanged: {js}"
    );
    assert!(
        !js.contains("__classPrivateFieldIn(_Foo_field, v) = 10;"),
        "downleveled private brand-check assignment should be split, not re-assigned: {js}"
    );
}

#[test]
fn test_class_wrapped_try_property_recovery_emits_top_level_try_tail() {
    let js = emit_ts_with(
        "class Foo {\n\
         \n\
             try {\n\
         \n\
                 public bar = someInitThatMightFail();\n\
         \n\
             } catch(e) {}\n\
         \n\
         \n\
         \n\
             public baz() {\n\
         \n\
                 return this.bar; // doesn't get rewritten to Foo.bar.\n\
         \n\
             }\n\
         \n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("class Foo {\n}\ntry {\n    bar = someInitThatMightFail();\n}\ncatch (e) { }\nbaz();\n{\n    return this.bar; // doesn't get rewritten to Foo.bar.\n}\n"),
        "expected class-wrapped try recovery tail: {js}"
    );
    assert!(
        !js.contains("constructor() {\n        this.bar = someInitThatMightFail();"),
        "field initializer should not be lowered into a synthesized constructor in this recovery shape: {js}"
    );
}

#[test]
fn test_async_iterable_return_type_recovery_recombines_split_tail() {
    let js = emit_ts_with(
        "export async function arrayFromAsync<T>(asyncIterable!: AsyncIterable<T>): Promise<T[]> {\n\
             const out = [];\n\
             for await (const v of asyncIterable) {\n\
                 out.push(await v);\n\
             }\n\
             return out;\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            " > ;\nPromise < T[] > {\n    const: out = [],\n    for: await (), const: v, of, asyncIterable,\n    out, : .push(await v)\n};\nreturn out;\n;\nexport {};\n"
        ),
        "expected async-iterable return type recovery tail: {js}"
    );
    assert!(
        !js.contains("!:;\n(AsyncIterable);"),
        "split placeholder statements should be recombined into the recovery tail: {js}"
    );
}

#[test]
fn test_function_decl_reserved_word_param_recovery_emits_tail_statements() {
    let js = emit_ts_with(
        "function f1(enum) {}\n\
         function f2(class) {}\n\
         function f3(function) {}\n\
         function f4(while) {}\n\
         function f5(for) {}\n\
         function f6([while, for, public]) {}\n\
         function f7(...while) {}\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "function f1() { }\nvar ;\n(function () {\n})( || ( = {}));\n{ }\nfunction f2() { }\nclass {\n}\n{ }\nfunction f3() { }\nfunction () { }\n{ }\nfunction f4() { }\nwhile () { }\nfunction f5() { }\nfor (;;) { }\nfunction f6([]) { }\nwhile (, )\n    for (, public; ; )\n        ;\n{ }\nfunction f7(...) { }\nwhile () { }\n"
        ),
        "expected reserved-word parameter recovery tails after stripped function declarations: {js}"
    );
}

#[test]
fn test_jsdoc_function_types_in_typescript_do_not_consume_runtime_code() {
    let js = emit_ts(
        "function hof(ctor: function(new: number, string)) { return new ctor('hi'); }\n\
         function hof2(f: function(this: number, string): string) { return f(12, 'hi'); }\n\
         var ques: ? = 'what';\n\
         var g: function(number, number): number = (n, m) => n + m;",
    );
    assert!(js.contains("return new ctor('hi');"), "{js}");
    assert!(js.contains("return f(12, 'hi');"), "{js}");
    assert!(js.contains("var ques = 'what';"), "{js}");
    assert!(js.contains("var g = (n, m) => n + m;"), "{js}");
}

#[test]
fn test_invalid_jsx_attribute_names_recover_as_trailing_statements() {
    let js = emit_ts_file_with(
        "test.tsx",
        "<test1 32data={32} />;\n<test2 -data={32} />;",
        CompilerOptions {
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(
        js.contains("<test1 />;\n32;\ndata = { 32:  } /  > ;"),
        "{js}"
    );
    assert!(js.contains("<test2 /> - data;\n{\n    32;\n}\n/>;"), "{js}");
}

#[test]
fn test_jsx_namespace_member_tag_recovers_suffix_and_following_statement() {
    let js = emit_ts_file_with(
        "test.tsx",
        "<b:c.x></b:c.x>;\n<this:b></this:b>;",
        CompilerOptions {
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(
        js.contains("<b:c x></b:c>;\nx > ;\n<this:b></this:b>;"),
        "{js}"
    );
}

#[test]
fn test_reserved_word_declaration_recovery_emits_split_reserved_tails() {
    let js = emit_ts_with(
        "import while = require(\"dfdf\");\n\
         import * as  while from \"foo\"\n\
         \n\
         var typeof = 10;\n\
         function throw() {}\n\
         namespace void {}\n\
         var {while, return} = { while: 1, return: 2 };\n\
         var {this, switch: { continue} } = { this: 1, switch: { continue: 2 }};\n\
         var [debugger, if] = [1, 2];\n\
         enum void {}\n\
         function f(default: number) {}\n\
         class C { m(null: string) {} }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "require();\nwhile ( = require(\"dfdf\"))\n    ;\nwhile (from)\n    \"foo\";\nvar ;\ntypeof ;\n10;\nfunction () { }\nthrow () => { };\nnamespace;\nvoid {};\nvar { while: , return:  } = { while: 1, return: 2 };\nvar { this: , switch: { continue:  } } = { this: 1, switch: { continue: 2 } };\nvar [];\ndebugger;\nif ()\n    ;\n[1, 2];\n(function () {\n})( || ( = {}));\nvoid {};\nfunction f() { }\nclass C {\n    m(, string) { }\n}\n"
        ),
        "expected reserved-word declaration recovery tails: {js}"
    );
}

#[test]
fn test_strip_type_annotation_on_variable() {
    let js = emit_ts("const x: number = 42;");
    assert!(
        js.contains("const x = 42;"),
        "type annotation should be stripped: {js}"
    );
    assert!(
        !js.contains(": number"),
        "type annotation should not appear: {js}"
    );
}

#[test]
fn test_invalid_unicode_escape_named_import_elides_to_module_marker() {
    let js = emit_ts_esm(r#"import { foo as \uD800\uDEA7 } from "./mod";"#);
    assert_eq!(js, "export {};\n");
}

#[test]
fn test_param_default_class_expr_static_field_uses_compact_body_and_late_temp() {
    let js = emit_ts_with(
        "let a = class { static x = 1 };\n\
         let b = class { static x = 2 };\n\
         function f(c = class { static x = 3 }) {}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function f(c) { var _c; if (c === void 0) { c = (_c = class {"),
        "expected compact function body with deferred temp allocation: {js}"
    );
}

#[test]
fn test_single_line_array_of_static_class_expr_gets_extra_indent() {
    let js = emit_ts_with(
        "let [c] = [class { static x = 1 }];",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("[(_a = class {\n        },\n        _a.x = 1,\n        _a)];"),
        "expected transformed class expression inside array to keep array indentation: {js}"
    );
}

#[test]
fn test_binding_pattern_computed_class_expr_with_static_field_uses_structured_emit() {
    let js = emit_ts_with(
        "(({ [class { static x = 1 }.x]: b = \"\" }) => {})();",
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            strict: Some(false),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);
    assert!(
        js.contains("(({ [class {\n    static x = 1;\n}.x]: b = \"\" }) => { })();"),
        "expected computed binding-pattern key to force structured class expr emit: {js}"
    );
    assert!(
        !js.contains("class { static x = 1 }.x"),
        "class expression inside computed key should not use source-copy fast path: {js}"
    );
}

#[test]
fn test_type_literal_less_than_recovery_emits_operator_tails() {
    let js = emit_ts(
        "var a: { x: number; <- };\n\
         var b: { x: number; <+ };\n\
         var c: { x: number; <! };\n\
         var d: { x: number; <~ };",
    );
    assert!(
        js.contains("var a;\n-;\n;"),
        "expected `a` recovery tail: {js}"
    );
    assert!(
        js.contains("var b;\n+;\n;"),
        "expected `b` recovery tail: {js}"
    );
    assert!(
        js.contains("var c;\n;"),
        "expected `c` empty recovery tail: {js}"
    );
    assert!(
        js.contains("var d;\n~;\n;"),
        "expected `d` recovery tail: {js}"
    );
}

#[test]
fn test_invalid_unicode_identifier_escape_var_recovery() {
    let js = emit_ts("var arg\\u003\nvar arg2\\uxxxx");
    assert!(
        js.contains("var arg, u003;"),
        "expected invalid unicode escape tail to stay in the variable declarator list: {js}"
    );
    assert!(
        js.contains("var arg2, uxxxx;"),
        "expected invalid unicode escape tail to stay in the variable declarator list: {js}"
    );
}

#[test]
fn test_invalid_unicode_identifier_escape_non_identifier_start_var_recovery() {
    let source = "var \\u0031a;";
    let file = tsc_rs_parser::parse("test.ts", source);
    let StmtKind::Var(var_stmt) = &file.statements[0].kind else {
        panic!("expected variable statement");
    };
    assert_eq!(
        var_stmt.declarations.len(),
        2,
        "expected parser recovery to keep an error placeholder plus `u0031a`"
    );
    let between = &source
        [var_stmt.declarations[0].span.end as usize..var_stmt.declarations[1].span.start as usize];
    assert!(
        !between.contains(','),
        "expected recovered declarator split to be source-visible: {between:?}"
    );
    let js = emit(&file, &CompilerOptions::default()).javascript;
    assert!(
        js.contains("var u0031a;"),
        "expected emitter to skip the invalid escape marker and keep the recovered declarator: {js}"
    );
}

#[test]
fn test_module_preserve_import_helpers_uses_named_tslib_import_for_simple_standard_decorators() {
    let js = emit_ts_file_with(
        "a.mts",
        "declare var dec: any;\n\n@dec()\nexport class A {}\n",
        CompilerOptions {
            module: Some(ModuleKind::Preserve),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("import { __esDecorate, __runInitializers } from \"tslib\";"),
        "expected preserve ESM helper import: {js}"
    );
    assert!(
        js.contains("let A = (() => {"),
        "expected standard-decorator wrapper: {js}"
    );
    assert!(js.contains("export { A };"), "expected named export: {js}");
}

#[test]
fn test_es2015_public_multi_method_decorators_match_stage3_transform() {
    let js = emit_ts_with(
        "declare let dec: any;\n\
         const method3 = \"method3\";\n\
         class C {\n\
             @dec(1) method1() {}\n\
             @dec(2) [\"method2\"]() {}\n\
             @dec(3) [method3]() {}\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&js),
        concat!(
            "\"use strict\";\n",
            "const method3 = \"method3\";\n",
            "let C = (() => {\n",
            "    var _a;\n",
            "    var _b;\n",
            "    let _instanceExtraInitializers = [];\n",
            "    let _method1_decorators;\n",
            "    let _member_decorators;\n",
            "    let _member_decorators_1;\n",
            "    return _a = class C {\n",
            "            method1() { }\n",
            "            [\"method2\"]() { }\n",
            "            [(_method1_decorators = [dec(1)], _member_decorators = [dec(2)], _member_decorators_1 = [dec(3)], _b = __propKey(method3))]() { }\n",
            "            constructor() {\n",
            "                __runInitializers(this, _instanceExtraInitializers);\n",
            "            }\n",
            "        },\n",
            "        (() => {\n",
            "            const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;\n",
            "            __esDecorate(_a, null, _method1_decorators, { kind: \"method\", name: \"method1\", static: false, private: false, access: { has: obj => \"method1\" in obj, get: obj => obj.method1 }, metadata: _metadata }, null, _instanceExtraInitializers);\n",
            "            __esDecorate(_a, null, _member_decorators, { kind: \"method\", name: \"method2\", static: false, private: false, access: { has: obj => \"method2\" in obj, get: obj => obj[\"method2\"] }, metadata: _metadata }, null, _instanceExtraInitializers);\n",
            "            __esDecorate(_a, null, _member_decorators_1, { kind: \"method\", name: _b, static: false, private: false, access: { has: obj => _b in obj, get: obj => obj[_b] }, metadata: _metadata }, null, _instanceExtraInitializers);\n",
            "            if (_metadata) Object.defineProperty(_a, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });\n",
            "        })(),\n",
            "        _a;\n",
            "})();\n",
        )
    );
}

#[test]
fn test_public_multi_member_standard_decorator_order_and_runtime() {
    let js = emit_ts_with(
        "const events: string[] = [];\n\
         function memberName() { events.push('key'); return 'dynamic'; }\n\
         function decorate(tag: string) {\n\
             events.push('eval:' + tag);\n\
             return function (value: any, context: any) {\n\
                 events.push('apply:' + tag + ':' + context.kind);\n\
                 context.addInitializer(function () { events.push('init:' + tag); });\n\
                 return function (this: any, ...args: any[]) {\n\
                     events.push('call:' + tag);\n\
                     return value.apply(this, args);\n\
                 };\n\
             };\n\
         }\n\
         class C {\n\
             constructor() { (this as any).value = 1; }\n\
             @decorate('m') m() { return 2; }\n\
             @decorate('g') get x() { return (this as any).value; }\n\
             @decorate('s') set x(value: number) { (this as any).value = value; }\n\
             @decorate('d') [memberName()]() { return 3; }\n\
         }\n\
         const c = new C();\n\
         const m = c.m();\n\
         const before = c.x;\n\
         c.x = 4;\n\
         const dynamic = (c as any).dynamic();\n\
         console.log(events.join('|'));\n\
         console.log([m, before, (c as any).value, dynamic].join('|'));",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("_b = __propKey(memberName())"),
        "computed key must be captured once: {js}"
    );
    assert_eq!(
        execute_with_node(&js),
        concat!(
            "eval:m|eval:g|eval:s|eval:d|key|",
            "apply:m:method|apply:g:getter|apply:s:setter|apply:d:method|",
            "init:m|init:g|init:s|init:d|call:m|call:g|call:s|call:d\n",
            "2|1|4|3"
        )
    );
}

#[test]
fn test_public_multi_member_standard_decorator_temps_avoid_user_names() {
    let js = emit_ts_with(
        "declare const dec: any; const _a = 1; const key = 'x';\n\
         class C { @dec m() {} @dec [key]() {} }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(js.contains("var _b;\n    var _c;"), "{js}");
    assert!(js.contains("_c = __propKey(key)"), "{js}");
    assert!(!js.contains("return _a = class C"), "{js}");
}

#[test]
fn test_public_multi_member_standard_decorator_bridge_rejects_private_and_fields() {
    for source in [
        "declare const dec: any; class C { @dec m() {} @dec #private() {} }",
        "declare const dec: any; class C { @dec m() {} @dec field = 1; }",
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                ..Default::default()
            },
        );
        assert!(
            !js.contains("let C = (() =>"),
            "unsafe shape was claimed: {js}"
        );
    }
}

#[test]
fn test_public_multi_member_standard_decorators_match_static_first_audit_order() {
    let source = r#"
const events: string[] = [];
function key(tag: string) { events.push(`key:${tag}`); return tag; }
function dec(tag: string) {
    events.push(`eval:${tag}`);
    return function(value: any, context: any) {
        events.push(`apply:${tag}:${context.kind}:${String(context.name)}:${context.static}`);
        const scope = context.static ? "static" : "instance";
        context.addInitializer(function(this: any) { events.push(`init:${tag}:${scope}`); });
        if (context.kind === "setter") {
            return function(this: any, x: any) { events.push(`call:${tag}`); value.call(this, x); };
        }
        return function(this: any, ...args: any[]) { events.push(`call:${tag}`); return value.apply(this, args); };
    }
}
class C {
    @dec("a1") @dec("a2") [key("a")]() { return 1; }
    @dec("sg") static get z() { return 3; }
    @dec("ss") static set z(v: number) { events.push(`set:z:${v}`); }
    @dec("b") [key("b")]() { return 2; }
    @dec("g") get x() { return 4; }
    @dec("s") set x(v: number) { events.push(`set:x:${v}`); }
}
const c = new C();
c.a(); c.b(); void c.x; c.x = 9; void C.z; C.z = 8;
console.log(events.join("|"));
"#;
    let expected = concat!(
        "eval:a1|eval:a2|key:a|eval:sg|eval:ss|eval:b|key:b|eval:g|eval:s|",
        "apply:sg:getter:z:true|apply:ss:setter:z:true|",
        "apply:a2:method:a:false|apply:a1:method:a:false|apply:b:method:b:false|",
        "apply:g:getter:x:false|apply:s:setter:x:false|",
        "init:sg:static|init:ss:static|init:a2:instance|init:a1:instance|",
        "init:b:instance|init:g:instance|init:s:instance|",
        "call:a1|call:a2|call:b|call:g|call:s|set:x:9|call:sg|call:ss|set:z:8"
    );
    for target in [ScriptTarget::ES2015, ScriptTarget::ES2022] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(target),
                ..Default::default()
            },
        );
        assert_eq!(execute_with_node(&js), expected, "target={target:?}\n{js}");
    }
}

#[test]
fn test_public_multi_member_standard_decorator_bindings_avoid_outer_capture() {
    let js = emit_ts_with(
        r#"
let calls = 0;
const _m_decorators = function(value: any, context: any) { calls++; };
const _instanceExtraInitializers = [function() { calls += 100; }];
const _metadata = 2;
class C { constructor() {} @_m_decorators m() {} }
new C().m();
console.log(calls);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(js.contains("let _m_decorators_1;"), "{js}");
    assert!(
        js.contains("let _instanceExtraInitializers_1 = [];"),
        "{js}"
    );
    assert!(js.contains("const _metadata_1 ="), "{js}");
    assert_eq!(execute_with_node(&js), "1");
}

#[test]
fn test_public_multi_member_standard_decorator_static_bindings_avoid_outer_capture() {
    let js = emit_ts_with(
        r#"
let calls = 0;
function dec(value: any, context: any) { calls++; }
const _static_s_decorators = dec;
const _staticExtraInitializers = [function() { calls += 100; }];
class C { @_static_s_decorators static s() {} }
C.s();
console.log(calls);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2022),
            ..Default::default()
        },
    );
    assert!(js.contains("let _static_s_decorators_1;"), "{js}");
    assert!(js.contains("let _staticExtraInitializers_1 = [];"), "{js}");
    assert_eq!(execute_with_node(&js), "1");
}

#[test]
fn test_public_multi_member_standard_decorator_paired_accessor_bindings_do_not_capture() {
    let js = emit_ts_with(
        r#"
let calls = 0;
function dec(value: any, context: any) { calls++; }
const _get_x_decorators = dec;
const _set_x_decorators = dec;
class C {
    @_get_x_decorators get x() { return 1; }
    @_set_x_decorators set x(value: number) {}
}
const c = new C(); void c.x; c.x = 2;
console.log(calls);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2022),
            ..Default::default()
        },
    );
    assert!(js.contains("let _get_x_decorators_1;"), "{js}");
    assert!(js.contains("let _set_x_decorators_1;"), "{js}");
    assert_eq!(execute_with_node(&js), "2");
}

#[test]
fn test_public_multi_member_standard_decorator_rejects_parameter_properties() {
    let js = emit_ts_with(
        r#"
const events: string[] = [];
function dec(value: any, context: any) {
    context.addInitializer(function() { events.push("init"); });
}
class C {
    @dec m() {}
    constructor(public x = (events.push("param"), 1)) { events.push("body"); }
}
new C();
console.log(events.join("|"));
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(js.contains("return class C {"), "prior path expected: {js}");
    assert!(
        !js.contains("return _a = class C"),
        "structural wrapper incorrectly claimed parameter property: {js}"
    );
}

#[test]
fn test_public_multi_member_standard_decorator_export_and_namespace_runtime() {
    let js = emit_ts_with(
        r#"
let calls = 0;
function dec(value: any, context: any) { calls++; }
export class E { @dec a() {} @dec static b() {} }
namespace N { export class C { @dec a() {} @dec static b() {} } }
new E().a(); E.b(); new N.C().a(); N.C.b();
console.log(calls);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert_eq!(execute_with_node(&js), "4", "{js}");
}

#[test]
fn test_public_multi_member_standard_decorator_named_default_export_routes() {
    let source = r#"
let calls = 0;
function dec(value: any, context: any) { calls++; }
export default class C { @dec a() {} @dec static b() {} }
new C().a(); C.b();
console.log(calls);
"#;
    let esm = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(esm.contains("let C = (() => {"), "{esm}");
    assert!(esm.contains("return _a = class C {"), "{esm}");
    assert!(esm.contains("export default C;"), "{esm}");
    assert_eq!(execute_module_with_node(&esm), "2", "{esm}");

    let commonjs = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(commonjs.contains("let C = (() => {"), "{commonjs}");
    assert!(commonjs.contains("return _a = class C {"), "{commonjs}");
    assert!(commonjs.contains("exports.default = C;"), "{commonjs}");
    assert_eq!(execute_with_node(&commonjs), "2", "{commonjs}");

    let commented = emit_ts_with(
        "function dec(value: any, context: any) {}\n\
         export default class C { /*before*/ @dec a() {} @dec static b() {} }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            remove_comments: Some(false),
            ..Default::default()
        },
    );
    assert!(commented.contains("return class C {"), "{commented}");
    assert!(!commented.contains("return _a = class C"), "{commented}");
    assert!(commented.contains("/*before*/"), "{commented}");
    assert!(commented.contains("export default C;"), "{commented}");
}

#[test]
fn test_public_multi_member_standard_decorator_comments_fail_closed() {
    let source = r#"
function dec(value: any, context: any) {}
class C {
    /*before*/ @dec /*decorator-a*/ a() {} // between-members
    @dec /*decorator-b*/ static b() {} /*after-member*/
}
"#;
    let preserved = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            remove_comments: Some(false),
            ..Default::default()
        },
    );
    assert!(preserved.contains("return class C {"), "{preserved}");
    assert!(!preserved.contains("return _a = class C"), "{preserved}");
    assert!(preserved.contains("/*before*/"), "{preserved}");

    let removed = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            remove_comments: Some(true),
            ..Default::default()
        },
    );
    assert!(removed.contains("return _a = class C"), "{removed}");
    for comment in [
        "/*before*/",
        "/*decorator-a*/",
        "// between-members",
        "/*decorator-b*/",
        "/*after-member*/",
    ] {
        assert!(!removed.contains(comment), "retained {comment}: {removed}");
    }
}

#[test]
fn test_module_preserve_import_helpers_uses_require_for_cts_standard_decorators() {
    let js = emit_ts_file_with(
        "b.cts",
        "declare var dec: any;\n\n@dec()\nclass B {}\nexport {};\n",
        CompilerOptions {
            module: Some(ModuleKind::Preserve),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const tslib_1 = require(\"tslib\");"),
        "expected preserve CJS helper require: {js}"
    );
    assert!(
        js.contains("tslib_1.__esDecorate("),
        "expected CJS helper prefix: {js}"
    );
    assert!(
        js.contains("let B = (() => {"),
        "expected standard-decorator wrapper: {js}"
    );
}

#[test]
fn test_module_preserve_erased_module_syntax_produces_empty_files() {
    let options = CompilerOptions {
        module: Some(ModuleKind::Preserve),
        target: Some(ScriptTarget::ESNext),
        ..Default::default()
    };

    for (file_name, source) in [
        ("g.ts", "import {} from \"./a\";"),
        ("h.mts", "export interface OnlyType {}"),
        ("i.cts", "export type OnlyType = string;"),
        ("dummy.ts", "export {};"),
    ] {
        let js = emit_ts_file_with(file_name, source, options.clone());
        assert_eq!(
            js, "",
            "{file_name} should not contain a synthesized marker or newline: {js:?}"
        );
    }
}

#[test]
fn test_module_preserve_keeps_runtime_and_verbatim_exports() {
    let js = emit_ts_with(
        "export const value = 1;",
        CompilerOptions {
            module: Some(ModuleKind::Preserve),
            target: Some(ScriptTarget::ESNext),
            ..Default::default()
        },
    );
    assert_eq!(js, "export const value = 1;\n");

    let verbatim = emit_ts_with(
        "export {};",
        CompilerOptions {
            module: Some(ModuleKind::Preserve),
            target: Some(ScriptTarget::ESNext),
            verbatim_module_syntax: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(verbatim, "export {};\n");
}

#[test]
fn test_async_function_for_await_downlevels_with_async_values_helper() {
    let js = emit_ts_file_with(
        "test.ts",
        "async function fn(c: Promise<string[]>) {\n    for await (const s of c) {}\n}\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __asyncValues = (this && this.__asyncValues) || function (o) {"),
        "expected __asyncValues helper: {js}"
    );
    assert!(
        js.contains("for (_d = true, c_1 = __asyncValues(c);"),
        "expected for-await downlevel loop: {js}"
    );
    assert!(
        js.contains("const s = _c;"),
        "expected loop binding inside downleveled body: {js}"
    );
}

#[test]
fn test_anonymous_standard_decorated_class_expr_with_fields_and_accessors_lowering() {
    let js = emit_ts_with(
        "declare var dec: any;\n\
         const Foo = class {\n\
             @dec(1) field: undefined;\n\
             @dec(2) static field: undefined;\n\
             @dec(3) accessor accessor: undefined;\n\
             @dec(4) static accessor accessor: undefined;\n\
         };\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __esDecorate = (this && this.__esDecorate) || function"),
        "expected inline __esDecorate helper: {js}"
    );
    assert!(
        js.contains("var __runInitializers = (this && this.__runInitializers) || function"),
        "expected inline __runInitializers helper: {js}"
    );
    assert!(
        js.contains("static { __setFunctionName(this, \"Foo\"); }"),
        "expected class expression named-evaluation helper: {js}"
    );
    assert!(
        js.contains("const Foo = (() => {"),
        "expected class expression IIFE wrapper: {js}"
    );
    assert!(
        js.contains(
            "static { this.field = __runInitializers(this, _static_field_initializers, void 0); }"
        ),
        "expected static decorated field lowering: {js}"
    );
    assert!(
        js.contains("this.field = __runInitializers(this, _field_initializers, void 0);"),
        "expected instance decorated field constructor lowering: {js}"
    );
    assert!(
        js.contains("static #accessor_1_accessor_storage = (__runInitializers(this, _static_field_extraInitializers), __runInitializers(this, _static_accessor_initializers, void 0));"),
        "expected static accessor storage lowering: {js}"
    );
    assert!(
        js.contains("this.#accessor_accessor_storage = (__runInitializers(this, _field_extraInitializers), __runInitializers(this, _accessor_initializers, void 0));"),
        "expected instance accessor storage lowering: {js}"
    );
}

#[test]
fn test_class_expr_computed_field_temps_follow_source_order() {
    let js = emit_ts_with(
        "async function* test(x: Promise<string>) {\n\
             return class {\n\
                 [await x] = await x;\n\
                 static [await x] = await x;\n\
\n\
                 [yield 1] = yield 2;\n\
                 static [yield 3] = yield 4;\n\
             }\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);
    assert!(
        js.contains(
            "return _e = class {\n            constructor() {\n                this[_a] = await x;\n                this[_c] = yield 2;\n            }\n        },\n        _a = await x,\n        _b = await x,\n        _c = yield 1,\n        _d = yield 3,\n        _e[_b] = await x,\n        _e[_d] = yield 4,\n        _e;"
        ),
        "expected class expression computed key temps in source order: {js}"
    );
}

#[test]
fn test_named_standard_decorated_class_decl_with_constructor_preserves_field_lowering() {
    let js = emit_ts_with(
        "declare var dec: any;\n\
         export class C {\n\
             @dec x: any;\n\
             constructor(@dec x: any) {}\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("let C = (() => {"),
        "expected class declaration wrapper: {js}"
    );
    assert!(
        js.contains("let _x_decorators;"),
        "expected decorator slot temp: {js}"
    );
    assert!(
        js.contains("this.x = __runInitializers(this, _x_initializers, void 0);"),
        "expected constructor field initializer lowering: {js}"
    );
    assert!(
        js.contains("__runInitializers(this, _x_extraInitializers);"),
        "expected extra initializers call: {js}"
    );
    assert!(
        js.contains("__esDecorate(null, null, _x_decorators"),
        "expected field decorator application: {js}"
    );
    assert!(js.contains("exports.C = C;"), "expected CJS export: {js}");
}

#[test]
fn test_standard_decorated_class_downlevels_static_this_members_outside_class() {
    let js = emit_ts_with(
        "declare const dec: any;\n\
         @dec\n\
         class C {\n\
             static { this; }\n\
             static x: any = this;\n\
             static m() { this; }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(js.contains("var C = _classThis = class {"), "{js}");
    assert!(js.contains("__setFunctionName(_classThis, \"C\");"), "{js}");
    assert!(
        js.contains("(() => {\n        _classThis;\n    })();"),
        "{js}"
    );
    assert!(js.contains("_classThis.x = _classThis;"), "{js}");
    assert!(!js.contains("static { _classThis = this; }"), "{js}");
}

#[test]
fn test_cjs_import_helpers_private_standard_decorated_field_uses_native_wrapper() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         class C {\n\
             @dec #x: any;\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const tslib_1 = require(\"tslib\");"),
        "expected tslib require: {js}"
    );
    assert!(
        js.contains("let C = (() => {"),
        "expected native wrapper: {js}"
    );
    assert!(
        js.contains("tslib_1.__esDecorate(null, null, _private_x_decorators"),
        "expected private field decorator application: {js}"
    );
    assert!(
        js.contains("#x = tslib_1.__runInitializers(this, _private_x_initializers, void 0);"),
        "expected private field initializer lowering: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_private_standard_decorated_getter_uses_private_descriptor_key() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         class C {\n\
             @dec get #foo() { return 1; }\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "_private_get_foo_descriptor = { get: tslib_1.__setFunctionName(function () { return 1; }, \"#foo\", \"get\") }"
        ),
        "expected getter descriptor key to stay `get`: {js}"
    );
    assert!(
        js.contains("get #foo() { return _private_get_foo_descriptor.get.call(this); }"),
        "expected private getter bridge: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_private_standard_decorated_auto_accessor_uses_native_wrapper() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         class C {\n\
             @dec accessor #x: any;\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "_private_x_descriptor = { get: tslib_1.__setFunctionName(function () { return this.#x_accessor_storage; }, \"#x\", \"get\"), set: tslib_1.__setFunctionName(function (value) { this.#x_accessor_storage = value; }, \"#x\", \"set\") }"
        ),
        "expected private accessor descriptor lowering: {js}"
    );
    assert!(
        js.contains("set #x(value) { return _private_x_descriptor.set.call(this, value); }"),
        "expected private accessor setter bridge: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_static_computed_standard_decorated_field_uses_prop_key() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         declare var x: any;\n\
         class C {\n\
             @dec static [x]: any;\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("tslib_1.__propKey(x)"),
        "expected __propKey helper use for computed decorator name: {js}"
    );
    assert!(
        js.contains("static [(_static_member_decorators = [dec], _a = tslib_1.__propKey(x))]"),
        "expected inline computed-key decorator assignment: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_class_decorator_with_private_static_method_uses_native_wrapper() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         @dec class C {\n\
             static #foo() {}\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("static { tslib_1.__setFunctionName(this, \"C\"); }"),
        "expected class name helper for wrapped class decorator case: {js}"
    );
    assert!(
        js.contains("static { _C_foo = function _C_foo() { }; }"),
        "expected private static method extraction: {js}"
    );
    assert!(
        js.contains(
            "__esDecorate(null, _classDescriptor = { value: _classThis }, _classDecorators"
        ),
        "expected class decorator application: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_standard_decorated_class_expr_in_field_initializer_uses_field_name() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         class C {\n\
             D = @dec class {};\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const tslib_1 = require(\"tslib\");"),
        "expected tslib require for nested decorated class expression: {js}"
    );
    assert!(
        js.contains("D = (() => {"),
        "expected field initializer wrapper: {js}"
    );
    assert!(
        js.contains("tslib_1.__setFunctionName(_classThis, \"D\");"),
        "expected field name to flow into __setFunctionName: {js}"
    );
    assert!(
        !js.contains("tslib_1.__setFunctionName(_classThis, \"\");"),
        "field initializer should not use an empty binding name: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_standard_decorated_class_expr_mid_line_wrapper_keeps_indent() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         var C;\n\
         [C = @dec class {}] = [];\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("[C = (() => {\n        let _classDecorators = [dec];"),
        "expected continuation indent inside inline wrapper: {js}"
    );
    assert!(
        js.contains("        return class_1 = _classThis;\n    })()] = [];"),
        "expected closing wrapper indentation to match TypeScript: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_standard_decorated_class_expr_in_static_computed_field_uses_key_name() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         declare var f: any;\n\
         class C {\n\
             static [\"x\"] = @dec class {};\n\
             static [f()] = @dec class {};\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("static [\"x\"] = (() => {"),
        "expected literal computed field initializer wrapper: {js}"
    );
    assert!(
        js.contains("tslib_1.__setFunctionName(_classThis, \"x\");"),
        "expected literal computed field name in __setFunctionName: {js}"
    );
    assert!(
        js.contains("var _a;"),
        "expected temp var for dynamic computed field name: {js}"
    );
    assert!(
        js.contains("static [_a = tslib_1.__propKey(f())] = (() => {"),
        "expected __propKey temp capture for dynamic computed field name: {js}"
    );
    assert!(
        js.contains("tslib_1.__setFunctionName(_classThis, _a);"),
        "expected dynamic computed field temp in __setFunctionName: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_narrow_decorated_class_expr_in_static_computed_field_uses_key_name() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         declare var f: any;\n\
         class C {\n\
             static [\"x\"] = class { @dec y: any };\n\
             static [f()] = class { @dec y: any };\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("static [\"x\"] = (() => {"),
        "expected literal computed field narrow wrapper: {js}"
    );
    assert!(
        js.contains("static { tslib_1.__setFunctionName(this, \"x\"); }"),
        "expected literal computed field name in narrow wrapper: {js}"
    );
    assert!(
        js.contains("static [_a = tslib_1.__propKey(f())] = (() => {"),
        "expected __propKey temp capture for narrow-wrapper dynamic key: {js}"
    );
    assert!(
        js.contains("static { tslib_1.__setFunctionName(this, _a); }"),
        "expected dynamic computed field temp in narrow wrapper __setFunctionName: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_narrow_decorated_static_field_keeps_native_field_shape() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         class C {\n\
             @dec static x = @dec class {};\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("static x = tslib_1.__runInitializers(this, _static_x_initializers, (() => {"),
        "expected native static field initializer shape in narrow wrapper: {js}"
    );
    assert!(
        !js.contains("static { this.x = tslib_1.__runInitializers(this, _static_x_initializers"),
        "narrow wrapper should not fall back to static block assignment in native field mode: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_standard_decorated_class_expr_in_object_property_uses_key_name() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         declare var f: any;\n\
         ({ [\"x\"]: @dec class {}, [f()]: @dec class {}, __proto__: @dec class {} });\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("({ [\"x\"]: (() => {"),
        "expected literal computed object property wrapper: {js}"
    );
    assert!(
        js.contains("tslib_1.__setFunctionName(_classThis, \"x\");"),
        "expected literal computed object property name in __setFunctionName: {js}"
    );
    assert!(
        js.contains("var _a;"),
        "expected temp var for dynamic computed object property name: {js}"
    );
    assert!(
        js.contains("[_a = tslib_1.__propKey(f())]: (() => {"),
        "expected __propKey temp capture for dynamic computed object property name: {js}"
    );
    assert!(
        js.contains("tslib_1.__setFunctionName(_classThis, _a);"),
        "expected dynamic computed object property temp in __setFunctionName: {js}"
    );
    assert!(
        js.contains("__proto__: (() => {"),
        "expected non-computed __proto__ property emission: {js}"
    );
    assert!(
        js.contains("tslib_1.__setFunctionName(_classThis, \"\");"),
        "non-computed __proto__ should not perform named evaluation: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_narrow_decorated_class_expr_in_object_property_uses_key_name() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         declare var f: any;\n\
         ({ [\"x\"]: class { @dec y: any }, [f()]: class { @dec y: any } });\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("({ [\"x\"]: (() => {"),
        "expected literal computed object property narrow wrapper: {js}"
    );
    assert!(
        js.contains("static { tslib_1.__setFunctionName(this, \"x\"); }"),
        "expected literal computed object property name in narrow wrapper: {js}"
    );
    assert!(
        js.contains("[_a = tslib_1.__propKey(f())]: (() => {"),
        "expected __propKey temp capture for narrow-wrapper dynamic object key: {js}"
    );
    assert!(
        js.contains("static { tslib_1.__setFunctionName(this, _a); }"),
        "expected dynamic computed object property temp in narrow wrapper __setFunctionName: {js}"
    );
}

#[test]
fn test_cjs_import_helpers_narrow_decorated_class_expr_mid_line_wrapper_keeps_indent() {
    let js = emit_ts_file_with(
        "main.ts",
        "export {};\n\
         declare var dec: any;\n\
         declare var obj: any;\n\
         [x = class { @dec y: any }] = obj;\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("[x = (() => {\n        let _y_decorators;"),
        "expected continuation indent inside narrow inline wrapper: {js}"
    );
    assert!(
        js.contains("        };\n    })()] = obj;"),
        "expected narrow inline wrapper closing indentation to match TypeScript: {js}"
    );
}

#[test]
fn test_comma_separated_recovery_overloads_emit_first_stub_only() {
    let js = emit_ts_file_with(
        "test.ts",
        "function f1(), function f1();\n\
         function f2(), function f2() {}\n\
         function f3() {}, function f3();\n\
         class C {\n\
             m1(), m1();\n\
             m2(), m2() {}\n\
             m3() {}, m3();\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(false),
            ..Default::default()
        },
    );
    assert_eq!(
        js.match_indices("function f1() { }").count(),
        1,
        "expected only the first bodyless f1 recovery stub: {js}"
    );
    assert_eq!(
        js.match_indices("function f2() { }").count(),
        2,
        "expected bodyless + bodyful f2 declarations: {js}"
    );
    assert_eq!(
        js.match_indices("function f3() { }").count(),
        1,
        "expected trailing bodyless f3 recovery overload to stay erased: {js}"
    );
    assert_eq!(
        js.match_indices("    m1() { }").count(),
        1,
        "expected only the first bodyless m1 recovery stub: {js}"
    );
    assert_eq!(
        js.match_indices("    m2() { }").count(),
        2,
        "expected bodyless + bodyful m2 methods: {js}"
    );
    assert_eq!(
        js.match_indices("    m3() { }").count(),
        1,
        "expected trailing bodyless m3 recovery overload to stay erased: {js}"
    );
}

#[test]
fn test_strip_function_return_type() {
    let js = emit_ts("function greet(name: string): string { return name; }");
    assert!(
        !js.contains(": string"),
        "return type annotation should be stripped: {js}"
    );
    assert!(
        js.contains("function greet(name)"),
        "function should have param without type: {js}"
    );
}

#[test]
fn test_strip_type_parameters() {
    let js = emit_ts("function identity<T>(x: T): T { return x; }");
    assert!(
        !js.contains("<T>"),
        "type parameters should be stripped: {js}"
    );
    assert!(
        js.contains("function identity(x)"),
        "function should have param without type: {js}"
    );
}

#[test]
fn test_strip_as_expression() {
    let js = emit_ts("const x = (foo as any);");
    assert!(
        !js.contains(" as "),
        "'as' expression should be stripped: {js}"
    );
    assert!(js.contains("foo"), "expression should be preserved: {js}");
}

#[test]
fn test_strip_type_assertion() {
    let js = emit_ts("const x = (<string>foo);");
    assert!(
        !js.contains("<string>"),
        "type assertion should be stripped: {js}"
    );
}

#[test]
fn test_for_of_object_default_continuation_keeps_header_indent() {
    let js = emit_ts(concat!(
        "for ({ skills: { primary: primaryA = \"primary\", secondary: secondaryA = \"secondary\" } =\n",
        "    { primary: \"nosKill\", secondary: \"noSkill\" } } of multiRobots) {\n",
        "    console.log(primaryA);\n",
        "}"
    ));
    assert!(
        js.contains(
            "for ({ skills: { primary: primaryA = \"primary\", secondary: secondaryA = \"secondary\" } =\n        { primary: \"nosKill\", secondary: \"noSkill\" } } of multiRobots) {"
        ),
        "for-of object default continuation should keep the extra loop-header indent: {js}"
    );
}

#[test]
fn test_for_of_multiline_pattern_with_type_assertion_rhs_preserves_header_wrap() {
    let js = emit_ts(concat!(
        "for ({ skills: { primary: primaryA = \"primary\", secondary: secondaryA = \"secondary\" } =\n",
        "    { primary: \"nosKill\", secondary: \"noSkill\" } } of\n",
        "    <MultiRobot[]>[{ name: \"mower\", skills: { primary: \"mowing\", secondary: \"none\" } },\n",
        "        { name: \"trimmer\", skills: { primary: \"trimming\", secondary: \"edging\" } }]) {\n",
        "    console.log(primaryA);\n",
        "}"
    ));
    assert!(
        !js.contains("<MultiRobot[]>"),
        "type assertion should still be stripped from the for-of rhs: {js}"
    );
    assert!(
        js.contains(
            "for ({ skills: { primary: primaryA = \"primary\", secondary: secondaryA = \"secondary\" } =\n        { primary: \"nosKill\", secondary: \"noSkill\" } } of [{ name: \"mower\", skills: { primary: \"mowing\", secondary: \"none\" } },\n    { name: \"trimmer\", skills: { primary: \"trimming\", secondary: \"edging\" } }]) {"
        ),
        "multiline for-of patterns should keep the wrapped header when stripping the rhs type assertion: {js}"
    );
}

#[test]
fn test_try_statement_internal_comments_are_preserved() {
    let js = emit_ts(concat!(
        "/*1*/ try /*2*/ { /*3*/\n",
        "    /*4*/ throw /*5*/ \"no\" /*6*/;\n",
        "/*7*/} /*8*/ catch /*9*/ ( /*10*/ e /*11*/ ) /*12*/ { /*13*/\n",
        "\n",
        "/*14*/} /*15*/ finally /*16*/ { /*17*/\n",
        "\n",
        "/*18*/} /*19*/\n"
    ));
    assert!(
        js.contains(
            "/*1*/ try /*2*/ { /*3*/\n    /*4*/ throw /*5*/ \"no\" /*6*/;\n    /*7*/ } /*8*/\ncatch /*9*/ ( /*10*/e /*11*/) /*12*/ { /*13*/\n    /*14*/ } /*15*/\nfinally /*16*/ { /*17*/\n    /*18*/ } /*19*/"
        ),
        "try/catch/finally internal comments should stay attached to the clause headers and braces: {js}"
    );
}

#[test]
fn test_recover_missing_type_assertion_gt_keeps_inner_expression() {
    let js =
        emit_ts("let a = (<unknown function foo() {});\nlet b = <unknown 123;\nlet c = <unknown");
    assert!(
        js.contains("let a = function foo() { };"),
        "missing `>` type assertion before function should preserve function expression: {js}"
    );
    assert!(
        js.contains("let b = 123;"),
        "missing `>` type assertion before literal should preserve literal: {js}"
    );
    assert!(
        js.contains("let c = ;"),
        "missing `>` type assertion without expression should recover to omitted initializer: {js}"
    );
}

#[test]
fn test_recover_jsdoc_style_instantiation_type_args() {
    let js = emit_ts(
        "const a = foo<?>;\nconst b = foo<string?>;\nconst c = foo<?string>;\nconst d = foo<?string?>;",
    );
    assert!(
        js.contains("const a = foo<?>;"),
        "lone JSDoc `?` type arg should stay attached to the expression: {js}"
    );
    assert!(
        js.contains("const b = foo<?string>;"),
        "postfix `string?` should normalize to JSDoc nullable emit: {js}"
    );
    assert!(
        js.contains("const c = foo<?string>;"),
        "prefix `?string` should stay attached to the expression: {js}"
    );
    assert!(
        js.contains("const d = foo<??string>;"),
        "nested nullable recovery should preserve both question marks: {js}"
    );
}

#[test]
fn test_recover_missing_commas_inside_call_arguments() {
    let source =
        "class C {\n    public bar() {\n        var v = foo(\n            public blaz() {}\n            );\n    }\n}";
    let file = tsc_rs_parser::parse("test.ts", source);
    let StmtKind::ClassDecl(class_decl) = &file.statements[0].kind else {
        panic!("expected class declaration");
    };
    let ClassMemberKind::Method(method) = &class_decl.members[0].kind else {
        panic!("expected class method");
    };
    let body = method.body.as_ref().expect("expected method body");
    let StmtKind::Var(var_stmt) = &body[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::Call(call) = &init.kind else {
        panic!("expected call initializer, got {:?}", init.kind);
    };
    assert_eq!(call.args.len(), 3, "expected recovered three-argument call");
    let opts = CompilerOptions::default();
    let js = emit(&file, &opts).javascript;
    assert!(
        js.contains("var v = foo(public, blaz(), {});"),
        "missing-comma argument recovery should emit normalized call args: {js}"
    );
}

#[test]
fn test_recover_invalid_arrow_block_inside_call_arguments() {
    let js = emit_ts("foo((1)=>{return 0;});");
    assert!(
        js.contains("foo((1), { return: 0 });"),
        "invalid arrow block inside call arguments should recover to object literal argument: {js}"
    );
}

#[test]
fn test_recover_typed_arrow_tail_in_variable_initializer() {
    let js = emit_ts("var y = x:number => x*x;");
    assert!(
        js.contains("var y = x, number;\nx * x;"),
        "typed arrow tail in variable initializer should recover to declarator split plus expression tail: {js}"
    );
}

#[test]
fn test_recover_empty_block_malformed_arrow_type_initializer() {
    let js = emit_ts_with(
        "var v = (a): => {\n   \n};\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var v = (a);\n{\n}\n;"),
        "empty-block malformed arrow type recovery should split into declarator, block, and semicolon: {js}"
    );
}

#[test]
fn test_recover_type_argument_list_trailing_backslash_elides_before_semi() {
    let js = emit_ts_with(
        "Foo<A,B,\\ C>(4, 5, 6);\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("Foo < A, B, ;\nC > (4, 5, 6);"),
        "type-argument recovery should drop the trailing backslash before the split semicolon: {js}"
    );
}

#[test]
fn test_recover_generator_param_yield_arrow_as_split_param_list() {
    let js = emit_ts_with(
        "function * foo(a = yield => yield) {\n}\n",
        CompilerOptions {
            target: ScriptTarget::parse("es6"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function* foo(a = yield, yield) {"),
        "generator parameter recovery should split `yield => yield` into a recovered parameter tail: {js}"
    );
}

#[test]
fn test_recover_invalid_let_array_decl_as_split_expr_fragments() {
    let js = emit_ts_with(
        "var let: any;\nlet[0] = 100;\n",
        CompilerOptions {
            target: ScriptTarget::parse("es6"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var let;\nlet [];\n0;\n100;"),
        "invalid `let[` declaration recovery should split into `let [];` plus fragment statements: {js}"
    );
}

#[test]
fn test_recover_module_in_expr_from_malformed_namespace_parse() {
    let js = emit_ts_with(
        "let module = 10;\nmodule in {}\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            ..Default::default()
        },
    );
    assert_eq!(
        js.trim(),
        "\"use strict\";\nlet module = 10;\nmodule in {};"
    );
}

#[test]
fn test_invalid_string_local_import_elides_without_from_tail() {
    let js = emit_ts_with(
        "import { foo as \"invalid 2\" } from \"./values-valid\";\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2022),
            target: ScriptTarget::parse("es2022"),
            ..Default::default()
        },
    );
    assert_eq!(js.trim(), "export {};");
}

#[test]
fn test_invalid_type_clause_string_local_import_elides_same_line_tail() {
    let js = emit_ts_with(
        "import { type as as \"invalid 4\" } from \"./type-clause-valid\";\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2022),
            target: ScriptTarget::parse("es2022"),
            ..Default::default()
        },
    );
    assert_eq!(js.trim(), "export {};");
}

#[test]
fn test_reexport_from_source_ignores_unrelated_global_type_only_name_collision() {
    let file = tsc_rs_parser::parse(
        "test.ts",
        "export { foo as \"valid 3\" } from \"./values-valid\";\n",
    );
    let js = emit_with_global_type_only(
        &file,
        &CompilerOptions {
            module: Some(ModuleKind::ES2022),
            target: ScriptTarget::parse("es2022"),
            ..Default::default()
        },
        &Default::default(),
        &HashSet::from([AstString::from("foo")]),
        &HashSet::new(),
    )
    .javascript;
    assert_eq!(
        js.trim(),
        "export { foo as \"valid 3\" } from \"./values-valid\";"
    );
}

#[test]
fn test_recover_async_await_arrow_initializer_var_split_across_targets() {
    let source = "var foo = async (a = await => await): Promise<void> => {\n}\n";
    for target in ["es2017", "es2015", "es5"] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: ScriptTarget::parse(target),
                no_emit_helpers: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(
            js.trim(),
            "\"use strict\";\nvar foo = async(a = await => await), Promise;\n;\n{\n}"
        );
    }
}

#[test]
fn test_if_with_leading_jsdoc_cast_comment_keeps_space_after_open_paren() {
    let js = emit_ts_file_with(
        "assertionTypePredicates2.js",
        "/**\n * @typedef {{ x: number }} A\n */\n/**\n * @typedef { A & { y: number } } B\n */\n/**\n * @param {A} a\n * @returns { asserts a is B }\n */\nconst foo = (a) => {\n    if (/** @type { B } */ (a).y !== 0)\n        throw TypeError();\n    return undefined;\n};\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("if ( /** @type { B } */(a).y !== 0)\n        throw TypeError();"),
        "leading jsdoc cast comments in structured if heads should keep a space after `(` and no trailing head space: {js}"
    );
}

#[test]
fn test_async_generator_parameter_evaluation_downlevel_keeps_inner_params() {
    let js = emit_ts_with(
        "async function* f1(x, y = z) {}\n\
         async function* f2({[z]: x}) {}\n\
         declare class Super { foo(): void; }\n\
         class Sub extends Super {\n\
             async * m(x, y = z, { ...w }) { super.foo(); }\n\
         }\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function f1(x_1) { return __asyncGenerator(this, arguments, function* f1_1(x, y = z) { }); }"),
        "async generator default-parameter lift should preserve inner params: {js}"
    );
    assert!(
        js.contains("function f2(_a) { return __asyncGenerator(this, arguments, function* f2_1({ [z]: x }) { }); }"),
        "async generator destructured params should move into the inner generator: {js}"
    );
    assert!(
        js.contains("m(x_1) { const _super = Object.create(null, {")
            && js.contains(
                "}); return __asyncGenerator(this, arguments, function* m_1(x, y = z, _a) { var w = __rest(_a, []); _super.foo.call(this); }); }"
            ),
        "async generator methods should keep lifted params and inner rest destructuring: {js}"
    );
}

#[test]
fn test_async_awaiter_var_shadowing_hoists_and_rewrites_conflicting_vars() {
    let js = emit_ts_with(
        "declare const y: any;\n\
         async function fn4(x) { var x = y; }\n\
         async function fn11(x) { var { ...x } = y; }\n\
         async function fn31(x) { for (var { x } = y;;) {} }\n\
         async function fn40(x) { try {} catch { var x; } }\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function fn4(x) {") && js.contains("function* () { var x; x = y; }"),
        "simple conflicting var initializers should hoist then rewrite to assignment: {js}"
    );
    assert!(
        js.contains("function fn11(x) {")
            && js.contains("function* () { var x; x = __rest(y, []); }"),
        "rest-only conflicting object vars should rewrite to __rest assignment without extra parens: {js}"
    );
    assert!(
        js.contains("function fn31(x) {")
            && js.contains("function* () { var x; for ({ x } = y;;) { } }"),
        "conflicting for-init vars should hoist and drop the inner `var` keyword: {js}"
    );
    assert!(
        js.contains("function fn40(x) {")
            && js.contains("function* () { var x; try {")
            && js.contains("catch (_a) {"),
        "catch clauses without bindings should still downlevel to synthetic catch params in awaiter bodies: {js}"
    );
}

#[test]
fn test_async_arrow_super_uses_enclosing_method_hoists() {
    let js = emit_ts_with(
        "class Base {\n\
             set setter(x: any) {}\n\
             get getter(): any { return; }\n\
             method(x: string): any {}\n\
         }\n\
         class Derived extends Base {\n\
             a() { return async () => super.method('') }\n\
             d() { return async () => super[\"method\"]('') }\n\
             f() { return async () => super[\"setter\"] = '' }\n\
         }\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "a() { const _super = Object.create(null, {\n        method: { get: () => super.method }\n    }); return () => __awaiter(this, void 0, void 0, function* () { return _super.method.call(this, ''); }); }"
        ),
        "property-access async arrows should hoist `_super` in the enclosing method and reuse it inside __awaiter: {js}"
    );
    assert!(
        js.contains(
            "d() {\n        const _superIndex = name => super[name];\n        return () => __awaiter(this, void 0, void 0, function* () { return _superIndex(\"method\").call(this, ''); });\n    }"
        ),
        "element-access async arrows should force a multiline enclosing method body with `_superIndex`: {js}"
    );
    assert!(
        js.contains(
            "f() {\n        const _superIndex = (function (geti, seti) {\n            const cache = Object.create(null);\n            return name => cache[name] || (cache[name] = { get value() { return geti(name); }, set value(v) { seti(name, v); } });\n        })(name => super[name], (name, value) => super[name] = value);\n        return () => __awaiter(this, void 0, void 0, function* () { return _superIndex(\"setter\").value = ''; });\n    }"
        ),
        "writeable element-access async arrows should hoist the setter-aware `_superIndex` helper in the enclosing method: {js}"
    );
}

#[test]
fn test_invalid_accessor_recovery_preserves_native_esnext_shapes() {
    let js = emit_ts_with(
        "abstract class C1 {\n\
             accessor accessor a: any;\n\
             accessor static h: any;\n\
             accessor i() {}\n\
             accessor get j() { return false; }\n\
             accessor set k(v: any) {}\n\
             accessor constructor() {}\n\
         }\n\
         accessor class C3 {}\n\
         accessor enum E1 {}\n\
         accessor var V1: any;\n\
         accessor function F1() {}\n\
         accessor import \"x\";\n\
         accessor import {} from \"x\";\n\
         accessor export { V1 };\n\
         accessor export default V1;\n",
        CompilerOptions {
            target: ScriptTarget::parse("esnext"),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "class C1 {\n    accessor accessor a;\n    accessor static h;\n    accessor i() { }\n    accessor get j() { return false; }\n    accessor set k(v) { }\n    constructor() { }\n}"
        ),
        "native auto-accessor targets should preserve invalid leading accessor tokens on recovered members: {js}"
    );
    assert!(
        js.contains("accessor class C3 {\n}"),
        "esnext class declarations should consume a leading recovery accessor prefix: {js}"
    );
    assert!(
        js.contains("accessor var E1;\n(function (E1) {\n})(E1 || (E1 = {}));"),
        "recovered enum declarations should keep the accessor prefix on the emitted var line: {js}"
    );
    assert!(
        js.contains("accessor var V1;")
            && js.contains("accessor function F1() { }")
            && js.contains("accessor import \"x\";"),
        "recovered var/function/import statements should keep the accessor prefix: {js}"
    );
    assert!(
        !js.contains("accessor import {} from \"x\";")
            && js.contains("export { V1 };")
            && js.contains("export default V1;")
            && !js.contains("accessor export { V1 };")
            && !js.contains("accessor export default V1;"),
        "elided imports and export statements should not keep a dangling accessor prefix: {js}"
    );
}

#[test]
fn test_invalid_accessor_recovery_drops_native_prefixes_when_downleveling() {
    let js = emit_ts_with(
        "abstract class C1 {\n\
             accessor accessor a: any;\n\
             accessor static h: any;\n\
             accessor i() {}\n\
             accessor get j() { return false; }\n\
             accessor set k(v: any) {}\n\
         }\n\
         accessor class C3 {}\n\
         accessor enum E1 {}\n\
         accessor var V1: any;\n\
         accessor function F1() {}\n\
         accessor import \"x\";\n",
        CompilerOptions {
            target: ScriptTarget::parse("es2017"),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("class C3 {\n}")
            && !js.contains("accessor class C3 {\n}"),
        "downlevel auto-accessor targets should drop the stray accessor prefix before classes: {js}"
    );
    assert!(
        !js.contains("accessor i() { }")
            && !js.contains("accessor get j() { return false; }")
            && !js.contains("accessor set k(v) { }"),
        "downlevel auto-accessor targets should not preserve invalid member accessor prefixes: {js}"
    );
    assert!(
        js.contains("accessor var E1;")
            && js.contains("accessor var V1;")
            && js.contains("accessor function F1() { }")
            && js.contains("accessor import \"x\";"),
        "statement-level accessor recovery should remain intact when class members are downleveled: {js}"
    );
}

#[test]
fn test_import_type_with_unparenthesized_generic_function_type_arg_is_erased() {
    let js = emit_ts("export declare const fail1: import(\"module\").Modifier<<T>(x: T) => T>;");
    assert_eq!(
        js.trim(),
        "export {};",
        "generic function type args inside import types should stay erased in JS emit: {js}"
    );
}

#[test]
fn test_new_expression_with_type_assertion_callee_recovery() {
    let js = emit_ts("var obj9: i1 = new <i1> anyVar;");
    assert!(
        js.contains("var obj9 = new  < i1 > anyVar;"),
        "new expression with type-assertion callee should preserve recovery form: {js}"
    );
}

#[test]
fn test_invalid_let_for_of_recovery_emits_classic_for_fallback() {
    let js = emit_ts("for (let of [1,2,3]) {}");
    assert!(
        js.contains("for (let of, []; 1, 2, 3; )\n    ;\n{ }"),
        "invalid `for (let of ...)` should recover to TypeScript's classic-for fallback: {js}"
    );
}

#[test]
fn test_invalid_let_for_in_recovery_emits_malformed_for_in_fallback() {
    let js = emit_ts("for (let in [1,2,3]) {}");
    assert!(
        js.contains("for (let  in [1, 2, 3]) { }"),
        "invalid `for (let in ...)` should recover to TypeScript's malformed for-in fallback: {js}"
    );
}

#[test]
fn test_invalid_let_for_type_annotation_recovery_emits_object_pattern_fallback() {
    let js = emit_ts_with(
        "for (let x: y) {\n    z(x);\n}\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("for (let x, { z }; (x); )\n    ;"),
        "invalid `for (let x: y)` should recover to TypeScript's object-pattern fallback: {js}"
    );
}

#[test]
fn test_recover_declare_module_without_name() {
    let js = emit_ts(
        "declare module {\n    export class XDate {\n        public getDay(): number;\n    }\n}\nvar d = new XDate();",
    );
    assert!(
        js.contains("declare;\nmodule;"),
        "recovered declare/module tokens should be preserved: {js}"
    );
    assert!(
        js.contains("export class XDate {"),
        "recovered module body should keep the export class wrapper: {js}"
    );
    assert!(
        js.contains("var d = new XDate();"),
        "statements after the recovered extern module should still emit: {js}"
    );
}

#[test]
fn test_recover_duplicate_extends_with_type_args_keeps_body() {
    let js = emit_ts("class C<T> {}\nclass D<T> extends C<number> extends C<string> { baz() { } }");
    assert!(
        js.contains("class D extends C extends C {"),
        "duplicate extends recovery should preserve both heritage clauses without type args: {js}"
    );
    assert!(
        js.contains("baz() { }"),
        "duplicate extends recovery should keep the real class body: {js}"
    );
}

#[test]
fn test_recover_extends_void_emits_void_tail() {
    let js = emit_ts("class C extends void {}");
    assert!(
        js.contains("class C extends  {"),
        "extends-void recovery should keep an empty heritage slot: {js}"
    );
    assert!(
        js.contains("void {};"),
        "extends-void recovery should emit the trailing void expression: {js}"
    );
}

#[test]
fn test_strip_satisfies_expression() {
    let js = emit_ts("const x = (val satisfies Record<string, string>);");
    assert!(
        !js.contains("satisfies"),
        "'satisfies' expression should be stripped: {js}"
    );
    assert!(js.contains("val"), "expression should be preserved: {js}");
}

#[test]
fn test_strip_non_null_assertion() {
    let js = emit_ts("const x = (foo!);");
    assert!(js.contains("foo"), "expression should be preserved: {js}");
}

#[test]
fn test_strip_declare_variable() {
    let js = emit_ts("declare const x: number;");
    assert!(
        !js.contains("declare"),
        "declare statement should be stripped: {js}"
    );
    assert!(
        !js.contains("const x"),
        "declared variable should not appear: {js}"
    );
}

#[test]
fn test_strip_declare_function() {
    let js = emit_ts("declare function foo(x: number): void;");
    assert!(
        !js.contains("declare"),
        "declare function should be stripped: {js}"
    );
    assert!(
        !js.contains("function foo"),
        "declared function should not appear: {js}"
    );
}

#[test]
fn test_only_types_produces_minimal_output() {
    let js = emit_ts("interface A { x: number; }\ntype B = string;\ndeclare const c: boolean;");
    let trimmed = js.trim();
    assert_eq!(
        trimmed, "\"use strict\";",
        "only-type file should produce just use strict: {js}"
    );
}

// ---------------------------------------------------------------
// Enum emission tests
// ---------------------------------------------------------------

#[test]
fn test_numeric_enum() {
    let js = emit_ts("enum Color { Red, Green, Blue }");
    assert!(js.contains("var Color;"), "should declare var: {js}");
    assert!(js.contains("(function (Color)"), "should have IIFE: {js}");
    assert!(
        js.contains("Color[Color[\"Red\"] = 0] = \"Red\""),
        "should have reverse mapping for Red: {js}"
    );
    assert!(
        js.contains("Color[Color[\"Green\"] = 1] = \"Green\""),
        "should have reverse mapping for Green: {js}"
    );
    assert!(
        js.contains("Color[Color[\"Blue\"] = 2] = \"Blue\""),
        "should have reverse mapping for Blue: {js}"
    );
}

#[test]
fn test_numeric_enum_with_initializer() {
    let js = emit_ts("enum Status { Active = 1, Inactive = 0 }");
    assert!(
        js.contains("Status[Status[\"Active\"] = 1] = \"Active\""),
        "should use initializer value: {js}"
    );
    assert!(
        js.contains("Status[Status[\"Inactive\"] = 0] = \"Inactive\""),
        "should use initializer value: {js}"
    );
}

#[test]
fn test_emit_deep_binary_expression_without_stack_overflow() {
    // Run in a thread with a larger stack to avoid debug-build stack overflow
    let result = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let terms = 20_000usize;
            let mut source = String::from("// deep binary stress\n");
            for i in 0..terms {
                if i > 0 {
                    source.push_str(" + ");
                }
                source.push_str(&i.to_string());
            }
            source.push_str(";\n");

            let file = Box::new(tsc_rs_parser::parse("file.js", &source));
            let leaked_file = Box::leak(file);
            let js = emit(leaked_file, &CompilerOptions::default()).javascript;

            assert!(
                js.starts_with("\"use strict\";\n// deep binary stress\n0 + 1 + 2"),
                "expected preserved leading comment and expression prefix"
            );
            assert!(
                js.ends_with(&format!(" + {};\n", terms - 1)),
                "expected preserved expression tail"
            );
        })
        .unwrap()
        .join();
    result.unwrap();
}

#[test]
fn test_string_enum() {
    let js = emit_ts(
        "enum Direction { Up = \"UP\", Down = \"DOWN\", Left = \"LEFT\", Right = \"RIGHT\" }",
    );
    assert!(js.contains("var Direction;"), "should declare var: {js}");
    // String enums should NOT have reverse mapping
    assert!(
        js.contains("Direction[\"Up\"] = \"UP\""),
        "should assign string value without reverse mapping: {js}"
    );
    assert!(
        !js.contains("Direction[Direction["),
        "should NOT have reverse mapping for string enum: {js}"
    );
}

#[test]
fn test_const_enum_elided() {
    let js = emit_ts("const enum Flags { A, B, C }");
    assert!(
        !js.contains("var Flags"),
        "const enum should be elided by default: {js}"
    );
}

#[test]
fn test_const_enum_preserved() {
    let opts = CompilerOptions {
        preserve_const_enums: Some(true),
        ..Default::default()
    };
    let js = emit_ts_with("const enum Flags { A, B, C }", opts);
    assert!(
        js.contains("var Flags"),
        "const enum should be preserved when preserveConstEnums is true: {js}"
    );
}

#[test]
fn test_cjs_named_export_of_imported_const_enum_namespace_merge_keeps_predecl_stub() {
    let js = emit_ts_with_const_enum_values(
        "import { Enum } from './enum';\nnamespace Enum { export type Foo = number; }\nexport { Enum };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
        &HashMap::from([(
            ("Enum".to_string(), "One".to_string()),
            ConstEnumValue::Number(1.0),
        )]),
    );
    assert_eq!(
        js,
        "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\nexports.Enum = void 0;\n"
    );
}

#[test]
fn test_cjs_runtime_import_export_survives_same_name_type_alias() {
    let js = emit_ts_with(
        "import { A } from './a';\ntype A = 0;\nexport { A };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            other: vec![(
                "__tsrsResolvedRuntimeExportImportLocals".to_string(),
                "A".to_string(),
            )],
            ..Default::default()
        },
    );
    assert!(js.contains("exports.A = void 0;"), "{js}");
    assert!(
        js.contains("Object.defineProperty(exports, \"A\", { enumerable: true, get: function () { return a_1.A; } });"),
        "{js}"
    );
}

#[test]
fn test_cjs_named_import_merged_with_namespace_is_elided() {
    let js = emit_ts_with(
        "import { A } from './a';\nnamespace A { export const displayName = 'A'; }\nA.displayName;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(!js.contains("require(\"./a\")"), "{js}");
    assert!(js.contains("A.displayName;"), "{js}");
    assert!(!js.contains("a_1.A"), "{js}");
}

#[test]
fn test_cjs_imported_const_enum_member_access_still_elides_plain_import() {
    let js = emit_ts_with_const_enum_values(
        "import { Enum } from './enum';\nEnum.One;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
        &HashMap::from([(
            ("Enum".to_string(), "One".to_string()),
            ConstEnumValue::Number(1.0),
        )]),
    );
    assert_eq!(
        js,
        "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\n1 /* Enum.One */;\n"
    );
}

#[test]
fn test_js_source_preserves_unused_runtime_imports() {
    let js = emit_ts_file_with(
        "file.js",
        "import { unused } from './dep.js';\nimport mod = require('./required.js');\nexport const value = 1;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("require(\"./dep.js\")"),
        "unused JavaScript named imports still have runtime side effects: {js}"
    );
    assert!(
        js.contains("const mod = require(\"./required.js\")"),
        "unused JavaScript import-equals declarations must be preserved: {js}"
    );
}

#[test]
fn test_cjs_imported_const_enum_from_runtime_export_source_keeps_require() {
    let js = emit_ts_with_const_enum_values(
        "import { Enum } from './merge';\nEnum.One;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            other: vec![(
                "__tsrsResolvedRuntimeExportImportLocals".to_string(),
                "Enum".to_string(),
            )],
            ..Default::default()
        },
        &HashMap::from([(
            ("Enum".to_string(), "One".to_string()),
            ConstEnumValue::Number(1.0),
        )]),
    );
    assert_eq!(
        js,
        "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\nconst merge_1 = require(\"./merge\");\n1 /* Enum.One */;\n"
    );
}

#[test]
fn test_declare_enum_elided() {
    let js = emit_ts("declare enum Ext { A, B }");
    assert!(
        !js.contains("var Ext"),
        "declare enum should be elided: {js}"
    );
}

#[test]
fn test_mixed_string_numeric_enum_members() {
    let js = emit_ts("enum Mixed { A = 0, B = \"bee\" }");
    assert!(
        js.contains("Mixed[Mixed[\"A\"] = 0] = \"A\""),
        "numeric member should have reverse mapping: {js}"
    );
    assert!(
        js.contains("Mixed[\"B\"] = \"bee\""),
        "string member should not have reverse mapping: {js}"
    );
}

// ---------------------------------------------------------------
// Enum auto-numbering tests
// ---------------------------------------------------------------

#[test]
fn test_enum_auto_numbering_continues_from_initializer() {
    let js = emit_ts("enum E { A = 1, B, C }");
    assert!(
        js.contains("E[E[\"A\"] = 1] = \"A\""),
        "A should be 1: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 2] = \"B\""),
        "B should auto-number to 2: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 3] = \"C\""),
        "C should auto-number to 3: {js}"
    );
}

#[test]
fn test_enum_auto_numbering_from_large_initializer() {
    let js = emit_ts("enum E { X = 100, Y, Z }");
    assert!(
        js.contains("E[E[\"X\"] = 100] = \"X\""),
        "X should be 100: {js}"
    );
    assert!(
        js.contains("E[E[\"Y\"] = 101] = \"Y\""),
        "Y should auto-number to 101: {js}"
    );
    assert!(
        js.contains("E[E[\"Z\"] = 102] = \"Z\""),
        "Z should auto-number to 102: {js}"
    );
}

#[test]
fn test_enum_multiple_initializers() {
    let js = emit_ts("enum E { A = 0, B, C = 10, D, E = 20, F }");
    assert!(
        js.contains("E[E[\"A\"] = 0] = \"A\""),
        "A should be 0: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 1] = \"B\""),
        "B should auto-number to 1: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 10] = \"C\""),
        "C should be 10: {js}"
    );
    assert!(
        js.contains("E[E[\"D\"] = 11] = \"D\""),
        "D should auto-number to 11: {js}"
    );
    assert!(
        js.contains("E[E[\"E\"] = 20] = \"E\""),
        "E should be 20: {js}"
    );
    assert!(
        js.contains("E[E[\"F\"] = 21] = \"F\""),
        "F should auto-number to 21: {js}"
    );
}

#[test]
fn test_enum_negative_initializer() {
    let js = emit_ts("enum E { A = -1, B, C }");
    assert!(
        js.contains("E[E[\"A\"] = -1] = \"A\""),
        "A should be -1: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 0] = \"B\""),
        "B should auto-number to 0: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 1] = \"C\""),
        "C should auto-number to 1: {js}"
    );
}

#[test]
fn test_enum_zero_initializer_followed_by_auto() {
    let js = emit_ts("enum E { A = 0, B, C, D }");
    assert!(
        js.contains("E[E[\"A\"] = 0] = \"A\""),
        "A should be 0: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 1] = \"B\""),
        "B should be 1: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 2] = \"C\""),
        "C should be 2: {js}"
    );
    assert!(
        js.contains("E[E[\"D\"] = 3] = \"D\""),
        "D should be 3: {js}"
    );
}

#[test]
fn test_enum_string_followed_by_numeric() {
    // After a string member, subsequent members must have explicit initializers
    // in valid TS, but we test that if they do have initializers, auto-numbering resumes
    let js = emit_ts("enum E { A = \"alpha\", B = 1, C }");
    assert!(
        js.contains("E[\"A\"] = \"alpha\""),
        "A should be string value: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 1] = \"B\""),
        "B should be 1: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 2] = \"C\""),
        "C should auto-number to 2: {js}"
    );
}

#[test]
fn test_enum_all_auto_numbered() {
    let js = emit_ts("enum E { A, B, C, D, E }");
    assert!(
        js.contains("E[E[\"A\"] = 0] = \"A\""),
        "A should be 0: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 1] = \"B\""),
        "B should be 1: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 2] = \"C\""),
        "C should be 2: {js}"
    );
    assert!(
        js.contains("E[E[\"D\"] = 3] = \"D\""),
        "D should be 3: {js}"
    );
    assert!(
        js.contains("E[E[\"E\"] = 4] = \"E\""),
        "E should be 4: {js}"
    );
}

#[test]
fn test_enum_single_member() {
    let js = emit_ts("enum E { Only }");
    assert!(
        js.contains("E[E[\"Only\"] = 0] = \"Only\""),
        "Only should be 0: {js}"
    );
}

#[test]
fn test_enum_single_member_with_initializer() {
    let js = emit_ts("enum E { Only = 42 }");
    assert!(
        js.contains("E[E[\"Only\"] = 42] = \"Only\""),
        "Only should be 42: {js}"
    );
}

#[test]
fn test_const_enum_preserved_auto_numbering() {
    let opts = CompilerOptions {
        preserve_const_enums: Some(true),
        ..Default::default()
    };
    let js = emit_ts_with("const enum E { A = 5, B, C }", opts);
    assert!(
        js.contains("E[E[\"A\"] = 5] = \"A\""),
        "A should be 5: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 6] = \"B\""),
        "B should auto-number to 6: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 7] = \"C\""),
        "C should auto-number to 7: {js}"
    );
}

#[test]
fn test_enum_all_string_members() {
    let js = emit_ts("enum Dir { Up = \"UP\", Down = \"DOWN\" }");
    assert!(
        js.contains("Dir[\"Up\"] = \"UP\""),
        "should have string member Up: {js}"
    );
    assert!(
        js.contains("Dir[\"Down\"] = \"DOWN\""),
        "should have string member Down: {js}"
    );
    assert!(
        !js.contains("Dir[Dir["),
        "should NOT have reverse mapping for string enum: {js}"
    );
}

#[test]
fn test_enum_gap_in_numbering() {
    // A=0, B=1, C=5, D=6
    let js = emit_ts("enum E { A, B, C = 5, D }");
    assert!(
        js.contains("E[E[\"A\"] = 0] = \"A\""),
        "A should be 0: {js}"
    );
    assert!(
        js.contains("E[E[\"B\"] = 1] = \"B\""),
        "B should be 1: {js}"
    );
    assert!(
        js.contains("E[E[\"C\"] = 5] = \"C\""),
        "C should be 5: {js}"
    );
    assert!(
        js.contains("E[E[\"D\"] = 6] = \"D\""),
        "D should auto-number to 6: {js}"
    );
}

// ---------------------------------------------------------------
// Class emission tests
// ---------------------------------------------------------------

#[test]
fn test_class_basic() {
    let js = emit_ts("class Animal { speak() { return \"woof\"; } }");
    assert!(js.contains("class Animal"), "should emit class: {js}");
    assert!(js.contains("speak()"), "should emit method: {js}");
}

#[test]
fn test_class_extends() {
    let js = emit_ts("class Dog extends Animal { bark() { } }");
    assert!(
        js.contains("class Dog extends Animal"),
        "should emit extends: {js}"
    );
}

#[test]
fn test_recover_class_with_keyword_name_emits_tail_statement() {
    let js = emit_ts("class void {}");
    assert!(
        js.contains("class {"),
        "class should still be emitted: {js}"
    );
    assert!(
        js.contains("void {};"),
        "keyword class name recovery tail should be emitted: {js}"
    );
}

#[test]
fn test_class_constructor_parameter_property() {
    let js = emit_ts("class Point { constructor(public x: number, public y: number) { } }");
    assert!(
        js.contains("this.x = x;"),
        "should emit parameter property assignment for x: {js}"
    );
    assert!(
        js.contains("this.y = y;"),
        "should emit parameter property assignment for y: {js}"
    );
    assert!(
        !js.contains("public"),
        "should strip 'public' keyword: {js}"
    );
}

#[test]
fn test_class_param_prop_complex_types() {
    // Definite assignment field with type annotation: erased (no runtime effect)
    let js1 = emit_ts("class Foo { readonly _A!: number; }");
    assert!(
        !js1.contains("_A"),
        "readonly field with ! and type-only should be erased: {js1}"
    );

    // Definite assignment field WITHOUT type annotation: kept as `p;`
    let js1b = emit_ts("class Foo { p!; }");
    assert!(
        js1b.contains("p;"),
        "definite field without type should emit: {js1b}"
    );

    // Field + constructor with parameter property
    let js2 = emit_ts("class Foo { readonly _A!: number; constructor(readonly name: string) {} }");
    assert!(
        js2.contains("this.name = name;"),
        "should emit parameter property for name: {js2}"
    );
    assert!(
        !js2.contains("_A"),
        "definite typed field should be erased: {js2}"
    );

    // Constructor with 'is' as parameter name (contextual keyword)
    let js3 = emit_ts("class Foo { constructor(readonly is: string) {} }");
    assert!(
        js3.contains("this.is = is;"),
        "should handle 'is' as param name with readonly modifier: {js3}"
    );
}

#[test]
fn test_class_static_field() {
    let js = emit_ts("class Config { static version = \"1.0\"; }");
    assert!(
        js.contains("static version = \"1.0\""),
        "should emit static field: {js}"
    );
}

#[test]
fn test_class_instance_field_initializer() {
    // Default target is ESNext (>= ES2022), so useDefineForClassFields is true.
    // Fields should be emitted as native class field declarations, not moved to constructor.
    let js = emit_ts("class Counter { count = 0; }");
    assert!(
        js.contains("count = 0;"),
        "should emit field as native class field declaration: {js}"
    );
    assert!(
        !js.contains("constructor"),
        "should not generate constructor with ESNext default: {js}"
    );
}

#[test]
fn test_class_instance_field_initializer_legacy() {
    // With explicit useDefineForClassFields: false, fields move to constructor.
    let js = emit_ts_with(
        "class Counter { count = 0; }",
        CompilerOptions {
            use_define_for_class_fields: Some(false),
            ..Default::default()
        },
    );
    assert!(
        js.contains("this.count = 0"),
        "legacy mode should move instance field init to constructor: {js}"
    );
    assert!(
        js.contains("constructor()"),
        "legacy mode should generate constructor: {js}"
    );
}

#[test]
fn test_legacy_class_field_multiline_binary_initializer_normalizes_function_spacing() {
    let js = emit_ts_with(
        "class B {\n\
         \x20\x20\x20\x20prop4 = '  ' +\n\
         \x20\x20\x20\x20function() {\n\
         \x20\x20\x20\x20} +\n\
         \x20\x20\x20\x20' ' +\n\
         \x20\x20\x20\x20(() => () => () => this);\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("this.prop4 = '  ' +\n            function () {"),
        "legacy multiline binary initializer should normalize anonymous function spacing: {js}"
    );
}

#[test]
fn test_class_fields_define_downlevel_emit_define_property() {
    let js = emit_ts_with(
        "class Base {}\n\
         class Sub extends Base {\n\
           prop: number;\n\
           constructor(public p: number) {\n\
             super();\n\
             this.prop = 1;\n\
           }\n\
           field = 0;\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            use_define_for_class_fields: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("Object.defineProperty(this, \"p\", {"),
        "downleveled define mode should define parameter property via Object.defineProperty: {js}"
    );
    assert!(
        js.contains("Object.defineProperty(this, \"prop\", {"),
        "downleveled define mode should define uninitialized field via Object.defineProperty: {js}"
    );
    assert!(
        js.contains("Object.defineProperty(this, \"field\", {"),
        "downleveled define mode should define initialized field via Object.defineProperty: {js}"
    );
    assert!(
        !js.contains("\n    p;\n"),
        "downleveled define mode should not emit native class field declaration: {js}"
    );
}

#[test]
fn test_abstract_class_strips_keyword() {
    let js = emit_ts("abstract class Shape { abstract area(): number; name = \"shape\"; }");
    assert!(
        !js.contains("abstract"),
        "should strip abstract keyword: {js}"
    );
    assert!(js.contains("class Shape"), "should emit class: {js}");
}

#[test]
fn test_class_implements_stripped() {
    let js = emit_ts("class MyClass implements Serializable { serialize() { return \"\"; } }");
    assert!(
        !js.contains("implements"),
        "should strip implements clause: {js}"
    );
    assert!(
        js.contains("class MyClass"),
        "should still emit class: {js}"
    );
}

#[test]
fn test_class_get_set_accessors() {
    let js = emit_ts(
        "class Box { private _value = 0; get value() { return this._value; } set value(v: number) { this._value = v; } }",
    );
    assert!(js.contains("get value()"), "should emit getter: {js}");
    assert!(
        js.contains("set value(v)"),
        "should emit setter with stripped type: {js}"
    );
}

#[test]
fn test_legacy_auto_accessor_ordering_transform_uses_private_storage() {
    let js = emit_ts_with(
        "class C { x = 0; accessor #x = 1; }",
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            use_define_for_class_fields: Some(false),
            ..Default::default()
        },
    );
    assert!(
        js.contains("this.x = 0;"),
        "legacy mode should hoist normal field initializer: {js}"
    );
    assert!(
        js.contains("this.#x_accessor_storage = 1;"),
        "legacy ordering transform should initialize accessor storage in constructor: {js}"
    );
    assert!(
        js.contains("#x_accessor_storage;"),
        "legacy ordering transform should emit storage field declaration: {js}"
    );
    assert!(
        js.contains("get #x() { return this.#x_accessor_storage; }"),
        "legacy ordering transform should emit getter using native private storage: {js}"
    );
    assert!(
        js.contains("set #x(value) { this.#x_accessor_storage = value; }"),
        "legacy ordering transform should emit setter using native private storage: {js}"
    );
    assert!(
        !js.contains("__classPrivateFieldGet"),
        "ESNext legacy ordering transform should not emit private-field helpers: {js}"
    );
}

#[test]
fn test_legacy_private_field_initializer_hoisted_for_ordering() {
    let js = emit_ts_with(
        "class C { x = 1; #y = 2; z = 3; }",
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            use_define_for_class_fields: Some(false),
            ..Default::default()
        },
    );
    let x_pos = js.find("this.x = 1;").unwrap_or(usize::MAX);
    let y_pos = js.find("this.#y = 2;").unwrap_or(usize::MAX);
    let z_pos = js.find("this.z = 3;").unwrap_or(usize::MAX);
    assert!(
        x_pos < y_pos && y_pos < z_pos,
        "legacy ordering should hoist x/#y/z initializers to constructor in source order: {js}"
    );
    assert!(
        js.contains("\n    #y;\n"),
        "private field declaration should remain in class body without initializer: {js}"
    );
    assert!(
        !js.contains("\n    #y = 2;\n"),
        "private field initializer should be moved out of class body: {js}"
    );
}

#[test]
fn test_private_field_anonymous_class_expr_uses_private_name_for_named_evaluation() {
    let js = emit_ts_with(
        "class B { #foo = class { static test = 123; }; }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __setFunctionName ="),
        "expected __setFunctionName helper for private field class expression: {js}"
    );
    assert!(
        js.contains("_B_foo.set(this, (_a = class {"),
        "expected private field initializer to downlevel through WeakMap.set: {js}"
    );
    assert!(
        js.contains("__setFunctionName(_a, \"#foo\")"),
        "expected anonymous class expression in private field to use #foo for named evaluation: {js}"
    );
}

#[test]
fn test_private_field_anonymous_class_expr_synth_ctor_hoists_temps_locally() {
    let js = emit_ts_with(
        "class B { #foo = class { static test = 123; }; #foo2 = class Foo { static otherClass = 123; }; }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let helper_pos = js.find("var _B_foo, _B_foo2;").unwrap_or(usize::MAX);
    let ctor_pos = js.find("constructor() {").unwrap_or(usize::MAX);
    let local_temps_pos = js.find("var _a, _b;").unwrap_or(usize::MAX);
    let first_init_pos = js.find("_B_foo.set(this").unwrap_or(usize::MAX);
    assert!(
        helper_pos < ctor_pos,
        "private WeakMap helpers should stay file-scoped ahead of the class: {js}"
    );
    assert!(
        ctor_pos < local_temps_pos && local_temps_pos < first_init_pos,
        "class-expression temps should be hoisted inside the synthesized constructor: {js}"
    );
}

#[test]
fn test_private_static_field_class_expr_uses_alias_and_new_parens() {
    let js = emit_ts_with(
        "class B {\n\
         static #foo = class {\n\
             constructor() {\n\
                 console.log(\"hello\");\n\
                 new B.#foo2();\n\
             }\n\
             static test = 123;\n\
             field = 10;\n\
         };\n\
         static #foo2 = class Foo {\n\
             static otherClass = 123;\n\
         };\n\
         m() {\n\
             console.log(B.#foo.test);\n\
             B.#foo.test = 10;\n\
             new B.#foo().field;\n\
         }\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);

    assert!(
        js.contains("new (__classPrivateFieldGet(_a, _a, \"f\", _B_foo2))();"),
        "nested class expression should use the outer static alias and preserve new-callee parens: {js}"
    );
    assert!(
        js.contains("_B_foo = { value: (_b = class {\n            constructor() {"),
        "wrapped private class-expression IIFE should preserve TypeScript indentation inside the class body: {js}"
    );
    assert!(
        js.contains(
            "        __setFunctionName(_b, \"#foo\"),\n        _b.test = 123,\n        _b) };"
        ),
        "wrapped private class-expression tail should stay aligned with TypeScript output: {js}"
    );
}

#[test]
fn test_class_static_block() {
    let js = emit_ts("class Foo { static { console.log(\"init\"); } }");
    assert!(js.contains("static {"), "should emit static block: {js}");
}

#[test]
fn test_downlevel_static_block_preserves_comment_only_body() {
    let source = "class C {\n\
                  @decorator\n\
                  static {\n\
                      // something\n\
                  }\n\
                  }";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2021),
            ..Default::default()
        },
    );
    assert!(
        js.contains("// something"),
        "downleveled static block should preserve comment-only body: {js}"
    );
}

#[test]
fn test_downlevel_static_block_leading_comment_moves_outside_class() {
    let js = emit_ts_with(
        "class C {\n\
         /* jsdocs */\n\
         static {\n\
             // something\n\
         }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2021),
            ..Default::default()
        },
    );
    let class_close = js.find("class C {\n}").unwrap_or(usize::MAX);
    let jsdoc_pos = js.find("/* jsdocs */").unwrap_or(0);
    assert!(
        class_close < jsdoc_pos,
        "leading static-block comment should appear after class close: {js}"
    );
}

#[test]
fn test_downlevel_static_block_unknown_private_name_recovers_to_obj_dot() {
    let js = emit_ts_with(
        "let getX: (c: C) => number;\n\
         class C {\n\
             #x = 1;\n\
             static {\n\
                 getX = (obj: C) => obj.#x;\n\
                 getY = (obj: D) => obj.#y;\n\
             }\n\
         }\n\
         let getY: (c: D) => number;\n\
         class D {\n\
             #y = 1;\n\
             static {\n\
                 getX = (obj: C) => obj.#x;\n\
                 getY = (obj: D) => obj.#y;\n\
             }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2021),
            ..Default::default()
        },
    );
    assert!(
        js.contains("getY = (obj) => obj.;"),
        "unknown private #y in class C static block should recover to obj.: {js}"
    );
    assert!(
        js.contains("getX = (obj) => obj.;"),
        "unknown private #x in class D static block should recover to obj.: {js}"
    );
    assert!(
        !js.contains("_C_y"),
        "unknown private #y should not synthesize _C_y helper: {js}"
    );
    assert!(
        !js.contains("_D_x"),
        "unknown private #x should not synthesize _D_x helper: {js}"
    );
}

#[test]
fn test_downlevel_static_block_rewrites_this_and_super_with_aliases() {
    let js = emit_ts_with(
        "class ElementsArray extends Array {\n\
         static {\n\
           const superisArray = super.isArray;\n\
           const customIsArray = (arg) => superisArray(arg);\n\
           this.isArray = customIsArray;\n\
         }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2021),
            ..Default::default()
        },
    );
    assert!(
        js.contains("Reflect.get(") && js.contains("\"isArray\""),
        "downleveled static super access should use Reflect.get: {js}"
    );
    assert!(
        js.contains(".isArray = customIsArray;"),
        "downleveled static this assignment should target class alias: {js}"
    );
    assert!(
        !js.contains("super.isArray"),
        "raw super property access should not remain after downleveling: {js}"
    );
    assert!(
        !js.contains("this.isArray = customIsArray;"),
        "raw static this assignment should be rewritten to alias: {js}"
    );
}

#[test]
fn test_js_file_jsx_type_args_recover_as_comma_and_fragment() {
    let js = emit_ts_file_with(
        "a.jsx",
        "<Foo<number>></Foo>;\n<Foo<number>/>;",
        CompilerOptions::default(),
    );
    assert!(
        js.contains("<Foo />, <number>></Foo>;"),
        "JS-file JSX type args should recover into comma form for element tags: {js}"
    );
    assert!(
        js.contains("<Foo />, <number>/>;"),
        "JS-file JSX type args should recover into comma form for self-closing tags: {js}"
    );
    assert!(
        js.contains("</>;"),
        "JS-file self-closing JSX type-arg recovery should emit trailing fragment close: {js}"
    );
}

#[test]
fn test_js_file_jsx_type_args_in_var_emit_fragment_tail() {
    let js = emit_ts_file_with(
        "file.jsx",
        "let x = <MyComp<Prop> a={10} b=\"hi\" />; // error\n",
        CompilerOptions {
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(js.contains("b=\"hi\" />; // error"), "{js}");
    assert!(js.ends_with("</>;\n"), "{js}");
}

#[test]
fn test_js_file_angle_type_assertion_recovery_emits_fragment_tail() {
    let js = emit_ts_file_with(
        "a.js",
        "var v = <string>undefined;",
        CompilerOptions::default(),
    );
    assert!(
        js.contains("var v = <string>undefined;"),
        "JS-file angle assertion recovery should preserve the original initializer text: {js}"
    );
    assert!(
        js.contains("</>;"),
        "JS-file angle assertion recovery should emit a trailing fragment close statement: {js}"
    );
}

#[test]
fn test_js_file_unary_lt_recovery_emits_jsx_like_tail() {
    let js = emit_ts_file_with("a.js", "~< <", CompilerOptions::default());
    assert!(
        js.contains("~< /> <"),
        "malformed JS-file unary less-than recovery should synthesize `< /> <`: {js}"
    );
}

#[test]
fn test_js_file_unary_fragment_recovery_preserves_fragment_text() {
    let js = emit_ts_file_with("a.js", "~<></> <", CompilerOptions::default());
    assert!(
        js.contains("~<></> <"),
        "malformed JS-file unary fragment recovery should preserve the fragment-like text: {js}"
    );
}

#[test]
fn test_js_file_unary_objectish_lt_recovery_emits_fragment_close() {
    let js = emit_ts_file_with("a.js", "!< {:>", CompilerOptions::default());
    assert!(
        js.contains("!< {...}>"),
        "malformed JS-file unary objectish less-than recovery should synthesize spread braces: {js}"
    );
    assert!(
        js.contains("</>;"),
        "malformed JS-file unary objectish less-than recovery should emit a fragment close tail: {js}"
    );
}

#[test]
fn test_tsx_unary_numeric_lt_recovery_keeps_var_statement_attached() {
    let js = emit_ts_file_with(
        "index.tsx",
        "const c = + <1234> x;",
        CompilerOptions {
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const c = + < />1234> x;"),
        "malformed TSX unary numeric less-than recovery should stay in the var statement: {js}"
    );
    assert!(
        js.contains("</></>;"),
        "malformed TSX unary numeric less-than recovery should emit the expected fragment tail: {js}"
    );
}

#[test]
fn test_type_alias_incorrect_return_token_recovery_skips_parameter_garbage() {
    let js = emit_ts_file_with(
        "a.ts",
        "type F2 = (n: number): string; // should be => not :",
        CompilerOptions::default(),
    );
    assert!(
        js.contains("string; // should be => not :"),
        "malformed type alias return token recovery should keep only the leaked return type tail: {js}"
    );
    assert!(
        !js.contains("number;"),
        "malformed type alias return token recovery should suppress the parameter-type garbage tail: {js}"
    );
}

#[test]
fn test_invalid_type_query_targets_recover_as_runtime_tails() {
    let js = emit_ts(
        "var x1: typeof {};\nvar x2: typeof (): void;\nvar x3: typeof 1;\nvar x4: typeof '';\nvar x5: typeof [];\nvar x6: typeof null;\nvar x8: typeof /123/;",
    );
    assert_eq!(
        js,
        "\"use strict\";\nvar x1, {};\nvar x2;\n() => ;\nvar x3;\n1;\nvar x4;\n'';\nvar x5;\nvar x6;\nvar x8;\n/123/;\n"
    );
}

#[test]
fn test_optional_method_return_recovery_in_class_and_object() {
    let js = emit_ts("class C { x()?: number; }\nvar b = {\n    x()?: 1 // error\n}");
    assert!(js.contains("class C {\n    x() { }\n}"), "{js}");
    assert!(
        js.contains("var b = {\n    x() { }, 1:  // error\n};"),
        "{js}"
    );
}

#[test]
fn test_member_expressions_recover_as_invalid_object_properties() {
    let js = emit_ts("var x = {\n    a.b,\n    a[\"ss\"],\n    a[1],\n};\nvar done = true;");
    assert!(
        js.contains("var x = {\n    a, : .b,\n    a, [\"ss\"]: ,\n    a, [1]: ,\n};"),
        "{js}"
    );
    assert!(js.contains("var done = true;"), "{js}");
}

#[test]
fn test_resolved_type_only_namespace_import_is_elided() {
    let js = emit_ts_with(
        "import * as types from './types';\nnew types.A();",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            other: vec![(
                "__tsrsResolvedTypeOnlyImportLocals".to_string(),
                "types".to_string(),
            )],
            ..Default::default()
        },
    );
    assert!(!js.contains("require(\"./types\")"), "{js}");
    assert!(js.contains("new types.A();"), "{js}");
}

#[test]
fn test_object_method_incorrect_return_token_recovery_emits_tail_block() {
    let js = emit_ts_file_with(
        "a.ts",
        "let o = {\n    m(n: number) => string {\n        return n.toString();\n    }\n};",
        CompilerOptions::default(),
    );
    assert!(
        js.contains("let o = {};"),
        "malformed object method return token recovery should collapse the object literal: {js}"
    );
    assert!(
        js.contains("string;\n{\n    return n.toString();\n}\n;"),
        "malformed object method return token recovery should emit the leaked tail block: {js}"
    );
}

#[test]
fn test_jsx_in_extends_clause_keeps_inner_class_body() {
    let src = r#"
declare namespace React {
  interface ComponentClass<P> { new (): Component<P, {}>; }
  class Component<A, B> {}
}
declare function createComponentClass<P>(factory: () => React.ComponentClass<P>): React.ComponentClass<P>;
class Foo extends createComponentClass(() => class extends React.Component<{}, {}> {
  render() {
    return <span>Hello, world!</span>;
  }
}) {}
"#;
    let js = emit_ts_file_with(
        "a.tsx",
        src,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        js.contains("class Foo extends createComponentClass(() => class extends React.Component {"),
        "outer extends/class header missing or malformed:\n{js}"
    );
    assert!(
        js.contains("render() {"),
        "inner class method body was dropped:\n{js}"
    );
    assert!(
        js.contains("return React.createElement(\"span\", null, \"Hello, world!\");"),
        "inner JSX transform missing:\n{js}"
    );
    assert!(
        js.contains("}) {"),
        "outer class body opener missing:\n{js}"
    );
}

#[test]
fn test_jsx_multiline_quoted_attr_values_escape_newlines() {
    let src = r#"
declare var React: any;
const a = <input value="
foo: 23
"></input>;
const b = <input value='
foo: 23
'></input>;
const c = <input value='
foo: 23\n
'></input>;
"#;
    let js = emit_ts_file_with(
        "a.tsx",
        src,
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const a = React.createElement(\"input\", { value: \"\\nfoo: 23\\n\" });"),
        "double-quoted multiline JSX attr should be escaped: {js}"
    );
    assert!(
        js.contains("const b = React.createElement(\"input\", { value: '\\nfoo: 23\\n' });"),
        "single-quoted multiline JSX attr should be escaped: {js}"
    );
    assert!(
        js.contains("const c = React.createElement(\"input\", { value: '\\nfoo: 23\\\\n\\n' });"),
        "escaped backslash-n inside multiline JSX attr should be preserved: {js}"
    );
}

#[test]
fn test_jsx_trailing_line_comment_inside_parens_is_not_duplicated() {
    let src = r#"
declare var React: any;
const Component = () => {
  const categories = ['Fruit', 'Vegetables'];
  return (
    <ul>
      <li>All</li>
      {categories.map((category) => (
        <li key={category}>{category}</li> // Error about 'key' only
      ))}
    </ul>
  );
};
"#;
    let js = emit_ts_file_with(
        "a.tsx",
        src,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "React.createElement(\"li\", { key: category }, category) // Error about 'key' only\n        ))));"
        ),
        "classic JSX emit should keep the trailing line comment attached to the nested element: {js}"
    );
    assert_eq!(
        js.matches("// Error about 'key' only").count(),
        1,
        "classic JSX emit should not duplicate trailing line comments: {js}"
    );
}

#[test]
fn test_jsx_namespace_prefix_recovery_react_emit() {
    let src = r#"
declare var React: any;
var tooManySeparators2 = <a:ele:ment></a:ele:ment>;
var endOfIdent2 = <a attr:={"value"} />;
var beginOfIdent1 = <:a attr={"value"} />;
"#;
    let js = emit_ts_file_with(
        "a.tsx",
        src,
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "var tooManySeparators2 = React.createElement(\"a:ele\", { ment: true }), ment;"
        ),
        "closing-tag extra-colon recovery should become `, ment` declarator: {js}"
    );
    assert!(
        js.contains("\n > ;\n"),
        "closing-tag recovery should leave standalone ` > ;` statement: {js}"
    );
    assert!(
        js.contains("var endOfIdent2 = React.createElement(\"a\", { \"attr:\": \"value\" });"),
        "attribute namespace recovery should keep bare trailing colon in key: {js}"
    );
    assert!(
        js.contains("var beginOfIdent1 =  < , a, attr = { \"value\":  } /  > ;"),
        "begin-of-ident recovery should stay in a single malformed var statement: {js}"
    );
}

#[test]
fn test_jsx_namespace_prefix_recovery_preserve_emit() {
    let src = r#"
var tooManySeparators2 = <a:ele:ment></a:ele:ment>;
var endOfIdent1 = <a: attr={"value"} />;
var beginOfIdent1 = <:a attr={"value"} />;
var beginOfIdent2 = <a :attr={"value"} />;
"#;
    let js = emit_ts_file_with(
        "a.tsx",
        src,
        CompilerOptions {
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var tooManySeparators2 = <a:ele ment></a:ele>, ment;"),
        "preserve-mode closing-tag recovery should keep `</a:ele>` before `, ment`: {js}"
    );
    assert!(
        js.contains("var endOfIdent1 = <a:attr {...\"value\"}/>;"),
        "preserve-mode should rewrite malformed `= {{...}}` shape into spread recovery: {js}"
    );
    assert!(
        js.contains("var beginOfIdent1 =  < , a, attr = { \"value\":  } /  > ;"),
        "preserve-mode begin-of-ident recovery should match TypeScript layout: {js}"
    );
    assert!(
        js.contains("var beginOfIdent2 = <a:attr {...\"value\"}/>;"),
        "preserve-mode should keep spread recovery for beginOfIdent2: {js}"
    );
}

#[test]
fn test_jsx_preserve_multiline_expression_container_uses_normalized_expr_copy() {
    let src = r#"
const emptyMessage = null as any;
const a = (
    <div>
      {0 ? (
        emptyMessage // must be identifier?
      ) : (
          // must be exactly two expression holes
        <span>
          {0}{0}
        </span>
      )}
    </div>
);
"#;
    let js = emit_ts_file_with(
        "a.tsx",
        src,
        CompilerOptions {
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(
        js.contains("{0 ? (emptyMessage // must be identifier?\n    ) : (\n    // must be exactly two expression holes\n    <span>\n          {0}{0}\n        </span>)}"),
        "preserve-mode multiline JSX expression containers should use normalized expression copy: {js}"
    );
}

#[test]
fn test_jsx_preserve_duplicates_self_closing_child_line_comments() {
    let src = r#"function View() {
  return (
    <div>
      <Item value={1} />  // error
      <Item value={2} />  // ok
    </div>
  )
}
"#;
    let js = emit_ts_file_with(
        "a.tsx",
        src,
        CompilerOptions {
            jsx: Some(JsxEmit::Preserve),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "      <Item value={1}/> // error\n      // error\n      <Item value={2}/> // ok\n      // ok"
        ),
        "preserve-mode child line comments should emit as trailing trivia and JSX text: {js}"
    );
}

#[test]
fn test_jsx_import_source_pragma_forces_runtime_under_react_mode() {
    let src = r#"
/* @jsxImportSource @emotion/react */
export const Comp = () => <div css="color: hotpink;"></div>;
"#;
    let js = emit_ts_file_with(
        "index.tsx",
        src,
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const jsx_runtime_1 = require(\"@emotion/react/jsx-runtime\");"),
        "jsxImportSource pragma should synthesize jsx-runtime require: {js}"
    );
    assert!(
        js.contains(
            "const Comp = () => (0, jsx_runtime_1.jsx)(\"div\", { css: \"color: hotpink;\" });"
        ),
        "jsxImportSource pragma should use jsx-runtime call instead of React.createElement: {js}"
    );
}

#[test]
fn test_class_method_overloads_stripped() {
    let js = emit_ts(
        "class C {\n  foo(x: number): void;\n  foo(x: string): void;\n  foo(x: any) { }\n}",
    );
    let count = js.matches("foo(").count();
    assert_eq!(count, 1, "should only emit method implementation: {js}");
}

#[test]
fn test_constructor_static_recovery_emits_post_class_assignment() {
    let js = emit_ts("class foo { constructor() { static f = 3; } }");
    assert!(
        js.contains("constructor() { }"),
        "recovery constructor body should be emptied: {js}"
    );
    assert!(
        js.contains("foo.f = 3;"),
        "recovery static field should be emitted after class body: {js}"
    );
    assert!(
        !js.contains("constructor() { static; f = 3; }"),
        "recovery artifacts should not remain in constructor body: {js}"
    );
}

#[test]
fn test_constructor_static_method_recovery_stays_in_class_body() {
    let js = emit_ts("class C { constructor() { static m1() {} } }");
    assert!(
        js.contains("constructor() {"),
        "constructor should still be emitted: {js}"
    );
    assert!(
        js.contains("static m1() { }"),
        "recovered static method should stay in the class body: {js}"
    );
    assert!(
        !js.contains("m1();"),
        "constructor body should not keep the parsed call artifact: {js}"
    );
}

#[test]
fn test_broken_constructor_still_synthesizes_field_init_ctor() {
    let js = emit_ts_with(
        "class Test {\n  prop = 42;\n  constructor\n}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            use_define_for_class_fields: Some(false),
            ..Default::default()
        },
    );
    assert!(
        js.contains("constructor() {"),
        "broken constructor recovery should still synthesize constructor: {js}"
    );
    assert!(
        js.contains("this.prop = 42;"),
        "synthesized constructor should include field initialization: {js}"
    );
}

#[test]
fn test_constructor_type_parameter_recovery_keeps_body_attached() {
    let js = emit_ts("class C { constructor<>() { } }");
    assert!(
        js.contains("constructor() { }"),
        "invalid constructor type params should be dropped during recovery: {js}"
    );
    assert!(
        !js.contains("constructor();"),
        "constructor recovery should not split the declaration from its body: {js}"
    );
}

#[test]
fn test_invalid_nonnullable_type_recovery_keeps_bodies_and_initializers() {
    let js = emit_ts(
        "function f1(a: string!) {}\nfunction f2(a: !string) {}\nfunction f3(): string! {}\nconst x: number! = 1;",
    );
    assert!(
        js.contains("function f1(a) { }"),
        "postfix invalid nonnullable param type should still emit a function body: {js}"
    );
    assert!(
        js.contains("function f2(a) { }"),
        "prefix invalid nonnullable param type should still emit a function body: {js}"
    );
    assert!(
        js.contains("function f3() { }"),
        "postfix invalid nonnullable return type should still emit a function body: {js}"
    );
    assert!(
        js.contains("const x = 1;"),
        "invalid nonnullable variable type should still keep its initializer: {js}"
    );
}

#[test]
fn test_invalid_unicode_assignment_recovery_drops_broken_lhs() {
    let js = emit_ts("var a\u{2081} = \"hello\"; alert(a\u{2081});");
    assert!(
        js.contains("var a;"),
        "invalid unicode continuation should split the declaration name: {js}"
    );
    assert!(
        js.contains("\"hello\";"),
        "broken assignment lhs should be dropped, keeping only the rhs expression: {js}"
    );
    assert!(
        js.contains("alert(a);"),
        "subsequent references should use the surviving identifier token: {js}"
    );
    assert!(
        !js.contains("₁ = \"hello\";"),
        "invalid unicode token should not remain as an emitted assignment lhs: {js}"
    );
}

#[test]
fn test_skipped_backslash_drops_attached_comment() {
    let js = emit_ts("\\ /*foo*/ ;");
    assert_eq!(js, "\"use strict\";\n;\n");
}

#[test]
fn test_null_between_unary_operator_and_operand_keeps_recovery_statements() {
    let js = emit_ts("foo\0+\0bar;");
    assert_eq!(js, "\"use strict\";\nfoo;\n+;\nbar;\n");
}

#[test]
fn test_unmatched_paren_after_missing_shift_operand_emits_empty_statement() {
    let js = emit_ts("retValue = bfs.VARIABLES >> );");
    assert_eq!(js, "\"use strict\";\nretValue = bfs.VARIABLES >> ;\n;\n");
}

#[test]
fn test_missing_paren_typed_arrow_recovery_combines_split_statements() {
    let js = emit_ts("(a:number => { }");
    assert_eq!(js, "\"use strict\";\n(a, {}) => ;\n");
}

#[test]
fn test_symbol_indexer_object_recovery_folds_trailing_literal() {
    let js = emit_ts("var x = {\n    [s: symbol]: \"\"\n}");
    assert_eq!(
        js,
        "\"use strict\";\nvar x = {\n    [s]: symbol, \"\": \n};\n"
    );
}

#[test]
fn test_mismatched_jsx_fragment_comment_moves_to_recovery_expression() {
    let js = emit_ts_file_with(
        "test.tsx",
        "<>hi</div> // Error",
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        "\"use strict\";\nReact.createElement(React.Fragment, null, \"hi\");\ndiv > ; // Error\n"
    );
}

#[test]
fn test_null_factory_sole_empty_fragment_strips_pragma_comments() {
    let js = emit_ts_file_with(
        "test.tsx",
        "/* @jsx jsx */\n/* @jsxfrag null */\nimport { jsx } from './renderer';\n<></>",
        CompilerOptions {
            jsx: Some(JsxEmit::React),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("@jsx"),
        "pragma comments should be stripped: {js}"
    );
    assert!(
        js.contains("jsx)(null, null)"),
        "empty fragment should still emit: {js}"
    );
}

#[test]
fn test_midfile_shebang_recovery_keeps_division_tail() {
    let js = emit_ts("var foo = 'x';\n#!/usr/bin/env node");
    assert!(
        js.contains("!/usr/bin / env;"),
        "mid-file shebang recovery should keep the broken division tail: {js}"
    );
    assert!(
        js.contains("node;"),
        "mid-file shebang should preserve trailing identifier: {js}"
    );
}

#[test]
fn test_unterminated_string_literal_recovery_preserves_raw_tail() {
    let js = emit_ts("var es1 = \"line 1\n\";\nvar es3 = 'line 1\\ \n';\nvar es13 = \" ");
    assert!(
        js.contains("var es1 = \"line 1;\n\";;"),
        "unterminated multiline string should preserve the split recovery form: {js}"
    );
    assert!(
        js.contains("var es3 = 'line 1\\ ;\n';;"),
        "backslash-space string recovery should preserve the raw space and split tail: {js}"
    );
    assert!(
        js.contains("var es13 = \" ;"),
        "EOF-terminated string should keep the trailing space before the semicolon: {js}"
    );
}

#[test]
fn test_index_signature_stripped() {
    let js = emit_ts("class Dict { [key: string]: any; get(k: string) { return this[k]; } }");
    assert!(
        !js.contains("[key: string]"),
        "index signature should be stripped: {js}"
    );
}

// ---------------------------------------------------------------
// Namespace emission tests
// ---------------------------------------------------------------

#[test]
fn test_namespace_basic() {
    let js = emit_ts("namespace MyNS { export function fn() { return 1; } }");
    assert!(js.contains("var MyNS;"), "should declare var: {js}");
    assert!(js.contains("(function (MyNS)"), "should have IIFE: {js}");
    assert!(
        js.contains("MyNS.fn = fn;"),
        "should assign exported function to namespace: {js}"
    );
}

#[test]
fn test_namespace_with_class() {
    let js = emit_ts("namespace Shapes { export class Circle { } }");
    assert!(js.contains("var Shapes;"), "should declare var: {js}");
    assert!(
        js.contains("Shapes.Circle = Circle;"),
        "should assign exported class: {js}"
    );
}

#[test]
fn test_declare_namespace_elided() {
    let js = emit_ts("declare namespace External { function foo(): void; }");
    assert!(
        !js.contains("var External"),
        "declare namespace should be elided: {js}"
    );
}

#[test]
fn test_declare_namespace_reserved_name_recovery_is_emitted() {
    let source = "declare namespace chrome.debugger {\n\
                  declare var tabId: number;\n\
                  }\n\
                  export const tabId = chrome.debugger.tabId;\n\
                  declare namespace test.class {}\n\
                  declare namespace debugger {} // still an error";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("declare;\nnamespace;\ndebugger;\n{ } // still an error"),
        "reserved namespace recovery should be tokenized: {js}"
    );
}

#[test]
fn test_namespace_var_export() {
    let js = emit_ts("namespace N { export const val = 42; }");
    assert!(
        js.contains("N.val = 42") || js.contains("N.val = val"),
        "namespace should export val: {js}"
    );
}

// ---------------------------------------------------------------
// Import/Export transformation tests (CommonJS)
// ---------------------------------------------------------------

#[test]
fn test_cjs_import_named() {
    let js = emit_ts_with(
        "import { foo, bar } from './module';\nconsole.log(foo, bar);",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("require(\"./module\")"),
        "should emit require: {js}"
    );
}

#[test]
fn test_cjs_import_namespace() {
    let js = emit_ts_with(
        "import * as fs from 'fs';\nconsole.log(fs);",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    // TypeScript 5.x always wraps namespace imports with __importStar
    assert!(
        js.contains("const fs = __importStar(require(\"fs\"))"),
        "should emit namespace require wrapped with __importStar: {js}"
    );
}

#[test]
fn test_generated_cjs_require_bindings_follow_script_target() {
    let source = "import defaultValue from 'default-dep';\n\
                  import * as namespaceValue from 'namespace-dep';\n\
                  import { namedValue } from 'named-dep';\n\
                  import requiredValue = require('required-dep');\n\
                  console.log(defaultValue.value, namespaceValue.value, namedValue, requiredValue.value);";

    let es5 = emit_ts_with(
        source,
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        es5.contains("var default_dep_1 = __importDefault(require(\"default-dep\"));"),
        "{es5}"
    );
    assert!(
        es5.contains("var namespaceValue = __importStar(require(\"namespace-dep\"));"),
        "{es5}"
    );
    assert!(
        es5.contains("var named_dep_1 = require(\"named-dep\");"),
        "{es5}"
    );
    assert!(
        es5.contains("var requiredValue = require(\"required-dep\");"),
        "{es5}"
    );
    assert!(
        !es5.lines()
            .any(|line| line.starts_with("const ") && line.contains("require(")),
        "pre-ES2015 generated require bindings must not retain const: {es5}"
    );

    let runtime = format!(
        "var require = function (name) {{\n\
             if (name === 'default-dep') return {{ value: 'default' }};\n\
             if (name === 'namespace-dep') return {{ value: 'namespace' }};\n\
             if (name === 'named-dep') return {{ namedValue: 'named' }};\n\
             if (name === 'required-dep') return {{ value: 'required' }};\n\
             throw new Error(name);\n\
         }};\n{es5}"
    );
    assert_eq!(
        execute_with_node(&runtime),
        "default namespace named required",
        "{es5}"
    );

    let es2015 = emit_ts_with(
        source,
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        es2015.contains("const default_dep_1 = __importDefault(require(\"default-dep\"));"),
        "{es2015}"
    );
    assert!(
        es2015.contains("const namespaceValue = __importStar(require(\"namespace-dep\"));"),
        "{es2015}"
    );
    assert!(
        es2015.contains("const named_dep_1 = require(\"named-dep\");"),
        "{es2015}"
    );
    assert!(
        es2015.contains("const requiredValue = require(\"required-dep\");"),
        "{es2015}"
    );
}

#[test]
fn test_generated_require_bindings_cover_jsx_tslib_node_and_preserve_boundaries() {
    let es5_jsx = emit_ts_file_with(
        "view.tsx",
        "export const view = <div />;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES5),
            jsx: Some(JsxEmit::ReactJSX),
            ..Default::default()
        },
    );
    assert!(
        es5_jsx.contains("var jsx_runtime_1 = require(\"react/jsx-runtime\");"),
        "{es5_jsx}"
    );

    let es2015_jsx = emit_ts_file_with(
        "view.tsx",
        "export const view = <div />;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            jsx: Some(JsxEmit::ReactJSX),
            ..Default::default()
        },
    );
    assert!(
        es2015_jsx.contains("const jsx_runtime_1 = require(\"react/jsx-runtime\");"),
        "{es2015_jsx}"
    );

    let es5_tslib = emit_ts_with(
        "export {}; using resource = makeResource();",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES5),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        es5_tslib.contains("var tslib_1 = require(\"tslib\");"),
        "{es5_tslib}"
    );

    let node_es5 = emit_ts_file_with(
        "index.ts",
        "const __require = null; const _createRequire = null; import value = require('dep'); value;",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            target: Some(ScriptTarget::ES5),
            other: vec![("__tsrsNodeEsmImportRequire".to_string(), "true".to_string())],
            ..Default::default()
        },
    );
    assert!(
        node_es5.contains("import { createRequire as _createRequire_1 } from \"module\";"),
        "{node_es5}"
    );
    assert!(
        node_es5.contains("var __require_1 = _createRequire_1(import.meta.url);"),
        "{node_es5}"
    );
    assert!(
        node_es5.contains("var value = __require_1('dep');"),
        "{node_es5}"
    );

    for (target, keyword) in [(ScriptTarget::ES5, "var"), (ScriptTarget::ES2015, "const")] {
        let preserve = emit_ts_with(
            "import value = require('dep'); value;",
            CompilerOptions {
                module: Some(ModuleKind::Preserve),
                target: Some(target),
                ..Default::default()
            },
        );
        assert!(
            preserve.contains(&format!("{keyword} value = require('dep');")),
            "module=preserve must retain require syntax and respect {target:?}: {preserve}"
        );
    }

    let preserve_es5_tslib = emit_ts_file_with(
        "resource.cts",
        "export {}; using resource = makeResource();",
        CompilerOptions {
            module: Some(ModuleKind::Preserve),
            target: Some(ScriptTarget::ES5),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        preserve_es5_tslib.contains("var tslib_1 = require(\"tslib\");"),
        "{preserve_es5_tslib}"
    );

    for module in [ModuleKind::Node16, ModuleKind::NodeNext] {
        let node_cjs = emit_ts_file_with(
            "index.cts",
            "import value from 'dep'; value;",
            CompilerOptions {
                module: Some(module),
                target: Some(ScriptTarget::ES5),
                ..Default::default()
            },
        );
        assert!(
            node_cjs.contains("var dep_1 = __importDefault(require(\"dep\"));"),
            "Node CJS output must downlevel generated requires for {module:?}: {node_cjs}"
        );
    }

    let umd_es5 = emit_ts_with(
        "import value from 'dep'; value;",
        CompilerOptions {
            module: Some(ModuleKind::UMD),
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        umd_es5.contains("var dep_1 = __importDefault(require(\"dep\"));"),
        "{umd_es5}"
    );
    let umd_es2015 = emit_ts_with(
        "import value from 'dep'; value;",
        CompilerOptions {
            module: Some(ModuleKind::UMD),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        umd_es2015.contains("const dep_1 = __importDefault(require(\"dep\"));"),
        "{umd_es2015}"
    );
}

#[test]
fn test_generated_cjs_require_name_collision_keeps_distinct_binding() {
    let javascript = emit_ts_with(
        "const dep_1 = 'local'; import { value } from 'dep'; console.log(dep_1, value);",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        javascript.contains("var dep_2 = require(\"dep\");"),
        "{javascript}"
    );
    assert!(
        javascript.contains("console.log(dep_1, dep_2.value);"),
        "{javascript}"
    );
    let runtime =
        format!("var require = function () {{ return {{ value: 'imported' }}; }};\n{javascript}");
    assert_eq!(
        execute_with_node(&runtime),
        "local imported",
        "{javascript}"
    );
}

#[test]
fn test_cjs_import_side_effect() {
    let js = emit_ts_with(
        "import './polyfill';",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("require(\"./polyfill\")"),
        "should emit side-effect require: {js}"
    );
}

#[test]
fn test_recover_bare_import_token_emits_import_marker() {
    let js = emit_ts_with(
        "import\nimport { foo } from './0';",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(
        js.contains("import ;"),
        "bare import token should emit recovery marker: {js}"
    );
}

#[test]
fn test_recover_import_keyword_with_type_arguments() {
    let js = emit_ts("import<T>\nconst a = import<string, number>");
    assert_eq!(js, "\"use strict\";\nimport;\nconst a = (import);\n");
}

#[test]
fn test_cjs_import_type_only_stripped() {
    let js = emit_ts_with(
        "import type { Foo } from './types';",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("require"),
        "type-only import should not generate require: {js}"
    );
}

#[test]
fn test_cjs_export_function() {
    let js = emit_ts_with(
        "export function greet() { return 'hi'; }",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.greet = greet;"),
        "should assign to exports: {js}"
    );
    assert!(
        js.contains("function greet()"),
        "should emit function: {js}"
    );
}

#[test]
fn test_cjs_export_class() {
    let js = emit_ts_with(
        "export class MyClass { }",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.MyClass = MyClass;"),
        "should assign to exports: {js}"
    );
}

#[test]
fn test_cjs_export_default_expression() {
    let js = emit_ts_with(
        "export default 42;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.default = 42;"),
        "should emit exports.default: {js}"
    );
}

#[test]
fn test_cjs_export_assign() {
    let js = emit_ts_with(
        "export = myObj;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("module.exports = myObj;"),
        "should emit module.exports: {js}"
    );
}

#[test]
fn test_cjs_use_strict_and_esmodule() {
    let js = emit_ts_with(
        "export const x = 1;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.starts_with("\"use strict\";\n"),
        "should start with use strict: {js}"
    );
    assert!(js.contains("__esModule"), "should define __esModule: {js}");
}

#[test]
fn test_cjs_re_export_all() {
    let js = emit_ts_with(
        "export * from './other';",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("require(\"./other\")"),
        "should require re-exported module: {js}"
    );
}

#[test]
fn test_cjs_export_type_only_stripped() {
    let js = emit_ts_with(
        "export type { Foo } from './types';",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("require(\"./types\")"),
        "type-only export should not generate require: {js}"
    );
}

#[test]
fn test_collect_type_only_export_names_for_import_type_reexport() {
    let file = tsc_rs_parser::parse("c.ts", "import type { C } from './b';\nexport { C as D };");
    let type_only = collect_type_only_export_names(&file);
    let value = collect_value_export_names(&file);
    assert!(
        type_only.contains("D"),
        "re-export of an explicit import type should stay type-only: {type_only:?}"
    );
    assert!(
        !value.contains("D"),
        "re-export of an explicit import type should not be classified as a value export: {value:?}"
    );
}

#[test]
fn test_collect_value_export_names_excludes_direct_const_enum_but_keeps_named_reexport() {
    let direct_const_enum = tsc_rs_parser::parse("enum.ts", "export const enum Enum { One = 1 }");
    let merged_reexport = tsc_rs_parser::parse(
        "merge.ts",
        "import { Enum } from './enum';\nexport { Enum };",
    );
    let direct_value = collect_value_export_names(&direct_const_enum);
    let merged_value = collect_value_export_names(&merged_reexport);
    assert!(
        !direct_value.contains("Enum"),
        "direct const enum exports should not be classified as runtime value exports: {direct_value:?}"
    );
    assert!(
        merged_value.contains("Enum"),
        "named re-export of an imported const enum should still be classified as a runtime export stub: {merged_value:?}"
    );
}

#[test]
fn test_cjs_reexport_from_type_only_source_skips_predecl() {
    let file = tsc_rs_parser::parse("b.ts", "export { B as C } from './a';");
    let out = emit_with_global_type_only(
        &file,
        &CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
        &HashMap::new(),
        &HashSet::new(),
        &HashSet::from([AstString::from("B")]),
    );
    assert!(
        !out.javascript.contains("exports.C = void 0;"),
        "type-only source re-export should not get a CommonJS predecl: {}",
        out.javascript
    );
}

#[test]
fn test_cjs_import_from_type_only_export_is_elided() {
    let file = tsc_rs_parser::parse(
        "d.ts",
        "import { D } from './c';\nnew D();\nconst d: D = {};",
    );
    let out = emit_with_global_type_only(
        &file,
        &CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
        &HashMap::new(),
        &HashSet::new(),
        &HashSet::from([AstString::from("D")]),
    );
    assert!(
        !out.javascript.contains("require(\"./c\")"),
        "type-only import should be elided even when used through a value-looking name: {}",
        out.javascript
    );
    assert!(
        out.javascript.contains("new D();"),
        "elided type-only import should leave the original identifier reference behind: {}",
        out.javascript
    );
}

#[test]
fn test_cjs_default_import_marked_type_only_is_elided() {
    let js = emit_ts_with(
        "import A from './a';\nimport type { default as B } from './a';\nexport { A, B };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            other: vec![(
                "__tsrsResolvedTypeOnlyImportLocals".to_string(),
                "A".to_string(),
            )],
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\n"
    );
}

// ---------------------------------------------------------------
// Import/Export transformation tests (ESM)
// ---------------------------------------------------------------

#[test]
fn test_esm_import_preserved() {
    let js = emit_ts_esm("import { foo } from './module';\nconsole.log(foo);");
    assert!(
        js.contains("import { foo } from './module'"),
        "ESM import should be preserved: {js}"
    );
}

#[test]
fn test_esm_export_preserved() {
    let js = emit_ts_esm("export function greet() { return 'hi'; }");
    assert!(
        js.contains("export function greet()"),
        "ESM export should be preserved: {js}"
    );
}

#[test]
fn test_esm_export_default() {
    let js = emit_ts_esm("export default function handler() { }");
    assert!(
        js.contains("export default function handler()"),
        "ESM export default should be preserved: {js}"
    );
}

#[test]
fn test_esm_re_export() {
    let js = emit_ts_esm("export { a, b } from './lib';");
    assert!(
        js.contains("export { a, b } from './lib'"),
        "ESM re-export should be preserved: {js}"
    );
}

#[test]
fn test_esm_export_all() {
    let js = emit_ts_esm("export * from './utils';");
    assert!(
        js.contains("export * from \"./utils\"") || js.contains("export * from './utils'"),
        "ESM export * should be preserved: {js}"
    );
}

#[test]
fn test_esm_import_type_only_stripped() {
    let js = emit_ts_esm("import type { Foo } from './types';");
    assert!(
        !js.contains("import"),
        "type-only import should be completely stripped in ESM: {js}"
    );
}

// ---------------------------------------------------------------
// Arrow function tests
// ---------------------------------------------------------------

#[test]
fn test_arrow_type_stripped() {
    let js = emit_ts("const add = (a: number, b: number): number => a + b;");
    assert!(
        !js.contains(": number"),
        "type annotations should be stripped from arrow: {js}"
    );
    assert!(
        js.contains("(a, b) => a + b"),
        "arrow should be preserved: {js}"
    );
}

#[test]
fn test_arrow_with_type_params() {
    let js = emit_ts("const identity = <T,>(x: T): T => x;");
    assert!(
        !js.contains("<T"),
        "type parameters should be stripped: {js}"
    );
}

#[test]
fn test_hazard_free_ordinary_arrow_downlevels_only_for_es5() {
    let source = "var empty = (a, b) => { };\nvar add = (a: number, b: number) => a + b;";
    let es5 = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&es5),
        "\"use strict\";\nvar empty = function (a, b) { };\nvar add = function (a, b) { return a + b; };\n"
    );

    let es2015 = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&es2015),
        "\"use strict\";\nvar empty = (a, b) => { };\nvar add = (a, b) => a + b;\n"
    );
}

#[test]
fn test_hazard_free_arrow_expression_statement_is_parenthesized() {
    let js = emit_ts_with(
        "(x) => ({ value: x } as { value: number }).value;",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&js),
        "\"use strict\";\n(function (x) { return ({ value: x }.value); });\n"
    );
}

#[test]
fn test_hazard_free_arrow_preserves_supported_detached_body_comment() {
    let js = emit_ts_with(
        "const pick = (value: string) =>\n    // retained\n    value?.trim();",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&js),
        "\"use strict\";\nvar pick = function (value) {\n    // retained\n    return value === null || value === void 0 ? void 0 : value.trim();\n};\n"
    );
}

#[test]
fn test_hazard_free_arrow_scopes_detached_body_temps_locally() {
    let js = emit_ts_with(
        "declare function reenter(): number;\n\
         const computed = (key: string) =>\n\
             // computed comment\n\
             ({ [key]: reenter() });\n\
         declare function get(): { value: number } | undefined;\n\
         const optional = () =>\n\
             // optional comment\n\
             get()?.value;",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&js),
        concat!(
            "\"use strict\";\n",
            "var computed = function (key) { var _a; \n",
            "// computed comment\n",
            "return (_a = {}, _a[key] = reenter(), _a); };\n",
            "var optional = function () { var _a; \n",
            "// optional comment\n",
            "return (_a = get()) === null || _a === void 0 ? void 0 : _a.value; };\n",
        )
    );
    assert!(
        !js.starts_with("var _a") && !js.contains("\nvar _a;\n"),
        "concise-body temps must not be hoisted to file scope: {js}"
    );
}

#[test]
fn test_hazard_free_arrow_temp_comment_uses_enclosing_indent_only() {
    let js = emit_ts_with(
        "declare function get(): { value: number } | undefined;\n\
         function outer() {\n\
             const nested = () =>\n\
                 // nested temp comment\n\
                 get()?.value;\n\
             const noTemp = (value: { value: number } | undefined) =>\n\
                 // no temp comment\n\
                 value?.value;\n\
             return [nested, noTemp];\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&js),
        concat!(
            "\"use strict\";\n",
            "function outer() {\n",
            "    var nested = function () { var _a; \n",
            "    // nested temp comment\n",
            "    return (_a = get()) === null || _a === void 0 ? void 0 : _a.value; };\n",
            "    var noTemp = function (value) {\n",
            "        // no temp comment\n",
            "        return value === null || value === void 0 ? void 0 : value.value;\n",
            "    };\n",
            "    return [nested, noTemp];\n",
            "}\n",
        )
    );

    let removed = emit_ts_with(
        "declare function get(): { value: number } | undefined;\n\
         const optional = () =>\n\
             // removed comment must not force detached layout\n\
             get()?.value;",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            remove_comments: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&removed),
        concat!(
            "\"use strict\";\n",
            "var optional = function () { var _a; return (_a = get()) === null || _a === void 0 ? void 0 : _a.value; };\n",
        )
    );
}

#[test]
fn test_hazard_free_arrow_compact_temp_source_map_anchors_follow_final_output() {
    let source = "declare function get(): { value: number } | undefined; declare const after: number; const arr=[()=>get()?.value, after];";
    let file = tsc_rs_parser::parse("test.ts", source);
    let output = emit(
        &file,
        &CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            source_map: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&output.javascript),
        concat!(
            "\"use strict\";\n",
            "var arr = [function () { var _a; return (_a = get()) === null || _a === void 0 ? void 0 : _a.value; }, after];\n",
            "//# sourceMappingURL=test.js.map",
        )
    );

    let mappings = decode_source_map_mappings(output.source_map.as_deref().expect("source map"));
    assert_source_map_columns_within_output(&output.javascript, &mappings);
    let get_original = byte_line_column(source, source.rfind("get()?.value").unwrap());
    let after_original = byte_line_column(source, source.rfind("after]").unwrap());
    assert!(mappings.contains(&DecodedSourceMapMapping {
        generated_line: 1,
        generated_column: 46,
        original_line: get_original.0,
        original_column: get_original.1,
    }));
    assert!(mappings.contains(&DecodedSourceMapMapping {
        generated_line: 1,
        generated_column: 103,
        original_line: after_original.0,
        original_column: after_original.1,
    }));
}

#[test]
fn test_hazard_free_arrow_detached_temp_source_map_anchors_use_final_indent() {
    let source = concat!(
        "declare function get(): { value: number } | undefined;\n",
        "declare const after: number;\n",
        "const arr = [() =>\n",
        "    // retained\n",
        "    get()?.value, after];",
    );
    let file = tsc_rs_parser::parse("test.ts", source);
    let output = emit(
        &file,
        &CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            source_map: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        normalize_newlines(&output.javascript),
        concat!(
            "\"use strict\";\n",
            "var arr = [function () { var _a; \n",
            "    // retained\n",
            "    return (_a = get()) === null || _a === void 0 ? void 0 : _a.value; }, after];\n",
            "//# sourceMappingURL=test.js.map",
        )
    );

    let mappings = decode_source_map_mappings(output.source_map.as_deref().expect("source map"));
    assert_source_map_columns_within_output(&output.javascript, &mappings);
    let get_original = byte_line_column(source, source.rfind("get()?.value").unwrap());
    let after_original = byte_line_column(source, source.rfind("after]").unwrap());
    assert!(mappings.contains(&DecodedSourceMapMapping {
        generated_line: 3,
        generated_column: 17,
        original_line: get_original.0,
        original_column: get_original.1,
    }));
    assert!(mappings.contains(&DecodedSourceMapMapping {
        generated_line: 3,
        generated_column: 74,
        original_line: after_original.0,
        original_column: after_original.1,
    }));
}

#[test]
fn test_hazard_free_arrow_detached_computed_temp_is_reentrant() {
    let source = "let depth = 0;\n\
        let inner;\n\
        function reenter() {\n\
            if (depth++ === 0) { inner = make('inner'); return 1; }\n\
            return 2;\n\
        }\n\
        const make = (key: string) =>\n\
            // keep the temp below inside each generated call frame\n\
            ({ [key]: reenter() });\n\
        const outer = make('outer');\n\
        console.log(Object.keys(outer).join(','), Object.keys(inner).join(','), outer === inner);";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function (key) { var _a; \n// keep the temp below inside each generated call frame\nreturn"),
        "generated temp must be declared inside the converted arrow: {js}"
    );
    let output = run_node_from_stdin(&[], &js);
    assert!(
        output.status.success(),
        "node failed: {}\n{js}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "outer inner false"
    );
}

#[test]
fn test_hazard_free_arrow_rejects_lexical_and_ownership_hazards() {
    let cases = [
        "const f = () => this.value;",
        "function outer() { const f = () => arguments[0]; }",
        "function outer() { const f = () => new.target; }",
        "class Derived extends Base { m() { const f = () => super.m(); } }",
        "const f = () => class Inner {};",
        "const f = () => eval('value');",
        "const f = (/* param */ value) => value;",
        "const f = (value) => value /* body */ + 1;",
        "const f = (value = 1) => value;",
        "const f = (...value) => value;",
        "const f = ({ value }) => value;",
        "const f = (value?: number) => value;",
        "const f = <T>(value: T): T => value;",
        "const f = async (value) => eval(await value);",
        "var await = () => { };",
        "const f = (value) => { if (value) return 1; return 0; };",
    ];
    for source in cases {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                ..Default::default()
            },
        );
        assert!(
            js.contains("=>"),
            "unsupported arrow must stay native for {source:?}: {js}"
        );
    }
}

#[test]
fn test_hazard_free_arrow_rejects_escaped_lexical_arguments_and_direct_eval() {
    let cases = [
        (
            "function outer(value) { return (() => arg\\u0075ments[0])(); } console.log(outer(7));",
            "escaped arguments",
        ),
        (
            "function outer(value) { return (() => ev\\u0061l('arguments[0]'))(); } console.log(outer(7));",
            "escaped direct eval",
        ),
    ];
    for (source, label) in cases {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                ..Default::default()
            },
        );
        assert!(
            js.contains("=>"),
            "{label} must keep its lexical arrow environment: {js}"
        );
        let output = run_node_from_stdin(&[], &js);
        assert!(
            output.status.success(),
            "node failed for {label}: {}\n{js}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "7",
            "{label} changed runtime binding"
        );
    }
}

#[test]
fn test_hazard_free_arrow_keeps_arguments_binding_and_runs_across_scopes() {
    let source = "const top = (x) => { return x + 1; };\n\
         function outer(base) { const nested = (x) => (y) => base + x + y; return nested(2)(3); }\n\
         const holder = { method() { const local = (x) => x * 2; return local(4); } };\n\
         console.log(top(1), outer(1), holder.method());";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("=>"),
        "all runtime arrows are hazard-free: {js}"
    );
    let output = run_node_from_stdin(&[], &js);
    assert!(
        output.status.success(),
        "node failed: {}\n{js}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "2 6 8");
}

#[test]
fn test_hazard_free_arrow_allows_arguments_parameter_without_capture() {
    let js = emit_ts_with(
        "function f() { var _arguments = 10; var a = (arguments) => () => _arguments; }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var a = function (arguments) { return function () { return _arguments; }; };"),
        "the parameter binding is not a lexical arguments reference: {js}"
    );
    assert!(
        !js.contains("_arguments = arguments"),
        "ordinary safe lowering must not synthesize an arguments capture: {js}"
    );
}

// ---------------------------------------------------------------
// Template literal tests
// ---------------------------------------------------------------

#[test]
fn test_template_literal_preserved() {
    let js = emit_ts("const msg = `hello ${name}`;");
    assert!(
        js.contains("`hello ${name}`"),
        "template literal should be preserved: {js}"
    );
}

fn es5_options() -> CompilerOptions {
    CompilerOptions {
        target: Some(ScriptTarget::ES5),
        ..CompilerOptions::default()
    }
}

#[test]
fn test_es5_template_cooks_physical_line_endings_and_escapes_line_separators() {
    let js = emit_ts_with(
        "let crlf = `a\r\nb`; let cr = `a\rb`; let ls = `a\u{2028}b`; let ps = `a\u{2029}b`;",
        es5_options(),
    );
    assert_eq!(
        js.matches("\"a\\nb\"").count(),
        2,
        "CR and CRLF must both cook to LF: {js}"
    );
    assert!(js.contains("\"a\\u2028b\""), "U+2028 must be escaped: {js}");
    assert!(js.contains("\"a\\u2029b\""), "U+2029 must be escaped: {js}");
}

#[test]
fn test_es5_template_legacy_octal_recovery_matches_strict_safe_cooked_values() {
    let js = emit_ts_with(
        r#"let one = `\1`; let seven = `\7`; let three = `\077`; let max = `\377`; let nul_digit = `\08`;"#,
        es5_options(),
    );
    assert!(js.contains(r#"var one = "\u0001";"#), "{js}");
    assert!(js.contains(r#"var seven = "\u0007";"#), "{js}");
    assert!(js.contains(r#"var three = "?";"#), "{js}");
    assert!(js.contains(r#"var max = "\u00FF";"#), "{js}");
    assert!(js.contains(r#"var nul_digit = "\x008";"#), "{js}");
    assert!(
        !js.contains(r#""\01""#),
        "must not emit strict-mode octal: {js}"
    );
}

#[test]
fn test_es5_template_preserves_interpolation_comments_and_pure_annotations() {
    let js = emit_ts_with(
        "let inline = `a${/*#__PURE__*/ factory() /*after*/}b`;\n\
         let multiline = `a${\n// before\nfactory()\n/* after */\n}b`;",
        es5_options(),
    );
    assert!(js.contains("/*#__PURE__*/ factory() /*after*/"), "{js}");
    assert!(js.contains("// before\nfactory()\n/* after */"), "{js}");
}

#[test]
fn test_es5_template_parenthesizes_comma_substitution_as_one_concat_argument() {
    let js = emit_ts_with(r#"let result = `${mark("a"), mark("b")}`;"#, es5_options());
    assert!(
        js.contains(r#"var result = "".concat((mark("a"), mark("b")));"#),
        "comma substitution must remain one concat argument: {js}"
    );
}

#[test]
fn test_es5_structured_tagged_template_uses_make_template_object() {
    let js = emit_ts_with(
        "let powered = tag`outer${x ** 2}tail`;\n\
         let nested = tag`outer${`inner${x}`}tail`;",
        es5_options(),
    );
    assert!(
        js.contains(
            r#"tag(__makeTemplateObject(["outer", "tail"], ["outer", "tail"]), Math.pow(x, 2))"#
        ),
        "valid tagged templates must lower for ES5: {js}"
    );
    assert!(
        js.contains(
            r#"tag(__makeTemplateObject(["outer", "tail"], ["outer", "tail"]), "inner".concat(x))"#
        ),
        "nested untagged substitutions must still lower: {js}"
    );
    assert!(
        js.contains("var __makeTemplateObject ="),
        "helper must be requested during the pre-scan: {js}"
    );
    assert!(
        !js.contains('`'),
        "ES5 output must not retain backticks: {js}"
    );
}

#[test]
fn test_es5_tagged_template_helper_is_prescanned_in_nested_control_flow() {
    let js = emit_ts_with(
        "class C { method() { while (ready) { switch (value) { case 1: return tag`nested`; } } } }",
        es5_options(),
    );
    let helper = js
        .find("var __makeTemplateObject =")
        .expect("nested tagged template must request helper");
    let use_site = js
        .rfind("__makeTemplateObject(")
        .expect("nested tagged template helper call");
    assert!(
        helper < use_site,
        "helper must be emitted before its first use: {js}"
    );
    assert!(
        js.contains(r#"tag(__makeTemplateObject(["nested"], ["nested"]))"#),
        "nested tag must lower: {js}"
    );
}

#[test]
fn test_es5_tagged_template_cooked_and_raw_values_are_utf16_and_line_ending_safe() {
    let js = emit_ts_with(
        "let mixed = tag`\\uD800\r\nline${x}bad \\unicode`;\n\
         let cr = tag`first\rsecond`;\n\
         let separators = tag`a\u{2028}b${x}bad \\unicode and \u{2029}`;",
        es5_options(),
    );
    assert!(
        js.contains(
            r#"__makeTemplateObject(["\uD800\nline", void 0], ["\\uD800\nline", "bad \\unicode"])"#
        ),
        "valid lone surrogate and invalid neighboring quasi must be independent: {js}"
    );
    assert!(
        js.contains(r#"__makeTemplateObject(["first\nsecond"], ["first\nsecond"])"#),
        "physical CR must normalize to LF in both cooked and raw arrays: {js}"
    );
    assert!(
        js.contains(
            r#"__makeTemplateObject(["a\u2028b", void 0], ["a\u2028b", "bad \\unicode and \u2029"])"#
        ),
        "line separators must be escaped and invalid cooked values must be void 0: {js}"
    );
    assert!(
        !js.contains('`'),
        "ES5 output must not retain backticks: {js}"
    );
}

#[test]
fn test_es5_tagged_template_preserves_comments_at_raw_and_value_boundaries() {
    let js = emit_ts_with(
        "const obj = { value: 7, tag(strings: any, value: any) { return [this.value, strings.raw[1], value]; } };\n\
         const result = obj.tag`a${/*before*/ 3 /*after*/}b`;\n\
         console.log(JSON.stringify(result));",
        es5_options(),
    );

    assert!(
        js.contains(
            r#"obj.tag(__makeTemplateObject(["a", "b"], ["a" /*before*/, /*after*/ "b"]), /*before*/ 3 /*after*/)"#
        ),
        "tagged interpolation comments must match TypeScript's duplicated placement: {js}"
    );
    assert_eq!(js.matches("/*before*/").count(), 2, "{js}");
    assert_eq!(js.matches("/*after*/").count(), 2, "{js}");
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), r#"[7,"b",3]"#);
}

#[test]
fn test_es5_async_tagged_template_preserves_receiver_and_comments() {
    let js = emit_ts_with(
        "const obj = { value: 9, tag(strings: any, value: any) { return [this.value, strings.raw[1], value]; } };\n\
         async function run() { return obj.tag`a${/*before*/ await Promise.resolve(4) /*after*/}b`; }\n\
         run().then(value => console.log(JSON.stringify(value)));",
        es5_options(),
    );

    assert!(js.contains("obj.tag(__makeTemplateObject("), "{js}");
    assert!(
        js.contains("/*before*/ yield Promise.resolve(4) /*after*/"),
        "async substitutions must keep comments while lowering await: {js}"
    );
    assert_eq!(js.matches("/*before*/").count(), 2, "{js}");
    assert_eq!(js.matches("/*after*/").count(), 2, "{js}");
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), r#"[9,"b",4]"#);
}

#[test]
fn test_es5_js_pseudo_type_args_remain_binary_and_do_not_evaluate_tag() {
    let js = emit_ts_file_with(
        "test.js",
        "let calls = 0;\n\
         function tag() { calls++; return 1; }\n\
         const T = 0, value = 2;\n\
         const result = tag<T>`x${value}`;\n\
         console.log(calls, result);",
        es5_options(),
    );

    assert!(
        js.contains(r#"var result = tag < T > "x".concat(value);"#),
        "JavaScript pseudo type args must remain binary comparisons: {js}"
    );
    assert!(
        !js.contains("__makeTemplateObject"),
        "a JavaScript comparison must not request the tagged-template helper: {js}"
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "0 false");
}

#[test]
fn test_es5_async_templates_share_plain_and_tagged_lowering() {
    let js = emit_ts_with(
        r#"declare const promise: Promise<string>;
           declare function tag(strings: TemplateStringsArray, ...values: unknown[]): unknown;
           async function plain() { return `head${await promise}tail`; }
           async function tagged() { return tag`head${await promise}tail`; }
           async function mixed() { return tag`\uD800${await promise}bad \unicode`; }"#,
        es5_options(),
    );
    assert!(
        js.contains(r#"return "head".concat(yield promise, "tail")"#),
        "async untagged templates must use ES5 concat lowering: {js}"
    );
    assert!(
        js.contains(
            r#"tag(__makeTemplateObject(["head", "tail"], ["head", "tail"]), yield promise)"#
        ),
        "async valid tagged templates must use the helper: {js}"
    );
    assert!(
        js.contains(
            r#"tag(__makeTemplateObject(["\uD800", void 0], ["\\uD800", "bad \\unicode"]), yield promise)"#
        ),
        "async mixed valid/invalid quasis must preserve cooked/raw values: {js}"
    );
    assert!(
        !js.contains('`'),
        "ES5 async output must not retain backticks: {js}"
    );
}

#[test]
fn test_es5_template_preserves_utf16_surrogates_and_astral_pairs() {
    let js = emit_ts_with(
        r#"let lone = `\uD800`; let braced_lone = `\u{D800}`; let astral = `\u{10000}`;"#,
        es5_options(),
    );
    assert_eq!(js.matches(r#""\uD800""#).count(), 2, "{js}");
    assert!(js.contains(r#""\uD800\uDC00""#), "{js}");
}

#[test]
fn test_es2015_templates_remain_unmodified() {
    let source = r#"let plain = `head${value}tail`; let tagged = tag`head${value}tail`;"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..CompilerOptions::default()
        },
    );
    assert!(js.contains("`head${value}tail`"), "{js}");
    assert!(
        js.contains("tag`head${value}tail`") || js.contains("tag `head${value}tail`"),
        "{js}"
    );
    assert!(
        !js.contains(".concat("),
        "ES2015 templates must not lower: {js}"
    );
}

#[test]
fn test_missing_comma_between_template_strings_array_normalizes_tagged_gap() {
    let source = "var array = [\n    `template string 1`\n    `template string 2`\n  ];";
    let file = tsc_rs_parser::parse("test.ts", source);
    let StmtKind::Var(var_stmt) = &file.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected array initializer");
    let ExprKind::ArrayLit(elements) = &init.kind else {
        panic!("expected array literal, got {:?}", init.kind);
    };
    assert_eq!(
        elements.len(),
        1,
        "expected missing comma recovery to parse as a single array element"
    );
    let Some(first) = &elements[0] else {
        panic!("expected first array element");
    };
    let ExprKind::TaggedTemplate(_) = &first.kind else {
        panic!(
            "expected first element to recover as a tagged template, got {:?}",
            first.kind
        );
    };
    let js = emit(&file, &CompilerOptions::default()).javascript;
    assert_eq!(
        js, "\"use strict\";\nvar array = [\n    `template string 1` `template string 2`\n];\n",
        "missing-comma template strings should normalize to a single tagged-template gap"
    );
}

#[test]
fn test_declare_class_missing_body_keeps_recovered_call_stmt() {
    let file = tsc_rs_parser::parse("test.ts", "declare class foo();\nfunction foo() {}");
    let js = emit(&file, &CompilerOptions::default()).javascript;
    assert_eq!(js, "\"use strict\";\n();\nfunction foo() { }\n");
}

#[test]
fn test_zero_span_error_expr_stmt_emits_empty_statement() {
    let file = tsc_rs_parser::parse("test.ts", "@<[[import(obju2c77,\n");
    let js = emit(&file, &CompilerOptions::default()).javascript;
    assert_eq!(js, "\"use strict\";\n;\n");
}

#[test]
fn test_class_global_namespace_recovery_tail_emits_namespace_after_class() {
    let source = "class C {\n    global x\n}";
    let file = tsc_rs_parser::parse("test.ts", source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        ..CompilerOptions::default()
    };
    let js = emit(&file, &options).javascript;
    assert_eq!(
        js,
        "\"use strict\";\nclass C {\n}\nvar global;\n(function (global) {\n})(global || (global = {}));\nx;\n"
    );
}

#[test]
fn test_midfile_hashbang_recovery_emits_bang_statements() {
    let source =
        "const a =!@#!@$\nconst b = !@#!@#!@#!\nOK!\nHERE's A shouty thing\nGOTTA GO FAST\n";
    let file = tsc_rs_parser::parse("test.ts", source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        ..CompilerOptions::default()
    };
    let js = emit(&file, &options).javascript;
    assert_eq!(
        js,
        "\"use strict\";\nconst a = !;\n!;\nconst b = !;\n!;\n!;\n!OK;\nHERE;\n's A shouty thing;\nGOTTA;\nGO;\nFAST;\n"
    );
}

#[test]
fn test_new_empty_array_call_callee_recovery_collapses_multiline_layout() {
    let source = "var t4 =\nnew\nstring\n[\n    ]\n    (\n        );";
    let file = tsc_rs_parser::parse("test.ts", source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        ..CompilerOptions::default()
    };
    let js = emit(&file, &options).javascript;
    assert_eq!(js, "\"use strict\";\nvar t4 = new string[]();\n");
}

#[test]
fn test_object_rest_property_name_recovery_uses_type_name_as_binding() {
    let source = "const { ...a: b } = {};";
    let file = tsc_rs_parser::parse("test.ts", source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        ..CompilerOptions::default()
    };
    let js = emit(&file, &options).javascript;
    assert!(
        js.contains("const b = __rest({}, []);"),
        "unexpected emit: {js}"
    );
    assert!(
        !js.contains("const a = __rest(, []);"),
        "unexpected emit: {js}"
    );
}

#[test]
fn test_object_literal_empty_element_access_recovery_keeps_next_property() {
    let source = "function bug() { return { tokens: Gar[], endState: state }; }";
    let file = tsc_rs_parser::parse("test.ts", source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        ..CompilerOptions::default()
    };
    let js = emit(&file, &options).javascript;
    assert!(js.contains("tokens: Gar[]"), "unexpected emit: {js}");
    assert!(js.contains("endState: state"), "unexpected emit: {js}");
    assert!(!js.contains("tokens: Gar[,"), "unexpected emit: {js}");
    assert!(!js.contains("state;\n"), "unexpected emit: {js}");
}

#[test]
fn test_nested_array_object_literal_indentation_matches_tsc() {
    let source = "type Style = StyleBase | StyleArray;\ninterface StyleArray extends Array<Style> {}\ninterface StyleBase {\n    foo: string;\n}\n\nconst blah: Style = [\n    [[{\n        foo: 'asdf',\n        jj: 1 // intentional error\n    }]]\n];\n";
    let file = tsc_rs_parser::parse("test.ts", source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        ..CompilerOptions::default()
    };
    let js = emit(&file, &options).javascript;
    assert!(
        js.contains(
            "    [[{\n                foo: 'asdf',\n                jj: 1 // intentional error\n            }]]"
        ),
        "unexpected emit: {js}"
    );
}

// ---------------------------------------------------------------
// Control flow tests
// ---------------------------------------------------------------

#[test]
fn test_for_of_with_typed_var() {
    let js = emit_ts("for (const item of items) { console.log(item); }");
    assert!(
        js.contains("for (const item of items)"),
        "for-of should be emitted correctly: {js}"
    );
}

#[test]
fn es5_indexed_for_of_uses_typescript_temp_names_for_literals_and_identifiers() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    for source in [
        "for (var v of \"\") {}",
        "for (var v of [true]) {}",
        "for (const v of 0) {}",
    ] {
        let js = emit_ts_with(source, options.clone());
        assert!(
            js.contains("for (var _i = 0, _a = ")
                && js.contains("; _i < _a.length; _i++) {")
                && js.contains("var v = _a[_i];"),
            "anonymous RHS should use the ordinary temp sequence: {js}"
        );
    }

    for (binding, source) in [
        ("union_1", "var union; for (let v of union) {}"),
        ("tuple_1", "var tuple; for (var v of tuple) {}"),
    ] {
        let js = emit_ts_with(source, options.clone());
        assert!(
            js.contains(&format!("for (var _i = 0, {binding} ="))
                && js.contains(&format!("_i < {binding}.length; _i++"))
                && js.contains(&format!("var v = {binding}[_i];")),
            "identifier RHS should use a derived temp: {js}"
        );
    }
}

#[test]
fn es5_indexed_for_of_temp_planning_is_ordered_nested_and_collision_safe() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let sequential = emit_ts_with("for(var a of []){} for(var b of []){}", options.clone());
    assert!(
        sequential.contains("for (var _i = 0, _a = [];")
            && sequential.contains("for (var _b = 0, _c = [];")
            && !sequential.contains("arr_"),
        "sequential loops should share the scoped temp sequence: {sequential}"
    );

    let nested = emit_ts_with("for(var a of []){for(var b of []){}}", options.clone());
    assert!(
        nested.contains("for (var _i = 0, _a = [];")
            && nested.contains("for (var _b = 0, _c = [];")
            && nested.contains("var b = _c[_b];"),
        "nested loops should reserve outer names before inner emission: {nested}"
    );

    let derived = emit_ts_with(
        "var xs=[]; for(var a of xs){} for(var b of xs){} for(var c of []){}",
        options.clone(),
    );
    assert!(
        derived.contains("for (var _i = 0, xs_1 = xs;")
            && derived.contains("for (var _a = 0, xs_2 = xs;")
            && derived.contains("for (var _b = 0, _c = [];")
            && !derived.contains("xs_3 = xs"),
        "derived names should not consume the ordinary temp sequence: {derived}"
    );

    let collisions = emit_ts_with(
        "var _i,_a,xs_1,xs_2,xs=[]; for(var a of xs){} for(var b of []){}",
        options,
    );
    assert!(
        collisions.contains("for (var _b = 0, xs_3 = xs;")
            && collisions.contains("for (var _c = 0, _d = [];")
            && collisions.contains("var a = xs_3[_b];"),
        "source reservations should advance both name families: {collisions}"
    );
}

#[test]
fn es5_for_of_simple_declaration_patterns_use_compact_declarators() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    for (kind, pattern, expected) in [
        ("const", "[a, b]", "var _a = X_1[_i], a = _a[0], b = _a[1];"),
        ("const", "{a, b}", "var _a = X_1[_i], a = _a.a, b = _a.b;"),
        ("let", "{a, b}", "var _a = X_1[_i], a = _a.a, b = _a.b;"),
        ("let", "[a, b]", "var _a = X_1[_i], a = _a[0], b = _a[1];"),
        ("var", "[a, b]", "var _a = X_1[_i], a = _a[0], b = _a[1];"),
        ("var", "{a, b}", "var _a = X_1[_i], a = _a.a, b = _a.b;"),
    ] {
        let javascript = emit_ts_with(
            &format!("for ({kind} {pattern} of X) {{}}"),
            options.clone(),
        );
        assert!(
            javascript.contains(expected),
            "{kind} {pattern}: {javascript}"
        );
    }

    for (source, expected) in [
        ("for (var of X) {}", "var _a = X_1[_i];"),
        ("for (var of of) {}", "var _a = of_1[_i];"),
    ] {
        let javascript = emit_ts_with(source, options.clone());
        assert!(javascript.contains(expected), "{javascript}");
        assert!(!javascript.contains("<error>"), "{javascript}");
    }
}

#[test]
fn es5_for_of_simple_declaration_patterns_are_single_eval_and_collision_safe() {
    let javascript = emit_ts_with(
        "var _a=10,calls=0,out=[];function rows(){calls++;return [[1,2],[3,4]]}for(const [a,b] of rows())out.push(a+b);for(let {x,y:z} of [{x:5,y:6}])out.push(x+z);console.log(_a+'|'+calls+'|'+out.join(','));",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(
        javascript.contains("var _c = _b[_i], a = _c[0], b = _c[1];"),
        "{javascript}"
    );
    assert!(
        javascript.contains("x = ") && javascript.contains(".x, z = "),
        "{javascript}"
    );
    assert_eq!(
        execute_with_node(&javascript),
        "10|1|3,7,11",
        "{javascript}"
    );
}

#[test]
fn es5_for_of_simple_array_binding_uses_read_only_for_iterator_lowering() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        down_level_iteration: Some(true),
        ..Default::default()
    };
    let shape = emit_ts_with("for(const [a,b] of iterable){}", options.clone());
    assert!(
        shape.contains("var _b = __read(arr_1_1.value, 2), a = _b[0], b = _b[1];"),
        "{shape}"
    );
    let array = emit_ts_with(
        "var log=[];var iterable={[Symbol.iterator]:function(){var done=false;return{next:function(){return done?{done:true}:(done=true,{value:new Set([2,3]),done:false})},return:function(){log.push('close');return{done:true}}}}};outer:for(const [a,b] of iterable){log.push(a+b);break outer}console.log(log.join(','));",
        options.clone(),
    );
    assert!(array.contains("var __read ="), "{array}");
    assert!(
        array.contains(" = __read(") && array.contains(".value, 2)"),
        "{array}"
    );
    assert_eq!(execute_with_node(&array), "5,close", "{array}");

    let object = emit_ts_with(
        "var out=[];for(const {a,b} of [{a:2,b:4}])out.push(a+b);console.log(out.join(','));",
        options,
    );
    assert!(!object.contains("__read"), "{object}");
    assert!(
        object.contains("var _b = arr_1_1.value, a = _b.a, b = _b.b;"),
        "{object}"
    );
    assert_eq!(execute_with_node(&object), "6", "{object}");
}

#[test]
fn es5_for_of_dli_simple_binding_discovery_matches_emission_eligibility() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        down_level_iteration: Some(true),
        ..Default::default()
    };
    let initialized = emit_ts_with("for(const [a] = seed of rows){}", options.clone());
    assert!(!initialized.contains("__read"), "{initialized}");
    assert!(initialized.contains("var [a] = "), "{initialized}");

    let trailing_comment = emit_ts_with(
        "for(const [a] /* trailing pattern comment */ of rows){}",
        options.clone(),
    );
    assert!(!trailing_comment.contains("__read"), "{trailing_comment}");
    assert!(
        trailing_comment.contains("var [a] /* trailing pattern comment */ = arr_1_1.value;"),
        "{trailing_comment}"
    );
    assert_eq!(
        trailing_comment
            .matches("/* trailing pattern comment */")
            .count(),
        1,
        "{trailing_comment}"
    );

    let keyword_comment = emit_ts_with(
        "for(const /* before pattern */ [a] of rows){}",
        options.clone(),
    );
    assert!(!keyword_comment.contains("__read"), "{keyword_comment}");
    assert!(
        keyword_comment.contains("var [a] /* before pattern */ = arr_1_1.value;"),
        "{keyword_comment}"
    );
    assert_eq!(
        keyword_comment.matches("/* before pattern */").count(),
        1,
        "{keyword_comment}"
    );

    let line_comment = emit_ts_with("for(const [a,b] // keep me\n of rows){}", options.clone());
    assert!(!line_comment.contains("__read"), "{line_comment}");
    assert_eq!(
        line_comment.matches("// keep me").count(),
        1,
        "{line_comment}"
    );
    assert!(
        line_comment.contains("        // keep me\n        var [a, b] = arr_1_1.value;"),
        "{line_comment}"
    );
    assert_node_syntax(&line_comment);

    let recovery_source = "for(var of X){} var broken: = 1; for(const [a,b] of rows){}";
    let recovery_ast = tsc_rs_parser::parse("test.ts", recovery_source);
    assert!(!recovery_ast.diagnostics.is_empty(), "diagnostic control");
    let recovery_file = emit(&recovery_ast, &options.clone()).javascript;
    assert!(!recovery_file.contains("__read"), "{recovery_file}");
    assert!(recovery_file.contains("var [a, b] = "), "{recovery_file}");
    assert_node_syntax(&recovery_file);

    let globally_unsupported_plan = emit_ts_with(
        "var flag=false,out=[];if(flag){function f(){}}for(const [a,b] of [new Set([2,3])])out.push(a+b);console.log(out.join(','));",
        options,
    );
    assert!(
        globally_unsupported_plan.contains("var __read ="),
        "{globally_unsupported_plan}"
    );
    assert!(
        globally_unsupported_plan.contains(" = __read("),
        "{globally_unsupported_plan}"
    );
    assert_node_syntax(&globally_unsupported_plan);
    assert_eq!(
        execute_with_node(&globally_unsupported_plan),
        "5",
        "{globally_unsupported_plan}"
    );
}

#[test]
fn es5_for_of_simple_binding_lowering_fails_closed_for_other_header_forms() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let assignments = emit_ts_with(
        "var pair=[0,0],holder={value:0};for([pair[0],pair[1]] of [[1,2]]){}for(holder.value of [3]){}console.log(pair.join(',')+'|'+holder.value);",
        options.clone(),
    );
    assert!(
        assignments.contains("[pair[0],pair[1]] = "),
        "{assignments}"
    );
    assert!(assignments.contains("holder.value = "), "{assignments}");
    assert_eq!(execute_with_node(&assignments), "1,2|3", "{assignments}");

    let captured = emit_ts_with(
        "var fs=[];for(let [a,b] of [[1,2],[3,4]])fs.push(()=>a+b);console.log(fs.map(f=>f()).join(','));",
        options.clone(),
    );
    assert!(captured.contains("var _loop_"), "{captured}");
    assert_eq!(execute_with_node(&captured), "3,7", "{captured}");

    let unsupported = emit_ts_with(
        "for(const [a=1,...rest] of rows){}for(const [x /*keep*/, y] of rows){}for(const {a:{b}} of rows){}",
        options,
    );
    assert!(
        unsupported.contains("var [a = 1, ...rest] = "),
        "{unsupported}"
    );
    assert!(unsupported.contains("/*keep*/"), "{unsupported}");
    assert!(unsupported.contains("var [x, y] = "), "{unsupported}");
    assert!(unsupported.contains("{ a: { b } }"), "{unsupported}");
}

#[test]
fn es5_indexed_for_of_temp_planning_resets_per_function_and_shares_rest_index() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::ESNext),
        ..Default::default()
    };
    let functions = emit_ts_with(
        "function f(){for(var x of []){}} function g(){for(var y of []){}}",
        options.clone(),
    );
    assert_eq!(
        functions.matches("for (var _i = 0, _a = [];").count(),
        2,
        "each function should start a fresh generated-name scope: {functions}"
    );

    let rest = emit_ts_with(
        "function f(...rest){for(var x of []){} return rest.length}",
        options.clone(),
    );
    assert!(
        rest.contains("for (var _i = 0; _i < arguments.length; _i++)")
            && rest.contains("for (var _a = 0, _b = [];")
            && rest.contains("var x = _b[_a];"),
        "rest copying and for-of must reserve from one scoped sequence: {rest}"
    );

    let collided_rest = emit_ts_with(
        "function f(_i,...rest){for(var x of []){} return rest.length}",
        options.clone(),
    );
    assert!(
        collided_rest.contains("for (var _a = 1; _a < arguments.length; _a++)")
            && collided_rest.contains("rest[_a - 1] = arguments[_a];")
            && collided_rest.contains("for (var _b = 0, _c = [];")
            && collided_rest.contains("var x = _c[_b];"),
        "an `_i` parameter should move the rest loop into the ordinary sequence exactly once: {collided_rest}"
    );

    let defaulted = emit_ts_with(
        "function f(value=0,...rest){for(var x of []){} return value+rest.length}",
        options.clone(),
    );
    assert!(
        defaulted.contains("for (var _i = 1; _i < arguments.length; _i++)")
            && defaulted.contains("for (var _a = 0, _b = [];")
            && defaulted.contains("var x = _b[_a];"),
        "default-parameter lowering must not pre-consume a rest-loop temp: {defaulted}"
    );

    let destructured = emit_ts_with(
        "function f({a,...rest}){for(var x of []){} return rest}",
        options.clone(),
    );
    assert!(
        destructured.contains("function f(_a)")
            && destructured.contains("for (var _i = 0, _b = [];")
            && destructured.contains("var x = _b[_i];"),
        "a real parameter temp should keep its ordinary slot while leaving `_i` available: {destructured}"
    );

    let class_floor = emit_ts_with(
        "class C { static #value=1; m(){for(var x of []){}} }",
        options.clone(),
    );
    assert!(
        class_floor.contains("var _a, _C_value;")
            && class_floor.contains("for (var _i = 0, _b = [];")
            && class_floor.contains("var x = _b[_i];"),
        "class-reserved `_a` must remain below the special loop index and its RHS: {class_floor}"
    );

    let collided_class_floor = emit_ts_with(
        "class C { static #value=1; m(_i){for(var x of []){}} }",
        options,
    );
    assert!(
        collided_class_floor.contains("var _a, _C_value;")
            && collided_class_floor.contains("for (var _b = 0, _c = [];")
            && collided_class_floor.contains("var x = _c[_b];"),
        "an `_i` collision must continue after the class-reserved `_a`: {collided_class_floor}"
    );
}

#[test]
fn es5_indexed_for_of_preserves_runtime_and_source_map_anchors() {
    let source = "var calls=0,sum=0; function get(){calls++;return [[2],[3]]} for(var row of get()){sum+=row[0]} console.log(calls+':'+sum);";
    let javascript = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(
        javascript.contains("for (var _i = 0, _a = get();")
            && javascript.contains("var row = _a[_i];"),
        "loop binding should read from the single-evaluation cache: {javascript}"
    );
    assert_eq!(execute_with_node(&javascript), "1:5", "{javascript}");

    let mapped_source = "var a=[1];\nfor (var v of a) { console.log(v); }\n";
    let file = tsc_rs_parser::parse("mapped.ts", mapped_source);
    let emitted = emit(
        &file,
        &CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            source_map: Some(true),
            ..Default::default()
        },
    );
    assert!(
        emitted
            .javascript
            .contains("for (var _i = 0, a_1 = a; _i < a_1.length; _i++)"),
        "mapped identifier RHS should use its derived cache: {}",
        emitted.javascript
    );
    let mappings = decode_source_map_mappings(emitted.source_map.as_deref().expect("source map"));
    assert_source_map_columns_within_output(&emitted.javascript, &mappings);
    for original_column in [0, 14, 19] {
        assert!(
            mappings
                .iter()
                .any(|mapping| mapping.original_line == 1
                    && mapping.original_column == original_column),
            "missing original for-of anchor at column {original_column}: {mappings:?}"
        );
    }
}

#[test]
fn indexed_for_of_temp_planning_does_not_change_modern_or_iterator_paths() {
    let source = "for (let v of values) { use(v); }";
    let modern = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(modern.contains("for (let v of values)"), "{modern}");
    assert!(!modern.contains("_i = 0"), "{modern}");

    let iterator = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert!(iterator.contains("__values(values)"), "{iterator}");
    assert!(iterator.contains("for (var arr_1 ="), "{iterator}");
    assert!(!iterator.contains("for (var _i = 0"), "{iterator}");
}

#[test]
fn test_try_catch() {
    let js = emit_ts("try { throw new Error(); } catch (e) { console.log(e); }");
    assert!(js.contains("try {"), "should emit try: {js}");
    assert!(js.contains("catch (e)"), "should emit catch: {js}");
}

// ---------------------------------------------------------------
// Edge case tests
// ---------------------------------------------------------------

#[test]
fn test_empty_file() {
    let js = emit_ts("");
    let trimmed = js.trim();
    assert_eq!(
        trimmed, "\"use strict\";",
        "empty file should just have use strict: {js}"
    );
}

#[test]
fn test_this_parameter_stripped() {
    let js = emit_ts("function handler(this: void, e: Event) { }");
    assert!(
        !js.contains("this:") && !js.contains("this,"),
        "'this' parameter should be stripped: {js}"
    );
}

#[test]
fn test_this_parameter_stripped_arrow() {
    let js = emit_ts("var f = (this: number, m: number) => m + 1;");
    assert!(
        !js.contains("(this") && js.contains("(m)"),
        "'this' parameter should be stripped from arrow function: {js}"
    );
}

#[test]
fn test_this_bare_parameter_stripped_arrow() {
    let js = emit_ts("var f = (this, m) => m + 1;");
    assert!(
        !js.contains("(this") && js.contains("(m)"),
        "bare 'this' parameter should be stripped from arrow function: {js}"
    );
}

#[test]
fn test_optional_parameter() {
    let js = emit_ts("function foo(x?: number) { }");
    assert!(
        js.contains("function foo(x)"),
        "optional param should strip ? and type: {js}"
    );
}

#[test]
fn test_non_module_file() {
    let js = emit_ts("const x = 1;");
    assert!(
        js.contains("\"use strict\";"),
        "non-module file should have use strict: {js}"
    );
    assert!(
        !js.contains("__esModule"),
        "non-module file should not have __esModule: {js}"
    );
}

// ---------------------------------------------------------------
// "use strict" directive handling tests
// ---------------------------------------------------------------

#[test]
fn test_cjs_module_emits_use_strict() {
    // CommonJS module files always get "use strict"
    let js = emit_ts_with(
        "export const x = 1;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.starts_with("\"use strict\";\n"),
        "CJS module should start with use strict: {js}"
    );
}

#[test]
fn test_cjs_script_without_directive_emits_use_strict() {
    // Non-module (script) files get "use strict" to match TypeScript's default behavior.
    let js = emit_ts("const x = 1;");
    assert!(
        js.contains("\"use strict\""),
        "CJS script should have use strict: {js}"
    );
}

#[test]
fn test_cjs_script_with_directive_preserves_use_strict() {
    // CJS non-module (script) files with "use strict" in source should preserve it
    let js = emit_ts("\"use strict\";\nconst x = 1;");
    assert!(
        js.contains("\"use strict\""),
        "CJS script with directive should preserve use strict: {js}"
    );
}

#[test]
fn test_esm_module_no_use_strict() {
    // ES module files never get "use strict" (ES modules are implicitly strict)
    let js = emit_ts_esm("export const x = 1;");
    assert!(
        !js.contains("\"use strict\""),
        "ESM module should not have use strict: {js}"
    );
}

#[test]
fn test_esm_script_without_directive_emits_use_strict() {
    // Non-module (script) files get "use strict" regardless of module setting.
    let js = emit_ts_esm("const x = 1;");
    assert!(
        js.contains("\"use strict\""),
        "ESM script should have use strict: {js}"
    );
}

#[test]
fn test_esm_script_with_directive_preserves_use_strict() {
    // ESM non-module (script) files with "use strict" in source should preserve it
    let js = emit_ts_esm("\"use strict\";\nconst x = 1;");
    assert!(
        js.contains("\"use strict\""),
        "ESM script with directive should preserve use strict: {js}"
    );
}

#[test]
fn test_cjs_module_with_source_directive_no_duplicate() {
    // CJS module files that also have "use strict" in source should not duplicate it
    let js = emit_ts_with(
        "\"use strict\";\nexport const x = 1;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    let count = js.matches("use strict").count();
    assert_eq!(
        count, 1,
        "CJS module with source directive should have exactly one use strict: {js}"
    );
}

#[test]
fn test_cts_forces_commonjs_emit_under_esnext_option() {
    let js = emit_ts_file_with(
        "index.cts",
        "const a = 2;",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert!(
        js.starts_with("\"use strict\";\n"),
        ".cts should emit CommonJS prologue: {js}"
    );
    assert!(
        js.contains("Object.defineProperty(exports, \"__esModule\", { value: true });"),
        ".cts should emit __esModule marker: {js}"
    );
    assert!(
        !js.contains("export {};"),
        ".cts should not emit ESM module marker: {js}"
    );
}

#[test]
fn test_mts_forces_esm_emit_under_commonjs_option() {
    let js = emit_ts_file_with(
        "index.mts",
        "const a = 2;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    // .mts with project-level CJS still gets "use strict" (tsc behavior)
    assert!(
        js.contains("\"use strict\";"),
        ".mts + CJS project should emit 'use strict': {js}"
    );
    // But it should NOT get CJS wrappers (Object.defineProperty, exports, etc.)
    assert!(
        !js.contains("Object.defineProperty"),
        ".mts should not emit CJS Object.defineProperty: {js}"
    );
}

#[test]
fn test_cjs_export_qualification_respects_arrow_param_shadow() {
    // Regression: a parameter that shadows an exported binding must NOT be
    // qualified to `exports.x`. Arrow params were never tracked in
    // `cjs_param_shadows`, so `export const v = ...; (v) => (n) => n < v`
    // mis-emitted `n < exports.v` (→ `n < NaN` at runtime, silently wrong).
    let js = emit_ts_file_with(
        "m.ts",
        "export const v = { t: 1 };\n\
         export function mk() { return { min: (v) => (n) => (n < v ? 1 : 0) }; }",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("n < v"),
        "arrow param must shadow the exported const: {js}"
    );
    assert!(
        !js.contains("n < exports.v"),
        "shadowing arrow param v must NOT be qualified to exports.v: {js}"
    );
}

#[test]
fn test_cjs_export_qualification_respects_nested_fn_param_shadow() {
    // A name shadowed by an OUTER function's parameter stays that param inside
    // a nested function — the shadow set must be the UNION of enclosing param
    // scopes, not replaced on each function entry.
    let js = emit_ts_file_with(
        "m.ts",
        "export const x = 1;\n\
         export function outer(x) { return function inner() { return x; }; }",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("return exports.x"),
        "x shadowed by outer's param must NOT be qualified inside inner: {js}"
    );
}

#[test]
fn test_elided_import_comment_is_not_attached_to_cjs_prologue() {
    let js = emit_ts_file_with(
        "index.cts",
        "import dedent from \"dedent\"; // Error\n",
        CompilerOptions {
            module: Some(ModuleKind::NodeNext),
            ..Default::default()
        },
    );
    assert!(
        js.contains("Object.defineProperty(exports, \"__esModule\", { value: true });"),
        "CJS prologue should be emitted: {js}"
    );
    assert!(
        !js.contains("// Error"),
        "trailing comment on erased import should not be attached to prologue: {js}"
    );
}

#[test]
fn test_node_esm_import_require_uses_create_require_bridge() {
    let js = emit_ts_file_with(
        "index.ts",
        "import foo = require(\"foo\");\nfoo;\n",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            target: Some(ScriptTarget::ES2020),
            other: vec![("__tsrsNodeEsmImportRequire".to_string(), "true".to_string())],
            ..Default::default()
        },
    );
    assert!(
        js.contains("import { createRequire as _createRequire } from \"module\";"),
        "missing createRequire import: {js}"
    );
    assert!(
        js.contains("const __require = _createRequire(import.meta.url);"),
        "missing __require bridge: {js}"
    );
    assert!(
        js.contains("const foo = __require(\"foo\");"),
        "import=require should use __require: {js}"
    );
}

#[test]
fn test_node_esm_import_require_helper_name_collision_suffixes() {
    let js = emit_ts_file_with(
        "index.ts",
        "const __require = null;\nconst _createRequire = null;\nimport foo = require(\"foo\");\nfoo;\n",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            target: Some(ScriptTarget::ES2020),
            other: vec![(
                "__tsrsNodeEsmImportRequire".to_string(),
                "true".to_string(),
            )],
            ..Default::default()
        },
    );
    assert!(
        js.contains("import { createRequire as _createRequire_1 } from \"module\";"),
        "createRequire helper should avoid local collision: {js}"
    );
    assert!(
        js.contains("const __require_1 = _createRequire_1(import.meta.url);"),
        "__require helper should avoid local collision: {js}"
    );
    assert!(
        js.contains("const foo = __require_1(\"foo\");"),
        "import=require should use suffixed helper: {js}"
    );
}

#[test]
fn test_overload_signatures_stripped() {
    let js = emit_ts(
        "function foo(x: number): number;\nfunction foo(x: string): string;\nfunction foo(x: any): any { return x; }",
    );
    let count = js.matches("function foo").count();
    assert_eq!(
        count, 1,
        "should only emit implementation, not overloads: {js}"
    );
}

#[test]
fn test_import_type_specifier_stripped() {
    let js = emit_ts_with(
        "import { type Foo, bar } from './mod';\nconsole.log(bar);",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("require(\"./mod\")"),
        "should still require the module: {js}"
    );
}

#[test]
fn test_enum_in_export() {
    let js = emit_ts_with(
        "export enum Dir { Up, Down }",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var Dir;"),
        "exported enum should have var declaration: {js}"
    );
    assert!(
        js.contains("exports.Dir = Dir"),
        "enum should be exported in CJS: {js}"
    );
}

#[test]
fn test_switch_statement() {
    let js = emit_ts("switch (x) { case 1: break; default: break; }");
    assert!(js.contains("switch (x)"), "should emit switch: {js}");
    assert!(js.contains("case 1:"), "should emit case: {js}");
    assert!(js.contains("default:"), "should emit default: {js}");
}

#[test]
fn test_labeled_statement() {
    let js = emit_ts("outer: for (;;) { break outer; }");
    assert!(js.contains("outer:"), "should emit label: {js}");
    assert!(
        js.contains("break outer;"),
        "should emit labeled break: {js}"
    );
}

#[test]
fn test_do_while() {
    let js = emit_ts("do { x++; } while (x < 10);");
    assert!(js.contains("do {"), "should emit do: {js}");
    assert!(js.contains("while (x < 10)"), "should emit while: {js}");
}

#[test]
fn test_debugger_statement() {
    let js = emit_ts("debugger;");
    assert!(js.contains("debugger;"), "should emit debugger: {js}");
}

#[test]
fn test_throw_statement() {
    let js = emit_ts("throw new Error(\"fail\");");
    assert!(
        js.contains("throw new Error(\"fail\")"),
        "should emit throw: {js}"
    );
}

#[test]
fn test_cjs_export_named_specifiers() {
    let js = emit_ts_with(
        "const a = 1; const b = 2; export { a, b };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.b = exports.a = void 0;"),
        "should emit pre-declaration chain: {js}"
    );
    assert!(
        js.contains("exports.a = a;"),
        "should emit post-declaration export assignment for a: {js}"
    );
    assert!(
        js.contains("exports.b = b;"),
        "should emit post-declaration export assignment for b: {js}"
    );
}

#[test]
fn test_cjs_export_named_specifier_alias_assignment() {
    let js = emit_ts_with(
        "const a = 1; export { a as renamed };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.renamed = void 0;"),
        "should predeclare aliased export: {js}"
    );
    assert!(
        js.contains("exports.renamed = a;"),
        "should assign aliased export to local binding: {js}"
    );
}

#[test]
fn test_cjs_export_named_function_specifier_no_duplicate_assignment() {
    let js = emit_ts_with(
        "function fn() {} export { fn };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    let assign_count = js.matches("exports.fn = fn;").count();
    assert_eq!(
        assign_count, 1,
        "function export assignment should be emitted once: {js}"
    );
    assert!(
        !js.contains("exports.fn = void 0;"),
        "function exports should not get void 0 pre-declaration: {js}"
    );
}

#[test]
fn test_amd_export_named_specifiers_post_assignment() {
    let js = emit_ts_with(
        "const a = 1; export { a };",
        CompilerOptions {
            module: Some(ModuleKind::AMD),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.a = a;"),
        "AMD emit should assign local named export after declarations: {js}"
    );
}

#[test]
fn test_amd_emits_rest_helper_when_object_rest_is_transformed() {
    let js = emit_ts_with(
        "const obj = { a: 1, b: 2 }; const { a, ...rest } = obj; export { rest };",
        CompilerOptions {
            module: Some(ModuleKind::AMD),
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __rest = (this && this.__rest) || function (s, e) {"),
        "AMD module should emit __rest helper when needed: {js}"
    );
}

#[test]
fn test_amd_es5_extends_helper_precedes_wrapper_once() {
    let js = emit_ts_with(
        "export class Base {} class Derived extends Base {}",
        CompilerOptions {
            module: Some(ModuleKind::AMD),
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    let helper = js
        .find("var __extends = ")
        .expect("missing __extends helper");
    let wrapper = js.find("define(").expect("missing AMD wrapper");
    assert!(helper < wrapper, "helper must precede AMD define: {js}");
    assert_eq!(
        js.matches("var __extends = ").count(),
        1,
        "duplicate helper: {js}"
    );
    assert!(
        js.contains("__extends(Derived, _super);"),
        "missing helper call: {js}"
    );
}

#[test]
fn test_amd_es5_no_emit_helpers_keeps_external_extends_call() {
    let js = emit_ts_with(
        "export class Base {} class Derived extends Base {}",
        CompilerOptions {
            module: Some(ModuleKind::AMD),
            target: Some(ScriptTarget::ES5),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("var __extends = "),
        "helper should be suppressed: {js}"
    );
    assert!(
        js.contains("__extends(Derived, _super);"),
        "external call missing: {js}"
    );
}

#[test]
fn test_es5_ambient_classes_do_not_request_extends_helper() {
    let js = emit_ts_with(
        "export {}; declare class C {} declare class D extends C {} declare namespace N { class A {} class B extends A {} }",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES5),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("__extends"),
        "ambient-only classes must not request a runtime helper: {js}"
    );
}

#[test]
fn test_es5_legacy_constructor_super_runtime_controls() {
    let direct = emit_ts_with(
        "class Base { constructor(value: number) { this.value = value; } }\n\
         class Derived extends Base { constructor(value: number) { super(value); this.extra = value + 1; } }\n\
         const value = new Derived(4); console.log(value.value, value.extra);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&direct);
    assert_eq!(execute_with_node(&direct), "4 5");
    assert!(
        direct.contains("var _this = _super.call(this, value) || this;"),
        "ordinary super call should initialize the receiver once: {direct}"
    );

    let retry = emit_ts_with(
        "class Base { constructor(fail: boolean) { if (fail) throw new Error('retry'); this.ok = true; } }\n\
         class Derived extends Base { constructor() { try { super(true); } catch (e) { super(false); } } }\n\
         console.log(new Derived().ok);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&retry);
    assert_eq!(execute_with_node(&retry), "true");
    assert_eq!(retry.matches("_this = _super.call(this").count(), 2);
}

#[test]
fn test_es5_legacy_constructor_preserves_return_semantics() {
    let js = emit_ts_with(
        "class Base { constructor() { return { token: 7 }; } }\n\
         class Bare extends Base { constructor() { super(); return; } }\n\
         class Explicit extends Base { constructor() { super(); return { token: 9 }; } }\n\
         class Branched extends Base { constructor(useBase: boolean) { if (useBase) return super(); super(); } }\n\
         console.log(new Bare().token, new Explicit().token, new Branched(true).token, new Branched(false).token);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "7 9 7 7");
    assert!(
        js.contains("return _this;"),
        "bare return must return the captured receiver: {js}"
    );
    assert!(
        js.contains("return { token: 9 };") || js.contains("return {\n"),
        "explicit object return must remain explicit: {js}"
    );
}

#[test]
fn test_es5_legacy_class_names_are_collision_safe_when_nested_and_sequential() {
    let js = emit_ts_with(
        "class Base { constructor() { this.base = true; } }\n\
         class Outer extends Base { constructor(_super: any, _super_1: any, _this: any, _this_1: any) {\n\
             super();\n\
             const make = () => { class Inner extends Base { constructor() { super(); } } return new Inner(); };\n\
             this.inner = make();\n\
         } }\n\
         class Next extends Base { constructor() { super(); } }\n\
         const outer = new Outer(null, null, null, null);\n\
         console.log(outer.base, outer.inner.base, new Next().base);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "true true true");
    assert!(
        js.contains("function (_super_2)") && js.contains("var _this_2 = _super_2.call(this)"),
        "generated aliases must avoid source bindings: {js}"
    );
}

#[test]
fn test_es5_legacy_class_names_see_bindings_in_template_substitutions() {
    let js = emit_ts_with(
        "class Base {}\n\
         class Derived extends Base {\n\
             constructor() {\n\
                 const value = `raw _super:${((_super: any, _this: any) => super())(0, 0)}`;\n\
                 this.value = value;\n\
             }\n\
         }\n\
         console.log(new Derived().value);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "raw _super:[object Object]");
    assert!(
        js.contains("function (_super_1)") && js.contains("var _this_1 = this;"),
        "template substitutions must participate in alias collision discovery: {js}"
    );
    assert!(
        js.contains("raw _super:"),
        "raw template text must be preserved: {js}"
    );
}

#[test]
fn test_es5_legacy_class_names_ignore_regex_text_and_see_following_bindings() {
    let exact = emit_ts_with(
        "const rx = /`/; class B {} class D extends B { constructor(_super: number) { super(); this.value = _super; } } console.log(rx.test('`'), new D(7).value);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&exact);
    assert_eq!(execute_with_node(&exact), "true 7");
    assert!(
        exact.contains("function (_super_1)"),
        "exact regex collision repro must allocate a safe base alias: {exact}"
    );

    let js = emit_ts_with(
        "const tick = /`/g;\n\
         const quote = /\"/;\n\
         const escaped = /[\\/\"`]/u;\n\
         const division = 8 / 2;\n\
         class Base {}\n\
         class Derived extends Base {\n\
             constructor(_super: number, _this: number) {\n\
                 super();\n\
                 this.value = _super + _this;\n\
             }\n\
         }\n\
         console.log(tick.test('`'), quote.test('\"'), escaped.test('/'), division, new Derived(3, 4).value);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "true true true 4 7");
    assert!(
        js.contains("function (_super_1)")
            && js.contains("var _this_1 = this;")
            && js.contains("_this_1 = _super_1.call(this)"),
        "regex quote/backtick bytes must not hide later source bindings: {js}"
    );
    assert!(
        js.contains("8 / 2"),
        "division must remain distinct from regex scanning: {js}"
    );
}

#[test]
fn test_es5_extends_helper_discovery_reaches_expression_owned_bodies() {
    let js = emit_ts_with(
        "const arrow = () => { class Base {} class Child extends Base {} return new Child(); };\n\
         const functionExpr = function () { class Base {} class Child extends Base {} return new Child(); };\n\
         const object = { make() { class Base {} class Child extends Base {} return new Child(); } };\n\
         console.log(arrow() instanceof Object, functionExpr() instanceof Object, object.make() instanceof Object);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "true true true");
    assert_eq!(
        js.matches("var __extends = ").count(),
        1,
        "nested declarations need exactly one helper: {js}"
    );
}

#[test]
fn test_es5_class_expression_does_not_use_declaration_only_lowering() {
    let js = emit_ts_with(
        "const C = class Named { constructor() {} }; console.log(new C() instanceof C);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "true");
    assert!(
        !js.contains("= var Named"),
        "class expression must remain an expression: {js}"
    );
}

#[test]
fn test_es5_legacy_class_lowers_public_methods_and_accessors() {
    let js = emit_ts_with(
         "class Base { base() { return 1; } }\n\
         class C extends Base {\n\
             set value(v: number) { this._value = v; }\n\
             method(a: number) { return this.value + a + this.base(); }\n\
             static make(a: number) { const value = new C(); value.value = 3; return value.method(a); }\n\
             get value() { return this._value; }\n\
             static set answer(v: number) { C.last = v; }\n\
             static get answer() { return 42; }\n\
         }\n\
         C.answer = 9; console.log(C.make(2), C.answer, C.last);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "6 42 9");
    assert!(
        js.contains("Base.prototype.base = function () { return 1; };")
            && js.contains("C.prototype.method = function (a)")
            && js.contains("C.make = function (a)")
            && js.contains("Object.defineProperty(C.prototype, \"value\", {")
            && js.contains("get: function () { return this._value; },")
            && js.contains("set: function (v) { this._value = v; },")
            && js.contains("Object.defineProperty(C, \"answer\", {")
            && !js.contains("class Base")
            && !js.contains("class C"),
        "ordinary public members should use the ES5 class IIFE shape: {js}"
    );
    assert_eq!(
        js.matches("Object.defineProperty(C.prototype, \"value\", {")
            .count(),
        1,
        "identifier getter/setter pairs must remain coalesced: {js}"
    );
}

#[test]
fn test_es5_legacy_class_lowers_computed_members_in_source_order_once() {
    let js = emit_ts_with(
        "const order: string[] = [];\n\
         function key(name: string) { order.push(name); return name; }\n\
         class C {\n\
             [key('method')]() { return 1; }\n\
             static [key('staticMethod')]() { return 2; }\n\
             get [key('read')]() { return 3; }\n\
             set [key('write')](value: number) { this.seen = value; }\n\
         }\n\
         const c = new C(); c.write = 4;\n\
         console.log(order.join(','), c.method(), C.staticMethod(), c.read, c.seen);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(
        execute_with_node(&js),
        "method,staticMethod,read,write 1 2 3 4"
    );
    assert!(
        js.contains("C.prototype[key('method')] = function ()")
            && js.contains("C[key('staticMethod')] = function ()")
            && js.contains("Object.defineProperty(C.prototype, key('read'), {")
            && js.contains("Object.defineProperty(C.prototype, key('write'), {")
            && !js.contains("class C"),
        "computed instance/static members should use ordered ES5 assignments and descriptors: {js}"
    );
}

#[test]
fn test_es5_legacy_computed_accessors_keep_distinct_key_evaluations() {
    let js = emit_ts_with(
        "let calls = 0; function key() { calls++; return 'slot' + calls; }\n\
         class C { get [key()]() { return 1; } set [key()](value: number) { this.seen = value; } }\n\
         const one = Object.getOwnPropertyDescriptor(C.prototype, 'slot1');\n\
         const two = Object.getOwnPropertyDescriptor(C.prototype, 'slot2');\n\
         console.log(calls, typeof one.get, typeof one.set, typeof two.get, typeof two.set);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(
        execute_with_node(&js),
        "2 function undefined undefined function"
    );
    assert_eq!(
        js.matches("Object.defineProperty(C.prototype, key(), {")
            .count(),
        2,
        "computed getter/setter keys must not be text-coalesced: {js}"
    );
}

#[test]
fn test_es5_legacy_computed_member_comments_and_source_map_are_owned() {
    let js = emit_ts_file_with(
        "computed.ts",
        "class C {\n\
             // computed docs\n\
             get [\"value\"]() { return 1; }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            source_map: Some(true),
            ..Default::default()
        },
    );
    let descriptor = js
        .find("Object.defineProperty(C.prototype, \"value\", {")
        .unwrap_or_else(|| panic!("missing computed descriptor: {js}"));
    let docs = js.find("// computed docs").unwrap();
    let getter = js.find("get: function ()").unwrap();
    assert!(
        descriptor < docs && docs < getter,
        "a computed accessor's leading comment belongs inside its descriptor: {js}"
    );
    assert!(
        js.trim_end()
            .ends_with("//# sourceMappingURL=computed.js.map"),
        "computed class lowering must preserve the source-map trailer: {js}"
    );

    let erased_index_comment = emit_ts_with(
        "class C {\n\
             // index signature docs are type-only\n\
             [name: string]: unknown;\n\
             set [\"value\"](value: number) { this.seen = value; }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        erased_index_comment.contains("Object.defineProperty(C.prototype, \"value\", {")
            && !erased_index_comment.contains("index signature docs"),
        "type-only index comments must not migrate into a later descriptor: {erased_index_comment}"
    );
}

#[test]
fn test_es5_legacy_computed_member_lowering_keeps_unsafe_boundaries_native() {
    for (label, source) in [
        ("self key", "class C { [C.name]() {} }"),
        ("wrapped self key", "class C { [(C as any).name]() {} }"),
        ("asserted self key", "class C { [(<any>C).name]() {} }"),
        (
            "satisfies-wrapped self key",
            "class C { [(C satisfies any).name]() {} }",
        ),
        ("non-null self key", "class C { [C!.name]() {} }"),
        ("instantiated self key", "class C { [C<any>.name]() {} }"),
        ("outer this key", "class C { [this.key]() {} }"),
        (
            "wrapped outer this key",
            "class C { [(this as any).key]() {} }",
        ),
        (
            "nested lexical arrow key",
            "class C { [(0, () => this.key)]() {} }",
        ),
        (
            "object computed self key",
            "class C { [({ [C.name]: 1 })]() {} }",
        ),
        (
            "object spread outer this key",
            "class C { [({ ...this })]() {} }",
        ),
        (
            "complex arrow key",
            "class C { [(value: number) => value]() {} }",
        ),
        (
            "body comment",
            "class C { get ['value']() { /* body ownership */ return 1; } }",
        ),
        (
            "effectful field",
            "function key() { return 'value'; } class C { [key()] = 1; method() {} }",
        ),
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                ..Default::default()
            },
        );
        assert!(
            js.contains("class C"),
            "{label} must stay on the established native/field path: {js}"
        );
    }
}

#[test]
fn test_es5_cjs_default_class_uses_legacy_declaration_shape() {
    let source = "export default class C { method(value: number) { return value + 1; } }";

    for module in [ModuleKind::CommonJS, ModuleKind::UMD] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                ..Default::default()
            },
        );
        assert!(
            js.contains("var C = /** @class */ (function ()")
                && js.contains("C.prototype.method = function (value)")
                && js.contains("exports.default = C;")
                && !js.contains("class C"),
            "ES5 {module:?} default declarations should use the existing legacy class lowering: {js}"
        );
    }

    let anonymous = emit_ts_with(
        "export default class { method(value: number) { return value + 1; } }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        anonymous.contains("var default_1 = /** @class */ (function ()")
            && anonymous.contains("default_1.prototype.method = function (value)")
            && anonymous.contains("exports.default = default_1;")
            && !anonymous.contains("class default_1"),
        "the synthetic name must be installed before the local default modifier is normalized: {anonymous}"
    );
}

#[test]
fn test_cjs_default_class_lowering_keeps_target_and_module_boundaries() {
    let source = "export default class C { method(value: number) { return value + 1; } }";

    let modern_cjs = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(modern_cjs.contains("class C"), "{modern_cjs}");
    assert!(
        !modern_cjs.contains("var C = /** @class */"),
        "{modern_cjs}"
    );

    for module in [ModuleKind::ESNext, ModuleKind::System] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(module),
                ..Default::default()
            },
        );
        assert!(
            !js.contains("var C = /** @class */"),
            "the CJS-local modifier normalization must not enter {module:?} emit: {js}"
        );
    }

    let unsupported = emit_ts_with(
        "export default class C { [name()]() {} }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        unsupported.contains("class C"),
        "normalizing the export modifier must not relax the legacy class shape gate: {unsupported}"
    );

    let decorated = emit_ts_with(
        "declare function dec(value: unknown): unknown; @dec export default class C {}",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            experimental_decorators: Some(true),
            ..Default::default()
        },
    );
    assert!(decorated.contains("__decorate"), "{decorated}");
    assert!(
        !decorated.contains("var C = /** @class */"),
        "decorated declarations must stay on their established wrapper path: {decorated}"
    );

    let namespace_recovery = emit_ts_with(
        "namespace N { { export default class C {} } }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        namespace_recovery.contains("class C")
            && !namespace_recovery.contains("var C = /** @class */"),
        "namespace-IIFE recovery must retain its established native class path: {namespace_recovery}"
    );
}

#[test]
fn test_es5_cjs_default_class_computed_member_keeps_export_boundary_native() {
    let js = emit_ts_with(
        "declare function name(): string; export default class C { [name()]() {} }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("class C") && js.contains("exports.default = C;"),
        "a computed CommonJS default class must retain its native recovery boundary: {js}"
    );
}

#[test]
fn test_es5_cjs_default_class_boundary_does_not_leak_to_nested_class() {
    let js = emit_ts_with(
        "declare function name(): string;\
         export default class Outer {\
             [name()]() {\
                 class Inner { [name()]() {} }\
                 return Inner;\
             }\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("class Outer") && js.contains("exports.default = Outer;"),
        "the source default class must retain its native computed-member boundary: {js}"
    );
    assert!(
        js.contains("var Inner = /** @class */ (function ()")
            && js.contains("Inner.prototype[name()] = function ()")
            && !js.contains("class Inner"),
        "a nested computed class must not inherit the outer default-export boundary: {js}"
    );
}

#[test]
fn test_es5_legacy_class_member_lowering_keeps_unsupported_boundaries_native() {
    let super_member = emit_ts_with(
        "class Base { method() { return 1; } } class Derived extends Base { method() { return super.method() + 1; } } console.log(new Derived().method());",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&super_member);
    assert_eq!(execute_with_node(&super_member), "2");
    assert!(
        super_member.contains("class Derived extends Base"),
        "super-bearing methods require a home object and must not be moved to ordinary functions: {super_member}"
    );

    let define_field = emit_ts_with(
        "class C { value: number; method() { return this.value; } }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            use_define_for_class_fields: Some(true),
            ..Default::default()
        },
    );
    assert!(
        define_field.contains("class C") && define_field.contains("Object.defineProperty(this"),
        "define-semantics fields stay on the established ordering-aware path: {define_field}"
    );
}

#[test]
fn test_es5_legacy_class_member_lowering_requires_lowered_same_file_base() {
    let js = emit_ts_with(
        "class NativeBase { static {} initialized = 1; }\n\
         class LoweredDerived extends NativeBase { method() { return this.initialized + 1; } }\n\
         console.log(new LoweredDerived().method());",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "2");
    assert!(
        js.contains("class LoweredDerived extends NativeBase"),
        "a derived class with expanded members must not call an unlowered native base via apply: {js}"
    );

    let constructor_only = emit_ts_with(
        "class NativeBase { static {} initialized = 1; }\n\
         class ConstructorOnly extends NativeBase { constructor() { super(); } }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        constructor_only.contains("var ConstructorOnly = /** @class */ (function (_super)"),
        "the new dependency gate must not change established constructor-only lowering: {constructor_only}"
    );
}

#[test]
fn test_es5_legacy_reversed_accessor_comments_fail_closed_in_source_order() {
    let js = emit_ts_with(
        "class C {\n\
             // setter lead\n\
             set value(v: number) {\n\
                 /* setter body */ this._value = v;\n\
             } // setter trail\n\
             // getter lead\n\
             get value() {\n\
                 /* getter body */ return this._value;\n\
             } // getter trail\n\
         }\n\
         const c = new C(); c.value = 2; console.log(c.value);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "2");
    let setter_lead = js.find("// setter lead").unwrap();
    let setter = js.find("set value(v)").unwrap();
    let setter_body = js.find("/* setter body */").unwrap();
    let setter_trail = js.find("// setter trail").unwrap();
    let getter_lead = js.find("// getter lead").unwrap();
    let getter = js.find("get value()").unwrap();
    let getter_body = js.find("/* getter body */").unwrap();
    let getter_trail = js.find("// getter trail").unwrap();
    assert!(
        setter_lead < setter
            && setter < setter_body
            && setter_body < setter_trail
            && setter_trail < getter_lead
            && getter_lead < getter
            && getter < getter_body
            && getter_body < getter_trail,
        "comment ownership must remain in exact source order when a setter precedes its getter: {js}"
    );
    assert!(
        js.contains("class C") && !js.contains("Object.defineProperty(C.prototype, \"value\""),
        "comment-bearing reversed accessor pairs must stay native until fragment emission is isolated: {js}"
    );
}

#[test]
fn test_es5_legacy_getter_first_accessor_comments_fail_closed_exactly() {
    let js = emit_ts_with(
        "class C {\n\
             /** getter docs */\n\
             get value() {\n\
                 /* getter body */ return 3;\n\
             } // getter trail\n\
             /** setter docs */\n\
             set value(v: number) {\n\
                 /* setter body */ this.seen = v;\n\
             } // setter trail\n\
         }\n\
         const c = new C(); c.value = 4; console.log(c.value, c.seen);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), "3 4");
    let ordered = [
        "/** getter docs */",
        "get value()",
        "/* getter body */",
        "// getter trail",
        "/** setter docs */",
        "set value(v)",
        "/* setter body */",
        "// setter trail",
    ];
    let positions: Vec<usize> = ordered
        .iter()
        .map(|marker| {
            assert_eq!(
                js.matches(marker).count(),
                1,
                "comment marker must be emitted exactly once: {marker}\n{js}"
            );
            js.find(marker).unwrap()
        })
        .collect();
    assert!(
        positions.windows(2).all(|window| window[0] < window[1]),
        "getter-first comments must preserve exact TS source ownership order: {js}"
    );
    assert!(
        js.contains("class C") && !js.contains("Object.defineProperty(C.prototype, \"value\""),
        "comment-bearing getter-first pairs must remain native: {js}"
    );
}

#[test]
fn test_es5_legacy_single_accessor_comments_fail_closed_exactly() {
    let getter = emit_ts_with(
        "class Read {\n\
             /** getter docs */\n\
             get value() {\n\
                 /* getter body */ return 7;\n\
             } // getter trail\n\
         }\n\
         console.log(new Read().value);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&getter);
    assert_eq!(execute_with_node(&getter), "7");
    let getter_markers = [
        "/** getter docs */",
        "get value()",
        "/* getter body */",
        "// getter trail",
    ];
    let getter_positions: Vec<usize> = getter_markers
        .iter()
        .map(|marker| {
            assert_eq!(getter.matches(marker).count(), 1, "{marker}\n{getter}");
            getter.find(marker).unwrap()
        })
        .collect();
    assert!(
        getter_positions
            .windows(2)
            .all(|window| window[0] < window[1])
            && getter.contains("class Read")
            && !getter.contains("Object.defineProperty(Read.prototype, \"value\""),
        "single getter comments must remain exact and native: {getter}"
    );

    let setter = emit_ts_with(
        "class Write {\n\
             /** setter docs */\n\
             set value(v: number) {\n\
                 /* setter body */ this.seen = v;\n\
             } // setter trail\n\
         }\n\
         const write = new Write(); write.value = 9; console.log(write.seen);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&setter);
    assert_eq!(execute_with_node(&setter), "9");
    let setter_markers = [
        "/** setter docs */",
        "set value(v)",
        "/* setter body */",
        "// setter trail",
    ];
    let setter_positions: Vec<usize> = setter_markers
        .iter()
        .map(|marker| {
            assert_eq!(setter.matches(marker).count(), 1, "{marker}\n{setter}");
            setter.find(marker).unwrap()
        })
        .collect();
    assert!(
        setter_positions
            .windows(2)
            .all(|window| window[0] < window[1])
            && setter.contains("class Write")
            && !setter.contains("Object.defineProperty(Write.prototype, \"value\""),
        "single setter comments must remain exact and native: {setter}"
    );
}

#[test]
fn test_cjs_export_named_specifier_from_import_rewrites_reference() {
    let js = emit_ts_with(
        "import zzz from \"./b\";\nexport { zzz as default };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            es_module_interop: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.default = b_1.default;"),
        "named export assignment should use rewritten import reference: {js}"
    );
}

#[test]
fn test_cjs_export_default_identifier_uses_exports_binding() {
    let js = emit_ts_with(
        "export const zzz = 123;\nexport default zzz;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.default = exports.zzz;"),
        "default export of exported identifier should reference exports binding: {js}"
    );
}

#[test]
fn test_cjs_export_named_default_with_type_and_value_same_name() {
    let js = emit_ts_with(
        "export default interface zzz { x: string; }\n\
         import zzz from \"./b\";\n\
         const x: zzz = { x: \"\" };\n\
         zzz;\n\
         export { zzz as default };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            es_module_interop: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.default = b_1.default;"),
        "re-exporting default import should keep value binding even when type name matches: {js}"
    );
}

#[test]
fn test_cjs_export_named_default_from_import_is_emitted_near_import() {
    let js = emit_ts_with(
        "export default interface zzz { x: string; }\n\
         import zzz from \"./b\";\n\
         const x: zzz = { x: \"\" };\n\
         zzz;\n\
         export { zzz as default };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            es_module_interop: Some(true),
            ..Default::default()
        },
    );

    let export_pos = js
        .find("exports.default = b_1.default;")
        .expect("expected default export assignment");
    let const_pos = js
        .find("const x = { x: \"\" };")
        .expect("expected const initializer");

    assert!(
        export_pos < const_pos,
        "default export from imported binding should be emitted before later statements: {js}"
    );
}

#[test]
fn test_recover_function_body_from_parameter_list_parse_artifact() {
    let js = emit_ts_with(
        "function A(): (public B) => C {\n}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function A() {"),
        "should recover function body: {js}"
    );
    assert!(
        !js.contains("\nB;"),
        "should skip recovery debris statement B: {js}"
    );
    assert!(
        !js.contains("\nC;"),
        "should skip recovery debris statement C: {js}"
    );
}

#[test]
fn test_bodyless_function_recovery_does_not_synthesize_body_without_closed_params() {
    let js = emit_ts_with(
        "function f(a {\n}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(
        js, "\"use strict\";\n",
        "malformed parameter list should not synthesize a function body: {js}"
    );
}

#[test]
fn test_anonymous_bodyless_function_arrow_tail_is_skipped() {
    let js = emit_ts_with(
        "function (a => b;",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(
        js, "\"use strict\";\n",
        "same-line arrow recovery tail should be skipped with the malformed function: {js}"
    );
}

#[test]
fn test_multiline_reserved_word_enum_recovery_keeps_tail_statement() {
    let js = emit_ts_with(
        "enum void {\n}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var ;\n(function () {\n})( || ( = {}));\nvoid {};"),
        "multiline reserved-word enum recovery should keep the trailing statement: {js}"
    );
}

#[test]
fn test_malformed_import_attributes_double_comma_type_alias_emits_empty_stmt_tail() {
    let js = emit_ts_with(
        "export type Test = typeof import(\"./a.json\", {\n\
            with: {\n\
                type: \"json\",,\n\
            }\n\
        });",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.ends_with(";\n"),
        "malformed import attributes double-comma recovery should emit the leaked empty statement: {js}"
    );
}

#[test]
fn test_interface_incorrect_return_token_recovery_emits_tail_lines() {
    let js = emit_ts_file_with(
        "a.ts",
        "interface I {\n    a: {\n        toString: () => {\n            return 1;\n        };\n    }",
        CompilerOptions::default(),
    );
    assert!(
        js.contains("return 1;\n;"),
        "malformed interface return-token recovery should emit the leaked return tail: {js}"
    );
}

#[test]
fn test_variable_list_trailing_return_recovery_emits_tail_stmt() {
    let js = emit_ts_with(
        "var a,\nreturn;",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(js, "\"use strict\";\nvar a;\nreturn;\n");
}

#[test]
fn test_array_literal_split_closer_error_emits_empty_stmt() {
    let js = emit_ts_with(
        "var texCoords = [2, 2, 0.5000001192092895, 0.8749999 ; 403953552, 0.5000001192092895, 0.8749999403953552];",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        "\"use strict\";\nvar texCoords = [2, 2, 0.5000001192092895, 0.8749999];\n403953552, 0.5000001192092895, 0.8749999403953552;\n;\n"
    );
}

#[test]
fn test_string_literal_with_null_character_is_preserved() {
    let js = emit_ts_with(
        "\" \0 \";",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(js, "\"use strict\";\n\" \0 \";\n");
}

#[test]
fn test_es5_downlevels_braced_unicode_escapes_in_string_literals() {
    let js = emit_ts_with(
        r#"var x = "\u{0}\u{48}\u{FFFF}\u{10000}\u{D800}\u{DC00}\u{10FFFF}";"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains(r#"var x = "\0H\uFFFF\uD800\uDC00\uD800\uDC00\uDBFF\uDFFF";"#),
        "ES5 string emit should use ES5-compatible UTF-16 escapes: {js}"
    );
}

#[test]
fn test_es5_braced_unicode_string_escape_respects_quotes_and_escape_parity() {
    let js = emit_ts_with(
        r#"var a = "\\u{65}|\u{22}|\u{27}|\u{5C}|\u{0}1";
var b = '\u{27}|\u{22}|\u{5C}';"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains(r#"var a = "\\u{65}|\"|'|\\|\x001";"#),
        "double-quoted string escaping or escape parity changed: {js}"
    );
    assert!(
        js.contains(r#"var b = "'|\"|\\";"#),
        "transformed single-quoted strings should use canonical double quotes: {js}"
    );
}

#[test]
fn test_es5_braced_unicode_string_escape_canonicalizes_mixed_recovery_text() {
    let js = emit_ts_with(
        r#"var mixed = "\u{61}\u{110000}\u{62}";
var missing = "\u{61}\u{62";
var escaped = "\u{61}\\u{62}";
var odd = "\u{61}\\\u{62}";"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        concat!(
            "\"use strict\";\n",
            r#"var mixed = "a\\u{110000}b";"#,
            "\n",
            r#"var missing = "a\\u{62";"#,
            "\n",
            r#"var escaped = "a\\u{62}";"#,
            "\n",
            r#"var odd = "a\\b";"#,
            "\n",
        ),
        "mixed valid and recovery escapes should be emitted as valid ES5 strings"
    );
}

#[test]
fn test_es5_braced_unicode_string_escape_preserves_invalid_recovery_text() {
    let source = concat!(
        r#"var a = "\u{110000}";"#,
        "\n",
        r#"var b = "\u{FFFFFFFF}";"#,
        "\n",
        r#"var c = "\u{-DDDD}";"#,
        "\n",
        r#"var d = "\u{r}\u{}\u{67";"#,
    );
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        format!("\"use strict\";\n{source}\n"),
        "invalid and incomplete escapes should remain source-identical"
    );
}

#[test]
fn test_es5_downlevels_complete_escape_and_closes_unterminated_string_recovery() {
    let js = emit_ts_with(
        "var x = \"\\u{00000000000067}",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var x = \"g\";"),
        "complete escape should downlevel and close the recovered string: {js:?}"
    );
}

#[test]
fn test_es2015_preserves_braced_unicode_string_escape_spelling() {
    let source = r#"var x = "\u{00000000000067}\u{10FFFF}";"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        format!("\"use strict\";\n{source}\n"),
        "ES2015 emit should preserve the original braced escape spelling"
    );
}

#[test]
fn test_invalid_unicode_escape_string_reemits_header_comments_at_eof() {
    let js = emit_ts_with(
        "// Copyright 2009 the Sputnik authors.  All rights reserved.\n\
         // This code is governed by the BSD license found in the LICENSE file.\n\
         \n\
         \"\\u000G\"",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("\"\\u000G\";\n// Copyright 2009 the Sputnik authors.  All rights reserved.\n// This code is governed by the BSD license found in the LICENSE file.\n"),
        "invalid unicode escape recovery should re-emit the file header comments at EOF: {js:?}"
    );
}

#[test]
fn test_unterminated_block_comment_eof_recovery_keeps_padding_lines() {
    let js = emit_ts_with(
        "/*CHECK#1/\n\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("/*CHECK#1/\n") && js.ends_with("\n\n \n"),
        "unterminated block-comment recovery should keep the trailing padding line: {js:?}"
    );
}

#[test]
fn test_recover_static_signatures_inside_bodyless_function() {
    let js = emit_ts_with(
        "function boo {\n  static test()\n  static test(name:string)\n  static test(name?:any){ }\n}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function boo() {\n    test();\n    test(name, string);\n    test(name ?  : any);\n    { }\n}"),
        "bodyless function recovery should wrap and rewrite static signatures into the recovered body: {js}"
    );
}

#[test]
fn test_malformed_await_using_for_await_recovery_keeps_export_split() {
    let js = emit_ts_with(
        "declare const x: any[]\n\nfor await (await using of x);\n\nexport async function test() {\n  for await (await using of x);\n}\n",
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        "for await (await using  of x)\n    ;\nexport async function test() {\n    for await (await using  of x)\n        ;\n}\n"
    );
}

#[test]
fn test_malformed_await_using_for_of_recovery_keeps_export_split() {
    let js = emit_ts_with(
        "declare const x: any[]\n\nfor (await using of x);\n\nexport async function test() {\n  for (await using of x);\n}\n",
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );
    assert_eq!(
        js,
        "for (await using  of x)\n    ;\nexport async function test() {\n    for (await using  of x)\n        ;\n}\n"
    );
}

#[test]
fn test_downlevel_async_using_emits_structured_disposal_scopes() {
    let js = emit_ts_with(
        "async function f() {\n\
             using sync = makeSync();\n\
             {\n\
                 await using asyncResource = await makeAsync();\n\
                 return value;\n\
             }\n\
         }\n\
         async function* g() {\n\
             await using resource = makeAsync();\n\
             yield value;\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(
        !js.contains("using sync") && !js.contains("await using"),
        "resource syntax must not survive in an ES2015 generator body: {js}"
    );
    assert!(
        js.contains("const sync = __addDisposableResource(env_1, makeSync(), false);")
            && js.contains(
                "const asyncResource = __addDisposableResource(env_2, yield makeAsync(), true);"
            ),
        "sync and awaited resource acquisition should use the disposal stack: {js}"
    );
    assert!(
        js.contains("yield result_2;") && js.contains("yield __await(result_3);"),
        "awaiter and async-generator disposal must use their distinct await protocols: {js}"
    );
    let awaiter = js.find("var __awaiter").expect("awaiter helper");
    let add = js
        .find("var __addDisposableResource")
        .expect("resource helper");
    let dispose = js.find("var __disposeResources").expect("disposal helper");
    let async_generator = js
        .find("var __asyncGenerator")
        .expect("async generator helper");
    let first_body = js.find("function f()").expect("transformed function body");
    assert!(
        awaiter < first_body
            && async_generator < first_body
            && add < dispose
            && dispose < first_body,
        "helpers must be available before transformed bodies and disposal helpers must stay ordered: {js}"
    );
}

#[test]
fn test_downlevel_await_using_in_classic_for_matches_scope_lifetime() {
    let js = emit_ts_with(
        "async function main() {\n\
             for (await using first = makeFirst(), second = makeSecond(); keepGoing(); step()) {\n\
                 work();\n\
             }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(
        js.contains("const first = __addDisposableResource(env_1, makeFirst(), true), second = __addDisposableResource(env_1, makeSecond(), true);")
            && js.contains("for (; keepGoing(); step())")
            && js.contains("yield result_1;"),
        "the resource scope must enclose the complete classic for loop: {js}"
    );
}

#[test]
fn test_nested_async_using_is_discovered_before_helper_emission() {
    let js = emit_ts_with(
        "class Owner {\n\
             async run() {\n\
                 if (enabled) {\n\
                     await using resource = makeResource();\n\
                 }\n\
             }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    let add = js
        .find("var __addDisposableResource")
        .expect("nested resource helper");
    let dispose = js
        .find("var __disposeResources")
        .expect("nested disposal helper");
    let class_body = js.find("class Owner").expect("class body");
    assert!(
        add < dispose && dispose < class_body,
        "nested class-method resources must be discovered before helper headers are emitted: {js}"
    );
    assert!(
        !js.contains("await using") && js.contains("yield result_1;"),
        "nested await-using syntax must be lowered through the awaiter: {js}"
    );
}

#[test]
fn test_async_arrow_using_is_discovered_before_helper_emission() {
    let js = emit_ts_with(
        "const run = async () => {\n\
             await using resource = makeResource();\n\
         };",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    let add = js
        .find("var __addDisposableResource")
        .expect("arrow resource helper");
    let arrow_body = js.find("const run =").expect("async arrow body");
    assert!(
        add < arrow_body && !js.contains("await using") && js.contains("yield result_1;"),
        "resource scopes inside async arrow initializers need helpers before the emitted body: {js}"
    );
}

#[test]
fn test_async_using_helpers_are_discovered_through_runtime_expression_paths() {
    let js = emit_ts_with(
        "export const holder = {\n\
             method() { return async () => {\n\
                 await using a = make();\n\
             }; },\n\
             get value() { return class extends (async () => {\n\
                 using b = make();\n\
             })() {\n\
                 static [(() => async () => {\n\
                     await using c = make();\n\
                 })()] = 1;\n\
             }; }\n\
         };\n\
         export default tag`${async () => {\n\
             await using d = make();\n\
         }}`;",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );

    let helper = js
        .find("var __addDisposableResource")
        .expect("resource helper discovered through nested expressions");
    let declaration = js
        .find("export const holder")
        .expect("exported declaration");
    assert!(
        helper < declaration,
        "helpers must precede exported initializers: {js}"
    );
    assert!(
        !js.contains("await using") && !js.contains("using b"),
        "{js}"
    );
}

#[test]
fn test_async_using_statement_lists_lower_inside_try_switch_and_for_await() {
    let js = emit_ts_with(
        "async function main(xs: any) {\n\
             try { await using a = make(); }\n\
             catch { using b = make(); }\n\
             finally { using c = make(); }\n\
             switch (value) { case 1: await using d = make(); break; }\n\
             for await (const value of xs) { await using e = value; }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(
        !js.contains("await using") && !js.contains("using b"),
        "{js}"
    );
    assert!(js.matches("__addDisposableResource").count() >= 6, "{js}");
    let output = Command::new("node")
        .args(["--check", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.as_mut().unwrap().write_all(js.as_bytes())?;
            child.wait_with_output()
        })
        .expect("Node.js is required for emitter syntax tests");
    assert!(output.status.success(), "invalid JavaScript: {js}");
}

#[test]
fn test_async_using_for_of_inline_object_method_keeps_container_indent() {
    let js = emit_ts_with(
        concat!(
            "async function main() {\n",
            "    for (await using d1 of [{ async [Symbol.asyncDispose]() {} }, { [Symbol.dispose]() {} }, null]) {\n",
            "    }\n",
            "}\n",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );

    assert!(
        js.contains(concat!(
            "        for (const d1_1 of [{ [Symbol.asyncDispose]() {\n",
            "                    return __awaiter(this, void 0, void 0, function* () { });\n",
            "                } }, { [Symbol.dispose]() { } }, null]) {",
        )),
        "the transformed method body must retain both array and object container indentation: {js}"
    );
}

#[test]
fn test_async_object_container_indent_has_compact_sibling_controls() {
    let js = emit_ts_with(
        concat!(
            "async function main() {\n",
            "    const plain = { async method() {} };\n",
            "    const array = [{ async method() {} }];\n",
            "    const sync = [{ method() {} }];\n",
            "}\n",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );

    assert!(
        js.contains(concat!(
            "        const plain = { method() {\n",
            "                return __awaiter(this, void 0, void 0, function* () { });\n",
            "            } };",
        )),
        "a plain object should own exactly one generated-body indent: {js}"
    );
    assert!(
        js.contains(concat!(
            "        const array = [{ method() {\n",
            "                    return __awaiter(this, void 0, void 0, function* () { });\n",
            "                } }];",
        )),
        "an inline array/object pair should own exactly two generated-body indents: {js}"
    );
    assert!(
        js.contains("        const sync = [{ method() { } }];"),
        "a compact synchronous sibling must remain compact: {js}"
    );
}

#[test]
fn test_async_object_method_indent_tracks_nested_inline_containers() {
    let js = emit_ts_with(
        concat!(
            "async function main() {\n",
            "    const nested = [[{ async method() { await 1; } }]];\n",
            "    const parenthesized = [([{ async method() { await 2; } }])];\n",
            "    const nestedObject = { value: [[{ async method() { await 3; } }]] };\n",
            "    const plainAfterNested = { async method() { await 4; } };\n",
            "}\n",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );

    assert!(
        js.contains(concat!(
            "        const nested = [[{ method() {\n",
            "                        return __awaiter(this, void 0, void 0, function* () { yield 1; });\n",
            "                    } }]];",
        )),
        "each nested inline array must contribute one indentation level: {js}"
    );
    assert!(
        js.contains(concat!(
            "        const parenthesized = [([{ method() {\n",
            "                        return __awaiter(this, void 0, void 0, function* () { yield 2; });\n",
            "                    } }])];",
        )),
        "parentheses must transparently propagate inline container depth: {js}"
    );
    assert!(
        js.contains(concat!(
            "        const nestedObject = { value: [[{ method() {\n",
            "                            return __awaiter(this, void 0, void 0, function* () { yield 3; });\n",
            "                        } }]] };",
        )),
        "object and array ancestors must compose structurally: {js}"
    );
    assert!(
        js.contains(concat!(
            "        const plainAfterNested = { method() {\n",
            "                return __awaiter(this, void 0, void 0, function* () { yield 4; });\n",
            "            } };",
        )),
        "container indentation state must be restored after recursive emission: {js}"
    );
}

#[test]
fn test_async_using_multiline_containers_consume_helper_indent() {
    let js = emit_ts_with(
        concat!(
            "async function acquire(): Promise<any> { return null; }\n",
            "async function probe() {\n",
            "    await using multilineArray = [\n",
            "        [{ async [Symbol.asyncDispose]() { await acquire(); } }],\n",
            "        { async [Symbol.asyncDispose]() { await acquire(); } }\n",
            "    ];\n",
            "    await using multilineObject = {\n",
            "        nested: [[{ async [Symbol.asyncDispose]() { await acquire(); } }]],\n",
            "        sibling: { async [Symbol.asyncDispose]() { await acquire(); } }\n",
            "    };\n",
            "    await using after = { async [Symbol.asyncDispose]() { await acquire(); } };\n",
            "}\n",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );

    assert!(
        js.contains(concat!(
            "            const multilineArray = __addDisposableResource(env_1, [\n",
            "                [{ [Symbol.asyncDispose]() {\n",
            "                            return __awaiter(this, void 0, void 0, function* () { yield acquire(); });\n",
            "                        } }],\n",
            "                { [Symbol.asyncDispose]() {\n",
            "                        return __awaiter(this, void 0, void 0, function* () { yield acquire(); });\n",
            "                    } }\n",
            "            ], true);",
        )),
        "a multiline array must consume the helper-owned container indent: {js}"
    );
    assert!(
        js.contains(concat!(
            "            const multilineObject = __addDisposableResource(env_1, {\n",
            "                nested: [[{ [Symbol.asyncDispose]() {\n",
            "                                return __awaiter(this, void 0, void 0, function* () { yield acquire(); });\n",
            "                            } }]],\n",
            "                sibling: { [Symbol.asyncDispose]() {\n",
            "                        return __awaiter(this, void 0, void 0, function* () { yield acquire(); });\n",
            "                    } }\n",
            "            }, true);",
        )),
        "a multiline object must consume the helper-owned container indent: {js}"
    );
    assert!(
        js.contains(concat!(
            "            const after = __addDisposableResource(env_1, { [Symbol.asyncDispose]() {\n",
            "                    return __awaiter(this, void 0, void 0, function* () { yield acquire(); });\n",
            "                } }, true);",
        )),
        "multiline container state must be restored before the next initializer: {js}"
    );
}

#[test]
fn test_async_using_for_initializer_reuses_helper_argument_indent() {
    let js = emit_ts_with(
        concat!(
            "async function main() {\n",
            "    for (await using d1 = { [Symbol.dispose]() {} },\n",
            "                    d2 = { async [Symbol.asyncDispose]() {} };;) {\n",
            "    }\n",
            "}\n",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            ..Default::default()
        },
    );

    assert!(
        js.contains(concat!(
            "                const d1 = __addDisposableResource(env_1, { [Symbol.dispose]() { } }, true), d2 = __addDisposableResource(env_1, { [Symbol.asyncDispose]() {\n",
            "                        return __awaiter(this, void 0, void 0, function* () { });\n",
            "                    } }, true);",
        )),
        "the helper argument already owns the generated object-method indentation: {js}"
    );
}

#[test]
fn test_downlevel_using_for_of_runtime_disposes_each_iteration_before_advance() {
    let source = r#"
        const events: string[] = [];
        const syncResource = (name: string) => ({
            name,
            [Symbol.dispose]() { events.push(`dispose:${name}`); }
        });
        const asyncResource = (name: string) => ({
            name,
            async [Symbol.asyncDispose]() {
                await Promise.resolve();
                events.push(`async-dispose:${name}`);
            }
        });

        async function main() {
            for (using resource of [syncResource("a"), syncResource("b")]) {
                events.push(`body:${resource.name}`);
            }
            for await (await using resource of [asyncResource("c"), asyncResource("d")]) {
                events.push(`async-body:${resource.name}`);
            }
            outer: for (await using loop = asyncResource("loop");;) {
                events.push("loop-body");
                break outer;
            }
            await using Resource = class {
                static actual = this.name;
                static async [Symbol.asyncDispose]() {}
            };
            if (Resource.name !== "Resource" || Resource.actual !== "Resource") {
                throw new Error(`bad class name: ${Resource.name}/${Resource.actual}`);
            }
            const expected = "body:a,dispose:a,body:b,dispose:b,async-body:c,async-dispose:c,async-body:d,async-dispose:d,loop-body,async-dispose:loop";
            if (events.join(",") !== expected) throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(js.contains("for (const resource_1 of"), "{js}");
    let outer = js.find("outer:").expect("outer label");
    let loop_start = js[outer..].find("for (;").expect("labeled loop") + outer;
    assert!(
        !js[outer..loop_start].contains("try"),
        "label must target the loop: {js}"
    );
    let output = Command::new("node")
        .args(["-e", &js])
        .output()
        .expect("Node.js is required for emitter runtime tests");
    assert!(
        output.status.success(),
        "per-iteration disposal runtime failed\nstdout:\n{}\nstderr:\n{}\njs:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        js
    );
}

#[test]
fn test_using_switch_uses_one_lexical_scope_and_disposes_after_switch() {
    let source = r#"
        const events: string[] = [];
        const make = (name: string) => ({
            name,
            [Symbol.dispose]() { events.push(`dispose:${name}`); }
        });
        function main(value: number) {
            switch (value) {
                case 0:
                    using resource = make("switch");
                    events.push("case0");
                case 1:
                    if (value === 0) events.push(`use:${resource.name}`);
                    events.push("case1");
                    break;
            }
            events.push("after");
        }
        main(0);
        const expected = "case0,use:switch,case1,dispose:switch,after";
        if (events.join(",") !== expected) throw new Error(events.join(","));
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(!js.contains("using resource"), "{js}");
    assert_eq!(js.matches("const env_").count(), 1, "{js}");
    execute_with_node(&js);
}

#[test]
fn test_es5_using_for_of_disposes_each_iteration() {
    let source = r#"
        const events: string[] = [];
        const make = (name: string) => ({
            name,
            [Symbol.dispose]() { events.push(`dispose:${name}`); }
        });
        for (using resource of [make("a"), make("b")]) {
            events.push(`body:${resource.name}`);
        }
        if (events.join(",") !== "body:a,dispose:a,body:b,dispose:b") {
            throw new Error(events.join(","));
        }
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );

    assert!(!js.contains("for (using"), "{js}");
    assert!(js.contains("__addDisposableResource"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_native_async_using_for_await_disposes_before_advance() {
    let source = r#"
        const events: string[] = [];
        const make = (name: string) => ({
            name,
            async [Symbol.asyncDispose]() {
                await Promise.resolve();
                events.push(`dispose:${name}`);
            }
        });
        async function main() {
            for await (await using resource of [make("a"), make("b")]) {
                events.push(`body:${resource.name}`);
            }
            const expected = "body:a,dispose:a,body:b,dispose:b";
            if (events.join(",") !== expected) throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2022),
            ..Default::default()
        },
    );

    assert!(!js.contains("await using"), "{js}");
    assert!(js.contains("for await (const resource_"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_chained_labels_target_downlevel_resource_for_await_loop() {
    let source = r#"
        const events: string[] = [];
        const make = (name: string) => ({
            name,
            async [Symbol.asyncDispose]() { events.push(`dispose:${name}`); }
        });
        async function main() {
            outer: inner: for await (await using resource of [make("a"), make("b")]) {
                events.push(`body:${resource.name}`);
                if (resource.name === "a") continue outer;
                continue inner;
            }
            const expected = "body:a,dispose:a,body:b,dispose:b";
            if (events.join(",") !== expected) throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    let outer = js.find("outer:").expect("outer label");
    let inner = js[outer..].find("inner:").expect("inner label") + outer;
    let loop_start = js[inner..].find("for (").expect("generated loop") + inner;
    assert!(outer < inner && inner < loop_start, "{js}");
    assert!(!js[inner..loop_start].contains("try"), "{js}");
    assert!(!js.contains("outer: inner: try"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_resource_helpers_respect_import_helpers_for_cjs_and_esm() {
    let source = "export {}; using resource = makeResource();";
    let cjs = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::CommonJS),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(cjs.contains("const tslib_1 = require(\"tslib\");"), "{cjs}");
    assert!(cjs.contains("tslib_1.__addDisposableResource("), "{cjs}");
    assert!(cjs.contains("tslib_1.__disposeResources("), "{cjs}");
    assert!(!cjs.contains("var __addDisposableResource"), "{cjs}");

    let esm = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::ESNext),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(esm.contains("from \"tslib\";"), "{esm}");
    assert!(esm.contains("__addDisposableResource"), "{esm}");
    assert!(esm.contains("__disposeResources"), "{esm}");
    assert!(!esm.contains("var __addDisposableResource"), "{esm}");
}

#[test]
fn test_using_environment_names_do_not_collide_with_source_identifiers() {
    let source = r#"
        const events: string[] = [];
        const make = () => ({
            async [Symbol.asyncDispose]() { events.push("dispose"); }
        });
        async function main() {
            const env_1 = 10, e_1 = 20, result_1 = 30;
            await using resource = make();
            if (env_1 + e_1 + result_1 !== 60) throw new Error("collision");
        }
        main().then(() => {
            if (events.join(",") !== "dispose") throw new Error(events.join(","));
        }).catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(js.contains("const env_2 ="), "{js}");
    assert!(js.contains("catch (e_2)"), "{js}");
    assert!(js.contains("result_2 = __disposeResources(env_2)"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_es5_async_only_resource_iteration_keeps_async_protocol() {
    let source = r#"
        const events: string[] = [];
        const iterable = {
            [Symbol.asyncIterator]() {
                let done = false;
                return {
                    async next() {
                        if (done) return { done: true, value: undefined };
                        done = true;
                        return { done: false, value: {
                            async [Symbol.asyncDispose]() { events.push("dispose"); }
                        }};
                    }
                };
            }
        };
        async function main() {
            for await (await using resource of iterable) events.push("body");
            if (events.join(",") !== "body,dispose") throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );

    assert!(js.contains("__asyncValues(iterable)"), "{js}");
    assert!(!js.contains("for await"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_es2017_native_async_for_await_emits_helper_and_lowers_body_using() {
    let source = r#"
        const events: string[] = [];
        const iterable = {
            async *[Symbol.asyncIterator]() { yield "value"; }
        };
        async function main() {
            for await (const value of iterable) {
                await using resource = {
                    async [Symbol.asyncDispose]() { events.push(`dispose:${value}`); }
                };
                events.push(`body:${value}`);
            }
            if (events.join(",") !== "body:value,dispose:value") throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2017),
            ..Default::default()
        },
    );

    let helper = js.find("var __asyncValues").expect("async values helper");
    let call = js
        .find("__asyncValues(iterable)")
        .expect("async values call");
    assert!(helper < call, "{js}");
    assert!(!js.contains("await using"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_es2017_native_for_await_state_avoids_user_name_collision() {
    let source = r#"
        async function main() {
            const _nativeForAwaitDone_1 = 1;
            const events: string[] = [];
            for await (const value of ["x"]) {
                await using resource = {
                    async [Symbol.asyncDispose]() { events.push(`dispose:${value}`); }
                };
                events.push(`body:${value}`);
            }
            if (_nativeForAwaitDone_1 !== 1) throw new Error("collision");
            if (events.join(",") !== "body:x,dispose:x") throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2017),
            ..Default::default()
        },
    );

    assert!(js.contains("_nativeForAwaitDone_2"), "{js}");
    assert_node_syntax(&js);
    execute_with_node(&js);
}

#[test]
fn test_es2017_resource_for_await_state_avoids_canonical_temp_collision() {
    let source = r#"
        async function main() {
            const _a = 1;
            const events: string[] = [];
            for await (await using resource of [{
                async [Symbol.asyncDispose]() { events.push("dispose"); }
            }]) {
                events.push("body");
            }
            if (_a !== 1) throw new Error("collision");
            if (events.join(",") !== "body,dispose") throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2017),
            ..Default::default()
        },
    );

    assert!(js.contains("_forAwaitDone_1"), "{js}");
    assert!(!js.contains("await using"), "{js}");
    assert_node_syntax(&js);
    execute_with_node(&js);
}

#[test]
fn test_downlevel_async_using_for_of_value_temp_avoids_outer_binding() {
    let source = r#"
        async function main() {
            const events: string[] = [];
            const resource_1 = 42;
            const made = { [Symbol.dispose]() { events.push("dispose"); } };
            for (using resource of [made]) {
                await Promise.resolve();
                events.push(`outer:${resource_1}`);
                events.push(resource === made ? "resource" : "wrong");
            }
            const expected = "outer:42,resource,dispose";
            if (events.join(",") !== expected) throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(js.contains("for (const resource_2 of"), "{js}");
    assert_node_syntax(&js);
    execute_with_node(&js);
}

#[test]
fn test_es5_downlevel_async_using_for_of_has_no_native_for_of() {
    let source = r#"
        async function main() {
            const events: string[] = [];
            const made = { [Symbol.dispose]() { events.push("dispose"); } };
            for (using resource of [made]) {
                await Promise.resolve();
                events.push(resource === made ? "body" : "wrong");
            }
            if (events.join(",") !== "body,dispose") throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );

    assert!(!js.contains("for (const "), "{js}");
    assert!(!js.contains(" of [made]"), "{js}");
    assert_node_syntax(&js);
    execute_with_node(&js);
}

#[test]
fn test_generic_classic_for_using_lowers_and_labels_actual_loop() {
    let source = r#"
        const events: string[] = [];
        const make = (name: string) => ({
            [Symbol.dispose]() { events.push(`dispose:${name}`); }
        });
        for (using first = make("first");;) {
            events.push("first-body");
            break;
        }
        outer: for (using second = make("second");;) {
            events.push("second-body");
            break outer;
        }
        const expected = "first-body,dispose:first,second-body,dispose:second";
        if (events.join(",") !== expected) throw new Error(events.join(","));
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );

    assert!(!js.contains("for (using"), "{js}");
    let label = js.find("outer:").expect("outer label");
    let loop_start = js[label..].find("for (;").expect("labeled loop") + label;
    assert!(!js[label..loop_start].contains("try"), "{js}");
    assert_node_syntax(&js);
    execute_with_node(&js);
}

#[test]
fn test_es5_ordinary_for_of_lowers_direct_body_using_scope() {
    let source = r#"
        const events: string[] = [];
        for (const value of ["a", "b"]) {
            using resource = {
                [Symbol.dispose]() { events.push(`dispose:${value}`); }
            };
            events.push(`body:${value}`);
        }
        const expected = "body:a,dispose:a,body:b,dispose:b";
        if (events.join(",") !== expected) throw new Error(events.join(","));
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );

    assert!(!js.contains("using resource"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_nested_and_sequential_resource_for_await_loops_have_isolated_state() {
    let source = r#"
        const events: string[] = [];
        const resource = (name: string) => ({
            name,
            async [Symbol.asyncDispose]() { events.push(`dispose:${name}`); }
        });
        async function main() {
            for await (await using outer of [resource("outer")]) {
                for await (await using inner of [resource("inner")]) {
                    events.push(`${outer.name}:${inner.name}`);
                }
            }
            for await (await using last of [resource("last")]) events.push(last.name);
            const expected = "outer:inner,dispose:inner,dispose:outer,last,dispose:last";
            if (events.join(",") !== expected) throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(js.contains("_forAwaitIterator_2"), "{js}");
    assert!(js.contains("_forAwaitIterator_3"), "{js}");
    execute_with_node(&js);
}

#[test]
fn test_downlevel_async_generator_resource_for_await_has_no_native_loop() {
    let source = r#"
        const events: string[] = [];
        const resource = (name: string) => ({
            name,
            async [Symbol.asyncDispose]() { events.push(`dispose:${name}`); }
        });
        async function* values() {
            for await (await using item of [resource("a"), resource("b")]) {
                yield item.name;
            }
        }
        async function main() {
            const iterator = values();
            const first = await iterator.next();
            if (first.value !== "a") throw new Error("bad yield");
            await iterator.return(undefined);
            if (events.join(",") !== "dispose:a") throw new Error(events.join(","));
        }
        main().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(!js.contains("for await"), "{js}");
    assert!(js.contains("yield __await("), "{js}");
    assert_node_syntax(&js);
    execute_with_node(&js);
}

#[test]
fn test_downlevel_async_using_runtime_orders_and_suppresses_errors() {
    let source = r#"
        const events: string[] = [];
        const syncResource = (name: string) => ({
            [Symbol.dispose]() { events.push(`dispose:${name}`); }
        });
        const asyncResource = (name: string, fail = false) => ({
            async [Symbol.asyncDispose]() {
                await Promise.resolve();
                events.push(`dispose:${name}`);
                if (fail) throw new Error(`dispose-error:${name}`);
            }
        });

        async function ordered() {
            using outer = syncResource("outer");
            {
                await using first = asyncResource("first");
                using second = syncResource("second");
                events.push("body");
            }
            events.push("after-inner");
            return "done";
        }

        async function abruptReturn() {
            await using resource = asyncResource("return");
            return "returned";
        }

        async function suppressed() {
            await using resource = asyncResource("failing", true);
            throw new Error("body-error");
        }

        async function* generator() {
            await using resource = asyncResource("generator");
            yield 1;
        }

        (async () => {
            const orderedValue = await ordered();
            if (orderedValue !== "done") throw new Error("bad ordered return");
            if (events.join(",") !== "body,dispose:second,dispose:first,after-inner,dispose:outer") {
                throw new Error(`bad order: ${events.join(",")}`);
            }

            const returned = await abruptReturn();
            if (returned !== "returned" || events.at(-1) !== "dispose:return") {
                throw new Error(`return settled before disposal: ${events.join(",")}`);
            }

            let caught: any;
            try { await suppressed(); } catch (error) { caught = error; }
            if (!caught || caught.name !== "SuppressedError"
                || caught.error?.message !== "dispose-error:failing"
                || caught.suppressed?.message !== "body-error") {
                throw new Error(`bad suppression: ${caught?.name}:${caught?.message}`);
            }

            const iterator = generator();
            const first = await iterator.next();
            if (first.value !== 1 || first.done) throw new Error("bad generator yield");
            await iterator.return(undefined);
            if (events.at(-1) !== "dispose:generator") {
                throw new Error(`generator return skipped disposal: ${events.join(",")}`);
            }
        })().catch(error => { console.error(error); process.exitCode = 1; });
    "#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    let output = Command::new("node")
        .args(["-e", &js])
        .output()
        .expect("Node.js is required for emitter runtime tests");
    assert!(
        output.status.success(),
        "downlevel resource-management runtime failed\nstdout:\n{}\nstderr:\n{}\njs:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        js
    );
}

#[test]
fn test_bare_function_arrow_recovery_emits_empty_anonymous_function() {
    let js = emit_ts_with(
        "function =>",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert_eq!(js, "\"use strict\";\nfunction () { }\n");
}

#[test]
fn test_recover_constructor_body_from_parameter_list_parse_artifact() {
    let js = emit_ts_with(
        "class C {\n  constructor(C: (public A) => any) {\n  }\n}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("constructor(C) {"),
        "should recover constructor body: {js}"
    );
    assert!(
        !js.contains("};"),
        "should not emit parser-recovery trailing `}};`: {js}"
    );
}

#[test]
fn test_recover_method_name_from_split_generic_parse_artifact() {
    let js = emit_ts_with(
        "class C<T extends C<T>> {\n    foo<U extends C<C<T>>(x: U) {\n        return null;\n    }\n}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("foo(x) {"),
        "should recover method name foo: {js}"
    );
    assert!(
        !js.contains("C(x) {"),
        "should not emit recovered method as C(x): {js}"
    );
}

#[test]
fn test_recover_call_with_missing_type_argument_emits_call() {
    let js = emit_ts_with(
        "Foo<a,,b>();",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("Foo();"),
        "missing type-argument recovery should keep bare call: {js}"
    );
    assert!(
        !js.contains("Foo <"),
        "missing type-argument recovery should drop binary-artifact emit: {js}"
    );
}

#[test]
fn test_recover_ambiguous_generic_assertion_var_initializer() {
    let js = emit_ts_with(
        "function f<T>(x: T): T { return null; }\nvar r3 = <<T>(x: T) => T>f;",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var r3 =  << T > (x), T;"),
        "ambiguous generic assertion recovery should keep the TS-style initializer tokenization: {js}"
    );
    assert!(
        js.contains("T > f;"),
        "ambiguous generic assertion recovery should preserve the trailing relational expression: {js}"
    );
    assert!(
        !js.contains("var r3 = << T;"),
        "ambiguous generic assertion recovery should not leave split recovery fragments in the var initializer: {js}"
    );
}

#[test]
fn test_multiline_asi_arithmetic_normalizes_like_tsc() {
    let js = emit_ts_with(
        "var x = 1;\n\nvar y = 1;\n\nvar z =\n\nx\n\n+\n\n+\n\n+\n\ny\n\n\nvar c =\n\nx\n\n-\n\n-\n\n-\n\ny\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var z = x\n    +\n        + +y;"),
        "multiline addition recovery should normalize to TS layout: {js}"
    );
    assert!(
        js.contains("var c = x\n    -\n        - -y;"),
        "multiline subtraction recovery should normalize to TS layout: {js}"
    );
}

#[test]
fn test_namespace_export_import_alias_to_empty_namespace_is_erased() {
    let js = emit_ts(
        "namespace M {\n\
         export namespace N {}\n\
         export import X = N;\n\
         }",
    );
    assert!(
        !js.contains("M.X = N;"),
        "alias to empty namespace should be erased: {js}"
    );
}

#[test]
fn test_namespace_export_import_alias_to_non_exported_empty_namespace_is_erased() {
    let js = emit_ts(
        "namespace M {\n\
         namespace N {}\n\
         export import X = N;\n\
         }",
    );
    assert!(
        !js.contains("M.X = N;"),
        "alias to non-exported empty namespace should be erased: {js}"
    );
}

#[test]
fn test_namespace_export_import_alias_via_runtime_local_is_emitted() {
    let js = emit_ts(
        "namespace M {\n\
         namespace N { class C {} }\n\
         import R = N;\n\
         export import X = R;\n\
         }",
    );
    assert!(
        js.contains("var R = N;"),
        "runtime local alias should be emitted: {js}"
    );
    assert!(
        js.contains("M.X = R;"),
        "export alias to runtime local should be emitted: {js}"
    );
}

#[test]
fn test_preserve_inline_block_comments_around_await_expression_statements() {
    let js = emit_ts_with(
        "async function foo() {\n\
         /*comment1*/ await 1;\n\
         await /*comment2*/ 2;\n\
         await 3 /*comment3*/\n\
         }",
        CompilerOptions {
            target: ScriptTarget::parse("esnext"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("/*comment1*/ await 1;"),
        "leading inline block comment should stay on the same line as await: {js}"
    );
    assert!(
        js.contains("await /*comment2*/ 2;"),
        "inner await comment should be preserved: {js}"
    );
    assert!(
        js.contains("await 3; /*comment3*/"),
        "trailing inline block comment should be preserved after semicolon: {js}"
    );
}

#[test]
fn test_parenthesized_expression_internal_comments_are_preserved() {
    let source = "/*1*/(/*2*/ \"foo\" /*3*/)/*4*/\n\
                  ;\n\
                  \n\
                  // open\n\
                  /*1*/(\n\
                      // next\n\
                      /*2*/\"foo\"\n\
                      //close\n\
                      /*3*/)/*4*/\n\
                  ;";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("/*1*/ ( /*2*/\"foo\" /*3*/) /*4*/;"),
        "single-line paren comments should match TypeScript spacing: {js}"
    );
    assert!(
        js.contains("// open\n/*1*/ (\n// next\n/*2*/ \"foo\"\n//close\n/*3*/ ) /*4*/;"),
        "multiline paren comments should stay attached to the inner expression and close paren: {js}"
    );
}

#[test]
fn test_property_access_expression_inner_comments_are_preserved() {
    let source = "// @strict: false\n\
                  /*1*/Array/*2*/./*3*/toString/*4*/\n\
                  \n\
                  /*1*/Array\n\
                  /*2*/./*3*/\n\
                  \x20\x20\x20\x20// Single-line comment\n\
                  \x20\x20\x20\x20toString/*4*/\n\
                  \n\
                  /*1*/Array/*2*/./*3*/\n\
                  \x20\x20\x20\x20// Single-line comment\n\
                  \x20\x20\x20\x20toString/*4*/\n\
                  \n\
                  /*1*/Array\n\
                  \x20\x20\x20\x20// Single-line comment\n\
                  \x20\x20\x20\x20/*2*/./*3*/toString/*4*/\n\
                  \n\
                  /* Existing issue: the \"2\" comments below are duplicated and \"3\"s are missing */\n\
                  \n\
                  /*1*/Array/*2*/?./*3*/toString/*4*/\n\
                  \n\
                  /*1*/Array\n\
                  /*2*/?./*3*/\n\
                  \x20\x20\x20\x20// Single-line comment\n\
                  \x20\x20\x20\x20toString/*4*/\n\
                  \n\
                  /*1*/Array/*2*/?./*3*/\n\
                  \x20\x20\x20\x20// Single-line comment\n\
                  \x20\x20\x20\x20toString/*4*/\n\
                  \n\
                  /*1*/Array\n\
                  \x20\x20\x20\x20// Single-line comment\n\
                  \x20\x20\x20\x20/*2*/?./*3*/toString/*4*/";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("/*1*/ Array /*2*/. /*3*/toString; /*4*/"),
        "single-line property access comments should be preserved: {js}"
    );
    assert!(
        js.contains("/*1*/ Array\n    /*2*/ . /*3*/\n        // Single-line comment\n        toString; /*4*/"),
        "multiline property access comments should keep TypeScript spacing: {js}"
    );
    assert!(
        js.contains("/*1*/ Array /*2*/ === null || Array /*2*/ === void 0 ? void 0 : Array /*2*/.toString; /*4*/"),
        "single-line optional chain downlevel should preserve pre-operator comments: {js}"
    );
    assert!(
        js.contains("/*1*/ Array === null || Array === void 0 ? void 0 : Array\n/*2*/ .\n// Single-line comment\ntoString; /*4*/"),
        "multiline optional chain downlevel should preserve comment layout after the alt branch: {js}"
    );
}

#[test]
fn test_numeric_literal_member_line_comments_are_preserved() {
    let source = "var test16 = 3  // comment time\n\
                  \x20\x20\x20\x20.toString();\n\
                  var test17 = 3. // comment time again\n\
                  \x20\x20\x20\x20.toString();";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var test16 = 3 // comment time\n    .toString();"),
        "inline line comments after integer literals should stay on the literal line: {js}"
    );
    assert!(
        js.contains("var test17 = 3. // comment time again\n    .toString();"),
        "inline line comments after trailing-decimal literals should stay on the literal line: {js}"
    );
}

#[test]
fn test_numeric_literal_member_newlines_survive_comment_removal() {
    let source = "var test12 = 3\n\
                  \x20\x20/* comment */ .toString();\n\
                  var test13 = 3.\n\
                  \x20\x20/* comment */ .toString();\n\
                  var test14 = 3\n\
                  \x20\x20\x20\x20// comment\n\
                  \x20\x20\x20\x20.toString();\n\
                  var test15 = 3.\n\
                  \x20\x20\x20\x20// comment\n\
                  \x20\x20\x20\x20.toString();\n\
                  var test16 = 3  // comment time\n\
                  \x20\x20\x20\x20.toString();\n\
                  var test17 = 3. // comment time again\n\
                  \x20\x20\x20\x20.toString();";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            remove_comments: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var test12 = 3\n    .toString();"),
        "block comments removed after integers should still leave the newline before member access: {js}"
    );
    assert!(
        js.contains("var test13 = 3.\n    .toString();"),
        "block comments removed after trailing-decimal literals should still leave the newline before member access: {js}"
    );
    assert!(
        js.contains("var test14 = 3\n    .toString();"),
        "line comments removed after integers should still leave the newline before member access: {js}"
    );
    assert!(
        js.contains("var test15 = 3.\n    .toString();"),
        "line comments removed after trailing-decimal literals should still leave the newline before member access: {js}"
    );
    assert!(
        js.contains("var test16 = 3\n    .toString();"),
        "inline line comments removed after integers should still leave the newline before member access: {js}"
    );
    assert!(
        js.contains("var test17 = 3.\n    .toString();"),
        "inline line comments removed after trailing-decimal literals should still leave the newline before member access: {js}"
    );
}

#[test]
fn test_import_equals_alias_to_erased_namespace_member_is_erased() {
    let js = emit_ts(
        "namespace M {\n\
         export namespace N {}\n\
         export import X = N;\n\
         }\n\
         import r = M.X;",
    );
    assert!(
        !js.contains("var r = M.X;"),
        "alias to erased namespace member should be erased: {js}"
    );
}

#[test]
fn test_import_equals_alias_to_runtime_namespace_member_is_emitted() {
    let js = emit_ts(
        "namespace M {\n\
         export const X = 1;\n\
         }\n\
         import r = M.X;",
    );
    assert!(
        js.contains("var r = M.X;"),
        "alias to runtime namespace member should be emitted: {js}"
    );
}

#[test]
fn test_exported_import_equals_follows_ambient_namespace_value_aliases() {
    let js = emit_ts_with(
        "declare namespace pack1 {\n\
             const test1: string;\n\
             export { test1 };\n\
         }\n\
         declare namespace pack2 {\n\
             import test1 = pack1.test1;\n\
             export { test1 };\n\
         }\n\
         export import test1 = pack2.test1;\n\
         declare namespace types {\n\
             interface OnlyType {}\n\
             export { OnlyType };\n\
         }\n\
         export import erased = types.OnlyType;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(js.contains("exports.test1 = pack2.test1;"), "{js}");
    assert!(!js.contains("exports.erased = types.OnlyType;"), "{js}");
}

#[test]
fn test_error_marker_comment_inside_expression_is_not_preserved() {
    let js = emit_ts(
        "namespace Test {\n\
         class Mocked {\n\
             myProp: string;\n\
         }\n\
         class Tester {\n\
             willThrowError() {\n\
                 Mocked = Mocked || function () { // => Error: Invalid left-hand side of assignment expression.\n\
                     return { myProp: \"test\" };\n\
                 };\n\
             }\n\
         }\n\
         }",
    );
    assert!(
        !js.contains("// => Error: Invalid left-hand side of assignment expression."),
        "inline error marker comment should be dropped: {js}"
    );
}

#[test]
fn test_recover_missing_arrow_body_preserves_trailing_semicolon_statements() {
    let js = emit_ts(
        "namespace missingCurliesWithArrow {\n\
         namespace withoutStatement {\n\
             var a = () => };\n\
             var b = (): void => }\n\
             var c = (x) => };\n\
             var d = (x: number, y: string) => };\n\
             var e = (x: number, y: string): void => };\n\
             var f = () => }\n\
         }\n\
         }",
    );
    assert!(
        js.contains("})(withoutStatement || (withoutStatement = {}));\n    ;\n    var b = () => ;"),
        "missing-body recovery should preserve semicolon after `var a = () => }};`: {js}"
    );
    assert!(
        js.contains("var c = (x) => ;\n;\nvar d = (x, y) => ;"),
        "missing-body recovery should preserve semicolon after `var c = ... }};`: {js}"
    );
    assert!(
        js.contains("var d = (x, y) => ;\n;\nvar e = (x, y) => ;"),
        "missing-body recovery should preserve semicolon after `var d = ... }};`: {js}"
    );
    assert!(
        js.contains("var e = (x, y) => ;\n;\nvar f = () => ;"),
        "missing-body recovery should preserve semicolon after `var e = ... }};`: {js}"
    );
}

#[test]
fn test_recover_missing_arrow_token_before_block_body() {
    let js = emit_ts(
        "namespace missingArrowsWithCurly {\n\
         var a = () { };\n\
         var b = (x) { };\n\
         var c = (): void { }\n\
         }",
    );
    assert!(
        js.contains("var a = () => { };"),
        "missing fat-arrow before block should recover to arrow emit: {js}"
    );
    assert!(
        js.contains("var b = (x) => { };"),
        "single-param missing fat-arrow before block should recover to arrow emit: {js}"
    );
    assert!(
        js.contains("var c = () => { };"),
        "typed missing fat-arrow before block should recover to arrow emit: {js}"
    );
}

#[test]
fn test_recover_typed_missing_arrow_token_as_omitted_body() {
    let js = emit_ts(
        "namespace typedMissingArrows {\n\
         var a = (): void;\n\
         var b = (x: number, y: string);\n\
         }",
    );
    assert!(
        js.contains("var a = () => ;"),
        "typed missing fat-arrow should recover to omitted-body arrow emit: {js}"
    );
    assert!(
        js.contains("var b = (x, y) => ;"),
        "typed params missing fat-arrow should recover to omitted-body arrow emit: {js}"
    );
}

#[test]
fn test_recover_typed_missing_arrow_token_member_tail_as_comma_expr() {
    let js = emit_ts("const { date2 } = (inspectedElement: any).props;");
    assert!(
        js.contains("const { date2 } = (inspectedElement) => , props;"),
        "typed missing fat-arrow with member tail should recover to comma-tail arrow body: {js}"
    );
}

#[test]
fn test_recover_invalid_conditional_tail_after_block_arrow() {
    let js = emit_ts("(a?) => { return a; } ? (b)=>(c)=>81 : (c)=>(d)=>82;");
    assert!(
        js.contains("(a) => { return a; };\n(b) => (c) => 81;\n(c) => (d) => 82;"),
        "invalid conditional tail after block-bodied arrow should split into recovered arrow statements: {js}"
    );
}

#[test]
fn test_parenthesized_conditional_inside_object_spread_is_not_arrow_recovery() {
    let js = emit_ts(
        "const param2 = Math.random() < 0.5 ? 'value2' : null;\n\
         const obj = { ...(param2 ? { param2 } : {}) };",
    );
    assert!(
        js.contains("const obj = { ...(param2 ? { param2 } : {}) };"),
        "parenthesized conditional inside object spread should stay a conditional expression: {js}"
    );
}

#[test]
fn test_recover_cast_of_bare_yield() {
    let js = emit_ts(
        "function* f() {\n\
         \x20\x20\x20\x20<number> yield 0;\n\
         }",
    );
    assert!(
        js.contains("function* f() {\n    ;\n    yield 0;\n}"),
        "type assertion around bare yield should recover to empty statement plus yield: {js}"
    );
}

#[test]
fn test_esm_export_named_alias() {
    let js = emit_ts_esm("const x = 1; export { x as default };");
    assert!(
        js.contains("export { x as default }"),
        "ESM export alias should be preserved: {js}"
    );
}

#[test]
fn test_cjs_re_export_named_from_source() {
    let js = emit_ts_with(
        "export { foo, bar } from './lib';",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("require(\"./lib\")"),
        "should require the source: {js}"
    );
    assert!(
        js.contains("Object.defineProperty(exports, \"foo\""),
        "should re-export foo via Object.defineProperty: {js}"
    );
    assert!(
        js.contains("Object.defineProperty(exports, \"bar\""),
        "should re-export bar via Object.defineProperty: {js}"
    );
}

// ---------------------------------------------------------------
// Destructuring pattern tests
// ---------------------------------------------------------------

#[test]
fn test_object_destructuring_basic() {
    let js = emit_ts("const { a, b } = obj;");
    assert!(
        js.contains("const { a, b } = obj;"),
        "basic object destructuring should be emitted: {js}"
    );
}

#[test]
fn test_array_destructuring_basic() {
    let js = emit_ts("const [a, b] = arr;");
    assert!(
        js.contains("const [a, b] = arr;"),
        "basic array destructuring should be emitted: {js}"
    );
}

#[test]
fn test_nested_object_destructuring() {
    let js = emit_ts("const { a: { b } } = obj;");
    assert!(
        js.contains("{ a: { b } }"),
        "nested object destructuring should be emitted: {js}"
    );
}

#[test]
fn test_nested_array_destructuring() {
    let js = emit_ts("const [a, [b, c]] = arr;");
    assert!(
        js.contains("[a, [b, c]]"),
        "nested array destructuring should be emitted: {js}"
    );
}

#[test]
fn test_object_destructuring_default_value() {
    let js = emit_ts("const { a = 1 } = obj;");
    assert!(
        js.contains("{ a = 1 }"),
        "object destructuring with default value should be emitted: {js}"
    );
}

#[test]
fn test_array_destructuring_default_value() {
    let js = emit_ts("const [a = 1, b = 2] = arr;");
    assert!(
        js.contains("[a = 1, b = 2]"),
        "array destructuring with default value should be emitted: {js}"
    );
}

#[test]
fn test_object_destructuring_rest() {
    let js = emit_ts("const { a, ...rest } = obj;");
    assert!(
        js.contains("{ a, ...rest }"),
        "object destructuring with rest should be emitted: {js}"
    );
}

#[test]
fn test_nested_object_rest_baseline_shape_downlevels() {
    let js = emit_ts_with(
        "var x, y;\n\
         [{ ...x }] = [{ abc: 1 }];\n\
         for ([{ ...y }] of [[{ abc: 1 }]]) ;",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var _a, _b;"),
        "nested object-rest baseline should hoist both temps: {js}"
    );
    assert!(
        js.contains("[_a] = [{ abc: 1 }], x = __rest(_a, []);"),
        "nested object-rest assignment should rewrite through __rest: {js}"
    );
    assert!(
        js.contains("for (let _c of [[{ abc: 1 }]]) {"),
        "for-of loop should remain preserved at ES2015 target: {js}"
    );
    assert!(
        js.contains("[_b] = _c, y = __rest(_b, []);"),
        "nested for-of object rest should rewrite through __rest inside the loop body: {js}"
    );
}

#[test]
fn test_nested_object_rest_assignment_with_inner_default_downlevels() {
    let js = emit_ts_with(
        "let a: any, b: any, c: any = { x: { a: 1, y: 2 } }, d: any;\n\
         ({ x: { a, ...b } = d } = c);",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var _a, _b;"),
        "nested defaulted object-rest assignment should hoist both temps: {js}"
    );
    assert!(
        js.contains(
            "(_a = c.x, _b = _a === void 0 ? d : _a, { a } = _b, b = __rest(_b, [\"a\"]));"
        ),
        "nested defaulted object-rest assignment should apply its default before rest: {js}"
    );
}

#[test]
fn test_array_defaulted_object_rest_assignment_is_valid_and_executes_once() {
    let js = emit_ts_with(
        "let a: any, r: any;\n\
         let sourceCalls = 0, defaultCalls = 0;\n\
         function source(): any { sourceCalls++; return [undefined]; }\n\
         function fallback(): any { defaultCalls++; return { a: 1, b: 2 }; }\n\
         ([{ a, ...r } = fallback()] = source());\n\
         console.log(JSON.stringify({ a, r, sourceCalls, defaultCalls }));\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(
        js.contains(
            "([_a] = source(), _b = _a === void 0 ? fallback() : _a, { a } = _b, r = __rest(_b, [\"a\"]));"
        ),
        "array slot, default, and rest must be emitted in tsc order: {js}"
    );
    assert_node_syntax(&js);
    assert_eq!(
        execute_with_node(&js),
        r#"{"a":1,"r":{"b":2},"sourceCalls":1,"defaultCalls":1}"#
    );
}

#[test]
fn test_nested_array_defaulted_object_rest_flattens_before_default() {
    let js = emit_ts_with(
        "let x: any, r: any, tail: any;\n\
         let order: string[] = [];\n\
         function source(): any { order.push('source'); return [[undefined, 9]]; }\n\
         function fallback(): any { order.push('fallback'); return { x: 3, y: 4 }; }\n\
         ([[{ x, ...r } = fallback(), tail]] = source());\n\
         console.log(JSON.stringify({ x, r, tail, order }));\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(
        js.contains(
            "([_a] = source(), [_b, tail] = _a, _c = _b === void 0 ? fallback() : _b, { x } = _c, r = __rest(_c, [\"x\"]));"
        ),
        "nested arrays must be captured outside-in before resolving defaults: {js}"
    );
    assert_node_syntax(&js);
    assert_eq!(
        execute_with_node(&js),
        r#"{"x":3,"r":{"y":4},"tail":9,"order":["source","fallback"]}"#
    );
}

#[test]
fn test_multiple_array_defaulted_object_rests_resolve_left_to_right() {
    let js = emit_ts_with(
        "let a: any, r: any, x: any, s: any;\n\
         let order: string[] = [];\n\
         function source(): any { order.push('source'); return [undefined, undefined]; }\n\
         function first(): any { order.push('first'); return { a: 1, b: 2 }; }\n\
         function second(): any { order.push('second'); return { x: 3, y: 4 }; }\n\
         ([{ a, ...r } = first(), { x, ...s } = second()] = source());\n\
         console.log(JSON.stringify({ a, r, x, s, order }));\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(
        js.contains(
            "([_a, _b] = source(), _c = _a === void 0 ? first() : _a, { a } = _c, r = __rest(_c, [\"a\"]), _d = _b === void 0 ? second() : _b, { x } = _d, s = __rest(_d, [\"x\"]));"
        ),
        "all slots must be captured before left-to-right default resolution: {js}"
    );
    assert_node_syntax(&js);
    assert_eq!(
        execute_with_node(&js),
        r#"{"a":1,"r":{"b":2},"x":3,"s":{"y":4},"order":["source","first","second"]}"#
    );
}

#[test]
fn test_array_defaulted_object_rest_assignment_is_preserved_at_es2018() {
    let js = emit_ts_with(
        "let a: any, r: any;\n\
         const source: any = [undefined];\n\
         const fallback: any = { a: 1, b: 2 };\n\
         ([{ a, ...r } = fallback] = source);\n\
         console.log(JSON.stringify({ a, r }));\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );

    assert!(js.contains("([{ a, ...r } = fallback] = source);"), "{js}");
    assert!(!js.contains("__rest"), "{js}");
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), r#"{"a":1,"r":{"b":2}}"#);
}

#[test]
fn test_array_destructuring_rest() {
    let js = emit_ts("const [a, ...rest] = arr;");
    assert!(
        js.contains("[a, ...rest]"),
        "array destructuring with rest should be emitted: {js}"
    );
}

#[test]
fn test_object_destructuring_rename() {
    let js = emit_ts("const { a: b } = obj;");
    assert!(
        js.contains("{ a: b }"),
        "object destructuring with rename should be emitted: {js}"
    );
}

#[test]
fn test_object_destructuring_computed_property() {
    let js = emit_ts("const { [expr]: a } = obj;");
    assert!(
        js.contains("[expr]: a"),
        "object destructuring with computed property should be emitted: {js}"
    );
}

#[test]
fn test_rest_param_object_rest_downlevels_to_temp_destructure() {
    let js = emit_ts_with(
        "function e(...{0: a = 1, 1: b = true, ...rest: rest}: [boolean, string, number]) { }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __rest = (this && this.__rest) || function (s, e) {"),
        "rest-param object rest should emit __rest helper: {js}"
    );
    assert!(
        js.contains("function e(..._a) {")
            && js
                .contains("var { 0: a = 1, 1: b = true } = _a, rest = __rest(_a, [\"0\", \"1\"]);"),
        "rest-param object rest should downlevel through a temp rest array: {js}"
    );
}

#[test]
fn test_rest_param_object_rest_empty_body_compacts_after_transform() {
    let js = emit_ts_with(
        "let obj = {};\nfunction test({ prop = { ...obj }, ...props }) {}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "function test(_a) { var { prop = Object.assign({}, obj) } = _a, props = __rest(_a, [\"prop\"]); }"
        ),
        "empty-body rest-param transform should stay compact: {js}"
    );
}

#[test]
fn test_rest_param_recovery_keeps_inner_rest_default() {
    let js = emit_ts_with(
        "function b(...[...foo = []]: string[]) { }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("function b(...[...foo = []]) { }"),
        "rest-param recovery should keep the default on the inner rest element: {js}"
    );
}

#[test]
fn test_constructor_rest_modifier_recovery_emits_trailing_ident() {
    let js = emit_ts("class C { constructor(...public rest: string[]) {} }");
    assert!(
        js.contains("constructor(...public, rest) { }"),
        "constructor rest-modifier recovery should keep the trailing identifier: {js}"
    );
}

#[test]
fn test_array_destructuring_holes() {
    let js = emit_ts("const [, , a] = arr;");
    assert!(
        js.contains("[, , a]"),
        "array destructuring with holes should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_in_function_params() {
    let js = emit_ts("function f({ a, b }: { a: number, b: string }) { return a; }");
    assert!(
        js.contains("function f({ a, b })"),
        "destructuring in function params should be emitted: {js}"
    );
    assert!(
        !js.contains(": number") && !js.contains(": string"),
        "type annotations should be stripped: {js}"
    );
}

#[test]
fn test_destructuring_in_arrow_params() {
    let js = emit_ts("const f = ({ a, b }: { a: number, b: string }) => a + b;");
    assert!(
        js.contains("({ a, b })"),
        "destructuring in arrow params should be emitted: {js}"
    );
}

#[test]
fn test_array_destructuring_in_function_params() {
    let js = emit_ts("function f([a, b]: number[]) { return a; }");
    assert!(
        js.contains("function f([a, b])"),
        "array destructuring in function params should be emitted: {js}"
    );
}

#[test]
fn test_nested_object_in_array_destructuring() {
    let js = emit_ts("const [{ a, b }, c] = arr;");
    assert!(
        js.contains("[{ a, b }, c]"),
        "nested object in array destructuring should be emitted: {js}"
    );
}

#[test]
fn test_nested_array_in_object_destructuring() {
    let js = emit_ts("const { a: [b, c] } = obj;");
    assert!(
        js.contains("{ a: [b, c] }"),
        "nested array in object destructuring should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_with_rename_and_default() {
    let js = emit_ts("const { a: b = 1 } = obj;");
    assert!(
        js.contains("{ a: b = 1 }"),
        "destructuring with rename and default should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_let() {
    let js = emit_ts("let { a, b } = obj;");
    assert!(
        js.contains("let { a, b } = obj;"),
        "let destructuring should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_var() {
    let js = emit_ts("var [a, b] = arr;");
    assert!(
        js.contains("var [a, b] = arr;"),
        "var destructuring should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_with_type_annotation() {
    let js = emit_ts("const { a, b }: { a: number, b: string } = obj;");
    assert!(
        js.contains("const { a, b } = obj;"),
        "destructuring should have type annotation stripped: {js}"
    );
    assert!(
        !js.contains(": { a:"),
        "type annotation should be stripped: {js}"
    );
}

#[test]
fn test_destructuring_param_with_default() {
    let js = emit_ts("function f({ a = 1, b = 2 }: { a?: number, b?: number } = {}) { }");
    assert!(
        js.contains("{ a = 1, b = 2 }"),
        "destructuring param with default values should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_rest_in_function_param() {
    let js = emit_ts("function f({ a, ...rest }: any) { }");
    assert!(
        js.contains("{ a, ...rest }"),
        "destructuring rest in function param should be emitted: {js}"
    );
}

#[test]
fn test_for_of_destructuring() {
    let js = emit_ts("for (const { a, b } of items) { console.log(a); }");
    assert!(
        js.contains("{ a, b }"),
        "for-of destructuring should be emitted: {js}"
    );
}

#[test]
fn test_for_of_array_destructuring() {
    let js = emit_ts("for (const [key, value] of entries) { console.log(key); }");
    assert!(
        js.contains("[key, value]"),
        "for-of array destructuring should be emitted: {js}"
    );
}

#[test]
fn test_for_of_nested_array_destructuring_multiline_header_indent() {
    let js = emit_ts(
        "for ([, [\n    primarySkillA = \"primary\",\n    secondarySkillA = \"secondary\"\n] = [\"none\", \"none\"]] of robots) {\n    console.log(primarySkillA);\n}",
    );
    assert!(
        js.contains(
            "for ([, [\n        primarySkillA = \"primary\",\n        secondarySkillA = \"secondary\"\n    ] = [\"none\", \"none\"]] of robots) {"
        ),
        "nested array destructuring opened on the first loop-header line should gain one extra indent level: {js}"
    );
}

#[test]
fn test_for_assignment_nested_array_destructuring_continuation_indent() {
    let js = emit_ts(
        "for ([nameMA = \"noName\",\n    [\n        primarySkillA = \"primary\",\n        secondarySkillA = \"secondary\"\n    ] = [\"none\", \"none\"]\n] = multiRobotA, i = 0; i < 1; i++) {\n    console.log(nameMA);\n}",
    );
    assert!(
        js.contains(
            "for ([nameMA = \"noName\",\n    [\n        primarySkillA = \"primary\",\n        secondarySkillA = \"secondary\"\n    ] = [\"none\", \"none\"]\n] = multiRobotA, i = 0; i < 1; i++) {"
        ),
        "nested array destructuring opened on a continuation line should keep its source indentation: {js}"
    );
}

#[test]
fn test_for_in_destructuring() {
    // for-in with destructuring is unusual but valid TypeScript
    let js = emit_ts("for (const key in obj) { console.log(key); }");
    assert!(
        js.contains("for (const key in obj)"),
        "for-in should be emitted correctly: {js}"
    );
}

#[test]
fn test_deeply_nested_destructuring() {
    let js = emit_ts("const { a: { b: { c } } } = obj;");
    assert!(
        js.contains("{ a: { b: { c } } }"),
        "deeply nested destructuring should be emitted: {js}"
    );
}

#[test]
fn test_complex_mixed_destructuring() {
    let js = emit_ts("const { a, b: [c, { d }], ...rest } = obj;");
    assert!(js.contains("a,"), "mixed destructuring should emit a: {js}");
    assert!(
        js.contains("...rest"),
        "mixed destructuring should emit rest: {js}"
    );
    assert!(
        js.contains("[c, { d }]"),
        "mixed destructuring should emit nested: {js}"
    );
}

#[test]
fn test_destructuring_assignment_object() {
    let js = emit_ts("({ a, b } = obj);");
    assert!(
        js.contains("a") && js.contains("b") && js.contains("obj"),
        "destructuring assignment should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_assignment_array() {
    let js = emit_ts("[a, b] = arr;");
    assert!(
        js.contains("a") && js.contains("b") && js.contains("arr"),
        "array destructuring assignment should be emitted: {js}"
    );
}

#[test]
fn test_empty_object_destructuring() {
    let js = emit_ts("const {} = obj;");
    assert!(
        js.contains("const {") && js.contains("} = obj;"),
        "empty object destructuring should be emitted: {js}"
    );
}

#[test]
fn test_empty_array_destructuring() {
    let js = emit_ts("const [] = arr;");
    assert!(
        js.contains("const [] = arr;"),
        "empty array destructuring should be emitted: {js}"
    );
}

#[test]
fn test_nested_destructuring_with_defaults() {
    let js = emit_ts("const { a: { b = 1 } = {} } = obj;");
    assert!(
        js.contains("b = 1"),
        "nested destructuring with defaults should emit default: {js}"
    );
}

#[test]
fn test_array_rest_with_nested() {
    let js = emit_ts("const [{ a }, ...rest] = arr;");
    assert!(
        js.contains("{ a }"),
        "array rest with nested object should emit nested: {js}"
    );
    assert!(
        js.contains("...rest"),
        "array rest with nested should emit rest: {js}"
    );
}

#[test]
fn test_destructuring_string_key() {
    let js = emit_ts("const { \"hello world\": x } = obj;");
    assert!(
        js.contains("\"hello world\": x"),
        "destructuring with string key should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_number_key() {
    let js = emit_ts("const { 0: first } = arr;");
    assert!(
        js.contains("0: first"),
        "destructuring with number key should be emitted: {js}"
    );
}

#[test]
fn test_object_destructuring_with_multiple_defaults() {
    let js = emit_ts("const { a = 1, b = 2, c = 3 } = obj;");
    assert!(
        js.contains("a = 1"),
        "multiple defaults should emit a = 1: {js}"
    );
    assert!(
        js.contains("b = 2"),
        "multiple defaults should emit b = 2: {js}"
    );
    assert!(
        js.contains("c = 3"),
        "multiple defaults should emit c = 3: {js}"
    );
}

#[test]
fn test_array_destructuring_with_computed_key_rename_default() {
    // Combine computed key with default in object destructuring
    let js = emit_ts("const { [key]: val = 42 } = obj;");
    assert!(
        js.contains("[key]: val = 42"),
        "computed key with default should be emitted: {js}"
    );
}

#[test]
fn test_catch_with_destructuring() {
    let js = emit_ts("try { } catch ({ message }) { console.log(message); }");
    assert!(
        js.contains("catch ({ message })") || js.contains("catch ("),
        "catch with destructuring should be emitted: {js}"
    );
    assert!(
        js.contains("message"),
        "catch destructuring should have binding: {js}"
    );
}

#[test]
fn test_for_of_nested_destructuring() {
    let js = emit_ts("for (const { a: { b } } of items) { }");
    assert!(
        js.contains("a: { b }"),
        "for-of nested destructuring should be emitted: {js}"
    );
}

#[test]
fn test_destructuring_param_with_rest() {
    let js = emit_ts("function f([first, ...rest]: number[]) { return first; }");
    assert!(
        js.contains("[first, ...rest]"),
        "function param with array rest destructuring should be emitted: {js}"
    );
}

#[test]
fn test_arrow_destructuring_param() {
    let js = emit_ts("const f = ([a, b]: [number, number]) => a + b;");
    assert!(
        js.contains("[a, b]"),
        "arrow with array destructuring param should be emitted: {js}"
    );
    assert!(
        !js.contains("[number, number]"),
        "type annotation should be stripped: {js}"
    );
}

#[test]
fn test_destructuring_export_esm() {
    let js = emit_ts_esm("export const { a, b } = obj;");
    assert!(
        js.contains("export const { a, b } = obj;"),
        "exported destructuring should be emitted in ESM: {js}"
    );
}

#[test]
fn test_deeply_nested_array_destructuring() {
    let js = emit_ts("const [[[a]]] = arr;");
    assert!(
        js.contains("[[[a]]]"),
        "deeply nested array destructuring should be emitted: {js}"
    );
}

// ---------------------------------------------------------------
// Arrow function object literal body tests (diagnostic)
// ---------------------------------------------------------------

#[test]
fn test_arrow_object_literal_body_with_type_annotation_diag() {
    let js = emit_ts("const f = (x: number) => ({a: x});");
    eprintln!("DIAG arrow typed param obj body: [{}]", js.trim());
    assert!(
        js.contains("=> ({"),
        "arrow with object literal body should wrap in parens: {js}"
    );
}

#[test]
fn test_arrow_object_literal_body_with_return_type_diag() {
    let js = emit_ts("const f = (): object => ({});");
    eprintln!("DIAG arrow return type obj body: [{}]", js.trim());
    assert!(
        js.contains("=> ({"),
        "arrow with empty object literal body should wrap in parens: {js}"
    );
}

#[test]
fn test_arrow_as_const_object_literal_diag() {
    // When 'as const' is stripped from `({a: 1} as const)`, the Paren wraps it
    let js = emit_ts("const f = () => ({a: 1} as const);");
    eprintln!("DIAG arrow as const: [{}]", js.trim());
    assert!(
        js.contains("=> ({"),
        "arrow with as const object literal should wrap in parens: {js}"
    );
}

#[test]
fn test_arrow_satisfies_object_literal_diag() {
    // When 'satisfies' is stripped, the object literal must remain wrapped
    let js = emit_ts("const f = () => ({a: 1} satisfies {a: number});");
    eprintln!("DIAG arrow satisfies: [{}]", js.trim());
    assert!(
        js.contains("=> ({"),
        "arrow with satisfies object literal should wrap in parens: {js}"
    );
}

#[test]
fn test_arrow_type_assertion_object_literal_diag() {
    // When type assertion is stripped from `<Foo>{a: 1}`, what happens?
    let js = emit_ts("const f = () => <any>{a: 1};");
    eprintln!("DIAG arrow type assertion: [{}]", js.trim());
    assert!(
        js.contains("=> ({"),
        "arrow with type assertion object literal should wrap in parens: {js}"
    );
}

#[test]
fn test_arrow_object_literal_preserves_inner_comments_when_transformed() {
    let js = emit_ts_with(
        "declare var value: boolean;\n\
         declare var a: any;\n\
         const test = () => ({\n\
             // \"Identifier expected.\" error on \"!\" and two \"Duplicate identifier '(Missing)'.\" errors on space.\n\
             prop: !value, // remove ! to see that errors will be gone\n\
             run: () => {\n\
                 // comment next line or remove \"()\" to see that errors will be gone\n\
                 if(!a.b()) { return 'special'; }\n\
                 return 'default';\n\
             }\n\
         });",
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            ..Default::default()
        },
    );
    assert!(
        js.contains("// \"Identifier expected.\" error on \"!\" and two \"Duplicate identifier '(Missing)'.\" errors on space."),
        "leading comment inside object literal should be preserved: {js}"
    );
    assert!(
        js.contains("prop: !value, // remove ! to see that errors will be gone"),
        "trailing comment on object literal property should be preserved: {js}"
    );
}

#[test]
fn test_multiline_assignment_object_literal_function_body_dedent() {
    let js = emit_ts(
        "x =\n\
\t{\ta:0,\n\
\t\tb:0,\n\
\t\tm: function(c,d) {\n\
\t\t\t\treturn c + d;\n\
\t\t\t}\n\
\t};",
    );
    assert!(
        js.contains(
            "x =\n    { a: 0,\n        b: 0,\n        m: function (c, d) {\n            return c + d;\n        }\n    };"
        ),
        "function-valued object literal properties should dedent their body to one level below the property line: {js}"
    );
}

#[test]
fn test_split_call_continuation_is_emitted_inline() {
    let js = emit_ts(
        "declare function foo(): string;\n\
         foo()\n\
             (1 + 2).toString();",
    );
    assert!(
        js.contains("foo()(1 + 2).toString();"),
        "split accidental-call continuation should be emitted inline: {js}"
    );
}

#[test]
fn test_multiline_chain_initializer_keeps_relative_indent() {
    let js = emit_ts(concat!(
        "function logErrors(fileName) {\n",
        "    let allDiagnostics = services.getCompilerOptionsDiagnostics()\n",
        "        .concat(services.getSyntacticDiagnostics(fileName))\n",
        "        .concat(services.getSemanticDiagnostics(fileName));\n",
        "}",
    ));
    assert!(
        js.contains("let allDiagnostics = services.getCompilerOptionsDiagnostics()\n")
            && js.contains("        .concat(services.getSyntacticDiagnostics(fileName))\n")
            && js.contains("        .concat(services.getSemanticDiagnostics(fileName));\n")
            && !js.contains("\n    .concat(services.getSyntacticDiagnostics(fileName))\n"),
        "continuation lines should keep extra indent relative to assignment line: {js}"
    );
}

#[test]
fn test_es5_split_member_initializer_uses_structural_continuation_indent() {
    let js = emit_ts_with(
        concat!(
            "let promise = new Promise(function(resolve) {})\n",
            "                .finally(function() {});\n",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );

    assert!(
        js.contains(
            "var promise = new Promise(function (resolve) { })\n    .finally(function () { });"
        ),
        "downlevel declarations must not retain arbitrary source padding before a split member: {js}"
    );
    assert!(
        !js.contains("\n                .finally("),
        "the over-indented source continuation leaked into output: {js}"
    );
}

#[test]
fn test_split_member_indent_does_not_rewrite_comment_or_string_text() {
    let js = emit_ts_with(
        concat!(
            "let text = \"keep\\n                .inside\";\n",
            "// keep\n",
            "//                .comment\n",
            "let value = source.finally();\n",
        ),
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );

    assert!(
        js.contains("\\n                .inside"),
        "string text changed: {js}"
    );
    assert!(
        js.contains("//                .comment"),
        "comment text changed: {js}"
    );
    assert!(
        js.contains("var value = source.finally();"),
        "same-line member changed: {js}"
    );
}

#[test]
fn test_chained_calls_keep_indent_after_trailing_comment() {
    let js = emit_ts_with(
        concat!(
            "var r1 = v1.func(num => num.toString())\n",
            "           .func(str => str.length) // error, number doesn't have a length\n",
            "           .func(num => num.toString())\n",
        ),
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "var r1 = v1.func(num => num.toString())\n    .func(str => str.length) // error, number doesn't have a length\n    .func(num => num.toString());"
        ),
        "structured chained calls should keep continuation indent after a trailing line comment: {js}"
    );
}

#[test]
fn test_initializer_after_trailing_comment_keeps_single_space_prefix() {
    let js = emit_ts_with(
        concat!(
            "const evenSquares: number[] = // should error\n",
            "    [1, 2, 3, 4]\n",
            "        .map(x => x)\n",
            "        .filter(Boolean);",
        ),
        CompilerOptions {
            target: ScriptTarget::parse("es2015"),
            declaration: Some(true),
            ..Default::default()
        },
    );
    assert!(
        js.contains(
            "const evenSquares = // should error\n [1, 2, 3, 4]\n    .map(x => x)\n    .filter(Boolean);"
        ),
        "array initializers after a trailing `//` comment should keep the single-space continuation prefix: {js}"
    );
}

#[test]
fn test_expression_arrow_chain_continuation_dedent() {
    let js = emit_ts(concat!(
        "const result = myArray\n",
        "  .map((arr) => arr // should error\n",
        "    .filter(Boolean)\n",
        "    // keep comment alignment\n",
        "    .map(x => x)\n",
        "  );",
    ));
    assert!(
        js.contains(
            "const result = myArray\n    .map((arr) => arr // should error\n    .filter(Boolean)\n    // keep comment alignment\n    .map(x => x));"
        ),
        "expression-bodied arrow chain continuations should dedent to one normalized level, including inline comments: {js}"
    );
}

#[test]
fn test_class_field_initializer_recovery_from_newline_elem_access() {
    let js = emit_ts_esm(
        "const o = { [\"prop.inner\"]: \"a\", prop: { inner: \"b\" } } as const;\n\
         export class Foo {\n\
             [o[\"prop.inner\"]] = \"A\"\n\
             [o.prop.inner] = \"B\"\n\
         }",
    );
    assert!(
        js.contains("[o[\"prop.inner\"]] = \"A\"[o.prop.inner] = \"B\";"),
        "expected merged initializer in class field recovery: {js}"
    );
}

#[test]
fn test_class_computed_field_no_asi_empty_block_tail_recovery() {
    let js = emit_ts_with(
        "class C {\n\
             // No ASI\n\
             [e] = 0\n\
             [e2]() { }\n\
         }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(false),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);
    assert!(
        js.contains("_a = e;\n{ }\n"),
        "expected empty block body from computed-property no-ASI recovery to survive after the class: {js}"
    );
}

#[test]
fn test_cjs_export_object_getter_closing_brace_indent() {
    let js = emit_ts_with(
        "export var basePrototype = {\n\
            get primaryPath() {\n\
                var _this = this;\n\
                return _this.collection.schema.primaryPath;\n\
            },\n\
        };",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        js.contains("exports.basePrototype = {\n    get primaryPath() {\n")
            && js.contains("        return _this.collection.schema.primaryPath;\n")
            && js.contains("    },\n};"),
        "getter closing brace should align with getter declaration indent: {js}"
    );
}

#[test]
fn test_trailing_multiline_comment_whitespace_preserved() {
    let js = emit_ts(
        "class Base { private a: string; }\n\
         class Derived extends Base { private b: string; }\n\
         var x: any = null;\n\
         var y: any = x[0];\n\
         /*\n\
         // Note\n\
         */ ",
    );
    assert!(
        js.contains("*/ \n"),
        "standalone multiline comment should preserve trailing spaces on closing line: {js}"
    );
}

#[test]
fn test_function_decl_body_indent_uses_structured_emit() {
    let js = emit_ts(concat!(
        "async function * asyncGen (n) {\n",
        "    for (let i = 0; i < n; i++)\n",
        "      yield i * 2;\n",
        "  }",
    ));
    assert!(
        js.contains("async function* asyncGen(n) {\n")
            && js.contains("    for (let i = 0; i < n; i++)\n")
            && js.contains("        yield i * 2;\n")
            && js.contains("}\n"),
        "function body indentation should follow structured block emit: {js}"
    );
}

#[test]
fn test_structured_if_preserves_multiline_condition_head() {
    let js = emit_ts(
        "function delint(node: any) {\n\
         \x20\x20\x20\x20if (node.a &&\n\
         \x20\x20\x20\x20\x20\x20\x20\x20node.b &&\n\
         \x20\x20\x20\x20\x20\x20\x20\x20node.c) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20report(node);\n\
         \x20\x20\x20\x20}\n\
         }",
    );
    assert!(
        js.contains("if (node.a &&\n        node.b &&\n        node.c) {"),
        "structured emit should preserve multiline if-head formatting: {js}"
    );
}

#[test]
fn test_structured_if_normalizes_newline_after_open_paren() {
    let js = emit_ts(
        "class Foo {\n\
         \x20\x20\x20\x20async foo(node) {\n\
         \x20\x20\x20\t\tif (\n\
         \x20\x20\x20\t\t\t!(node instanceof CommitFileNode) &&\n\
         \x20\x20\x20\t\t\t!(node instanceof StashFileNode) &&\n\
         \x20\x20\x20\t\t\t!(node instanceof ResultsFileNode)\n\
         \x20\x20\x20\t\t) {\n\
         \x20\x20\x20\t\t\treturn;\n\
         \x20\x20\x20\t\t}\n\
         \x20\x20\x20\t\tawait this.bar(node);\n\
         \x20\x20\x20\x20}\n\
         }",
    );
    assert!(
        js.contains(
            "if (!(node instanceof CommitFileNode) &&\n            !(node instanceof StashFileNode) &&\n            !(node instanceof ResultsFileNode)) {"
        ),
        "newline after `if (` should normalize to TypeScript-style multiline condition emit: {js}"
    );
}

#[test]
fn test_structured_if_does_not_preserve_body_indent_as_blank_line() {
    let js = emit_ts(
        "for (let x = 1, y = 2; x < y; ++x, --y) {\n\
         \x20\x20\x20\x20let a = () => x++ + y++;\n\
         \x20\x20\x20\x20if (x == 1) \n\
         \x20\x20\x20\x20\x20\x20\x20\x20break;\n\
         \x20\x20\x20\x20else \n\
         \x20\x20\x20\x20\x20\x20\x20\x20y = 5;\n\
         }",
    );
    assert!(
        js.contains("if (x == 1)\n        break;\n    else\n        y = 5;"),
        "single-line if condition should not preserve a blank line before the body: {js}"
    );
}

#[test]
fn test_cjs_async_generator_nested_import_yield_downlevels_with_async_generator_helpers() {
    let js = emit_ts_with(
        "async function* foo() {\n\
         import((await import(yield \"foo\")).default);\n\
         }",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __await = (this && this.__await) || function (v) {"),
        "downleveled async generator should emit __await helper: {js}"
    );
    assert!(
        js.contains("var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {"),
        "downleveled async generator should emit __asyncGenerator helper: {js}"
    );
    assert!(
        !js.contains("var __awaiter = (this && this.__awaiter) ||"),
        "async generator-only downlevel should not emit __awaiter helper: {js}"
    );
    assert!(
        js.contains(
            "function foo() {\n    return __asyncGenerator(this, arguments, function* foo_1() {"
        ),
        "downleveled async generator declaration should wrap body with __asyncGenerator: {js}"
    );
    assert!(
        js.contains("yield __await(Promise.resolve(`${yield yield __await(\"foo\")}`).then(s => __importStar(require(s))))"),
        "await + yield in async generator should lower to __await forms: {js}"
    );
}

#[test]
fn test_es2017_async_generator_function_expression_downlevels() {
    let js = emit_ts_with(
        "const fnExpr = async function* () {};",
        CompilerOptions {
            target: Some(ScriptTarget::ES2017),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __await = (this && this.__await) || function (v) {"),
        "ES2017 should still emit __await for async generators: {js}"
    );
    assert!(
        js.contains("var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {"),
        "ES2017 should still emit __asyncGenerator helper: {js}"
    );
    assert!(
        js.contains("const fnExpr = function () {\n    return __asyncGenerator(this, arguments, function* () {"),
        "async generator function expressions should downlevel at ES2017: {js}"
    );
}

#[test]
fn test_es2017_object_rest_pattern_async_generator_default_downlevels() {
    let js = emit_ts_with(
        "let { fn = async function*() {}, ...rest } = {} as any;",
        CompilerOptions {
            target: Some(ScriptTarget::ES2017),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var __await = (this && this.__await) || function (v) {"),
        "pattern defaults should trigger async generator helper emission: {js}"
    );
    assert!(
        js.contains("var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {"),
        "pattern defaults should trigger __asyncGenerator helper emission: {js}"
    );
    assert!(
        js.contains("{ fn = function () {")
            && js.contains("return __asyncGenerator(this, arguments, function* () { });")
            && js.contains("} } = _a, rest = __rest(_a, [\"fn\"])"),
        "object-rest destructuring should downlevel async generator defaults at ES2017: {js}"
    );
    assert!(
        js.find("var __rest = (this && this.__rest) || function (s, e) {")
            < js.find("var __await = (this && this.__await) || function (v) {"),
        "__rest helper should emit before async-generator helpers: {js}"
    );
}

#[test]
fn test_object_rest_assignment_default_preserves_rhs_value() {
    let js = emit_ts_with(
        "let obj = {};\nlet { more = { ...obj } = { ...obj }, ...props } = {} as any;",
        CompilerOptions {
            target: Some(ScriptTarget::ES2017),
            ..Default::default()
        },
    );
    assert!(
        js.contains("=== void 0 ? (_a = Object.assign({}, obj), obj = __rest(_a, []), _a) :"),
        "object-rest default assignment should preserve the RHS value via a temp-backed comma expression: {js}"
    );
}

#[test]
fn test_array_binding_pattern_omitted_slots_spacing() {
    let js = emit_ts(concat!(
        "var results: string[];\n",
        "function f([, a, , b, , , , s, , , ] = results) {\n",
        "    a = s[1];\n",
        "    b = s[2];\n",
        "}",
    ));
    assert!(
        js.contains("function f([, a, , b, , , , s, , ,] = results) {\n"),
        "omitted array-binding slots should keep spaces between holes: {js}"
    );
}

#[test]
fn test_cjs_omitted_array_export_emits_temp_chain() {
    let js = emit_ts_with(
        "export let [,,[,[],,[],]] = undefined as any;",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var _a, _b, _c, _d;"),
        "omitted array export should declare temp vars: {js}"
    );
    assert!(
        js.contains("_a = undefined, _b = _a[2], _c = _b[1], _d = _b[3];"),
        "omitted array export should preserve destructuring side effects: {js}"
    );
}

#[test]
fn test_amd_reference_path_emitted_when_not_covered_by_import() {
    let js = emit_ts_with(
        "/// <reference path=\"a.d.ts\"/>\nimport * as http from 'intern/dojo/node!http';",
        CompilerOptions {
            module: Some(ModuleKind::AMD),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    // TypeScript emits reference path directives in AMD JS output when
    // they're not covered by an import (they may declare modules).
    assert!(
        js.contains("/// <reference path"),
        "reference path directive should be in AMD output when not covered by import: {js}"
    );
}

#[test]
fn test_cjs_require_import_elided_for_reference_preamble() {
    let js = emit_ts_with(
        "///<reference path='types.d.ts' />\nimport foo = require(\"foo\")\nfoo.bar(\"x\");",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        !js.contains("const foo = require(\"foo\");"),
        "require import should be elided for reference-preamble ambient imports: {js}"
    );
    assert!(
        js.contains("foo.bar(\"x\");"),
        "value references should be preserved even when require import is elided: {js}"
    );
}

#[test]
fn test_cjs_require_import_kept_for_non_declaration_reference_preamble() {
    let js = emit_ts_with(
        "///<reference path='types.ts' />\nimport foo = require(\"foo\")\nfoo.bar(\"x\");",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("const foo = require(\"foo\");"),
        "require import should be kept when reference preamble points at a non-declaration file: {js}"
    );
}

#[test]
fn test_transport_stream_garbage_keeps_single_identifier_recovery() {
    let js = emit_ts_with(
        "G@\u{4}\u{4}\u{4}G@\u{5}\u{5}\u{5}G@\u{6}\u{6}\u{6}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let g_count = js.lines().filter(|l| l.trim() == "G;").count();
    assert_eq!(
        g_count, 1,
        "binary-garbage recovery should keep one leading identifier statement: {js}"
    );
}

#[test]
fn test_class_recovery_emits_var_constructor_tail() {
    let js = emit_ts_with(
        "class C {\n\
            public const var export foo = 10;\n\
\n\
            var constructor() { }\n\
        }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        js.contains("var constructor;"),
        "class parser-recovery should emit trailing var constructor: {js}"
    );
    assert!(
        js.contains("() => { };"),
        "class parser-recovery should emit trailing recovery arrow: {js}"
    );
}

#[test]
fn test_recover_multiline_array_elisions_with_trailing_comment() {
    let js = emit_ts(
        "const array = [\n\
            ,, /* comment */\n\
        ];",
    );
    assert!(
        js.contains("const array = [\n    ,\n    , /* comment */\n];"),
        "multiline array elisions should stay line-separated with comment: {js}"
    );
}

#[test]
fn test_recover_class_member_with_missing_identifier_tail_block() {
    let js = emit_ts(
        "class C {\n\
            public {};\n\
        }",
    );
    assert!(
        js.contains("class C {\n}\n{ }\n;"),
        "missing class member identifier should emit recovery tail block: {js}"
    );
}

#[test]
fn test_recover_class_member_with_missing_identifier_tail_index_signature() {
    let js = emit_ts(
        "class C {\n\
            public {[name:string]:VariableDeclaration};\n\
        }",
    );
    assert!(
        js.contains("class C {\n}\n{\n    [name, string];\n    VariableDeclaration;\n}\n;"),
        "index-signature recovery tail should be emitted after class: {js}"
    );
}

#[test]
fn test_reserved_word_class_expr_does_not_emit_public_tail_block() {
    let js = emit_ts(
        "function foo() {\n\
            var myClass = class package extends public {}\n\
            var b: public.bar;\n\
        }",
    );
    assert!(
        js.contains("var myClass = class package extends public {\n    };"),
        "class expression should stay attached to its empty body without a recovery block tail: {js}"
    );
    assert!(
        !js.contains("{ };"),
        "class expression reserved-word heritage should not trigger missing-member block recovery: {js}"
    );
}

#[test]
fn test_class_body_statement_recovery_emits_trailing_statements() {
    let js = emit_ts_with(
        "class C {\n\
             var x = 1;\n\
         }\n\
         class C2 {\n\
             function foo() {}\n\
         }\n\
         var x = 1;\n\
         var y = 2;\n\
         class C3 {\n\
             x: number = y + 1;\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);
    assert!(
        js.contains("class C {\n}\nvar x = 1;"),
        "invalid `var` in class body should recover to a trailing var statement: {js}"
    );
    assert!(
        js.contains("class C2 {\n}\nfunction foo() { }"),
        "invalid `function` in class body should recover to a trailing function declaration: {js}"
    );
    assert!(
        !js.contains("class C {\n    constructor() {\n        this.x = 1;"),
        "recovered class-body statements must not be lowered as instance fields: {js}"
    );
    assert!(
        js.contains("class C3 {\n    constructor() {\n        this.x = y + 1;"),
        "real class fields should still lower normally: {js}"
    );
}

#[test]
fn test_private_indexer_object_var_recovery_keeps_tail_inside_object_literal() {
    let js = emit_ts_with(
        "var x = {\n\
            private [x: string]: string;\n\
        }\n\
        \n\
        var y: {\n\
            private[x: string]: string;\n\
        }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);
    assert!(
        js.contains("var x = {\n    [x]: string, string\n};"),
        "private indexer recovery should keep the trailing `string` inside the object literal: {js}"
    );
    assert!(
        !js.contains("};\nstring;"),
        "private indexer recovery must not leak the recovered tail as a top-level statement: {js}"
    );
    assert!(
        js.contains("var y;"),
        "subsequent type-only declaration should still erase normally: {js}"
    );
}

#[test]
fn test_nested_class_recovery_restores_trailing_class_and_object_tail() {
    let js = emit_ts_with(
        "class C {\n\
            x: string;\n\
            class C2 {\n\
            }\n\
        }\n\
        \n\
        function foo() {\n\
            class C3 {\n\
            }\n\
        }\n\
        \n\
        var x = {\n\
            class C4 {\n\
            }\n\
        }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);
    assert!(
        js.contains("class C {\n}\nclass C2 {\n}"),
        "nested class declaration inside a class body should recover as a trailing class declaration: {js}"
    );
    assert!(
        js.contains("function foo() {\n    class C3 {\n    }\n}"),
        "nested class declarations inside functions should still emit unchanged: {js}"
    );
    assert!(
        js.contains("var x = {\n    class: C4\n}, {};"),
        "object-literal nested class recovery should keep the empty block attached to the var statement: {js}"
    );
    assert!(
        !js.contains("var x = {\n    class,\n    C4\n};\n{\n}"),
        "object-literal recovery must not leak the nested class body as a top-level block: {js}"
    );
}

#[test]
fn test_private_method_assignment_downlevels_destructure_and_update() {
    let js = emit_ts_with(
        "class A3 {\n\
         #method() { }\n\
         static #staticMethod() { }\n\
         constructor(a: A3, b: any) {\n\
             ({ x: this.#method } = { x: () => {} });\n\
             b.#method++;\n\
             ({ x: A3.#staticMethod } = { x: () => {} });\n\
             b.#staticMethod++;\n\
         }\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);

    assert!(
        js.contains(
            "(_b = this, { x: ({ set value(_c) { __classPrivateFieldSet(_b, _A3_instances, _c, \"m\"); } }).value } = { x: () => { } });"
        ),
        "instance private-method destructuring assignment should lower through a setter proxy: {js}"
    );
    assert!(
        js.contains(
            "__classPrivateFieldSet(_c = b, _A3_instances, (_d = __classPrivateFieldGet(_c, _A3_instances, \"m\", _A3_method), _d++, _d), \"m\");"
        ),
        "instance private-method postfix update should lower through get/set helpers: {js}"
    );
    assert!(
        js.contains(
            "({ x: ({ set value(_e) { __classPrivateFieldSet(_a, _a, _e, \"m\"); } }).value } = { x: () => { } });"
        ),
        "static private-method destructuring assignment should lower through a setter proxy: {js}"
    );
    assert!(
        js.contains(
            "__classPrivateFieldSet(_e = b, _a, (_f = __classPrivateFieldGet(_e, _a, \"m\", _A3_staticMethod), _f++, _f), \"m\");"
        ),
        "static private-method postfix update should lower through get/set helpers: {js}"
    );
}

#[test]
fn test_private_write_only_accessor_destructure_uses_setter_proxies() {
    let js = emit_ts_with(
        "class Test {\n\
         set #value(v: { foo: { bar: number } }) {}\n\
         set #valueRest(v: number[]) {}\n\
         set #valueOne(v: number) {}\n\
         set #valueCompound(v: number) {}\n\
         m() {\n\
             const foo = { bar: 1 };\n\
             ({ o: this.#value } = { o: { foo } });\n\
             ({ ...this.#value } = { foo });\n\
             ({ foo: { ...this.#value.foo } } = { foo });\n\
             [this.#valueOne, ...this.#valueRest] = [1, 2, 3];\n\
         }\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    let js = normalize_newlines(&js);

    assert!(
        js.contains("var _a, _b, _c, _d;"),
        "write-only accessor destructuring should reserve the expected receiver temps: {js}"
    );
    assert!(
        js.contains(
            "(_b = this, ({ set value(_e) { __classPrivateFieldSet(_b, _Test_instances, _e, \"a\", _Test_value_set); } }).value = __rest({ foo }, []));"
        ),
        "object rest assignment should lower through the private accessor setter: {js}"
    );
    assert!(
        js.contains(
            "(__classPrivateFieldGet(this, _Test_instances, \"a\").foo = __rest({ foo }.foo, []));"
        ),
        "nested object rest should reuse the direct property source instead of a temp destructure: {js}"
    );
    assert!(
        js.contains(
            "_c = this, _d = this, [({ set value(_e) { __classPrivateFieldSet(_c, _Test_instances, _e, \"a\", _Test_valueOne_set); } }).value, ...({ set value(_e) { __classPrivateFieldSet(_d, _Test_instances, _e, \"a\", _Test_valueRest_set); } }).value] = [1, 2, 3];"
        ),
        "array destructuring should lower write-only accessor targets through setter proxies: {js}"
    );
}

#[test]
fn test_reserved_word_implements_class_does_not_emit_public_tail_block() {
    let js = emit_ts(
        "interface public { }\n\
         class E implements public { }\n\
         class F implements public.private.B { }\n",
    );
    assert!(
        js.contains("class E {\n}"),
        "implements clause should still be stripped from the class declaration: {js}"
    );
    assert!(
        js.contains("class F {\n}"),
        "subsequent class declaration should still emit normally: {js}"
    );
    assert!(
        !js.contains("{ }\nclass F"),
        "heritage clause `implements public` should not emit a standalone block tail: {js}"
    );
}

#[test]
fn test_invalid_export_type_only_names_recover_without_module_export_prefix() {
    let js = emit_ts(
        "export namespace 100 {}\n\
         export interface 100 {}\n\
         export type 100 {}\n",
    );
    assert!(
        js.starts_with("\"use strict\";\n"),
        "invalid export type-only names should not make the file a module: {js}"
    );
    assert!(
        js.contains("namespace;\n100;\n{ }"),
        "invalid export namespace should recover as bare leaked statements: {js}"
    );
    assert!(
        js.contains("interface;\n100;\n{ }"),
        "invalid export interface should recover as bare leaked statements: {js}"
    );
    assert!(
        js.contains("type;\n100;\n{ }"),
        "invalid export type should recover as bare leaked statements: {js}"
    );
    assert!(
        !js.contains("export namespace;") && !js.contains("export interface;") && !js.contains("export type;"),
        "invalid export type-only names should not preserve the `export` keyword in JS recovery: {js}"
    );
}

#[test]
fn test_preserve_type_annotations_as() {
    let source = "const x = 1 as number;";

    let opts = CompilerOptions {
        preserve_type_annotations: Some(true),
        ..Default::default()
    };
    eprintln!("Options: {:?}", opts);
    let js = emit_ts_with(source, opts);
    eprintln!("JS output: {}", js);
    assert!(
        js.contains("as number"),
        "as type annotation should be preserved: {js}"
    );
}

#[test]
fn test_preserve_type_annotations_satisfies() {
    let opts = CompilerOptions {
        preserve_type_annotations: Some(true),
        ..Default::default()
    };
    let js = emit_ts_with("const x = 1 satisfies number;", opts);
    assert!(
        js.contains("satisfies number"),
        "satisfies type annotation should be preserved: {js}"
    );
}

#[test]
fn test_preserve_type_annotations_angle_bracket() {
    let opts = CompilerOptions {
        preserve_type_annotations: Some(true),
        ..Default::default()
    };
    let js = emit_ts_with("const x = 1;", opts);
    // Note: angle-bracket syntax <Type>x is only valid in .tsx files
    // For regular .ts files, this would be parsed as a type assertion differently
    println!("JS: {}", js);
}

#[test]
fn test_preserve_comments() {
    let opts = CompilerOptions {
        preserve_comments: Some(true),
        ..Default::default()
    };
    let js = emit_ts_with("// this is a comment\nconst x = 1;", opts);
    assert!(
        js.contains("this is a comment"),
        "comment should be preserved: {js}"
    );
}

#[test]
fn test_rewrite_relative_import_extensions_in_preserve_output() {
    let js = emit_ts_with(
        "import {} from \"./foo.ts\";\n\
         export * from \"../bar.mts\";\n\
         import(\"./view.tsx\");\n\
         import(\"\" + \"./computed.ts\");\n",
        CompilerOptions {
            module: Some(ModuleKind::Preserve),
            target: Some(ScriptTarget::ESNext),
            verbatim_module_syntax: Some(true),
            other: vec![(
                "rewriteRelativeImportExtensions".to_string(),
                "true".to_string(),
            )],
            ..Default::default()
        },
    );

    assert!(js.contains("import {} from \"./foo.js\";"), "{js}");
    assert!(js.contains("export * from \"../bar.mjs\";"), "{js}");
    assert!(js.contains("import(\"./view.js\");"), "{js}");
    assert!(
        js.contains("import(__rewriteRelativeImportExtension(\"\" + \"./computed.ts\"));"),
        "{js}"
    );
    assert_eq!(
        js.matches("var __rewriteRelativeImportExtension =").count(),
        1
    );
}

#[test]
fn test_rewrite_relative_import_extensions_in_commonjs_calls() {
    let js = emit_ts_file_with(
        "test.js",
        "{\n\
             require(\"./foo.cts\");\n\
             require(getPath());\n\
             import(\"./bar.ts\");\n\
             import(\"\" + \"./computed.ts\");\n\
         }\n",
        CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ESNext),
            other: vec![(
                "rewriteRelativeImportExtensions".to_string(),
                "true".to_string(),
            )],
            ..Default::default()
        },
    );

    assert!(js.contains("require(\"./foo.cjs\");"), "{js}");
    assert!(
        js.contains("require(__rewriteRelativeImportExtension(getPath()));"),
        "{js}"
    );
    assert!(
        js.contains("Promise.resolve().then(() => __importStar(require(\"./bar.js\")))"),
        "{js}"
    );
    assert!(
        js.contains("`${__rewriteRelativeImportExtension(\"\" + \"./computed.ts\")}`"),
        "{js}"
    );
}

#[test]
fn test_using_anonymous_classes_preserve_named_evaluation_when_downleveled() {
    let js = emit_ts_with(
        "export {};\n\
         declare const dec: any;\n\
         using C1 = class { static [Symbol.dispose]() {} };\n\
         using C2 = class { static x = 1; static [Symbol.dispose]() {} };\n\
         using C3 = @dec class { static [Symbol.dispose]() {} };\n",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            target: Some(ScriptTarget::ES2018),
            ..Default::default()
        },
    );

    assert!(js.contains("__setFunctionName(_a, \"C1\")"), "{js}");
    assert!(js.contains("__setFunctionName(_b, \"C2\")"), "{js}");
    assert!(js.contains("var class_1 = _classThis = class {"), "{js}");
    assert!(js.contains("__setFunctionName(_classThis, \"C3\")"), "{js}");
    assert!(
        !js.contains("static { _classThis = this; }"),
        "ES2018 output must not use native static blocks: {js}"
    );
}

#[test]
fn test_class_expression_computed_field_temps_use_block_scope() {
    let js = emit_ts_file_with(
        "test.js",
        "const array = [];\n\
         for (let i = 0; i < 10; ++i) {\n\
             array.push(class C {\n\
                 [i] = () => C;\n\
                 static [i] = 100;\n\
             });\n\
         }\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(js.contains("var _a;\nconst array"), "{js}");
    assert!(
        js.contains("    let _b, _c;\n    array.push((_c = class C"),
        "{js}"
    );
    assert!(js.contains("this[_b] = () => _c;"), "{js}");
    assert!(js.contains("_a = i,\n        _c[_a] = 100"), "{js}");
}

#[test]
fn test_orphan_try_clauses_recover_with_synthetic_try_blocks() {
    let js = emit_ts(
        "function fn() {\n\
             catch(x) { } // error missing try\n\
             finally { } // potential error; can be absorbed by the 'catch'\n\
             try { }; // error missing finally\n\
         }\n",
    );

    assert!(
        js.contains("    try {\n    }\n    catch (x) { } // error missing try"),
        "{js}"
    );
    assert!(
        js.contains("    catch (x) { } // error missing try\n    finally { } // potential error"),
        "{js}"
    );
    assert!(
        js.contains("    finally { // error missing finally\n     } // error missing finally\n    ; // error missing finally"),
        "{js}"
    );
}

#[test]
fn test_module_none_es2015_dynamic_import_without_resolution_context_preserves_specifier() {
    let js = emit_ts_with(
        "const loaded = import(\"./dep\");\n",
        CompilerOptions {
            module: Some(ModuleKind::None),
            target: Some(ScriptTarget::ES2015),
            out_file: Some("bundle.js".into()),
            ..Default::default()
        },
    );

    assert!(js.contains("var __createBinding"), "{js}");
    assert!(js.contains("var __setModuleDefault"), "{js}");
    assert!(js.contains("var __importStar"), "{js}");
    assert!(
        js.contains(
            "const loaded = Promise.resolve().then(() => __importStar(require(\"./dep\")));"
        ),
        "{js}"
    );
}

#[test]
fn test_module_none_es2020_dynamic_import_stays_native() {
    let js = emit_ts_with(
        "const loaded = import(\"./dep\");\n",
        CompilerOptions {
            module: Some(ModuleKind::None),
            target: Some(ScriptTarget::ES2020),
            out_file: Some("bundle.js".into()),
            ..Default::default()
        },
    );

    assert!(js.contains("const loaded = import(\"./dep\");"), "{js}");
    assert!(!js.contains("__importStar"), "{js}");
}

#[test]
fn test_es2015_namespace_reexport_lowers_and_keeps_import_attributes() {
    let js = emit_ts_with(
        "export * as ns from './dep' with { type: \"json\" };\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2015),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert_eq!(
        js,
        "import * as ns_1 from './dep' with { type: \"json\" };\nexport { ns_1 as ns };\n"
    );
}

#[test]
fn test_es2015_namespace_reexport_synthetic_name_avoids_collisions() {
    let js = emit_ts_with(
        "const ns_1 = 0;\nexport * as ns from './dep';\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2015),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert!(js.contains("import * as ns_2 from './dep';"), "{js}");
    assert!(js.contains("export { ns_2 as ns };"), "{js}");
}

#[test]
fn test_es2015_string_namespace_reexport_uses_identifier_temp() {
    let js = emit_ts_with(
        "export * as \"<namespace>\" from './dep';\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2015),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert_eq!(
        js,
        "import * as _a from './dep';\nexport { _a as \"<namespace>\" };\n"
    );
}

#[test]
fn test_es2020_namespace_reexport_stays_native() {
    let js = emit_ts_with(
        "export * as ns from './dep' with { type: \"json\" };\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2020),
            target: Some(ScriptTarget::ES2020),
            ..Default::default()
        },
    );

    assert_eq!(js, "export * as ns from './dep' with { type: \"json\" };\n");
}

#[test]
fn test_out_file_dynamic_import_uses_resolved_bundle_ids_in_sync_and_async_paths() {
    let js = emit_ts_with(
        "const direct = import('./b.js');\n\
         const nested = import('./sub/b.js');\n\
         const parent = import('../shared.js');\n\
         const missing = import('./missing.js');\n\
         async function load() { return await import('../shared.js'); }\n",
        CompilerOptions {
            module: Some(ModuleKind::None),
            target: Some(ScriptTarget::ES2015),
            out_file: Some("bundle.js".into()),
            other: vec![(
                "__tsrsOutFileDynamicImportMap".into(),
                "./b.js\tb\n./sub/b.js\tsub/b\n../shared.js\tshared".into(),
            )],
            ..Default::default()
        },
    );

    assert!(js.contains("require('b')"), "{js}");
    assert!(js.contains("require('sub/b')"), "{js}");
    assert_eq!(js.matches("require('shared')").count(), 2, "{js}");
    assert!(js.contains("require('./missing.js')"), "{js}");
    assert!(!js.contains("require('../shared.js')"), "{js}");
}

#[test]
fn test_es2015_namespace_reexport_attributes_ignore_keyword_in_module_path() {
    let options = CompilerOptions {
        module: Some(ModuleKind::ES2015),
        target: Some(ScriptTarget::ES2015),
        ..Default::default()
    };
    let with_js = emit_ts_with(
        "export * as ns from './with-path' with { type: \"json\" };\n",
        options.clone(),
    );
    let assert_js = emit_ts_with(
        "export * as ns from './assert-path' assert { type: \"json\" };\n",
        options,
    );

    assert_eq!(
        with_js,
        "import * as ns_1 from './with-path' with { type: \"json\" };\nexport { ns_1 as ns };\n"
    );
    assert_eq!(
        assert_js,
        "import * as ns_1 from './assert-path' assert { type: \"json\" };\nexport { ns_1 as ns };\n"
    );
}

#[test]
fn test_es2015_namespace_reexport_preserves_comments_on_moved_tokens() {
    let js = emit_ts_with(
        "/*0*/ export /*1*/ * /*2*/ as /*3*/ ns /*4*/ from /*5*/ \"./dep\" /*6*/;\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2015),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert_eq!(
        js,
        "/*0*/ import * as ns_1 from \"./dep\" /*6*/;\nexport { ns_1 as ns /*4*/ };\n"
    );
}

#[test]
fn test_es2015_namespace_reexport_preserves_safe_source_quote() {
    let js = emit_ts_with(
        "export * as ns from \"./it's\" with { type: 'json' };\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2015),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert_eq!(
        js,
        "import * as ns_1 from \"./it's\" with { type: 'json' };\nexport { ns_1 as ns };\n"
    );
}

#[test]
fn test_es2015_namespace_reexport_preserves_string_alias_literal() {
    let js = emit_ts_with(
        "export * as 'a\"b' from \"./dep\";\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2015),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert_eq!(
        js,
        "import * as _a from \"./dep\";\nexport { _a as 'a\"b' };\n"
    );
}

#[test]
fn test_es2015_namespace_reexport_preserves_moved_line_comment_terminator() {
    let js = emit_ts_with(
        "export * as ns // alias comment\nfrom \"./dep\";\n",
        CompilerOptions {
            module: Some(ModuleKind::ES2015),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );

    assert_eq!(
        js,
        "import * as ns_1 from \"./dep\";\nexport { ns_1 as ns // alias comment\n };\n"
    );
}

fn module_none_es2015_options() -> CompilerOptions {
    CompilerOptions {
        module: Some(ModuleKind::None),
        target: Some(ScriptTarget::ES2015),
        ..Default::default()
    }
}

fn assert_dynamic_import_helpers(js: &str) {
    assert!(js.contains("var __createBinding"), "{js}");
    assert!(js.contains("var __setModuleDefault"), "{js}");
    assert!(js.contains("var __importStar"), "{js}");
}

#[test]
fn test_module_none_dynamic_import_in_computed_object_name_injects_helpers() {
    let js = emit_ts_with(
        "const value = { [import(\"./dep\")]: 1 };\nvoid value;\n",
        module_none_es2015_options(),
    );

    assert_dynamic_import_helpers(&js);
    assert!(
        js.contains("[Promise.resolve().then(() => __importStar(require(\"./dep\")))]: 1"),
        "{js}"
    );
}

#[test]
fn test_module_none_dynamic_import_in_computed_class_name_injects_helpers() {
    let js = emit_ts_with(
        "class C { [import(\"./dep\")]() {} }\n",
        module_none_es2015_options(),
    );

    assert_dynamic_import_helpers(&js);
    assert!(js.contains("class C"), "{js}");
    assert!(js.contains("__importStar(require(\"./dep\"))"), "{js}");
}

#[test]
fn test_module_none_dynamic_import_in_binding_default_injects_helpers() {
    let js = emit_ts_with(
        "const { value = import(\"./dep\") } = {};\n",
        module_none_es2015_options(),
    );

    assert_dynamic_import_helpers(&js);
    assert!(js.contains("__importStar(require(\"./dep\"))"), "{js}");
}

#[test]
fn test_module_none_erased_dynamic_import_declaration_does_not_inject_helpers() {
    let js = emit_ts_with(
        "declare function f(value = import(\"./dep\")): void;\n",
        module_none_es2015_options(),
    );

    assert_eq!(js, "\"use strict\";\n");
    assert!(!js.contains("__importStar"), "{js}");
}

#[test]
fn test_module_none_ambient_var_dynamic_import_does_not_inject_helpers() {
    let js = emit_ts_with(
        "declare const direct = import(\"./direct\");\n\
         declare const { [import(\"./key\")]: value } = source;\n\
         declare let { nested = import(\"./default\") } = source;\n",
        module_none_es2015_options(),
    );

    assert_eq!(js, "\"use strict\";\n");
    assert!(!js.contains("__importStar"), "{js}");
}

#[test]
fn test_module_none_erased_class_properties_do_not_inject_helpers() {
    let js = emit_ts_with(
        "abstract class AbstractFields {\n\
             abstract [import(\"./abstract\")]: unknown;\n\
             declare [import(\"./declared\")]: unknown;\n\
         }\n\
         class DeclaredField {\n\
             declare [import(\"./member\")]: unknown;\n\
         }\n",
        module_none_es2015_options(),
    );

    assert!(!js.contains("__importStar"), "{js}");
    assert!(!js.contains("require("), "{js}");
    assert!(js.contains("class AbstractFields"), "{js}");
    assert!(js.contains("class DeclaredField"), "{js}");
}

#[test]
fn test_module_none_legacy_decorated_declared_property_injects_helpers() {
    let js = emit_ts_with(
        "declare const dec: any;\n\
         class C {\n\
             @dec declare [import(\"./decorated-key\")]: unknown;\n\
         }\n",
        CompilerOptions {
            experimental_decorators: Some(true),
            ..module_none_es2015_options()
        },
    );

    assert_dynamic_import_helpers(&js);
    assert!(js.contains("__decorate(["), "{js}");
    assert!(
        js.contains("__importStar(require(\"./decorated-key\"))"),
        "{js}"
    );
}

#[test]
fn test_out_file_dynamic_import_hex_map_escapes_synthesized_literal() {
    let js = emit_ts_with(
        "const loaded = import('./dep');\n",
        CompilerOptions {
            module: Some(ModuleKind::None),
            target: Some(ScriptTarget::ES2015),
            out_file: Some("bundle.js".into()),
            other: vec![(
                "__tsrsOutFileDynamicImportMap".into(),
                "hex-v1\n2e2f646570\t62756e646c6527730969640a".into(),
            )],
            ..Default::default()
        },
    );

    assert!(js.contains("require('bundle\\'s\\tid\\n')"), "{js}");
}

fn emit_es5_params(source: &str) -> String {
    emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    )
}

#[test]
fn test_es5_simple_default_and_rest_function_parameters() {
    let js = emit_es5_params(
        "function f(a, b = mark('b'), c = mark('c'), ...rest) {\n\
             'ngInject';\n\
             return [this.tag, a, b, c, rest.join(','), arguments.length].join('|');\n\
         }",
    );

    assert!(js.contains("function f(a, b, c) {"), "{js}");
    let directive = js
        .find("ngInject")
        .unwrap_or_else(|| panic!("directive missing:\n{js}"));
    let default_b = js
        .find("if (b === void 0) { b = mark('b'); }")
        .expect("first default");
    let default_c = js
        .find("if (c === void 0) { c = mark('c'); }")
        .expect("second default");
    let rest = js.find("var rest = [];").expect("rest prologue");
    assert!(
        directive < default_b && default_b < default_c && default_c < rest,
        "{js}"
    );
    assert!(
        js.contains("for (var _i = 3; _i < arguments.length; _i++)")
            && js.contains("rest[_i - 3] = arguments[_i];"),
        "{js}"
    );

    let runtime = format!(
        "var events = []; function mark(x) {{ events.push(x); return x; }}\n{js}\n\
         console.log(f.call({{tag:'T'}}, 1, undefined, undefined, 4, 5));\n\
         console.log(events.join(','));"
    );
    assert_eq!(execute_with_node(&runtime), "T|1|b|c|4,5|5\nb,c");
}

#[test]
fn test_es5_simple_rest_function_expression_preserves_arguments_semantics() {
    let js = emit_es5_params(
        "const f = function (head = arguments.length, ...tail) {\n\
             return [head, tail.length, arguments.length].join(':');\n\
         };",
    );

    assert!(js.contains("function (head) {"), "{js}");
    assert!(
        js.contains("if (head === void 0) { head = arguments.length; }")
            && js.contains("for (var _i = 1; _i < arguments.length; _i++)")
            && js.contains("tail[_i - 1] = arguments[_i];"),
        "{js}"
    );
    assert_eq!(
        execute_with_node(&format!("{js}\nconsole.log(f(undefined, 2, 3));")),
        "3:2:3"
    );
}

#[test]
fn test_es5_simple_object_and_class_method_parameters() {
    let js = emit_es5_params(
        "const obj = { m(x = this.seed, ...rest) { return x + rest.length; } };\n\
         class C { m(x = this.seed, ...rest) { return x + rest.length; } }",
    );

    assert!(js.contains("m: function (x) {"), "{js}");
    assert!(js.contains("C.prototype.m = function (x) {"), "{js}");
    assert_eq!(
        js.matches("if (x === void 0) { x = this.seed; }").count(),
        2,
        "{js}"
    );
    assert_eq!(
        execute_with_node(&format!(
            "{js}\nobj.seed = 3; console.log(obj.m(undefined, 1, 2)); var c = new C(); c.seed = 4; console.log(c.m(undefined, 1));"
        )),
        "5\n5"
    );
}

#[test]
fn test_simple_parameter_lowering_is_narrowly_gated() {
    let es2015 = emit_ts_with(
        "function modern(x = 1, ...rest) { return rest; }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(
        es2015.contains("function modern(x = 1, ...rest)"),
        "{es2015}"
    );

    let es5 = emit_es5_params(
        "function destructured({x} = {x: 1}) { return x; }\n\
         async function asynchronous(x = 1) { return x; }\n\
         function* generator(x = 1) { yield x; }\n\
         class Derived extends Base { constructor(x = 1) { super(); } }",
    );
    assert!(
        es5.contains("function destructured({ x } = { x: 1 })"),
        "{es5}"
    );
    assert!(
        !es5.contains("function asynchronous(x) {\n    if (x === void 0)"),
        "{es5}"
    );
    assert!(es5.contains("x = 1"), "{es5}");
}

#[test]
fn test_es5_simple_block_arrow_parameter_bridge() {
    let js = emit_es5_params(
        "const f = (x = 2, ...rest) => { return x + rest.length; };\n\
         console.log(f(undefined, 3, 4));",
    );
    assert!(js.contains("var f = function (x) {"), "{js}");
    assert!(js.contains("if (x === void 0) { x = 2; }"), "{js}");
    assert!(
        js.contains("for (var _i = 1; _i < arguments.length; _i++)")
            && js.contains("rest[_i - 1] = arguments[_i];"),
        "{js}"
    );
    assert_eq!(execute_with_node(&js), "4");
}

#[test]
fn test_es5_simple_arrow_bridge_excludes_lexical_and_complex_boundaries() {
    let js = emit_es5_params(
        "function outer() {\n\
             const byThis = (x = 1) => { return this.value + x; };\n\
             const byArguments = (x = 1) => { return arguments.length + x; };\n\
             const concise = (x = 1) => x;\n\
             const destructured = ({x} = {x: 1}) => { return x; };\n\
             const asynchronous = async (x = 1) => { return x; };\n\
             return [byThis, byArguments, concise, destructured, asynchronous];\n\
         }",
    );
    assert!(js.contains("(x = 1) => { return this.value + x; }"), "{js}");
    assert!(
        js.contains("(x = 1) => { return arguments.length + x; }"),
        "{js}"
    );
    assert!(js.contains("(x = 1) => x"), "{js}");
    assert!(js.contains("({ x } = { x: 1 }) =>"), "{js}");
    assert!(!js.contains("var byThis = function"), "{js}");
    assert!(!js.contains("var byArguments = function"), "{js}");
}

#[test]
fn test_es5_rest_loop_index_is_collision_safe() {
    let js = emit_es5_params(
        "function f(_i, ...rest) { var _a = 7, total = 0; for (const value of rest) total += value; return [_i, _a, rest.join(','), total].join('|'); }\n\
         console.log(f(1, 2, 3));",
    );

    assert!(
        js.contains("for (var _b = 1; _b < arguments.length; _b++)"),
        "{js}"
    );
    assert!(js.contains("rest[_b - 1] = arguments[_b];"), "{js}");
    assert!(!js.contains("for (var _b = 0"), "{js}");
    assert_eq!(execute_with_node(&js), "1|7|2,3|5");
}

#[test]
fn test_es5_simple_parameter_bridge_rejects_comments_recovery_and_new_target() {
    let js = emit_es5_params(
        "function comments(/* lead */ x = 1, ...rest /* tail */) { return rest; }\n\
         function badPosition(...first, second) { return second; }\n\
         function badCount(...first, ...second) { return second; }\n\
         function lexicalMeta(x = new /* separated */ . target) { return x; }",
    );

    assert!(!js.contains("if (x === void 0)"), "{js}");
    assert!(!js.contains("var rest = []"), "{js}");
    assert!(!js.contains("var first = []"), "{js}");
    assert!(!js.contains("var second = []"), "{js}");
    assert!(js.contains("/* lead */"), "{js}");
    assert!(js.contains("/* tail */"), "{js}");
    assert!(js.contains("new /* separated */ . target"), "{js}");
}

#[test]
fn test_es5_legacy_method_bridge_detects_super_in_compound_statements() {
    let js = emit_es5_params(
        "class K {}\n\
         class B {}\n\
         B.prototype.K = K; B.prototype.nameKey = 'hit';\n\
         class C extends B {\n\
             m(x = 1) {\n\
                 for (;;) { if (x) break; }\n\
                 switch (x) { case 1: try { return super.m(); } finally {} }\n\
             }\n\
             heritage(x = 1) { return class D extends super.K {}; }\n\
             computed(x = 1) { return class D { [super.nameKey]() { return 7; } }; }\n\
             }\n\
         const obj = { m(x = super.value) { return x; } };\n\
         const c = new C();\n\
         console.log(new (c.heritage())() instanceof K);\n\
         console.log(new (c.computed())().hit());",
    );

    assert!(!js.contains("C.prototype.m = function"), "{js}");
    assert!(!js.contains("if (x === void 0)"), "{js}");
    assert!(js.contains("super.m()"), "{js}");
    assert!(!js.contains("m: function"), "{js}");
    assert!(js.contains("x = super.value"), "{js}");
    assert!(js.contains("class D extends super.K"), "{js}");
    assert!(js.contains("[super.nameKey]()"), "{js}");
    assert_eq!(execute_with_node(&js), "true\n7");
}

#[test]
fn test_es5_arrow_hazard_scan_is_structural_and_respects_boundaries() {
    let safe = emit_es5_params(
        "const safe = (x = 1, ...rest) => {\n\
             const thisValue = 'arguments new.target super';\n\
             /* this arguments super new.target */\n\
             function inner() { return [this, arguments, new.target]; }\n\
             class Inner { method() { return this; } }\n\
             return x + rest.length + thisValue.length - thisValue.length;\n\
         };\n\
         console.log(safe(undefined, 2, 3));",
    );
    assert!(safe.contains("var safe = function (x)"), "{safe}");
    assert_eq!(execute_with_node(&safe), "3");

    let hazards = emit_es5_params(
        "const throughLoop = (x = 1) => { while (x) { try { return this.v; } finally {} } };\n\
         const throughArrow = (x = 1) => { return (() => arguments.length)(); };\n\
         const separatedMeta = (x = 1) => { return new /* c */ . target; };",
    );
    assert!(hazards.contains("(x = 1) => { while"), "{hazards}");
    assert!(hazards.contains("(() => arguments.length)"), "{hazards}");
    assert!(hazards.contains("new /* c */ . target"), "{hazards}");
    assert!(
        hazards.contains("var throughLoop = (x = 1) =>"),
        "{hazards}"
    );
    assert!(
        hazards.contains("var throughArrow = (x = 1) =>"),
        "{hazards}"
    );
    assert!(
        hazards.contains("var separatedMeta = (x = 1) =>"),
        "{hazards}"
    );

    let class_externals = emit_es5_params(
        "function run() {\n\
             const heritage = (x = 1) => { class D extends this.Base {} return new D() instanceof this.Base; };\n\
             const computed = (x = 1) => { class D { [this.key]() { return 7; } } return new D()[this.key](); };\n\
             const classOwned = (x = 1) => { class D { static value = this; method() { return this; } } return x; };\n\
             return [heritage(), computed(), classOwned()].join('|');\n\
         }\n\
         console.log(run.call({ Base: function Base() {}, key: 'hit' }));",
    );
    assert!(
        class_externals.contains("var heritage = (x = 1) =>"),
        "{class_externals}"
    );
    assert!(
        class_externals.contains("var computed = (x = 1) =>"),
        "{class_externals}"
    );
    assert!(
        class_externals.contains("var classOwned = function (x)"),
        "{class_externals}"
    );
    assert_eq!(execute_with_node(&class_externals), "true|7|1");
}

#[test]
fn cjs_arbitrary_module_names_keep_string_literal_provenance() {
    let source = r#"
const local = 1;
export { local as "valid", local as "odd-name", local as "quote\"slash\\name" };
import { "valid" as importedValid, plain as importedPlain } from "dep";
console.log(importedValid(), importedPlain());
export { "valid" as publicName, plain as "quotedOut" } from "dep";
export * as "validNs" from "dep";
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2022),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );

    assert!(js.contains("exports[\"valid\"]"), "{js}");
    assert!(js.contains(" = void 0;"), "{js}");
    assert!(js.contains("exports[\"odd-name\"]"), "{js}");
    assert!(js.contains(r#"exports["quote\"slash\\name"]"#), "{js}");
    assert!(js.contains("(0, dep_1[\"valid\"])()"), "{js}");
    assert!(js.contains("(0, dep_1.plain)()"), "{js}");
    assert!(
        js.contains("get: function () { return dep_2[\"valid\"]; }")
            || js.contains("get: function () { return dep_3[\"valid\"]; }"),
        "quoted re-export source must be bracketed independently of publicName: {js}"
    );
    assert!(
        js.contains("get: function () { return dep_2.plain; }")
            || js.contains("get: function () { return dep_3.plain; }"),
        "unquoted re-export source must remain dotted: {js}"
    );
    assert!(js.contains("exports[\"validNs\"] = __importStar("), "{js}");

    let runnable = js.replace("require(\"dep\")", "globalThis.__dep");
    let runnable = format!(
        "globalThis.__dep = {{ __esModule: true, valid() {{ return 'quoted'; }}, plain() {{ return 'plain'; }} }};\n{runnable}\nconsole.log(exports[\"valid\"], exports[\"odd-name\"], exports[\"validNs\"].valid());"
    );
    assert_eq!(execute_with_node(&runnable), "quoted plain\n1 1 quoted");
}

#[test]
fn cjs_arbitrary_module_names_cook_hostile_string_escapes_exactly_once() {
    let js = emit_ts_with(
        r#"
const value = 7;
export {
    value as "\uD83D\uDE00",
    value as "\b",
    value as "\v",
    value as "\f",
    value as "\u2028"
};
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2022),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );

    for access in [
        r#"exports["\uD83D\uDE00"]"#,
        r#"exports["\b"]"#,
        r#"exports["\v"]"#,
        r#"exports["\f"]"#,
        r#"exports["\u2028"]"#,
    ] {
        assert!(js.contains(access), "missing {access}: {js}");
    }
    for doubled in [r#"["\\b"]"#, r#"["\\v"]"#, r#"["\\f"]"#] {
        assert!(!js.contains(doubled), "escape cooked twice: {js}");
    }
    assert_eq!(
        execute_with_node(&format!(
            "{js}\nconsole.log(exports['😀'], exports['\\b'], exports['\\v'], exports['\\f'], exports['\\u2028']);"
        )),
        "7 7 7 7 7"
    );
}

#[test]
fn cjs_arbitrary_export_writes_cover_for_loop_families() {
    let js = emit_ts_with(
        r#"
export { init as "init-name", key as "key-name", item as "item-name" };
for (var init = 0; init < 2; init++) {}
for (var key in { final: 1 }) console.log(key);
for (var item of [3, 4]) console.log(item);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );

    for name in ["init-name", "key-name", "item-name"] {
        assert!(js.contains(&format!(r#"exports["{name}"]"#)), "{js}");
        assert!(!js.contains(&format!("exports.{name}")), "{js}");
    }
    assert_node_syntax(&js);
    assert_eq!(
        execute_with_node(&format!(
            "{js}\nconsole.log(exports['init-name'], exports['key-name'], exports['item-name']);"
        )),
        "final\n3\n4\n2 final 4"
    );
}

#[test]
fn cjs_arbitrary_imports_survive_async_decorators_and_jsx_factories() {
    let async_js = emit_ts_with(
        r#"
import { "odd-name" as invoke } from "dep";
export async function run() { return invoke(); }
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert!(
        async_js.contains(r#"(0, dep_1["odd-name"])()"#),
        "{async_js}"
    );
    assert!(!async_js.contains("dep_1.odd-name"), "{async_js}");
    let runnable = async_js.replace("require(\"dep\")", "globalThis.__dep");
    assert_eq!(
        execute_with_node(&format!(
            "globalThis.__dep = {{ 'odd-name': function () {{ 'use strict'; return this === undefined ? 'detached' : 'bound'; }} }};\n{runnable}\nexports.run().then(console.log);"
        )),
        "detached"
    );

    let decorator_js = emit_ts_with(
        r#"
import { "odd-name" as decorate } from "dep";
@decorate
export class C {}
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            experimental_decorators: Some(true),
            ..Default::default()
        },
    );
    assert!(
        decorator_js.contains(r#"dep_1["odd-name"]"#),
        "{decorator_js}"
    );
    assert!(!decorator_js.contains("dep_1.odd-name"), "{decorator_js}");
    assert_node_syntax(&decorator_js);

    let jsx_js = emit_ts_file_with(
        "test.tsx",
        r#"
import { "odd-name" as h } from "dep";
console.log(<div />);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES2018),
            module: Some(ModuleKind::CommonJS),
            jsx: Some(JsxEmit::React),
            jsx_factory: Some("h".into()),
            ..Default::default()
        },
    );
    assert!(
        jsx_js.contains(r#"(0, dep_1["odd-name"])("div", null)"#),
        "{jsx_js}"
    );
    assert!(!jsx_js.contains("dep_1.odd-name"), "{jsx_js}");
    let runnable = jsx_js.replace("require(\"dep\")", "globalThis.__dep");
    assert_eq!(
        execute_with_node(&format!(
            "globalThis.__dep = {{ 'odd-name': (tag) => tag }};\n{runnable}"
        )),
        "div"
    );

    let namespace_factory = emit_ts_file_with(
        "control.tsx",
        "import * as React from 'react';\n<div />;\n",
        CompilerOptions {
            target: Some(ScriptTarget::ES2018),
            module: Some(ModuleKind::CommonJS),
            jsx: Some(JsxEmit::React),
            ..Default::default()
        },
    );
    assert!(
        namespace_factory.contains("React.createElement(\"div\", null)"),
        "{namespace_factory}"
    );
    assert!(
        !namespace_factory.contains("..createElement"),
        "{namespace_factory}"
    );
}

#[test]
fn cjs_arbitrary_imports_survive_direct_and_es5_computed_object_values() {
    let source = r#"
import { "odd-name" as quoted, "valid" as quotedValid, plain } from "dep";
const key = "computed";
const direct = { quoted, quotedValid, plain };
const computed = { [key]: 0, quoted };
console.log(direct.quoted, direct.quotedValid, direct.plain, computed.quoted);
"#;
    for target in [ScriptTarget::ES2022, ScriptTarget::ES5] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(target),
                module: Some(ModuleKind::CommonJS),
                ..Default::default()
            },
        );
        assert!(js.contains(r#"dep_1["odd-name"]"#), "{target:?}: {js}");
        assert!(js.contains(r#"dep_1["valid"]"#), "{target:?}: {js}");
        assert!(js.contains("dep_1.plain"), "{target:?}: {js}");
        assert!(!js.contains("dep_1.odd-name"), "{target:?}: {js}");
        let runnable = js.replace("require(\"dep\")", "globalThis.__dep");
        assert_eq!(
            execute_with_node(&format!(
                "globalThis.__dep = {{ 'odd-name': 1, valid: 2, plain: 3 }};\n{runnable}"
            )),
            "1 2 3 1",
            "{target:?}: {js}"
        );
    }
}

#[test]
fn cjs_arbitrary_imports_survive_legacy_decorator_metadata() {
    let js = emit_ts_with(
        r#"
import { "\uD83D\uDE00" as Smile, "\b" as Back, plain as Plain } from "dep";
function dec(..._args: any[]) {}
@dec
class C {
    @dec smile!: Smile;
    @dec standard!: Plain;
    constructor(value: Back) {}
}
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            experimental_decorators: Some(true),
            emit_decorator_metadata: Some(true),
            ..Default::default()
        },
    );

    assert!(
        js.contains(r#"__metadata("design:type", dep_1["\uD83D\uDE00"])"#),
        "{js}"
    );
    assert!(
        js.contains(r#"__metadata("design:paramtypes", [dep_1["\b"]])"#),
        "{js}"
    );
    assert!(
        js.contains(r#"__metadata("design:type", dep_1.plain)"#),
        "unquoted control must remain dotted: {js}"
    );
    assert!(!js.contains(r#"dep_1["\\b"]"#), "escape cooked twice: {js}");
    assert!(!js.contains("dep_1.\\uD83D"), "{js}");
    assert_node_syntax(&js);

    let runnable = js.replace("require(\"dep\")", "globalThis.__dep");
    assert_eq!(
        execute_with_node(&format!(
            "globalThis.__seen = [];\n\
             globalThis.__dep = {{ '😀': function Smile() {{}}, '\\b': function Back() {{}}, plain: function Plain() {{}} }};\n\
             Reflect.metadata = (key, value) => {{ __seen.push([key, value]); return () => {{}}; }};\n\
             {runnable}\n\
             console.log(\
                 __seen.some(([key, value]) => key === 'design:type' && value === __dep['😀']),\
                 __seen.some(([key, value]) => key === 'design:paramtypes' && value[0] === __dep['\\b']),\
                 __seen.some(([key, value]) => key === 'design:type' && value === __dep.plain)\
             );"
        )),
        "true true true"
    );
}

#[test]
fn test_es5_class_fields_initialize_after_super_and_before_constructor_body() {
    let js = emit_ts_with(
        r#"
var events: string[] = [];
class Base {
    base = (events.push("base field"), 1);
    constructor(value = 2) { "use strict"; events.push("base body " + value); }
}
class Derived extends Base {
    value = (events.push("derived field"), this.base + 3);
    constructor() { super(5); events.push("derived body " + this.value); }
}
class Implicit extends Base { value = this.base + 8; }
var d = new Derived();
console.log(events.join(","), d.value, new Implicit().value);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(
        !js.contains("class Base")
            && !js.contains("class Derived")
            && !js.contains("class Implicit"),
        "{js}"
    );
    assert_eq!(
        execute_with_node(&js),
        "base field,base body 5,derived field,derived body 4 4 9"
    );
    assert!(js.contains("_this.value ="), "{js}");
}

#[test]
fn test_es5_fields_use_the_object_returned_by_super() {
    let js = emit_ts_with(
        r#"
class Base { constructor() { return { value: 7 }; } }
class Derived extends Base {
    copied = this.value;
    constructor() { super(); this.copied += 1; }
}
console.log(new Derived().copied);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("class Derived"), "{js}");
    assert_eq!(execute_with_node(&js), "8");
}

#[test]
fn test_es5_static_fields_follow_methods_and_keep_class_this() {
    let js = emit_ts_with(
        r#"
class Values {
    static first = Values.make(3);
    static second = this.first + 4;
    private value = 8;
    static make<T>(value: T): T { return value; }
    read<T>(ignored: T) { return this.value; }
}
console.log(Values.first, Values.second, new Values().read(0));
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("class Values"), "{js}");
    assert!(
        js.find("Values.make = function").unwrap() < js.find("Values.first =").unwrap(),
        "{js}"
    );
    assert_eq!(execute_with_node(&js), "3 7 8");
}

#[test]
fn test_es5_erases_abstract_and_declared_members_before_class_lowering() {
    let js = emit_ts_with(
        r#"
abstract class Base {
    abstract property: number;
    abstract get value(): number;
    abstract set value(value: number);
    abstract method<T>(value: T): T;
    declare erased: string;
}
class Derived extends Base {
    property = 6;
    get value() { return this.property; }
    set value(value: number) { this.property = value; }
    method<T>(value: T): T { return value; }
}
var value = new Derived(); value.value = 9;
console.log(value.method(value.value), Object.keys(Base.prototype).join(","));
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(
        !js.contains("class Base") && !js.contains("class Derived"),
        "{js}"
    );
    assert_eq!(execute_with_node(&js), "9");
}

#[test]
fn test_es5_field_initializers_preserve_function_this_and_constructor_temp_scope() {
    let js = emit_ts_with(
        r#"
class Base {
    value = 3;
    arrow = () => this.value;
    fn = function () { return this.value; };
    static value = 7;
    static arrow = () => this.value;
    static fn = function () { return this.value; };
}
class Derived extends Base {
    computed = ({ value: this.value })?.value;
    arrow = () => this.computed;
    fn = function () { return this.value; };
}
class Explicit extends Base {
    computed = ({ value: this.value })?.value;
    constructor() { super(); }
}
var value = new Derived();
console.log(value.arrow.call({ value: 99 }), value.fn.call({ value: 11 }),
    Base.arrow.call({ value: 99 }), Base.fn.call({ value: 13 }), new Explicit().computed);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(
        !js.contains("class Base") && !js.contains("class Derived"),
        "{js}"
    );
    assert!(js.contains("function Derived() {\n        var _a;"), "{js}");
    assert!(
        js.contains("function Explicit() {\n        var _a;"),
        "{js}"
    );
    assert_eq!(execute_with_node(&js), "3 11 7 13 3");
}

#[test]
fn test_es5_nested_class_field_initializers_keep_their_native_environment() {
    let js = emit_ts_with(
        r#"
class Base {}
class Derived extends Base {
    value = 3;
    Nested = class { value = 19; read = this.value; };
}
var value = new Derived();
console.log(new value.Nested().read);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(js.contains("class Derived extends Base"), "{js}");
    assert_eq!(execute_with_node(&js), "19");
}

#[test]
fn test_es5_constructor_field_temps_preserve_directive_prologues() {
    let js = emit_ts_with(
        r#"
class Value {
    value = ({ value: 1 })?.value;
    probe = function () { return this === void 0; };
    constructor() { "use strict"; }
}
var probe = new Value().probe;
console.log(probe());
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            always_strict: Some(false),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("class Value"), "{js}");
    assert!(
        js.contains("function Value() {\n        \"use strict\";\n        var _a;"),
        "{js}"
    );
    assert_eq!(execute_with_node(&js), "true");
}

#[test]
fn test_es5_async_state_machine_preserves_suspension_and_receiver() {
    let js = emit_ts_with(
        r#"
async function compute(delta: number) {
    var value = await Promise.resolve(this.value + delta);
    if (value > 4) { return await Promise.resolve(value * 2); }
    else { return 0; }
}
Promise.all([compute.call({value: 4}, 2), compute.call({value: 1}, 1)]).then(console.log);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert!(js.contains("__generator(this"), "{js}");
    assert_eq!(execute_with_node(&js), "[ 12, 0 ]");
}

#[test]
fn test_es5_async_state_machine_propagates_rejection_and_empty_completion() {
    let js = emit_ts_with(
        r#"
async function empty() {}
async function fail() { await Promise.reject("failed"); return "unreachable"; }
Promise.all([empty(), fail().catch(function (error) { return error; })]).then(console.log);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), "[ undefined, 'failed' ]");
}

#[test]
fn test_es5_async_state_machine_keeps_parameter_redeclarations_and_temps_distinct() {
    let js = emit_ts_with(
        r#"
async function compute(value, _a) {
    var value;
    var other = ({ value: value })?.value;
    var result = await Promise.resolve(other + _a);
    return result;
}
Promise.all([compute(3, 4), compute(5, 6)]).then(console.log);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert!(!js.contains("var value;"), "{js}");
    assert_eq!(execute_with_node(&js), "[ 7, 11 ]");
}

#[test]
fn test_es5_async_state_machine_preserves_directives() {
    let js = emit_ts_with(
        r#"
async function probe() {
    "use strict";
    var value = await Promise.resolve(3);
    var check = function () { return this === void 0; };
    return check();
}
probe().then(console.log);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::None),
            always_strict: Some(false),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert!(
        js.contains("function () {\n        \"use strict\";\n        var value, check;"),
        "{js}"
    );
    assert_eq!(execute_with_node(&js), "true");
}

#[test]
fn test_es5_async_state_machine_rejects_partial_expression_plans() {
    let js = emit_ts_with(
        r#"
var events = [];
function before() { events.push("before"); return 4; }
async function compute() {
    var value = 2;
    return eval(await Promise.resolve("before() + value"));
}
compute().then(function (value) { console.log(value, events.join(",")); });
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(
        !js.contains("__generator"),
        "an unused helper was emitted: {js}"
    );
    assert!(js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), "6 before");
}

#[test]
fn test_es5_async_state_machine_updates_resume_labels_on_branch_fallthrough() {
    let js = emit_ts_with(
        r#"
var calls = 0;
function step() { calls++; return Promise.resolve(7); }
async function compute(flag) {
    if (flag) { await Promise.resolve(1); } else { 0; }
    var value = await step();
    return value;
}
Promise.all([compute(true), compute(false)]).then(function (values) {
    console.log(values.join(","), calls);
});
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), "7,7 2");
}

#[test]
fn test_es5_async_state_machine_preserves_comma_return_values() {
    let js = emit_ts_with(
        r#"
var values = [];
async function compute() {
    var value = await Promise.resolve(null) ?? 3;
    return values.push(value), 7;
}
compute().then(function (value) { console.log(value, values.join(",")); });
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), "7 3");
}

#[test]
fn test_es5_async_state_machine_locals_shadow_imports_and_exports() {
    let js = emit_ts_with(
        r#"
import { value } from "dep";
export var state = 11;
async function compute() {
    var value = 3;
    var state = 4;
    var result = await Promise.resolve(value + state);
    return result;
}
console.log(value);
compute().then(console.log);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    let runnable = format!("function require(name) {{ return {{ value: 20 }}; }}\n{js}");
    assert_eq!(execute_with_node(&runnable), "20\n7");
}

#[test]
fn test_es5_async_expression_staging_freezes_operands_and_call_receivers() {
    let source = r#"
var log = [];
var first = 3;
var receiver = { value: 2, get method() {
    log.push("get");
    return function (a, b, c) { log.push("call"); return this.value + a + b + c; };
}};
function suspend() {
    log.push("await"); first = 50; receiver = { value: 100 };
    return Promise.resolve(4);
}
async function compute() { return receiver.method(first, await suspend(), 5); }
async function sum() { return first + await Promise.resolve(first = 90); }
compute().then(function (value) {
    console.log(value, log.join(","));
    sum().then(console.log);
});
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "14 get,await,call\n140");
}

#[test]
fn test_es5_async_expression_staging_preserves_lazy_branches() {
    let source = r#"
var log = [];
function step(name, value) { log.push(name); return Promise.resolve(value); }
async function compute() {
    var a = false && await step("bad-and", 1);
    var b = true || await step("bad-or", 2);
    var c = 3 ?? await step("bad-nullish", 3);
    var d = true && await step("and", 4);
    var e = false || await step("or", 5);
    var f = null ?? await step("nullish", 6);
    var g = await step("test", false) ? await step("bad-arm", 7) : await step("arm", 8);
    return [a, b, c, d, e, f, g].join(",");
}
compute().then(function (value) { console.log(value, log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(
        execute_with_node(&js),
        "false,true,3,4,5,6,8 and,or,nullish,test,arm"
    );
}

#[test]
fn test_es5_async_expression_staging_freezes_assignment_references() {
    let source = r#"
var original = {}, object = original, key = "first";
function change() { object = {}; key = "second"; return Promise.resolve(7); }
async function compute() { object[key] = await change(); return original.first; }
compute().then(function (value) { console.log(value, Object.keys(object).length); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "7 0");
}

#[test]
fn test_es5_async_expression_staging_propagates_discarded_await_rejections() {
    let source = r#"
var log = [];
async function compute() {
    return log.push("first"), await Promise.reject("stop"), await Promise.resolve(log.push("bad"));
}
compute().catch(function (error) { console.log(error, log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "stop first");
}

#[test]
fn test_es5_async_expression_staging_preserves_compound_getter_setter_order() {
    let source = r#"
var log = [], keyName = "a", result = 0;
var key = { toString() { log.push("key-" + keyName); return keyName; } };
var object = { get a() { log.push("get"); return 2; }, set b(value) { log.push("set"); result = value; } };
function suspend() { log.push("await"); keyName = "b"; return Promise.resolve(3); }
async function compute() { return object[key] += await suspend(); }
compute().then(function (value) { console.log(value, result, log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "5 5 key-a,get,await,key-b,set");
}

#[test]
fn test_es5_async_expression_staging_skips_logical_assignment_setters() {
    let source = r#"
var log = [], stored = 3;
var object = { get value() { log.push("get"); return stored; }, set value(value) { log.push("set"); stored = value; } };
function step() { log.push("await"); return Promise.resolve(8); }
async function compute() {
    var first = object.value ||= await step();
    var second = object.value &&= await step();
    var third = object.value ??= await step();
    return [first, second, third].join(",");
}
compute().then(function (value) { console.log(value, stored, log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "3,8,8 8 get,get,await,set,get");
}

#[test]
fn test_es5_async_expression_staging_preserves_array_holes_and_argument_prefixes() {
    let source = r#"
var value = 1;
function step(next) { value = next; return Promise.resolve(next); }
function collect() { return Array.prototype.join.call(arguments, ","); }
async function compute() {
    var array = [value, , await step(2), value, await step(3), , value];
    var args = collect(value, await step(4), value, await step(5), value);
    return array.join(",") + ";" + (1 in array) + "," + (5 in array) + ";" + args;
}
compute().then(console.log);
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "1,,2,2,3,,3;false,false;3,4,4,5,5");
}

#[test]
fn test_es5_async_loops_preserve_awaited_conditions_and_early_exits() {
    let source = r#"
var log = [];
function step(name, value) { log.push(name); return Promise.resolve(value); }
async function run() {
    var i = 0;
    while (await step("test" + i, i < 4)) {
        i++;
        if (i === 1) continue;
        await step("body" + i, 0);
        if (i === 3) break;
    }
    return i;
}
run().then(function (value) { console.log(value, log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "3 test0,test1,body2,test2,body3");
}

#[test]
fn test_es5_async_loops_do_continue_reaches_condition_and_break_skips_it() {
    let source = r#"
var log = [];
function step(name, value) { log.push(name); return Promise.resolve(value); }
async function run() {
    var i = 0;
    again: do {
        i++;
        if (i === 1) continue again;
        await step("body" + i, 0);
        if (i === 3) break again;
    } while (await step("test" + i, i < 5));
    return i;
}
run().then(function (value) { console.log(value, log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "3 test1,body2,test2,body3");
}

#[test]
fn test_es5_async_loops_for_awaited_initializer_condition_and_update() {
    let source = r#"
var log = [];
function step(name, value) { log.push(name); return Promise.resolve(value); }
async function run() {
    for (var a = (log.push("first"), 7), i = await step("init", 0);
         await step("test" + i, i < 4); i = await step("update" + i, i + 1)) {
        if (i === 0) continue;
        await step("body" + i, 0);
        if (i === 2) break;
    }
    return a + i;
}
run().then(function (value) { console.log(value, log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(
        execute_with_node(&js),
        "9 first,init,test0,update0,test1,body1,update1,test2,body2"
    );
}

#[test]
fn test_es5_async_loops_resolve_nested_native_and_suspending_labels() {
    let source = r#"
async function run() {
    var log = [], i = 0;
    outer: alias: while (i < 4) {
        await Promise.resolve();
        i++;
        while (i === 1) { log.push("native-continue"); continue alias; }
        for (var j = 0; j < 3; j++) {
            if (j === 0) continue;
            log.push(i + ":" + j);
            if (j === 1) break;
        }
        inner: do {
            await Promise.resolve();
            if (i === 2) { log.push("suspended-continue"); continue outer; }
            break inner;
        } while (false);
        while (i === 3) { log.push("native-break"); break outer; }
    }
    return log.join(",");
}
run().then(console.log);
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(
        execute_with_node(&js),
        "native-continue,2:1,suspended-continue,3:1,native-break"
    );
}

#[test]
fn test_es5_async_loops_preserve_native_return_and_var_hoisting() {
    let source = r#"
async function native(flag) {
    if (flag) {
        for (var i = 0, j = 1; i < 3; i++) {
            if (i === 2) return i + j;
        }
    }
    do { var value = 9; break; } while (false);
    while (value > 0) value--;
    return [i, j, value].join(":");
}
async function suspended() {
    var i = 0;
    for (;;) {
        await Promise.resolve();
        if (++i === 2) return i;
    }
}
Promise.all([native(true), native(false), suspended()]).then(function (values) { console.log(values.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "3,::0,2");
}

#[test]
fn test_es5_async_loops_propagate_condition_body_and_update_rejections() {
    let source = r#"
var log = [];
async function condition() {
    while (await Promise.reject("condition")) { log.push("bad-body"); }
}
async function body() {
    do { await Promise.reject("body"); } while (log.push("bad-test"));
}
async function update() {
    for (var i = 0; i < 2; await Promise.reject("update")) { log.push("body"); }
}
Promise.all([condition().catch(function (e) { return e; }), body().catch(function (e) { return e; }), update().catch(function (e) { return e; })]).then(function (values) { console.log(values.join(","), log.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "condition,body,update body");
}

#[test]
fn test_es5_async_expression_forms_find_helpers_in_arrays_and_conditionals() {
    let source = r#"
var functions = [async (x) => await Promise.resolve(x + 1),
    true ? async (x) => { return await Promise.resolve(x + 2); } : null, async (x) => x];
Promise.all([functions[0](2), functions[1](2), functions[2](2)]).then(function (values) { console.log(values.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert!(!js.contains("=>"), "{js}");
    assert_eq!(js.matches("var __generator =").count(), 1, "{js}");
    assert_eq!(js.matches("var __awaiter =").count(), 1, "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "3,4,2");
}

#[test]
fn test_es5_async_expression_forms_preserve_function_expression_receivers() {
    let source = r#"
var object = { value: 7, method: async function (x) {
    await Promise.resolve(); return this.value + x + arguments[0];
}};
var functions = [async function (x) { return await Promise.resolve(x * 2); }];
Promise.all([object.method(2), functions[0](5)]).then(function (values) { console.log(values.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "11,10");
}

#[test]
fn test_es5_async_expression_forms_keep_lexical_arguments_aliases_in_each_arrow() {
    let source = r#"
function factory(x) {
    var first = async () => { await Promise.resolve(); return this.value + arguments[0]; };
    var second = async () => { await Promise.resolve(); return this.value * arguments[0]; };
    return [first, second];
}
var callbacks = factory.call({ value: 6 }, 3);
Promise.all([callbacks[0].call({ value: 90 }, 40), callbacks[1]()]).then(function (values) { console.log(values.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(execute_with_node(&js), "9,18");
}

#[test]
fn test_es5_async_expression_forms_do_not_suspend_the_enclosing_function() {
    let source = r#"
var log = [];
async function create(x) {
    var nested = async function (y) { log.push("nested"); return x + await Promise.resolve(y); };
    log.push("created");
    return nested;
}
create(8).then(function (nested) {
    console.log(log.join(","));
    nested(4).then(function (value) { console.log(value, log.join(",")); });
});
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "created\n12 created,nested");
}

#[test]
fn test_es5_async_expression_forms_handle_getters_object_methods_and_arguments() {
    let source = r#"
var object = {
    async method(x) { return await Promise.resolve(x + 1); },
    get callback() { return async function (x) { return await Promise.resolve(x + 2); }; }
};
function identity(x) { return x; }
var callback = identity(async function (x) { return await Promise.resolve(x + 3); });
Promise.all([object.method(1), object.callback(1), callback(1)]).then(function (values) { console.log(values.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "2,3,4");
}

#[test]
fn test_es5_async_expression_forms_propagate_rejection_and_keep_invocations_separate() {
    let source = r#"
var callback = async (x) => {
    var value = await Promise.resolve(x);
    if (value < 0) throw new Error("negative");
    return value * 2;
};
Promise.all([callback(3), callback(5), callback(-1).catch(function (e) { return e.message; })]).then(function (values) { console.log(values.join(",")); });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert!(!js.contains("=>"), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source));
    assert_eq!(execute_with_node(&js), "6,10,negative");
}

fn assert_es5_async_runtime(source: &str, expected: &str) {
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            always_strict: Some(false),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    let actual = execute_with_node(&js);
    assert_eq!(actual, execute_with_node(source), "{js}");
    assert_eq!(actual, expected, "{js}");
}

#[test]
fn test_es5_async_exceptions_catch_sync_and_async_failures() {
    assert_es5_async_runtime(
        r#"
async function run(mode) {
    try {
        if (mode === 0) throw "sync";
        if (mode === 1) await Promise.reject("async");
        return await Promise.resolve("ok");
    } catch (error) {
        var result = await Promise.resolve(error + "-caught");
        return result;
    }
}
Promise.all([run(0), run(1), run(2)]).then(function (values) { console.log(values.join(",")); });
"#,
        "sync-caught,async-caught,ok",
    );
}

#[test]
fn test_es5_async_exceptions_finally_preserves_pending_completion() {
    assert_es5_async_runtime(r#"
async function run(mode) {
    var log = [];
    try {
        try {
            log.push("try");
            await Promise.resolve();
            if (mode === 0) return log;
            if (mode === 1) throw "fail";
            log.push("normal");
        } finally {
            log.push("finally");
            await Promise.resolve();
            log.push("done");
        }
        log.push("after");
    } catch (error) { log.push(error); }
    return log;
}
Promise.all([run(0), run(1), run(2)]).then(function (values) { console.log(JSON.stringify(values)); });
"#, "[[\"try\",\"finally\",\"done\"],[\"try\",\"finally\",\"done\",\"fail\"],[\"try\",\"normal\",\"finally\",\"done\",\"after\"]]");
}

#[test]
fn test_es5_async_exceptions_finally_overrides_return_throw_and_rejection() {
    assert_es5_async_runtime(
        r#"
async function run(mode) {
    try {
        if (mode === 0) throw "original";
        return await Promise.resolve("original");
    } finally {
        if (mode === 0) return await Promise.resolve("replacement");
        if (mode === 1) throw "thrown";
        await Promise.reject("rejected");
    }
}
Promise.all([run(0), run(1).catch(function (e) { return e; }), run(2).catch(function (e) { return e; })]).then(function (values) { console.log(values.join(",")); });
"#,
        "replacement,thrown,rejected",
    );
}

#[test]
fn test_es5_async_exceptions_run_finally_for_loop_control() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    outer: for (var i = 0; i < 3; i++) {
        try {
            await Promise.resolve();
            for (var j = 0; j < 2; j++) {
                try {
                    log.push(i + ":" + j);
                    if (i === 0) continue outer;
                    if (i === 2) break outer;
                } finally { log.push("inner"); }
            }
        } finally { log.push("outer"); await Promise.resolve(); }
    }
    return log.join(",");
}
run().then(console.log);
"#,
        "0:0,inner,outer,1:0,inner,1:1,inner,outer,2:0,inner,outer",
    );
}

#[test]
fn test_es5_async_exceptions_nested_handlers_rethrow_to_correct_scope() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    try {
        try { await Promise.reject("first"); }
        catch (e) {
            log.push(e);
            try { throw "second"; }
            catch (e) { log.push(e); }
            log.push(e);
            throw await Promise.resolve("third");
        } finally { log.push("inner-finally"); }
    } catch (e) { log.push(e); }
    finally { log.push("outer-finally"); await Promise.resolve(); }
    return log.join(",");
}
run().then(console.log);
"#,
        "first,second,first,inner-finally,third,outer-finally",
    );
}

#[test]
fn test_es5_async_exceptions_preserve_catch_bindings_and_closures() {
    assert_es5_async_runtime(
        r#"
var e_1 = "reserved";
async function run(value) {
    var e = "outer";
    var captured;
    try { await Promise.reject(value); }
    catch (e) {
        captured = () => e;
        var object = { e };
        var shadowed = (e) => e;
        var e = await Promise.resolve(object.e + "-changed");
        for (var e = e + "-loop", i = 0; i < 1; i++) { await Promise.resolve(); }
        try { throw "native"; }
        catch (e) { var e = "native-changed"; }
        return [captured(), object.e, shadowed("shadow"), e_1].join(":");
    }
    return e;
}
Promise.all([run("one"), run("two")]).then(function (values) { console.log(values.join(",")); });
"#,
        "one-changed-loop:one:shadow:reserved,two-changed-loop:two:shadow:reserved",
    );
}

#[test]
fn test_es5_async_exceptions_support_optional_catch_bindings() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    try { throw "native"; } catch { log.push("native"); }
    try { await Promise.reject("async"); } catch { log.push(await Promise.resolve("async")); }
    return log.join(",");
}
run().then(console.log);
"#,
        "native,async",
    );
}

#[test]
fn test_es5_async_exceptions_keep_native_finally_completion() {
    assert_es5_async_runtime(
        r#"
async function run(mode) {
    var log = [];
    try {
        try { if (mode) return log; throw "error"; }
        finally { log.push("inner"); }
    } catch (error) { log.push(error); }
    finally { log.push("outer"); }
    return log;
}
Promise.all([run(true), run(false)]).then(function (values) { console.log(JSON.stringify(values)); });
"#,
        "[[\"inner\",\"outer\"],[\"inner\",\"error\",\"outer\"]]",
    );
}

#[test]
fn test_es5_async_exceptions_keep_unsupported_catch_environments_atomic() {
    for source in [
        "async function run() { var f = []; for (var i = 0; i < 2; i++) { try { await Promise.reject(i); } catch (e) { f.push(() => e); } } return f; }",
        "async function run() { try { await Promise.reject({value: 1}); } catch ({value}) { return value; } }",
        "async function run() { try { await Promise.reject(1); } catch (e) { return eval('e'); } }",
    ] {
        let js = emit_ts_with(source, CompilerOptions { target: Some(ScriptTarget::ES5), ..Default::default() });
        assert!(js.contains("function*"), "{js}");
        assert!(!js.contains(".trys.push("), "{js}");
        assert_node_syntax(&js);
    }
}

#[test]
fn test_es5_async_exceptions_catch_arguments_shadows_function_arguments() {
    assert_es5_async_runtime(
        r#"
async function run(value) {
    var before = arguments[0];
    try { await Promise.reject(["caught"]); }
    catch (arguments) {
        var first = () => arguments[0];
        var normal = function (value) { return arguments[0]; };
        var method = { read(value) { return arguments[0]; } };
        await Promise.resolve();
        try { throw ["native"]; }
        catch (arguments) { var second = arguments[0]; }
        return [before, first(), second, normal("normal"), method.read("method")].join(",");
    }
}
run("input").then(console.log);
"#,
        "input,caught,native,normal,method",
    );
}

#[test]
fn test_es5_async_exceptions_catch_bindings_shadow_commonjs_imports() {
    let js = emit_ts_with(
        r#"
import { error } from "dependency";
export async function run() {
    var before = error;
    try { await Promise.reject("caught"); }
    catch (error) {
        var captured = () => error;
        var object = { error };
        await Promise.resolve();
        try { throw "native"; }
        catch (error) { var native = { error }.error; }
        return [before, captured(), object.error, native].join(",");
    }
}
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    let runtime = format!("var exports = {{}}; var require = function () {{ return {{error: 'imported'}}; }};\n{js}\nexports.run().then(console.log);");
    assert_eq!(
        execute_with_node(&runtime),
        "imported,caught,caught,native",
        "{js}"
    );
}

#[test]
fn test_es5_async_exceptions_finally_loop_control_replaces_pending_completion() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    for (var i = 0; i < 3; i++) {
        try {
            try { return "wrong"; }
            finally { await Promise.resolve(); log.push(i); continue; }
        } finally { log.push("outer"); }
    }
    while (true) {
        try { throw "wrong"; }
        finally { await Promise.resolve(); log.push("break"); break; }
    }
    return log.join(",");
}
run().then(console.log);
"#,
        "0,outer,1,outer,2,outer,break",
    );
}

#[test]
fn test_es5_async_switches_evaluate_tests_until_first_match() {
    assert_es5_async_runtime(
        r#"
var log = [];
var value = 2;
function mark(name, result) { log.push(name); value = 99; return result; }
async function run() {
    switch (value) {
        case mark("first", 1): log.push("bad-first"); break;
        case await Promise.resolve(mark("second", 2)): log.push("matched"); break;
        case await Promise.reject("bad-rejection"): log.push("bad-third"); break;
        default: log.push("bad-default");
    }
    return log.join(",");
}
run().then(console.log);
"#,
        "first,second,matched",
    );
}

#[test]
fn test_es5_async_switches_default_position_and_empty_arm_fallthrough() {
    assert_es5_async_runtime(r#"
async function run(value) {
    var log = [];
    switch (value) {
        case 0: log.push("zero");
        default: log.push("default");
        case 1:
        case await Promise.resolve(2): log.push("two");
        case 3: log.push(await Promise.resolve("three")); break;
        case 4:
    }
    return log.join(":");
}
Promise.all([run(0), run(1), run(2), run(3), run(4), run(9)]).then(function (values) { console.log(JSON.stringify(values)); });
"#, "[\"zero:default:two:three\",\"two:three\",\"two:three\",\"three\",\"\",\"default:two:three\"]");
}

#[test]
fn test_es5_async_switches_no_match_and_default_only() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    switch (9) {
        case (log.push("first"), 1): log.push("bad"); break;
        case await Promise.resolve((log.push("second"), 2)): log.push("bad"); break;
    }
    switch (await Promise.resolve(log.push("empty"))) {}
    switch (log.push("value")) {
        default: log.push(await Promise.resolve("default"));
    }
    log.push("after");
    return log.join(",");
}
run().then(console.log);
"#,
        "first,second,empty,value,default,after",
    );
}

#[test]
fn test_es5_async_switches_native_discriminant_await_and_loop_control() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    for (var i = 0; i < 3; i++) {
        switch (await Promise.resolve(i)) {
            case 0: log.push("continue"); continue;
            case 1: log.push("break"); break;
            default: log.push("default");
        }
        log.push(i);
    }
    for (var j = 0; j < 3; j++) {
        switch (j) {
            case 0: continue;
            case 1: break;
            default: return log.join(",");
        }
        log.push("native");
    }
}
run().then(console.log);
"#,
        "continue,break,1,default,2,native",
    );
}

#[test]
fn test_es5_async_switches_labeled_exits_and_nested_control_scopes() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    outer: for (var i = 0; i < 3; i++) {
        choose: switch (i) {
            case 0:
                switch (await Promise.resolve(i)) {
                    case 0: log.push("continue"); continue outer;
                }
            case 1:
                for (var j = 0; j < 2; j++) {
                    switch (j) { case 0: log.push("inner"); break; }
                    if (j === 1) break choose;
                }
            default: log.push(await Promise.resolve("default")); break outer;
        }
        log.push("after-switch");
    }
    native: switch (1) {
        case 1: while (true) { log.push("native-label"); break native; }
        default: log.push("bad");
    }
    return log.join(",");
}
run().then(console.log);
"#,
        "continue,inner,after-switch,default,native-label",
    );
}

#[test]
fn test_es5_async_switches_breaks_run_finalizers_and_finally_can_continue() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    for (var i = 0; i < 2; i++) {
        try {
            switch (i) {
                case 0:
                    try { await Promise.resolve(); log.push("zero"); break; }
                    finally { log.push("inner"); await Promise.resolve(); }
                case 1:
                    try { return "bad"; }
                    finally { log.push(await Promise.resolve("continue")); continue; }
            }
            log.push("after-switch");
        } finally { log.push("outer"); await Promise.resolve(); }
    }
    return log.join(",");
}
run().then(console.log);
"#,
        "zero,inner,after-switch,outer,continue,outer",
    );
}

#[test]
fn test_es5_async_switches_rejections_and_case_expression_order() {
    assert_es5_async_runtime(
        r#"
async function run(mode) {
    var log = [];
    try {
        switch (mode === 0 ? await Promise.reject("value") : 5) {
            case (log.push("first"), 1): break;
            default: log.push("bad-default"); break;
            case (await Promise.resolve(2)) + (await Promise.reject("test")): break;
            case (log.push("bad-last"), 5): break;
        }
    } catch (e) { log.push(e); }
    finally { log.push(await Promise.resolve("finally")); }
    return log.join(":");
}
Promise.all([run(0), run(1)]).then(function (values) { console.log(values.join(",")); });
"#,
        "value:finally,first:test:finally",
    );
}

#[test]
fn test_es5_async_switches_keep_catch_closures_without_loop_environments() {
    assert_es5_async_runtime(
        r#"
async function run(value) {
    switch (value) {
        case 1:
            try { await Promise.reject(value); }
            catch (e) { return () => e; }
        default: return () => 0;
    }
}
Promise.all([run(1), run(2)]).then(function (values) { console.log(values[0](), values[1]()); });
"#,
        "1 0",
    );
}

#[test]
fn test_es5_async_switches_use_strict_equality_and_object_identity() {
    assert_es5_async_runtime(
        r#"
var object = {};
async function run(value) {
    switch (value) {
        case await Promise.resolve(NaN): return "bad-nan";
        case "1": return "string";
        case 1: return "number";
        case await Promise.resolve(object): return "object";
        case 0: return "zero";
        default: return "none";
    }
}
Promise.all([run(NaN), run("1"), run(1), run(object), run({}), run(-0)]).then(function (values) { console.log(values.join(",")); });
"#,
        "none,string,number,object,none,zero",
    );
}

#[test]
fn test_es5_async_for_in_preserves_order_inheritance_and_single_rhs_evaluation() {
    assert_es5_async_runtime(
        r#"
var reads = 0;
var getters = 0;
var parent = { inherited: 1 };
var object = Object.create(parent);
object.beta = 1;
object[2] = 2;
object[1] = 3;
Object.defineProperty(object, "hidden", { value: 0, enumerable: false });
Object.defineProperty(object, "getter", { get: function () { getters++; return 4; }, enumerable: true });
function source() { reads++; return object; }
async function run() {
    var log = [];
    for (var key in await Promise.resolve(source())) { log.push(key); }
    return log.join(",") + ":" + reads + ":" + getters;
}
run().then(console.log);
"#,
        "1,2,beta,getter,inherited:1:0",
    );
}

#[test]
fn test_es5_async_for_in_skips_deleted_keys_and_captures_original_object() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var original = { first: 1, removed: 2, last: 3 };
    var object = original;
    var log = [];
    for (var key in object) {
        log.push(key);
        await Promise.resolve();
        delete original.removed;
        original.added = 4;
        object = { wrong: 0 };
    }
    return log.join(",");
}
run().then(console.log);
"#,
        "first,last",
    );
}

#[test]
fn test_es5_async_for_in_evaluates_awaited_targets_each_iteration() {
    assert_es5_async_runtime(
        r#"
var log = [];
var target = {};
function receiver() { log.push("receiver"); return target; }
function index() { log.push("index"); return Promise.resolve("key"); }
async function run() {
    for ((await Promise.resolve(receiver())).key in { a: 1, b: 2 }) { log.push(target.key); }
    for (receiver()[await index()] in { c: 1, d: 2 }) { log.push(target.key); }
    for (receiver()[await index()] in {}) { log.push("bad-empty"); }
    return log.join(",");
}
run().then(console.log);
"#,
        "receiver,a,receiver,b,receiver,index,c,receiver,index,d",
    );
}

#[test]
fn test_es5_async_for_in_nested_labels_finalizers_and_temp_collisions() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var _i = "source-i", _a = "source-a", _g = "source-g";
    var log = [];
    outer: for (var key in { a: 1, b: 2, c: 3 }) {
        try {
            for (var inner in { one: 1, two: 2 }) {
                await Promise.resolve();
                log.push(key + ":" + inner);
                if (key === "a") continue outer;
                if (inner === "two") break outer;
            }
        } finally { log.push("finally"); await Promise.resolve(); }
    }
    return log.join(",") + ":" + [_i, _a, _g, key, inner].join("/");
}
run().then(console.log);
"#,
        "a:one,finally,b:one,b:two,finally:source-i/source-a/source-g/b/two",
    );
}

#[test]
fn test_es5_async_for_in_var_hoisting_and_concurrent_invocations() {
    assert_es5_async_runtime(
        r#"
async function run(key, object, enabled) {
    var log = [];
    if (enabled) {
        for (var key in await Promise.resolve(object)) { log.push(key); await Promise.resolve(); }
    }
    for (var never in {}) { log.push("bad"); }
    return log.join(":") + "/" + key + "/" + never;
}
Promise.all([run("initial", { a: 1, b: 2 }, true), run("other", { c: 3 }, true), run("unchanged", {}, false)]).then(function (values) { console.log(values.join(",")); });
"#,
        "a:b/b/undefined,c/c/undefined,/unchanged/undefined",
    );
}

#[test]
fn test_es5_async_for_in_retains_catch_binding_identity_in_targets() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var e = "outer";
    var saved;
    try { await Promise.reject("caught"); }
    catch (e) {
        saved = () => e;
        e = await Promise.resolve("assigned");
        var before = saved();
        for (e in await Promise.resolve({ first: 1, last: 2 })) { await Promise.resolve(); }
        var after = saved();
        for (var e in { native: 1 }) {}
        return [before, after, saved()].join(",");
    }
    return e;
}
run().then(console.log);
"#,
        "assigned,last,native",
    );
}

#[test]
fn test_es5_async_for_in_propagates_rejections_and_enumeration_errors() {
    assert_es5_async_runtime(
        r#"
async function run(mode) {
    var log = [];
    try {
        if (mode === 0) {
            for (var a in await Promise.reject("rhs")) { log.push("bad"); }
        }
        if (mode === 1) {
            for ((await Promise.reject("target")).key in { a: 1 }) { log.push("bad"); }
        }
        if (mode === 2) {
            for (var b in { a: 1 }) { await Promise.reject("body"); }
        }
        if (mode === 3) {
            for (var c in new Proxy({}, { ownKeys: function () { throw "enumeration"; } })) { await Promise.resolve(); }
        }
    } catch (e) { log.push(e); }
    finally { log.push(await Promise.resolve("finally")); }
    return log.join(":");
}
Promise.all([run(0), run(1), run(2), run(3)]).then(function (values) { console.log(values.join(",")); });
"#,
        "rhs:finally,target:finally,body:finally,enumeration:finally",
    );
}

#[test]
fn test_es5_async_for_in_handles_null_empty_objects_and_boxed_strings() {
    assert_es5_async_runtime(
        r#"
async function run(value) {
    var log = [];
    for (var key in await Promise.resolve(value)) { log.push(key); }
    return log.join(":");
}
Promise.all([run(null), run(undefined), run({}), run(Object.create(null)), run(Object("abc"))]).then(function (values) { console.log(JSON.stringify(values)); });
"#,
        "[\"\",\"\",\"\",\"\",\"0:1:2\"]",
    );
}

#[test]
fn test_es5_async_for_in_native_control_and_receiver_side_effects() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var log = [];
    var target = {};
    for (target.key in { a: 1, b: 2 }) {
        if (target.key === "a") continue;
        log.push(target.key);
        break;
    }
    outer: for (var key in { x: 1, y: 2 }) {
        while (true) {
            switch (key) {
                case "x": log.push(key); continue outer;
                default: return log.join(",");
            }
        }
    }
}
run().then(console.log);
"#,
        "b,x",
    );
}

#[test]
fn test_es5_async_for_in_keeps_unsupported_binding_forms_atomic() {
    for source in [
        "async function run(object) { for (let key in object) { await key; } }",
        "async function run(object) { for (var [key] in object) { await key; } }",
        "async function run(object) { for (var key = 1 in object) { await key; } }",
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                always_strict: Some(false),
                ..Default::default()
            },
        );
        assert!(js.contains("function*"), "{js}");
        assert!(!js.contains(".label)"), "{js}");
    }
}

#[test]
fn test_es5_array_spread_order_sparse_values_and_snapshot() {
    let source = r#"
var log = [], source = [1, , 3];
Object.defineProperty(source, '2', { get: function () { log.push('get'); return 3; } });
function before() { log.push('before'); return 0; }
function after() { log.push('after'); source[0] = 9; return 4; }
var result = [before(), ...source, after(), ...[5, 6], , 7];
console.log(JSON.stringify([result, Object.keys(result), source[0], log]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(execute_with_node(&js), "[[0,1,null,3,4,5,6,null,7],[\"0\",\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"8\"],9,[\"before\",\"get\",\"after\"]]");
}

#[test]
fn test_es5_array_spread_dense_literals_need_no_helpers() {
    let source = "var a = [...[1, 2], ...[], 3]; console.log(JSON.stringify(a));";
    for iteration in [false, true] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                down_level_iteration: Some(iteration),
                ..Default::default()
            },
        );
        assert!(!js.contains("__spreadArray"), "{js}");
        assert!(!js.contains("__read"), "{js}");
        assert!(!js.contains("..."), "{js}");
        assert_eq!(execute_with_node(&js), "[1,2,3]", "{js}");
    }
}

#[test]
fn test_es5_array_spread_iterators_unicode_and_cleanup() {
    let source = r#"
var log = [], iterable = {};
iterable[Symbol.iterator] = function () {
    var i = 0;
    return { next: function () { log.push('next' + i); return { value: i++, done: i > 3 }; },
        return: function () { log.push('return'); return {}; } };
};
var values = [...iterable, ...'A😀B'];
var bad = {};
bad[Symbol.iterator] = function () {
    return { next: function () { return { done: false, get value() { throw 'value'; } }; },
        return: function () { log.push('close'); return {}; } };
};
try { [...bad]; } catch (e) { log.push(e); }
console.log(JSON.stringify([values, log]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    // TypeScript's __read closes the iterator when value access throws.
    assert_eq!(execute_with_node(&js), "[[0,1,2,\"A\",\"😀\",\"B\"],[\"next0\",\"next1\",\"next2\",\"next3\",\"close\",\"value\"]]", "{js}");
}

#[test]
fn test_es5_array_spread_helper_modes() {
    let source = "export var result = [...input];";
    let cjs = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            import_helpers: Some(true),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert!(cjs.contains("require(\"tslib\")"), "{cjs}");
    assert!(
        cjs.contains("tslib_1.__spreadArray([], tslib_1.__read(input), false)"),
        "{cjs}"
    );
    assert!(!cjs.contains("var __spreadArray"), "{cjs}");
    let esm = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::ESNext),
            import_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        esm.contains("import { __spreadArray } from \"tslib\""),
        "{esm}"
    );
    assert!(esm.contains("__spreadArray([], input, true)"), "{esm}");
    let suppressed = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        suppressed.contains("__spreadArray([], input, true)"),
        "{suppressed}"
    );
    assert!(!suppressed.contains("var __spreadArray"), "{suppressed}");
    let modern = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(modern.contains("...input"), "{modern}");
    assert!(!modern.contains("__spreadArray"), "{modern}");
}

#[test]
fn test_es5_async_array_spread_stages_values_across_awaits() {
    assert_es5_async_runtime(r#"
var log = [];
async function run() {
    var source = [1, , 3];
    var result = [0, ...source, await Promise.resolve().then(function () {
        source[0] = 9; source.push(4); log.push('await'); return 5;
    }), ...source, await 6];
    console.log(JSON.stringify([result, Object.keys(result), log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#, "[[0,1,null,3,5,9,null,3,4,6],[\"0\",\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\"],[\"await\"]]");
}

#[test]
fn test_es5_async_array_spread_awaited_operands_and_nested_arrays() {
    assert_es5_async_runtime(
        r#"
var log = [];
function value(n) { log.push(n); return n; }
async function run() {
    var a = [...(await [value(1), value(2)]), value(3)];
    var b = [value(4), ...(await [value(5)]), await value(6)];
    var c = [await value(7), ...[value(8), await value(9)]];
    var d = [...[...[value(10)]], ...[await value(11)]];
    console.log(JSON.stringify([a, b, c, d, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[1,2,3],[4,5,6],[7,8,9],[10,11],[1,2,3,4,5,6,7,8,9,10,11]]",
    );
}

#[test]
fn test_es5_async_array_spread_rejections_and_catch_bindings() {
    assert_es5_async_runtime(
        r#"
var log = [];
async function run() {
    try {
        try { throw [1, 2]; } catch (e) {
            var first = [...e, ...(await e)];
            log.push(JSON.stringify(first));
            [...e, ...(await Promise.reject('rejected')), log.push('late')];
        }
    } catch (e) { log.push(e); }
    finally { log.push(await 'finally'); }
    console.log(JSON.stringify(log));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[\"[1,2,1,2]\",\"rejected\",\"finally\"]",
    );
}

#[test]
fn test_es5_array_spread_distinguishes_rest_targets_and_default_values() {
    let source = r#"
var input = [1, 2], result, rest;
({ result = [...input] } = {});
([...rest] = result);
console.log(JSON.stringify([result, rest]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert!(js.contains("__spreadArray([], input, true)"), "{js}");
    for source in [
        "var rest; ([...rest] = input);",
        "var rest; ([...[...rest]] = input);",
        "var rest; for ([...rest] of input) {}",
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                ..Default::default()
            },
        );
        assert_node_syntax(&js);
        assert!(!js.contains("__spreadArray"), "{js}");
    }
}

#[test]
fn test_es5_array_spread_does_not_request_helpers_for_erased_code() {
    for source in [
        "declare var value = [...input];",
        "declare namespace N { var value = [...input]; }",
        "declare class C { value = [...input]; }",
        "class C { declare value = [...input]; }",
        "const enum E { A = [...input].length }",
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                down_level_iteration: Some(true),
                ..Default::default()
            },
        );
        assert!(!js.contains("__spreadArray"), "{source}\n{js}");
        assert!(!js.contains("__read"), "{source}\n{js}");
    }
}

#[test]
fn test_es5_async_array_spread_downlevel_iteration() {
    let source = r#"
var log = [], values = {};
values[Symbol.iterator] = function () {
    var n = 0;
    return { next: function () { log.push(n); return { value: n++, done: n > 2 }; } };
};
async function run() {
    var result = [...values, await 2, ...(await '😀x')];
    console.log(JSON.stringify([result, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(
        execute_with_node(&js),
        "[[0,1,2,\"😀\",\"x\"],[0,1,2]]",
        "{js}"
    );
}

#[test]
fn test_es5_array_spread_iteration_option_controls_protocol_use() {
    let source = r#"
var log = [], value = { 0: 'indexed', length: 1 };
value[Symbol.iterator] = function () {
    log.push('iterator');
    var done = false;
    return { next: function () { var old = done; done = true; return { done: old, value: 'iterated' }; } };
};
console.log(JSON.stringify([[...value], log]));
"#;
    for (iteration, expected) in [
        (false, "[[\"indexed\"],[]]"),
        (true, "[[\"iterated\"],[\"iterator\"]]"),
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                down_level_iteration: Some(iteration),
                ..Default::default()
            },
        );
        assert_node_syntax(&js);
        assert_eq!(execute_with_node(&js), expected, "{js}");
    }
}

#[test]
fn test_es5_call_spread_preserves_receivers_and_evaluation_order() {
    let source = r#"
var log = [], object = { name: 'receiver', get method() {
    log.push('method'); return function () { log.push(this.name); return Array.prototype.slice.call(arguments); };
}};
function receiver() { log.push('receiver'); return object; }
function key() { log.push('key'); return 'method'; }
function args() { log.push('args'); return [1, , 3]; }
var result = receiver()[key()](0, ...args(), 4);
console.log(JSON.stringify([result, log]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(
        execute_with_node(&js),
        "[[0,1,null,3,4],[\"receiver\",\"key\",\"method\",\"args\",\"receiver\"]]",
        "{js}"
    );
}

#[test]
fn test_es5_call_spread_bare_functions_parentheses_and_detached_methods() {
    let source = r#"
function collect() { 'use strict'; return [this === undefined, Array.prototype.slice.call(arguments)]; }
var object = { collect: collect }, values = [1, 2];
console.log(JSON.stringify([collect(...values), (collect)(...values),
    (0, object.collect)(...values), (object.collect)(...values),
    (function () { return arguments.length; })(...values)]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
}

#[test]
fn test_es5_call_spread_iterator_protocol_and_helper_modes() {
    let source = "function collect() { return Array.prototype.slice.call(arguments); } console.log(JSON.stringify(collect(...new Set([1, 2]), ...'😀x')));";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    let cjs = emit_ts_with(
        "export {}; f(...values);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            import_helpers: Some(true),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert!(
        cjs.contains("tslib_1.__spreadArray([], tslib_1.__read(values), false)"),
        "{cjs}"
    );
    assert!(!cjs.contains("var __spreadArray"), "{cjs}");
    let simple = emit_ts_with(
        "f(...values);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(simple.contains("f.apply(void 0, values)"), "{simple}");
    assert!(!simple.contains("__spreadArray"), "{simple}");
    let suppressed = emit_ts_with(
        "f(0, ...values);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            no_emit_helpers: Some(true),
            ..Default::default()
        },
    );
    assert!(
        suppressed.contains("__spreadArray([0], values, false)"),
        "{suppressed}"
    );
    assert!(!suppressed.contains("var __spreadArray"), "{suppressed}");
}

#[test]
fn test_es5_call_spread_retains_indirect_eval_semantics() {
    let source =
        "globalThis.value = 7; function run() { var value = 42; return [eval('value'), eval(...['value'])]; } console.log(JSON.stringify(run()));";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(execute_with_node(&js), "[42,7]", "{js}");
    assert!(js.contains("eval.apply"), "{js}");
}

#[test]
fn test_es5_async_call_spread_stages_callees_and_arguments() {
    assert_es5_async_runtime(
        r#"
var log = [], values = [1, , 3];
function collect() { log.push('old'); return Array.prototype.slice.call(arguments); }
async function run() {
    var result = collect(...values, await Promise.resolve().then(function () {
        values[0] = 9; values.push(4); collect = function () { throw 'new callee'; }; return 5;
    }), ...values);
    console.log(JSON.stringify([result, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[1,null,3,5,9,null,3,4],[\"old\"]]",
    );
}

#[test]
fn test_es5_async_call_spread_awaited_callees_receivers_and_keys() {
    assert_es5_async_runtime(
        r#"
var log = [], object = { name: 'old', method: function () { return [this.name, Array.prototype.slice.call(arguments)]; } };
function receiver() { log.push('receiver'); return object; }
async function run() {
    var a = receiver().method(...(await [1, 2]));
    var b = (await object).method(0, ...(await [3]));
    var c = object[await Promise.resolve().then(function () {
        object = { name: 'new', method: object.method }; return 'method';
    })](...[4]);
    var d = (await function () { return arguments[0] + arguments[1]; })(...(await [5, 6]));
    console.log(JSON.stringify([a, b, c, d, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[\"old\",[1,2]],[\"old\",[0,3]],[\"old\",[4]],11,[\"receiver\"]]",
    );
}

#[test]
fn test_es5_async_call_spread_nested_synchronous_calls_keep_local_temps() {
    assert_es5_async_runtime(
        r#"
function receiver(n) { return { value: n, call: function (x) { return this.value + x; } }; }
function collect() { return Array.prototype.slice.call(arguments); }
async function run(n) {
    var a = receiver(n).call(...[1]);
    var b = [receiver(n).call(...[2]), { value: receiver(n).call(...[3]) }];
    var c = true ? receiver(n).call(...[4]) : 0;
    var d = 1 + receiver(n).call(...[5]);
    var e = collect(...collect(receiver(n).call(...[6])));
    await 0;
    return [a, b, c, d, e];
}
Promise.all([run(10), run(20)]).then(function (result) { console.log(JSON.stringify(result)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[11,[12,{\"value\":13}],14,16,[16]],[21,[22,{\"value\":23}],24,26,[26]]]",
    );
}

#[test]
fn test_es5_async_call_spread_rejections_getters_and_finalizers() {
    assert_es5_async_runtime(
        r#"
var log = [], object = { get method() { log.push('get'); return function () { log.push('call'); }; } };
async function run() {
    try { object.method(...(await Promise.reject('reject'))); }
    catch (e) { log.push(e); }
    finally { log.push(await 'finally'); }
    console.log(JSON.stringify(log));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[\"get\",\"reject\",\"finally\"]",
    );
}

#[test]
fn test_es5_call_spread_scopes_temps_in_nested_functions() {
    let source = r#"
var _a = 100;
function receiver(n) { return { call: function (x) { return n + x; } }; }
function run(n) {
    var _b = 200;
    function nested(m) { return receiver(m).call(...[2]); }
    return [receiver(n).call(...[1]), nested(n), _a, _b];
}
console.log(JSON.stringify([run(10), run(20)]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(
        execute_with_node(&js),
        "[[11,12,100,200],[21,22,100,200]]",
        "{js}"
    );
}

#[test]
fn test_es5_async_call_spread_reads_iterators_around_awaits() {
    let source = r#"
function collect() { return Array.prototype.slice.call(arguments); }
async function run() {
    var value = new Set([1, 2]);
    var result = collect(...value, await Promise.resolve().then(function () { value.add(3); return 0; }), ...(await '😀x'));
    console.log(JSON.stringify(result));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(execute_with_node(&js), "[1,2,0,\"😀\",\"x\"]", "{js}");
}

#[test]
fn test_es5_call_spread_erased_code_and_modern_targets() {
    for source in [
        "declare var result = f(1, ...values);",
        "declare namespace N { var result = f(...values); }",
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                down_level_iteration: Some(true),
                ..Default::default()
            },
        );
        assert!(!js.contains("__spreadArray"), "{js}");
        assert!(!js.contains("__read"), "{js}");
    }
    let js = emit_ts_with(
        "object.method(0, ...values);",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    assert!(js.contains("...values"), "{js}");
    assert!(!js.contains(".apply"), "{js}");
}

#[test]
fn test_es5_async_call_spread_computed_method_names_and_nested_scopes() {
    assert_es5_async_runtime(
        r#"
function receiver(n) { return { call: function (x) { return n + x; } }; }
async function run(n) {
    var object = { [receiver(n).call(...[1])]() { return receiver(n).call(...[2]); } };
    await 0;
    return object[n + 1]();
}
Promise.all([run(10), run(20)]).then(function (values) { console.log(JSON.stringify(values)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[12,22]",
    );
}

#[test]
fn test_es5_new_spread_preserves_constructor_getters_and_argument_order() {
    let source = r#"
var log = [];
function C() { this.values = Array.prototype.slice.call(arguments); log.push('construct'); }
var object = { get C() { log.push('get'); return C; } };
function receiver() { log.push('receiver'); return object; }
function key() { log.push('key'); return 'C'; }
function values() { log.push('values'); return [1, , 3]; }
var result = new (receiver()[key()])(0, ...values(), 4);
console.log(JSON.stringify([result instanceof C, result.values, log]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    assert_eq!(
        execute_with_node(&js),
        "[true,[0,1,null,3,4],[\"receiver\",\"key\",\"get\",\"values\",\"construct\"]]",
        "{js}"
    );
}

#[test]
fn test_es5_new_spread_bound_native_and_returning_constructors() {
    let source = r#"
function C(a, b) { this.sum = a + b; }
var Bound = C.bind(null, 10);
function Returns() { return { values: Array.prototype.slice.call(arguments) }; }
function Callable(a, b) { return function () { return a + b; }; }
var values = [2, 3];
console.log(JSON.stringify([new Bound(...[5]).sum, new Returns(...values), new Callable(...values)(), new Date(...[0]).getTime(), new C(...values) instanceof C]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
}

#[test]
fn test_es5_new_spread_iterator_protocol_and_helpers() {
    let source = "function C() { this.values = Array.prototype.slice.call(arguments); } console.log(JSON.stringify(new C(...new Set([1, 2]), ...'😀x').values));";
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
    for (suppressed, imported) in [(false, false), (true, false), (false, true)] {
        let js = emit_ts_with(
            "export {}; new C(...values);",
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                module: Some(ModuleKind::CommonJS),
                down_level_iteration: Some(true),
                no_emit_helpers: Some(suppressed),
                import_helpers: Some(imported),
                ..Default::default()
            },
        );
        assert_node_syntax(&js);
        assert_eq!(
            js.contains("var __spreadArray"),
            !suppressed && !imported,
            "{js}"
        );
        assert_eq!(js.contains("var __read"), !suppressed && !imported, "{js}");
        assert!(
            js.contains(if imported {
                "tslib_1.__read(values)"
            } else {
                "__read(values)"
            }),
            "{js}"
        );
        assert!(!js.contains("..."), "{js}");
    }
    let dense = emit_ts_with(
        "new C(...[1, 2]);",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(!dense.contains("__spreadArray"), "{dense}");
    assert!(dense.contains("C.bind.apply(C, [void 0, 1, 2])"), "{dense}");
}

#[test]
fn test_es5_new_spread_array_like_and_iterable_options() {
    let source = r#"
var log = [], values = { 0: 'indexed', length: 1 };
values[Symbol.iterator] = function () { log.push('iterator'); var done = false; return { next: function () { if (done) return { done: true }; done = true; return { value: 'iterated', done: false }; } }; };
function C() { this.values = Array.prototype.slice.call(arguments); }
console.log(JSON.stringify([new C(...values).values, log]));
"#;
    for (iteration, expected) in [
        (false, "[[\"indexed\"],[]]"),
        (true, "[[\"iterated\"],[\"iterator\"]]"),
    ] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES5),
                down_level_iteration: Some(iteration),
                ..Default::default()
            },
        );
        assert_node_syntax(&js);
        assert_eq!(execute_with_node(&js), expected, "{js}");
    }
}

#[test]
fn test_es5_async_new_captures_mutable_constructor_before_await() {
    assert_es5_async_runtime(
        r#"
var log = [], value = 1;
function Original() { this.values = Array.prototype.slice.call(arguments); log.push('old'); }
var C = Original;
async function run() {
    var result = new C(value, await Promise.resolve().then(function () {
        C = function () { throw 'wrong constructor'; }; value = 9; return 2;
    }), value);
    console.log(JSON.stringify([result instanceof Original, result.values, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[true,[1,2,9],[\"old\"]]",
    );
}

#[test]
fn test_es5_async_new_awaited_constructors_receivers_and_keys() {
    assert_es5_async_runtime(
        r#"
var log = [];
function C(value) { this.value = value; }
var object = { get C() { log.push('get'); return C; } };
async function run() {
    var a = new (await Promise.resolve(C))(1);
    var b = new (await Promise.resolve(object)).C(2);
    var c = new object[await Promise.resolve('C')](3);
    var d = new (await Promise.resolve(object.C))(...[4]);
    var e = await new C(5);
    console.log(JSON.stringify([[a.value, b.value, c.value, d.value, e.value], log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[1,2,3,4,5],[\"get\",\"get\",\"get\"]]",
    );
}

#[test]
fn test_es5_async_new_spread_stages_arguments_and_constructor() {
    assert_es5_async_runtime(
        r#"
var values = [1, , 3];
function Original() { this.values = Array.prototype.slice.call(arguments); }
var C = Original;
async function run() {
    var result = new C(...values, await Promise.resolve().then(function () {
        values[0] = 9; values.push(4); C = function () { throw 'wrong constructor'; }; return 5;
    }), ...values);
    console.log(JSON.stringify([result instanceof Original, result.values]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[true,[1,null,3,5,9,null,3,4]]",
    );
}

#[test]
fn test_es5_async_new_spread_awaited_operands_and_nested_construction() {
    assert_es5_async_runtime(
        r#"
function C() { this.values = Array.prototype.slice.call(arguments); }
function factory() { return C; }
async function run(n) {
    return new (factory())(...(await Promise.resolve([n])), new (factory())(...[n + 1]), await Promise.resolve(n + 2));
}
Promise.all([run(10), run(20)]).then(function (values) { console.log(JSON.stringify(values)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[{\"values\":[10,{\"values\":[11]},12]},{\"values\":[20,{\"values\":[21]},22]}]",
    );
}

#[test]
fn test_es5_async_new_rejections_and_finalizers() {
    assert_es5_async_runtime(
        r#"
var log = [];
function C(x) { log.push('construct'); if (x === 2) throw 'constructor'; }
var object = { get C() { log.push('get'); return C; } };
async function run(mode) {
    try { return new object.C(...[await (mode === 1 ? Promise.reject('argument') : Promise.resolve(mode))]); }
    catch (error) { log.push(error); }
    finally { log.push(await Promise.resolve('finally')); }
}
run(1).then(function () { return run(2); }).then(function () { console.log(JSON.stringify(log)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[\"get\",\"argument\",\"finally\",\"get\",\"construct\",\"constructor\",\"finally\"]",
    );
}

#[test]
fn test_es5_new_spread_temporary_scopes_and_modern_targets() {
    let source = r#"
function C(x) { this.value = x; }
function factory() { return C; }
function run(_a) { return function (_b) { return new (factory())(...[_a + _b]); }; }
console.log(JSON.stringify([run(10)(1), run(20)(2)]));
"#;
    for target in [ScriptTarget::ES5, ScriptTarget::ES2015] {
        let js = emit_ts_with(
            source,
            CompilerOptions {
                target: Some(target),
                ..Default::default()
            },
        );
        assert_node_syntax(&js);
        assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
        if target == ScriptTarget::ES2015 {
            assert!(!js.contains(".bind.apply"), "{js}");
        }
    }
    let erased = emit_ts_with(
        "declare function f(x = new C(...values)): void; const enum E { A = new C(...values) }",
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert!(!erased.contains("__spreadArray"), "{erased}");
}

#[test]
fn test_es5_new_spread_private_and_optional_constructor_references() {
    let source = r#"
function C(x) { this.value = x; }
class Holder {
    static #C = C;
    static create() { return new this.#C(...[3]); }
}
var object = { C: C };
console.log(JSON.stringify([Holder.create().value, new (object?.C)(...[4]).value]));
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("..."), "{js}");
    assert_eq!(execute_with_node(&js), execute_with_node(source), "{js}");
}

#[test]
fn test_es5_async_new_preserves_catch_constructor_bindings() {
    assert_es5_async_runtime(
        r#"
function Original(x) { this.value = x; }
var C = 'outer';
async function run() {
    try { throw Original; }
    catch (C) {
        await 0;
        var a = new C(await Promise.resolve(1));
        var b = new C(...(await Promise.resolve([2])));
        return [a.value, b.value];
    }
}
run().then(function (values) { console.log(JSON.stringify([values, C])); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[1,2],\"outer\"]",
    );
}

#[test]
fn test_es5_async_new_spread_iterator_cleanup_before_later_awaits() {
    let source = r#"
var log = [];
function C() { log.push('construct'); this.values = Array.prototype.slice.call(arguments); }
var iterable = {};
iterable[Symbol.iterator] = function () {
    var count = 0;
    return { next: function () { log.push('next'); if (count++) throw 'iterator'; return { value: 1, done: false }; }, return: function () { log.push('close'); return {}; } };
};
async function run() {
    try { new C(...iterable, await Promise.resolve().then(function () { log.push('later'); return 2; })); }
    catch (error) { log.push(error); }
    finally { log.push(await Promise.resolve('finally')); }
    console.log(JSON.stringify(log));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#;
    let js = emit_ts_with(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            down_level_iteration: Some(true),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("function*"), "{js}");
    // TypeScript's __read helper closes an iterator when its next method throws.
    assert_eq!(
        execute_with_node(&js),
        "[\"next\",\"next\",\"close\",\"iterator\",\"finally\"]",
        "{js}"
    );
}

#[test]
fn test_es5_async_new_first_argument_await_and_later_suspensions() {
    assert_es5_async_runtime(
        r#"
var log = [], value = 2;
function C() { log.push('construct'); this.values = Array.prototype.slice.call(arguments); }
var object = { get C() { log.push('get'); return C; } };
async function step(n) { log.push(n); await 0; if (n === 3) value = 9; return n; }
async function run() {
    var result = new object.C(await step(1), value, await step(3), value);
    console.log(JSON.stringify([result.values, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[1,2,3,9],[\"get\",1,3,\"construct\"]]",
    );
}

#[test]
fn test_es5_async_object_values_preserve_order_across_awaits() {
    assert_es5_async_runtime(
        r#"
var log = [], value = 1;
async function step(n) { log.push(n); await 0; value++; return n; }
async function run() {
    var object = { before: value, a: await step(10), between: value, b: await step(20), after: value };
    console.log(JSON.stringify([object, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[{\"before\":1,\"a\":10,\"between\":2,\"b\":20,\"after\":3},[10,20]]",
    );
}

#[test]
fn test_es5_async_object_computed_keys_are_evaluated_before_values() {
    assert_es5_async_runtime(
        r#"
var log = [], key = 'old';
function readKey() { log.push('key'); return key; }
async function value() { log.push('value'); await 0; key = 'new'; return 1; }
async function run() {
    var object = { [readKey()]: await value(), [await Promise.resolve('other')]: key, tail: 2 };
    console.log(JSON.stringify([object, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[{\"old\":1,\"other\":\"new\",\"tail\":2},[\"key\",\"value\"]]",
    );
}

#[test]
fn test_es5_async_object_nested_literals_and_concurrent_invocations() {
    assert_es5_async_runtime(r#"
async function run(n) {
    return { a: await Promise.resolve(n), nested: { before: n + 1, [await Promise.resolve('key')]: await Promise.resolve(n + 2) }, end: await Promise.resolve(n + 3) };
}
Promise.all([run(10), run(20)]).then(function (values) { console.log(JSON.stringify(values)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#, "[{\"a\":10,\"nested\":{\"before\":11,\"key\":12},\"end\":13},{\"a\":20,\"nested\":{\"before\":21,\"key\":22},\"end\":23}]");
}

#[test]
fn test_es5_async_object_accessors_merge_and_data_properties_replace_them() {
    assert_es5_async_runtime(
        r#"
var log = [];
async function run() {
    var value = 1;
    var object = { a: await Promise.resolve(0), get x() { return value; }, set x(v) { log.push(v); value = v; }, tail: await Promise.resolve(2) };
    object.x = 3;
    var replaced = { get x() { throw 'getter'; }, a: await Promise.resolve(1), x: 9 };
    console.log(JSON.stringify([object.x, Object.keys(object), replaced, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[3,[\"a\",\"x\",\"tail\"],{\"x\":9,\"a\":1},[3]]",
    );
}

#[test]
fn test_es5_async_object_methods_keep_this_arguments_and_nested_async_scopes() {
    assert_es5_async_runtime(
        r#"
async function run() {
    var object = { value: await Promise.resolve(10), [await Promise.resolve('method')](x = 2) { return this.value + x + arguments.length; }, async next(x) { return this.value + await Promise.resolve(x); } };
    console.log(JSON.stringify([object.method(3), await object.next(4)]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[14,14]",
    );
}

#[test]
fn test_es5_async_object_spread_reads_getters_before_later_awaits() {
    assert_es5_async_runtime(
        r#"
var log = [], value = 1;
var source = { get x() { log.push(value); return value; } };
async function run() {
    var object = { ...source, a: await Promise.resolve().then(function () { value = 2; return 3; }), ...source, b: await Promise.resolve(4) };
    console.log(JSON.stringify([object, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[{\"x\":2,\"a\":3,\"b\":4},[1,2]]",
    );
}

#[test]
fn test_es5_async_object_rejections_leave_assignment_target_unchanged() {
    assert_es5_async_runtime(
        r#"
var log = [], result = 'unchanged';
function later() { log.push('later'); return 2; }
async function run() {
    try { result = { a: await Promise.resolve(1), b: await Promise.reject('error'), c: later() }; }
    catch (error) { log.push(error); }
    finally { log.push(await Promise.resolve('finally')); }
    console.log(JSON.stringify([result, log]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[\"unchanged\",[\"error\",\"finally\"]]",
    );
}

#[test]
fn test_es5_async_object_catch_bindings_and_quoted_numeric_keys() {
    assert_es5_async_runtime(
        r#"
var key = 'outer';
async function run() {
    try { throw 'inner'; }
    catch (key) { return { key, 'a-b': await Promise.resolve(1), [key]: 2, 3: await Promise.resolve(4) }; }
}
run().then(function (value) { console.log(JSON.stringify([value, key])); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[{\"3\":4,\"key\":\"inner\",\"a-b\":1,\"inner\":2},\"outer\"]",
    );
}

#[test]
fn test_es5_async_object_computed_accessors_symbols_and_escaped_keys() {
    assert_es5_async_runtime(
        r#"
var symbol = Symbol('key');
async function run() {
    var value = 1;
    var object = { [await Promise.resolve(symbol)]: await Promise.resolve(7), get [await Promise.resolve('x')]() { return value; }, set [await Promise.resolve('x')](next) { value = next; }, 'a"b\\c': await Promise.resolve(2) };
    object.x = 3;
    console.log(JSON.stringify([object[symbol], object.x, Object.keys(object), Object.getOwnPropertyDescriptor(object, 'x').enumerable]));
}
run().catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[7,3,[\"x\",\"a\\\"b\\\\c\"],true]",
    );
}

#[test]
fn test_es5_async_object_shorthand_after_await_and_lexical_function_values() {
    assert_es5_async_runtime(
        r#"
function outer() {
    var _a = 10;
    return async function (n) { return { first: await Promise.resolve(n), _a, method: () => n + _a, nested: async () => await Promise.resolve(n + 1) }; };
}
var run = outer();
Promise.all([run(1), run(2)]).then(async function (values) { console.log(JSON.stringify([values[0]._a, values[0].method(), await values[1].nested()])); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[10,11,3]",
    );
}

#[test]
fn test_es5_async_object_branches_and_nested_constructor_arguments() {
    assert_es5_async_runtime(
        r#"
function C(value) { this.value = value; }
async function run(flag) {
    return new C(flag ? { a: await Promise.resolve(1), b: 2 } : { a: 3, b: await Promise.resolve(4) });
}
Promise.all([run(true), run(false)]).then(function (values) { console.log(JSON.stringify(values)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[{\"value\":{\"a\":1,\"b\":2}},{\"value\":{\"a\":3,\"b\":4}}]",
    );
}

#[test]
fn test_es5_async_blocks_preserve_nested_returns_before_and_after_await() {
    assert_es5_async_runtime(
        r#"
async function before() { { { return 1; } } }
async function after() { await 0; { { return 2; } } }
async function suspended() { { return await Promise.resolve(3); } }
Promise.all([before(), after(), suspended()]).then(function (values) { console.log(JSON.stringify(values)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[1,2,3]",
    );
}

#[test]
fn test_es5_async_blocks_native_labels_and_multiple_label_names() {
    assert_es5_async_runtime(
        r#"
var log = [];
async function run() {
    await 0;
    outer: inner: { { log.push(1); break outer; } log.push('bad'); }
    later: { log.push(2); if (true) { break later; } log.push('bad'); }
    log.push(3);
}
run().then(function () { console.log(JSON.stringify(log)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[1,2,3]",
    );
}

#[test]
fn test_es5_async_blocks_labeled_breaks_cross_suspension() {
    assert_es5_async_runtime(
        r#"
async function run(flag) {
    var log = [];
    outer: inner: {
        log.push(await Promise.resolve(1));
        if (flag) { { break inner; } }
        log.push(await Promise.resolve(2));
        { break outer; }
        log.push('bad');
    }
    log.push(3);
    return log;
}
Promise.all([run(true), run(false)]).then(function (values) { console.log(JSON.stringify(values)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[[1,3],[1,2,3]]",
    );
}

#[test]
fn test_es5_async_blocks_unlabeled_control_skips_labeled_block_scopes() {
    assert_es5_async_runtime(
        r#"
var log = [];
async function run() {
    var i = 0;
    while (i < 4) {
        await 0;
        i++;
        marker: {
            if (i === 1) { continue; }
            if (i === 3) { break; }
            log.push(i);
        }
        log.push('tail');
    }
    log.push(i);
}
run().then(function () { console.log(JSON.stringify(log)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[2,\"tail\",3]",
    );
}

#[test]
fn test_es5_async_blocks_switch_breaks_and_outer_loop_continues() {
    assert_es5_async_runtime(
        r#"
var log = [];
async function run() {
    outer: for (var i = 0; i < 3; i++) {
        await 0;
        block: {
            switch (i) {
                case 0: { log.push('switch'); break; }
                case 1: { continue outer; }
                default: { break block; }
            }
            log.push(i);
        }
        log.push('tail');
    }
}
run().then(function () { console.log(JSON.stringify(log)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[\"switch\",0,\"tail\",\"tail\"]",
    );
}

#[test]
fn test_es5_async_blocks_labeled_jumps_run_suspending_finalizers() {
    assert_es5_async_runtime(
        r#"
var log = [];
async function run() {
    exit: {
        try { log.push(await Promise.resolve('body')); { break exit; } }
        finally { log.push(await Promise.resolve('finally')); }
        log.push('bad');
    }
    log.push('after');
}
run().then(function () { console.log(JSON.stringify(log)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[\"body\",\"finally\",\"after\"]",
    );
}

#[test]
fn test_es5_async_blocks_resolve_catch_redeclarations_and_native_finalizers() {
    assert_es5_async_runtime(
        r#"
var error = 'outer', log = [];
async function run() {
    try { throw 'inner'; }
    catch (error) {
        { var error = 'changed'; }
        await 0;
        leave: { try { { log.push(error); break leave; } } finally { { log.push('finally'); } } }
    }
    log.push(error);
}
run().then(function () { console.log(JSON.stringify(log)); }).catch(function (e) { console.error(e); process.exitCode = 1; });
"#,
        "[\"changed\",\"finally\",null]",
    );
}

#[test]
fn jsx_this_tags_preserve_instance_identity_in_nested_arrows() {
    let source = r#"
class View {
    Component = function () {};
    render() {
        const nested = () => [<this />, <this.Component />, <this.Component></this.Component>];
        return nested();
    }
}
const view = new View();
const tags = view.render();
console.log(tags[0] === view && tags[1] === view.Component && tags[2] === view.Component);
"#;
    for target in [ScriptTarget::ES5, ScriptTarget::ES2015] {
        let javascript = emit_ts_file_with(
            "this-tags.tsx",
            source,
            CompilerOptions {
                target: Some(target),
                jsx: Some(JsxEmit::React),
                ..Default::default()
            },
        );
        let runtime = format!(
            "var React = {{ createElement: function (tag) {{ return tag; }} }};\n{javascript}"
        );
        assert_eq!(
            execute_with_node(&runtime),
            "true",
            "{target:?}: {javascript}"
        );
    }
}

#[test]
fn jsx_comma_containers_keep_evaluation_order_and_single_values() {
    let source = r#"
const log = [];
function step(n) { log.push(n); return n; }
const node = <div value={step(1), step(2)}>{step(3), step(4)}{step(5)}</div>;
console.log(JSON.stringify([log, node.props.value, node.children]));
"#;
    let runtime = r#"
function automatic(tag, props) { return { props: props, children: props.children }; }
var React = { createElement: function(tag, props) { return { props: props, children: Array.prototype.slice.call(arguments, 2) }; } };
require = function () { return { jsx: automatic, jsxs: automatic, jsxDEV: automatic }; };
"#;
    for jsx in [JsxEmit::React, JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        for target in [ScriptTarget::ES5, ScriptTarget::ES2018] {
            let javascript = emit_ts_file_with(
                "comma.tsx",
                source,
                CompilerOptions {
                    jsx: Some(jsx),
                    target: Some(target),
                    module: Some(ModuleKind::CommonJS),
                    ..Default::default()
                },
            );
            assert_eq!(
                execute_with_node(&format!("{runtime}\n{javascript}")),
                "[[1,2,3,4,5],2,[4,5]]",
                "{jsx:?} {target:?}: {javascript}"
            );
        }
    }
}

#[test]
fn jsx_spread_comma_operands_copy_only_the_final_object() {
    let runtime = r#"
function automatic(tag, props) { return { props: props, children: [props.children] }; }
function classic(tag, props) { return { props: props, children: Array.prototype.slice.call(arguments, 2) }; }
var React = { createElement: classic };
require = function () { return { jsx: automatic, jsxs: automatic, jsxDEV: automatic, createElement: classic }; };
"#;
    for attributes in [
        "key={step(1), step(2)} {...obj(3), obj(4)}",
        "{...obj(3), obj(4)} key={step(1), step(2)}",
    ] {
        let source = format!(
            r#"
const log = [];
function step(n) {{ log.push(n); return n; }}
function obj(n) {{ step(n); return n === 3 ? {{ left: true }} : {{ right: true }}; }}
const node = <div {attributes}>{{step(5), step(6)}}</div>;
console.log(JSON.stringify([log, !!node.props.left, node.props.right, node.children]));
"#
        );
        for jsx in [JsxEmit::React, JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
            for target in [ScriptTarget::ES5, ScriptTarget::ES2018] {
                let javascript = emit_ts_file_with(
                    "spread-comma.tsx",
                    &source,
                    CompilerOptions {
                        jsx: Some(jsx),
                        target: Some(target),
                        module: Some(ModuleKind::CommonJS),
                        ..Default::default()
                    },
                );
                // Automatic runtimes extract `key` to a later argument; their
                // reference emit evaluates the spread before the extracted key.
                let order = if jsx == JsxEmit::React && attributes.starts_with("key") {
                    "1,2,3,4,5,6"
                } else if attributes.starts_with("key") {
                    "3,4,5,6,1,2"
                } else {
                    "3,4,1,2,5,6"
                };
                assert_eq!(
                    execute_with_node(&format!("{runtime}\n{javascript}")),
                    format!("[[{order}],false,true,[6]]"),
                    "{jsx:?} {target:?}: {javascript}"
                );
            }
        }
    }
}

#[test]
fn empty_jsx_attribute_containers_emit_boolean_values_in_all_runtimes() {
    let runtime = r#"
function classic(tag, props) { return props; }
function automatic(tag, props, key) { props.key = key; return props; }
var React = { createElement: classic };
require = function () { return { jsx: automatic, jsxs: automatic, jsxDEV: automatic, createElement: classic }; };
"#;
    for attributes in [
        "key={} plain={} comment={/*empty*/} literal=\"\"",
        "key={} {...{}} plain={} comment={/*empty*/} literal=\"\"",
        "{...{}} key={} plain={} comment={/*empty*/} literal=\"\"",
    ] {
        let source = format!("const props = <div {attributes} />; console.log(JSON.stringify([props.key, props.plain, props.comment, props.literal]));");
        for jsx in [JsxEmit::React, JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
            for target in [ScriptTarget::ES5, ScriptTarget::ES2018] {
                let javascript = emit_ts_file_with(
                    "empty-attributes.tsx",
                    &source,
                    CompilerOptions {
                        jsx: Some(jsx),
                        target: Some(target),
                        module: Some(ModuleKind::CommonJS),
                        ..Default::default()
                    },
                );
                assert_eq!(
                    execute_with_node(&format!("{runtime}\n{javascript}")),
                    "[true,true,true,\"\"]",
                    "{jsx:?} {target:?}: {javascript}"
                );
            }
        }
    }
}

#[test]
fn nested_jsx_values_and_delimiter_strings_preserve_following_attributes() {
    let runtime = r#"
function element(tag, props) { return props; }
var React = { createElement: element };
require = function () { return { jsx: element, jsxs: element, jsxDEV: element }; };
"#;
    for recovery in ["", "<div bad= />;"] {
        let source = format!(
            r#"{recovery}
const props = <div child={{<span empty={{}} />}} text="/>" /* /> */ data={{42}} tail={{}} />;
console.log(JSON.stringify([props.child.empty, props.text, props.data, props.tail]));
"#
        );
        for jsx in [JsxEmit::React, JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
            let javascript = emit_ts_file_with(
                "nested-attributes.tsx",
                &source,
                CompilerOptions {
                    jsx: Some(jsx),
                    target: Some(ScriptTarget::ES2018),
                    module: Some(ModuleKind::CommonJS),
                    ..Default::default()
                },
            );
            assert_eq!(
                execute_with_node(&format!("{runtime}\n{javascript}")),
                "[true,\"/>\",42,true]",
                "{jsx:?}: {javascript}"
            );
        }
    }
}

#[test]
fn es5_accessors_lower_default_and_rest_parameters() {
    let js = emit_ts_with(
        r#"
let calls = 0;
function fallback() { calls++; return 7; }
class C {
    set value(v = fallback()) { this.seen = v; }
    get value() { return this.seen; }
    static set values(...items: number[]) { C.items = items; }
}
const c = new C();
c.value = undefined;
console.log(c.value, calls);
c.value = 3;
console.log(c.value, calls);
Object.getOwnPropertyDescriptor(C, "values").set.apply(C, [4, 5]);
console.log(C.items.join(","));
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("class C"), "{js}");
    assert!(js.contains("if (v === void 0) { v = fallback(); }"), "{js}");
    assert!(js.contains("items[_i] = arguments[_i]"), "{js}");
    assert_eq!(execute_with_node(&js), "7 1\n3 1\n4,5");
}

#[test]
fn es5_literal_class_members_preserve_keys_and_pair_accessors() {
    let js = emit_ts_with(
        r#"
class C {
    "quoted"() { return 2; }
    3() { return 3; }
    static "\""() { return 4; }
    get "\x78"() { return this.saved; }
    set x(value: number) { this.saved = value; }
    get 0x10() { return this.number; }
    set "16"(value: number) { this.number = value; }
    get 1_000() { return 10; }
    set "1000"(value: number) { this.thousand = value; }
    get 1e21() { return 21; }
    set "1e+21"(value: number) { this.large = value; }
}
const c = new C();
c.x = 5;
c[16] = 6;
console.log(c.quoted(), c[3](), C['"'](), c.x, c[16]);
"#,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..Default::default()
        },
    );
    assert_node_syntax(&js);
    assert!(!js.contains("class C"), "{js}");
    assert!(js.contains("C.prototype[\"quoted\"] = function"), "{js}");
    assert!(js.contains("C.prototype[3] = function"), "{js}");
    assert_eq!(
        js.matches("Object.defineProperty(C.prototype,").count(),
        4,
        "{js}"
    );
    assert_eq!(execute_with_node(&js), "2 3 4 5 6");
}

#[test]
fn es5_constructor_comments_stay_with_the_constructor() {
    let source = r#"
class C {
    // erased field
    static field: number; // erased tail
    /** constructor leading */
    constructor() {
        // constructor body
    } // constructor trailing
}
"#;
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        ..Default::default()
    };
    let js = emit_ts_with(source, options.clone());
    assert!(!js.contains("class C"), "{js}");
    assert!(!js.contains("erased"), "{js}");
    for comment in [
        "/** constructor leading */",
        "// constructor body",
        "// constructor trailing",
    ] {
        assert_eq!(js.matches(comment).count(), 1, "{js}");
    }
    assert!(
        js.contains("/** constructor leading */\n    function C()"),
        "{js}"
    );
    assert!(
        js.contains("} // constructor trailing\n    return C;"),
        "{js}"
    );
    let stripped = emit_ts_with(
        source,
        CompilerOptions {
            remove_comments: Some(true),
            ..options
        },
    );
    assert!(
        !stripped.contains("constructor leading")
            && !stripped.contains("constructor body")
            && !stripped.contains("constructor trailing"),
        "{stripped}"
    );
}
