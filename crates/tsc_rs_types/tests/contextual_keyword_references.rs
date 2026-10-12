use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

const CONTEXTUAL_NAMES: &[&str] = &[
    "public",
    "private",
    "protected",
    "static",
    "readonly",
    "override",
    "accessor",
    "declare",
    "is",
    "asserts",
    "infer",
    "out",
    "satisfies",
    "async",
    "of",
    "from",
    "as",
    "get",
    "set",
];

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("keywords.ts", source);
    assert!(
        file.diagnostics.is_empty(),
        "{source}: {:?}",
        file.diagnostics
    );
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                always_strict: Some(false),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

#[test]
fn contextual_keywords_are_not_implicitly_declared_globals() {
    for name in CONTEXTUAL_NAMES {
        let source = format!("({name});");
        let errors = diagnostics(&source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        // In an ES2015 library, the ordinary missing-name check suggests
        // the global Set constructor for the unbound identifier `set`.
        let (code, message) = if *name == "set" {
            (
                2552,
                "Cannot find name 'set'. Did you mean 'Set'?".to_string(),
            )
        } else {
            (2304, format!("Cannot find name '{name}'."))
        };
        assert_eq!(errors[0].code, code, "{source}: {errors:?}");
        assert_eq!(errors[0].message, message);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], *name);
    }
}

#[test]
fn declared_contextual_names_use_ordinary_value_lookup() {
    for name in CONTEXTUAL_NAMES {
        let source =
            format!("let {name} = 1; const object = {{ {name} }}; {name}++; object.{name};");
        let errors = diagnostics(&source);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn computed_names_report_missing_values_alongside_strict_mode_errors() {
    let source = "class C { [public]: any; }";
    let errors = diagnostics(source);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [1213, 2304],
        "{errors:?}"
    );
    for error in errors {
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "public");
    }
    let errors = diagnostics("const object = { [readonly]: 1 };");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2304);
    assert!(diagnostics("const object = { readonly: 1, public: 2 }; object.readonly;").is_empty());
}

#[test]
fn contextual_names_with_only_a_type_meaning_are_not_values() {
    let errors = diagnostics("type accessor = {}; accessor;");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2693);
    assert_eq!(
        errors[0].message,
        "'accessor' only refers to a type, but is being used as a value here."
    );
}
