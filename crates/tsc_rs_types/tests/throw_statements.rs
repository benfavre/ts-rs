use tsc_rs_ast::{Diagnostic, Span};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let file = tsc_rs_parser::parse("throw.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let output = TypeChecker::new().check(&file, &symbols);
    (file.diagnostics, output.diagnostics)
}

#[test]
fn missing_throw_operand_is_a_parse_error_without_a_name_error() {
    for source in [
        "throw",
        "throw;",
        "throw /* comment */;",
        "function f() { throw }",
    ] {
        let (parse, check) = diagnostics(source);
        assert_eq!(parse.len(), 1, "{source}: {parse:?}");
        assert_eq!(parse[0].code, 1109, "{source}");
        assert!(check.is_empty(), "{source}: {check:?}");
    }
}

#[test]
fn throw_line_break_is_reported_before_trivia_and_checks_the_next_statement() {
    for source in [
        "throw\nmissing;",
        "throw\r\nmissing;",
        "throw /*\ncomment */ missing;",
        "throw // comment\nmissing;",
        "throw\u{2028}missing;",
        "throw\u{2029}missing;",
    ] {
        let (parse, check) = diagnostics(source);
        assert!(parse.is_empty(), "{source}: {parse:?}");
        assert_eq!(check.len(), 2, "{source}: {check:?}");
        let line_break = check.iter().find(|d| d.code == 1142).unwrap();
        assert_eq!(line_break.span, Some(Span::new(5, 5)), "{source}");
        let name_error = check.iter().find(|d| d.code == 2304).unwrap();
        assert_eq!(name_error.message, "Cannot find name 'missing'.");
    }
}

#[test]
fn throw_before_a_final_line_break_is_a_grammar_error() {
    let (parse, check) = diagnostics("throw\n");
    assert!(parse.is_empty(), "{parse:?}");
    assert_eq!(check.len(), 1, "{check:?}");
    assert_eq!(check[0].code, 1142);
    assert_eq!(check[0].span, Some(Span::new(5, 5)));
}

#[test]
fn throw_grammar_error_is_suppressed_by_parse_or_ambient_errors() {
    for source in ["throw\n0; const x = ;", "declare namespace N { throw\n0; }"] {
        let (parse, check) = diagnostics(source);
        assert!(!parse.is_empty(), "{source}");
        assert!(check.is_empty(), "{source}: {check:?}");
    }
}

#[test]
fn valid_throw_expressions_allow_trivia_and_statement_asi() {
    for source in [
        "throw 1;",
        "throw /* comment */ 1;",
        "throw (\n1\n);",
        "function f() { throw 1 }",
        "throw 1\ndebugger;",
    ] {
        let (parse, check) = diagnostics(source);
        assert!(parse.is_empty(), "{source}: {parse:?}");
        assert!(check.is_empty(), "{source}: {check:?}");
    }
}
