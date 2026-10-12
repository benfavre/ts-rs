use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str, target: ScriptTarget) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("constructors.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                target: Some(target),
                strict: Some(false),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

#[test]
fn constructor_typos_use_library_names_and_declaration_locations() {
    for (source, property, suggestion, owner) in [
        (
            "Object.getOwnPropertyNamess(null);",
            "getOwnPropertyNamess",
            "getOwnPropertyNames",
            "ObjectConstructor",
        ),
        (
            "const Alias = Object; Alias.getOwnPropertyNamess(null);",
            "getOwnPropertyNamess",
            "getOwnPropertyNames",
            "ObjectConstructor",
        ),
        ("Date.prase('2020');", "prase", "parse", "DateConstructor"),
        (
            "Array.isArrray([]);",
            "isArrray",
            "isArray",
            "ArrayConstructor",
        ),
        (
            "const Alias = Date; Alias.prase('2020');",
            "prase",
            "parse",
            "DateConstructor",
        ),
    ] {
        let errors = diagnostics(source, ScriptTarget::ES2015);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, 2551, "{source}: {errors:?}");
        assert_eq!(errors[0].message, format!("Property '{property}' does not exist on type '{owner}'. Did you mean '{suggestion}'?"));
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], property);
        let related = errors[0].related.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].code, 2728);
        assert_eq!(related[0].file_name.as_deref(), Some("lib.es5.d.ts"));
        assert_eq!(
            related[0].message,
            format!("'{suggestion}' is declared here.")
        );
    }
}

#[test]
fn distant_missing_properties_keep_ts2339() {
    let errors = diagnostics("Object.unrelated;", ScriptTarget::ES2015);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2339);
    assert_eq!(
        errors[0].message,
        "Property 'unrelated' does not exist on type 'ObjectConstructor'."
    );
}

#[test]
fn declared_library_and_inherited_function_members_remain_valid() {
    let errors = diagnostics("Object.call(null); Object.prototype; Object.toString(); Object.getOwnPropertySymbols({}); Date.call(null); Array.call(null);", ScriptTarget::ES2015);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn constructor_and_function_augmentations_remain_valid() {
    let errors = diagnostics("interface ObjectConstructor { customMember(): void; } interface Function { customFunction(): void; } Object.customMember(); Object.customFunction();", ScriptTarget::ES2015);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = diagnostics("interface Custom { customMember(): void; } interface ObjectConstructor extends Custom {} Object.customMember();", ScriptTarget::ES2015);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = diagnostics(
        "interface ObjectConstructor { [key: string]: any; } Object.customMember();",
        ScriptTarget::ES2015,
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn shadowed_globals_use_the_local_type() {
    let errors = diagnostics(
        "function use(Object: any) { Object.getOwnPropertyNamess(null); }",
        ScriptTarget::ES2015,
    );
    assert!(errors.is_empty(), "{errors:?}");
    let errors = diagnostics("function use(Object: { customMember: number }) { Object.customMember; Object.getOwnPropertyNamess; }", ScriptTarget::ES2015);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2339);
    assert_eq!(
        errors[0].message,
        "Property 'getOwnPropertyNamess' does not exist on type '{ customMember: number; }'."
    );
}

#[test]
fn unavailable_members_keep_the_newer_library_diagnostic() {
    let errors = diagnostics("Object.values({});", ScriptTarget::ES5);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2550, "{errors:?}");
    assert!(errors[0].message.contains("'es2017'"), "{errors:?}");
}
