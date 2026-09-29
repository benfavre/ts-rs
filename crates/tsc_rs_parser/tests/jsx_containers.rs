use tsc_rs_ast::{ExprKind, JsxAttribute, JsxChild, StmtKind};

#[test]
fn jsx_containers_parse_complete_comma_expressions() {
    let source = "<div value={a, b} {...a, b}>{a, b}</div>; after;";
    let file = tsc_rs_parser::parse("containers.tsx", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let StmtKind::Expr(expr) = &file.statements[0].kind else {
        panic!("expected expression")
    };
    let ExprKind::JsxElement(element) = &expr.kind else {
        panic!("expected JSX")
    };
    let JsxAttribute::Normal {
        value: Some(value), ..
    } = &element.attributes[0]
    else {
        panic!("expected value")
    };
    let JsxAttribute::Spread(spread, _) = &element.attributes[1] else {
        panic!("expected spread")
    };
    let JsxChild::Expression(Some(child), _) = &element.children[0] else {
        panic!("expected child")
    };
    for expr in [value, spread, child] {
        assert!(matches!(&expr.kind, ExprKind::Comma(values) if values.len() == 2));
        assert_eq!(
            &source[expr.span.start as usize..expr.span.end as usize],
            "a, b"
        );
    }
    assert_eq!(file.statements.len(), 2);
    assert!(
        matches!(&file.statements[1].kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::Ident(name) if name == "after"))
    );
}

#[test]
fn jsx_text_reports_forbidden_characters_at_exact_byte_positions() {
    for text in [
        "}",
        ">",
        ">>",
        "é}>😀",
        "// >}",
        "/* >} */",
        "/* >}\n */",
        "// >}\n",
    ] {
        for suffix in ["", "{value}", "<span />"] {
            let source = format!("<div>{text}{suffix}</div>;");
            let file = tsc_rs_parser::parse("text.tsx", &source);
            let StmtKind::Expr(expr) = &file.statements[0].kind else {
                panic!("expected expression")
            };
            let ExprKind::JsxElement(element) = &expr.kind else {
                panic!("expected JSX")
            };
            let JsxChild::Text(actual, span) = &element.children[0] else {
                panic!("expected captured text")
            };
            assert_eq!(actual, text, "{source}");
            assert_eq!(span.start, 5);
            assert_eq!(span.end as usize, 5 + text.len());
            let expected: Vec<_> = text
                .bytes()
                .enumerate()
                .filter_map(|(i, b)| match b {
                    b'}' => Some((1381, i as u32 + 5)),
                    b'>' => Some((1382, i as u32 + 5)),
                    _ => None,
                })
                .collect();
            let actual: Vec<_> = file
                .diagnostics
                .iter()
                .map(|d| {
                    let span = d.span.unwrap();
                    assert_eq!(span.end, span.start + 1);
                    (d.code, span.start)
                })
                .collect();
            assert_eq!(actual, expected, "{source}: {:?}", file.diagnostics);
        }
    }
}

#[test]
fn jsx_entities_attributes_and_expression_literals_allow_delimiter_characters() {
    let source = r#"<div value=">}" other={">}"}>&gt;&rbrace;{"}>"}</div>;"#;
    let file = tsc_rs_parser::parse("text.tsx", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
}

#[test]
fn nested_jsx_text_diagnostics_keep_source_order() {
    let source = "<div>}<span>></span>}</div>;";
    let file = tsc_rs_parser::parse("nested.tsx", source);
    let actual: Vec<_> = file
        .diagnostics
        .iter()
        .map(|d| (d.code, d.span.unwrap().start as usize))
        .collect();
    assert_eq!(
        actual,
        [
            (1381, source.find('}').unwrap()),
            (1382, source.find(">>").unwrap() + 1),
            (1381, source.rfind('}').unwrap()),
        ]
    );
}
