use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str, options: &CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("grammar.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut errors = file.diagnostics.clone();
    errors.extend(
        TypeChecker::new()
            .check_with_options(&file, &symbols, options)
            .diagnostics,
    );
    errors
}

#[test]
fn grammar_errors_do_not_suppress_comma_operand_diagnostics() {
    let source = "const value = { [0, 1]: {} };";
    let errors = diagnostics(source, &CompilerOptions::default());
    assert_eq!(errors.len(), 2, "{errors:?}");
    let grammar = errors
        .iter()
        .find(|d| d.code == 1171)
        .unwrap()
        .span
        .unwrap();
    let unused = errors
        .iter()
        .find(|d| d.code == 2695)
        .unwrap()
        .span
        .unwrap();
    assert_eq!(
        &source[grammar.start as usize..grammar.end as usize],
        "0, 1"
    );
    assert_eq!(&source[unused.start as usize..unused.end as usize], "0");
}

#[test]
fn unreachable_code_option_preserves_the_independent_grammar_error() {
    let options = CompilerOptions {
        allow_unreachable_code: Some(true),
        ..Default::default()
    };
    let errors = diagnostics("const value = { [0, 1]: {} };", &options);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 1171);
}
