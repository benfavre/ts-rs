use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("loop.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

#[test]
fn for_in_checks_undeclared_targets_and_invalid_references() {
    for (target, expected) in [
        ("missing", vec![2304]),
        ("missing()", vec![2304, 2406]),
        ("new missing()", vec![2304, 2406]),
        ("missing = 1", vec![2304, 2405]),
        ("this", vec![2405]),
        ("1", vec![2405]),
        ("({})", vec![2406]),
    ] {
        let source = format!("declare const values: any; for ({target} in values) {{}}");
        let errors = diagnostics(&source);
        let mut codes: Vec<_> = errors.iter().map(|error| error.code).collect();
        codes.sort_unstable();
        assert_eq!(codes, expected, "{source}: {errors:?}");
        for error in errors
            .iter()
            .filter(|error| matches!(error.code, 2405 | 2406))
        {
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], target);
        }
    }
}

#[test]
fn for_in_rejects_binding_and_assignment_destructuring() {
    for target in ["{}", "[]", "var {}", "let []"] {
        let source = format!("declare const values: any; for ({target} in values) {{}}");
        let errors = diagnostics(&source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2491);
        let span = errors[0].span.unwrap();
        assert!(matches!(
            &source[span.start as usize..span.end as usize],
            "{}" | "[]"
        ));
    }
    for (target, missing_code) in [("{ missing }", 18004), ("[missing]", 2304)] {
        let source = format!("declare const values: any; for ({target} in values) {{}}");
        let errors = diagnostics(&source);
        assert_eq!(errors.len(), 2, "{source}: {errors:?}");
        assert!(errors.iter().any(|error| error.code == 2491));
        assert!(errors.iter().any(|error| error.code == missing_code));
    }
}

#[test]
fn for_in_uses_the_existing_target_type_and_preserves_it_in_the_body() {
    for target_type in ["string", "any", "unknown", "string | number"] {
        let source =
            format!("declare const values: any; let key: {target_type}; for (key in values) {{}}");
        let errors = diagnostics(&source);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
    for target in ["key", "(key)"] {
        let source = format!(
            "declare const values: any; let key: number; for ({target} in values) {{ const n: number = key; }}"
        );
        let errors = diagnostics(&source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2405);
    }
    for target in ["key", "(key)"] {
        let source = format!(
            "declare const values: any; let key: string | number = 0; for ({target} in values) {{ const value: string = key; }}"
        );
        assert!(
            diagnostics(&source).is_empty(),
            "{source}: {:?}",
            diagnostics(&source)
        );
    }
}

#[test]
fn for_in_distinguishes_property_access_from_optional_access() {
    for (target, expected) in [
        ("holder.key", None),
        ("holder['key']", None),
        ("holder?.key", Some(2780)),
        ("holder?.['key']", Some(2780)),
        ("holder.count", Some(2405)),
        ("holder?.count", Some(2405)),
    ] {
        let source = format!(
            "declare const values: any; declare const holder: {{ key: string; count: number }}; for ({target} in values) {{}}"
        );
        let errors = diagnostics(&source);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            expected.into_iter().collect::<Vec<_>>(),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn for_in_assignment_targets_are_not_read_before_assignment() {
    let source = "function f(values: object) { let key: string; for ((key) in values) { const value: string = key; } }";
    let file = tsc_rs_parser::parse("loop.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let errors = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(true),
                ..Default::default()
            },
        )
        .diagnostics;
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn parentheses_end_optional_chains_before_a_property_target() {
    for (target, expected) in [
        ("(holder?.inner)!.key", vec![]),
        ("holder?.inner.key", vec![2780]),
        ("(holder.inner?.key)", vec![2780]),
    ] {
        let source = format!(
            "declare const values: any; declare const holder: {{inner: {{key: string}}}}; for ({target} in values) {{}}"
        );
        let errors = diagnostics(&source);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            expected,
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn for_in_preserves_the_keys_of_generic_mapped_objects() {
    for source in [
        "function f<K extends string, T>(values: { [P in K]: T }, key: K) { for (key in values) { const k: K = key; } }",
        "function f<T, K extends keyof T>(values: { [P in K]: T[P] }, key: K) { for (key in values) { const k: K = key; } }",
        "let key: 'value'; for (key in { value: 1 }) {}",
    ] {
        let errors = diagnostics(source);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
    for source in [
        "let key: 'other'; for (key in { value: 1 }) {}",
        "function f<K extends string>(key: K) { for (key in { value: 1 }) {} }",
        "declare const values: {[key: string]: number; value: number}; let key: 'value'; for (key in values) {}",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2405);
    }
}

#[test]
fn readonly_for_in_targets_do_not_cascade_into_type_errors() {
    for target in ["holder.key", "holder['key']", "(holder.key)"] {
        let source = format!(
            "declare const holder: {{ readonly key: number }}; for ({target} in {{}}) {{}}"
        );
        let errors = diagnostics(&source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2540);
    }
}
