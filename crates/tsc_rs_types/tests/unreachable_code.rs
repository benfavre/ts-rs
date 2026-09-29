use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

/// Source text of every TS7027 span under `allowUnreachableCode: false`.
fn unreachable(source: &str, preserve_const_enums: bool) -> Vec<String> {
    let file = tsc_rs_parser::parse("unreachable.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        allow_unreachable_code: Some(false),
        preserve_const_enums: Some(preserve_const_enums),
        ..CompilerOptions::default()
    };
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == 7027)
        .map(|d| {
            let span = d.span.unwrap();
            source[span.start as usize..span.end as usize].into()
        })
        .collect()
}

#[test]
fn namespace_bodies_and_declarations_that_emit() {
    assert_eq!(
        unreachable("namespace A {\n    while (true);\n    let x;\n}", false),
        ["let x;"]
    );
    // An uninstantiated namespace and a const enum emit nothing.
    assert!(unreachable(
        "function f() { return; namespace N { interface I {} } }",
        false
    )
    .is_empty());
    assert!(unreachable("function f() { return; const enum E { X } }", false).is_empty());
    assert_eq!(
        unreachable("function f() { return; const enum E { X } }", true),
        ["const enum E { X }"]
    );
}

#[test]
fn nothing_inside_reported_code_is_reported_again() {
    assert_eq!(
        unreachable(
            "while (true);\nnamespace A {\n    while (true);\n    let x;\n}",
            false
        ),
        ["namespace A {\n    while (true);\n    let x;\n}"]
    );
}

#[test]
fn exhaustive_typeof_switch_in_an_arrow_ends_the_flow() {
    let source = "const f = (x: any): number => {\n    switch (typeof x) {\n        case 'string': return 0\n        case 'number': return 0\n        case 'bigint': return 0\n        case 'boolean': return 0\n        case 'symbol': return 0\n        case 'undefined': return 0\n        case 'object': return 0\n        case 'function': return 0\n    }\n    x;\n}";
    assert_eq!(unreachable(source, false), ["x;"]);
}
