use tsc_rs_harness::{BaselineKind, BaselineRunner, Suite};

fn baseline(source: &str) -> String {
    let root = tempfile::tempdir().unwrap();
    let cases = root.path().join("tests/cases/compiler");
    std::fs::create_dir_all(&cases).unwrap();
    let path = cases.join("case.ts");
    std::fs::write(&path, source).unwrap();
    BaselineRunner::new(root.path())
        .run_case_with_kind(&path, Suite::Compiler, BaselineKind::Types)
        .actual_output
}

#[test]
fn exact_expression_labels_keep_preorder_and_property_tokens() {
    let output = baseline("const item = { value: 1 }; item.value + 2;");
    let labels: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with('>') && !line.starts_with("> "))
        .collect();
    assert!(labels.contains(&">item.value + 2 : number"), "{output}");
    let whole = labels
        .iter()
        .position(|line| line.starts_with(">item.value + 2 :"))
        .unwrap();
    let member = labels
        .iter()
        .position(|line| line.starts_with(">item.value :"))
        .unwrap();
    let receiver = labels
        .iter()
        .rposition(|line| line.starts_with(">item :"))
        .unwrap();
    let property = labels
        .iter()
        .rposition(|line| line.starts_with(">value :"))
        .unwrap();
    assert!(
        whole < member && member < receiver && receiver < property,
        "{output}"
    );
}

#[test]
fn multiline_expression_labels_remove_line_breaks_only() {
    let output = baseline("const value = (\r\n  1 +\r\n  2\r\n);");
    assert!(output.contains(">(  1 +  2) : number\n"), "{output}");
    assert!(output.contains(">1 +  2 : number\n"), "{output}");
}

#[test]
fn recovered_bindings_do_not_invent_source_identifiers() {
    for source in [
        "const",
        "var",
        "for (var of values) {}",
        "for (const of values) {}",
    ] {
        let output = baseline(source);
        assert!(
            !output.contains(">of :") && !output.contains("> : any"),
            "{output}"
        );
    }
}

#[test]
fn null_literals_and_null_named_properties_are_distinct() {
    let output = baseline("null.foo; const obj = { null: 1 }; obj.null;");
    assert!(output.contains(">null.foo : any"), "{output}");
    assert!(!output.contains(">null : null"), "{output}");
    assert!(output.contains(">null : number"), "{output}");
}

#[test]
fn missing_unary_operands_have_empty_source_labels() {
    let output = baseline("void ;");
    assert!(output.contains(">void : undefined\n"), "{output}");
    assert!(output.contains("> : any\n"), "{output}");
    assert!(!output.contains(">;"), "{output}");
}
