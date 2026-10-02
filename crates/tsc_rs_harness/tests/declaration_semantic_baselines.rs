use tsc_rs_harness::{BaselineKind, BaselineRunner, Suite};

fn baseline(source: &str, kind: BaselineKind) -> String {
    let root = tempfile::tempdir().unwrap();
    let cases = root.path().join("tests/cases/compiler");
    std::fs::create_dir_all(&cases).unwrap();
    let path = cases.join("case.ts");
    std::fs::write(&path, source).unwrap();
    BaselineRunner::new(root.path())
        .run_case_with_kind(&path, Suite::Compiler, kind)
        .actual_output
}

#[test]
fn declaration_files_retain_type_annotations() {
    for extension in ["d.ts", "d.mts", "d.cts"] {
        let output = baseline(
            &format!("// @filename: ambient.{extension}\ndeclare const version: number;\n"),
            BaselineKind::Types,
        );
        assert!(
            output.contains(&format!("=== ambient.{extension} ===")),
            "{output}"
        );
        assert!(
            output.contains(">version : number\n>        : ^^^^^^\n"),
            "{output}"
        );
    }
}

#[test]
fn declaration_files_retain_symbol_annotations() {
    for extension in ["d.ts", "d.mts", "d.cts"] {
        let output = baseline(
            &format!("// @filename: ambient.{extension}\ndeclare const version: number;\n"),
            BaselineKind::Symbols,
        );
        assert!(
            output.contains(&format!("=== ambient.{extension} ===")),
            "{output}"
        );
        assert!(
            output.contains(&format!(
                ">version : Symbol(version, Decl(ambient.{extension}, 0, 13))"
            )),
            "{output}"
        );
    }
}

#[test]
fn semantic_baselines_keep_declarations_alongside_implementation_files() {
    let source = "// @filename: ambient.d.ts\ndeclare const version: number;\n// @filename: config.json\n{}\n// @filename: implementation.ts\nconst value = 1;\n";
    for kind in [BaselineKind::Types, BaselineKind::Symbols] {
        let output = baseline(source, kind);
        assert!(output.contains("=== ambient.d.ts ==="), "{output}");
        assert!(output.contains("=== implementation.ts ==="), "{output}");
        assert!(!output.contains("=== config.json ==="), "{output}");
    }
}
