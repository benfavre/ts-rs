use tsc_rs_ast::{Diagnostic, Span};

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    tsc_rs_parser::parse("grammar.ts", source).diagnostics
}

#[test]
fn class_heritage_errors_report_the_first_invalid_clause_or_base() {
    for (source, code, token) in [
        ("class C extends A extends B {}", 1172, "extends"),
        (
            "class C extends A implements B extends C {}",
            1172,
            "extends",
        ),
        ("class C implements A extends B {}", 1173, "extends"),
        ("class C extends A, B {}", 1174, "B"),
        ("class C extends A, B.C<T> {}", 1174, "B"),
        ("class C extends A extends B extends D {}", 1172, "extends"),
        (
            "class C implements A extends B extends D {}",
            1173,
            "extends",
        ),
        ("class C extends A, B, C extends D {}", 1174, "B"),
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, code);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], token);
        assert!(!tsc_rs_parser::has_syntax_errors(&errors));
    }
    for source in [
        "class C {}",
        "class C extends A<T> implements B, D {}",
        "class C implements A, B {}",
        "const C = class extends A {};",
    ] {
        assert!(diagnostics(source).is_empty(), "{source}");
    }
}

#[test]
fn empty_for_in_and_for_of_declarations_point_after_the_keyword() {
    for (source, end) in [
        ("for (var in items) {}", 8),
        ("for (var of items) {}", 8),
        ("for (const of items) {}", 10),
        ("for (var /* comment */ of of) {}", 8),
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1123);
        assert_eq!(errors[0].span, Some(Span::new(end, end)));
    }
    for source in [
        "for (var of of items) {}",
        "for (var of;;) {}",
        "for (var of = 0 in items) {}",
        "for (const item of items) {}",
    ] {
        assert!(diagnostics(source).is_empty(), "{source}");
    }
}

#[test]
fn optional_object_members_are_reported_at_the_question_mark() {
    for source in [
        "const value = { name? };",
        "const value = { name?: 1 };",
        "const value = { method?() {} };",
        "const value = { method?<T>() {} };",
        "const value = { async method?() {} };",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1162);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "?");
    }
    assert!(diagnostics("interface I { property?: number; method?(): void }").is_empty());
    assert!(diagnostics("class C { property?: number; method?() {} }").is_empty());
}

#[test]
fn computed_object_names_reject_only_unparenthesized_comma_expressions() {
    for source in [
        "const value = { [0, 1]: 2 };",
        "const value = { [0, 1]() {} };",
        "const value = { get [0, 1]() { return 2; } };",
        "const value = { set [0, 1](v) {} };",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1171);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "0, 1");
    }
    for source in [
        "const value = { [(0, 1)]: 2 };",
        "enum E { [0, 1] = 2 }",
        "class C { [0, 1] = 2 }",
        "interface I { [0, 1]: number }",
    ] {
        assert!(
            !diagnostics(source).iter().any(|error| error.code == 1171),
            "{source}"
        );
    }
}

#[test]
fn assignment_patterns_have_different_member_grammar_from_value_literals() {
    for source in [
        "({ x? } = value);",
        "({ x /* comment */ ? } = value);",
        "({ [0, 1]: x } = value);",
        "({ x?: { y? } } = value);",
        "[{ x? }] = value;",
        "for ({ x? } of values) {}",
        "for ({ [0, 1]: x } in values) {}",
    ] {
        assert!(
            diagnostics(source).is_empty(),
            "{source}: {:?}",
            diagnostics(source)
        );
    }
    for source in [
        "({ x = { y? } } = value);",
        "({ x: target = { y? } } = value);",
        "({ x? }) = value;",
        "({ x? } += value);",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1162);
    }
}

#[test]
fn recovered_grammar_checks_yield_to_real_syntax_errors() {
    for source in [
        "class C extends A extends B {} const broken = ;",
        "for (var in items) {} const broken = ;",
        "const value = { name? }; const broken = ;",
        "const value = { [0, 1]: 2 }; const broken = ;",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1109);
    }
}
