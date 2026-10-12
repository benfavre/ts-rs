// Exact types and diagnostics checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify_options(
    source: &str,
    target: ScriptTarget,
    unchecked: bool,
    strict: bool,
    expected_type: &str,
    expected: &[(u32, &str, u32, u32)],
) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let output = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(target),
            strict: Some(strict),
            no_unchecked_indexed_access: Some(unchecked),
            ..CompilerOptions::default()
        },
    );
    let mut actual = output
        .diagnostics
        .iter()
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                d.message.as_str(),
                span.start,
                span.end - span.start,
            )
        })
        .collect::<Vec<_>>();
    actual.sort_by_key(|d| (d.2, d.0));
    assert_eq!(actual, expected, "{source}");
    let position = source.rfind("result;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&position).map(String::as_str),
        Some(expected_type),
        "{source}: {:?}",
        output.expression_types
    );
}

#[test]
fn number_union_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?1:2] satisfies (1|2)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"(1 | 2)[]"###,
        &[],
    );
}

#[test]
fn string_union_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?'a':'b'] satisfies ('a'|'b')[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"("a" | "b")[]"###,
        &[],
    );
}

#[test]
fn bigint_union_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?1n:2n] satisfies (1n|2n)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"(1n | 2n)[]"###,
        &[],
    );
}

#[test]
fn boolean_union_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?true:false] satisfies boolean[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"boolean[]"###,
        &[],
    );
}

#[test]
fn mixed_union_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?1:'a'] satisfies (1|string)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"(1 | "a")[]"###,
        &[],
    );
}

#[test]
fn different_literal_kind() {
    verify_options(
        r###"const result=['a'] satisfies (1|string)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"string[]"###,
        &[],
    );
}

#[test]
fn number_in_string_enum_context() {
    verify_options(
        r###"enum E{A='a'}const result=[1] satisfies (E|number)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn string_in_number_enum_context() {
    verify_options(
        r###"enum E{A=1}const result=['a'] satisfies (E|string)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"string[]"###,
        &[],
    );
}

#[test]
fn enum_member_context() {
    verify_options(
        r###"enum E{A='a'}const result=[1] satisfies (E.A|number)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn same_enum_kind_context() {
    verify_options(
        r###"enum E{A=1}const result=[2] satisfies (E|string)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"2[]"###,
        &[(
            2322,
            r###"Type '2' is not assignable to type 'string | E'."###,
            25,
            1,
        )],
    );
}

#[test]
fn generic_alias_context() {
    verify_options(
        r###"type Keys<T>=keyof T;const result=['a'] satisfies Keys<{a:1;b:2}>[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###""a"[]"###,
        &[],
    );
}

#[test]
fn generic_alias_union_context() {
    verify_options(
        r###"type Choice<T>=T|'fixed';declare const flag:boolean;const result=[flag?'fixed':'other'] satisfies Choice<'other'>[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"("fixed" | "other")[]"###,
        &[],
    );
}

#[test]
fn keyof_parameter_context() {
    verify_options(
        r###"function f<T>(){const result=['a'] satisfies (keyof T)[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###""a"[]"###,
        &[(
            2322,
            r###"Type 'string' is not assignable to type 'keyof T'."###,
            30,
            3,
        )],
    );
}

#[test]
fn string_constraint() {
    verify_options(
        r###"function f<T extends string>(){const result=['a'] satisfies T[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###""a"[]"###,
        &[(
            2322,
            r###"Type 'string' is not assignable to type 'T'.
  'string' is assignable to the constraint of type 'T', but 'T' could be instantiated with a different subtype of constraint 'string'."###,
            45,
            3,
        )],
    );
}

#[test]
fn number_constraint() {
    verify_options(
        r###"function f<T extends number>(){const result=[1] satisfies T[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"1[]"###,
        &[(
            2322,
            r###"Type 'number' is not assignable to type 'T'.
  'number' is assignable to the constraint of type 'T', but 'T' could be instantiated with a different subtype of constraint 'number'."###,
            45,
            1,
        )],
    );
}

#[test]
fn boolean_constraint() {
    verify_options(
        r###"function f<T extends boolean>(){const result=[true] satisfies T[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"true[]"###,
        &[(
            2322,
            r###"Type 'boolean' is not assignable to type 'T'.
  'boolean' is assignable to the constraint of type 'T', but 'T' could be instantiated with a different subtype of constraint 'boolean'."###,
            46,
            4,
        )],
    );
}

#[test]
fn constraint_chain() {
    verify_options(
        r###"function f<A extends string,B extends A,C extends B,D extends C,E extends D,F extends E>(){const result=['a'] satisfies F[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###""a"[]"###,
        &[(
            2322,
            r###"Type 'string' is not assignable to type 'F'.
  'string' is assignable to the constraint of type 'F', but 'F' could be instantiated with a different subtype of constraint 'string'."###,
            105,
            3,
        )],
    );
}

#[test]
fn unique_symbol_context() {
    verify_options(
        r###"declare const key:unique symbol;const result=[key] satisfies (typeof key)[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"(typeof key)[]"###,
        &[],
    );
}

#[test]
fn template_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?'a1':'a2'] satisfies `a${number}`[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"("a1" | "a2")[]"###,
        &[],
    );
}

