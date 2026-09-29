use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source: &str, no_implicit_any: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse_with_jsx("spans.tsx", source, true);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                no_implicit_any: Some(no_implicit_any),
                ..Default::default()
            },
        )
        .diagnostics
}

#[test]
fn implicit_any_intrinsics_underline_the_entire_self_closing_element() {
    for element in [
        "<div />",
        "<My-widget disabled />",
        "<svg:path title={'text'} />",
        "<div\n  title='text'\n/>",
    ] {
        let source = format!("const x = {element};");
        let diagnostics = check(&source, true);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, 7026);
        let span = diagnostics[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], element);
        assert!(check(&source, false).is_empty());
    }
}

#[test]
fn intrinsic_span_does_not_expand_attribute_expression_errors() {
    let source = "const x = <div value={missing} />;";
    let diagnostics = check(source, true);
    assert_eq!(
        diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        [7026, 2304]
    );
    let intrinsic = diagnostics[0].span.unwrap();
    let value = diagnostics[1].span.unwrap();
    assert_eq!(
        &source[intrinsic.start as usize..intrinsic.end as usize],
        "<div value={missing} />"
    );
    assert_eq!(&source[value.start as usize..value.end as usize], "missing");
}

#[test]
fn paired_intrinsic_diagnostics_cover_each_tag_without_children() {
    let source = "const x = <div title='text'>hello<span /></div>;";
    let diagnostics = check(source, true);
    assert_eq!(
        diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        [7026, 7026, 7026]
    );
    let mut spans: Vec<_> = diagnostics.iter().map(|d| d.span.unwrap()).collect();
    spans.sort_by_key(|span| span.start);
    let text: Vec<_> = spans
        .iter()
        .map(|span| &source[span.start as usize..span.end as usize])
        .collect();
    assert_eq!(text, ["<div title='text'>", "<span />", "</div>"]);
}

#[test]
fn paired_component_tags_check_names_and_qualified_properties_at_both_locations() {
    for (source, code, name) in [
        ("const x = <Missing></Missing>;", 2304, "Missing"),
        (
            "const View = { present: () => null }; const x = <View.Missing></View.Missing>;",
            2339,
            "Missing",
        ),
        (
            "class View { render() { return <this.missing></this.missing>; } }",
            2339,
            "missing",
        ),
    ] {
        let diagnostics = check(source, false);
        assert_eq!(
            diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            [code, code],
            "{source}: {diagnostics:?}"
        );
        let positions: Vec<_> = source
            .match_indices(name)
            .map(|(pos, _)| pos as u32)
            .collect();
        for (diagnostic, start) in diagnostics.iter().zip(positions) {
            let span = diagnostic.span.unwrap();
            assert_eq!(span.start, start);
            assert_eq!(&source[span.start as usize..span.end as usize], name);
        }
    }
}

#[test]
fn missing_closing_tags_do_not_invent_component_references() {
    for source in [
        "const x = <Missing>",
        "const Outer = () => null; const x = <Outer><Missing></Outer>;",
    ] {
        let file = tsc_rs_parser::parse("spans.tsx", source);
        assert!(file.diagnostics.iter().any(|d| d.code == 17008));
        let symbols = tsc_rs_symbols::bind(&file);
        let output = TypeChecker::new().check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                no_implicit_any: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(
            output.diagnostics.len(),
            1,
            "{source}: {:?}",
            output.diagnostics
        );
        assert_eq!(output.diagnostics[0].code, 2304);
        let span = output.diagnostics[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "Missing");
    }
}
