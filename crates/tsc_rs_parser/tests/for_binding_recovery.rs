use tsc_rs_ast::{ForInOfLeft, PatKind, StmtKind};

#[test]
fn missing_const_loop_bindings_preserve_the_loop_structure() {
    for source in ["for (const of values) {}", "for (const in values) {}"] {
        let file = tsc_rs_parser::parse("case.ts", source);
        assert_eq!(file.statements.len(), 1, "{source}: {:?}", file.statements);
        let left = match &file.statements[0].kind {
            StmtKind::ForOf(loop_) => &loop_.left,
            StmtKind::ForIn(loop_) => &loop_.left,
            other => panic!("{source}: {other:?}"),
        };
        let ForInOfLeft::Var(declaration) = left else {
            panic!("{source}: {left:?}")
        };
        assert!(
            matches!(&declaration.declarations[0].name.kind, PatKind::Ident(name) if name == "<error>")
        );
    }
}

#[test]
fn a_const_binding_named_of_remains_a_real_binding() {
    for source in ["for (const of of values) {}", "for (const of in values) {}"] {
        let file = tsc_rs_parser::parse("case.ts", source);
        assert!(
            file.diagnostics.is_empty(),
            "{source}: {:?}",
            file.diagnostics
        );
        assert_eq!(file.statements.len(), 1);
        let left = match &file.statements[0].kind {
            StmtKind::ForOf(loop_) => &loop_.left,
            StmtKind::ForIn(loop_) => &loop_.left,
            other => panic!("{source}: {other:?}"),
        };
        let ForInOfLeft::Var(declaration) = left else {
            panic!("{source}: {left:?}")
        };
        assert!(
            matches!(&declaration.declarations[0].name.kind, PatKind::Ident(name) if name == "of")
        );
    }
}
