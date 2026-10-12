use tsc_rs_ast::{Diagnostic, Span};

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    tsc_rs_parser::parse("lists.ts", source).diagnostics
}

#[test]
fn omitted_call_arguments_are_reported_at_the_comma() {
    for (source, position) in [
        ("f(,);", 2),
        ("f(a,,b);", 4),
        ("new F(,);", 6),
        ("f?.(,);", 4),
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1135);
        assert_eq!(errors[0].span, Some(Span::new(position, position + 1)));
    }
    let errors = diagnostics("f(,,);");
    assert_eq!(
        errors.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![1135, 1135]
    );
}

#[test]
fn a_statement_keyword_ends_a_broken_argument_list() {
    let source = "function f() { call(value, return 1; }";
    let errors = diagnostics(source);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 1135);
    let span = errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "return");
}

#[test]
fn trailing_argument_commas_and_array_holes_remain_valid() {
    for source in ["f();", "f(a,);", "new F(a,);", "f?.(a,);", "f([, ,]);"] {
        assert!(diagnostics(source).is_empty(), "{source}");
    }
}

#[test]
fn enum_members_require_commas_even_across_line_breaks() {
    for (source, token) in [
        ("enum E { A B }", "B"),
        ("enum E { A\nB }", "B"),
        ("enum E { A; B }", ";"),
        ("enum E { A: 1 }", ":"),
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1357);
        assert!(tsc_rs_parser::has_syntax_errors(&errors));
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], token);
    }
    for source in ["enum E {}", "enum E { A, B, }", "enum E { A = 1,\n B = 2 }"] {
        assert!(diagnostics(source).is_empty(), "{source}");
    }
}

#[test]
fn accessor_bodies_in_type_contexts_report_the_whole_body() {
    for source in [
        "type T = { get value() { return 1; } };",
        "interface T { set value(v: number) { } }",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1183);
        assert!(!tsc_rs_parser::has_syntax_errors(&errors));
        let span = errors[0].span.unwrap();
        let body = &source[span.start as usize..span.end as usize];
        assert!(matches!(body, "{ return 1; }" | "{ }"), "{body}");
    }
    assert!(diagnostics("interface T { get value(): number; set value(v: number); }").is_empty());
    let errors = diagnostics("type T = { get value() {} }; const broken = ;");
    assert!(tsc_rs_parser::has_syntax_errors(&errors));
    assert!(!errors.iter().any(|d| d.code == 1183), "{errors:?}");
    let errors = diagnostics("interface T { get value() { const broken = ; } }");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 1109);
}
