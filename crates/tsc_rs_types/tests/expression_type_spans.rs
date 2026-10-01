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
