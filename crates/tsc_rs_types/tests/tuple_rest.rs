use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<tsc_rs_ast::Diagnostic> {
    let file = tsc_rs_parser::parse("tuple_rest.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

#[test]
fn tuple_rest_assertions_and_literal_lengths_match_typescript() {
    let diagnostics = diagnostics(
        r#"
declare const strings: string[];
const ok = strings as [string, ...string[]];
const bad = [1, 2] as [string, ...string[]];

const literalOk: [string, ...string[]] = ["a", "b"];
const literalBad: [string, ...string[]] = [];
"#,
    );

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2352)
            .count(),
        1,
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2322)
            .count(),
        1,
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("[string, ...string[]]")),
        "tuple rest marker was lost: {diagnostics:?}"
    );
}
