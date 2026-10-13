use tsc_rs_parser::parse;

#[test]
fn missing_commas_after_rest_parameters_report_the_following_token() {
    // The parser retains grammar diagnostics for the compiler to filter when
    // syntax errors are present. TypeScript 6.0.3 only reports the missing
    // separator in these cases, not the recovered parameter's TS1014.
    for (file, source, token) in [
        (
            "input.ts",
            "class C { constructor(...public rest: string[]) {} }",
            "rest",
        ),
        ("input.ts", "function f(...args next) {}", "next"),
        ("input.js", "function f(...args next) {}", "next"),
        (
            "input.tsx",
            "function f(...args next) { return <span />; }",
            "next",
        ),
        ("input.ts", "function f(...args [next]) {}", "["),
        (
            "input.ts",
            "function f(...args /*missing*/ next: number) {}",
            "next",
        ),
        ("input.ts", "function f(...args\nnext) {}", "next"),
        ("input.ts", "function f(...args public next) {}", "public"),
    ] {
        let parsed = parse(file, source);
        let start = source.find(token).unwrap() as u32;
        let rest = source.find("...").unwrap() as u32;
        let diagnostics: Vec<_> = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let span = diagnostic.span.unwrap();
                (
                    diagnostic.code,
                    span.start,
                    span.end,
                    diagnostic.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            diagnostics,
            [
                (1005, start, start + token.len() as u32, "',' expected."),
                (
                    1014,
                    rest,
                    rest + 3,
                    "A rest parameter must be last in a parameter list.",
                ),
            ],
            "{file}: {source}"
        );
    }
}

#[test]
fn missing_rest_parameter_separator_preserves_recovered_parameters_and_body() {
    let source = "function f(...args next: string) { return next; }";
    let parsed = parse("input.ts", source);
    let tsc_rs_ast::StmtKind::FnDecl(function) = &parsed.statements[0].kind else {
        panic!("expected function declaration");
    };
    assert_eq!(function.params.len(), 2);
    assert!(function.params[0].dotdotdot);
    assert!(!function.params[1].dotdotdot);
    for (param, expected) in function.params.iter().zip(["args", "next"]) {
        assert!(matches!(&param.name.kind, tsc_rs_ast::PatKind::Ident(name) if name == expected));
    }
    assert!(function.params[1].type_ann.is_some());
    let body = function.body.as_ref().expect("body remains attached");
    assert_eq!(body.len(), 1);
    assert!(matches!(body[0].kind, tsc_rs_ast::StmtKind::Return(_)));
}

#[test]
fn correctly_separated_rest_parameters_and_speculative_expressions_are_unchanged() {
    for (file, source) in [
        (
            "input.ts",
            "function f(first: string, ...rest: string[]) {}",
        ),
        ("input.js", "function f(...rest) { return rest; }"),
        ("input.ts", "const f = (...rest: string[]) => rest;"),
        (
            "input.ts",
            "f(...args, next); const value = a ? (b ? c : d) : e;",
        ),
        ("input.tsx", "const view = <span>{f(...args, next)}</span>;"),
    ] {
        let parsed = parse(file, source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }
    let parsed = parse("input.ts", "function f(...rest, next) {}");
    let codes: Vec<_> = parsed
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(
        codes,
        [1014],
        "a written comma leaves rest-position grammar intact"
    );
}

#[test]
fn incomplete_if_statements_report_errors_without_consuming_enclosing_braces() {
    use tsc_rs_ast::{ClassMemberKind, ExprKind, PropName, StmtKind};

    for (statement, expected_code, expected_message) in [
        ("if (", 1109, "Expression expected."),
        ("if (ready", 1005, "')' expected."),
        ("if (state.ready", 1005, "')' expected."),
        ("if (ready)", 1109, "Expression expected."),
    ] {
        let source =
            format!("class Example {{\n first() {{\n {statement}\n }}\n next() {{}}\n}}\n");
        let parsed = parse("incomplete-if.ts", &source);
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "{statement}: {:?}",
            parsed.diagnostics
        );
        let diagnostic = &parsed.diagnostics[0];
        assert_eq!(diagnostic.code, expected_code, "{statement}");
        assert_eq!(diagnostic.message, expected_message, "{statement}");
        let span = diagnostic.span.expect("missing diagnostic span");
        assert_eq!(
            span.start as usize,
            source.find('}').unwrap(),
            "{statement}"
        );
        assert_eq!(&source[span.start as usize..span.end as usize], "}");
        if expected_code == 1005 {
            let related = diagnostic
                .related
                .as_ref()
                .expect("missing opening delimiter");
            assert_eq!(related.len(), 1);
            assert_eq!(related[0].code, 1007);
            let opening = related[0].span.unwrap();
            assert_eq!(opening.start as usize, source.find("if (").unwrap() + 3);
            assert_eq!(&source[opening.start as usize..opening.end as usize], "(");
        } else {
            assert!(diagnostic.related.is_none(), "{statement}");
        }

        assert_eq!(parsed.statements.len(), 1, "{statement}");
        let StmtKind::ClassDecl(class) = &parsed.statements[0].kind else {
            panic!("expected class after recovering {statement}");
        };
        assert_eq!(class.members.len(), 2, "{statement}");
        let ClassMemberKind::Method(first) = &class.members[0].kind else {
            panic!("expected first method after recovering {statement}");
        };
        let body = first.body.as_ref().unwrap();
        assert_eq!(body.len(), 1, "{statement}");
        let StmtKind::If(if_statement) = &body[0].kind else {
            panic!("expected recovered if statement for {statement}");
        };
        if statement == "if (" {
            assert!(matches!(if_statement.test.kind, ExprKind::Omitted));
        }
        assert!(matches!(if_statement.consequent.kind, StmtKind::Empty));
        let ClassMemberKind::Method(next) = &class.members[1].kind else {
            panic!("expected following method after recovering {statement}");
        };
        assert!(matches!(&next.name, PropName::Ident(name, _) if name == "next"));
        assert!(next.body.as_ref().unwrap().is_empty());
    }
}

