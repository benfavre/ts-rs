#[test]
fn function_type_parameter_modifier_does_not_cascade_parser_errors() {
    let parsed = tsc_rs_parser::parse("parameter.ts", "function A(): (public B) => C {\n}\n");

    assert!(
        parsed.diagnostics.is_empty(),
        "diagnostics: {:?}",
        parsed.diagnostics
    );
}
