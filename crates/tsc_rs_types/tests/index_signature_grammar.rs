use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<(u32, String)> {
    let file = tsc_rs_parser::parse("indexes.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
    file.diagnostics
        .into_iter()
        .chain(result.diagnostics)
        .filter(|d| matches!(d.code, 1021 | 1268 | 1337))
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                source[span.start as usize..span.end as usize].into(),
            )
        })
        .collect()
}

#[test]
fn index_key_types_are_checked_in_classes_interfaces_and_nested_annotations() {
    for (key, expected) in [
        ("string", None),
        ("number", None),
        ("symbol", None),
        ("string | number | symbol", None),
        ("string | 'x'", None),
        ("string & {}", None),
        ("`prefix-${string}`", None),
        ("`${boolean}-${string}`", None),
        ("keyof any", None),
        ("boolean", Some(1268)),
        ("false", Some(1268)),
        ("any", Some(1268)),
        ("any | 'x'", Some(1268)),
        ("unknown | 'x'", Some(1268)),
        ("unknown", Some(1268)),
        ("never", Some(1268)),
        ("string & number", Some(1268)),
        ("'x'", Some(1337)),
        ("1", Some(1337)),
        ("'x' | 'y'", Some(1337)),
        ("'x' & {}", Some(1337)),
        ("`prefix-${boolean}`", Some(1337)),
        ("keyof { x: number }", Some(1337)),
    ] {
        for prefix in ["class C", "interface I", "type X =", "let value:"] {
            let source = format!("{prefix} {{ [key: {key}]: any; }}");
            let expected: Vec<_> = expected
                .into_iter()
                .map(|code| (code, "key".into()))
                .collect();
            assert_eq!(diagnostics(&source), expected, "{source}");
        }
    }
}

#[test]
fn index_key_aliases_generics_and_missing_result_annotations() {
    for (source, expected) in [
        (
            "type K = string; interface I { [key: K & number]: any; }",
            vec![(1268, "key")],
        ),
        (
            "enum E { A } interface I { [key: E.A]: any; }",
            vec![(1337, "key")],
        ),
        (
            "declare const s: unique symbol; interface I { [key: typeof s]: any; }",
            vec![(1337, "key")],
        ),
        ("type K = string; interface I { [key: K]: any; }", vec![]),
        (
            "type K = string; interface I { [key: K | 'x']: any; }",
            vec![],
        ),
        (
            "type K = 'x'; interface I { [key: K]: any; }",
            vec![(1337, "key")],
        ),
        (
            "interface I<T extends string> { [key: T]: any; }",
            vec![(1337, "key")],
        ),
        (
            "type I<T extends string> = { [key: `x${T}`]: any; };",
            vec![(1337, "key")],
        ),
        ("class C<T> { [key: keyof T]: any; }", vec![(1337, "key")]),
        (
            "interface I { [key: string]; }",
            vec![(1021, "[key: string];")],
        ),
        ("interface I { [key: boolean]; }", vec![(1268, "key")]),
        ("interface I { [key?: boolean]; }", vec![]),
        ("interface I { [key: boolean,]: any; }", vec![(1268, "key")]),
        ("interface I { [key: boolean]: any; x: ; }", vec![]),
    ] {
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(code, span)| (code, span.into()))
            .collect();
        assert_eq!(diagnostics(source), expected, "{source}");
    }
}
