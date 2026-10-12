use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

#[test]
fn empty_for_of_checks_the_body_without_an_iterated_binding() {
    for (keyword, expected_names) in [("of", vec!["missing"]), ("in", vec!["values", "missing"])] {
        let source = format!("for (var {keyword} values) {{ missing; }}");
        let file = tsc_rs_parser::parse("loop.ts", &source);
        assert_eq!(file.diagnostics.len(), 1);
        assert_eq!(file.diagnostics[0].code, 1123);
        let symbols = tsc_rs_symbols::bind(&file);
        let checked =
            TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
        let mut names = Vec::new();
        for diagnostic in &checked.diagnostics {
            assert_eq!(diagnostic.code, 2304, "{diagnostic:?}");
            let span = diagnostic.span.unwrap();
            names.push(&source[span.start as usize..span.end as usize]);
        }
        assert_eq!(names, expected_names);
    }
}
