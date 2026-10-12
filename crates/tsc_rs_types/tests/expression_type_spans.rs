use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::{TypeCheckOutput, TypeChecker};

fn check(source: &str, collect_spans: bool) -> TypeCheckOutput {
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    if collect_spans {
        checker.enable_expression_type_spans();
    }
    checker.check_with_options(&file, &symbols, &CompilerOptions::default())
}

fn entry<'a>(source: &str, result: &'a TypeCheckOutput, text: &str) -> &'a str {
    let start = source.find(text).unwrap() as u32;
    result
        .expression_type_spans
        .get(&(start, start + text.len() as u32))
        .map(|entry| entry.type_string.as_str())
        .unwrap_or_else(|| panic!("missing {text:?}: {:?}", result.expression_type_spans))
}

#[test]
fn member_and_call_types_retain_distinct_spans_at_one_position() {
    let source = "const obj = { f: () => 1 }; obj.f();";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "obj.f()"), "number");
    assert_eq!(entry(source, &result, "obj.f"), "() => number");
    let start = source.find("obj.f").unwrap() as u32;
    assert!(result
        .expression_type_spans
        .contains_key(&(start, start + 3)));
}

#[test]
fn contextual_arrow_and_its_parameter_keep_separate_types() {
    let source = "const f: (n: number) => number = value => value + 1;";
    let result = check(source, true);
    assert_eq!(
        entry(source, &result, "value => value + 1"),
        "(value: number) => number"
    );
    assert_eq!(entry(source, &result, "value"), "number");
    assert_eq!(entry(source, &result, "value + 1"), "number");
}

#[test]
fn source_spans_preserve_strings_comments_and_multiline_expressions() {
    let source = "const x = (\n  'é😀' /* comment */\n);";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "'é😀'"), "\"é😀\"");
    assert_eq!(
        entry(source, &result, "(\n  'é😀' /* comment */\n)"),
        "\"é😀\""
    );
}

#[test]
fn generic_class_names_report_the_instance_type() {
    let source = "class Box<T, U> {}";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "Box"), "Box<T, U>");
}

#[test]
fn collecting_exact_spans_preserves_diagnostics_and_hover_types() {
    let source = "const x: number = 'bad'; const f = (n: number) => n + 1; f(x);";
    let ordinary = check(source, false);
    let inspected = check(source, true);
    assert!(ordinary.expression_type_spans.is_empty());
    assert!(!inspected.expression_type_spans.is_empty());
    assert_eq!(ordinary.expression_types, inspected.expression_types);
    assert_eq!(
        format!("{:?}", ordinary.diagnostics),
        format!("{:?}", inspected.diagnostics)
    );
}

#[test]
fn assignment_targets_keep_the_declared_type_and_full_expression() {
    let source = "let value: number | string = 1; value = 'text';";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "value = 'text'"), "\"text\"");
    let start = source.find("value =").unwrap() as u32;
    assert_eq!(
        result.expression_type_spans[&(start, start + 5)].type_string,
        "string | number"
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn destructured_names_have_types_without_treating_containers_as_expressions() {
    let source =
        "declare const input: any; const { value, other = 1 } = input; const [first] = [2];";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "value"), "any");
    assert_eq!(entry(source, &result, "other"), "any");
    assert_eq!(entry(source, &result, "first"), "number");
    let start = source.find("{ value").unwrap() as u32;
    assert!(result
        .expression_type_spans
        .iter()
        .any(|(&(from, _), entry)| from == start
            && entry.kind == tsc_rs_types::TypeSpanKind::BindingPattern));
}

#[test]
fn class_literal_and_computed_names_keep_complete_source_spans() {
    let source = "class C { 0() {} 'name' = 1; [Symbol.iterator]() {} set value(v: number) {} }";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "0"), "() => void");
    assert_eq!(entry(source, &result, "'name'"), "number");
    assert_eq!(entry(source, &result, "[Symbol.iterator]"), "() => void");
    assert_eq!(entry(source, &result, "value"), "number");
}

