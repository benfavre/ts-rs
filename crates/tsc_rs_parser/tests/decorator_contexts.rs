use tsc_rs_ast::{ClassMemberKind, ExprKind, StmtKind};
use tsc_rs_parser::parse;

#[test]
fn decorator_recovery_preserves_contexts_and_at_sign_spans() {
    let source = r#"
const value = @classDecorator class Named {
    @fieldDecorator ["field"]: unknown;
    @methodDecorator ["method"]() {}
    named(@methodParameter parameter: unknown) {}
    @getterDecorator get ["value"]() { return 1; }
    @setterDecorator set value(@setterParameter value: number) {}
    constructor(@constructorParameter value: unknown) {}
};
function plain(@functionParameter value: unknown) {}
"#;
    let file = parse("decorators.ts", source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);

    let StmtKind::Var(var) = &file.statements[0].kind else {
        panic!("expected variable statement");
    };
    let initializer = var.declarations[0]
        .init
        .as_ref()
        .expect("class expression initializer");
    let ExprKind::ClassExpr(class) = &initializer.kind else {
        panic!("expected class expression");
    };
    assert_eq!(class.decorators.len(), 1);
    assert_eq!(
        class.decorators[0].span.start,
        source.find("@classDecorator").unwrap() as u32
    );

    let expected_member_decorators = [
        (0, "@fieldDecorator"),
        (1, "@methodDecorator"),
        (3, "@getterDecorator"),
        (4, "@setterDecorator"),
    ];
    for (index, expected) in expected_member_decorators {
        let member = &class.members[index];
        let decorators = match &member.kind {
            ClassMemberKind::Property(property) => &property.decorators,
            ClassMemberKind::Method(method) => &method.decorators,
            ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                &accessor.decorators
            }
            other => panic!("unexpected member: {other:?}"),
        };
        assert_eq!(decorators.len(), 1, "{expected}");
        assert_eq!(
            decorators[0].span.start,
            source.find(expected).unwrap() as u32,
            "{expected}"
        );
    }

    let ClassMemberKind::Method(method) = &class.members[2].kind else {
        panic!("expected named method");
    };
    assert_eq!(method.params[0].decorators.len(), 1);
    assert_eq!(
        method.params[0].decorators[0].span.start,
        source.find("@methodParameter").unwrap() as u32
    );

    let ClassMemberKind::SetAccessor(setter) = &class.members[4].kind else {
        panic!("expected setter");
    };
    assert_eq!(setter.params[0].decorators.len(), 1);

    let ClassMemberKind::Constructor(constructor) = &class.members[5].kind else {
        panic!("expected constructor");
    };
    assert_eq!(constructor.params[0].decorators.len(), 1);

    let StmtKind::FnDecl(function) = &file.statements[1].kind else {
        panic!("expected function declaration");
    };
    assert_eq!(function.params[0].decorators.len(), 1);
}

#[test]
fn decorated_this_parameters_remain_identifiable_for_grammar_precedence() {
    let source = "class C { method(@decorator this: C) {} }\nfunction f(@decorator this: C) {}";
    let file = parse("decorators.ts", source);
    let ts1433: Vec<_> = file
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1433)
        .map(|diagnostic| diagnostic.span.expect("TS1433 span"))
        .collect();
    let expected: Vec<_> = ["@decorator", "@decorator"]
        .into_iter()
        .scan(0, |offset, needle| {
            let start = source[*offset..].find(needle).unwrap() + *offset;
            *offset = start + needle.len();
            Some(tsc_rs_ast::Span::new(
                start as u32,
                (start + needle.len()) as u32,
            ))
        })
        .collect();
    assert_eq!(ts1433, expected, "{:#?}", file.diagnostics);

    let StmtKind::ClassDecl(class) = &file.statements[0].kind else {
        panic!("expected class declaration");
    };
    let ClassMemberKind::Method(method) = &class.members[0].kind else {
        panic!("expected method");
    };
    assert!(
        matches!(&method.params[0].name.kind, tsc_rs_ast::PatKind::Ident(name) if name == "this"),
        "{:#?}",
        method.params[0]
    );

    let StmtKind::FnDecl(function) = &file.statements[1].kind else {
        panic!("expected function");
    };
    assert!(
        matches!(&function.params[0].name.kind, tsc_rs_ast::PatKind::Ident(name) if name == "this"),
        "{:#?}",
        function.params[0]
    );
}

#[test]
fn parser_reports_decorators_discarded_by_recovery_once_at_the_at_sign() {
    let source = r#"
@enumDec enum E {}
@interfaceDec interface I {}
@typeDec type T = number;
@namespaceDec namespace N {}
@variableDec const value = 1;
class C {
    @staticBlockDec static {}
}
export @betweenExportAndDefault default class Invalid {}
export @allowedAfterExport class Allowed {}
export default @allowedAfterDefault class AlsoAllowed {}
"#;
    let file = parse("decorators.ts", source);
    let observed: Vec<_> = file
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1206)
        .map(|diagnostic| diagnostic.span.expect("TS1206 span"))
        .collect();
    let expected: Vec<_> = [
        ("@enumDec", 1),
        ("@interfaceDec", 1),
        ("@typeDec", 1),
        ("@namespaceDec", 1),
        ("@variableDec", 1),
        ("@staticBlockDec", 1),
        ("@betweenExportAndDefault", "@betweenExportAndDefault".len()),
    ]
    .into_iter()
    .map(|(needle, length)| {
        let start = source.find(needle).unwrap() as u32;
        tsc_rs_ast::Span::new(start, start + length as u32)
    })
    .collect();
    assert_eq!(observed, expected, "{:#?}", file.diagnostics);
}

