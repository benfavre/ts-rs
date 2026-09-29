use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

#[test]
fn explicit_lib_with_no_lib_still_reports_the_required_globals() {
    let file = tsc_rs_parser::parse("nolib_globals.ts", "");
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                no_lib: Some(true),
                lib: vec!["es5".to_string()],
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    let missing_globals: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2318)
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();

    assert_eq!(
        missing_globals,
        [
            "Cannot find global type 'Array'.",
            "Cannot find global type 'Boolean'.",
            "Cannot find global type 'CallableFunction'.",
            "Cannot find global type 'Function'.",
            "Cannot find global type 'IArguments'.",
            "Cannot find global type 'NewableFunction'.",
            "Cannot find global type 'Number'.",
            "Cannot find global type 'Object'.",
            "Cannot find global type 'RegExp'.",
            "Cannot find global type 'String'.",
        ],
        "diagnostics: {diagnostics:?}"
    );
}
