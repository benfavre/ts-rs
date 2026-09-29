use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn ts2714_count(source: &str) -> usize {
    let file = tsc_rs_parser::parse("ambient_export.d.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2714)
        .count()
}

#[test]
fn ambient_export_assignments_require_entity_names() {
    assert_eq!(ts2714_count("export default 2 + 2;"), 1);
    assert_eq!(ts2714_count("export = 2 + 2;"), 1);
    assert_eq!(
        ts2714_count(
            "declare namespace Foo { const value: number; }\nexport default typeof Foo.value;"
        ),
        1
    );
    assert_eq!(
        ts2714_count("declare namespace Foo { const value: number; }\nexport = typeof Foo;"),
        1
    );
}

#[test]
fn ambient_export_assignments_accept_identifiers_and_qualified_names() {
    assert_eq!(ts2714_count("declare const Foo: number;\nexport = Foo;"), 0);
    assert_eq!(
        ts2714_count("declare namespace Foo { const value: number; }\nexport default Foo.value;"),
        0
    );
}
