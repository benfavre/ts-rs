use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

#[test]
fn abstract_constructor_grammar_is_reported_once_in_ambient_classes() {
    for source in [
        "declare abstract class C { abstract constructor() {} }",
        "declare abstract class C { public abstract constructor() {} }",
    ] {
        let file = tsc_rs_parser::parse("modifiers.ts", source);
        let symbols = tsc_rs_symbols::bind(&file);
        let result =
            TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
        let mut diagnostics = file.diagnostics;
        diagnostics.extend(result.diagnostics);
        let grammar: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1242)
            .map(|diagnostic| {
                let span = diagnostic.span.unwrap();
                &source[span.start as usize..span.end as usize]
            })
            .collect();
        assert_eq!(grammar, ["abstract"], "{source}");
        assert_eq!(diagnostics.iter().filter(|d| d.code == 1183).count(), 1);
    }
}

#[test]
fn ambient_property_initializers_follow_the_selected_decorator_mode() {
    let source = "declare let dec: any; class C { @dec declare value = 1; }";
    for (legacy, expected) in [(false, 1206), (true, 1039)] {
        let file = tsc_rs_parser::parse("modifiers.ts", source);
        assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
        let symbols = tsc_rs_symbols::bind(&file);
        let result = TypeChecker::new().check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                experimental_decorators: Some(legacy),
                ..CompilerOptions::default()
            },
        );
        let grammar: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.code, 1039 | 1206))
            .map(|d| d.code)
            .collect();
        assert_eq!(grammar, [expected], "legacy={legacy}");
    }
}
