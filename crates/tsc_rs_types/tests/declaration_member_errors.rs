use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str, options: &CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("members.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, options)
        .diagnostics
}

#[test]
fn enum_non_literal_computed_names_are_rejected_without_name_lookup() {
    let source =
        "const name = 'a'; enum E { [name] = 1, [missing] = 2, [('b')] = 3, [`x${1}`] = 4 }";
    let errors = diagnostics(source, &CompilerOptions::default());
    assert_eq!(errors.len(), 4, "{errors:?}");
    let spans: Vec<_> = errors
        .iter()
        .map(|d| {
            assert_eq!(d.code, 1164);
            let span = d.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(spans, vec!["[name]", "[missing]", "[('b')]", "[`x${1}`]"]);
}

#[test]
fn enum_string_and_template_literal_names_remain_valid() {
    let source = "enum E { ['a'] = 1, [`b`] = 2, C = 3 }";
    let errors = diagnostics(source, &CompilerOptions::default());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn enum_name_errors_survive_parser_recovery_but_respect_no_check() {
    let source = "enum E { [missing] = 1; B = 2 }";
    let errors = diagnostics(source, &CompilerOptions::default());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 1164);
    let options = CompilerOptions {
        no_check: Some(true),
        ..Default::default()
    };
    assert!(diagnostics(source, &options).is_empty());
}

#[test]
fn declared_class_methods_cannot_have_an_implementation() {
    for source in [
        "class C { declare method() {} }",
        "class C { declare static method() {} }",
    ] {
        let errors = diagnostics(source, &CompilerOptions::default());
        let bodies: Vec<_> = errors.iter().filter(|d| d.code == 1183).collect();
        assert_eq!(bodies.len(), 1, "{source}: {errors:?}");
        let span = bodies[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "{");
    }
    for source in [
        "class C { method() {} }",
        "declare class C { method(): void; }",
        "class C { declare method() {} } const broken = ;",
    ] {
        let errors = diagnostics(source, &CompilerOptions::default());
        assert!(
            !errors.iter().any(|d| d.code == 1183),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn invalid_declared_constructors_and_accessors_stop_at_the_modifier_error() {
    for source in [
        "class C { declare constructor() {} }",
        "class C { declare get value() { return 1; } }",
        "class C { declare set value(v: number) {} }",
    ] {
        let file = tsc_rs_parser::parse("members.ts", source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        assert_eq!(file.diagnostics[0].code, 1031);
        let errors = diagnostics(source, &CompilerOptions::default());
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn enum_numeric_names_include_numeric_strings_and_computed_literals() {
    for name in [
        "1", "0x10", "1_000", "1n", "'2'", "'-3'", "'1e+21'", "'1e-7'", "[4]", "['5']", "[`6`]",
    ] {
        let source = format!("enum E {{ {name} = 0 }}");
        let errors = diagnostics(&source, &CompilerOptions::default());
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2452);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], name);
    }
}

#[test]
fn enum_numeric_looking_names_must_have_the_canonical_number_spelling() {
    for name in [
        "'01'",
        "'-0'",
        "'1.0'",
        "'1e2'",
        "'1e21'",
        "'0x10'",
        "'NaN'",
        "'Infinity'",
        "'-Infinity'",
        "['01']",
        "1e999",
    ] {
        let source = format!("enum E {{ {name} = 0 }}");
        let errors = diagnostics(&source, &CompilerOptions::default());
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn catch_annotation_errors_use_the_first_token_and_recover_as_any() {
    for (annotation, first_token) in [
        ("number", "number"),
        ("string[]", "string"),
        ("{ message: string }", "{"),
        ("never", "never"),
    ] {
        let source = format!("try {{}} catch (e: {annotation}) {{ e.anyProperty(); }}");
        let errors = diagnostics(&source, &CompilerOptions::default());
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 1196);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], first_token);
    }
}

#[test]
fn catch_annotations_resolve_aliases_and_preserve_grammar_suppression() {
    for annotation in [
        "any",
        "unknown",
        "(any)",
        "number | any",
        "number | unknown",
        "A",
        "U",
        "number | U",
        "number & A",
        "U & unknown",
    ] {
        let source =
            format!("type A = any; type U = unknown; try {{}} catch (e: {annotation}) {{}}");
        let errors = diagnostics(&source, &CompilerOptions::default());
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
    let errors = diagnostics(
        "type N = number; try {} catch (e: N) {}",
        &CompilerOptions::default(),
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 1196);
    let errors = diagnostics("try {} catch (e: Missing) {}", &CompilerOptions::default());
    assert!(!errors.iter().any(|error| error.code == 1196), "{errors:?}");
    let errors = diagnostics(
        "try {} catch (e: number) {} const broken = ;",
        &CompilerOptions::default(),
    );
    assert!(!errors.iter().any(|error| error.code == 1196), "{errors:?}");
    let options = CompilerOptions {
        no_check: Some(true),
        ..Default::default()
    };
    assert!(diagnostics("try {} catch (e: number) {}", &options).is_empty());
}
