use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn ts1100_count(source: &str, always_strict: bool) -> usize {
    let file = tsc_rs_parser::parse("strict_mode_arguments.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                always_strict: Some(always_strict),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1100)
        .count()
}

#[test]
fn strict_mode_rejects_arguments_bindings_and_assignment_targets() {
    let source = r#"
var arguments = 1;
function f() {
    var arguments = [];
    arguments = 1;
}
const arrow = () => {
    const arguments = undefined;
    return { arguments: [], arguments };
};
"#;

    assert_eq!(ts1100_count(source, true), 4);
    assert_eq!(ts1100_count(source, false), 0);
}
