use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str, legacy: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    TypeChecker::new()
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                experimental_decorators: Some(legacy),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

fn assert_arity(
    source: &str,
    legacy: bool,
    code: u32,
    kind: &str,
    detail: &str,
    marked: &str,
) -> Diagnostic {
    let errors = check(source, legacy);
    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
    let error = errors.into_iter().next().unwrap();
    assert_eq!(error.code, code);
    assert_eq!(
        error.message,
        format!(
            "Unable to resolve signature of {kind} decorator when called as an expression.\n  {}",
            detail.replace('\n', "\n  ")
        )
    );
    let span = error.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], marked);
    error
}

#[test]
fn legacy_decorator_arity_uses_each_runtime_argument_list() {
    for (source, code, kind, count, marked) in [
        (
            "declare function dec(a:any,b:any):void; @dec class C {}",
            1238,
            "class",
            1,
            "@dec",
        ),
        (
            "declare function dec(a:any):void; class C { @dec p: any; }",
            1240,
            "property",
            2,
            "dec",
        ),
        (
            "declare function dec(a:any):void; class C { @dec m() {} }",
            1241,
            "method",
            3,
            "dec",
        ),
        (
            "declare function dec(a:any):void; class C { @dec accessor p = 1; }",
            1240,
            "property",
            3,
            "dec",
        ),
        (
            "declare function dec(a:any):void; class C { constructor(@dec p:any) {} }",
            1239,
            "parameter",
            3,
            "dec",
        ),
    ] {
        let expected = if code == 1238 { 2 } else { 1 };
        assert_arity(source, true, code, kind, &format!("The runtime will invoke the decorator with {count} arguments, but the decorator expects {expected}."), marked);
    }
}

#[test]
fn missing_decorator_arguments_include_the_parameter_note() {
    let cases = [
        ("declare function dec(a:any,b:any,c:any,d:any):void;", "4"),
        (
            "declare function dec(this:void,a:any,b:any,c:any,d:any):void;",
            "4",
        ),
        (
            concat!(
                "declare function dec(a:any,b:any,c:any,d:any,e:any):void; ",
                "declare function dec(a:any,b:any,c:any,d:any):void;"
            ),
            "4-5",
        ),
    ];
    for (declaration, expected) in cases {
        for expression in ["dec", "(dec)"] {
            let source = format!("{declaration} class C {{ @{expression} m() {{}} }}");
            let detail = format!(
                "The runtime will invoke the decorator with 3 arguments, \
                 but the decorator expects {expected}."
            );
            let error = assert_arity(
                &source,
                true,
                1241,
                "method",
                &detail,
                &format!("@{expression}"),
            );
            let related = error.related.unwrap();
            assert_eq!(related.len(), 1);
            assert_eq!(related[0].code, 6210);
            assert_eq!(related[0].message, "An argument for 'd' was not provided.");
            assert_eq!(related[0].file_name.as_deref(), Some("subject.ts"));
            let span = related[0].span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], "d:any");
            assert_eq!(span.start as usize, source.rfind("d:any").unwrap());
        }
    }
}

#[test]
fn inline_and_factory_decorators_retain_parameter_locations() {
    for source in [
        "declare function factory(): (a:any,b:any,c:any) => void; @factory() class C {}",
        "@((a:any,b:any,c:any) => {}) class C {}",
        "@(function(a:any,b:any,c:any) {}) class C {}",
    ] {
        let errors = check(source, false);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1238);
        let related = errors[0].related.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].code, 6210);
        assert_eq!(related[0].message, "An argument for 'c' was not provided.");
        let span = related[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "c:any");
    }
}

#[test]
fn missing_decorator_binding_pattern_uses_a_pattern_note() {
    let source =
        "declare function dec(a:any,b:any,c:any,{d}:{d:any}):void; class C { @dec m() {} }";
    let error = assert_arity(
        source,
        true,
        1241,
        "method",
        "The runtime will invoke the decorator with 3 arguments, but the decorator expects 4.",
        "@dec",
    );
    let related = error.related.unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].code, 6211);
    assert_eq!(
        related[0].message,
        "An argument matching this binding pattern was not provided."
    );
    let span = related[0].span.unwrap();
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        "{d}:{d:any}"
    );
}

#[test]
fn parenthesized_factory_is_checked_as_a_call_instead_of_ts1329() {
    assert_arity(
        "declare function dec(): void; class C { @(dec) m() {} }",
        true,
        1241,
        "method",
        "The runtime will invoke the decorator with 3 arguments, but the decorator expects 0.",
        "(dec)",
    );
}

