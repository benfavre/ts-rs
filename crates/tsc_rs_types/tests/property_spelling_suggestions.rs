use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("properties.ts", source);
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

fn assert_suggestion(source: &str, misspelled: &str, suggested: &str, shown_type: &str) {
    let errors = diagnostics(source);
    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
    let error = &errors[0];
    assert_eq!(error.code, 2551, "{source}: {error:?}");
    assert_eq!(error.message, format!("Property '{misspelled}' does not exist on type '{shown_type}'. Did you mean '{suggested}'?"));
    let span = error.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], misspelled);
    let related = error
        .related
        .as_ref()
        .expect("suggested declaration location");
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].code, 2728);
    assert_eq!(
        related[0].message,
        format!("'{suggested}' is declared here.")
    );
    assert_eq!(related[0].file_name.as_deref(), Some("properties.ts"));
    let span = related[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], suggested);
}

#[test]
fn object_literals_and_type_literals_keep_the_suggested_declaration() {
    assert_suggestion(
        "const item = { currentValue: 1 }; item.curentValue;",
        "curentValue",
        "currentValue",
        "{ currentValue: number; }",
    );
    assert_suggestion(
        "declare const item: { currentValue: number }; item.curentValue;",
        "curentValue",
        "currentValue",
        "{ currentValue: number; }",
    );
}

#[test]
fn named_interfaces_and_aliases_suggest_properties_and_methods() {
    assert_suggestion(
        "interface Point { coordinateX: number; } declare const point: Point; point.coordinatX;",
        "coordinatX",
        "coordinateX",
        "Point",
    );
    assert_suggestion(
        "type Point = { computeValue(): void }; declare const point: Point; point.computeVale();",
        "computeVale",
        "computeValue",
        "Point",
    );
}

#[test]
fn class_static_and_instance_members_use_the_correct_side() {
    assert_suggestion(
        "class C { currentValue = 1; } new C().curentValue;",
        "curentValue",
        "currentValue",
        "C",
    );
    assert_suggestion(
        "class C { static currentValue = 1; } C.curentValue;",
        "curentValue",
        "currentValue",
        "typeof C",
    );
    assert_suggestion(
        "class C { currentValue = 1; m() { this.curentValue; } }",
        "curentValue",
        "currentValue",
        "C",
    );
}

#[test]
fn inherited_members_keep_the_base_declaration_location() {
    assert_suggestion(
        "class Base { currentValue = 1; } class Derived extends Base {} new Derived().curentValue;",
        "curentValue",
        "currentValue",
        "Derived",
    );
    assert_suggestion("interface Base { currentValue: number; } interface Derived extends Base {} declare const item: Derived; item.curentValue;", "curentValue", "currentValue", "Derived");
}

#[test]
fn enum_objects_suggest_the_closest_member_and_show_typeof() {
    assert_suggestion(
        "enum U8 { BIT_0 = 1, BIT_1 = 2, BIT_2 = 4 } U8.bit_2;",
        "bit_2",
        "BIT_2",
        "typeof U8",
    );
}

#[test]
fn inaccessible_members_are_not_spelling_candidates() {
    let alias_errors = diagnostics("class C { private secretValue = 1; } type Alias = C; declare const object: Alias; object.secretVale;");
    assert_eq!(
        alias_errors
            .iter()
            .map(|error| error.code)
            .collect::<Vec<_>>(),
        [2339],
        "{alias_errors:?}"
    );
    let errors = diagnostics("class C { private secretValue = 1; protected sharedValue = 2; } new C().secretVale; new C().sharedVale;");
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2339, 2339],
        "{errors:?}"
    );
    assert_suggestion(
        "class C { private secretValue = 1; m() { this.secretVale; } }",
        "secretVale",
        "secretValue",
        "C",
    );
    assert_suggestion("class Base { protected sharedValue = 1; } class Derived extends Base { m() { this.sharedVale; } }", "sharedVale", "sharedValue", "Derived");
}

#[test]
fn library_suggestions_point_to_library_declarations() {
    let errors = diagnostics("'abc'.toUperCase();");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2551, "{errors:?}");
    assert_eq!(
        errors[0].message,
        "Property 'toUperCase' does not exist on type '\"abc\"'. Did you mean 'toUpperCase'?"
    );
    let related = errors[0].related.as_ref().unwrap();
    assert_eq!(related[0].file_name.as_deref(), Some("lib.es5.d.ts"));
    assert_eq!(related[0].message, "'toUpperCase' is declared here.");
}

#[test]
fn valid_access_and_distant_names_keep_their_existing_behavior() {
    assert!(diagnostics("const item = { currentValue: 1 }; item.currentValue;").is_empty());
    let errors = diagnostics("const item = { currentValue: 1 }; item.unrelated;");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2339);
    assert!(errors[0].related.is_none());
}
