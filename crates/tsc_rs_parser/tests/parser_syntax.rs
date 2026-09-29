use tsc_rs_parser::parse;

#[test]
fn exponentiation_rejects_unparenthesized_unary_left_operands() {
    let source = r#"-1 ** 2;
+1 ** 2;
typeof value ** 2;
void value ** 2;
delete object.key ** 2;
~value ** 2;
!value ** 2;
(-1) ** 2;
2 ** -1;
2 ** -1 ** 2;
async function exponentiationWithAwait() {
    await value ** 2;
    (await value) ** 2;
    2 ** await value;
    2 ** await value ** 2;
}
"#;

    let parsed = parse("exponentiation.ts", source);
    let diagnostics: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 17006)
        .collect();

    assert_eq!(diagnostics.len(), 10);
    let highlighted_expressions: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic
                .span
                .expect("TS17006 should identify the unary expression");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        highlighted_expressions,
        [
            "-1",
            "+1",
            "typeof value",
            "void value",
            "delete object.key",
            "~value",
            "!value",
            "-1",
            "await value",
            "await value"
        ]
    );
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == 17006),
        "unexpected diagnostics: {:?}",
        parsed.diagnostics
    );
}

#[test]
fn variable_declarations_preserve_typescript_full_start_offsets() {
    let parsed = parse("variables.ts", "var a,   b;");
    let tsc_rs_ast::StmtKind::Var(statement) = &parsed.statements[0].kind else {
        panic!(
            "expected variable statement, got {:?}",
            parsed.statements[0]
        );
    };

    assert_eq!(statement.declarations.len(), 2);
    assert_eq!(statement.declarations[0].full_start, 3);
    assert_eq!(statement.declarations[0].name.span.start, 4);
    assert_eq!(statement.declarations[1].full_start, 6);
    assert_eq!(statement.declarations[1].name.span.start, 9);
}

#[test]
fn destructuring_bindings_preserve_binding_element_full_starts() {
    let source = "const { alpha, /* object */ key: renamed = 1, ...rest } = source;\n\
let [first, , /* array */ second = 2, ...tail] = values;\n\
var { outer: { inner, alias: deep }, list: [nested, ...nestedRest] } = source;";
    let parsed = parse("bindings.ts", source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let declarations: Vec<_> = parsed
        .statements
        .iter()
        .map(|statement| {
            let tsc_rs_ast::StmtKind::Var(var_statement) = &statement.kind else {
                panic!("expected variable statement, got {statement:?}");
            };
            &var_statement.declarations[0]
        })
        .collect();

    let observed: Vec<Vec<(&str, usize)>> = declarations
        .iter()
        .map(|declaration| {
            declaration
                .binding_name_full_starts
                .iter()
                .map(|binding| {
                    (
                        &source[binding.name_span.start as usize..binding.name_span.end as usize],
                        binding.full_start as usize,
                    )
                })
                .collect()
        })
        .collect();

    assert_eq!(
        observed[0],
        [
            ("alpha", source.find("{ alpha").unwrap() + 1),
            ("renamed", source.find(", /* object */ key").unwrap() + 1),
            ("rest", source.find(", ...rest").unwrap() + 1),
        ]
    );
    assert_eq!(
        observed[1],
        [
            ("first", source.find("[first").unwrap() + 1),
            ("second", source.find(", /* array */ second").unwrap() + 1),
            ("tail", source.find(", ...tail").unwrap() + 1),
        ]
    );
    assert_eq!(
        observed[2],
        [
            ("inner", source.find("{ inner").unwrap() + 1),
            ("deep", source.find(", alias: deep").unwrap() + 1),
            ("nested", source.find("[nested").unwrap() + 1),
            ("nestedRest", source.find(", ...nestedRest").unwrap() + 1),
        ]
    );
}
