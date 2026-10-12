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

#[test]
fn typescript_constructor_assignments_do_not_declare_properties() {
    let source = "class Box { read() { return this.value; } constructor() { this.value = 1; } }";
    let errors = diagnostics("box.ts", source);
    assert_eq!(errors.len(), 2, "{errors:?}");
    for error in errors {
        assert_eq!(error.code, 2339, "{error:?}");
        assert_eq!(
            error.message,
            "Property 'value' does not exist on type 'Box'."
        );
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "value");
    }
}

#[test]
fn typescript_constructor_typos_suggest_declared_fields() {
    let source = "class Box { currentValue = 0; constructor() { this.curentValue = 1; } }";
    let errors = diagnostics("box.ts", source);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2551, "{errors:?}");
    assert_eq!(
        errors[0].message,
        "Property 'curentValue' does not exist on type 'Box'. Did you mean 'currentValue'?"
    );
    let declaration = errors[0].related.as_ref().unwrap()[0].span.unwrap();
    assert_eq!(
        &source[declaration.start as usize..declaration.end as usize],
        "currentValue"
    );
}

#[test]
fn javascript_constructor_assignments_still_declare_properties() {
    let source = "class Box { read() { return this.value; } constructor() { this.value = 1; } } const value = new Box().value;";
    let errors = diagnostics("box.js", source);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn declared_parameter_and_inherited_properties_accept_assignments() {
    let source = "class Base { inherited = 0; } class Box extends Base { declared: number; constructor(public parameter: number) { super(); this.declared = 1; this.parameter = 2; this.inherited = 3; } }";
    let errors = diagnostics("box.ts", source);
    assert!(errors.is_empty(), "{errors:?}");
}
