use tsc_rs_ast::{CompilerOptions, ScriptTarget};

fn emit(source: &str) -> String {
    let file = tsc_rs_parser::parse("declarations.ts", source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    tsc_rs_emitter::emit(&file, &options).javascript
}

#[test]
fn trailing_declaration_commas_are_not_copied_to_javascript() {
    for source in ["var a,;", "var a,", "var a /* comment */,;"] {
        let output = emit(source);
        assert!(!output.contains(','), "{source}: {output}");
        assert!(output.contains("var a"), "{source}: {output}");
    }
    assert_eq!(emit("var a,\nreturn;"), "var a;\nreturn;\n");
    assert_eq!(emit("var a /* , */;"), "var a /* , */;\n");
}

#[test]
fn empty_loop_declaration_lists_retain_keyword_spacing() {
    for keyword in ["of", "in"] {
        let output = emit(&format!("for (var {keyword} values) {{}}"));
        assert!(
            output.contains(&format!("for (var  {keyword} values)")),
            "{output}"
        );
    }
}
