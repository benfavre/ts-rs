use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<(u32, String, String)> {
    let file = tsc_rs_parser::parse("declaration-recovery.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    // These recovery cases exercise keyword spellings in non-strict code.
    let checked = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    let mut result: Vec<_> = file
        .diagnostics
        .into_iter()
        .chain(checked.diagnostics)
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                source[span.start as usize..span.end as usize].to_string(),
                d.message,
            )
        })
        .collect();
    result.sort();
    result
}

#[test]
fn invalid_interface_name_keeps_expression_recovery_diagnostics() {
    assert_eq!(
        diagnostics("interface void {}"),
        vec![
            (
                2304,
                "interface".into(),
                "Cannot find name 'interface'.".into()
            ),
            (
                2427,
                "void".into(),
                "Interface name cannot be 'void'.".into()
            ),
        ]
    );
}

#[test]
fn missing_interface_name_points_at_the_opening_brace() {
    assert_eq!(
        diagnostics("interface {}"),
        vec![
            (1438, "{".into(), "Interface must be given a name.".into()),
            (
                2304,
                "interface".into(),
                "Cannot find name 'interface'.".into()
            ),
        ]
    );
}

#[test]
fn invalid_alias_name_preserves_following_value_reference() {
    assert_eq!(
        diagnostics("interface I {} type void = I;"),
        vec![
            (1109, "=".into(), "Expression expected.".into()),
            (2304, "type".into(), "Cannot find name 'type'.".into()),
            (
                2457,
                "void".into(),
                "Type alias name cannot be 'void'.".into()
            ),
            (
                2693,
                "I".into(),
                "'I' only refers to a type, but is being used as a value here.".into()
            ),
        ]
    );
}

#[test]
fn keyword_spellings_obey_value_and_type_bindings() {
    assert!(diagnostics("var type = 1; var interface = 2; type; interface;").is_empty());
    assert_eq!(
        diagnostics("interface interface {} interface;"),
        vec![(
            2693,
            "interface".into(),
            "'interface' only refers to a type, but is being used as a value here.".into()
        ),]
    );
}

#[test]
fn semicolons_and_line_breaks_end_keyword_expression_statements() {
    for source in [
        "interface\nvoid {}",
        "interface; void {}",
        "type\nvoid 0",
        "type; void 0",
    ] {
        let got = diagnostics(source);
        assert_eq!(got.len(), 1, "{source}: {got:?}");
        assert_eq!(got[0].0, 2304, "{source}: {got:?}");
    }
}

#[test]
fn namespace_recovery_reports_the_unbound_keyword() {
    assert_eq!(
        diagnostics("namespace {}"),
        vec![
            (1437, "{".into(), "Namespace must be given a name.".into()),
            (
                2304,
                "namespace".into(),
                "Cannot find name 'namespace'.".into()
            ),
        ]
    );
    assert!(diagnostics("var namespace = 1; namespace;").is_empty());
}
