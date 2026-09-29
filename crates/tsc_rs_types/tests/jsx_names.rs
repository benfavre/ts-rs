use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source: &str) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let file = tsc_rs_parser::parse_with_jsx("names.tsx", source, true);
    let symbols = tsc_rs_symbols::bind(&file);
    let output = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            no_implicit_any: Some(false),
            ..CompilerOptions::default()
        },
    );
    (file.diagnostics, output.diagnostics)
}

#[test]
fn escaped_jsx_components_resolve_their_decoded_names() {
    let (parse, semantic) = check(
        r"const Compa = (x: {x: number}) => null; const x = {video: () => null}; <Comp\u0061 x={12} />; <x.\u0076ideo />;",
    );
    assert_eq!(
        parse.iter().map(|d| d.code).collect::<Vec<_>>(),
        [17021, 17021]
    );
    assert!(semantic.is_empty(), "{semantic:?}");
}

#[test]
fn jsx_escape_errors_do_not_hide_unresolved_expressions() {
    let (parse, semantic) = check(r"<M\u0069ssing />; <\u0061 value={missing} />;");
    assert_eq!(
        parse.iter().map(|d| d.code).collect::<Vec<_>>(),
        [17021, 17021]
    );
    let messages: Vec<_> = semantic
        .iter()
        .filter(|d| d.code == 2304)
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            r"Cannot find name 'M\u0069ssing'.",
            "Cannot find name 'missing'."
        ]
    );
}

#[test]
fn uppercase_hyphenated_and_namespaced_tags_are_intrinsic() {
    let (parse, semantic) = check("<My-tag />; <Svg:Rect />;");
    assert!(parse.is_empty(), "{parse:?}");
    assert!(semantic.is_empty(), "{semantic:?}");
}

#[test]
fn escaped_namespace_parts_remain_intrinsic() {
    let (parse, semantic) = check(r"<S\u0076g:Rect />; <Svg:R\u0065ct />;");
    assert_eq!(
        parse.iter().map(|d| d.code).collect::<Vec<_>>(),
        [17021, 17021]
    );
    assert!(semantic.is_empty(), "{semantic:?}");
}

#[test]
fn intrinsic_names_still_require_jsx_typings_under_no_implicit_any() {
    let file = tsc_rs_parser::parse_with_jsx("names.tsx", "<My-tag />; <Svg:Rect />;", true);
    let symbols = tsc_rs_symbols::bind(&file);
    let output = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [7026, 7026]
    );
}

#[test]
fn missing_escaped_jsx_properties_report_source_spelling_and_span() {
    let source = r"const x = {good: () => null}; <x.\u0062ad />;";
    let (_, semantic) = check(source);
    assert_eq!(semantic.len(), 1, "{semantic:?}");
    let diagnostic = &semantic[0];
    assert_eq!(diagnostic.code, 2339);
    assert!(diagnostic
        .message
        .starts_with(r"Property '\u0062ad' does not exist on type "));
    let span = diagnostic.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], r"\u0062ad");
}

#[test]
fn escaped_jsx_property_hover_uses_the_source_token_start() {
    let source = r"const x = {video: () => null}; <x.\u0076ideo />;";
    let file = tsc_rs_parser::parse_with_jsx("names.tsx", source, true);
    let symbols = tsc_rs_symbols::bind(&file);
    let output = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            no_implicit_any: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let start = source.find(r"\u0076ideo").unwrap() as u32;
    assert!(
        output
            .expression_types
            .get(&start)
            .is_some_and(|ty| ty.contains("=>")),
        "{:?}",
        output.expression_types
    );
    assert!(!output.expression_types.contains_key(&(start + 5)));
}

#[test]
fn jsx_this_components_resolve_instance_members() {
    let (parse, semantic) = check(
        "class View { Component = () => null; render() { return <this.Component />; } paired() { return <this.Component></this.Component>; } }",
    );
    assert!(parse.is_empty(), "{parse:?}");
    assert!(semantic.is_empty(), "{semantic:?}");
}

#[test]
fn jsx_this_components_still_report_missing_properties() {
    for source in [
        "class View { render() { return <this.missing />; } }",
        "class View { static render() { return <this.missing />; } }",
        "class View { render() { return this.missing; } }",
        "class View { static render() { return this.missing; } }",
    ] {
        let (parse, semantic) = check(source);
        assert!(parse.is_empty(), "{parse:?}");
        assert_eq!(semantic.len(), 1, "{source}: {semantic:?}");
        assert_eq!(semantic[0].code, 2339);
        assert!(semantic[0]
            .message
            .starts_with("Property 'missing' does not exist on type "));
        let span = semantic[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "missing");
    }
}
