use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("namespace.ts", source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

#[test]
fn default_declarations_report_the_modifier_and_still_check_the_body() {
    let source = "namespace N { export /* default in a comment */ default class C { value: number = 'bad'; } } namespace M { export default function f() { const value: number = 'bad'; } }";
    let diagnostics = check(source);
    let defaults: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1319)
        .map(|diagnostic| {
            assert_eq!(
                diagnostic.message,
                "A default export can only be used in an ECMAScript-style module."
            );
            let span = diagnostic.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(defaults, ["default", "default"]);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2322)
            .count(),
        2,
        "{diagnostics:#?}"
    );
}

#[test]
fn namespace_export_assignments_report_the_whole_statement_without_checking_the_value() {
    let source = "namespace N { export default missing; } namespace M { export = alsoMissing; }";
    let diagnostics = check(source);
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (1319, "export default missing;"),
            (1063, "export = alsoMissing;")
        ]
    );
}

#[test]
fn ambient_and_dotted_namespaces_are_internal_modules() {
    let source = "declare namespace N { export default class C {} } namespace A.B { export default function f() {} } namespace global { export default class G {} }";
    let diagnostics = check(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1319)
            .count(),
        3,
        "{diagnostics:#?}"
    );
}

#[test]
fn source_file_external_modules_and_global_augmentations_allow_default_exports() {
    for source in [
        "export default class C {}",
        "export default 1;",
        "declare module 'm' { export default class C {} }",
        "declare module 'm' { const value: number; export = value; }",
        "export {}; declare global { export default class C {} }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.code, 1063 | 1319)),
            "{source}: {diagnostics:#?}"
        );
    }
}

#[test]
fn an_internal_namespace_inside_an_external_module_still_rejects_default_exports() {
    let diagnostics = check("declare module 'm' { namespace N { export default class C {} } } namespace O { declare module 'n' { export default class D {} } }");
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1319)
            .count(),
        1,
        "{diagnostics:#?}"
    );
}

#[test]
fn no_check_disables_namespace_export_diagnostics() {
    let file = tsc_rs_parser::parse(
        "namespace.ts",
        "namespace N { export default class C {} export = missing; }",
    );
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                no_check: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}
