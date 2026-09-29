use super::*;

fn parse_src(src: &str) -> SourceFile {
    parse("test.ts", src)
}

fn parse_tsx_src(src: &str) -> SourceFile {
    parse_with_jsx("test.tsx", src, true)
}

fn first_stmt(src: &str) -> Stmt {
    let sf = parse_src(src);
    assert!(!sf.statements.is_empty(), "expected at least one statement");
    sf.statements.into_iter().next().unwrap()
}

fn first_expr(src: &str) -> Expr {
    match first_stmt(src).kind {
        StmtKind::Expr(e) => *e,
        other => panic!("expected expression statement, got {:?}", other),
    }
}

fn assert_no_errors(src: &str) {
    let sf = parse_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "expected no errors, got: {:?}",
        sf.diagnostics
    );
}

#[test]
fn legacy_octal_suffix_requires_a_same_line_semicolon_boundary() {
    let file = parse_src(
        r#"
00.5; 000.5; 01.5; 001.5;
00e5; 000e5; 01e5; 001e5;
00.5e5; 000.5e5; 01.5e5; 001.5e5;
00.5_5; 000.5_5; 01.5_5; 001.5_5;
00e5_5; 000e5_5; 01e5_5; 001e5_5;
00.5_5e5_5; 000.5_5e5_5; 01.5_5e5_5; 001.5_5e5_5;
"#,
    );
    let ts1005: Vec<_> = file
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1005)
        .collect();

    assert_eq!(ts1005.len(), 24, "{:#?}", file.diagnostics);
}

#[test]
fn legacy_octal_boundary_respects_asi_recovery_and_speculation() {
    let negative = parse_src("08.5; 01 02; 01 /*same*/ 02; 01 08;\n00\n.5;");
    assert!(
        negative
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1005),
        "{:#?}",
        negative.diagnostics
    );

    let nested = parse_src("const f = (x = () => { 01.5; }) => x;");
    assert_eq!(
        nested
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1005)
            .count(),
        1,
        "{:#?}",
        nested.diagnostics
    );
}

#[test]
fn legacy_octal_boundary_reports_signed_and_incomplete_exponent_suffixes() {
    let file = parse_src(
        "01e+5; 01e-5; 01e; 01e_5; 01E+5; 01E-5; 01E; 01E_5; 01 /*x*/ e+5; 01efoo; 01Ex; 01e$;",
    );

    assert_eq!(
        file.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1005)
            .count(),
        12,
        "{:#?}",
        file.diagnostics
    );
}

#[test]
fn tsx_class_expr_in_extends_keeps_inner_members() {
    let src = r#"
class Foo extends createComponentClass(() => class extends React.Component<{}, {}> {
  render() {
    return <span>Hello, world!</span>;
  }
}) {}
"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let StmtKind::ClassDecl(outer) = &sf.statements[0].kind else {
        panic!("expected outer class declaration");
    };
    let Some(extends_expr) = &outer.extends else {
        panic!("expected extends expression");
    };
    let ExprKind::Call(call) = &extends_expr.kind else {
        panic!(
            "expected call in extends expression, got {:?}",
            extends_expr.kind
        );
    };
    let Some(first_arg) = call.args.first() else {
        panic!("expected call argument");
    };
    let ExprKind::Arrow(arrow) = &first_arg.kind else {
        panic!("expected arrow argument, got {:?}", first_arg.kind);
    };
    let ArrowBody::Expr(inner_expr) = &arrow.body else {
        panic!("expected expression-bodied arrow");
    };
    let ExprKind::ClassExpr(inner_class) = &inner_expr.kind else {
        panic!("expected inner class expression, got {:?}", inner_expr.kind);
    };
    assert!(
        !inner_class.members.is_empty(),
        "inner class should keep members; got empty class"
    );
}

#[test]
fn tsx_this_self_closing_tag_parses_as_jsx() {
    let src = r#"const x = <this type="foo" />;"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let StmtKind::Var(v) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = v.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::JsxSelfClosing(el) = &init.kind else {
        panic!("expected JSX self-closing element, got {:?}", init.kind);
    };
    assert!(
        matches!(&el.name.kind, ExprKind::This),
        "expected JSX tag name to be a `this` expression, got {:?}",
        el.name.kind
    );
}

#[test]
fn tsx_multiline_quoted_jsx_attr_values_parse_as_string_literals() {
    let src = r#"
const a = <input value="
foo: 23
"></input>;
const b = <input value='
foo: 23
'></input>;
"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    assert_eq!(sf.statements.len(), 2, "expected two variable statements");
    for stmt in &sf.statements {
        let StmtKind::Var(v) = &stmt.kind else {
            panic!("expected variable statement");
        };
        let init = v.declarations[0]
            .init
            .as_ref()
            .expect("expected initializer");
        let ExprKind::JsxElement(el) = &init.kind else {
            panic!("expected JSX element, got {:?}", init.kind);
        };
        let value_expr = el
            .attributes
            .iter()
            .find_map(|a| match a {
                JsxAttribute::Normal { name, value, .. } if name == "value" => value.as_deref(),
                _ => None,
            })
            .expect("expected value attribute");
        assert!(
            matches!(value_expr.kind, ExprKind::StrLit(_)),
            "expected string literal attribute value, got {:?}",
            value_expr.kind
        );
    }
}

#[test]
fn non_leading_shebang_recovers_to_not_on_division_chain() {
    let sf = parse_src("var foo = 0;\n#!/usr/bin/env node");
    assert!(
        sf.statements.len() >= 3,
        "expected recovered shebang to produce trailing statements, got {:?}",
        sf.statements
    );
    let StmtKind::Expr(expr) = &sf.statements[1].kind else {
        panic!(
            "expected second statement to be an expression, got {:?}",
            sf.statements[1]
        );
    };
    let ExprKind::Binary(bin) = &expr.kind else {
        panic!(
            "expected recovered shebang to keep `/ env` as division, got {:?}",
            expr.kind
        );
    };
    assert_eq!(bin.op, BinaryOp::Div);
    assert!(
        matches!(
            &bin.left.kind,
            ExprKind::Unary(un)
                if un.op == UnaryOp::LogNot
                    && matches!(
                        &un.argument.kind,
                        ExprKind::RegexpLit(re)
                            if re.pattern == "usr" && re.flags == "bin"
                    )
        ),
        "expected division LHS to stay `!/usr/bin`, got {:?}",
        bin.left.kind
    );
    assert!(
        matches!(bin.right.kind, ExprKind::Ident(ref name) if name == "env"),
        "expected division RHS to be `env`, got {:?}",
        bin.right.kind
    );
}

#[test]
fn unterminated_string_literal_preserves_trailing_text_in_recovery() {
    let sf = parse_src("var es1 = \"line 1\n\";\nvar es3 = 'line 1\\ \n';");
    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected first statement to be a variable statement");
    };
    let Some(init) = &var_stmt.declarations[0].init else {
        panic!("expected initializer");
    };
    assert!(
        matches!(&init.kind, ExprKind::StrLit(s) if s == "line 1"),
        "unterminated string recovery should keep the last character, got {:?}",
        init.kind
    );

    let StmtKind::Expr(expr_stmt) = &sf.statements[1].kind else {
        panic!("expected recovered trailing string expression");
    };
    assert!(
        matches!(&expr_stmt.kind, ExprKind::StrLit(s) if s == ";"),
        "expected trailing line to recover as string literal containing `;`, got {:?}",
        expr_stmt.kind
    );

    let StmtKind::Var(var_stmt) = &sf.statements[2].kind else {
        panic!("expected third statement to be a variable statement");
    };
    let Some(init) = &var_stmt.declarations[0].init else {
        panic!("expected initializer");
    };
    assert!(
        matches!(&init.kind, ExprKind::StrLit(s) if s == "line 1\\ "),
        "backslash-space recovery should preserve the space after the slash, got {:?}",
        init.kind
    );
}

#[test]
fn tsx_namespace_begin_of_ident_recovery_stays_local() {
    let src = r#"
var beginOfIdent1 = <:a attr={"value"} />;
var beginOfIdent2 = <a :attr={"value"} />;
"#;
    let sf = parse_tsx_src(src);
    assert_eq!(sf.statements.len(), 2, "expected two var statements");

    let StmtKind::Var(first) = &sf.statements[0].kind else {
        panic!("expected first statement to be var");
    };
    assert_eq!(
        first.declarations.len(),
        3,
        "expected recovered first var statement to keep three declarators"
    );
    assert!(
        matches!(&first.declarations[1].name.kind, PatKind::Ident(n) if n == "a"),
        "expected second recovered declarator name to be `a`"
    );
    assert!(
        matches!(&first.declarations[2].name.kind, PatKind::Ident(n) if n == "attr"),
        "expected third recovered declarator name to be `attr`"
    );

    let StmtKind::Var(second) = &sf.statements[1].kind else {
        panic!("expected second statement to be var");
    };
    assert_eq!(
        second.declarations.len(),
        1,
        "expected second var statement not to be swallowed by recovery"
    );
    let init = second.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    assert!(
        matches!(init.kind, ExprKind::JsxSelfClosing(_)),
        "expected beginOfIdent2 initializer to remain JSX self-closing"
    );
}

