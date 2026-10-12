use tsc_rs_ast::{ForInOfLeft, Span, StmtKind};

fn assert_diagnostics(source: &str, expected: &[(u32, Span)]) -> tsc_rs_ast::SourceFile {
    let file = tsc_rs_parser::parse("declarations.ts", source);
    let actual: Vec<_> = file
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.span.unwrap()))
        .collect();
    assert_eq!(actual, expected, "{source}: {:?}", file.diagnostics);
    file
}

#[test]
fn empty_lists_preserve_their_zero_width_span() {
    for source in ["var", "var ;", "var /* comment */ ;", "var\n"] {
        let file = assert_diagnostics(source, &[(1123, Span::new(3, 3))]);
        let StmtKind::Var(statement) = &file.statements[0].kind else {
            panic!("expected variable statement");
        };
        assert!(statement.declarations.is_empty());
    }
    assert_diagnostics("for (var ; ; ) {}", &[(1123, Span::new(8, 8))]);
}

#[test]
fn trailing_commas_do_not_create_phantom_declarations() {
    for source in ["var a,", "var a,;", "var a,\nfunction f() {}"] {
        let file = assert_diagnostics(source, &[(1009, Span::new(5, 6))]);
        let StmtKind::Var(statement) = &file.statements[0].kind else {
            panic!("expected variable statement");
        };
        assert_eq!(statement.declarations.len(), 1);
    }
    assert_diagnostics("for (var a,; ; ) {}", &[(1009, Span::new(10, 11))]);
}

#[test]
fn empty_for_in_and_of_lists_are_represented_as_empty() {
    for keyword in ["in", "of"] {
        let source = format!("for (var {keyword} values) {{}}");
        let file = assert_diagnostics(&source, &[(1123, Span::new(8, 8))]);
        let left = match &file.statements[0].kind {
            StmtKind::ForIn(statement) => &statement.left,
            StmtKind::ForOf(statement) => &statement.left,
            _ => panic!("expected loop"),
        };
        let ForInOfLeft::Var(statement) = left else {
            panic!("expected declaration list");
        };
        assert!(statement.declarations.is_empty());
    }
}

#[test]
fn missing_separators_report_commas_before_expression_recovery() {
    for (source, span) in [
        ("var a typeof b;", Span::new(6, 12)),
        ("var a void b;", Span::new(6, 10)),
        ("var a class C {}", Span::new(6, 11)),
        ("var a: void.x;", Span::new(11, 12)),
    ] {
        let file = assert_diagnostics(source, &[(1005, span)]);
        assert_eq!(file.diagnostics[0].message, "',' expected.");
    }
    assert_diagnostics(
        "var a-;",
        &[(1005, Span::new(5, 6)), (1109, Span::new(6, 7))],
    );
}

#[test]
fn extra_commas_recover_to_the_next_binding() {
    let file = assert_diagnostics("var a,,b;", &[(1134, Span::new(6, 7))]);
    let StmtKind::Var(statement) = &file.statements[0].kind else {
        panic!("expected variable statement");
    };
    assert_eq!(statement.declarations.len(), 2);
}

#[test]
fn invalid_decorators_take_precedence_over_list_grammar() {
    for source in ["@dec var;", "@dec const;", "@dec var a,;"] {
        assert_diagnostics(source, &[(1206, Span::new(0, 1))]);
    }
}

#[test]
fn valid_multiline_lists_and_contextual_names_are_preserved() {
    for source in [
        "var a,\nb;",
        "var of;",
        "for (var of of []) {}",
        "var a\nfunction f() {}",
        "var a = 1, b = 2;",
        "var { a } = { a: 1 }; var [b] = [2];",
    ] {
        assert_diagnostics(source, &[]);
    }
}
