use tsc_rs_ast::Diagnostic;
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("missing_arguments.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new().check(&file, &symbols).diagnostics
}

fn assert_missing_argument(source: &str, code: u32, message: &str, parameter: &str) {
    let diagnostics = diagnostics(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2554, "{diagnostics:?}");
    let related = diagnostics[0].related.as_ref().expect("parameter note");
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].code, code);
    assert_eq!(related[0].message, message);
    assert_eq!(
        related[0].file_name.as_deref(),
        Some("missing_arguments.ts")
    );
    let span = related[0].span.expect("parameter span");
    assert_eq!(&source[span.start as usize..span.end as usize], parameter);
}

#[test]
fn omitted_binding_patterns_have_related_notes_for_calls_and_constructors() {
    for pattern in ["{value}", "[value]"] {
        for source in [
            format!("function f(first, {pattern}) {{}} f(0);"),
            format!("class C {{ method(first, {pattern}) {{}} }} new C().method(0);"),
            format!("class C {{ constructor(first, {pattern}) {{}} }} new C(0);"),
            format!(
                "class C {{ constructor(first, {pattern}) {{}} }} class D extends C {{}} new D(0);"
            ),
        ] {
            assert_missing_argument(
                &source,
                6211,
                "An argument matching this binding pattern was not provided.",
                pattern,
            );
        }
    }
}

#[test]
fn omitted_named_parameters_keep_their_named_related_notes() {
    for source in [
        "function f(first, value) {} f(0);",
        "class C { method(first, value) {} } new C().method(0);",
        "class C { constructor(first, value) {} } new C(0);",
    ] {
        assert_missing_argument(
            source,
            6210,
            "An argument for 'value' was not provided.",
            "value",
        );
    }
}

#[test]
fn supplied_or_defaulted_binding_patterns_do_not_report_missing_arguments() {
    let source = r#"
function object({value}) {}
function array([value]) {}
function defaulted({value} = {value: 1}) {}
class C { constructor({value}) {} }
object({value: 1});
array([1]);
defaulted();
new C({value: 1});
"#;
    let diagnostics = diagnostics(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