#[test]
fn tsx_namespace_attr_trailing_colon_recovery_keeps_colon() {
    let src = r#"const x = <a attr:={"value"} />;"#;
    let sf = parse_tsx_src(src);
    assert!(
        !sf.statements.is_empty(),
        "expected at least one statement, diagnostics: {:?}",
        sf.diagnostics
    );
    let StmtKind::Var(v) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = v.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::JsxSelfClosing(el) = &init.kind else {
        panic!("expected JSX self-closing element");
    };
    let attr_name = el
        .attributes
        .iter()
        .find_map(|a| match a {
            JsxAttribute::Normal { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .expect("expected recovered attribute");
    assert_eq!(attr_name, "attr:");
}

#[test]
fn tsx_namespace_closing_tag_extra_colon_recovers_as_var_declarator_split() {
    let src = r#"var x = <a:ele:ment></a:ele:ment>;"#;
    let sf = parse_tsx_src(src);
    let StmtKind::Var(v) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    assert_eq!(
        v.declarations.len(),
        2,
        "expected malformed closing tag to recover as `, ment` declarator split"
    );
    assert!(
        matches!(&v.declarations[1].name.kind, PatKind::Ident(n) if n == "ment"),
        "expected second declarator to be `ment`"
    );
    assert!(
        matches!(&sf.statements[1].kind, StmtKind::Expr(_)),
        "expected trailing ` > ;` recovery expression statement"
    );
}

// =======================================================================
// Variable declarations
// =======================================================================

#[test]
fn var_declaration() {
    let stmt = first_stmt("var x = 1;");
    match stmt.kind {
        StmtKind::Var(v) => {
            assert_eq!(v.kind, VarKind::Var);
            assert_eq!(v.declarations.len(), 1);
            assert!(matches!(v.declarations[0].name.kind, PatKind::Ident(ref n) if n == "x"));
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn let_declaration() {
    let stmt = first_stmt("let y = 2;");
    match stmt.kind {
        StmtKind::Var(v) => {
            assert_eq!(v.kind, VarKind::Let);
            assert_eq!(v.declarations.len(), 1);
        }
        _ => panic!("expected let"),
    }
}

#[test]
fn const_declaration() {
    let stmt = first_stmt("const z: number = 3;");
    match stmt.kind {
        StmtKind::Var(v) => {
            assert_eq!(v.kind, VarKind::Const);
            assert!(v.declarations[0].type_ann.is_some());
        }
        _ => panic!("expected const"),
    }
}

#[test]
fn multiple_declarators() {
    let stmt = first_stmt("let a = 1, b = 2, c = 3;");
    match stmt.kind {
        StmtKind::Var(v) => assert_eq!(v.declarations.len(), 3),
        _ => panic!("expected var"),
    }
}

#[test]
fn destructuring_object_pattern() {
    let stmt = first_stmt("const { a, b: c } = obj;");
    match stmt.kind {
        StmtKind::Var(v) => {
            assert!(matches!(v.declarations[0].name.kind, PatKind::Object(_)));
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn destructuring_array_pattern() {
    let stmt = first_stmt("const [a, , b] = arr;");
    match stmt.kind {
        StmtKind::Var(v) => {
            assert!(matches!(v.declarations[0].name.kind, PatKind::Array(_)));
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn repro_parser_recovery() {
    let src = "class C { public {}; }";
    let file = parse("test.ts", src);

    // After fix, we expect diagnostics and NO property named "{"
    assert!(
        !file.diagnostics.is_empty(),
        "expected diagnostics for malformed class member"
    );

    if let StmtKind::ClassDecl(c) = &file.statements[0].kind {
        // It should NOT have a property named "{"
        for member in &c.members {
            if let ClassMemberKind::Property(p) = &member.kind {
                if let PropName::Ident(name, _) = &p.name {
                    assert_ne!(name, "{", "Should not parse '{{' as property name");
                }
            }
        }
    }
}

#[test]
fn definite_assignment() {
    let stmt = first_stmt("let x!: number;");
    match stmt.kind {
        StmtKind::Var(v) => {
            assert!(v.declarations[0].definite);
            assert!(v.declarations[0].type_ann.is_some());
        }
        _ => panic!("expected var"),
    }
}

// =======================================================================
// Control flow statements
// =======================================================================

#[test]
fn if_else_statement() {
    let stmt = first_stmt("if (x) { a(); } else { b(); }");
    assert!(matches!(
        stmt.kind,
        StmtKind::If(IfStmt {
            alternate: Some(_),
            ..
        })
    ));
}

#[test]
fn while_statement() {
    let stmt = first_stmt("while (true) { break; }");
    assert!(matches!(stmt.kind, StmtKind::While(_)));
}

#[test]
fn do_while_statement() {
    let stmt = first_stmt("do { x++; } while (x < 10);");
    assert!(matches!(stmt.kind, StmtKind::DoWhile(_)));
}

#[test]
fn for_statement() {
    let stmt = first_stmt("for (let i = 0; i < 10; i++) { }");
    assert!(matches!(stmt.kind, StmtKind::For(_)));
}

#[test]
fn malformed_empty_for_header_keeps_for_statement() {
    let sf = parse_src("for () { // error\n}");
    assert_eq!(sf.statements.len(), 1, "{:?}", sf.statements);
    let StmtKind::For(for_stmt) = &sf.statements[0].kind else {
        panic!("{:?}", sf.statements[0]);
    };
    assert!(matches!(
        &for_stmt.init,
        Some(ForInit::Expr(expr)) if matches!(expr.kind, ExprKind::Ident(ref name) if name == "<error>")
    ));
    assert!(for_stmt.update.is_some(), "{for_stmt:?}");
}

#[test]
fn for_in_statement() {
    let stmt = first_stmt("for (const k in obj) { }");
    assert!(matches!(stmt.kind, StmtKind::ForIn(_)));
}

#[test]
fn for_of_statement() {
    let stmt = first_stmt("for (const v of arr) { }");
    assert!(matches!(
        stmt.kind,
        StmtKind::ForOf(ref fo) if !fo.is_await
    ));
}

fn non_variable_for_left(source: &str) -> ForInOfLeft {
    let file = parse_src(source);
    assert!(
        file.diagnostics.is_empty(),
        "unexpected parse diagnostics: {:?}",
        file.diagnostics
    );
    match file
        .statements
        .into_iter()
        .next()
        .expect("expected for-in/of statement")
        .kind
    {
        StmtKind::ForIn(statement) => statement.left,
        StmtKind::ForOf(statement) => statement.left,
        other => panic!("expected for-in/of statement, got {other:?}"),
    }
}

fn non_variable_for_left_expr(source: &str, expected_source: &str) -> Box<Expr> {
    let ForInOfLeft::Expr(expression) = non_variable_for_left(source) else {
        panic!("expected expression for-in/of target");
    };
    assert_eq!(
        &source[expression.span.start as usize..expression.span.end as usize],
        expected_source
    );
    expression
}

#[test]
fn for_in_of_property_targets_preserve_expressions() {
    assert!(matches!(
        non_variable_for_left_expr("for (ns.value in source) {}", "ns.value").kind,
        ExprKind::Member(_)
    ));
    assert!(matches!(
        non_variable_for_left_expr("for (ns['value'] of source) {}", "ns['value']").kind,
        ExprKind::ElemAccess(_)
    ));
    assert!(matches!(
        non_variable_for_left_expr("for ((ns.value) in source) {}", "(ns.value)").kind,
        ExprKind::Paren(_)
    ));
}

#[test]
fn for_in_of_wrapped_property_targets_preserve_expressions() {
    assert!(matches!(
        non_variable_for_left_expr("for (ns.value as any in source) {}", "ns.value as any").kind,
        ExprKind::As(_)
    ));
    assert!(matches!(
        non_variable_for_left_expr(
            "for (ns.value satisfies any in source) {}",
            "ns.value satisfies any"
        )
        .kind,
        ExprKind::Satisfies(_)
    ));
    assert!(matches!(
        non_variable_for_left_expr("for (<any>ns.value in source) {}", "<any>ns.value").kind,
        ExprKind::TypeAssertion(_)
    ));
    assert!(matches!(
        non_variable_for_left_expr("for (ns.value! in source) {}", "ns.value!").kind,
        ExprKind::NonNull(_)
    ));
}

#[test]
fn for_in_of_bindings_and_destructuring_remain_patterns() {
    assert!(matches!(
        non_variable_for_left("for (key in source) {}"),
        ForInOfLeft::Pat(Pat {
            kind: PatKind::Ident(name),
            ..
        }) if name == "key"
    ));
    assert!(matches!(
        non_variable_for_left("for ((key) of source) {}"),
        ForInOfLeft::Pat(Pat {
            kind: PatKind::Ident(name),
            ..
        }) if name == "key"
    ));
    assert!(matches!(
        non_variable_for_left("for ([value] of source) {}"),
        ForInOfLeft::Pat(Pat {
            kind: PatKind::Array(_),
            ..
        })
    ));
    assert!(matches!(
        non_variable_for_left("for ({ value } of source) {}"),
        ForInOfLeft::Pat(Pat {
            kind: PatKind::Object(_),
            ..
        })
    ));
}

#[test]
fn for_of_array_object_rest_left_converts_to_pattern() {
    let stmt = first_stmt("for ([{ ...y }] of arr) ;");
    let StmtKind::ForOf(fo) = stmt.kind else {
        panic!("expected for-of statement");
    };
    let ForInOfLeft::Pat(pat) = fo.left else {
        panic!("expected for-of left to be converted to a pattern");
    };
    let PatKind::Array(elements) = pat.kind else {
        panic!("expected array pattern");
    };
    let Some(ArrayPatElem::Pat(inner)) = elements.first().and_then(|elem| elem.clone()) else {
        panic!("expected first array element pattern");
    };
    let PatKind::Object(props) = inner.kind else {
        panic!("expected nested object pattern");
    };
    assert!(
        props.iter().any(|prop| matches!(prop, ObjPatProp::Rest(_))),
        "expected nested object pattern to preserve the rest element"
    );
}

#[test]
fn for_of_array_default_left_preserves_assign_pattern() {
    let stmt = first_stmt("for ([x = await a] of y) { z; }");
    let StmtKind::ForOf(fo) = stmt.kind else {
        panic!("expected for-of statement");
    };
    let ForInOfLeft::Pat(pat) = fo.left else {
        panic!("expected for-of left to be converted to a pattern");
    };
    let PatKind::Array(elements) = pat.kind else {
        panic!("expected array pattern");
    };
    let Some(ArrayPatElem::Pat(first)) = elements.first().and_then(|elem| elem.clone()) else {
        panic!("expected first array element pattern");
    };
    match &first.kind {
        PatKind::Assign(inner, init) => {
            assert!(
                matches!(inner.kind, PatKind::Ident(ref name) if name == "x"),
                "expected default binding to target x"
            );
            assert!(
                matches!(init.kind, ExprKind::Await(_)),
                "expected default initializer to preserve await"
            );
        }
        other => panic!("expected assign pattern, got {:?}", other),
    }
}

#[test]
fn for_await_of_statement() {
    let stmt = first_stmt("for await (const v of gen) { }");
    assert!(matches!(
        stmt.kind,
        StmtKind::ForOf(ref fo) if fo.is_await
    ));
}

#[test]
fn switch_statement() {
    let stmt = first_stmt("switch (x) { case 1: a(); break; default: b(); }");
    match stmt.kind {
        StmtKind::Switch(s) => assert_eq!(s.cases.len(), 2),
        _ => panic!("expected switch"),
    }
}

#[test]
fn try_catch_finally() {
    let stmt = first_stmt("try { a(); } catch (e) { b(); } finally { c(); }");
    match stmt.kind {
        StmtKind::Try(t) => {
            assert!(t.handler.is_some());
            assert!(t.finalizer.is_some());
        }
        _ => panic!("expected try"),
    }
}

#[test]
fn try_catch_no_binding() {
    let stmt = first_stmt("try { a(); } catch { b(); }");
    match stmt.kind {
        StmtKind::Try(t) => {
            let handler = t.handler.unwrap();
            assert!(handler.param.is_none());
        }
        _ => panic!("expected try"),
    }
}

#[test]
fn throw_statement() {
    let stmt = first_stmt("throw new Error('oops');");
    assert!(matches!(stmt.kind, StmtKind::Throw(_)));
}

#[test]
fn return_statement() {
    let stmt = first_stmt("return x + 1;");
    assert!(matches!(stmt.kind, StmtKind::Return(Some(_))));
}

#[test]
fn return_void() {
    let stmt = first_stmt("return;");
    assert!(matches!(stmt.kind, StmtKind::Return(None)));
}

#[test]
fn break_statement() {
    let stmt = first_stmt("break;");
    assert!(matches!(stmt.kind, StmtKind::Break(None)));
}

#[test]
fn break_with_label() {
    let stmt = first_stmt("break outer;");
    assert!(matches!(stmt.kind, StmtKind::Break(Some(ref l)) if l == "outer"));
}

#[test]
fn continue_statement() {
    let stmt = first_stmt("continue;");
    assert!(matches!(stmt.kind, StmtKind::Continue(None)));
}

#[test]
fn labeled_statement() {
    let stmt = first_stmt("outer: for (;;) { }");
    match stmt.kind {
        StmtKind::Labeled(l) => assert_eq!(l.label, "outer"),
        _ => panic!("expected labeled"),
    }
}

#[test]
fn debugger_statement() {
    let stmt = first_stmt("debugger;");
    assert!(matches!(stmt.kind, StmtKind::Debugger));
}

#[test]
fn with_statement() {
    let stmt = first_stmt("with (obj) { x; }");
    assert!(matches!(stmt.kind, StmtKind::With(_)));
}

#[test]
fn empty_statement() {
    let stmt = first_stmt(";");
    assert!(matches!(stmt.kind, StmtKind::Empty));
}

#[test]
fn block_statement() {
    let stmt = first_stmt("{ a; b; }");
    match stmt.kind {
        StmtKind::Block(stmts) => assert_eq!(stmts.len(), 2),
        _ => panic!("expected block"),
    }
}

// =======================================================================
// Binary expressions with precedence
// =======================================================================

#[test]
fn binary_add_mul_precedence() {
    // a + b * c should parse as a + (b * c)
    let expr = first_expr("a + b * c;");
    match expr.kind {
        ExprKind::Binary(b) => {
            assert_eq!(b.op, BinaryOp::Add);
            assert!(
                matches!(b.right.kind, ExprKind::Binary(ref inner) if inner.op == BinaryOp::Mul)
            );
        }
        _ => panic!("expected binary"),
    }
}

#[test]
fn binary_comparison() {
    let expr = first_expr("a === b;");
    match expr.kind {
        ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::StrictEq),
        _ => panic!("expected binary"),
    }
}

#[test]
fn binary_logical() {
    let expr = first_expr("a && b || c;");
    match expr.kind {
        ExprKind::Binary(b) => {
            assert_eq!(b.op, BinaryOp::LogOr);
            assert!(
                matches!(b.left.kind, ExprKind::Binary(ref inner) if inner.op == BinaryOp::LogAnd)
            );
        }
        _ => panic!("expected binary"),
    }
}

#[test]
fn nullish_coalescing() {
    let expr = first_expr("a ?? b;");
    match expr.kind {
        ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::NullCoal),
        _ => panic!("expected binary"),
    }
}

#[test]
fn instanceof_operator() {
    let expr = first_expr("a instanceof B;");
    match expr.kind {
        ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::InstanceOf),
        _ => panic!("expected binary"),
    }
}

#[test]
fn in_operator() {
    let expr = first_expr("'key' in obj;");
    match expr.kind {
        ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::In),
        _ => panic!("expected binary"),
    }
}

#[test]
fn exponentiation_right_assoc() {
    // 2 ** 3 ** 4 should parse as 2 ** (3 ** 4)
    let expr = first_expr("2 ** 3 ** 4;");
    match expr.kind {
        ExprKind::Binary(b) => {
            assert_eq!(b.op, BinaryOp::Exp);
            assert!(
                matches!(b.right.kind, ExprKind::Binary(ref inner) if inner.op == BinaryOp::Exp)
            );
        }
        _ => panic!("expected binary"),
    }
}

#[test]
fn bitwise_ops() {
    let expr = first_expr("a & b | c ^ d;");
    // | has lowest precedence among these, so should be the root
    match expr.kind {
        ExprKind::Binary(b) => assert_eq!(b.op, BinaryOp::BitOr),
        _ => panic!("expected binary"),
    }
}

// =======================================================================
// Unary and update expressions
// =======================================================================

#[test]
fn unary_operators() {
    let expr = first_expr("!a;");
    assert!(matches!(
        expr.kind,
        ExprKind::Unary(UnaryExpr {
            op: UnaryOp::LogNot,
            ..
        })
    ));

    let expr = first_expr("-x;");
    assert!(matches!(
        expr.kind,
        ExprKind::Unary(UnaryExpr {
            op: UnaryOp::Neg,
            ..
        })
    ));

    let expr = first_expr("typeof x;");
    assert!(matches!(expr.kind, ExprKind::Typeof(_)));

    let expr = first_expr("void 0;");
    assert!(matches!(expr.kind, ExprKind::Void(_)));

    let expr = first_expr("delete obj.x;");
    assert!(matches!(expr.kind, ExprKind::Delete(_)));
}

#[test]
fn prefix_update() {
    let expr = first_expr("++x;");
    assert!(matches!(
        expr.kind,
        ExprKind::Update(UpdateExpr {
            op: UpdateOp::PreInc,
            ..
        })
    ));
    let ExprKind::Update(update) = &expr.kind else {
        unreachable!()
    };
    assert_eq!((expr.span.start, expr.span.end), (0, 3));
    assert_eq!(
        (update.argument.span.start, update.argument.span.end),
        (2, 3)
    );

    let expr = first_expr("--x;");
    assert!(matches!(
        expr.kind,
        ExprKind::Update(UpdateExpr {
            op: UpdateOp::PreDec,
            ..
        })
    ));
    let ExprKind::Update(update) = &expr.kind else {
        unreachable!()
    };
    assert_eq!((expr.span.start, expr.span.end), (0, 3));
    assert_eq!(
        (update.argument.span.start, update.argument.span.end),
        (2, 3)
    );
}

#[test]
fn postfix_update() {
    let expr = first_expr("x++;");
    assert!(matches!(
        expr.kind,
        ExprKind::Update(UpdateExpr {
            op: UpdateOp::PostInc,
            ..
        })
    ));
    let ExprKind::Update(update) = &expr.kind else {
        unreachable!()
    };
    assert_eq!((expr.span.start, expr.span.end), (0, 3));
    assert_eq!(
        (update.argument.span.start, update.argument.span.end),
        (0, 1)
    );

    let expr = first_expr("x--;");
    assert!(matches!(
        expr.kind,
        ExprKind::Update(UpdateExpr {
            op: UpdateOp::PostDec,
            ..
        })
    ));
    let ExprKind::Update(update) = &expr.kind else {
        unreachable!()
    };
    assert_eq!((expr.span.start, expr.span.end), (0, 3));
    assert_eq!(
        (update.argument.span.start, update.argument.span.end),
        (0, 1)
    );
}

#[test]
fn await_expr() {
    let expr = first_expr("await promise;");
    assert!(matches!(expr.kind, ExprKind::Await(_)));
}

// =======================================================================
// Ternary / conditional expression
// =======================================================================

#[test]
fn ternary_expression() {
    let expr = first_expr("a ? b : c;");
    match expr.kind {
        ExprKind::Cond(c) => {
            assert!(matches!(c.test.kind, ExprKind::Ident(ref n) if n == "a"));
            assert!(matches!(c.consequent.kind, ExprKind::Ident(ref n) if n == "b"));
            assert!(matches!(c.alternate.kind, ExprKind::Ident(ref n) if n == "c"));
        }
        _ => panic!("expected conditional"),
    }
}

// =======================================================================
// Call, member access, optional chaining
// =======================================================================

#[test]
fn function_call() {
    let expr = first_expr("foo(1, 2);");
    match expr.kind {
        ExprKind::Call(c) => {
            assert!(matches!(c.callee.kind, ExprKind::Ident(ref n) if n == "foo"));
            assert_eq!(c.args.len(), 2);
            assert!(!c.optional);
        }
        _ => panic!("expected call"),
    }
}

#[test]
fn member_access() {
    let expr = first_expr("a.b.c;");
    match expr.kind {
        ExprKind::Member(m) => {
            assert_eq!(m.property, "c");
            assert!(!m.optional);
        }
        _ => panic!("expected member"),
    }
}

#[test]
fn computed_member_access() {
    let expr = first_expr("a[0];");
    match expr.kind {
        ExprKind::ElemAccess(e) => {
            assert!(!e.optional);
        }
        _ => panic!("expected elem access"),
    }
}

#[test]
fn optional_chaining_member() {
    let expr = first_expr("a?.b;");
    match expr.kind {
        ExprKind::Member(m) => {
            assert!(m.optional);
            assert_eq!(m.property, "b");
        }
        _ => panic!("expected optional member"),
    }
}

#[test]
fn optional_chaining_call() {
    let expr = first_expr("a?.();");
    match expr.kind {
        ExprKind::Call(c) => assert!(c.optional),
        _ => panic!("expected optional call"),
    }
}

#[test]
fn optional_chaining_element() {
    let expr = first_expr("a?.[0];");
    match expr.kind {
        ExprKind::ElemAccess(e) => assert!(e.optional),
        _ => panic!("expected optional elem access"),
    }
}

// =======================================================================
// New expression
// =======================================================================

#[test]
fn new_expression() {
    let expr = first_expr("new Foo(1, 2);");
    match expr.kind {
        ExprKind::New(n) => {
            assert!(matches!(n.callee.kind, ExprKind::Ident(ref name) if name == "Foo"));
            assert_eq!(n.args.unwrap().len(), 2);
        }
        _ => panic!("expected new"),
    }
}

#[test]
fn new_without_args() {
    let expr = first_expr("new Foo;");
    match expr.kind {
        ExprKind::New(n) => assert!(n.args.is_none()),
        _ => panic!("expected new"),
    }
}

#[test]
fn new_expression_with_type_assertion_callee_recovers() {
    let expr = first_expr("new <i1> anyVar;");
    let ExprKind::New(new_expr) = expr.kind else {
        panic!("expected new expression");
    };
    assert!(
        matches!(new_expr.callee.kind, ExprKind::TypeAssertion(_)),
        "expected `new` callee to recover as a type assertion, got {:?}",
        new_expr.callee.kind
    );
}

// =======================================================================
// Assignment expressions
// =======================================================================

#[test]
fn simple_assignment() {
    let expr = first_expr("x = 1;");
    match expr.kind {
        ExprKind::Assign(a) => assert_eq!(a.op, AssignOp::Assign),
        _ => panic!("expected assign"),
    }
}

#[test]
fn compound_assignments() {
    let cases = vec![
        ("x += 1;", AssignOp::AddAssign),
        ("x -= 1;", AssignOp::SubAssign),
        ("x *= 1;", AssignOp::MulAssign),
        ("x /= 1;", AssignOp::DivAssign),
        ("x %= 1;", AssignOp::ModAssign),
        ("x **= 1;", AssignOp::ExpAssign),
        ("x &&= 1;", AssignOp::LogAndAssign),
        ("x ||= 1;", AssignOp::LogOrAssign),
        ("x ??= 1;", AssignOp::NullCoalAssign),
    ];
    for (src, expected_op) in cases {
        let expr = first_expr(src);
        match expr.kind {
            ExprKind::Assign(a) => assert_eq!(a.op, expected_op, "failed for: {}", src),
            _ => panic!("expected assign for: {}", src),
        }
    }
}

// =======================================================================
// Comma expression
// =======================================================================

#[test]
fn comma_expression() {
    let expr = first_expr("a, b, c;");
    match expr.kind {
        ExprKind::Comma(parts) => assert_eq!(parts.len(), 3),
        _ => panic!("expected comma"),
    }
}

// =======================================================================
// Template literals
// =======================================================================

#[test]
fn no_substitution_template() {
    let expr = first_expr("`hello`;");
    assert!(matches!(expr.kind, ExprKind::NoSubstTemplate(_)));
}

// =======================================================================
// Spread expression
// =======================================================================

#[test]
fn spread_in_array() {
    let expr = first_expr("[...arr];");
    match expr.kind {
        ExprKind::ArrayLit(elems) => {
            assert_eq!(elems.len(), 1);
            let elem = elems[0].as_ref().unwrap();
            assert!(matches!(elem.kind, ExprKind::Spread(_)));
        }
        _ => panic!("expected array"),
    }
}

// =======================================================================
// Yield expression
// =======================================================================

#[test]
fn yield_expression() {
    let expr = first_expr("yield 1;");
    assert!(matches!(expr.kind, ExprKind::Yield(false, Some(_))));
}

#[test]
fn yield_delegate() {
    let expr = first_expr("yield* gen();");
    assert!(matches!(expr.kind, ExprKind::Yield(true, Some(_))));
}

// =======================================================================
// Literals
// =======================================================================

#[test]
fn literals() {
    assert!(matches!(first_expr("42;").kind, ExprKind::NumLit(_)));
    assert!(matches!(first_expr("'hello';").kind, ExprKind::StrLit(_)));
    assert!(matches!(first_expr("true;").kind, ExprKind::BoolLit(true)));
    assert!(matches!(
        first_expr("false;").kind,
        ExprKind::BoolLit(false)
    ));
    assert!(matches!(first_expr("null;").kind, ExprKind::NullLit));
    assert!(matches!(first_expr("this;").kind, ExprKind::This));
    assert!(matches!(first_expr("super;").kind, ExprKind::Super));
}

// =======================================================================
// Object literals
// =======================================================================

#[test]
fn object_literal() {
    let expr = first_expr("({ a: 1, b, ...c });");
    match expr.kind {
        ExprKind::Paren(inner) => match inner.kind {
            ExprKind::ObjectLit(props) => assert_eq!(props.len(), 3),
            _ => panic!("expected object"),
        },
        _ => panic!("expected paren"),
    }
}

#[test]
fn object_method_shorthand() {
    let expr = first_expr("({ foo() {} });");
    match expr.kind {
        ExprKind::Paren(inner) => match inner.kind {
            ExprKind::ObjectLit(props) => {
                assert!(matches!(props[0], ObjLitProp::Method(_)));
            }
            _ => panic!("expected object"),
        },
        _ => panic!("expected paren"),
    }
}

// =======================================================================
// Arrow functions
// =======================================================================

#[test]
fn arrow_simple() {
    let expr = first_expr("x => x + 1;");
    match expr.kind {
        ExprKind::Arrow(a) => {
            assert_eq!(a.params.len(), 1);
            assert!(!a.is_async);
            assert!(matches!(a.body, ArrowBody::Expr(_)));
        }
        _ => panic!("expected arrow"),
    }
}

#[test]
fn arrow_with_parens() {
    let expr = first_expr("(x, y) => x + y;");
    match expr.kind {
        ExprKind::Arrow(a) => assert_eq!(a.params.len(), 2),
        _ => panic!("expected arrow"),
    }
}

#[test]
fn arrow_with_union_type_annotation() {
    let expr = first_expr("(x: typeof a | typeof b) => x;");
    match expr.kind {
        ExprKind::Arrow(a) => {
            assert_eq!(a.params.len(), 1);
            assert!(a.params[0].type_ann.is_some());
            assert!(matches!(a.body, ArrowBody::Expr(_)));
        }
        _ => panic!("expected arrow"),
    }
}

#[test]
fn arrow_with_qualified_type_and_multiple_params() {
    let expr = first_expr("(x: ns.T, y: ns.U) => x;");
    match expr.kind {
        ExprKind::Arrow(a) => {
            assert_eq!(a.params.len(), 2);
            assert!(a.params.iter().all(|param| param.type_ann.is_some()));
            assert!(matches!(a.body, ArrowBody::Expr(_)));
        }
        _ => panic!("expected arrow"),
    }
}

#[test]
fn class_method_with_nested_generic_constraint() {
    let sf = parse_src(include_str!(
        "../../../tests/cases/compiler/assertInWrapSomeTypeParameter.ts"
    ));
    assert!(sf.diagnostics.is_empty(), "{:?}", sf.diagnostics);
    let StmtKind::ClassDecl(class) = &sf.statements[0].kind else {
        panic!("expected class declaration");
    };
    assert_eq!(class.members.len(), 1);
    assert!(matches!(class.members[0].kind, ClassMemberKind::Method(_)));
}

#[test]
fn class_expression_extends_nested_intersection_type_arguments() {
    let type_only = parse_src("type X = Component<CoachMarkAnchorProps<AnchorType<P>> & P, {}>;");
    assert!(
        type_only.diagnostics.is_empty(),
        "{:?}",
        type_only.diagnostics
    );
    let sf = parse_src(include_str!(
        "../../../tests/cases/compiler/thisIndexOnExistingReadonlyFieldIsNotNever.ts"
    ));
    assert!(sf.diagnostics.is_empty(), "{:?}", sf.diagnostics);
}

#[test]
fn arrow_block_body() {
    let expr = first_expr("(x) => { return x; };");
    match expr.kind {
        ExprKind::Arrow(a) => assert!(matches!(a.body, ArrowBody::Block(_))),
        _ => panic!("expected arrow"),
    }
}

#[test]
fn async_arrow() {
    let expr = first_expr("async x => x;");
    match expr.kind {
        ExprKind::Arrow(a) => assert!(a.is_async),
        _ => panic!("expected arrow"),
    }
}

#[test]
fn async_identifier_call_is_not_an_arrow() {
    let expr = first_expr("async(() => await(Promise.resolve(1)));");
    assert!(matches!(expr.kind, ExprKind::Call(_)));
}

#[test]
fn async_identifier_instantiation_call_is_not_an_arrow() {
    let expr = first_expr("async<number>();");
    assert!(matches!(expr.kind, ExprKind::Call(_)));
}

#[test]
fn async_generic_arrow_with_union_callback_return_type() {
    let expr = first_expr(
        "async <U, R, S>(com: () => Iterator<S, U, R> | AsyncIterator<S, U, R>): Promise<U> => { throw com; };",
    );
    let ExprKind::Arrow(arrow) = expr.kind else {
        panic!("expected async generic arrow");
    };
    assert!(arrow.is_async);
    assert_eq!(arrow.type_params.as_ref().map(Vec::len), Some(3));
    assert_eq!(arrow.params.len(), 1);
}

#[test]
fn tsx_generic_arrow_disambiguation_matches_jsx_grammar() {
    let source = r#"
        var x1 = <T>() => {}</T>;
        var x2 = <T extends {}>() => {};
        var x3 = <T, U>() => {};
        var x4 = <T extends={true}>() => {}</T>;
        var x5 = <T extends>() => {}</T>;
        var x6 = <T = string,>() => {};
    "#;
    let result = parse_with_jsx("file.tsx", source, true);
    let initializers: Vec<&Expr> = result
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::Var(var_stmt) => var_stmt
                .declarations
                .first()
                .and_then(|decl| decl.init.as_deref()),
            _ => None,
        })
        .collect();
    assert_eq!(initializers.len(), 6);
    assert!(matches!(initializers[0].kind, ExprKind::JsxElement(_)));
    assert!(matches!(initializers[1].kind, ExprKind::Arrow(_)));
    assert!(matches!(initializers[2].kind, ExprKind::Arrow(_)));
    assert!(matches!(initializers[3].kind, ExprKind::JsxElement(_)));
    assert!(matches!(initializers[4].kind, ExprKind::JsxElement(_)));
    assert!(matches!(initializers[5].kind, ExprKind::Arrow(_)));
}

#[test]
fn arrow_missing_open_brace_recovers_as_block_body() {
    let sf = parse_src("namespace N { var a = () => var k = 10;}; var b = () => var k = 10;}; }");
    let module = match &sf.statements[0].kind {
        StmtKind::ModuleDecl(m) => m,
        other => panic!("expected namespace, got {:?}", other),
    };
    let body = match module.body.as_ref() {
        Some(ModuleBody::Block(stmts)) => stmts,
        other => panic!("expected namespace block body, got {:?}", other),
    };

    assert_eq!(
        body.len(),
        2,
        "both malformed declarations should stay inside the namespace"
    );
    for stmt in body {
        let var_stmt = match &stmt.kind {
            StmtKind::Var(v) => v,
            other => panic!("expected var statement, got {:?}", other),
        };
        let init = var_stmt.declarations[0]
            .init
            .as_ref()
            .expect("expected initializer");
        let arrow = match &init.kind {
            ExprKind::Arrow(a) => a,
            other => panic!("expected arrow initializer, got {:?}", other),
        };
        assert!(
            matches!(&arrow.body, ArrowBody::Block(_)),
            "recovered body should be a block"
        );
    }
}

#[test]
fn arrow_missing_body_does_not_consume_outer_close_brace() {
    let sf = parse_src("namespace N { var a = () => }; var b = () => }; }");
    let module = match &sf.statements[0].kind {
        StmtKind::ModuleDecl(m) => m,
        other => panic!("expected namespace, got {:?}", other),
    };
    let body = match module.body.as_ref() {
        Some(ModuleBody::Block(stmts)) => stmts,
        other => panic!("expected namespace block body, got {:?}", other),
    };

    assert_eq!(
        body.len(),
        1,
        "first `}}` should close namespace when body is omitted"
    );
    let init = match &body[0].kind {
        StmtKind::Var(v) => v.declarations[0]
            .init
            .as_ref()
            .expect("expected initializer"),
        other => panic!("expected var statement, got {:?}", other),
    };
    let arrow = match &init.kind {
        ExprKind::Arrow(a) => a,
        other => panic!("expected arrow initializer, got {:?}", other),
    };
    assert!(
        matches!(&arrow.body, ArrowBody::Expr(expr) if matches!(expr.kind, ExprKind::Omitted)),
        "missing body should recover to omitted expression"
    );
    assert!(
        sf.statements
            .iter()
            .skip(1)
            .any(|stmt| matches!(stmt.kind, StmtKind::Var(_))),
        "subsequent declarations should parse after the namespace closes"
    );
    assert!(
        sf.statements
            .iter()
            .skip(1)
            .any(|stmt| matches!(stmt.kind, StmtKind::Empty)),
        "semicolon after recovered close brace should remain as an empty statement"
    );
}

#[test]
fn arrow_missing_fat_arrow_before_block_recovers() {
    let sf = parse_src("var a = () { }; var b = (x) { }; var c = (): void { }");
    let stmts = &sf.statements;
    assert_eq!(stmts.len(), 3);
    for stmt in stmts {
        let init = match &stmt.kind {
            StmtKind::Var(v) => v.declarations[0]
                .init
                .as_ref()
                .expect("expected initializer"),
            other => panic!("expected var statement, got {:?}", other),
        };
        let arrow = match &init.kind {
            ExprKind::Arrow(a) => a,
            other => panic!("expected arrow initializer, got {:?}", other),
        };
        assert!(
            matches!(&arrow.body, ArrowBody::Block(_)),
            "missing fat-arrow before block should recover as block body"
        );
    }
}

#[test]
fn typed_arrow_missing_fat_arrow_recovers_as_omitted_body() {
    let sf = parse_src("var a = (): void; var b = (x: number, y: string);");
    let stmts = &sf.statements;
    assert_eq!(stmts.len(), 2);
    for stmt in stmts {
        let init = match &stmt.kind {
            StmtKind::Var(v) => v.declarations[0]
                .init
                .as_ref()
                .expect("expected initializer"),
            other => panic!("expected var statement, got {:?}", other),
        };
        let arrow = match &init.kind {
            ExprKind::Arrow(a) => a,
            other => panic!("expected arrow initializer, got {:?}", other),
        };
        assert!(
            matches!(&arrow.body, ArrowBody::Expr(expr) if matches!(expr.kind, ExprKind::Omitted)),
            "typed arrow missing fat-arrow should recover as omitted body"
        );
    }
}

#[test]
fn conditional_with_parenthesized_arrow_consequent_stays_conditional() {
    let expr = first_expr("false ? (() => 51) : null;");
    let ExprKind::Cond(cond) = expr.kind else {
        panic!("expected conditional expression");
    };
    let ExprKind::Paren(inner) = &cond.consequent.kind else {
        panic!(
            "expected parenthesized arrow consequent, got {:?}",
            cond.consequent.kind
        );
    };
    assert!(
        matches!(inner.kind, ExprKind::Arrow(_)),
        "expected consequent to remain a parenthesized arrow, got {:?}",
        inner.kind
    );
}

#[test]
fn invalid_conditional_tail_after_block_arrow_splits_into_expression_statements() {
    let sf = parse_src("(a?) => { return a; } ? (b)=>(c)=>81 : (c)=>(d)=>82;");
    assert_eq!(
        sf.statements.len(),
        3,
        "expected recovered invalid conditional tail to split into three statements"
    );
    for (index, stmt) in sf.statements.iter().enumerate() {
        let StmtKind::Expr(expr) = &stmt.kind else {
            panic!(
                "expected expression statement at index {index}, got {:?}",
                stmt.kind
            );
        };
        assert!(
            matches!(expr.kind, ExprKind::Arrow(_)),
            "expected recovered statement {index} to remain an arrow expression, got {:?}",
            expr.kind
        );
    }
}

#[test]
fn parenthesized_conditional_inside_object_spread_does_not_recover_as_arrow() {
    let stmt = first_stmt("const obj = { ...(param2 ? { param2 } : {}) };");
    let StmtKind::Var(var_stmt) = stmt.kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected object literal initializer");
    let ExprKind::ObjectLit(props) = &init.kind else {
        panic!("expected object literal initializer, got {:?}", init.kind);
    };
    let Some(ObjLitProp::Spread(spread, _)) = props.first() else {
        panic!("expected object spread property");
    };
    let ExprKind::Paren(inner) = &spread.kind else {
        panic!(
            "expected parenthesized spread expression, got {:?}",
            spread.kind
        );
    };
    assert!(
        matches!(inner.kind, ExprKind::Cond(_)),
        "expected spread expression to remain a conditional, got {:?}",
        inner.kind
    );
}

#[test]
fn comma_terminated_object_call_signature_does_not_consume_following_statements() {
    let sf = parse_src("var b = { foo(x = 1), foo(x = 1) { }, }; b.foo(); b.foo(1);");
    assert_eq!(sf.statements.len(), 3);

    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected object literal initializer");
    let ExprKind::ObjectLit(props) = &init.kind else {
        panic!("expected object literal initializer, got {:?}", init.kind);
    };
    assert_eq!(props.len(), 2);
    assert!(props
        .iter()
        .all(|prop| matches!(prop, ObjLitProp::Method(_))));
    assert!(matches!(sf.statements[1].kind, StmtKind::Expr(_)));
    assert!(matches!(sf.statements[2].kind, StmtKind::Expr(_)));
}

#[test]
fn comma_terminated_object_accessors_do_not_consume_following_properties() {
    let sf = parse_src("var value = { get first, set second, third, fourth };");
    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected object literal initializer");
    let ExprKind::ObjectLit(props) = &init.kind else {
        panic!("expected object literal initializer, got {:?}", init.kind);
    };
    assert_eq!(props.len(), 4);
    assert!(matches!(props[0], ObjLitProp::Get(_)));
    assert!(matches!(props[1], ObjLitProp::Set(_)));
    assert!(matches!(props[2], ObjLitProp::Shorthand(_, _)));
    assert!(matches!(props[3], ObjLitProp::Shorthand(_, _)));
}

// =======================================================================
// As / satisfies / non-null assertions
// =======================================================================

#[test]
fn as_expression() {
    let expr = first_expr("x as string;");
    assert!(matches!(expr.kind, ExprKind::As(_)));
}

#[test]
fn as_const() {
    let expr = first_expr("x as const;");
    assert!(matches!(expr.kind, ExprKind::As(_)));
}

#[test]
fn satisfies_expression() {
    let expr = first_expr("x satisfies T;");
    assert!(matches!(expr.kind, ExprKind::Satisfies(_)));
}

#[test]
fn non_null_assertion() {
    let expr = first_expr("x!;");
    assert!(matches!(expr.kind, ExprKind::NonNull(_)));
}

// =======================================================================
// Function declaration
// =======================================================================

#[test]
fn function_declaration() {
    let stmt = first_stmt("function foo(x: number): string { return ''; }");
    match stmt.kind {
        StmtKind::FnDecl(f) => {
            assert_eq!(f.name.as_deref(), Some("foo"));
            assert_eq!(f.params.len(), 1);
            assert!(f.return_type.is_some());
            assert!(f.body.is_some());
        }
        _ => panic!("expected fn decl"),
    }
}

#[test]
fn async_function() {
    let stmt = first_stmt("async function fetchData() { }");
    match stmt.kind {
        StmtKind::FnDecl(f) => assert!(f.is_async),
        _ => panic!("expected fn decl"),
    }
}

#[test]
fn generator_function() {
    let stmt = first_stmt("function* gen() { yield 1; }");
    match stmt.kind {
        StmtKind::FnDecl(f) => assert!(f.is_generator),
        _ => panic!("expected fn decl"),
    }
}

// =======================================================================
// Class declarations
// =======================================================================

#[test]
fn class_declaration() {
    let stmt = first_stmt("class Foo extends Bar implements Baz { }");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert_eq!(c.name.as_deref(), Some("Foo"));
            assert!(c.extends.is_some());
            assert_eq!(c.implements.len(), 1);
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn class_with_members() {
    let src = r#"
    class Foo {
        x: number;
        private y = 1;
        constructor(public z: number) {}
        method() {}
        get prop() { return 1; }
        set prop(v: number) {}
        static staticMethod() {}
    }
    "#;
    let stmt = first_stmt(src);
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert!(
                c.members.len() >= 7,
                "expected 7+ members, got {}",
                c.members.len()
            );
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn abstract_class() {
    let stmt = first_stmt("abstract class Base { abstract method(): void; }");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert!(c.modifiers & MOD_ABSTRACT != 0);
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn class_static_block() {
    let stmt = first_stmt("class Foo { static { console.log('init'); } }");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert!(c
                .members
                .iter()
                .any(|m| matches!(m.kind, ClassMemberKind::StaticBlock(_))));
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn class_private_field() {
    let stmt = first_stmt("class Foo { #name: string; #getInfo() { return this.#name; } }");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert!(c.members.len() >= 2);
            match &c.members[0].kind {
                ClassMemberKind::Property(p) => {
                    assert!(matches!(p.name, PropName::Private(_, _)));
                }
                _ => panic!("expected private property"),
            }
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn class_private_name_missing_identifier_recovers_as_empty_name() {
    let sf = parse_src(
        r#"
#

class C {
    #

    m() {
        this.#
    }
}
"#,
    );
    assert!(
        !sf.diagnostics.is_empty(),
        "expected recovery diagnostics for malformed private names"
    );
    assert!(
        matches!(&sf.statements[0].kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::Ident(name) if name == "#")),
        "expected leading lone `#` to recover as an identifier expression"
    );
    let StmtKind::ClassDecl(class_decl) = &sf.statements[1].kind else {
        panic!("expected recovered class declaration");
    };
    match &class_decl.members[0].kind {
        ClassMemberKind::Property(prop) => {
            assert!(
                matches!(&prop.name, PropName::Private(name, _) if name.is_empty()),
                "expected class member `#` to recover as an empty private name"
            );
        }
        other => panic!("expected recovered private property, got {:?}", other),
    }
    let ClassMemberKind::Method(method) = &class_decl.members[1].kind else {
        panic!("expected method member");
    };
    let body = method
        .body
        .as_ref()
        .expect("expected recovered method body");
    let StmtKind::Expr(expr_stmt) = &body[0].kind else {
        panic!("expected expression statement in method body");
    };
    let ExprKind::Member(member) = &expr_stmt.kind else {
        panic!("expected private member access in method body");
    };
    assert_eq!(member.property.as_str(), "#");
}

#[test]
fn class_with_generics() {
    let stmt = first_stmt("class Container<T extends Serializable = any> { value: T; }");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            let tp = c.type_params.unwrap();
            assert_eq!(tp.len(), 1);
            assert_eq!(tp[0].name, "T");
            assert!(tp[0].constraint.is_some());
            assert!(tp[0].default.is_some());
        }
        _ => panic!("expected class"),
    }
}

// =======================================================================
// Interface declarations
// =======================================================================

#[test]
fn interface_declaration() {
    let src = r#"
    interface Foo extends Bar {
        x: number;
        y?: string;
        method(a: number): void;
        readonly z: boolean;
        [key: string]: any;
    }
    "#;
    let stmt = first_stmt(src);
    match stmt.kind {
        StmtKind::InterfaceDecl(i) => {
            assert_eq!(i.name, "Foo");
            assert_eq!(i.extends.len(), 1);
            assert!(i.members.len() >= 5);
        }
        _ => panic!("expected interface"),
    }
}

#[test]
fn interface_with_call_signature() {
    let stmt = first_stmt("interface Callable { (x: number): string; }");
    match stmt.kind {
        StmtKind::InterfaceDecl(i) => {
            assert!(matches!(i.members[0].kind, TypeMemberKind::CallSig(_)));
        }
        _ => panic!("expected interface"),
    }
}

#[test]
fn interface_with_construct_signature() {
    let stmt = first_stmt("interface Newable { new (x: number): Foo; }");
    match stmt.kind {
        StmtKind::InterfaceDecl(i) => {
            assert!(matches!(i.members[0].kind, TypeMemberKind::ConstructSig(_)));
        }
        _ => panic!("expected interface"),
    }
}

#[test]
fn new_keyword_is_a_property_name_before_colon() {
    let src = r#"
interface DerivedTable<S extends { base: any; new: any }> {
    schema: S["base"] & S["new"];
}

declare const source: DerivedTable<{ base: Base; new: New }>;
"#;

    assert_no_errors(src);
}

#[test]
fn generic_construct_signature_still_parses() {
    let src = "interface Newable { new <T>(value: T): Box<T>; }";
    let sf = parse_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let StmtKind::InterfaceDecl(interface) = &sf.statements[0].kind else {
        panic!("expected interface");
    };
    assert!(matches!(
        interface.members[0].kind,
        TypeMemberKind::ConstructSig(_)
    ));
}

// =======================================================================
// Type alias
// =======================================================================

#[test]
fn type_alias() {
    let stmt = first_stmt("type StringOrNumber = string | number;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert_eq!(t.name, "StringOrNumber");
            assert!(matches!(t.type_ann.kind, TypeNodeKind::Union(_)));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_alias_with_generics() {
    let stmt = first_stmt("type Result<T, E = Error> = { ok: T } | { err: E };");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            let tp = t.type_params.unwrap();
            assert_eq!(tp.len(), 2);
            assert!(tp[1].default.is_some());
        }
        _ => panic!("expected type alias"),
    }
}

// =======================================================================
// Enum declarations
// =======================================================================

#[test]
fn enum_declaration() {
    let stmt = first_stmt("enum Color { Red, Green = 1, Blue }");
    match stmt.kind {
        StmtKind::EnumDecl(e) => {
            assert_eq!(e.name, "Color");
            assert_eq!(e.members.len(), 3);
            assert!(!e.is_const);
        }
        _ => panic!("expected enum"),
    }
}

#[test]
fn const_enum() {
    let stmt = first_stmt("const enum Direction { Up, Down }");
    match stmt.kind {
        StmtKind::EnumDecl(e) => {
            assert!(e.is_const);
        }
        _ => panic!("expected enum"),
    }
}

// =======================================================================
// Module / namespace declarations
// =======================================================================

#[test]
fn namespace_declaration() {
    let stmt = first_stmt("namespace MyLib { export function foo() {} }");
    match stmt.kind {
        StmtKind::ModuleDecl(m) => {
            assert!(matches!(m.name, ModuleName::Ident(ref n) if n == "MyLib"));
            assert!(m.body.is_some());
        }
        _ => panic!("expected module"),
    }
}

#[test]
fn module_string_name() {
    let stmt = first_stmt("module 'my-module' { }");
    match stmt.kind {
        StmtKind::ModuleDecl(m) => {
            assert!(matches!(m.name, ModuleName::String(ref s) if s == "my-module"));
        }
        _ => panic!("expected module"),
    }
}

// =======================================================================
// Import declarations
// =======================================================================

#[test]
fn import_default() {
    let stmt = first_stmt("import Foo from 'foo';");
    match stmt.kind {
        StmtKind::Import(i) => {
            assert!(!i.type_only);
            match i.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    assert_eq!(default, Some("Foo".into()));
                    assert!(named.is_empty());
                    assert!(namespace.is_none());
                }
                _ => panic!("expected named clause"),
            }
        }
        _ => panic!("expected import"),
    }
}

#[test]
fn import_named() {
    let stmt = first_stmt("import { A, B as C } from 'mod';");
    match stmt.kind {
        StmtKind::Import(i) => match i.specifiers {
            ImportClause::Named { named, .. } => {
                assert_eq!(named.len(), 2);
                assert_eq!(named[0].local, "A");
                assert_eq!(named[1].local, "C");
                assert_eq!(named[1].imported, Some("B".into()));
            }
            _ => panic!("expected named"),
        },
        _ => panic!("expected import"),
    }
}

#[test]
fn import_named_string_literal_module_name() {
    let stmt = first_stmt("import { \"0n\" as foo } from './mod';");
    match stmt.kind {
        StmtKind::Import(i) => match i.specifiers {
            ImportClause::Named { named, .. } => {
                assert_eq!(i.source, "./mod");
                assert_eq!(named.len(), 1);
                assert_eq!(named[0].local, "foo");
                assert_eq!(named[0].imported.as_deref(), Some("0n"));
            }
            _ => panic!("expected named"),
        },
        _ => panic!("expected import"),
    }
}

#[test]
fn import_named_bigint_recovery_keeps_source() {
    let stmt = first_stmt("import { 0n as foo } from './mod';");
    match stmt.kind {
        StmtKind::Import(i) => match i.specifiers {
            ImportClause::Named { named, .. } => {
                assert_eq!(i.source, "./mod");
                assert_eq!(named.len(), 1);
                assert_eq!(named[0].local, "<error>");
            }
            _ => panic!("expected named"),
        },
        _ => panic!("expected import"),
    }
}

#[test]
fn import_named_invalid_unicode_escape_local_recovery_keeps_source() {
    let sf = parse_src(r#"import { foo as \uD800\uDEA7 } from "./mod";"#);
    assert_eq!(
        sf.statements.len(),
        1,
        "malformed unicode local name should stay inside the import: {:?}",
        sf.statements.iter().map(|s| &s.kind).collect::<Vec<_>>()
    );
    match &sf.statements[0].kind {
        StmtKind::Import(i) => match &i.specifiers {
            ImportClause::Named { named, .. } => {
                assert_eq!(i.source, "./mod");
                assert_eq!(named.len(), 1);
                assert_eq!(named[0].local, "<error>");
                assert_eq!(named[0].imported.as_deref(), Some("foo"));
            }
            _ => panic!("expected named"),
        },
        _ => panic!("expected import"),
    }
}

#[test]
fn import_namespace() {
    let stmt = first_stmt("import * as ns from 'mod';");
    match stmt.kind {
        StmtKind::Import(i) => match i.specifiers {
            ImportClause::Named { namespace, .. } => {
                assert_eq!(namespace, Some("ns".into()));
            }
            _ => panic!("expected named"),
        },
        _ => panic!("expected import"),
    }
}

#[test]
fn import_side_effect() {
    let stmt = first_stmt("import 'polyfill';");
    match stmt.kind {
        StmtKind::Import(i) => {
            assert_eq!(i.source, "polyfill");
        }
        _ => panic!("expected import"),
    }
}

#[test]
fn import_type_named() {
    let stmt = first_stmt("import type { Foo } from 'mod';");
    match stmt.kind {
        StmtKind::Import(i) => {
            assert!(i.type_only);
            match i.specifiers {
                ImportClause::Named { named, .. } => {
                    assert_eq!(named.len(), 1);
                }
                _ => panic!("expected named"),
            }
        }
        _ => panic!("expected import"),
    }
}

#[test]
fn import_type_default() {
    let stmt = first_stmt("import type Foo from 'mod';");
    match stmt.kind {
        StmtKind::Import(i) => {
            assert!(i.type_only);
            match i.specifiers {
                ImportClause::Named { default, .. } => {
                    assert_eq!(default, Some("Foo".into()));
                }
                _ => panic!("expected named"),
            }
        }
        _ => panic!("expected import"),
    }
}

#[test]
fn import_type_namespace() {
    let stmt = first_stmt("import type * as ns from 'mod';");
    match stmt.kind {
        StmtKind::Import(i) => {
            assert!(i.type_only);
            match i.specifiers {
                ImportClause::Named { namespace, .. } => {
                    assert_eq!(namespace, Some("ns".into()));
                }
                _ => panic!("expected named"),
            }
        }
        _ => panic!("expected import"),
    }
}

// =======================================================================
// Export declarations
// =======================================================================

#[test]
fn export_named() {
    let stmt = first_stmt("export { foo, bar as baz };");
    match stmt.kind {
        StmtKind::Export(e) => match e.kind {
            ExportDeclKind::Named {
                specifiers,
                source,
                type_only,
            } => {
                assert_eq!(specifiers.len(), 2);
                assert!(source.is_none());
                assert!(!type_only);
            }
            _ => panic!("expected named export"),
        },
        _ => panic!("expected export"),
    }
}

#[test]
fn export_named_string_literal_module_name() {
    let stmt = first_stmt("export { foo as \"0n\" };");
    match stmt.kind {
        StmtKind::Export(e) => match e.kind {
            ExportDeclKind::Named { specifiers, .. } => {
                assert_eq!(specifiers.len(), 1);
                assert_eq!(specifiers[0].local, "foo");
                assert_eq!(specifiers[0].exported.as_deref(), Some("0n"));
            }
            _ => panic!("expected named export"),
        },
        _ => panic!("expected export"),
    }
}

#[test]
fn export_named_bigint_alias_recovery_keeps_as_placeholder() {
    let stmt = first_stmt("export { foo as 0n };");
    match stmt.kind {
        StmtKind::Export(e) => match e.kind {
            ExportDeclKind::Named { specifiers, .. } => {
                assert_eq!(specifiers.len(), 1);
                assert_eq!(specifiers[0].local, "foo");
                assert_eq!(specifiers[0].exported.as_deref(), Some(""));
            }
            _ => panic!("expected named export"),
        },
        _ => panic!("expected export"),
    }
}

#[test]
fn bigint_arbitrary_identifier_multifile_parses_bad_imports_as_imports() {
    let case_src = include_str!("../../../tests/cases/compiler/bigintArbirtraryIdentifier.ts");
    let tc = tsc_rs_ast::parse_test_case("bigintArbirtraryIdentifier.ts", case_src);

    let bad_import = tc
        .files
        .iter()
        .find(|f| f.name == "badImport.ts")
        .expect("missing badImport.ts section");
    let bad_import2 = tc
        .files
        .iter()
        .find(|f| f.name == "badImport2.ts")
        .expect("missing badImport2.ts section");

    let sf1 = parse("badImport.ts", &bad_import.content);
    let sf2 = parse("badImport2.ts", &bad_import2.content);

    assert!(
        matches!(
            sf1.statements.first().map(|s| &s.kind),
            Some(StmtKind::Import(_))
        ),
        "badImport first stmt was: {:?}",
        sf1.statements.first().map(|s| &s.kind)
    );
    assert!(
        matches!(
            sf2.statements.first().map(|s| &s.kind),
            Some(StmtKind::Import(_))
        ),
        "badImport2 first stmt was: {:?}",
        sf2.statements.first().map(|s| &s.kind)
    );
}

#[test]
fn export_default_expression() {
    let stmt = first_stmt("export default 42;");
    match stmt.kind {
        StmtKind::Export(e) => {
            assert!(matches!(e.kind, ExportDeclKind::Default(_)));
        }
        _ => panic!("expected export"),
    }
}

#[test]
fn export_default_function() {
    let stmt = first_stmt("export default function foo() {}");
    match stmt.kind {
        StmtKind::Export(e) => {
            assert!(matches!(e.kind, ExportDeclKind::DefaultDecl(_)));
        }
        _ => panic!("expected export"),
    }
}

#[test]
fn export_all() {
    let stmt = first_stmt("export * from 'mod';");
    match stmt.kind {
        StmtKind::Export(e) => {
            assert!(matches!(e.kind, ExportDeclKind::All { .. }));
        }
        _ => panic!("expected export"),
    }
}

#[test]
fn export_all_as() {
    let stmt = first_stmt("export * as ns from 'mod';");
    match stmt.kind {
        StmtKind::Export(e) => match e.kind {
            ExportDeclKind::All { alias, .. } => {
                assert_eq!(alias, Some("ns".into()));
            }
            _ => panic!("expected all export"),
        },
        _ => panic!("expected export"),
    }
}

#[test]
fn export_type_named() {
    let stmt = first_stmt("export type { Foo, Bar };");
    match stmt.kind {
        StmtKind::Export(e) => match e.kind {
            ExportDeclKind::Named { type_only, .. } => assert!(type_only),
            _ => panic!("expected named export"),
        },
        _ => panic!("expected export"),
    }
}

#[test]
fn export_declaration() {
    let stmt = first_stmt("export const x = 1;");
    match stmt.kind {
        StmtKind::Export(e) => {
            assert!(matches!(e.kind, ExportDeclKind::Decl(_)));
        }
        _ => panic!("expected export"),
    }
}

#[test]
fn export_assign() {
    let stmt = first_stmt("export = myModule;");
    assert!(matches!(stmt.kind, StmtKind::ExportAssign(_)));
}

// =======================================================================
// Type annotations
// =======================================================================

#[test]
fn type_keyword_types() {
    let cases = vec![
        ("let x: number;", KeywordTypeKind::Number),
        ("let x: string;", KeywordTypeKind::String),
        ("let x: boolean;", KeywordTypeKind::Boolean),
        ("let x: any;", KeywordTypeKind::Any),
        ("let x: void;", KeywordTypeKind::Void),
        ("let x: never;", KeywordTypeKind::Never),
        ("let x: unknown;", KeywordTypeKind::Unknown),
        ("let x: undefined;", KeywordTypeKind::Undefined),
        ("let x: null;", KeywordTypeKind::Null),
        ("let x: object;", KeywordTypeKind::Object),
        ("let x: symbol;", KeywordTypeKind::Symbol),
        ("let x: bigint;", KeywordTypeKind::BigInt),
    ];
    for (src, expected) in cases {
        let stmt = first_stmt(src);
        match stmt.kind {
            StmtKind::Var(v) => match &v.declarations[0].type_ann {
                Some(t) => assert!(
                    matches!(t.kind, TypeNodeKind::Keyword(k) if k == expected),
                    "failed for {}",
                    src
                ),
                None => panic!("no type annotation for {}", src),
            },
            _ => panic!("expected var for {}", src),
        }
    }
}

#[test]
fn type_reference() {
    let stmt = first_stmt("let x: Foo;");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            assert!(matches!(ty.kind, TypeNodeKind::Reference(_)));
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_reference_with_args() {
    let stmt = first_stmt("let x: Map<string, number>;");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            match &ty.kind {
                TypeNodeKind::Reference(r) => {
                    assert!(r.type_args.is_some());
                    assert_eq!(r.type_args.as_ref().unwrap().len(), 2);
                }
                _ => panic!("expected reference"),
            }
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_array() {
    let stmt = first_stmt("let x: number[];");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            assert!(matches!(ty.kind, TypeNodeKind::Array(_)));
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_tuple() {
    let stmt = first_stmt("let x: [string, number, boolean];");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            match &ty.kind {
                TypeNodeKind::Tuple(elems) => assert_eq!(elems.len(), 3),
                _ => panic!("expected tuple"),
            }
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_union() {
    let stmt = first_stmt("let x: string | number | boolean;");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            match &ty.kind {
                TypeNodeKind::Union(types) => assert_eq!(types.len(), 3),
                _ => panic!("expected union"),
            }
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_intersection() {
    let stmt = first_stmt("let x: A & B & C;");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            match &ty.kind {
                TypeNodeKind::Intersection(types) => assert_eq!(types.len(), 3),
                _ => panic!("expected intersection"),
            }
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_function() {
    let stmt = first_stmt("let x: (a: number, b: string) => boolean;");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            match &ty.kind {
                TypeNodeKind::Function(f) => {
                    assert_eq!(f.params.len(), 2);
                }
                _ => panic!("expected function type, got {:?}", ty.kind),
            }
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_conditional() {
    let stmt = first_stmt("type X = T extends string ? true : false;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(t.type_ann.kind, TypeNodeKind::Conditional(_)));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_mapped() {
    let stmt = first_stmt("type Partial<T> = { [K in keyof T]?: T[K] };");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(t.type_ann.kind, TypeNodeKind::Mapped(_)));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_indexed_access() {
    let stmt = first_stmt("type X = Foo['bar'];");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(t.type_ann.kind, TypeNodeKind::IndexedAccess(_, _)));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_keyof() {
    let stmt = first_stmt("type K = keyof T;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(t.type_ann.kind, TypeNodeKind::Keyof(_)));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_typeof() {
    let stmt = first_stmt("let x: typeof y;");
    match stmt.kind {
        StmtKind::Var(v) => {
            let ty = v.declarations[0].type_ann.as_ref().unwrap();
            assert!(matches!(ty.kind, TypeNodeKind::TypeQuery(_)));
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn type_infer() {
    let stmt = first_stmt("type ReturnType<T> = T extends (...args: any) => infer R ? R : never;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(t.type_ann.kind, TypeNodeKind::Conditional(_)));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_literal() {
    let stmt = first_stmt("type X = { a: number; b: string };");
    match stmt.kind {
        StmtKind::TypeAlias(t) => match &t.type_ann.kind {
            TypeNodeKind::TypeLit(members) => assert_eq!(members.len(), 2),
            _ => panic!("expected type lit"),
        },
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_constructor() {
    let stmt = first_stmt("type T = new (x: number) => Foo;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(t.type_ann.kind, TypeNodeKind::Constructor(_)));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_literal_string() {
    let stmt = first_stmt("type X = 'hello';");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(
                t.type_ann.kind,
                TypeNodeKind::Literal(LiteralTypeKind::String(_))
            ));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_literal_number() {
    let stmt = first_stmt("type X = 42;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(
                t.type_ann.kind,
                TypeNodeKind::Literal(LiteralTypeKind::Number(_))
            ));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_literal_boolean() {
    let stmt = first_stmt("type X = true;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(
                t.type_ann.kind,
                TypeNodeKind::Literal(LiteralTypeKind::Boolean(true))
            ));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_this() {
    let stmt = first_stmt("type X = this;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => {
            assert!(matches!(t.type_ann.kind, TypeNodeKind::This));
        }
        _ => panic!("expected type alias"),
    }
}

#[test]
fn type_generic_function() {
    let stmt = first_stmt("type X = <T>(x: T) => T;");
    match stmt.kind {
        StmtKind::TypeAlias(t) => match &t.type_ann.kind {
            TypeNodeKind::Function(f) => {
                assert!(f.type_params.is_some());
            }
            _ => panic!("expected function type"),
        },
        _ => panic!("expected type alias"),
    }
}

#[test]
fn import_type_with_unparenthesized_generic_function_type_arg_stays_in_type_position() {
    let sf = parse_src("export declare const fail1: import(\"module\").Modifier<<T>(x: T) => T>;");
    assert_eq!(
        sf.statements.len(),
        1,
        "generic function type arg inside import type should not leak as extra statements"
    );
    let StmtKind::Export(export_decl) = &sf.statements[0].kind else {
        panic!(
            "expected export declaration, got {:?}",
            sf.statements[0].kind
        );
    };
    let ExportDeclKind::Decl(decl) = &export_decl.kind else {
        panic!(
            "expected export declaration wrapper, got {:?}",
            export_decl.kind
        );
    };
    let StmtKind::Var(var_stmt) = &decl.kind else {
        panic!("expected exported variable statement, got {:?}", decl.kind);
    };
    let ty = var_stmt.declarations[0]
        .type_ann
        .as_ref()
        .expect("expected type annotation");
    let TypeNodeKind::ImportType(import_ty) = &ty.kind else {
        panic!("expected import type annotation, got {:?}", ty.kind);
    };
    let Some(type_args) = &import_ty.type_args else {
        panic!("expected import type arguments");
    };
    assert_eq!(type_args.len(), 1, "expected one import type argument");
    assert!(
        matches!(type_args[0].kind, TypeNodeKind::Function(_)),
        "expected type argument to remain a generic function type, got {:?}",
        type_args[0].kind
    );
}

// =======================================================================
// Decorators
// =======================================================================

#[test]
fn class_decorator() {
    let stmt = first_stmt("@Component class Foo {}");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert_eq!(c.decorators.len(), 1);
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn decorated_class_expression_recovery_keeps_class_expression() {
    let stmt = first_stmt("var v = @decorate class C { static p = 1 };");
    let StmtKind::Var(v) = stmt.kind else {
        panic!("expected variable statement");
    };
    let init = v.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::ClassExpr(class_expr) = &init.kind else {
        panic!("expected recovered class expression, got {:?}", init.kind);
    };
    assert_eq!(class_expr.name.as_deref(), Some("C"));
    assert!(
        matches!(class_expr.decorators.as_slice(), [Expr { kind: ExprKind::Ident(name), .. }] if name == "decorate"),
        "expected class-expression decorator to be preserved, got {:?}",
        class_expr.decorators
    );
    assert!(
        class_expr.members.iter().any(
            |m| matches!(&m.kind, ClassMemberKind::Property(p) if p.modifiers & MOD_STATIC != 0)
        ),
        "expected recovered class expression to keep static members"
    );
}

#[test]
fn decorated_default_class_recovery_keeps_decorators_on_class_decl() {
    let stmt = first_stmt("@decorator\ndefault class {}");
    let StmtKind::ClassDecl(class_decl) = stmt.kind else {
        panic!("expected recovered class declaration");
    };
    assert!(
        class_decl.name.is_none(),
        "expected anonymous recovered class"
    );
    assert_eq!(
        class_decl.decorators.len(),
        1,
        "expected decorator to stay attached for emitter recovery"
    );
}

#[test]
fn class_duplicate_extends_with_type_args_keeps_body_members() {
    let stmt = first_stmt("class D<T> extends C<number> extends C<string> { baz() { } }");
    let StmtKind::ClassDecl(class_decl) = stmt.kind else {
        panic!("expected class declaration");
    };
    assert!(
        class_decl
            .members
            .iter()
            .any(|member| matches!(&member.kind, ClassMemberKind::Method(method) if matches!(&method.name, PropName::Ident(name, _) if name == "baz"))),
        "expected recovered class body to keep baz method"
    );
    assert!(
        !class_decl
            .members
            .iter()
            .any(|member| matches!(&member.kind, ClassMemberKind::Method(method) if matches!(&method.name, PropName::Ident(name, _) if name == "C"))),
        "duplicate heritage clause should not be parsed as a fake class member"
    );
}

#[test]
fn class_duplicate_implements_recovery_keeps_body_members() {
    let stmt = first_stmt("class D implements C implements C { baz() { } }");
    let StmtKind::ClassDecl(class_decl) = stmt.kind else {
        panic!("expected class declaration");
    };
    assert!(
        class_decl
            .members
            .iter()
            .any(|member| matches!(&member.kind, ClassMemberKind::Method(method) if matches!(&method.name, PropName::Ident(name, _) if name == "baz"))),
        "expected duplicate implements recovery to keep baz method"
    );
}

#[test]
fn class_heritage_error_recovers_to_existing_body() {
    let sf = parse_src("class C extends A ¬ {\n}");
    assert_eq!(
        sf.statements.len(),
        1,
        "heritage recovery should keep the class body instead of splitting out a trailing block"
    );
    let StmtKind::ClassDecl(class_decl) = &sf.statements[0].kind else {
        panic!("expected class declaration");
    };
    assert!(
        class_decl.members.is_empty(),
        "expected malformed heritage clause to keep the empty class body"
    );
}

#[test]
fn decorator_with_call() {
    let stmt = first_stmt("@Component({ selector: 'app' }) class Foo {}");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert_eq!(c.decorators.len(), 1);
            assert!(matches!(c.decorators[0].kind, ExprKind::Call(_)));
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn method_decorator() {
    let src = r#"
    class Foo {
        @log
        method() {}
    }
    "#;
    let stmt = first_stmt(src);
    match stmt.kind {
        StmtKind::ClassDecl(c) => match &c.members[0].kind {
            ClassMemberKind::Method(m) => {
                assert_eq!(m.decorators.len(), 1);
            }
            _ => panic!("expected method"),
        },
        _ => panic!("expected class"),
    }
}

#[test]
fn top_level_await_error_recovery_keeps_decorators_and_parameters() {
    let sf = parse_src(
        "export {}; await <number, string>(1); \
         class C extends await<string> {} \
         @await(x) class A {} \
         class D { method(@await(1) [x]) {} }",
    );

    assert!(
        matches!(&sf.statements[1].kind, StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::Comma(_))),
        "invalid await type arguments should reparse as one comma expression"
    );

    let StmtKind::ClassDecl(c) = &sf.statements[2].kind else {
        panic!("expected recovered heritage class");
    };
    assert!(
        matches!(c.extends.as_deref().map(|expr| &expr.kind), Some(ExprKind::Ident(name)) if name == "string"),
        "await<T> heritage recovery should retain T as the base expression"
    );

    let StmtKind::ClassDecl(a) = &sf.statements[3].kind else {
        panic!("expected decorated class");
    };
    assert!(matches!(a.decorators[0].kind, ExprKind::Paren(_)));

    let StmtKind::ClassDecl(d) = &sf.statements[4].kind else {
        panic!("expected class with decorated parameter");
    };
    let ClassMemberKind::Method(method) = &d.members[0].kind else {
        panic!("expected method");
    };
    assert!(matches!(method.params[0].name.kind, PatKind::Array(_)));
    assert!(matches!(
        method.params[0].decorators[0].kind,
        ExprKind::Paren(_)
    ));
}

// =======================================================================
// Declare / ambient contexts
// =======================================================================

#[test]
fn declare_function() {
    let stmt = first_stmt("declare function foo(): void;");
    match stmt.kind {
        StmtKind::FnDecl(f) => {
            assert!(f.modifiers & MOD_DECLARE != 0);
            assert!(f.body.is_none());
        }
        _ => panic!("expected fn decl"),
    }
}

#[test]
fn declare_variable() {
    let stmt = first_stmt("declare const x: number;");
    match stmt.kind {
        StmtKind::Var(v) => {
            assert!(v.modifiers & MOD_DECLARE != 0);
        }
        _ => panic!("expected var"),
    }
}

#[test]
fn declare_class() {
    let stmt = first_stmt("declare class Foo { method(): void; }");
    match stmt.kind {
        StmtKind::ClassDecl(c) => {
            assert!(c.modifiers & MOD_DECLARE != 0);
        }
        _ => panic!("expected class"),
    }
}

#[test]
fn declare_namespace() {
    let stmt = first_stmt("declare namespace N { function foo(): void; }");
    match stmt.kind {
        StmtKind::ModuleDecl(m) => {
            assert!(m.modifiers & MOD_DECLARE != 0);
        }
        _ => panic!("expected module"),
    }
}

#[test]
fn declaration_file_reports_first_executable_statement_in_ambient_list() {
    let sf = parse(
        "types.d.ts",
        "while (true) {} debugger; declare const ok: number;",
    );
    let ambient: Vec<_> = sf.diagnostics.iter().filter(|d| d.code == 1036).collect();
    assert_eq!(ambient.len(), 1, "diagnostics: {:?}", sf.diagnostics);
    let span = ambient[0].span.expect("TS1036 should have a token span");
    assert_eq!(&sf.text[span.start as usize..span.end as usize], "while");
}

#[test]
fn declared_namespaces_have_independent_ambient_statement_lists() {
    let sf = parse(
        "test.ts",
        "declare namespace A { debugger; while (true) {} }\n\
         declare namespace B { break; }",
    );
    let ambient: Vec<_> = sf.diagnostics.iter().filter(|d| d.code == 1036).collect();
    assert_eq!(ambient.len(), 2, "diagnostics: {:?}", sf.diagnostics);
    let tokens: Vec<_> = ambient
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1036 should have a token span");
            &sf.text[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(tokens, ["debugger", "break"]);
}

#[test]
fn runtime_namespace_statements_are_not_ambient() {
    let sf = parse("test.ts", "namespace Runtime { while (true) {} }");
    assert!(
        sf.diagnostics.iter().all(|d| d.code != 1036),
        "diagnostics: {:?}",
        sf.diagnostics
    );
}

#[test]
fn with_statement_reports_unsupported_syntax() {
    let sf = parse("test.ts", "with (value) {}");
    let diagnostic = sf
        .diagnostics
        .iter()
        .find(|d| d.code == 2410)
        .expect("with statement should report TS2410");
    assert_eq!(diagnostic.span, Some(Span::new(0, 12)));
}

#[test]
fn nested_with_region_reports_ts2410_once() {
    let sf = parse("test.ts", "with (outer) { with (inner) {} }");
    assert_eq!(
        sf.diagnostics.iter().filter(|d| d.code == 2410).count(),
        1,
        "diagnostics: {:?}",
        sf.diagnostics
    );
}

#[test]
fn declaration_file_reports_modifier_initializer_and_generator_grammar() {
    let sf = parse(
        "types.d.ts",
        "var value = make();\n\
         declare namespace Outer {\n\
           declare namespace Inner {}\n\
           function *iterator(): any;\n\
         }",
    );
    let codes: Vec<_> = sf
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&1046), "diagnostics: {:?}", sf.diagnostics);
    assert!(codes.contains(&1039), "diagnostics: {:?}", sf.diagnostics);
    assert!(codes.contains(&1038), "diagnostics: {:?}", sf.diagnostics);
    assert!(codes.contains(&1221), "diagnostics: {:?}", sf.diagnostics);
}

#[test]
fn ambient_const_literal_initializers_are_allowed() {
    let sf = parse(
        "types.d.ts",
        "export const numberValue = 1;\n\
         export const stringValue = 'ok';\n\
         export const templateValue = `ok`;",
    );
    assert!(
        sf.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1039),
        "diagnostics: {:?}",
        sf.diagnostics
    );
}

// =======================================================================
// Error recovery
// =======================================================================

#[test]
fn error_recovery_unexpected_token() {
    let sf = parse_src("@ @ let x = 1;");
    // Should produce diagnostics but still parse something
    assert!(!sf.statements.is_empty());
}

#[test]
fn error_recovery_missing_semicolon() {
    // Missing semicolons should be tolerated (ASI)
    let sf = parse_src("let x = 1\nlet y = 2");
    assert_eq!(sf.statements.len(), 2);
    assert!(sf.diagnostics.is_empty());
}

#[test]
fn error_recovery_empty_source() {
    let sf = parse_src("");
    assert!(sf.statements.is_empty());
    assert!(sf.diagnostics.is_empty());
}

// =======================================================================
// Meta properties
// =======================================================================

#[test]
fn new_target() {
    let expr = first_expr("new.target;");
    assert!(matches!(
        expr.kind,
        ExprKind::MetaProp(ref meta) if meta.meta == "new" && meta.property == "target"
    ));
}

#[test]
fn import_meta() {
    let expr = first_expr("import.meta;");
    assert!(matches!(
        expr.kind,
        ExprKind::MetaProp(ref meta) if meta.meta == "import" && meta.property == "meta"
    ));
}

// =======================================================================
// Dynamic import
// =======================================================================

#[test]
fn dynamic_import() {
    let expr = first_expr("import('module');");
    assert!(matches!(expr.kind, ExprKind::Call(_)));
}

// =======================================================================
// Parenthesized expression
// =======================================================================

#[test]
fn parenthesized_expr() {
    let expr = first_expr("(1 + 2);");
    assert!(matches!(expr.kind, ExprKind::Paren(_)));
}

// =======================================================================
// Function expression
// =======================================================================

#[test]
fn function_expression() {
    let expr = first_expr("(function foo(x) { return x; });");
    match expr.kind {
        ExprKind::Paren(inner) => {
            assert!(matches!(inner.kind, ExprKind::FnExpr(_)));
        }
        _ => panic!("expected paren"),
    }
}

// =======================================================================
// Class expression
// =======================================================================

#[test]
fn class_expression() {
    let expr = first_expr("(class { });");
    match expr.kind {
        ExprKind::Paren(inner) => {
            assert!(matches!(inner.kind, ExprKind::ClassExpr(_)));
        }
        _ => panic!("expected paren"),
    }
}

// =======================================================================
// Complex / integration tests
// =======================================================================

#[test]
fn complex_generic_function() {
    assert_no_errors(
        "function merge<T extends object, U extends object>(a: T, b: U): T & U { return Object.assign({}, a, b) as T & U; }"
    );
}

#[test]
fn complex_class_with_everything() {
    assert_no_errors(
        r#"
        class EventEmitter<T extends Record<string, any[]>> {
            private listeners: Map<string, Function[]> = new Map();
            on<K extends keyof T>(event: K, fn: (...args: T[K]) => void): this {
                return this;
            }
            emit<K extends keyof T>(event: K, ...args: T[K]): boolean {
                return true;
            }
        }
    "#,
    );
}

#[test]
fn complex_module_with_imports_exports() {
    assert_no_errors(
        r#"
        import { Component } from 'framework';
        import type { Config } from './config';

        export interface AppConfig extends Config {
            name: string;
        }

        export default class App {
            constructor(private config: AppConfig) {}
        }

        export { App as default };
    "#,
    );
}

#[test]
fn conditional_type_with_infer() {
    assert_no_errors("type UnwrapPromise<T> = T extends Promise<infer U> ? U : T;");
}

#[test]
fn mapped_type_with_as() {
    assert_no_errors(
        "type Getters<T> = { [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K] };",
    );
}

#[test]
fn complex_expression_chain() {
    assert_no_errors(
        "const result = arr.filter(x => x > 0).map(x => x * 2).reduce((a, b) => a + b, 0);",
    );
}

#[test]
fn multiple_statements_program() {
    let sf = parse_src(
        r#"
        const x = 1;
        let y: string = "hello";
        function add(a: number, b: number): number {
            return a + b;
        }
        interface Shape {
            area(): number;
        }
        class Circle implements Shape {
            constructor(public radius: number) {}
            area(): number {
                return Math.PI * this.radius ** 2;
            }
        }
    "#,
    );
    assert!(sf.diagnostics.is_empty(), "errors: {:?}", sf.diagnostics);
    assert!(sf.statements.len() >= 5);
}

#[test]
fn nested_conditional_types() {
    assert_no_errors(
        "type Deep<T> = T extends string ? 'string' : T extends number ? 'number' : 'other';",
    );
}

#[test]
fn readonly_and_optional_mapped() {
    assert_no_errors("type ReadonlyPartial<T> = { readonly [K in keyof T]?: T[K] };");
}

#[test]
fn ternary_parenthesized_consequent_no_spurious_diag() {
    // A ternary whose consequent is a parenthesized (possibly nested-ternary)
    // expression must not leak a diagnostic from the speculative arrow-param
    // parse — `(b ? c : d)` is not `(b?): ...`. A stray diagnostic here would
    // flip file_has_recovery_errors and disable emitter recovery fast-outs.
    assert_no_errors("const x = a ? (b ? c : d) : e;");
    // Single parenthesized consequent — `(b)` must not be mis-parsed as an
    // incomplete arrow `(b): c =>` (which consumed the ternary alternate).
    assert_no_errors("const x = a ? (b) : c;");
    assert_no_errors("let y = cond ? (foo) : bar;");
    // Genuine arrows with return types must still parse.
    assert_no_errors("const g = (): number => 1;");
    assert_no_errors("const h = <T>(x: T): T => x;");
    let sf = parse_tsx_src("const y = <p>{a ? (b ? c : d) : e}</p>;");
    assert!(
        sf.diagnostics.is_empty(),
        "expected no errors, got: {:?}",
        sf.diagnostics
    );

    // The consequent must be a conditional expression, not an arrow function.
    match &first_stmt("const x = a ? (b) : c;").kind {
        StmtKind::Var(v) => match &v.declarations[0].init.as_ref().unwrap().kind {
            ExprKind::Cond(_) => {}
            other => panic!("expected Cond, got {:?}", other),
        },
        other => panic!("expected Var, got {:?}", other),
    }
}

#[test]
fn readonly_as_member_name() {
    // `readonly` is a valid member name when not in modifier position — it must
    // not be swallowed as the readonly modifier. Both the optional and required
    // forms, plus the genuine modifier and index-signature forms, must parse.
    assert_no_errors("interface I { readonly?: boolean; }");
    assert_no_errors("interface I { readonly: boolean; }");
    assert_no_errors("interface I { readonly foo: boolean; }");
    assert_no_errors("interface I { readonly [k: string]: boolean; }");
    assert_no_errors("type T = { readonly?: () => void; readonly bar: number };");
}

#[test]
fn generic_arrow_with_constraint() {
    assert_no_errors("const f = <T extends { id: number }>(x: T): T => x;");
}

#[test]
fn generic_arrow_with_multi_argument_type_intersection() {
    let src = r#"
const post = <Path extends string>(
    path: Path,
    options: Omit<RequestInit, "body"> & { body: unknown }
) => path;
"#;
    let sf = parse_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let StmtKind::Var(var) = &sf.statements[0].kind else {
        panic!("expected variable declaration");
    };
    assert!(matches!(
        var.declarations[0].init.as_deref().map(|expr| &expr.kind),
        Some(ExprKind::Arrow(_))
    ));
}

#[test]
fn type_assertion_with_comparison_comma_expression_is_not_an_arrow() {
    let sf = parse_src("const value = <T>(a < b, c > d);");
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let StmtKind::Var(var) = &sf.statements[0].kind else {
        panic!("expected variable declaration");
    };
    assert!(matches!(
        var.declarations[0].init.as_deref().map(|expr| &expr.kind),
        Some(ExprKind::TypeAssertion(_))
    ));
}

#[test]
fn index_signature_with_type_keyword() {
    // This should not hang
    let _file = parse(
        "test.ts",
        r#"
interface IHandlerMap {
    [type: string]: boolean;
}
"#,
    );
}

#[test]
fn type_literal_with_less_than_minus() {
    // Should not hang - error recovery for invalid tokens in type literal
    let _file = parse("test.ts", "var f: { x: number; <- };");
}

#[test]
fn jsdoc_nullable_type_args_recover_as_instantiation() {
    let sf = parse_src(
        "const a = foo<?>;\nconst b = foo<string?>;\nconst c = foo<?string>;\nconst d = foo<?string?>;",
    );
    assert_eq!(sf.statements.len(), 4, "expected four variable statements");
    for stmt in &sf.statements {
        let StmtKind::Var(v) = &stmt.kind else {
            panic!("expected variable statement");
        };
        let init = v.declarations[0]
            .init
            .as_ref()
            .expect("expected initializer");
        let ExprKind::Instantiation(inst) = &init.kind else {
            panic!("expected recovered instantiation, got {:?}", init.kind);
        };
        assert_eq!(
            inst.type_args.len(),
            1,
            "expected a single recovered type arg"
        );
        assert!(
            matches!(inst.type_args[0].kind, TypeNodeKind::JSDocNullable(_)),
            "expected JSDoc nullable recovery, got {:?}",
            inst.type_args[0].kind
        );
    }
}

#[test]
fn postfix_nullable_type_before_initializer_is_consumed() {
    let sf = parse_src("const a: number? = 1;\nconst b = 2;");
    assert_eq!(
        sf.statements.len(),
        2,
        "postfix nullable recovery should not leave a stray question statement"
    );
    let StmtKind::Var(first) = &sf.statements[0].kind else {
        panic!("expected first statement to stay a variable declaration");
    };
    assert!(
        first.declarations[0].init.is_some(),
        "initializer should stay attached after postfix nullable recovery"
    );
    assert!(
        matches!(
            first.declarations[0].type_ann.as_ref().map(|ty| &ty.kind),
            Some(TypeNodeKind::JSDocNullable(Some(_)))
        ),
        "expected recoverable postfix nullable type annotation"
    );
    assert!(
        matches!(&sf.statements[1].kind, StmtKind::Var(_)),
        "subsequent declarations should remain aligned"
    );
}

#[test]
fn invalid_nonnullable_types_keep_bodies_and_initializers_attached() {
    let sf = parse_src(
        "function f1(a: string!) {}\nfunction f2(a: !string) {}\nfunction f3(): string! {}\nconst x: number! = 1;\nconst y = 2;",
    );
    assert_eq!(
        sf.statements.len(),
        5,
        "invalid nonnullable type recovery should not spill extra statements"
    );

    for index in 0..3 {
        let StmtKind::FnDecl(func) = &sf.statements[index].kind else {
            panic!("expected function declaration at statement {index}");
        };
        assert!(
            func.body.is_some(),
            "function {index} should keep its body after invalid nonnullable recovery"
        );
    }

    let StmtKind::Var(first_var) = &sf.statements[3].kind else {
        panic!("expected first variable declaration after functions");
    };
    assert!(
        first_var.declarations[0].init.is_some(),
        "invalid postfix nonnullable type should keep the initializer attached"
    );
    assert!(
        matches!(&sf.statements[4].kind, StmtKind::Var(_)),
        "subsequent declarations should remain aligned after invalid nonnullable recovery"
    );
}

#[test]
fn unknown_assignment_statement_recovery_keeps_only_rhs_expression() {
    let sf = parse_src("var a\u{2081} = \"hello\"; alert(a\u{2081});");
    assert!(
        matches!(
            sf.statements.first().map(|stmt| &stmt.kind),
            Some(StmtKind::Var(_))
        ),
        "the leading declaration should stay a variable statement"
    );
    assert!(
        sf.statements.iter().any(
            |stmt| matches!(&stmt.kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::StrLit(value) if value == "hello"))
        ),
        "invalid assignment lhs should be dropped, keeping only the rhs string literal"
    );
    assert!(
        sf.statements.iter().any(
            |stmt| matches!(
                &stmt.kind,
                StmtKind::Expr(expr)
                    if matches!(
                        &expr.kind,
                        ExprKind::Call(call)
                            if matches!(&call.callee.kind, ExprKind::Ident(name) if name == "alert")
                                && matches!(call.args.first().map(|arg| &arg.kind), Some(ExprKind::Ident(name)) if name == "a")
                    )
            )
        ),
        "subsequent alert call should stay parseable as `alert(a)`"
    );
}

#[test]
fn call_arguments_recover_missing_commas_between_expression_starts() {
    let stmt = first_stmt("foo(public blaz() {});");
    let StmtKind::Expr(expr) = stmt.kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Call(call) = expr.kind else {
        panic!("expected call expression");
    };
    assert_eq!(call.args.len(), 3, "expected recovered missing commas");
    assert!(
        matches!(call.args[0].kind, ExprKind::Ident(ref name) if name == "public"),
        "expected first arg to remain `public`"
    );
    assert!(
        matches!(call.args[1].kind, ExprKind::Call(_)),
        "expected second arg to recover as `blaz()`"
    );
    assert!(
        matches!(call.args[2].kind, ExprKind::ObjectLit(_)),
        "expected third arg to recover as object literal"
    );
}

#[test]
fn call_arguments_recover_invalid_arrow_block_as_object_literal() {
    let stmt = first_stmt("foo((1)=>{return 0;});");
    let StmtKind::Expr(expr) = stmt.kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Call(call) = expr.kind else {
        panic!("expected call expression");
    };
    assert_eq!(
        call.args.len(),
        2,
        "expected recovered arrow block object literal"
    );
    assert!(
        matches!(call.args[0].kind, ExprKind::Paren(_)),
        "expected first arg to remain parenthesized, got {:?}",
        call.args[0].kind
    );
    let ExprKind::ObjectLit(props) = &call.args[1].kind else {
        panic!(
            "expected second arg to recover as object literal, got {:?}",
            call.args[1].kind
        );
    };
    assert_eq!(
        props.len(),
        1,
        "expected a single recovered object property"
    );
    let ObjLitProp::Property(prop) = &props[0] else {
        panic!("expected recovered property, got {:?}", props[0]);
    };
    assert!(
        matches!(&prop.key, PropName::Ident(name, _) if name == "return"),
        "expected recovered property key to be `return`, got {:?}",
        prop.key
    );
    assert!(
        matches!(prop.value.kind, ExprKind::NumLit(ref n) if n == "0"),
        "expected recovered property value to be `0`, got {:?}",
        prop.value.kind
    );
}

#[test]
fn call_arguments_recover_missing_commas_inside_class_method_initializer() {
    let sf = parse_src(
        "class C {\n    public bar() {\n        var v = foo(\n            public blaz() {}\n            );\n    }\n}",
    );
    let StmtKind::ClassDecl(class_decl) = &sf.statements[0].kind else {
        panic!("expected class declaration");
    };
    let ClassMemberKind::Method(method) = &class_decl.members[0].kind else {
        panic!("expected class method");
    };
    let body = method.body.as_ref().expect("expected method body");
    let StmtKind::Var(var_stmt) = &body[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::Call(call) = &init.kind else {
        panic!("expected call initializer, got {:?}", init.kind);
    };
    assert_eq!(
        call.args.len(),
        3,
        "expected recovered missing commas inside class method call"
    );
    assert!(
        matches!(call.args[0].kind, ExprKind::Ident(ref name) if name == "public"),
        "expected first arg to remain `public`, got {:?}",
        call.args[0].kind
    );
    assert!(
        matches!(call.args[1].kind, ExprKind::Call(_)),
        "expected second arg to recover as `blaz()`, got {:?}",
        call.args[1].kind
    );
    assert!(
        matches!(call.args[2].kind, ExprKind::ObjectLit(_)),
        "expected third arg to recover as object literal, got {:?}",
        call.args[2].kind
    );
}

#[test]
fn variable_initializer_rewrites_malformed_typed_arrow_tail() {
    let sf = parse_src("var y = x:number => x*x;");
    assert_eq!(
        sf.statements.len(),
        2,
        "expected var statement plus expression tail"
    );
    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    assert_eq!(
        var_stmt.declarations.len(),
        2,
        "expected malformed arrow tail to recover as a second declarator"
    );
    assert!(
        matches!(&var_stmt.declarations[0].init, Some(init) if matches!(init.kind, ExprKind::Ident(ref name) if name == "x")),
        "expected first declarator initializer to remain `x`, got {:?}",
        var_stmt.declarations[0].init
    );
    assert!(
        matches!(&var_stmt.declarations[1].name.kind, PatKind::Ident(name) if name == "number"),
        "expected second declarator name to be `number`, got {:?}",
        var_stmt.declarations[1].name.kind
    );
    let StmtKind::Expr(expr) = &sf.statements[1].kind else {
        panic!("expected recovered expression statement");
    };
    assert!(
        matches!(expr.kind, ExprKind::Binary(_)),
        "expected trailing `x * x` expression, got {:?}",
        expr.kind
    );
}

#[test]
fn declare_class_missing_body_keeps_trailing_call_and_following_fn() {
    let sf = parse_src("declare class foo();\nfunction foo() {}");
    assert_eq!(
        sf.statements.len(),
        3,
        "expected declare class, recovered call, and following function"
    );
    let StmtKind::ClassDecl(class_decl) = &sf.statements[0].kind else {
        panic!("expected class declaration");
    };
    assert!(
        class_decl.members.is_empty(),
        "missing class body should not consume following tokens as fake members"
    );
    assert!(
        matches!(
            &sf.statements[1].kind,
            StmtKind::Expr(expr)
                if matches!(
                    expr.kind,
                    ExprKind::Paren(ref inner)
                        if matches!(inner.kind, ExprKind::Omitted)
                )
        ),
        "expected recovered `();` expression statement, got {:?}",
        sf.statements[1].kind
    );
    assert!(
        matches!(sf.statements[2].kind, StmtKind::FnDecl(_)),
        "expected following function declaration to remain separate"
    );
}

#[test]
fn unmatched_type_assertion_recovers_zero_span_error_expr_stmt() {
    let sf = parse_src("@<[[import(obju2c77,\n");
    assert_eq!(
        sf.statements.len(),
        1,
        "expected a single recovered expression statement"
    );
    let StmtKind::Expr(expr) = &sf.statements[0].kind else {
        panic!(
            "expected expression statement, got {:?}",
            sf.statements[0].kind
        );
    };
    assert!(
        matches!(&expr.kind, ExprKind::Ident(name) if name == "<error>"),
        "expected zero-span error placeholder, got {:?}",
        expr.kind
    );
    assert_eq!(
        expr.span.start, expr.span.end,
        "expected unmatched type assertion recovery to keep an empty statement placeholder"
    );
}

#[test]
fn midfile_hashbang_recovery_preserves_bang_statement_shapes() {
    let sf = parse_src(
        "const a =!@#!@$\nconst b = !@#!@#!@#!\nOK!\nHERE's A shouty thing\nGOTTA GO FAST\n",
    );
    assert_eq!(
        sf.statements.len(),
        10,
        "expected recovered statement split"
    );
    assert!(
        matches!(
            &sf.statements[0].kind,
            StmtKind::Var(var_stmt)
                if matches!(
                    &var_stmt.declarations[0].init,
                    Some(init) if matches!(&init.kind, ExprKind::Unary(_))
                )
        ),
        "expected first statement to keep unary recovery in const initializer"
    );
    assert!(
        matches!(
            &sf.statements[1].kind,
            StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::NonNull(inner) if matches!(&inner.kind, ExprKind::Ident(name) if name == "<error>"))
        ),
        "expected `#!` fragment to recover as non-null on an error placeholder"
    );
    assert!(
        matches!(
            &sf.statements[4].kind,
            StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::NonNull(inner) if matches!(&inner.kind, ExprKind::Ident(name) if name == "OK"))
        ),
        "expected trailing `OK!` fragment to recover as non-null on `OK`"
    );
}

#[test]
fn new_operator_multiline_array_callee_recovers_into_elem_access_with_empty_args() {
    let sf = parse_src("var t4 =\nnew\nstring\n[\n    ]\n    (\n        );");
    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::New(new_expr) = &init.kind else {
        panic!("expected new expression, got {:?}", init.kind);
    };
    let ExprKind::ElemAccess(ea) = &new_expr.callee.kind else {
        panic!(
            "expected elem-access callee, got {:?}",
            new_expr.callee.kind
        );
    };
    assert!(
        matches!(&ea.object.kind, ExprKind::Ident(name) if name == "string"),
        "expected callee object `string`, got {:?}",
        ea.object.kind
    );
    assert!(
        matches!(ea.index.kind, ExprKind::Omitted),
        "expected empty bracket index to recover as omitted, got {:?}",
        ea.index.kind
    );
    assert!(
        matches!(&new_expr.args, Some(args) if args.is_empty()),
        "expected trailing call parens to remain the new-expression argument list"
    );
}

#[test]
fn object_rest_with_property_name_recovers_binding_and_keeps_initializer() {
    let sf = parse_src("const { ...a: b } = {};");
    assert_eq!(
        sf.statements.len(),
        1,
        "the malformed binding must remain one declaration statement"
    );
    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    let [decl] = var_stmt.declarations.as_slice() else {
        panic!("expected a single declarator");
    };
    assert!(
        matches!(&decl.name.kind, PatKind::Object(props) if matches!(props.as_slice(), [ObjPatProp::Rest(rest)] if matches!(&rest.kind, PatKind::Ident(name) if name == "b"))),
        "expected the property-name side to become rest binding `b`, got {:?}",
        decl.name.kind
    );
    assert!(
        decl.type_ann.is_none(),
        "the invalid colon is not a declaration type annotation: {:?}",
        decl.type_ann
    );
    assert!(
        matches!(decl.init.as_deref().map(|expr| &expr.kind), Some(ExprKind::ObjectLit(props)) if props.is_empty()),
        "expected `= {{}}` to remain the declarator initializer, got {:?}",
        decl.init
    );
}

#[test]
fn object_literal_empty_element_access_recovery_keeps_following_properties() {
    let expr = first_expr("({ tokens: Gar[], endState: state });");
    let ExprKind::Paren(inner) = &expr.kind else {
        panic!("expected parenthesized object literal expression");
    };
    let ExprKind::ObjectLit(props) = &inner.kind else {
        panic!("expected object literal, got {:?}", inner.kind);
    };
    assert_eq!(
        props.len(),
        2,
        "expected empty element access recovery not to swallow the next property"
    );

    let ObjLitProp::Property(tokens_prop) = &props[0] else {
        panic!("expected first object member to be a property");
    };
    assert!(
        matches!(&tokens_prop.key, PropName::Ident(name, _) if name == "tokens"),
        "expected first property key `tokens`, got {:?}",
        tokens_prop.key
    );
    let ExprKind::ElemAccess(elem_access) = &tokens_prop.value.kind else {
        panic!(
            "expected `tokens` value to recover as element access, got {:?}",
            tokens_prop.value.kind
        );
    };
    assert!(
        matches!(&elem_access.object.kind, ExprKind::Ident(name) if name == "Gar"),
        "expected element access object `Gar`, got {:?}",
        elem_access.object.kind
    );
    assert!(
        matches!(elem_access.index.kind, ExprKind::Omitted),
        "expected empty bracket index to recover as omitted, got {:?}",
        elem_access.index.kind
    );

    let ObjLitProp::Property(end_state_prop) = &props[1] else {
        panic!("expected second object member to be a property");
    };
    assert!(
        matches!(&end_state_prop.key, PropName::Ident(name, _) if name == "endState"),
        "expected second property key `endState`, got {:?}",
        end_state_prop.key
    );
    assert!(
        matches!(&end_state_prop.value.kind, ExprKind::Ident(name) if name == "state"),
        "expected second property value `state`, got {:?}",
        end_state_prop.value.kind
    );
}

#[test]
fn variable_declarators_recover_invalid_unicode_identifier_escape_separator() {
    for (source, expected_tail) in [("var arg\\u003", "u003"), ("var arg\\uxxxx", "uxxxx")] {
        let sf = parse_src(source);
        assert_eq!(
            sf.statements.len(),
            1,
            "expected a single recovered variable statement for {source}"
        );
        let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
            panic!("expected variable statement for {source}");
        };
        assert_eq!(
            var_stmt.declarations.len(),
            2,
            "expected invalid identifier escape to recover as a second declarator for {source}"
        );
        assert!(
            matches!(&var_stmt.declarations[0].name.kind, PatKind::Ident(name) if name == "arg"),
            "expected first declarator name to remain `arg`, got {:?}",
            var_stmt.declarations[0].name.kind
        );
        assert!(
            matches!(&var_stmt.declarations[1].name.kind, PatKind::Ident(name) if name == expected_tail),
            "expected second declarator name to recover as `{expected_tail}`, got {:?}",
            var_stmt.declarations[1].name.kind
        );
    }
}

#[test]
fn variable_declarators_recover_invalid_unicode_escape_non_identifier_start() {
    let sf = parse_src("var \\u0031a;");
    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    assert_eq!(
        var_stmt.declarations.len(),
        2,
        "expected invalid unicode escape leading token to recover as skipped error declarator plus tail identifier"
    );
    assert!(
        matches!(&var_stmt.declarations[0].name.kind, PatKind::Ident(name) if name == "<error>"),
        "expected first declarator to be recovered error placeholder, got {:?}",
        var_stmt.declarations[0].name.kind
    );
    assert!(
        matches!(&var_stmt.declarations[1].name.kind, PatKind::Ident(name) if name == "u0031a"),
        "expected second declarator name to recover as `u0031a`, got {:?}",
        var_stmt.declarations[1].name.kind
    );
}

#[test]
fn declare_module_without_name_keeps_body_block() {
    let sf = parse_src("declare module { export class X {} }");
    let StmtKind::ModuleDecl(module_decl) = &sf.statements[0].kind else {
        panic!("expected module declaration");
    };
    assert!(
        matches!(&module_decl.name, ModuleName::Ident(name) if name == "<error>"),
        "expected recovered missing module name, got {:?}",
        module_decl.name
    );
    let Some(ModuleBody::Block(stmts)) = &module_decl.body else {
        panic!("expected recovered module body block");
    };
    assert_eq!(stmts.len(), 1, "expected block contents to be preserved");
}

#[test]
fn test_type_predicate_body_exists() {
    let src = r#"function isFoo1(object: {}): object is Foo {
    return 'foo' in object;
}"#;
    let sf = parse_src(src);
    assert_eq!(sf.statements.len(), 1);
    if let StmtKind::FnDecl(f) = &sf.statements[0].kind {
        assert!(f.body.is_some());
        let body = f.body.as_ref().unwrap();
        assert_eq!(body.len(), 1);
    } else {
        panic!("Expected FnDecl");
    }
}

#[test]
fn mapped_type_with_extra_members_fully_erased() {
    // Mapped types with extra members after the `[K in T]` clause must still
    // produce a complete TypeAlias node so the emitter erases the whole thing.
    let src = r#"type After = {
    [placeType in PlaceType]: void;
    model: 'hour' | 'day'
}
class C { }"#;
    let sf = parse_src(src);
    assert_eq!(sf.statements.len(), 2);
    assert!(matches!(&sf.statements[0].kind, StmtKind::TypeAlias(_)));
    assert!(matches!(&sf.statements[1].kind, StmtKind::ClassDecl(_)));
}

#[test]
fn infer_extends_conditional_speculative_lookahead() {
    // `infer U extends number ? 1 : 0` inside parens: the `?` means it's a
    // conditional type, not an infer constraint (speculative lookahead).
    // Without parens (in extends clause): it IS the infer constraint.
    let src = r#"type X10<T> = T extends (infer U extends number ? 1 : 0) ? 1 : 0;
type X13<T> = T extends infer U extends number ? 1 : 0;
function f() { return 1; }"#;
    let sf = parse_src(src);
    let mut expr_count = 0;
    for s in &sf.statements {
        if matches!(&s.kind, StmtKind::Expr(_)) {
            expr_count += 1;
        }
    }
    assert_eq!(
        expr_count, 0,
        "type alias content leaked as expression statements"
    );
    assert_eq!(sf.statements.len(), 3); // 2 TypeAlias + 1 FnDecl
}

#[test]
fn infer_types_with_extends1_no_leaks() {
    // Full regression test: all type aliases in inferTypesWithExtends1.ts must
    // parse completely with no expression statement leaks.
    let src = include_str!(
        "../../../tests/cases/conformance/types/conditional/inferTypesWithExtends1.ts"
    );
    let sf = parse_src(src);
    let mut expr_count = 0;
    for s in &sf.statements {
        if matches!(&s.kind, StmtKind::Expr(_)) {
            expr_count += 1;
        }
    }
    assert_eq!(
        expr_count, 0,
        "expression statements leaked from type aliases"
    );
}

#[test]
fn arrow_recovery_multiple_stmts() {
    let src = r#"class C {
    where(filter) {
        return fromDoWhile(test =>
            var index = 0;
            return this.doWhile((item, i) => filter(item, i) ? test(item, index++) : true);
        });
    }
}"#;
    let sf = parse_src(src);
    // Should have 1 class declaration
    assert_eq!(sf.statements.len(), 1, "expected 1 class");
    if let StmtKind::ClassDecl(cls) = &sf.statements[0].kind {
        assert_eq!(cls.members.len(), 1, "expected 1 member");
        if let ClassMemberKind::Method(m) = &cls.members[0].kind {
            let body = m.body.as_ref().expect("expected method body");
            assert_eq!(body.len(), 1, "expected 1 stmt in method body");
            if let StmtKind::Return(Some(call_expr)) = &body[0].kind {
                if let ExprKind::Call(call) = &call_expr.kind {
                    if let ExprKind::Arrow(arrow) = &call.args[0].kind {
                        if let ArrowBody::Block(stmts) = &arrow.body {
                            assert_eq!(
                                stmts.len(),
                                2,
                                "expected 2 stmts in recovered arrow body, got {}",
                                stmts.len()
                            );
                        } else {
                            panic!("expected ArrowBody::Block");
                        }
                    } else {
                        panic!(
                            "expected Arrow, got {:?}",
                            std::mem::discriminant(&call.args[0].kind)
                        );
                    }
                } else {
                    panic!("expected Call");
                }
            } else {
                panic!("expected Return with call");
            }
        } else {
            panic!("expected Method");
        }
    } else {
        panic!("expected ClassDecl");
    }
}

// ---------------------------------------------------------------------------
// JSX text content: scanner-comment defenses
//
// The scanner is JSX-unaware and treats `//` and `/* */` as comments uniformly.
// Inside JSX text these are literal characters, not comments — without a
// defense in parse_jsx_children, comments swallow JSX structure (closing tags,
// sibling elements). These tests guard the recovery path.
// ---------------------------------------------------------------------------

fn collect_text(children: &[JsxChild]) -> String {
    let mut out = String::new();
    for child in children {
        if let JsxChild::Text(t, _) = child {
            out.push_str(t);
        }
    }
    out
}

fn count_element_children(children: &[JsxChild]) -> usize {
    children
        .iter()
        .filter(|c| matches!(c, JsxChild::Element(_)))
        .count()
}

fn jsx_root_element(sf: &SourceFile) -> &JsxElement {
    let StmtKind::Var(v) = &sf.statements[0].kind else {
        panic!(
            "expected variable statement, got {:?}",
            sf.statements[0].kind
        );
    };
    let init = v.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    match &init.kind {
        ExprKind::JsxElement(el) => el,
        other => panic!("expected JSX element, got {:?}", other),
    }
}

#[test]
fn jsx_text_apostrophe_before_quoted_expression_container() {
    // Regression: JSX text is not a lexical context in the batch scanner, so
    // the `'` of `n'avez` paired with the opening quote of `{' '}`. That left
    // StringLiteral `'avez pas ?{'` swallowing the `{`, and its closing quote
    // opened a second literal that ate the rest of the file — "expected
    // CloseBrace, got StringLiteral", then recovery emitted invalid JS.
    let src = r#"const x = <p>Vous n'avez pas de compte ?{' '}<a href="/s">Ici</a></p>;"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let el = jsx_root_element(&sf);
    let text = collect_text(&el.children);
    assert_eq!(
        text.trim(),
        "Vous n'avez pas de compte ?",
        "the apostrophe belongs to the text, the container must survive"
    );
    assert_eq!(
        count_element_children(&el.children),
        1,
        "the <a> sibling after the container must still be a child"
    );
}

#[test]
fn jsx_text_apostrophes_around_repeated_containers() {
    // Two containers, an apostrophe on each side: every `{' '}` must survive.
    let src = r#"const x = <p>L'un{' '}et{' '}l'autre</p>;"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let el = jsx_root_element(&sf);
    let text = collect_text(&el.children);
    assert!(
        text.contains("L'un") && text.contains("et") && text.contains("l'autre"),
        "text pieces should all be present, got {text:?}"
    );
}

#[test]
fn jsx_text_url_with_double_slash_does_not_swallow_closing_tag() {
    // Regression: scanner saw `//demo.bext.dev/foo</code>` as a line comment,
    // which dropped the closing `</code>` from the token stream.
    let src = r#"const x = <code>https://demo.bext.dev/examples/not-found-file</code>;"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let el = jsx_root_element(&sf);
    let text = collect_text(&el.children);
    assert_eq!(
        text.trim(),
        "https://demo.bext.dev/examples/not-found-file",
        "JSX text should be the URL only — no leaked `</code>`"
    );
    assert_eq!(
        count_element_children(&el.children),
        0,
        "<code> should have no element children"
    );
}

#[test]
fn jsx_text_url_followed_by_sibling_element_keeps_correct_nesting() {
    // The line comment swallowed the rest of the line including `</code></p>`,
    // causing the next `<p>` sibling to be parsed as a child of the broken
    // `<code>`. Verify each `<p>` is a direct child of the outer `<div>`.
    let src = r#"
const x = <div>
  <p>A: <code>https://example.com/foo</code></p>
  <p>B: <code>http_no_slashes/example.com/foo</code></p>
  <p>C: with question? <code>x?y</code></p>
</div>;
"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let div = jsx_root_element(&sf);
    let p_children: Vec<&JsxElement> = div
        .children
        .iter()
        .filter_map(|c| match c {
            JsxChild::Element(e) => match &e.kind {
                ExprKind::JsxElement(el) => Some(el.as_ref()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        p_children.len(),
        3,
        "<div> should have exactly 3 <p> children, got {}",
        p_children.len()
    );
    for p in &p_children {
        let nested = count_element_children(&p.children);
        assert_eq!(
            nested, 1,
            "each <p> should contain exactly one <code>, got {}",
            nested
        );
    }
}

#[test]
fn jsx_text_with_block_comment_chars_keeps_them_as_text() {
    // `/* */` in JSX text is literal — the scanner treats it as a block
    // comment (skipping it as trivia), but the slice from start..text_end
    // includes the bytes verbatim and `</p>` after still closes properly.
    let src = r#"const x = <p>foo /* bar */ baz</p>;"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let el = jsx_root_element(&sf);
    let text = collect_text(&el.children);
    assert!(
        text.contains("/* bar */"),
        "JSX text should preserve `/* bar */` literally, got {:?}",
        text
    );
    assert!(
        text.contains("foo") && text.contains("baz"),
        "JSX text should preserve text on both sides of comment-like chars, got {:?}",
        text
    );
}

#[test]
fn jsx_text_with_unterminated_block_comment_recovers_to_close_tag() {
    // `/*` with no `*/` would have the scanner consume to EOF as a block
    // comment, dropping every closing tag and sibling. Byte-scan must still
    // find the literal `<` of `</p>` and re-tokenize the gap.
    let src = r#"const x = <p>before /* unterminated</p>;"#;
    let sf = parse_tsx_src(src);
    let el = jsx_root_element(&sf);
    let text = collect_text(&el.children);
    assert!(
        text.contains("before") && text.contains("/*"),
        "JSX text should preserve `before /*` literally, got {:?}",
        text
    );
    assert_eq!(
        count_element_children(&el.children),
        0,
        "<p> with unterminated /* should still close cleanly with no element children"
    );
}

#[test]
fn mismatched_jsx_closing_names_report_the_closing_name() {
    for (src, opening, closing) in [
        ("const x = <div></span>;", "div", "span"),
        ("const x = <Foo . Bar></Foo.Baz>;", "Foo . Bar", "Foo.Baz"),
        ("const x = <svg:path></svg:g>;", "svg:path", "svg:g"),
    ] {
        let sf = parse_tsx_src(src);
        assert_eq!(sf.diagnostics.len(), 1, "{src}: {:?}", sf.diagnostics);
        let diagnostic = &sf.diagnostics[0];
        assert_eq!(diagnostic.code, 17002);
        assert_eq!(
            diagnostic.message,
            format!("Expected corresponding JSX closing tag for '{opening}'.")
        );
        let start = src.rfind(closing).unwrap() as u32;
        assert_eq!(
            diagnostic.span,
            Some(Span::new(start, start + closing.len() as u32))
        );
    }
    for src in [
        "const x = <Foo . Bar></Foo.Bar>;",
        "const x = <svg : path></svg:path>;",
        "const x = <this.Widget></this . Widget>;",
        "const x = <my-widget></my-widget>;",
    ] {
        assert!(parse_tsx_src(src).diagnostics.is_empty(), "{src}");
    }
    let escaped = parse_tsx_src(r"const x = <C\u006fmponent></Component>;");
    assert_eq!(
        escaped
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [17021]
    );
}

#[test]
fn missing_inner_jsx_close_leaves_parent_close_and_following_statement() {
    for src in [
        "const x = <div><span></div>; const after = 1;",
        "const x = <Foo.Bar><span></Foo . Bar>; const after = 1;",
        "const x = <my-widget><span></my-widget>; const after = 1;",
        "const x = <svg:path><span></svg : path>; const after = 1;",
    ] {
        let sf = parse_tsx_src(src);
        assert_eq!(sf.diagnostics.len(), 1, "{src}: {:?}", sf.diagnostics);
        assert_eq!(sf.diagnostics[0].code, 17008);
        let start = src.find("span").unwrap() as u32;
        assert_eq!(sf.diagnostics[0].span, Some(Span::new(start, start + 4)));
        assert_eq!(sf.statements.len(), 2, "{src}");
        let outer = jsx_root_element(&sf);
        assert_eq!(outer.children.len(), 1);
        let JsxChild::Element(inner) = &outer.children[0] else {
            panic!("expected inner element");
        };
        assert_eq!(inner.span.end, src.find("</").unwrap() as u32);
    }
}

#[test]
fn mismatched_fragment_close_retains_the_name_for_expression_recovery() {
    let src = "<>hi</div>value; const after = 1;";
    let sf = parse_tsx_src(src);
    assert_eq!(
        sf.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        [17015]
    );
    let start = src.find("div").unwrap() as u32;
    assert_eq!(sf.diagnostics[0].span, Some(Span::new(start, start + 3)));
    assert_eq!(sf.statements.len(), 3);
    for source in ["const x = <></>>1;", "const x = <></>=1;"] {
        assert!(parse_tsx_src(source).diagnostics.is_empty(), "{source}");
    }
}

#[test]
fn unclosed_jsx_elements_report_opening_names_and_one_missing_delimiter() {
    for (src, name) in [
        ("const x = <A>", "A"),
        ("const x = <   Foo . Bar >hello", "Foo . Bar"),
        ("const x = <svg:path> // trailing\n", "svg:path"),
    ] {
        let sf = parse_tsx_src(src);
        assert_eq!(sf.diagnostics.len(), 2, "{src}: {:?}", sf.diagnostics);
        let missing_tag = &sf.diagnostics[0];
        assert_eq!(missing_tag.code, 17008);
        assert_eq!(
            missing_tag.message,
            format!("JSX element '{name}' has no corresponding closing tag.")
        );
        let start = src.find(name).unwrap() as u32;
        assert_eq!(
            missing_tag.span,
            Some(Span::new(start, start + name.len() as u32))
        );
        let delimiter = &sf.diagnostics[1];
        assert_eq!(delimiter.code, 1005);
        assert_eq!(delimiter.message, "'</' expected.");
        assert_eq!(
            delimiter.span,
            Some(Span::new(src.len() as u32, src.len() as u32))
        );
        assert!(matches!(
            jsx_root_element(&sf).name.kind,
            ExprKind::Ident(_) | ExprKind::Member(_)
        ));
    }
}

#[test]
fn unclosed_nested_jsx_preserves_children_and_reports_each_opening() {
    let src = "const x = <A><B>tail";
    let sf = parse_tsx_src(src);
    assert_eq!(
        sf.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        [17008, 1005, 17008]
    );
    assert_eq!(sf.diagnostics[0].span, Some(Span::new(14, 15)));
    assert_eq!(sf.diagnostics[2].span, Some(Span::new(11, 12)));
    let outer = jsx_root_element(&sf);
    let JsxChild::Element(child) = &outer.children[0] else {
        panic!("nested element must be retained");
    };
    let ExprKind::JsxElement(inner) = &child.kind else {
        panic!("expected nested JSX element");
    };
    assert_eq!(collect_text(&inner.children), "tail");
}

#[test]
fn unclosed_jsx_fragments_report_the_opening_fragment_range() {
    for (src, expected_codes, fragment_index, span) in [
        ("const x = <>", vec![17014, 1005], 0, Span::new(9, 12)),
        (
            "const x = <><A>tail",
            vec![17008, 1005, 17014],
            2,
            Span::new(9, 12),
        ),
        (
            "const x = <A><>",
            vec![17014, 1005, 17008],
            0,
            Span::new(13, 15),
        ),
    ] {
        let sf = parse_tsx_src(src);
        assert_eq!(
            sf.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            expected_codes,
            "{src}"
        );
        assert_eq!(sf.diagnostics[fragment_index].span, Some(span), "{src}");
        assert_eq!(
            sf.diagnostics[fragment_index].message,
            "JSX fragment has no corresponding closing tag."
        );
    }
    for src in [
        "const x = <A/>;",
        "const x = <>hello</>",
        "const x = <A>text</A>",
    ] {
        assert!(parse_tsx_src(src).diagnostics.is_empty(), "{src}");
    }
}

#[test]
fn jsx_unclosed_expression_container_leaves_closing_tag_for_parent() {
    let src = r#"function foo() {
    var x = <div>  { </div>
}
var y = { a: 1 };
"#;
    let sf = parse_tsx_src(src);
    assert_eq!(
        sf.statements.len(),
        2,
        "following variable must stay top-level"
    );
    let StmtKind::FnDecl(function) = &sf.statements[0].kind else {
        panic!("expected function declaration");
    };
    let body = function.body.as_ref().expect("expected function body");
    let StmtKind::Var(var_stmt) = &body[0].kind else {
        panic!("expected JSX variable declaration");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected JSX initializer");
    let ExprKind::JsxElement(element) = &init.kind else {
        panic!("expected JSX element");
    };
    assert!(
        element
            .children
            .iter()
            .any(|child| matches!(child, JsxChild::Expression(None, _))),
        "unclosed container should recover as an empty JSX expression"
    );
    assert!(matches!(sf.statements[1].kind, StmtKind::Var(_)));
}

#[test]
fn adjacent_jsx_elements_recover_as_comma_expression() {
    let sf = parse_tsx_src("var x = <div></div><span></span>;\n");
    let StmtKind::Var(var_stmt) = &sf.statements[0].kind else {
        panic!("expected variable statement");
    };
    let init = var_stmt.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::Comma(elements) = &init.kind else {
        panic!("expected adjacent JSX to recover as comma expression");
    };
    assert_eq!(elements.len(), 2);
    assert!(matches!(elements[0].kind, ExprKind::JsxElement(_)));
    assert!(matches!(elements[1].kind, ExprKind::JsxElement(_)));
}

#[test]
fn jsx_text_with_line_comment_chars_keeps_them_as_text() {
    // Plain `//` inside JSX text (no URL) — the scanner runs a line comment
    // to the next newline, which would swallow the closing tag if on the
    // same line. We need to recover the closing tag.
    let src = "const x = <p>// this is text not a comment</p>;";
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let el = jsx_root_element(&sf);
    let text = collect_text(&el.children);
    assert!(
        text.contains("//") && text.contains("not a comment"),
        "JSX text should preserve `// ... not a comment` literally, got {:?}",
        text
    );
}

#[test]
fn jsx_expression_container_still_treats_comments_as_real_comments() {
    // Defense: my fix is scoped to text-mode children. Inside `{...}` we
    // run the regular expression parser, where `// foo` IS a real comment.
    // Don't regress this.
    let src = r#"const x = <p>{ /* keep this */ value }</p>;"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let el = jsx_root_element(&sf);
    let expr_child = el
        .children
        .iter()
        .find(|c| matches!(c, JsxChild::Expression(Some(_), _)))
        .expect("expected an expression child");
    let JsxChild::Expression(Some(expr), _) = expr_child else {
        unreachable!()
    };
    assert!(
        matches!(&expr.kind, ExprKind::Ident(name) if name == "value"),
        "expression inside {{...}} should resolve to identifier `value` (comment skipped), got {:?}",
        expr.kind
    );
}

#[test]
fn jsx_text_two_sibling_urls_back_to_back() {
    // After splicing recovered tokens for the first `</code>`, the next
    // sibling text region must hit the same recovery path independently.
    let src = r#"
const x = <div>
  <a href="x">https://one.example.com/a</a>
  <a href="y">https://two.example.com/b</a>
</div>;
"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let div = jsx_root_element(&sf);
    let a_children: Vec<&JsxElement> = div
        .children
        .iter()
        .filter_map(|c| match c {
            JsxChild::Element(e) => match &e.kind {
                ExprKind::JsxElement(el) => Some(el.as_ref()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        a_children.len(),
        2,
        "expected two <a> children, got {}",
        a_children.len()
    );
    let first = collect_text(&a_children[0].children);
    let second = collect_text(&a_children[1].children);
    assert_eq!(first.trim(), "https://one.example.com/a");
    assert_eq!(second.trim(), "https://two.example.com/b");
}

#[test]
fn jsx_text_url_inside_fragment() {
    // Same defense should apply to fragment children, which use the same
    // parse_jsx_children code path.
    let src = r#"const x = <><a>https://example.com/foo</a><b>after</b></>;"#;
    let sf = parse_tsx_src(src);
    assert!(
        sf.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        sf.diagnostics
    );
    let StmtKind::Var(v) = &sf.statements[0].kind else {
        panic!("expected var");
    };
    let init = v.declarations[0]
        .init
        .as_ref()
        .expect("expected initializer");
    let ExprKind::JsxFragment(frag) = &init.kind else {
        panic!("expected JSX fragment, got {:?}", init.kind);
    };
    let elements: Vec<&JsxElement> = frag
        .children
        .iter()
        .filter_map(|c| match c {
            JsxChild::Element(e) => match &e.kind {
                ExprKind::JsxElement(el) => Some(el.as_ref()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        elements.len(),
        2,
        "fragment should have <a> and <b> as siblings, got {}",
        elements.len()
    );
    assert_eq!(
        collect_text(&elements[0].children).trim(),
        "https://example.com/foo"
    );
}

#[test]
fn index_signature_type_predicate_tail_recovers_outside_interface() {
    let sf = parse_src("interface I { [index: number]: value is string; }\n");
    assert!(matches!(sf.statements[0].kind, StmtKind::InterfaceDecl(_)));
    assert!(
        sf.statements
            .iter()
            .skip(1)
            .any(|stmt| matches!(&stmt.kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::Ident(name) if name == "is"))),
        "the invalid predicate tail should remain available to statement recovery"
    );
}

#[test]
fn javascript_files_report_typescript_only_grammar_at_exact_spans() {
    fn diagnostics(source: &str) -> Vec<(u32, &str)> {
        let file = parse("test.js", source);
        file.diagnostics
            .iter()
            .map(|diagnostic| {
                let span = diagnostic.span.expect("parser diagnostic span");
                (
                    diagnostic.code,
                    &source[span.start as usize..span.end as usize],
                )
            })
            .collect()
    }

    assert_eq!(
        diagnostics("interface I {}\ntype T = string;\nenum E {}\nnamespace N {}"),
        vec![(8006, "I"), (8008, "T"), (8006, "E"), (8006, "N")]
    );
    assert_eq!(
        diagnostics("import x = require('x');\nexport = x;"),
        vec![(8002, "import x = require('x');"), (8003, "export = x;")]
    );
    assert_eq!(
        diagnostics("import type { A } from 'a';\nexport type { A };"),
        vec![
            (8006, "import type { A } from 'a';"),
            (8006, "export type { A };")
        ]
    );
    assert_eq!(
        diagnostics("import { type A } from 'a';\nexport { type A };"),
        vec![(8006, "type A"), (8006, "type A")]
    );
    assert_eq!(
        diagnostics("abstract class C<T> extends Base<U> implements I { public override value?; }"),
        vec![
            (8009, "abstract"),
            (8004, "T"),
            (8011, "U"),
            (8005, "implements I"),
            (8009, "public"),
            (8009, "override"),
            (8009, "?")
        ]
    );
    assert_eq!(
        diagnostics("var value: number; function f<T>(input?: string): void {}"),
        vec![
            (8010, "number"),
            (8004, "T"),
            (8009, "?"),
            (8010, "string"),
            (8010, "void")
        ]
    );
    assert_eq!(
        diagnostics("class C { constructor(public value: number) {} }"),
        vec![(8012, "public"), (8010, "number")]
    );
}

#[test]
fn mapped_type_parameter_accepts_contextual_keyword_tokens() {
    for name in [
        "as",
        "readonly",
        "object",
        "string",
        "number",
        "keyof",
        "unknown",
        "intrinsic",
    ] {
        let source = format!("type M = {{ [{name} in string]: 1 }};");
        let file = parse("mapped-contextual.ts", &source);
        assert!(
            file.diagnostics.is_empty(),
            "{source}: {:?}",
            file.diagnostics
        );
        let StmtKind::TypeAlias(alias) = &file.statements[0].kind else {
            panic!("{source}")
        };
        let TypeNodeKind::Mapped(mapped) = &alias.type_ann.kind else {
            panic!("expected mapped type: {source}")
        };
        assert_eq!(mapped.type_param.name, name);
    }
}

#[test]
fn recovered_namespace_names_report_name_errors_without_consuming_the_name() {
    for (source, code, text) in [
        ("namespace {}", 1437, "{"),
        ("namespace true {}", 2819, "true"),
        ("module true {}", 2819, "true"),
    ] {
        let file = parse("namespace-recovery.ts", source);
        let diagnostic = file
            .diagnostics
            .iter()
            .find(|d| d.code == code)
            .unwrap_or_else(|| panic!("{source}: {:?}", file.diagnostics));
        let span = diagnostic.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], text);
        assert!(
            file.statements.len() >= 2,
            "recovery consumed the following statement: {source}"
        );
    }
}

#[test]
fn missing_delimiters_retain_the_actual_opening_token() {
    for (source, opening, close) in [
        ("function f() { var x = 1;", '{', '}'),
        ("const x = { a: 1", '{', '}'),
        ("const x = [1, 2", '[', ']'),
        ("function f() { const x = [1", '[', ']'),
    ] {
        let file = parse("delimiters.ts", source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let d = &file.diagnostics[0];
        assert_eq!(d.code, 1005);
        assert_eq!(d.message, format!("'{close}' expected."));
        assert_eq!(
            d.span,
            Some(Span::new(source.len() as u32, source.len() as u32))
        );
        let related = d.related.as_ref().expect("opening delimiter location");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].code, 1007);
        assert_eq!(related[0].file_name.as_deref(), Some("delimiters.ts"));
        let start = source.rfind(opening).unwrap() as u32;
        assert_eq!(related[0].span, Some(Span::new(start, start + 1)));
    }
}

#[test]
fn missing_identifier_at_eof_precedes_trailing_trivia() {
    for suffix in ["", " ", "\n", " /* comment */"] {
        let source = format!("Object.{suffix}");
        let file = parse("identifier.ts", &source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        assert_eq!(file.diagnostics[0].code, 1003);
        assert_eq!(file.diagnostics[0].message, "Identifier expected.");
        assert_eq!(file.diagnostics[0].span, Some(Span::new(7, 7)));
    }
}

#[test]
fn expected_token_diagnostics_use_source_spelling() {
    for (source, message) in [
        ("if true) {}", "'(' expected."),
        ("let x: number 1;", "',' expected."),
    ] {
        let file = parse("expected-token.ts", source);
        assert!(
            file.diagnostics
                .iter()
                .any(|d| d.code == 1005 && d.message == message),
            "{source}: {:?}",
            file.diagnostics
        );
    }
}

#[test]
fn missing_types_report_type_expected() {
    for source in ["let x: ;", "type T = ;", "type T = "] {
        let file = parse_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, 1110, "{source}");
        assert_eq!(diagnostic.message, "Type expected.");
        let expected = source
            .find(';')
            .map(|p| Span::new(p as u32, p as u32 + 1))
            .unwrap_or(Span::new(source.len() as u32, source.len() as u32));
        assert_eq!(diagnostic.span, Some(expected), "{source}");
    }
}

#[test]
fn missing_qualified_type_name_uses_identifier_diagnostic() {
    for source in ["type T = A.;", "type T = A.   "] {
        let file = parse_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, 1003);
        assert_eq!(diagnostic.message, "Identifier expected.");
        assert_eq!(
            diagnostic.span,
            Some(Span::new(11, if source.ends_with(';') { 12 } else { 11 }))
        );
    }
}

#[test]
fn nonidentifier_object_keys_require_a_colon() {
    for source in [
        r#"const o = { "x" };"#,
        "const o = { 1 };",
        "const o = { [x] };",
    ] {
        let file = parse_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, 1005);
        assert_eq!(diagnostic.message, "':' expected.");
        let end = source.find('}').unwrap() as u32;
        assert_eq!(diagnostic.span, Some(Span::new(end, end + 1)));
    }
}

#[test]
fn jsx_missing_names_and_tag_terminators_use_grammar_diagnostics() {
    for (source, code, message, start, end) in [
        ("const o = <A /", 1005, "'>' expected.", 14, 14),
        ("const o = <A. />", 1003, "Identifier expected.", 14, 15),
        ("const o = <a foo: />", 1003, "Identifier expected.", 18, 19),
    ] {
        let file = parse_tsx_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, code);
        assert_eq!(diagnostic.message, message);
        assert_eq!(diagnostic.span, Some(Span::new(start, end)));
    }
}

#[test]
fn missing_types_preserve_the_surrounding_delimiters() {
    for source in [
        "let x: ; let y = 1;",
        "function f(x: ) {}",
        "type T = { a: };",
        "type T = [string, , number];",
        "type T = A<, string>;",
    ] {
        let file = parse_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        assert_eq!(file.diagnostics[0].code, 1110, "{source}");
        if source.starts_with("let x") {
            assert_eq!(file.statements.len(), 2);
        }
    }
}

#[test]
fn keyword_type_references_and_jsdoc_all_are_not_missing_types() {
    for source in [
        "type T = break;",
        "class C { f(x: break) {} }",
        "let q = <const>10;",
        "type T = *;",
        "class C { a: *; }",
    ] {
        assert_no_errors(source);
    }
}

#[test]
fn invalid_index_type_preserves_private_binding_recovery() {
    let source = "const value: C[#field] = 3;";
    let file = parse_src(source);
    let StmtKind::Var(var) = &file.statements[0].kind else {
        panic!("expected variable statement")
    };
    assert_eq!(var.declarations.len(), 2);
    assert!(matches!(
        &var.declarations[0].type_ann.as_ref().unwrap().kind,
        TypeNodeKind::Array(_)
    ));
    assert!(matches!(&var.declarations[1].name.kind, PatKind::Ident(name) if name == "#field"));
    assert!(file
        .diagnostics
        .iter()
        .any(|d| d.code == 1005 && d.message == "']' expected."));
    assert!(!file.diagnostics.iter().any(|d| d.code == 1110));
}

#[test]
fn unmatched_delimiter_assignment_recovers_the_rhs_statement() {
    for source in ["]=3;", ")=3;", "}=3;", "]=3; // keep"] {
        let file = parse_src(source);
        assert_eq!(
            file.diagnostics.len(),
            2,
            "{source}: {:?}",
            file.diagnostics
        );
        for (index, diagnostic) in file.diagnostics.iter().enumerate() {
            assert_eq!(diagnostic.code, 1128);
            assert_eq!(diagnostic.message, "Declaration or statement expected.");
            assert_eq!(
                diagnostic.span,
                Some(Span::new(index as u32, index as u32 + 1))
            );
        }
        assert_eq!(file.statements.len(), 1);
        assert_eq!(file.statements[0].span, Span::new(2, 4));
        assert!(
            matches!(&file.statements[0].kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::NumLit(value) if value == "3"))
        );
    }
}

#[test]
fn jsx_names_accept_trailing_repeated_and_numeric_hyphens() {
    for name in [
        "a-",
        "a--",
        "a-1",
        "A-B",
        "A--1",
        "ns-:local--1",
        "a-\u{0301}",
    ] {
        let source = format!("const x = <{name} data--1='' trailing- />;");
        let file = parse_tsx_src(&source);
        assert!(
            file.diagnostics.is_empty(),
            "{source}: {:?}",
            file.diagnostics
        );
        let StmtKind::Var(var) = &file.statements[0].kind else {
            panic!("expected variable")
        };
        let ExprKind::JsxSelfClosing(element) = &var.declarations[0].init.as_ref().unwrap().kind
        else {
            panic!("expected JSX")
        };
        assert!(matches!(&element.name.kind, ExprKind::Ident(actual) if actual.as_str() == name));
        assert!(
            matches!(&element.attributes[0], JsxAttribute::Normal { name, .. } if name == "data--1")
        );
        assert!(
            matches!(&element.attributes[1], JsxAttribute::Normal { name, .. } if name == "trailing-")
        );
    }
}

#[test]
fn jsx_unicode_escape_diagnostics_cover_the_complete_name_part() {
    for (source, spelling) in [
        (r"const x = <\u0061-b />;", r"\u0061-b"),
        (r"const x = <a-\u{0062} />;", r"a-\u{0062}"),
        (r"const x = <Ns.\u0062 />;", r"\u0062"),
        (r"const x = <ns:\u0062- />;", r"\u0062-"),
        (r"const x = <a data-\u0062 />;", r"data-\u0062"),
        (r"const x = <a-\u0032 />;", r"a-\u0032"),
    ] {
        let file = parse_tsx_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, 17021);
        assert_eq!(
            diagnostic.message,
            "Unicode escape sequence cannot appear here."
        );
        let span = diagnostic.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], spelling);
    }
}

#[test]
fn jsx_name_boundaries_preserve_trivia_and_straddling_tokens() {
    for (source, name, diagnostic) in [
        ("const x = <a- b />; after;", "a-", false),
        ("const x = <a -b />; after;", "a", true),
        ("const x = <a-1.2 />; after;", "a-1", true),
    ] {
        let file = parse_tsx_src(source);
        assert_eq!(!file.diagnostics.is_empty(), diagnostic, "{source}");
        let StmtKind::Var(var) = &file.statements[0].kind else {
            panic!("expected variable")
        };
        let ExprKind::JsxSelfClosing(element) = &var.declarations[0].init.as_ref().unwrap().kind
        else {
            panic!("expected JSX")
        };
        assert!(matches!(&element.name.kind, ExprKind::Ident(actual) if actual.as_str() == name));
        assert!(
            matches!(&file.statements.last().unwrap().kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::Ident(name) if name == "after"))
        );
        if let Some(start) = source.find(".2") {
            let diagnostic = &file.diagnostics[0];
            assert_eq!(diagnostic.code, 1003);
            assert_eq!(
                diagnostic.span,
                Some(Span::new(start as u32, start as u32 + 2))
            );
        }
    }
}

#[test]
fn jsx_this_tags_are_expressions_unless_extended_or_namespaced() {
    for (name, is_this, member, escaped) in [
        ("this", true, false, false),
        ("this.Component", true, true, false),
        (r"\u0074his", true, false, true),
        (r"\u0074his.Component", true, true, true),
        ("this-tag", false, false, false),
        ("this:tag", false, false, false),
        (r"\u0074his-tag", false, false, true),
        (r"\u0074his:tag", false, false, true),
    ] {
        let source = format!("<{name} />;");
        let file = parse_tsx_src(&source);
        assert_eq!(file.diagnostics.len(), usize::from(escaped), "{source}");
        if escaped {
            assert_eq!(file.diagnostics[0].code, 17021);
        }
        let StmtKind::Expr(expr) = &file.statements[0].kind else {
            panic!("expected expression")
        };
        let ExprKind::JsxSelfClosing(element) = &expr.kind else {
            panic!("expected JSX")
        };
        let root = if member {
            let ExprKind::Member(member) = &element.name.kind else {
                panic!("expected member")
            };
            assert_eq!(member.property, "Component");
            &member.object
        } else {
            &element.name
        };
        assert_eq!(matches!(root.kind, ExprKind::This), is_this, "{source}");
        if !is_this {
            assert!(matches!(&root.kind, ExprKind::Ident(actual) if actual == name));
        }
    }
}

#[test]
fn class_const_modifiers_report_the_member_span() {
    let source = "class C { static const H = 1; const constructor() {} const get x() { return 1; } const [\"p\"] = 1; const m() {} } let D = class { const a = 4; };";
    let file = parse_src(source);
    let errors: Vec<_> = file
        .diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.code, 1248, "{diagnostic:?}");
            assert_eq!(
                diagnostic.message,
                "A class member cannot have the 'const' keyword."
            );
            let span = diagnostic.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(errors, ["H", "const constructor", "x", "[\"p\"]", "m", "a"]);
    assert_eq!(file.statements.len(), 2);
}

#[test]
fn const_class_member_names_and_type_parameters_remain_valid() {
    let file = parse_src("class C<const T> { const = 1; static const() {} method<const U>() {} } const enum E { A } ");
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
}

#[test]
fn signature_defaults_report_complete_parameter_and_binding_spans() {
    let source = "function f(x: number = 1); class C { constructor(x = 1); m(y = 2); } interface I { (a = 1): void; new(b = 2): C; m(c = 3): void; } type F = ({ first = 0 }: {first?: number}, [item = 1]: number[], x = 2) => void;";
    let file = parse_src(source);
    let spans: Vec<_> = file
        .diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.code, 2371, "{diagnostic:?}");
            let span = diagnostic.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        spans,
        [
            "x: number = 1",
            "x = 1",
            "y = 2",
            "a = 1",
            "b = 2",
            "c = 3",
            "first",
            "item",
            "x = 2"
        ]
    );
}

#[test]
fn implementation_parameter_defaults_remain_valid() {
    let file = parse_src("function f(x = 1, {first = 0} = {}) {} class C { constructor(x = 1) {} m(y = 2) {} } const arrow = ({first = 0} = {}) => first; const fn = function(x = 1) {}; ");
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
}

#[test]
fn index_signature_initializers_are_consumed_and_diagnosed() {
    let source = "class C { [a: number = 1]: number; } interface I { [b: string = 'x']: number; }";
    let file = parse_src(source);
    let actual: Vec<_> = file
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (1020, "a"),
            (2371, "a: number = 1"),
            (1020, "b"),
            (2371, "b: string = 'x'")
        ]
    );
    assert_eq!(file.statements.len(), 2);
}

#[test]
fn property_signature_initializers_keep_context_and_recover_members() {
    let source = "interface I { x: number = 5; nested: { y: number = 6 }; next: string; } type T = { p: number = 7; next: string; };";
    let file = parse_src(source);
    let actual: Vec<_> = file
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(actual, [(1246, "5"), (1247, "6"), (1247, "7")]);
    let StmtKind::InterfaceDecl(interface) = &file.statements[0].kind else {
        panic!("expected interface");
    };
    assert_eq!(interface.members.len(), 3);
    let StmtKind::TypeAlias(alias) = &file.statements[1].kind else {
        panic!("expected type alias");
    };
    let TypeNodeKind::TypeLit(members) = &alias.type_ann.kind else {
        panic!("expected type literal");
    };
    assert_eq!(members.len(), 2);
}

#[test]
fn parameter_grammar_reports_first_rule_with_precise_spans() {
    for (source, code, text) in [
        ("function f(...rest: any[], x: number) {}", 1014, "..."),
        ("const f = (...rest?: any[]) => {};", 1047, "?"),
        ("function f(...rest = []) {}", 1048, "rest"),
        ("function f(x?: number = 1) {}", 1015, "x"),
        ("type F = (x?: string, y: number) => void;", 1016, "y"),
        ("function f(...rest?: any[], x: number) {}", 1014, "..."),
    ] {
        let file = parse_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, code, "{source}");
        let span = diagnostic.span.unwrap();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            text,
            "{source}"
        );
    }
}

#[test]
fn valid_optional_default_and_rest_parameter_order_is_preserved() {
    let file = parse_src("function f(x = 1, y: number, z?: string, ...rest: any[]) {} const g = (x?: number, y = 1) => {}; type T = (x?: number, ...rest: any[]) => void;");
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
}

#[test]
fn class_modifier_grammar_reports_first_error_at_the_modifier() {
    for (source, code, text) in [
        ("class C {static public x: number;}", 1029, "public"),
        ("class C {readonly private x: number;}", 1029, "private"),
        ("class C {public private x: number;}", 1028, "private"),
        ("class C {readonly readonly x: number;}", 1030, "readonly"),
        (
            "abstract class C {static abstract m(): void;}",
            1243,
            "abstract",
        ),
        (
            "abstract class C {private abstract x: number;}",
            1243,
            "abstract",
        ),
        (
            "abstract class C {abstract static x: number;}",
            1243,
            "static",
        ),
        ("class C {readonly m() {}}", 1024, "readonly"),
        ("class C {declare m(): void;}", 1031, "declare"),
        ("class C {export x = 1;}", 1031, "export"),
        ("class C {abstract m(): void;}", 1244, "abstract"),
        ("class C {abstract x: number;}", 1253, "abstract"),
        ("class C {private #x: number;}", 18010, "private"),
        ("class C {declare #x: number;}", 18019, "declare"),
        ("class C {accessor m() {}}", 1275, "accessor"),
        ("class C {readonly accessor x: number;}", 1243, "accessor"),
        ("class C {accessor readonly x: number;}", 1243, "readonly"),
        ("class C {readonly override x: number;}", 1029, "override"),
        ("class C {async x: number;}", 1042, "async"),
        ("class C {static constructor() {}}", 1089, "static"),
        (
            "abstract class C {abstract constructor();}",
            1242,
            "abstract",
        ),
        ("class C {private [x: string]: number;}", 1071, "private"),
        (
            "abstract class C {async abstract m(): void;}",
            1243,
            "async",
        ),
        ("declare class C {async m(): void;}", 1040, "async"),
        ("class C {async get x() {return 1;}}", 1042, "async"),
    ] {
        let file = parse_src(source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        let diagnostic = &file.diagnostics[0];
        assert_eq!(diagnostic.code, code, "{source}");
        let span = diagnostic.span.unwrap();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            text,
            "{source}"
        );
    }
}

#[test]
fn class_modifier_names_and_syntax_recovery_are_preserved() {
    let file = parse_src("class C {public public() {} static static() {} readonly readonly: string;} abstract class D {public abstract m(): void; protected static readonly x: number;}");
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let source = "class C {public public x; static static y;}";
    let file = parse_src(source);
    assert_eq!(file.diagnostics.len(), 1, "{:?}", file.diagnostics);
    assert_eq!(file.diagnostics[0].code, 1434);
    let span = file.diagnostics[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "static");
    let StmtKind::ClassDecl(class) = &file.statements[0].kind else {
        panic!("expected class")
    };
    assert_eq!(class.members.len(), 3);
}

#[test]
fn ambient_class_properties_reject_initializers_with_readonly_inference_exception() {
    for (file_name, source, expected) in [
        ("a.ts", "class C { declare x = 1; }", vec![(1039, "1")]),
        ("a.ts", "declare class C { x = 'a'; }", vec![(1039, "'a'")]),
        (
            "a.d.ts",
            "class C { x = 1 + 2; }",
            vec![(1039, "1 + 2"), (1046, "class")],
        ),
        (
            "a.ts",
            "declare namespace N { class C { x = 1; } }",
            vec![(1039, "1")],
        ),
        (
            "a.ts",
            "class C { declare readonly x: number = 1; }",
            vec![(1039, "1")],
        ),
        (
            "a.ts",
            "class C { export declare x = 1; }",
            vec![(1031, "export")],
        ),
        (
            "a.ts",
            "class C { async declare x = 1; }",
            vec![(1040, "declare")],
        ),
        ("a.ts", "class C { x = 1; readonly y: number = 2; }", vec![]),
        (
            "a.ts",
            "class C { declare x: number; declare readonly y = 1; }",
            vec![],
        ),
        (
            "a.d.ts",
            "declare class C { readonly x = 1; readonly y = -1; readonly z = 'a'; }",
            vec![],
        ),
    ] {
        let file = parse(file_name, source);
        let actual: Vec<_> = file
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.span.unwrap();
                (d.code, &source[span.start as usize..span.end as usize])
            })
            .collect();
        assert_eq!(actual, expected, "{file_name}: {source}");
    }
}

#[test]
fn constructor_type_grammar_preserves_ranges_and_error_precedence() {
    for (source, expected) in [
        ("class C { constructor<T>() {} }", vec![(1092, "T")]),
        (
            "class C { constructor< /*a*/ T, U, >(): void {} }",
            vec![(1092, "T, U,")],
        ),
        (
            "class C { constructor<T extends Array<number>>() {} }",
            vec![(1092, "T extends Array<number>")],
        ),
        (
            "class C { constructor(): number {} }",
            vec![(1093, "number")],
        ),
        ("declare class C { constructor(): C; }", vec![(1093, "C")]),
        (
            "class C { constructor<>() {} }",
            vec![(1098, "<>"), (1092, "")],
        ),
        (
            "class C { static constructor<T>(): void {} }",
            vec![(1092, "T"), (1089, "static")],
        ),
        (
            "class C { constructor(x: number) {} method<T>(): void {} }",
            vec![],
        ),
        ("interface I { new<T>(): T; }", vec![]),
    ] {
        let file = parse_src(source);
        let actual: Vec<_> = file
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.span.unwrap();
                (d.code, &source[span.start as usize..span.end as usize])
            })
            .collect();
        assert_eq!(actual, expected, "{source}");
    }
}

#[test]
fn generic_argument_lists_reject_empty_lists_and_trailing_commas() {
    for (source, expected) in [
        ("f<>();", vec![(1099, "<>")]),
        ("new C< /*empty*/ >();", vec![(1099, "< /*empty*/ >")]),
        ("type T = C<>;", vec![(1099, "<>")]),
        ("const x = f<>;", vec![(1099, "<>")]),
        ("f<string,>();", vec![(1009, ",")]),
        ("type T = C<string, /*a*/ >;", vec![(1009, ",")]),
        ("new C<Array<string>,>();", vec![(1009, ",")]),
        ("tag<string,>`x`;", vec![(1009, ",")]),
        ("type T = C<D<>>;", vec![(1099, "<>")]),
        ("type T = C<D<string,>>;", vec![(1009, ",")]),
        ("class C<T,> {} function f<T,>() {} f<string>();", vec![]),
        ("const x = a < b, y = c > d;", vec![]),
    ] {
        let file = parse_src(source);
        let actual: Vec<_> = file
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.span.unwrap();
                (d.code, &source[span.start as usize..span.end as usize])
            })
            .collect();
        assert_eq!(actual, expected, "{source}");
    }
}

