use tsc_rs_parser::parse;
use tsc_rs_symbols::bind;
use tsc_rs_types::TypeChecker;

fn diagnostic_codes(source: &str) -> Vec<u32> {
    let file = parse("overloads.ts", source);
    let symbols = bind(&file);
    TypeChecker::new()
        .check(&file, &symbols)
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn differently_named_method_body_reports_name_mismatch() {
    let codes = diagnostic_codes(
        r#"
class Example {
    value(input: string): string;
    other(input: unknown) { return String(input); }
}
"#,
    );

    assert_eq!(codes, vec![2389]);
}

#[test]
fn literal_and_computed_names_follow_the_same_overload_rule() {
    let string_codes = diagnostic_codes(
        r#"
class Example {
    "value"(): void;
    "other"() {}
}
"#,
    );
    let computed_codes = diagnostic_codes(
        r#"
class Example {
    ["value"](): void;
    ["other"]() {}
}
"#,
    );

    assert_eq!(string_codes, vec![2389]);
    assert_eq!(computed_codes, vec![2389]);
}

#[test]
fn matching_implementation_and_ambient_signatures_are_not_rejected() {
    let implementation_codes = diagnostic_codes(
        r#"
class Example {
    value(input: string): string;
    value(input: unknown) { return String(input); }
}
"#,
    );
    let ambient_codes = diagnostic_codes(
        r#"
declare class Example {
    value(input: string): string;
    other(input: number): number;
}
"#,
    );

    assert!(!implementation_codes.contains(&2389));
    assert!(!implementation_codes.contains(&2391));
    assert!(!ambient_codes.contains(&2389));
    assert!(!ambient_codes.contains(&2391));
}

#[test]
fn non_abstract_signature_in_abstract_class_still_needs_implementation() {
    let codes = diagnostic_codes(
        r#"
abstract class Example {
    value(): void;
}
"#,
    );

    assert!(codes.contains(&2391));
}

#[test]
fn constructor_overload_must_fit_implementation_arity() {
    let incompatible_codes = diagnostic_codes(
        r#"
class Example {
    constructor();
    constructor(value: string) {}
}
"#,
    );
    let compatible_codes = diagnostic_codes(
        r#"
class Example {
    constructor(value: "first");
    constructor(value: "second");
    constructor(value: string) {}
}
"#,
    );

    assert!(incompatible_codes.contains(&2394));
    assert!(!compatible_codes.contains(&2394));
}

#[test]
fn abstract_signature_does_not_claim_following_method_as_implementation() {
    let codes = diagnostic_codes(
        r#"
abstract class Example {
    abstract value(): unknown;
    other() {}
}
"#,
    );

    assert!(!codes.contains(&2389));
    assert!(!codes.contains(&2391));
}

#[test]
fn any_rest_constructor_implementation_accepts_every_overload() {
    let codes = diagnostic_codes(
        r#"
class Example {
    constructor(value: string, context: unknown);
    constructor(value: { id: number });
    constructor(...args: any[]) {}
}
"#,
    );
    let typed_rest_codes = diagnostic_codes(
        r#"
class Example {
    constructor(value: number);
    constructor(...args: string[]) {}
}
"#,
    );

    assert!(!codes.contains(&2394));
    assert!(typed_rest_codes.contains(&2394));
}
