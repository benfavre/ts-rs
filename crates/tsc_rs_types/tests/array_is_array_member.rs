use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

#[test]
fn optional_array_member_is_narrowed_without_hiding_unguarded_access() {
    let source = r#"
declare const overrides: { recipients?: string[] };
const effective =
    Array.isArray(overrides.recipients) && overrides.recipients.length > 0
        ? overrides.recipients.filter((value): value is string => typeof value === "string")
        : [];

const invalid = overrides.recipients.length;
"#;
    let file = tsc_rs_parser::parse("array_is_array_member.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 18048)
            .count(),
        1,
        "diagnostics: {diagnostics:?}"
    );
}
