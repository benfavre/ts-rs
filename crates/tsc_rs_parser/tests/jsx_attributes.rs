use tsc_rs_ast::{ExprKind, JsxAttribute, JsxChild, StmtKind};

#[test]
fn missing_jsx_initializers_report_the_unconsumed_token() {
    for (source, token) in [
        ("<div attr= />; after;", "/"),
        ("<div attr= ></div>; after;", ">"),
        ("<div attr=value />; after;", "value"),
        ("<div attr=null />; after;", "null"),
    ] {
        let file = tsc_rs_parser::parse("attributes.tsx", source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, 1145);
        assert_eq!(diagnostic.message, "'{' or JSX element expected.");
        let span = diagnostic.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], token);
        assert_eq!(file.statements.len(), 2);
        assert!(
            matches!(&file.statements[1].kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::Ident(name) if name == "after"))
        );
    }
}

#[test]
fn empty_jsx_attribute_containers_retain_braces_and_following_attributes() {
    let source = r#"<div bare empty={} comment={/*empty*/} literal="">{}</div>; after;"#;
    let file = tsc_rs_parser::parse("attributes.tsx", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let StmtKind::Expr(expr) = &file.statements[0].kind else {
        panic!("expected expression")
    };
    let ExprKind::JsxElement(element) = &expr.kind else {
        panic!("expected JSX")
    };
    assert_eq!(element.attributes.len(), 4);
    assert!(matches!(
        &element.attributes[0],
        JsxAttribute::Normal { value: None, .. }
    ));
    for (attr, text) in [
        (&element.attributes[1], "{}"),
        (&element.attributes[2], "{/*empty*/}"),
    ] {
        let JsxAttribute::Normal {
            value: Some(value), ..
        } = attr
        else {
            panic!("expected container")
        };
        assert!(matches!(value.kind, ExprKind::Omitted));
        assert_eq!(
            &source[value.span.start as usize..value.span.end as usize],
            text
        );
    }
    assert!(
        matches!(&element.attributes[3], JsxAttribute::Normal { value: Some(value), .. } if matches!(&value.kind, ExprKind::StrLit(text) if text.is_empty()))
    );
    assert!(matches!(
        &element.children[0],
        JsxChild::Expression(None, _)
    ));
    assert_eq!(file.statements.len(), 2);
}
