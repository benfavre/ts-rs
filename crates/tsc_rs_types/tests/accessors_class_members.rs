use tsc_rs_parser::parse;
use tsc_rs_symbols::bind;
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<tsc_rs_ast::Diagnostic> {
    let file = parse("accessors.ts", source);
    let symbols = bind(&file);
    TypeChecker::new().check(&file, &symbols).diagnostics
}

#[test]
fn setter_parameters_reject_initializers_and_rest() {
    let source = r#"
class Declaration {
    set declarationInitializer(value = 0) {}
    set declarationRest(...values: number[]) {}
}
const Expression = class {
    set expressionInitializer(value = 0) {}
    set expressionRest(...values: number[]) {}
};
const object = {
    set objectInitializer(value = 0) {},
    set objectRest(...values: number[]) {},
};
"#;
    let diagnostics = diagnostics(source);
    let initializers: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1052)
        .collect();
    let rests: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1053)
        .collect();

    assert_eq!(initializers.len(), 3, "diagnostics: {diagnostics:?}");
    assert_eq!(rests.len(), 3, "diagnostics: {diagnostics:?}");
    let initializer_names: Vec<_> = initializers
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("initializer span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        initializer_names,
        [
            "declarationInitializer",
            "expressionInitializer",
            "objectInitializer"
        ]
    );
    assert!(rests.iter().all(|diagnostic| {
        let span = diagnostic.span.expect("rest span");
        &source[span.start as usize..span.end as usize] == "..."
    }));
}

#[test]
fn invalid_setter_arity_takes_precedence_over_parameter_form_diagnostics() {
    let diagnostics = diagnostics(
        r#"
class Declaration {
    set initialized(first = 0, second = 1) {}
    set rest(first: number, ...others: number[]) {}
}
const Expression = class {
    set initialized(first = 0, second = 1) {}
};
const object = {
    set rest(first: number, ...others: number[]) {},
};
"#,
    );

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code, 1052 | 1053))
            .count(),
        0,
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ordinary_parameters_keep_initializer_and_rest_support() {
    let diagnostics = diagnostics(
        r#"
class Example {
    method(value = 0, ...rest: number[]) {}
    constructor(value = 0, ...rest: number[]) {}
}
function standalone(value = 0, ...rest: number[]) {}
"#,
    );

    assert!(!diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic.code, 1052 | 1053)));
}
