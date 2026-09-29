use tsc_rs_ast::*;

#[test]
fn generic_function_type_arg_after_less_less_parses_as_call_type_argument() {
    let file = tsc_rs_parser::parse("test.ts", "const b = foo<<T>(x: T) => number>(() => 1);");
    let StmtKind::Var(var_stmt) = &file.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::Call(call) = &init.kind else {
        panic!("expected call initializer, got {:?}", init.kind);
    };
    let type_args = call
        .type_args
        .as_ref()
        .expect("expected generic call type arguments");
    assert_eq!(type_args.len(), 1, "expected a single type argument");
    assert!(
        matches!(
            &type_args[0].kind,
            TypeNodeKind::Function(fn_ty) if fn_ty.type_params.is_some()
        ),
        "expected the type argument to stay a generic function type, got {:?}",
        type_args[0].kind
    );
    assert_eq!(call.args.len(), 1, "expected a single call argument");
    assert!(
        matches!(call.args[0].kind, ExprKind::Arrow(_)),
        "expected the runtime argument to remain an arrow expression"
    );
}
