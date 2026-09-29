use tsc_rs_ast::{CompilerOptions, Diagnostic};

fn check(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("interfaces.ts", source);
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
fn incompatible_override_is_reported_at_the_interface_name() {
    let source =
        "interface Base { value: number }\ninterface Derived extends Base { value: string }";
    let diagnostics = check(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.code, 2430);
    assert_eq!(diagnostic.message, "Interface 'Derived' incorrectly extends interface 'Base'.\n  Types of property 'value' are incompatible.\n    Type 'string' is not assignable to type 'number'.");
    let span = diagnostic.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "Derived");
}

#[test]
fn nested_override_reports_the_property_path() {
    let diagnostics = check("interface Base { value: { nested: number } }\ninterface Derived extends Base { value: { nested: string } }");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].message, "Interface 'Derived' incorrectly extends interface 'Base'.\n  The types of 'value.nested' are incompatible between these types.\n    Type 'string' is not assignable to type 'number'.");
}

#[test]
fn generic_base_arguments_are_substituted() {
    let diagnostics = check("interface Box<T> { value: T }\ninterface Good extends Box<number> { value: 1 }\ninterface Bad extends Box<number> { value: string }");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2430);
    assert!(diagnostics[0]
        .message
        .contains("'Bad' incorrectly extends interface 'Box<number>'"));
    assert!(diagnostics[0]
        .message
        .ends_with("Type 'string' is not assignable to type 'number'."));
}

#[test]
fn inherited_index_signatures_must_be_compatible() {
    let diagnostics = check("interface Numbers { [key: string]: number }\ninterface Strings { [key: string]: string }\ninterface Combined extends Numbers, Strings {}");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].message, "Interface 'Combined' incorrectly extends interface 'Strings'.\n  'string' index signatures are incompatible.\n    Type 'number' is not assignable to type 'string'.");
}

#[test]
fn private_class_member_cannot_be_redeclared_publicly() {
    let diagnostics = check("declare class Base { private value: string }\ninterface Derived extends Base { value: string }");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].message, "Interface 'Derived' incorrectly extends interface 'Base'.\n  Property 'value' is private in type 'Base' but not in type 'Derived'.");
}

#[test]
fn compatible_covariant_and_recursive_overrides_are_accepted() {
    let diagnostics = check(
        r#"
interface Base { value: string | number; next?: Base }
interface Derived extends Base { value: string; next?: Derived }
interface Generic<T> { value: T }
interface GenericDerived<T> extends Generic<T> { value: T; extra: number }
interface Optional { value?: number }
interface Required extends Optional { value: number }
interface Left { left: string }
interface Right { right: number }
interface Both extends Left, Right { extra: boolean }
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn optional_override_cannot_weaken_a_required_property() {
    let diagnostics = check(
        "interface Base { value: number }\ninterface Derived extends Base { value?: number }",
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2430);
}

#[test]
fn function_property_and_method_return_types_are_checked() {
    let diagnostics = check("interface Base { method(): number; property: () => number }\ninterface BadMethod extends Base { method(): string }\ninterface BadProperty extends Base { property: () => string }");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| diagnostic.code == 2430));
}

#[test]
fn methods_are_bivariant_but_function_properties_are_contravariant() {
    let diagnostics = check(
        r#"
interface Animal { name: string }
interface Dog extends Animal { bark(): void }
interface Base { method(value: Animal): void; property: (value: Animal) => void }
interface MethodOverride extends Base { method(value: Dog): void }
interface PropertyOverride extends Base { property: (value: Dog) => void }
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2430);
    assert!(diagnostics[0]
        .message
        .contains("Interface 'PropertyOverride'"));
}