#[test]
fn index_signature_parameters_use_index_specific_grammar() {
    for (member, expected) in [
        ("[];", vec![(1096, "[];")]),
        ("[a: string, b: number]: any;", vec![(1096, "a")]),
        ("[x,]: any;", vec![(1025, ","), (1022, "x")]),
        ("[...x: string,]: any;", vec![(1025, ","), (1017, "...")]),
        ("[public x?: string]: any;", vec![(1018, "x")]),
        (
            "[x?: string = 'a']: any;",
            vec![(1019, "?"), (2371, "x?: string = 'a'")],
        ),
        (
            "[x: string = 'a']: any;",
            vec![(1020, "x"), (2371, "x: string = 'a'")],
        ),
        ("[x: string]: any;", vec![]),
    ] {
        for prefix in ["class C", "interface I", "type T ="] {
            let source = format!("{prefix} {{ {member} }}");
            let file = parse_src(&source);
            let actual: Vec<_> = file
                .diagnostics
                .iter()
                .map(|d| {
                    let span = d.span.unwrap();
                    (d.code, &source[span.start as usize..span.end as usize])
                })
                .collect();
            assert_eq!(actual, expected, "{source}");
        }
    }
}

#[test]
fn index_signatures_preserve_readonly_and_computed_property_disambiguation() {
    let file = parse_src("interface I { readonly [key: string]: number; [key]: number; [a + b]: number; [a ? b : c]: number; }");
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let StmtKind::InterfaceDecl(interface) = &file.statements[0].kind else {
        panic!("expected interface")
    };
    let TypeMemberKind::IndexSig(index) = &interface.members[0].kind else {
        panic!("expected index signature")
    };
    assert_ne!(index.modifiers & MOD_READONLY, 0);
    assert_eq!(index.params.len(), 1);
    assert!(interface.members[1..]
        .iter()
        .all(|member| { matches!(member.kind, TypeMemberKind::PropertySig(_)) }));
}
