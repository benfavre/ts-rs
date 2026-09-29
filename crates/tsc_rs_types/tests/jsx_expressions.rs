use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

#[test]
fn jsx_comma_grammar_distinguishes_containers_parentheses_and_spread_attributes() {
    for (jsx, comma_error, unused_error) in [
        ("<div value={a, b} />", true, true),
        ("<div>{a, b}</div>", true, true),
        ("<div value={(a, b)} />", false, true),
        ("<div>{(a, b)}</div>", false, true),
        ("<div {...a, b} />", false, true),
        ("<div value={f(a, b)} />", false, false),
        ("<div>{f(a, b)}</div>", false, false),
    ] {
        let source = format!(
            "const a = {{}}; const b = {{}}; function f(x: any, y: any) {{ return y; }} {jsx};"
        );
        let file = tsc_rs_parser::parse("expressions.tsx", &source);
        assert!(
            file.diagnostics.is_empty(),
            "{source}: {:?}",
            file.diagnostics
        );
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
            usize::from(comma_error) + usize::from(unused_error),
            "{jsx}: {:?}",
            output.diagnostics
        );
        for diagnostic in &output.diagnostics {
            let span = diagnostic.span.unwrap();
            let text = &source[span.start as usize..span.end as usize];
            match diagnostic.code {
                18007 => {
                    assert!(comma_error);
                    assert_eq!(text, "a, b");
                }
                2695 => {
                    assert!(unused_error);
                    assert_eq!(text, "a");
                }
                _ => panic!("unexpected diagnostic: {diagnostic:?}"),
            }
        }
    }
}
