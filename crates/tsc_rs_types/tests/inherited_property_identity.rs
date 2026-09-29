use tsc_rs_ast::{CompilerOptions, Diagnostic};

fn check(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("bases.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    tsc_rs_types::check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
}

#[test]
fn conflicting_generic_bases_report_the_name_and_instantiations() {
    let source =
        "interface Base<T> { value: T }\ninterface Both extends Base<string>, Base<number> {}";
    let diagnostics = check(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2320);
    assert_eq!(diagnostics[0].message, "Interface 'Both' cannot simultaneously extend types 'Base<string>' and 'Base<number>'.\n  Named property 'value' of types 'Base<string>' and 'Base<number>' are not identical.");
    let span = diagnostics[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "Both");
}

#[test]
fn own_override_resolves_base_conflict_and_is_checked_against_each_base() {
    let diagnostics = check(
        r#"
interface Left { value: { left: string } }
interface Right { value: { right: number } }
interface Good extends Left, Right { value: { left: string; right: number } }
interface Bad extends Left, Right { value: { left: string; right: boolean } }
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2430);
    assert!(diagnostics[0].message.contains("Interface 'Bad'"));
}

#[test]
fn merged_declarations_compare_all_bases_at_the_first_declaration() {
    let source = "interface Base<T> { value: T }\ninterface Both extends Base<string> {}\ninterface Both extends Base<number> {}";
    let diagnostics = check(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2320);
    assert_eq!(
        diagnostics[0].span.unwrap().start as usize,
        source.find("Both").unwrap()
    );
}

#[test]
fn merged_own_member_can_resolve_a_conflict_from_another_fragment() {
    let diagnostics = check(
        r#"
interface Left { value: { left: string } }
interface Right { value: { right: number } }
interface Both extends Left, Right {}
interface Both { value: { left: string; right: number } }
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn identity_checks_readonly_optionality_and_any() {
    for (left, right) in [
        ("readonly value: string", "value: string"),
        ("value?: string", "value: string"),
        ("value: any", "value: string"),
    ] {
        let diagnostics = check(&format!("interface Left {{ {left} }}\ninterface Right {{ {right} }}\ninterface Both extends Left, Right {{}}"));
        assert_eq!(diagnostics.len(), 1, "{left}, {right}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, 2320);
    }
}

#[test]
fn identity_accepts_structural_aliases_and_reordered_unions() {
    let diagnostics = check(
        r#"
type First = { a: string; b: number };
type Second = { b: number; a: string };
interface Left { object: First; choice: string | number }
interface Right { object: Second; choice: number | string }
interface Both extends Left, Right {}
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn generic_signature_identity_ignores_binder_and_parameter_spelling() {
    let diagnostics = check(
        r#"
interface Left { map<T>(left: T): T; create: new <T>(left: T) => T }
interface Right { map<U>(right: U): U; create: new <U>(right: U) => U }
interface Both extends Left, Right {}
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn separate_private_and_protected_members_have_distinct_origins() {
    for visibility in ["private", "protected"] {
        let diagnostics = check(&format!("declare class Left {{ {visibility} value: string }}\ndeclare class Right {{ {visibility} value: string }}\ninterface Both extends Left, Right {{}}"));
        assert_eq!(diagnostics.len(), 1, "{visibility}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, 2320);
    }
}

#[test]
fn diamond_inheritance_keeps_the_same_private_member_identity() {
    let diagnostics = check(
        r#"
declare class Root { private value: string }
declare class Left extends Root {}
declare class Right extends Root {}
interface Both extends Left, Right {}
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn recursive_structural_identity_terminates() {
    let diagnostics = check(
        r#"
interface NodeA { next: NodeA; value: string }
interface NodeB { next: NodeB; value: string }
interface Left { node: NodeA }
interface Right { node: NodeB }
interface Both extends Left, Right {}
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn merged_class_base_participates_in_interface_identity() {
    let diagnostics = check(
        r#"
declare abstract class Base { abstract value: number }
class Bad extends Base {}
interface Other { value: string }
interface Bad extends Other {}
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2320);
    assert!(diagnostics[0].message.contains("'Base' and 'Other'"));
}

#[test]
fn inherited_mapped_optionality_respects_exact_optional_property_types() {
    let source = r#"
interface Values { port: number }
interface Left extends Partial<Values> {}
interface Right { port?: number | undefined }
interface Both extends Left, Right {}
"#;
    let file = tsc_rs_parser::parse("bases.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    for exact in [true, false] {
        let diagnostics = tsc_rs_types::check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(true),
                exact_optional_property_types: Some(exact),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
        let conflicts: Vec<_> = diagnostics.iter().filter(|d| d.code == 2320).collect();
        assert_eq!(
            conflicts.len(),
            usize::from(exact),
            "exact={exact}: {diagnostics:?}"
        );
    }
}

#[test]
fn unrelated_interface_merge_does_not_satisfy_an_abstract_requirement() {
    let diagnostics = check(
        r#"
declare abstract class Base { abstract value: number }
class Missing extends Base {}
interface Missing { other: string }
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2515);
}

#[test]
fn inherited_user_defined_mapped_type_keeps_optional_members() {
    let diagnostics = check(
        r#"
type OptionalValues<T> = { [P in keyof T]?: T[P] };
interface Values { port: number }
interface Left extends OptionalValues<Values> {}
interface Right { port: number }
interface Both extends Left, Right {}
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2320);
}

#[test]
fn namespace_base_diagnostics_use_declaration_names() {
    let diagnostics = check(
        r#"
namespace One { export interface Base { value: string } }
namespace Two { export interface Base { value: number } }
interface Both extends One.Base, Two.Base {}
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].message, "Interface 'Both' cannot simultaneously extend types 'Base' and 'Base'.\n  Named property 'value' of types 'Base' and 'Base' are not identical.");
}