#[test]
fn string_mapping_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?'A':'B'] satisfies Uppercase<string>[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"("A" | "B")[]"###,
        &[],
    );
}

#[test]
fn regular_assertion() {
    verify_options(
        r###"const result=[1 as 1] satisfies number[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"1[]"###,
        &[],
    );
}

#[test]
fn plain_primitive_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?1:2] satisfies number[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn unique_symbol_without_context() {
    verify_options(
        r###"declare const key:unique symbol;const result=[key];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"symbol[]"###,
        &[],
    );
}

#[test]
fn unique_symbol_primitive_context() {
    verify_options(
        r###"declare const key:unique symbol;const result=[key] satisfies symbol[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"symbol[]"###,
        &[],
    );
}

#[test]
fn array_reference_keeps_literals() {
    verify_options(
        r###"const a=[1] satisfies 1[];const result=a;result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"1[]"###,
        &[],
    );
}

#[test]
fn keyof_constraint_accepts_known_key() {
    verify_options(
        r###"function f<T extends {a:number}>(){const result=['a'] satisfies (keyof T)[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###""a"[]"###,
        &[],
    );
}

#[test]
fn keyof_constraint_rejects_unknown_key() {
    verify_options(
        r###"function f<T extends {a:number}>(){const result=['b'] satisfies (keyof T)[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###""b"[]"###,
        &[(
            2322,
            r###"Type 'string' is not assignable to type 'keyof T'.
  Type '"b"' is not assignable to type '"a"'."###,
            49,
            3,
        )],
    );
}

#[test]
fn unconstrained_parameter_context() {
    verify_options(
        r###"function f<T>(){const result=['a'] satisfies T[];result;}"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"string[]"###,
        &[(
            2322,
            r###"Type 'string' is not assignable to type 'T'.
  'T' could be instantiated with an arbitrary type which could be unrelated to 'string'."###,
            30,
            3,
        )],
    );
}

#[test]
fn primitive_alias_context() {
    verify_options(
        r###"type TextValue=string;const result=['a'] satisfies TextValue[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"string[]"###,
        &[],
    );
}

#[test]
fn lowercase_context() {
    verify_options(
        r###"declare const flag:boolean;const result=[flag?'a':'b'] satisfies Lowercase<string>[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"("a" | "b")[]"###,
        &[],
    );
}

#[test]
fn uppercase_user_alias() {
    verify_options(
        r###"export {};type Uppercase<T>=number;const result=[1] satisfies Uppercase<string>[];result;"###,
        ScriptTarget::ES2020,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}
