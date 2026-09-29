use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source: &str) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let file = tsc_rs_parser::parse("attributes.tsx", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let output = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            no_implicit_any: Some(false),
            ..Default::default()
        },
    );
    (file.diagnostics, output.diagnostics)
}

#[test]
fn jsx_attribute_grammar_reports_the_first_duplicate_or_empty_container() {
    for (source, code, text) in [
        ("<div x={} y={} />;", 17000, "{}"),
        ("<div x={/*empty*/} />;", 17000, "{/*empty*/}"),
        ("<div x x x />;", 17001, "x"),
        ("<div x x y={} />;", 17001, "x"),
        ("<div x={} x />;", 17000, "{}"),
        ("<div x {...{}} x />;", 17001, "x"),
        ("<div ns:x ns:x />;", 17001, "ns:x"),
        ("<div ns : x ns : x={true} />;", 17001, "ns : x"),
    ] {
        let (parse, semantic) = check(source);
        assert!(parse.is_empty(), "{source}: {parse:?}");
        assert_eq!(semantic.len(), 1, "{source}: {semantic:?}");
        assert_eq!(semantic[0].code, code);
        let span = semantic[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], text);
        if code == 17001 {
            let first = source.find(text).unwrap();
            let second = first + text.len() + source[first + text.len()..].find(text).unwrap();
            assert_eq!(span.start as usize, second, "{source}");
        }
    }
}

#[test]
fn jsx_attribute_grammar_keeps_nested_and_value_checks() {
    let (parse, semantic) = check("<div x={} other={missing}><span x={} /></div>;");
    assert!(parse.is_empty());
    assert_eq!(semantic.len(), 3, "{semantic:?}");
    assert_eq!(semantic.iter().filter(|d| d.code == 17000).count(), 2);
    assert_eq!(semantic.iter().filter(|d| d.code == 2304).count(), 1);
    let (parse, semantic) = check("<div X x ns:x ns:y>{/*empty*/}</div>;");
    assert!(parse.is_empty(), "{parse:?}");
    assert!(semantic.is_empty(), "{semantic:?}");
}

#[test]
fn parse_errors_suppress_jsx_grammar_but_keep_expression_diagnostics() {
    let (parse, semantic) = check("<div bad= x x empty={} value={a, b} />;");
    assert_eq!(parse.iter().map(|d| d.code).collect::<Vec<_>>(), [1145]);
    let mut codes: Vec<_> = semantic.iter().map(|d| d.code).collect();
    codes.sort_unstable();
    assert_eq!(codes, [2304, 2304, 2695], "{semantic:?}");
}
