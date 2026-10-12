use tsc_rs_ast::{ForInOfLeft, StmtKind};

#[test]
fn for_in_preserves_the_complete_target_expression() {
    for target in [
        "key",
        "key = 1",
        "unknown()",
        "new unknown()",
        "this",
        "{}",
        "(key)",
    ] {
        let source = format!("for ({target} in values) {{}}");
        let file = tsc_rs_parser::parse("loop.ts", &source);
        assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
        let StmtKind::ForIn(loop_) = &file.statements[0].kind else {
            panic!("{:?}", file.statements)
        };
        let ForInOfLeft::Expr(expression) = &loop_.left else {
            panic!("{:?}", loop_.left)
        };
        assert_eq!(
            &source[expression.span.start as usize..expression.span.end as usize],
            target
        );
    }
}
