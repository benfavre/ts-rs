use tsc_rs_ast::Diagnostic;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    tsc_rs_parser::parse("ambient.ts", source).diagnostics
}

#[test]
fn ambient_variables_and_annotated_constants_cannot_have_initializers() {
    for source in [
        "declare var x = 4;",
        "declare let x = 4;",
        "declare const x: number = 4;",
        "declare namespace N { var x = 4; }",
        "declare namespace N { const x: number = 4; }",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1039);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "4");
    }
}

#[test]
fn ambient_constants_allow_only_the_permitted_literal_forms() {
    for value in ["4", "-4", "4n", "-4n", "'text'", "`text`", "true", "false"] {
        let source = format!("declare const x = {value};");
        assert!(diagnostics(&source).is_empty(), "{source}");
    }
    for value in ["+4", "~4", "4 + 1", "(4)", "null", "undefined", "{}", "f()"] {
        for source in [
            format!("declare const x = {value};"),
            format!("declare namespace N {{ const x = {value}; }}"),
        ] {
            let errors = diagnostics(&source);
            assert_eq!(errors.len(), 1, "{source}: {errors:?}");
            assert_eq!(errors[0].code, 1254);
            let span = errors[0].span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], value);
        }
    }
}

#[test]
fn ambient_initializer_grammar_errors_yield_to_syntax_errors() {
    for source in [
        "declare var x = 4; const broken = ;",
        "declare const x = f(); const broken = ;",
    ] {
        let errors = diagnostics(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1109);
    }
    assert!(diagnostics("var x = 4; let y = 4; const z = f();").is_empty());
}
