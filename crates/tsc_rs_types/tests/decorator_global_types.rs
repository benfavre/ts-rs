use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

const CORE_GLOBALS: &str = "interface Array<T> {} interface Boolean {} interface Function {} interface IArguments {} interface Number {} interface Object {} interface RegExp {} interface String {}";
const DECORATOR: &str = "declare function dec(target: any, key: any, descriptor: any): void;";

fn check(body: &str, providers: &[(&str, &str)], options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("subject.ts", &format!("{CORE_GLOBALS}\n{body}"));
    let providers: Vec<_> = providers
        .iter()
        .map(|(name, source)| tsc_rs_parser::parse(name, source))
        .collect();
    let mut checker = TypeChecker::new();
    if !providers.is_empty() {
        let mut files: Vec<_> = providers.iter().collect();
        files.push(&file);
        checker.inject_external_types(&files);
        checker.take_diagnostics();
    }
    checker
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                no_lib: Some(true),
                experimental_decorators: Some(true),
                ..options
            },
        )
        .diagnostics
}

fn check_default(body: &str) -> Vec<Diagnostic> {
    check(body, &[], CompilerOptions::default())
}

fn assert_missing_descriptor(errors: &[Diagnostic]) {
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2318);
    assert_eq!(
        errors[0].message,
        "Cannot find global type 'TypedPropertyDescriptor'."
    );
    assert!(errors[0].span.is_none());
    assert!(errors[0].file_name.is_none());
}

#[test]
fn callable_legacy_method_and_accessor_decorators_require_the_descriptor_type() {
    for member in [
        "@dec method() {}",
        "@dec get value() { return 1; }",
        "@dec set value(input: number) {}",
        "@dec accessor value = 1;",
        "@dec static method() {}",
    ] {
        assert_missing_descriptor(&check_default(&format!(
            "{DECORATOR} class C {{ {member} }}"
        )));
    }
}

#[test]
fn the_missing_descriptor_is_a_single_program_diagnostic() {
    assert_missing_descriptor(&check_default(&format!(
        "{DECORATOR} class C {{ @dec first() {{}} @dec second() {{}} }} class D {{ @dec method() {{}} }}"
    )));
}

#[test]
fn each_stacked_decorator_uses_its_own_signature() {
    for decorators in ["@dec @factory", "@factory @dec"] {
        let errors = check_default(&format!(
            "{DECORATOR} declare function factory(): any; class C {{ {decorators} method() {{}} }}"
        ));
        let mut codes: Vec<_> = errors.iter().map(|error| error.code).collect();
        codes.sort();
        assert_eq!(codes, [1329, 2318], "{errors:?}");
        assert!(errors
            .iter()
            .find(|error| error.code == 1329)
            .unwrap()
            .message
            .starts_with("'factory' accepts"));
    }
}

#[test]
fn ordinary_properties_and_parameters_do_not_require_a_descriptor() {
    let errors = check_default("declare function dec(...args: any[]): void; @dec class C { @dec value = 1; method(@dec input: any) {} constructor(@dec input: any) {} }");
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn untyped_decorators_do_not_request_the_descriptor_global() {
    for ty in ["any", "Function", "unknown", "number"] {
        let errors = check_default(&format!(
            "declare const dec: {ty}; class C {{ @dec method() {{}} }}"
        ));
        assert!(errors.is_empty(), "{ty}: {errors:?}");
    }
    let errors = check_default("class C { @missing method() {} }");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2304);
}

#[test]
fn invalid_decorator_placements_keep_their_grammar_diagnostics() {
    for (body, expected) in [
        ("class C { @dec #method() {} }", 1206),
        ("const C = class { @dec method() {} };", 1206),
        ("abstract class C { @dec abstract method(): void; }", 1249),
        ("declare class C { @dec method(): void; }", 1249),
        ("class C { @dec method(): void; method() {} }", 1249),
    ] {
        let errors = check_default(&format!("{DECORATOR} {body}"));
        assert_eq!(errors.len(), 1, "{body}: {errors:?}");
        assert_eq!(errors[0].code, expected, "{body}: {errors:?}");
    }
}

#[test]
fn callable_decorator_expressions_and_named_callable_types_request_the_descriptor() {
    for body in [
        "class C { @((target, key, descriptor) => {}) method() {} }",
        "class C { @(function(target, key, descriptor) {}) method() {} }",
        "declare function factory(): (target, key, descriptor) => void; class C { @factory() method() {} }",
        "interface Decorator { (target: any, key: any, descriptor: any): void } declare const dec: Decorator; class C { @dec method() {} }",
        "type Decorator = (target: any, key: any, descriptor: any) => void; declare const dec: Decorator; class C { @dec method() {} }",
        "declare function original(target: any, key: any, descriptor: any): void; declare const dec: typeof original; class C { @dec method() {} }",
        "interface Base { (target: any, key: any, descriptor: any): void } interface Decorator extends Base {} declare const dec: Decorator; class C { @dec method() {} }",
    ] {
        let errors = check_default(body);
        assert_eq!(errors.len(), 1, "{body}: {errors:?}");
        assert_missing_descriptor(&errors);
    }
}

