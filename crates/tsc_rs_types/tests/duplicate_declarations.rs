use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(file_name: &str, source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(false),
                allow_js: Some(true),
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

// Expected spans and messages were checked against TypeScript 6.0.3.
fn duplicate_spans(file_name: &str, source: &str) -> Vec<(usize, String)> {
    let mut duplicates: Vec<_> = diagnostics(file_name, source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == 2393)
        .map(|diagnostic| {
            assert_eq!(diagnostic.message, "Duplicate function implementation.");
            let span = diagnostic.span.unwrap();
            (
                span.start as usize,
                source[span.start as usize..span.end as usize].to_owned(),
            )
        })
        .collect();
    duplicates.sort();
    duplicates
}

#[test]
fn reports_both_function_bodies() {
    let source = "function f() {} function f() {}";
    let result = diagnostics("case.ts", source);
    assert_eq!(result.len(), 2, "{result:?}");
    assert_eq!(
        duplicate_spans("case.ts", source),
        [(9, "f".into()), (25, "f".into())]
    );
}

#[test]
fn duplicate_implementations_also_report_the_overload_signature() {
    let source = "function f(x: string): void; function f(x: any) {} function f(x: any) {}";
    assert_eq!(
        duplicate_spans("case.ts", source),
        [(9, "f".into()), (38, "f".into()), (60, "f".into())]
    );
}

#[test]
fn legal_overloads_and_separate_lexical_scopes_do_not_conflict() {
    for source in [
        "function f(x: string): void; function f(x: number): void; function f(x: any) {}",
        "declare function f(x: string): void; declare function f(x: number): void;",
        "function f() {} { function f() {} }",
        "function f() {} function outer() { function f() {} }",
        "namespace A { export function f() {} } namespace B { export function f() {} }",
    ] {
        assert!(
            duplicate_spans("case.ts", source).is_empty(),
            "unexpected duplicate implementation in {source}"
        );
    }
}

#[test]
fn duplicate_local_functions_are_checked_in_their_own_scope() {
    for source in [
        "function outer() { function f() {} function f() {} }",
        "namespace N { function f() {} function f() {} }",
        "{ function f() {} function f() {} }",
    ] {
        let expected: Vec<_> = source
            .match_indices("function f")
            .map(|(start, _)| (start + "function ".len(), "f".into()))
            .collect();
        assert_eq!(duplicate_spans("case.ts", source), expected, "{source}");
    }
}

#[test]
fn ambient_context_does_not_hide_duplicate_bodies() {
    let source = "declare namespace N { function f() {} function f() {} }";
    assert_eq!(
        duplicate_spans("case.ts", source),
        [(31, "f".into()), (47, "f".into())]
    );
}

#[test]
fn default_functions_share_an_export_name_and_keep_their_local_name() {
    for (source, expected) in [
        (
            "export default function() {} export default function() {}",
            vec![(0, "export".into()), (29, "export".into())],
        ),
        (
            "export default function f() {} export default function g() {}",
            vec![(24, "f".into()), (55, "g".into())],
        ),
        (
            "export default function f() {} function f() {}",
            vec![(24, "f".into()), (40, "f".into())],
        ),
        (
            "export default function f() {} export default function f() {}",
            vec![(24, "f".into()), (55, "f".into())],
        ),
    ] {
        assert_eq!(duplicate_spans("case.ts", source), expected, "{source}");
    }
}

#[test]
fn javascript_allows_repeated_function_declarations() {
    assert!(duplicate_spans("case.js", "function f() {} function f() {}").is_empty());
}