#[test]
fn empty_if_conditions_report_the_closing_parenthesis() {
    for (source, expected_tokens) in [
        ("if () ;", vec![")"]),
        ("function f() { if () }", vec![")", "}"]),
    ] {
        let parsed = parse("empty-if.ts", source);
        assert_eq!(parsed.diagnostics.len(), expected_tokens.len(), "{source}");
        for (diagnostic, expected_token) in parsed.diagnostics.iter().zip(expected_tokens) {
            assert_eq!(diagnostic.code, 1109, "{source}");
            let span = diagnostic.span.unwrap();
            assert_eq!(
                &source[span.start as usize..span.end as usize],
                expected_token
            );
            assert!(diagnostic.related.is_none());
        }
    }

    for source in ["if (true) ;", "if (true) {} else {}"] {
        let parsed = parse("valid-if.ts", source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }
}

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
fn conditional_type_line_break_preserves_an_extends_property() {
    use tsc_rs_ast::{KeywordTypeKind, PropName, StmtKind, TypeMemberKind, TypeNodeKind};

    for separator in [
        "\n",
        "\r\n",
        "\r",
        "/*\n*/",
        "// comment\n",
        "\u{2028}",
        "\u{2029}",
    ] {
        for declaration in ["interface Shape", "type Shape ="] {
            let source = format!(
                "{declaration} {{ value?: number{separator} extends?: string | string[] }}"
            );
            let parsed = parse("conditional-boundary.ts", &source);
            assert!(
                parsed.diagnostics.is_empty(),
                "{source:?}: {:?}",
                parsed.diagnostics
            );
            assert_eq!(parsed.statements.len(), 1);
            let members = match &parsed.statements[0].kind {
                StmtKind::InterfaceDecl(interface) => &interface.members,
                StmtKind::TypeAlias(alias) => {
                    let TypeNodeKind::TypeLit(members) = &alias.type_ann.kind else {
                        panic!("expected object type for {source:?}");
                    };
                    members
                }
                _ => panic!("expected type declaration for {source:?}"),
            };
            assert_eq!(members.len(), 2, "{source:?}");
            let TypeMemberKind::PropertySig(value) = &members[0].kind else {
                panic!("expected first property for {source:?}");
            };
            assert!(matches!(&value.name, PropName::Ident(name, _) if name == "value"));
            assert!(value.optional);
            let value_type = value.type_ann.as_ref().unwrap();
            assert!(matches!(
                value_type.kind,
                TypeNodeKind::Keyword(KeywordTypeKind::Number)
            ));
            assert_eq!(
                &source[value_type.span.start as usize..value_type.span.end as usize],
                "number"
            );
            let TypeMemberKind::PropertySig(extends) = &members[1].kind else {
                panic!("expected extends property for {source:?}");
            };
            assert!(matches!(&extends.name, PropName::Ident(name, _) if name == "extends"));
            assert!(extends.optional);
            let TypeNodeKind::Union(types) = &extends.type_ann.as_ref().unwrap().kind else {
                panic!("expected union for extends property in {source:?}");
            };
            assert_eq!(types.len(), 2);
            assert!(matches!(
                types[0].kind,
                TypeNodeKind::Keyword(KeywordTypeKind::String)
            ));
            let TypeNodeKind::Array(element) = &types[1].kind else {
                panic!("expected array union constituent for {source:?}");
            };
            assert!(matches!(
                element.kind,
                TypeNodeKind::Keyword(KeywordTypeKind::String)
            ));
        }
    }
}

#[test]
fn conditional_type_line_break_also_terminates_method_returns() {
    use tsc_rs_ast::{KeywordTypeKind, StmtKind, TypeMemberKind, TypeNodeKind};

    let parsed = parse(
        "method-boundary.ts",
        "interface Shape { get(): number\n extends?: string }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let StmtKind::InterfaceDecl(interface) = &parsed.statements[0].kind else {
        panic!("expected interface");
    };
    assert_eq!(interface.members.len(), 2);
    let TypeMemberKind::MethodSig(method) = &interface.members[0].kind else {
        panic!("expected method signature");
    };
    assert!(matches!(
        method.return_type.as_ref().unwrap().kind,
        TypeNodeKind::Keyword(KeywordTypeKind::Number)
    ));
    assert!(matches!(
        interface.members[1].kind,
        TypeMemberKind::PropertySig(_)
    ));
}

#[test]
fn conditional_types_allow_line_breaks_after_the_extends_keyword() {
    use tsc_rs_ast::{StmtKind, TypeNodeKind};

    for source in [
        "type Pick<T> = T extends\n string ?\n true :\n false;",
        "type Pick<T> = T /* comment */ extends string ? true : false;",
        "type Pick<T> = T extends string ? T extends 'a' ? 1 : 2 : 3;",
    ] {
        let parsed = parse("conditional.ts", source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            parsed.diagnostics
        );
        let StmtKind::TypeAlias(alias) = &parsed.statements[0].kind else {
            panic!("expected type alias for {source}");
        };
        assert!(
            matches!(alias.type_ann.kind, TypeNodeKind::Conditional(_)),
            "{source}"
        );
    }

    let parsed = parse("constraint.ts", "type Constrained<T\n extends string> = T;");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let StmtKind::TypeAlias(alias) = &parsed.statements[0].kind else {
        panic!("expected constrained type alias");
    };
    assert!(alias.type_params.as_ref().unwrap()[0].constraint.is_some());
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
fn export_assignment_modifiers_highlight_the_first_token() {
    use tsc_rs_ast::{ExportDeclKind, StmtKind};

    for (prefix, first) in [
        ("declare ", "declare"),
        ("export declare ", "export"),
        ("export\ndeclare ", "export"),
        ("export /* comment */ declare ", "export"),
    ] {
        for assignment in ["export = value;", "export default value;"] {
            let source = format!("/* leading */\n{prefix}{assignment}");
            let parsed = parse("assignment.ts", &source);
            assert_eq!(
                parsed.diagnostics.len(),
                1,
                "{source}: {:?}",
                parsed.diagnostics
            );
            let diagnostic = &parsed.diagnostics[0];
            assert_eq!(diagnostic.code, 1120, "{source}");
            assert_eq!(
                diagnostic.message,
                "An export assignment cannot have modifiers."
            );
            let span = diagnostic.span.unwrap();
            assert_eq!(span.start as usize, source.find(prefix).unwrap());
            assert_eq!(&source[span.start as usize..span.end as usize], first);

            // Reporting the grammar error must preserve the recovered export
            // assignment so existing JavaScript emission remains unchanged.
            let mut statement = &parsed.statements[0];
            while let StmtKind::Export(export) = &statement.kind {
                match &export.kind {
                    ExportDeclKind::Decl(inner) => statement = inner,
                    ExportDeclKind::Default(_) => break,
                    _ => panic!("unexpected export in {source}"),
                }
            }
            assert!(
                matches!(statement.kind, StmtKind::ExportAssign(_))
                    || matches!(&statement.kind, StmtKind::Export(export)
                    if matches!(export.kind, ExportDeclKind::Default(_)))
            );
        }
    }
}

#[test]
fn export_assignment_modifiers_respect_module_context() {
    for source in [
        "namespace N { declare export = value; }",
        "declare namespace N { declare export = value; }",
        "function f() { declare export = value; }",
        "if (true) { declare export = value; }",
        "declare module 'pkg' { declare export = value; }",
        "declare module 'pkg' { export declare export = value; }",
        "declare global { declare export = value; }",
    ] {
        let parsed = parse("context.ts", source);
        assert!(
            !parsed.diagnostics.iter().any(|d| d.code == 1120),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn export_assignment_modifiers_preserve_grammar_error_precedence() {
    for source in [
        "export export export = value;",
        "declare declare export = value;",
        "declare export export = value;",
        "@decorator declare export = value;",
        "declare\nexport = value;",
        "export declare\nexport = value;",
        "declare export = ;",
        "declare export = value; const broken = ;",
        "const broken = ; export export default value;",
    ] {
        let parsed = parse("recovery.ts", source);
        assert!(
            !parsed.diagnostics.iter().any(|d| d.code == 1120),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }

    for source in [
        "export = value;",
        "export default value;",
        "export default class C {}",
        "export default function f() {}",
        "export default interface I {}",
        "export declare class C {}",
        "declare export default class C {}",
        "declare export default function f();",
    ] {
        let parsed = parse("declaration.ts", source);
        assert!(
            !parsed.diagnostics.iter().any(|d| d.code == 1120),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }

    let parsed = parse("assignment.js", "declare export = value;");
    assert_eq!(
        parsed.diagnostics.iter().filter(|d| d.code == 1120).count(),
        1
    );

    // An export-only prefix needs compiler options to choose TS1120 versus
    // TS1287 in CommonJS/verbatimModuleSyntax, so the parser leaves it alone.
    for source in ["export export = value;", "export export default value;"] {
        let parsed = parse("options.ts", source);
        assert!(!parsed.diagnostics.iter().any(|d| d.code == 1120));
    }
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

#[test]
fn missing_exponent_digits_report_the_end_of_the_exponent() {
    for (literal, offset) in [
        ("1e", 2),
        ("1E", 2),
        ("1e+", 3),
        ("1E-", 3),
        ("0.1e+", 5),
        ("1.e-", 4),
        (".1e+", 4),
        ("1efoo", 2),
        ("1e+foo", 3),
    ] {
        let source = format!("const n = {literal};");
        let parsed = parse("exponent.ts", &source);
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "{source}: {:?}",
            parsed.diagnostics
        );
        let diagnostic = &parsed.diagnostics[0];
        assert_eq!(diagnostic.code, 1124, "{source}");
        assert_eq!(diagnostic.message, "Digit expected.");
        assert_eq!(
            diagnostic.span,
            Some(tsc_rs_ast::Span::new(10 + offset, 10 + offset))
        );
    }

    // Separators and a bigint suffix can have their own diagnostics. The
    // missing decimal digit is still located after separators, before `n`.
    for (source, offset) in [("1e_", 3), ("1e+__n;", 5), ("1en;", 2)] {
        let parsed = parse("exponent.ts", source);
        let digits: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.code == 1124)
            .collect();
        assert_eq!(digits.len(), 1, "{source}: {:?}", parsed.diagnostics);
        assert_eq!(digits[0].span, Some(tsc_rs_ast::Span::new(offset, offset)));
    }
}

#[test]
fn missing_exponent_digits_are_checked_in_numeric_syntax_positions() {
    for (source, offset) in [
        ("1e", 2),
        ("type Value = 1e;", 15),
        ("const value = { 1e: 0 };", 18),
        ("declare const value = 1e;", 24),
        ("const t = `1e${2e+}3e-`;", 18),
    ] {
        let parsed = parse("exponent.ts", source);
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "{source}: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.diagnostics[0].code, 1124, "{source}");
        assert_eq!(
            parsed.diagnostics[0].span,
            Some(tsc_rs_ast::Span::new(offset, offset))
        );
    }

    let parsed = parse("exponent.ts", "1e; 2E+; 3e-;");
    let observed: Vec<_> = parsed
        .diagnostics
        .iter()
        .map(|d| (d.code, d.span.unwrap()))
        .collect();
    assert_eq!(
        observed,
        [
            (1124, tsc_rs_ast::Span::new(2, 2)),
            (1124, tsc_rs_ast::Span::new(7, 7)),
            (1124, tsc_rs_ast::Span::new(12, 12)),
        ]
    );
}

#[test]
fn exponent_digit_checks_ignore_complete_numbers_and_literal_text() {
    for source in [
        "0; 1e0; 1E+2; 1e-2; 0.1e2; .1e2; 1.e2; 1e1_0;",
        "0x1e; 0xEn; 0XdeadBEEFn; 0o17; 0b10; 12n;",
        "const text = '1e+'; const template = `1e${value}2e-`;",
        "// 1e+\n/* 2e- */ const value = 1;",
        "const rx = value < /1e+ 2E- 3e/;",
        "type T = `1e${string}2e+`;",
    ] {
        let parsed = parse("valid-exponent.ts", source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }

    for source in ["01e+;", "0b1e;", "0o1e;"] {
        let parsed = parse("other-numeric-errors.ts", source);
        assert!(
            parsed.diagnostics.iter().all(|d| d.code != 1124),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }

    let source = "const view = <div title=\"1e+\">1e+ {2e+}</div>;";
    let parsed = parse("exponent.tsx", source);
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", parsed.diagnostics);
    assert_eq!(parsed.diagnostics[0].code, 1124);
    assert_eq!(
        parsed.diagnostics[0].span,
        Some(tsc_rs_ast::Span::new(38, 38))
    );
}
