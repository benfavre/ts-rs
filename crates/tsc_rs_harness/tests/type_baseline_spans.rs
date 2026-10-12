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

#[test]
fn destructuring_annotations_use_binding_names() {
    let output = baseline("declare const input: any; const { value } = input; const [item] = [1];");
    assert!(output.contains(">value : any\n"), "{output}");
    assert!(output.contains(">item : number\n"), "{output}");
    assert!(
        !output.contains(">{ :") && !output.contains(">[ :"),
        "{output}"
    );
}

#[test]
fn class_members_and_assignment_targets_match_upstream_types_baselines() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let runner = BaselineRunner::new(&root);
    for (suite, relative) in [
        (
            Suite::Conformance,
            "conformance/classes/classDeclarations/classBody/classBodyWithStatements.ts",
        ),
        (
            Suite::Conformance,
            "conformance/parser/ecmascript5/RegressionTests/parser585151.ts",
        ),
        (Suite::Compiler, "compiler/ClassDeclaration21.ts"),
        (Suite::Compiler, "compiler/MemberAccessorDeclaration15.ts"),
        (Suite::Compiler, "compiler/optionalSetterParam.ts"),
        (Suite::Compiler, "compiler/newLineFlagWithLF.ts"),
        (
            Suite::Compiler,
            "compiler/sourceMapValidationDestructuringVariableStatementArrayBindingPattern4.ts",
        ),
        (
            Suite::Conformance,
            "conformance/parser/ecmascript6/ComputedPropertyNames/parserComputedPropertyName12.ts",
        ),
    ] {
        let case = root.join("tests/cases").join(relative);
        assert!(case.is_file(), "{}", case.display());
        let result = runner.run_case_with_kind(&case, suite, BaselineKind::Types);
        assert!(
            result.baseline_exists && result.passed,
            "{relative}: {:?}",
            result.diff
        );
    }
}

#[test]
fn keyword_field_names_remain_visible_when_they_are_not_recovery_tokens() {
    let output = baseline("class C { var; function; } class D { foo bar; }");
    for name in ["var", "function", "foo", "bar"] {
        assert!(output.contains(&format!(">{name} : any\n")), "{output}");
    }
}
