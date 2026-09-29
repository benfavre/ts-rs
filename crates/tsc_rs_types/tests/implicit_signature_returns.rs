use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str, implicit_any: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("signatures.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                no_implicit_any: Some(implicit_any),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

fn assert_signatures(source: &str, expected: &[(u32, &str)]) {
    let result = diagnostics(source, true);
    let actual: Vec<_> = result
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("source span");
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}\n{result:?}");
}

#[test]
fn signatures_report_missing_return_annotations_with_complete_spans() {
    assert_signatures(
        "interface I { (); new (); method(); }",
        &[(7020, "();"), (7013, "new ();"), (7010, "method();")],
    );
    assert_signatures(
        "type I = { <T>(value: T); new <T>(value: T); method<T>(value: T); };",
        &[
            (7020, "<T>(value: T);"),
            (7013, "new <T>(value: T);"),
            (7010, "method<T>(value: T);"),
        ],
    );
    assert_signatures(
        "interface I { new (\nvalue: number\n); }",
        &[(7013, "new (\nvalue: number\n);")],
    );
}

#[test]
fn nested_annotations_are_checked_once() {
    for source in [
        "type Outer = { inner: { new (); } };",
        "declare let value: { inner: { new (); } };",
        "function f(value: { inner: { new (); } }) {}",
        "function f<T extends { inner: { new (); } }>() {}",
        "function f<T = { inner: { new (); } }>() {}",
        "function f(): { inner: { new (); } } { throw 1; }",
        "class C { field: { inner: { new (); } } = null as any; }",
    ] {
        assert_signatures(source, &[(7013, "new ();")]);
    }
}

#[test]
fn member_names_keep_their_source_spelling() {
    let source = "interface I { 'quoted'(); 42(); ['computed'](); }";
    let result = diagnostics(source, true);
    assert_eq!(result.len(), 3, "{result:?}");
    for (diagnostic, name) in result.iter().zip(["'quoted'", "42", "['computed']"]) {
        assert_eq!(diagnostic.code, 7010);
        assert_eq!(diagnostic.message, format!("'{name}', which lacks return-type annotation, implicitly has an 'any' return type."));
    }
}

#[test]
fn explicit_returns_and_disabled_implicit_any_do_not_report() {
    assert!(diagnostics("interface I { (): any; new (): any; method(): any; }", true).is_empty());
    assert!(diagnostics("interface I { (); new (); method(); }", false).is_empty());
    assert!(diagnostics("const value = { method() {} }; function f() {}", true).is_empty());
}

#[test]
fn bodyless_functions_keep_name_spans() {
    assert_signatures("declare function f();", &[(7010, "f")]);
    assert_signatures(
        "namespace N { export function f(); export function f() {} }",
        &[(7010, "f")],
    );
}

#[test]
fn separators_and_trivia_match_signature_diagnostic_spans() {
    assert_signatures(
        "type I = { () /* comment */ ; new ()\n, method()\nother(): any }",
        &[
            (7020, "() /* comment */ ;"),
            (7013, "new ()\n,"),
            (7010, "method()"),
        ],
    );
}
