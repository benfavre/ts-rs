use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

#[test]
fn assignment_in_nullish_if_branch_narrows_the_join() {
    let source = r#"
let assigned: { ready: boolean } | null = null;
function valid() {
    if (!assigned) {
        assigned = { ready: true };
    }
    return assigned.ready;
}

let untouched: { ready: boolean } | null = null;
function invalid() {
    if (!untouched) {
        const unrelated = true;
    }
    return untouched.ready;
}
"#;
    let file = tsc_rs_parser::parse("control_flow_assignment_join.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 18047)
            .count(),
        1,
        "diagnostics: {diagnostics:?}"
    );
}
