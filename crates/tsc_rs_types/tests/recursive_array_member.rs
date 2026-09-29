use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn ts2322_count(source: &str) -> usize {
    let file = tsc_rs_parser::parse("recursive_array_member.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2322)
        .count()
}

#[test]
fn recursive_array_member_normalization_preserves_the_recursive_element_type() {
    let source = r#"
type Where = {
    AND?: Where | Where[];
    OR?: Where[];
};

declare const conditions: Where[];
let where: Where = {};
const existingAnd = Array.isArray(where.AND)
    ? where.AND
    : where.AND
      ? [where.AND]
      : [];

where = { ...where, AND: [...existingAnd, { OR: conditions }] };
where = { ...where, AND: [...existingAnd, 1] };
"#;

    assert_eq!(ts2322_count(source), 1);
}