#[test]
fn a_current_file_global_descriptor_satisfies_the_requirement() {
    let errors = check_default(&format!(
        "interface TypedPropertyDescriptor<T> {{}} {DECORATOR} class C {{ @dec method() {{}} }}"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn injected_globals_satisfy_the_requirement_but_module_exports_do_not() {
    let body = format!("{DECORATOR} class C {{ @dec method() {{}} }}");
    for source in [
        "interface TypedPropertyDescriptor<T> {}",
        "export {}; declare global { interface TypedPropertyDescriptor<T> {} }",
    ] {
        let errors = check(&body, &[("globals.ts", source)], CompilerOptions::default());
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
    assert_missing_descriptor(&check(
        &body,
        &[(
            "globals.ts",
            "export interface TypedPropertyDescriptor<T> {}",
        )],
        CompilerOptions::default(),
    ));
}

#[test]
fn namespace_members_do_not_satisfy_the_descriptor_requirement() {
    assert_missing_descriptor(&check_default(&format!(
        "namespace Local {{ export interface TypedPropertyDescriptor<T> {{}} }} {DECORATOR} class C {{ @dec method() {{}} }}"
    )));
}

#[test]
fn decorators_use_the_descriptor_from_the_active_standard_library() {
    let file = tsc_rs_parser::parse(
        "subject.ts",
        &format!("{DECORATOR} class C {{ @dec method() {{}} }}"),
    );
    let errors = TypeChecker::new()
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                strict: Some(false),
                experimental_decorators: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn uncalled_factories_are_checked_against_each_decorator_position() {
    for (count, expected) in [(0, 5), (1, 4), (2, 2), (3, 0)] {
        let parameters = (0..count)
            .map(|i| format!("x{i}?: any"))
            .collect::<Vec<_>>()
            .join(", ");
        let body = format!("interface TypedPropertyDescriptor<T> {{}} declare function dec({parameters}): any; @dec class C {{ @dec field = 1; @dec method() {{}} @dec accessor auto = 1; p(@dec x: any) {{}} }}");
        let errors = check_default(&body);
        assert_eq!(errors.len(), expected, "{count}: {errors:?}");
        let source = format!("{CORE_GLOBALS}\n{body}");
        for error in errors {
            assert_eq!(error.code, 1329, "{error:?}");
            assert_eq!(error.message, "'dec' accepts too few arguments to be used as a decorator here. Did you mean to call it first and write '@dec()'?");
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], "@dec");
        }
    }
}

#[test]
fn uncalled_factory_messages_preserve_expression_spelling_and_decorator_spans() {
    for (declaration, decorator, expression) in [
        ("declare function dec(): any;", "@ /* before */ dec", "dec"),
        (
            "declare namespace N { function dec(): any; }",
            "@N.dec",
            "N.dec",
        ),
    ] {
        let body = format!("{declaration} class C {{ {decorator} method() {{}} }}");
        let errors = check_default(&body);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].code, 1329);
        assert_eq!(errors[0].message, format!("'{expression}' accepts too few arguments to be used as a decorator here. Did you mean to call it first and write '@{expression}()'?"));
        let source = format!("{CORE_GLOBALS}\n{body}");
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], decorator);
    }
}

#[test]
fn rest_parameters_and_an_applicable_overload_are_not_uncalled_factories() {
    for declaration in [
        "declare function dec(...args: any[]): any;",
        "declare function dec(): any; declare function dec(target: any, key?: any, descriptor?: any): any;",
    ] {
        let errors = check_default(&format!("interface TypedPropertyDescriptor<T> {{}} {declaration} @dec class C {{ @dec field = 1; @dec method() {{}} }}"));
        assert!(errors.is_empty(), "{declaration}: {errors:?}");
    }
}

#[test]
fn standard_decorators_also_reject_an_uncalled_zero_parameter_factory() {
    let file = tsc_rs_parser::parse(
        "subject.ts",
        "declare function dec(): any; @dec class C { @dec method() {} @dec field = 1; }",
    );
    let errors = TypeChecker::new()
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                strict: Some(false),
                target: Some(ScriptTarget::ES2015),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [1329, 1329, 1329],
        "{errors:?}"
    );
}

#[test]
fn strict_nullable_decorators_do_not_reach_signature_dependent_checks() {
    for signature in [
        "() => any",
        "(target: any, key: any, descriptor: any) => void",
    ] {
        let errors = check(
            &format!(
                "declare const dec: ({signature}) | undefined; class C {{ @dec method() {{}} }}"
            ),
            &[],
            CompilerOptions {
                strict_null_checks: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            errors
                .iter()
                .all(|error| !matches!(error.code, 2318 | 1329)),
            "{errors:?}"
        );
    }
}
