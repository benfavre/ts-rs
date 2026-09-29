use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

#[test]
fn definite_assignment_reports_each_var_and_let_use_once() {
    let source = r#"
var functionScoped: number;
functionScoped.toFixed();

let blockScoped: number;
blockScoped.toFixed();

var narrowed: string | number;
if (typeof narrowed === "string") {
    narrowed.toUpperCase();
}

var bufferCandidate: Object;
if (ArrayBuffer.isView(bufferCandidate)) {
    const view: ArrayBufferView = bufferCandidate;
}
"#;
    let file = tsc_rs_parser::parse("definite_assignment_var.ts", source);
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
    let definite_assignment: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2454)
        .collect();

    assert_eq!(definite_assignment.len(), 4, "diagnostics: {diagnostics:?}");
    for name in [
        "functionScoped",
        "blockScoped",
        "narrowed",
        "bufferCandidate",
    ] {
        assert_eq!(
            definite_assignment
                .iter()
                .filter(|diagnostic| diagnostic.message.contains(name))
                .count(),
            1,
            "diagnostics: {diagnostics:?}"
        );
    }
}
