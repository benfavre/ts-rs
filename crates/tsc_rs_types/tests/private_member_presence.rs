use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn diagnostics(file_name: &str, source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                allow_js: Some(true),
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

fn assert_missing(source: &str, property: &str, receiver: &str) {
    let errors = diagnostics("private.ts", source);
    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
    assert_eq!(errors[0].code, 2339, "{source}: {errors:?}");
    assert_eq!(
        errors[0].message,
        format!("Property '{property}' does not exist on type '{receiver}'.")
    );
    let span = errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], property);
}

#[test]
fn private_fields_must_be_declared_on_the_receiver() {
    assert_missing("class C { m() { this.#missing; } }", "#missing", "C");
    assert_missing(
        "class C { static #field = 1; m() { this.#field; } }",
        "#field",
        "C",
    );
    assert_missing(
        "class D {} class C { #field = 1; m(value: D) { value.#field; } }",
        "#field",
        "D",
    );
    assert_missing(
        "class C { #field = 1; m(value: C) { value.#unrelated; } }",
        "#unrelated",
        "C",
    );
}

#[test]
fn private_access_on_any_requires_a_lexical_declaration() {
    for source in [
        "class C { m(value: any) { value.#missing; } }",
        "class C { '#missing' = 1; m(value: any) { value.#missing; } }",
        "class C { #field = 1; m(value: any) { return class D { n() { value.#missing; } }; } }",
    ] {
        assert_missing(source, "#missing", "any");
    }
    let source = "class C { #field = 1; static #method() {} m(value: any) { value.#field; value.#method; return class D { #field = 2; n() { value.#field; value.#method; } }; } }";
    let errors = diagnostics("private.ts", source);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn private_access_on_any_outside_a_class_is_rejected() {
    let source = "declare const value: any; value.#missing;";
    let errors = diagnostics("private.ts", source);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 18016);
    assert_eq!(
        errors[0].message,
        "Private identifiers are not allowed outside class bodies."
    );
    let span = errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "#missing");
}

#[test]
fn index_signatures_and_string_keys_do_not_declare_private_names() {
    assert_missing(
        "class C { [key: string]: any; m() { this.#missing = 1; } }",
        "#missing",
        "C",
    );
    assert_missing(
        "class C { '#field' = 1; m() { this.#field; } }",
        "#field",
        "C",
    );
    assert_missing(
        "class C { private '#field' = 1; m() { this.#field; } }",
        "#field",
        "C",
    );
}

#[test]
fn javascript_assignments_cannot_declare_private_identifiers() {
    for source in [
        "class C { constructor() { this.#missing; } }",
        "class C { constructor() { this.#missing = 1; } }",
    ] {
        let errors = diagnostics("private.js", source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2339, "{source}: {errors:?}");
        assert_eq!(
            errors[0].message,
            "Property '#missing' does not exist on type 'C'."
        );
    }
}

#[test]
fn accessible_private_names_are_spelling_candidates() {
    for misspelled in ["#secretVale", "secretVale"] {
        let source = format!("class C {{ #secretValue = 1; m() {{ this.{misspelled}; }} }}");
        let errors = diagnostics("private.ts", &source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2551, "{source}: {errors:?}");
        assert_eq!(
            errors[0].message,
            format!(
                "Property '{misspelled}' does not exist on type 'C'. Did you mean '#secretValue'?"
            )
        );
        let related = errors[0].related.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].code, 2728);
        let span = related[0].span.unwrap();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "#secretValue"
        );
    }
}

#[test]
fn inaccessible_and_uninherited_private_names_are_not_suggestions() {
    assert_missing(
        "class C { #secretValue = 1; } new C().#secretVale;",
        "#secretVale",
        "C",
    );
    assert_missing("class Base { static #secretValue = 1; m() { Derived.#secretVale; } } class Derived extends Base {}", "#secretVale", "typeof Derived");
}

#[test]
fn valid_private_fields_methods_accessors_and_inheritance_remain_valid() {
    for source in [
        "class C { #field = 1; m() { this.#field; } }",
        "class C { #method() {} m() { this.#method(); } }",
        "class C { get #field() { return 1; } m() { this.#field; } }",
        "class Base { #field = 1; m() { new Derived().#field; } } class Derived extends Base {}",
        "class C { static #field = 1; static m() { this.#field; } }",
        "class C { '#field' = 1; m() { this['#field']; } }",
    ] {
        let errors = diagnostics("private.ts", source);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn captured_this_aliases_keep_the_outer_class_receiver() {
    for source in [
        "class Outer { #field = 1; m() { const captured = this; class Inner { read() { return captured.#field; } } } }",
        "class Outer { #field = 1; value = (() => { const captured = this; return class Inner { read() { return captured.#field; } }; })(); }",
        "class Outer { #field = 1; m() { const captured = this; class Inner { read() { const alias = captured; return alias.#field; } } } }",
        "class Outer { #field = 'key'; m() { const captured = this; class Inner { [captured.#field]() {} } } }",
        "class Outer { #field = 1; m() { const captured = this; class Inner { #other = 2; read(captured: Outer) { return captured.#field; } } } }",
    ] {
        let errors = diagnostics("private.ts", source);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
    assert_missing(
        "class Outer { #field = 1; m() { const captured = this; class Inner { #other = 2; read() { captured.#other; } } } }",
        "#other",
        "Outer",
    );
}

#[test]
fn anonymous_class_static_methods_use_the_constructor_receiver() {
    let source = "const C = class { static #field = 1; static #method() { return this.#field; } static m() { return this.#method(); } };";
    let errors = diagnostics("private.ts", source);
    assert!(errors.is_empty(), "{errors:?}");
}