#[test]
fn parser_does_not_invent_ts1206_for_missing_decorators_or_using_recovery() {
    let source = r#"
@
enum E {}
@dec
using 1
@dec
await using 1
"#;
    let file = parse("decorators.ts", source);
    assert!(
        file.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1206),
        "{:#?}",
        file.diagnostics
    );
}

#[test]
fn parser_only_recovers_declaration_decorators_at_statement_boundaries() {
    let source = "const broken =!@#!@$\nconst next = 1;\nvalue; @dec const decorated = 1;";
    let file = parse("decorators.ts", source);
    let observed: Vec<_> = file
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1206)
        .map(|diagnostic| diagnostic.span.expect("TS1206 span"))
        .collect();
    assert!(observed.is_empty(), "{:#?}", file.diagnostics);
}

#[test]
fn parser_matches_javascript_syntactic_decorator_spans() {
    let source = r#"
@variableDec const value = 1;
@interfaceDec interface I {}
@usingDec using resource;
@usingExpressionDec using 1;
class C {
    @staticBlockDec static {}
}
export @betweenExportAndDefault default class Invalid {}
"#;
    let file = parse("decorators.js", source);
    let observed: Vec<_> = file
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1206)
        .map(|diagnostic| diagnostic.span.expect("TS1206 span"))
        .collect();
    let expected: Vec<_> = [
        "@variableDec",
        "@usingDec",
        "@usingExpressionDec",
        "@staticBlockDec",
        "@betweenExportAndDefault",
    ]
    .into_iter()
    .map(|needle| {
        let start = source.find(needle).unwrap() as u32;
        tsc_rs_ast::Span::new(start, start + needle.len() as u32)
    })
    .collect();
    assert_eq!(observed, expected, "{:#?}", file.diagnostics);
}

#[test]
fn typescript_using_decorators_follow_declaration_recovery() {
    for (source, expected) in [
        ("@dec using resource = null;", true),
        ("@dec using resource;", true),
        ("@dec using {} = resource;", true),
        ("@dec using [] = resource;", true),
        ("@dec await using resource = null;", true),
        ("@dec await using missing;", true),
        ("@dec await using {} = resource;", true),
        ("@dec using 1;", false),
        ("@dec await using [];", false),
    ] {
        let file = parse("decorators.ts", source);
        assert_eq!(
            file.diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 1206)
                .count(),
            usize::from(expected),
            "{source}: {:#?}",
            file.diagnostics
        );
    }

    let displaced = parse("decorators.ts", "@dec using 1;\n@dec using resource;");
    assert!(
        displaced
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1206),
        "{:#?}",
        displaced.diagnostics
    );
}

#[test]
fn array_access_after_using_remains_an_expression() {
    let file = parse("usingDeclarations.4.ts", "{ using [a] = null; }");
    let StmtKind::Block(statements) = &file.statements[0].kind else {
        panic!("expected block");
    };
    assert!(
        matches!(statements[0].kind, StmtKind::Expr(_)),
        "{:#?}",
        statements[0]
    );
}

#[test]
fn bare_variable_declarations_keep_ts1206_without_accepting_invalid_bindings() {
    for (source, expected) in [
        ("@dec var;", true),
        ("@dec var 1;", false),
        ("@dec let;", true),
        ("@dec let 1;", false),
        ("@dec const;", true),
        ("@dec const 1;", false),
        ("@dec using;", true),
        ("@dec using 1;", false),
    ] {
        let file = parse("decorators.ts", source);
        assert_eq!(
            file.diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 1206)
                .count(),
            usize::from(expected),
            "{source}: {:#?}",
            file.diagnostics
        );
    }
}

#[test]
fn decorator_element_access_recovers_before_the_bracket() {
    let source = "@dec[x] const value = 1;";
    let typescript = parse("decorators.ts", source);
    assert!(
        typescript
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1206),
        "{:#?}",
        typescript.diagnostics
    );
    assert_eq!(
        typescript
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1146)
            .map(|diagnostic| diagnostic.span)
            .collect::<Vec<_>>(),
        [Some(tsc_rs_ast::Span::new(4, 4))]
    );

    let javascript = parse("decorators.js", source);
    assert_eq!(
        javascript
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1206)
            .map(|diagnostic| diagnostic.span)
            .collect::<Vec<_>>(),
        [Some(tsc_rs_ast::Span::new(0, 4))]
    );
}

#[test]
fn javascript_recovery_reports_whole_discarded_decorators() {
    for source in [
        "const value = !@dec const next = 1;",
        "const value = @dec function() {};",
        "@dec import value from \"module\";",
        "@dec export * from \"module\";",
    ] {
        let file = parse("decorators.js", source);
        let start = source.find("@dec").unwrap() as u32;
        assert_eq!(
            file.diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 1206)
                .map(|diagnostic| diagnostic.span)
                .collect::<Vec<_>>(),
            [Some(tsc_rs_ast::Span::new(start, start + 4))],
            "{source}: {:#?}",
            file.diagnostics
        );
    }
}