#[test]
fn standard_decorators_accept_one_parameter_but_reject_zero_or_three() {
    for member in ["", "m() {}", "p = 1;"] {
        for (parameters, expected) in [
            ("", Some(0)),
            ("a:any", None),
            ("a:any,b:any", None),
            ("a:any,b:any,c:any", Some(3)),
        ] {
            let source = if member.is_empty() {
                format!("declare function dec({parameters}):void; @(dec) class C {{}}")
            } else {
                format!("declare function dec({parameters}):void; class C {{ @(dec) {member} }}")
            };
            let (code, kind) = if member.is_empty() {
                (1238, "class")
            } else if member.starts_with('m') {
                (1241, "method")
            } else {
                (1240, "property")
            };
            if let Some(expected) = expected {
                assert_arity(&source, false, code, kind, &format!("The runtime will invoke the decorator with 2 arguments, but the decorator expects {expected}."), if expected > 2 { "@(dec)" } else { "(dec)" });
            } else {
                let errors = check(&source, false);
                assert!(errors.is_empty(), "{source}: {errors:?}");
            }
        }
    }
}

#[test]
fn optional_and_variadic_parameter_ranges_are_preserved() {
    let errors = check(
        "declare function dec(...args:[any,any]):void; @dec class C {}",
        false,
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_arity(
        "declare function dec(a:any,b:any,c?:any):void; @dec class C {}",
        true,
        1238,
        "class",
        "The runtime will invoke the decorator with 1 arguments, but the decorator expects 2-3.",
        "@dec",
    );
    assert_arity("declare function dec(a:any,b:any,c:any,d:any,...rest:any[]):void; class C { @dec m() {} }", true, 1241, "method", "The runtime will invoke the decorator with 3 arguments, but the decorator expects at least 4.", "@dec");
    assert_arity(
        "declare function dec(...args:[any,any,any,any]):void; class C { @dec m() {} }",
        true,
        1241,
        "method",
        "The runtime will invoke the decorator with 3 arguments, but the decorator expects 4.",
        "@dec",
    );
}

#[test]
fn an_arity_compatible_overload_prevents_an_arity_diagnostic() {
    let source = "declare function dec(a:any):void; declare function dec(a:any,b:any,c:any):void; class C { @dec m() {} }";
    let errors = check(source, true);
    assert!(errors.is_empty(), "{errors:?}");
    assert_arity("declare function dec(a:any,b:any):void; declare function dec(a:any,b:any,c:any,d:any):void; class C { constructor(@dec x:any) {} }", true, 1239, "parameter", "No overload expects 3 arguments, but overloads do exist that expect either 2 or 4 arguments.", "@dec");
}

#[test]
fn primitive_and_constructible_decorator_values_are_not_callable() {
    for (declaration, expected) in [
        ("declare const dec: number;", "Number"),
        ("type D = number; declare const dec: D;", "Number"),
        ("declare const dec: unknown;", "unknown"),
        ("declare const dec: {};", "{}"),
        ("class dec {}", "typeof dec"),
    ] {
        let source = format!("{declaration} class C {{ @dec m() {{}} }}");
        assert_arity(
            &source,
            true,
            1241,
            "method",
            &format!(
                "This expression is not callable.\n  Type '{expected}' has no call signatures."
            ),
            "dec",
        );
    }
}

#[test]
fn unresolved_decorator_types_do_not_report_a_second_error() {
    let errors = check("declare const dec: Missing; class C { @dec m() {} }", true);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2304);
}

#[test]
fn nullable_decorators_report_the_noncallable_union_member() {
    assert_arity("declare const dec: (() => any) | undefined; class C { @dec m() {} }", true, 1241, "method", "This expression is not callable.\n  Not all constituents of type '(() => any) | undefined' are callable.\n    Type 'undefined' has no call signatures.", "dec");
    assert_arity("type D = (() => void) | undefined; declare const dec: D; class C { @dec m() {} }", true, 1241, "method", "This expression is not callable.\n  Not all constituents of type 'D' are callable.\n    Type 'undefined' has no call signatures.", "dec");
}

#[test]
fn function_types_without_signatures_remain_untyped_decorators() {
    for declaration in [
        "declare const dec: Function;",
        "interface Callable extends Function {} declare const dec: Callable;",
        "class Callable extends Function {} declare const dec: Callable;",
    ] {
        let source = format!("{declaration} @dec class C {{ @dec m() {{}} }}");
        let errors = check(&source, true);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}
