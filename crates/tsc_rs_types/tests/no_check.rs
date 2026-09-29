use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn parse_and_bind(source: &str) -> (tsc_rs_ast::SourceFile, tsc_rs_symbols::SymbolTable) {
    let file = tsc_rs_parser::parse("no_check.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    (file, symbols)
}

fn semantic_codes(diagnostics: &[Diagnostic]) -> Vec<u32> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn no_check_option_suppresses_all_semantic_diagnostics() {
    let source = r#"const value: string = 1;
const regex = /\00/;
"#;
    let (file, symbols) = parse_and_bind(source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);

    let mut options = CompilerOptions::default();
    options.no_check = Some(true);
    let output = TypeChecker::new().check_with_options(&file, &symbols, &options);

    assert!(
        output.diagnostics.is_empty(),
        "noCheck must suppress both ordinary type errors and regex grammar errors: {:?}",
        semantic_codes(&output.diagnostics)
    );
    assert!(output.expression_types.is_empty());
    assert!(output.selected_overload_indices.is_empty());
    assert!(output.stable_types.is_empty());
}

#[test]
fn leading_ts_nocheck_suppresses_both_checker_entry_points() {
    let source = r#"// Copyright header
// @ts-nocheck: generated file
const value: string = 1;
const regex = /\00/;
"#;
    let (file, symbols) = parse_and_bind(source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);

    let without_options = TypeChecker::new().check(&file, &symbols);
    assert!(
        without_options.diagnostics.is_empty(),
        "check() must honor a leading @ts-nocheck pragma: {:?}",
        semantic_codes(&without_options.diagnostics)
    );

    let with_options =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
    assert!(
        with_options.diagnostics.is_empty(),
        "check_with_options() must honor a leading @ts-nocheck pragma: {:?}",
        semantic_codes(&with_options.diagnostics)
    );
}

#[test]
fn non_leading_ts_nocheck_text_does_not_disable_semantic_checking() {
    let source = r#"const first = 1;
// @ts-nocheck
const value: string = 1;
"#;
    let (file, symbols) = parse_and_bind(source);
    let output =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());

    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 2322),
        "only a pragma in leading trivia may suppress checking: {:?}",
        semantic_codes(&output.diagnostics)
    );
}
