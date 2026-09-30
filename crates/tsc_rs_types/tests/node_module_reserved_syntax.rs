use tsc_rs_ast::{CompilerOptions, Diagnostic, ModuleKind};
use tsc_rs_types::TypeChecker;

fn check(name: &str, source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(name, source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

fn reserved_spans<'a>(source: &'a str, diagnostics: &[Diagnostic]) -> Vec<(u32, &'a str)> {
    diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code, 7059 | 7060))
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect()
}

#[test]
fn node_extensions_reject_ambiguous_arrows_and_nested_assertions() {
    let source = "const x = <T>() => <T><any>(void 0);";
    for name in ["index.mts", "index.cts"] {
        let diagnostics = check(name, source, CompilerOptions::default());
        assert_eq!(
            reserved_spans(source, &diagnostics),
            [
                (7060, "T"),
                (7059, "<T><any>(void 0)"),
                (7059, "<any>(void 0)")
            ]
        );
        assert!(diagnostics.iter().filter(|d| d.code == 7059).all(|d| d.message == "This syntax is reserved in files with the .mts or .cts extension. Use an `as` expression instead."));
        assert!(diagnostics.iter().filter(|d| d.code == 7060).all(|d| d.message == "This syntax is reserved in files with the .mts or .cts extension. Add a trailing comma or explicit constraint."));
    }
}

#[test]
fn ordinary_typescript_permits_the_syntax_even_in_node_next_modules() {
    let source = "const x = <T>() => <T><any>(void 0);";
    let diagnostics = check(
        "index.ts",
        source,
        CompilerOptions {
            module: Some(ModuleKind::NodeNext),
            ..CompilerOptions::default()
        },
    );
    assert!(reserved_spans(source, &diagnostics).is_empty());
}

#[test]
fn commas_constraints_and_multiple_parameters_disambiguate_arrows() {
    let source = "const a = <T,>() => 1; const b = <T extends unknown>() => 1; const c = <T, U>() => 1; const d = <T /* comment */,>() => 1; function f<T>() { return 1; }";
    let diagnostics = check("index.mts", source, CompilerOptions::default());
    assert!(
        reserved_spans(source, &diagnostics).is_empty(),
        "{diagnostics:#?}"
    );
}

#[test]
fn defaults_modifiers_and_comments_do_not_disambiguate_arrows() {
    let source = "const a = <T = any>() => 1; const b = <const T>() => 1; const c = <T /* comma, in comment */>() => 1;";
    let diagnostics = check("index.cts", source, CompilerOptions::default());
    assert_eq!(
        reserved_spans(source, &diagnostics),
        [(7060, "T = any"), (7060, "const T"), (7060, "T")]
    );
}

#[test]
fn contextually_typed_arrows_are_checked_once() {
    let source = "const fn: <T>() => number = <T>() => 1;";
    let diagnostics = check("index.mts", source, CompilerOptions::default());
    assert_eq!(reserved_spans(source, &diagnostics), [(7060, "T")]);
}

#[test]
fn as_assertions_are_allowed_but_angle_const_assertions_are_reserved() {
    let source = "const a = 1 as number; const b = <const>[1, 2];";
    let diagnostics = check("index.cts", source, CompilerOptions::default());
    assert_eq!(
        reserved_spans(source, &diagnostics),
        [(7059, "<const>[1, 2]")]
    );
}

#[test]
fn no_check_and_parser_errors_suppress_reserved_syntax_diagnostics() {
    let source = "const x = <T>() => <T><any>(void 0);";
    let diagnostics = check(
        "index.mts",
        source,
        CompilerOptions {
            no_check: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");

    let source = format!("{source} const broken = ;");
    let file = tsc_rs_parser::parse("index.mts", &source);
    assert!(!file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    assert!(
        reserved_spans(&source, &diagnostics).is_empty(),
        "{diagnostics:#?}"
    );
}