#[test]
fn generic_constructors_can_infer_different_type_parameter_lists() {
    let diagnostics = check(
        r#"
interface Base {
    construct: new <T>(value: { first: T; second: T }) => T[];
    discard: new <T>(value: T) => void;
}
interface Derived extends Base {
    construct: new <U, V>(value: { first: U; second: V }) => U[];
    discard: new <U>(value: U) => U;
}
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn rest_signature_allows_more_required_parameters() {
    let diagnostics = check("interface Base { apply: (...values: number[]) => number }\ninterface Derived extends Base { apply: (left: number, right: number) => number }");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn protected_members_keep_class_ownership_through_class_inheritance() {
    let diagnostics = check("declare class Root { protected value: string }\ndeclare class Base extends Root {}\ninterface Derived extends Base { value: string }");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].message, "Interface 'Derived' incorrectly extends interface 'Base'.\n  Property 'value' is protected but type 'Derived' is not a class derived from 'Root'.");
}

#[test]
fn member_diagnostic_uses_base_declaration_spelling() {
    let diagnostics =
        check("interface Base { 2.0: number }\ninterface Derived extends Base { 2: string }");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("Types of property '2.0'"));
}

#[test]
fn namespace_bases_resolve_lexically() {
    let diagnostics = check(
        r#"
interface Base { value: string }
namespace Inner {
    interface Base { value: number }
    interface Good extends Base { value: 1 }
    interface Bad extends Base { value: string }
}
interface Good extends Base { value: "ok" }
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].message, "Interface 'Bad' incorrectly extends interface 'Base'.\n  Types of property 'value' are incompatible.\n    Type 'string' is not assignable to type 'number'.");
}

#[test]
fn generic_signature_inference_uses_the_contextual_return_type() {
    let diagnostics = check(
        r#"
interface Base {
    call: (value: number) => string[];
    construct: new (value: number) => string[];
}
interface Derived extends Base {
    call: <T, U>(value: T) => U[];
    construct: new <T, U>(value: T) => U[];
}
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn branching_recursive_heritage_is_bounded_and_checks_leaf_types() {
    let diagnostics = check(
        r#"
interface Base { left: Base; right: Base; value: string | number }
interface Good extends Base { left: Good; right: Good; value: string }
interface Bad extends Base { left: Bad; right: Bad; value: boolean }
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2430);
    assert!(diagnostics[0].message.contains("Interface 'Bad'"));
}

#[test]
fn generic_callback_inference_uses_the_last_contextual_overload() {
    let diagnostics = check(
        r#"
interface Base {
    invoke: (callback: { (value: number): number; (value: string): string }) => string[];
}
interface Derived extends Base {
    invoke: <T>(callback: (value: T) => T) => T[];
}
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn recursive_generic_methods_preserve_their_own_binders() {
    let diagnostics = check(
        r#"
interface Base<T> {
    value: T;
    map<M>(callback: (value: T) => M): Base<M>;
    concat<C>(value: C): Base<T | C>;
}
interface Derived<T> extends Base<T> {
    map<M>(callback: (value: T) => M): Derived<M>;
    concat<C>(value: C): Derived<T | C>;
}
interface Bad<T> extends Base<T> {
    map<M>(callback: (value: T) => M): { value: boolean };
}
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2430);
    assert!(diagnostics[0].message.contains("Interface 'Bad<T>'"));
}

#[test]
fn expanding_recursive_type_arguments_terminate() {
    let diagnostics = check(
        r#"
interface A<T> { x: A<B<T>> }
interface B<T> extends A<T> { x: B<A<T>> }
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn namespace_generic_base_methods_receive_outer_type_arguments() {
    let diagnostics = check(
        r#"
namespace Library {
    export interface Base<K, V> {
        map<M>(callback: (value: V, key: K) => M): Base<K, M>;
    }
    export interface Derived<T> extends Base<number, T> {
        map<M>(callback: (value: T, key: number) => M): Derived<M>;
    }
    export interface Invalid extends Base<number, string> {
        map<M>(callback: (value: boolean, key: number) => M): Derived<M>;
    }
}
"#,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2430);
    assert!(diagnostics[0].message.contains("Interface 'Invalid'"));
}
