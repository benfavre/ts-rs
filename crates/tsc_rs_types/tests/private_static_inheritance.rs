use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("private.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

#[test]
fn derived_constructors_do_not_inherit_private_static_fields() {
    for source in [
        "class Base { static #field = 1; read() { Base.#field; Derived.#field; } } class Derived extends Base {}",
        "class Base { static #field = 1; } class Derived extends Base {} Derived.#field;",
        "class Base { static #field = 1; read() { Derived.#field; } } class Derived extends Base { #field = 2; }",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2339, "{source}: {errors:?}");
        assert_eq!(errors[0].message, "Property '#field' does not exist on type 'typeof Derived'.");
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "#field");
    }
}

#[test]
fn private_static_accessors_and_methods_are_not_inherited() {
    for declaration in [
        "static get #field() { return 1; }",
        "static set #field(value: number) {}",
        "static #field() {}",
    ] {
        let source = format!("class Base {{ {declaration} read() {{ Derived.#field; }} }} class Derived extends Base {{}}");
        let errors = diagnostics(&source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2339, "{source}: {errors:?}");
        assert_eq!(
            errors[0].message,
            "Property '#field' does not exist on type 'typeof Derived'."
        );
    }
}

#[test]
fn instance_private_names_and_public_statics_keep_their_inheritance() {
    for source in [
        "class Base { #field = 1; read() { new Derived().#field; } } class Derived extends Base {}",
        "class Base { static field = 1; } class Derived extends Base {} Derived.field;",
        "class Base { static #field = 1; } class Derived extends Base { static #field = 2; read() { Derived.#field; } }",
    ] {
        let errors = diagnostics(source);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}