#[test]
fn readonly_property_names_use_the_class_member_type() {
    let source = "const FOO = 'FOO'; class Bar { readonly tag = FOO; value = FOO; static readonly code = 1; }";
    let result = check(source, true);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(entry(source, &result, "tag"), "\"FOO\"");
    assert_eq!(entry(source, &result, "value"), "string");
    assert_eq!(entry(source, &result, "code"), "1");
}

#[test]
fn alias_name_types_are_separate_from_expanded_hover_bodies() {
    let source =
        "type Choice = 'a' | 'b'; type List<T> = T[]; type Scalar = string; type Identity<T> = T;";
    let result = check(source, true);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(entry(source, &result, "Choice"), "Choice");
    assert_eq!(entry(source, &result, "List"), "List<T>");
    assert_eq!(entry(source, &result, "Scalar"), "string");
    assert_eq!(entry(source, &result, "Identity"), "T");
    let ordinary = check(source, false);
    assert_eq!(ordinary.expression_types, result.expression_types);
    assert_eq!(
        result.expression_types[&(source.find("Choice").unwrap() as u32)],
        "type_alias_body=\"a\" | \"b\""
    );
}

#[test]
fn property_signatures_keep_identifier_string_number_and_computed_name_spans() {
    let source = "declare const key: unique symbol; interface I { count: number; 'caption': string; 42: boolean; [key]: number; }";
    let result = check(source, true);
    for (name, ty) in [
        ("count", "number"),
        ("'caption'", "string"),
        ("42", "boolean"),
        ("[key]", "number"),
    ] {
        assert_eq!(entry(source, &result, name), ty);
    }
    assert_eq!(
        result.expression_types,
        check(source, false).expression_types
    );
}

#[test]
fn nested_type_literal_property_names_have_semantic_types() {
    let source = "type Outer<T> = { child: { value: T }; flag: boolean }; declare const x: { score: number };";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "value"), "T");
    assert_eq!(entry(source, &result, "flag"), "boolean");
    assert_eq!(entry(source, &result, "score"), "number");
    assert_eq!(
        result.expression_types,
        check(source, false).expression_types
    );
}

#[test]
fn optional_property_signature_names_follow_strict_null_checks() {
    let source = "interface I { value?: number; optionalAny?: any; }";
    let file = tsc_rs_parser::parse("case.ts", source);
    for strict in [false, true] {
        let symbols = tsc_rs_symbols::bind(&file);
        let mut checker = TypeChecker::new();
        checker.enable_expression_type_spans();
        let result = checker.check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict_null_checks: Some(strict),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            entry(source, &result, "value"),
            if strict {
                "number | undefined"
            } else {
                "number"
            }
        );
        assert_eq!(entry(source, &result, "optionalAny"), "any");
    }
}

#[test]
fn property_signature_spans_preserve_scoped_types_and_diagnostics() {
    let source = "namespace N { type Value = string; export interface I<T> { text: Value; generic: T; } } interface Other { outer: number; }";
    let result = check(source, true);
    assert_eq!(entry(source, &result, "text"), "string");
    assert_eq!(entry(source, &result, "generic"), "T");
    assert_eq!(entry(source, &result, "outer"), "number");
    let ordinary = check(source, false);
    assert_eq!(result.expression_types, ordinary.expression_types);
    assert_eq!(
        format!("{:?}", result.diagnostics),
        format!("{:?}", ordinary.diagnostics)
    );
}

#[test]
fn synthesized_jsdoc_properties_are_not_source_declaration_names() {
    let source = "/** @typedef {Object} Options\n * @property {boolean} flag\n * @property {string} text\n */\n/** @typedef {{ value: number }} Box */\ninterface Actual { real: number; }";
    let file = tsc_rs_parser::parse("case.js", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let result = checker.check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            allow_js: Some(true),
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(entry(source, &result, "real"), "number");
    for &(start, _) in result.expression_type_spans.keys() {
        assert!(
            file.comments
                .iter()
                .all(|comment| start < comment.pos || start >= comment.end),
            "comment leaked into spans: {:?}",
            result.expression_type_spans
        );
    }
}
