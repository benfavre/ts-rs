use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

/// (code, related code, text at the related span) for each related note.
fn notes(source: &str) -> Vec<(u32, u32, String)> {
    let file = tsc_rs_parser::parse("notes.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let result =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
    let mut out = Vec::new();
    for diagnostic in result.diagnostics {
        for related in diagnostic.related.iter().flatten() {
            let span = related.span.unwrap();
            out.push((
                diagnostic.code,
                related.code,
                source[span.start as usize..span.end as usize].into(),
            ));
        }
    }
    out
}

#[test]
fn missing_property_of_anonymous_type_points_at_its_declaration() {
    assert_eq!(
        notes(
            "declare let x: { (): string; prop: number };\ndeclare let y: { (): string };\nx = y;"
        ),
        [(2741, 2728, "prop".into())]
    );
    assert_eq!(
        notes("var x = { one: 1 };\ndeclare var y: { [index: string]: any };\nx = y;"),
        [(2741, 2728, "one".into())]
    );
}

#[test]
fn unconstrained_type_parameter_source_suggests_a_constraint() {
    let found = notes("function f<T, U>(t: T, u: U) { t = u; }");
    assert_eq!(found, [(2322, 2208, "U".into())]);
    // A constrained source gets no suggestion.
    assert!(
        notes("function f<T, U extends string>(t: T, u: U) { t = u; }")
            .iter()
            .all(|(_, related, _)| *related != 2208)
    );
}

#[test]
fn failed_overloads_point_at_an_implementation_that_would_accept_the_call() {
    assert_eq!(
        notes(
            "function foo(bar: string): string;\nfunction foo(bar: number): number;\nfunction foo(bar: any): any { return bar }\nfoo(true);"
        ),
        [(2769, 2793, "foo".into())]
    );
    // One overload of the right arity reports its own argument error.
    assert_eq!(
        notes("function foo(): string;\nfunction foo(bar: string): number;\nfunction foo(bar?: any): any { return '' }\nfoo(5);"),
        [(2345, 2793, "foo".into())]
    );
    // An implementation that would reject the call adds nothing.
    assert!(notes(
        "function foo(bar: string): string;\nfunction foo(bar: number): number;\nfunction foo(bar: string | number): any { return bar }\nfoo(true);"
    )
    .is_empty());
}
