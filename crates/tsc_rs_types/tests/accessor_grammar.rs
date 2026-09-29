use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<(u32, String)> {
    let file = tsc_rs_parser::parse("accessors.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
    let mut diagnostics = file.diagnostics;
    diagnostics.extend(result.diagnostics);
    diagnostics.sort_by_key(|d| (d.span.map(|span| span.start), d.code));
    diagnostics
        .into_iter()
        .filter(|d| {
            matches!(
                d.code,
                1014 | 1015 | 1016 | 1047..=1054 | 1094 | 1095 | 1341 | 2378 | 2408 | 2676 | 2808
            )
        })
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                source[span.start as usize..span.end as usize].to_string(),
            )
        })
        .collect()
}

#[test]
fn accessor_arity_optionality_and_return_annotations() {
    for (declaration, expected) in [
        ("get value(x) { return 1; }", (1054, "value")),
        ("set value() {}", (1049, "value")),
        ("set value(x, y) {}", (1049, "value")),
        ("set value(x?): void {}", (1095, "value")),
        ("set value(x?) {}", (1051, "?")),
        ("set value(...x: any[]) {}", (1053, "...")),
        ("set value(x = 1) {}", (1052, "value")),
    ] {
        for source in [
            format!("class C {{ {declaration} }}"),
            format!("let obj = {{ {declaration} }};"),
        ] {
            assert_eq!(
                diagnostics(&source),
                [(expected.0, expected.1.to_string())],
                "{source}"
            );
        }
    }
}

#[test]
fn parameter_grammar_takes_precedence_over_accessor_grammar() {
    assert_eq!(diagnostics("class C { set a(...x = []) {} set b(x? = 1) {} set c(x?, y) {} get d<T>() {return 1;} }"), [(1048, "x".into()), (1015, "x".into()), (1016, "y".into()), (1094, "d".into())]);
}

#[test]
fn this_parameters_and_type_signatures_use_accessor_arity() {
    assert!(
        diagnostics("class C { get a(this: C) {return 1;} set a(this: C, x: number) {} }")
            .is_empty()
    );
    assert_eq!(
        diagnostics("interface I {get a(x): number; set b(x?); } type T = {set c();};"),
        [(1054, "a".into()), (1051, "?".into()), (1049, "c".into())]
    );
}

#[test]
fn setter_returns_exclude_nested_functions_and_allow_bare_return() {
    let source = "class C {set value(x: number) { function nested() {return 1;} const arrow = () => {return 2;}; if (x) {return x;} try {return 3;} finally {} return; }} let obj = {set value(x: number) {while(x) {return x;} }};";
    assert_eq!(diagnostics(source), vec![(2408, "return".into()); 3]);
}

#[test]
fn getter_visibility_cannot_be_narrower_than_its_setter() {
    for (get_visibility, set_visibility, invalid) in [
        ("public", "private", false),
        ("protected", "private", false),
        ("protected", "public", true),
        ("private", "protected", true),
        ("private", "public", true),
        ("private", "private", false),
    ] {
        let source = format!("class C {{ {get_visibility} get value() {{return 1;}} {set_visibility} set value(x: number) {{}} }}");
        assert_eq!(
            diagnostics(&source),
            if invalid {
                vec![(2808, "value".into()); 2]
            } else {
                vec![]
            },
            "{source}"
        );
    }
    assert!(diagnostics(
        "class C { private static get value() {return 1;} public set value(x: number) {} }"
    )
    .is_empty());
}

#[test]
fn accessor_pairs_agree_on_abstractness_and_match_literal_names() {
    assert_eq!(diagnostics("abstract class C { abstract get value(): number; set value(x: number) {} protected get 'other'() {return 1;} public set other(x: number) {} }"), vec![(2676, "value".into()), (2676, "value".into()), (2808, "'other'".into()), (2808, "other".into())]);
}

#[test]
fn class_constructor_cannot_be_an_accessor() {
    assert_eq!(
        diagnostics("class C { get constructor() {return 1;} set constructor(x: number) {} }"),
        vec![(1341, "constructor".into()); 2]
    );
    assert!(diagnostics("let obj = {get constructor() {return 1;}};").is_empty());
}

#[test]
fn unresolved_computed_accessors_do_not_form_a_pair() {
    assert!(diagnostics("declare function f(): string; declare function g(): string; class C { private get [f()]() {return 1;} public set [g()](x: number) {} }").is_empty());
}

#[test]
fn getter_missing_return_follows_reachable_control_flow() {
    for (body, missing) in [
        ("", true),
        ("function nested() {return 1;} const arrow = () => 1;", true),
        ("return;", false),
        ("throw 1;", false),
        ("if (false) {return 1;}", true),
        ("if ((false)) {return 1;}", false),
        ("if (false && true) {return 1;}", true),
        ("if (false || false) {return 1;}", true),
        ("if (true) {throw 1;}", false),
        ("while (true) {}", false),
        ("while (1) {}", true),
        ("while (false) {return 1;}", true),
        ("while (true) {if (false) break;}", false),
        ("while (true) {break;}", true),
        ("while (true) {switch (1) {case 1: break;}}", false),
        ("for (;;) {continue;}", false),
        ("do {throw 1;} while (false);", false),
        ("do {} while (false);", true),
        ("switch (1) {case 1: throw 1; default: throw 2;}", false),
        ("switch (1) {case 1: break; default: throw 2;}", true),
        ("try {} catch {return 1;}", false),
        ("try {throw 1;} finally {}", false),
        ("try {} finally {throw 1;}", false),
        ("try {throw 1;} catch {} finally {}", true),
        ("exit: {break exit; return 1;}", true),
        (
            "outer: while(true) {inner: while(true) {break inner;} continue outer;}",
            false,
        ),
    ] {
        for source in [
            format!("class C {{get value() {{{body}}}}}"),
            format!("let obj = {{get value() {{{body}}}}};"),
        ] {
            assert_eq!(
                diagnostics(&source),
                if missing {
                    vec![(2378, "value".into())]
                } else {
                    vec![]
                },
                "{source}"
            );
        }
    }
}

#[test]
fn getter_missing_return_respects_ambient_bodies_and_never_calls() {
    assert!(diagnostics("declare class C {get value(): number;}").is_empty());
    assert_eq!(
        diagnostics("declare function fail(): never; class C {get value() {fail();}}"),
        [(2378, "value".into())]
    );
}
