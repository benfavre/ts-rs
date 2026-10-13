// Diagnostic expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str, expected: &[(u32, &str, u32, u32)]) {
    let file = tsc_rs_parser::parse("conditional.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let output = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = output
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.code,
                diagnostic.message.as_str(),
                span.start,
                span.end - span.start,
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn object_constraint_preserves_both_array_outcomes() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends object>(x: IsArray<T>) {
    let onlyTrue: true = x;
    let onlyFalse: false = x;
    let either: boolean = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                108,
                8,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                136,
                9,
            ),
        ],
    );
}

#[test]
fn array_constraint_selects_true_branch() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends unknown[]>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}
function g<T extends string[]>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}"###,
        &[
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'false'.
  Type 'true' is not assignable to type 'false'."###,
                134,
                2,
            ),
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'false'.
  Type 'true' is not assignable to type 'false'."###,
                230,
                2,
            ),
        ],
    );
}

#[test]
fn disjoint_constraint_selects_false_branch() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends string>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}
function g<T extends Function>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}"###,
        &[
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'true'.
  Type 'false' is not assignable to type 'true'."###,
                108,
                3,
            ),
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'true'.
  Type 'false' is not assignable to type 'true'."###,
                204,
                3,
            ),
        ],
    );
}

#[test]
fn partial_union_overlap_keeps_true_branch() {
    check(
        r###"type Matches<T> = T extends "abc" | 42 ? true : false;
function f<T extends string>(x: Matches<T>) {
    let no: false = x;
    let yes: true = x;
    let either: boolean = x;
}
function g<T extends number | string>(x: Matches<T>) {
    let no: false = x;
    let either: boolean = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                109,
                2,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                132,
                3,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                241,
                2,
            ),
        ],
    );
}

#[test]
fn nested_conditions_respect_non_nullable_constraint() {
    check(
        r###"type Zero<T> = T extends null ? null : T extends undefined ? undefined : T extends string ? "" : T extends number ? 0 : T extends boolean ? false : never;
function f<T extends {}>(x: Zero<T>) {
    let zero: "" | 0 | false = x;
}"###,
        &[],
    );
}

#[test]
fn deferred_branches_and_targets_remain_supported() {
    check(
        r###"type Unwrap<T> = T extends (infer U)[] ? U : T;
type Deferred<T, U> = T extends U ? true : false;
function f<T, U>(x: Deferred<T, U>, y: Unwrap<T>, z: T extends string ? T : never) {
    let same: Deferred<T, U> = x;
    let value: Unwrap<T> = y;
    let original: T = z;
}
function g<T>(x: T extends string ? string : number) {
    let primitive: string | number = x;
}"###,
        &[],
    );
}

#[test]
fn unconstrained_and_never_constraints_keep_default_union() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T>(x: IsArray<T>) {
    let no: false = x;
    let either: boolean = x;
}
function g<T extends never>(x: IsArray<T>) {
    let no: false = x;
    let either: boolean = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                93,
                2,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                192,
                2,
            ),
        ],
    );
}

#[test]
fn primitive_branches_can_flow_to_union_destinations() {
    check(
        r###"type Select<T> = T extends string ? "text" : 42;
function f<T extends object>(x: Select<T>) {
    let yes: 42 = x;
    let either: "text" | 42 = x;
}
function g<T extends string | number>(x: Select<T>) {
    let either: "text" | 42 = x;
}"###,
        &[],
    );
}

#[test]
fn any_and_unknown_extends_respect_explicit_constraint() {
    check(
        r###"type Always<T> = T extends unknown ? true : false;
type AnyTest<T> = T extends any ? true : false;
function f<T>(x: Always<T>, y: AnyTest<T>) {
    let yes: true = x;
    let alsoYes: true = y;
}
function g<T extends unknown>(x: Always<T>) {
    let yes: true = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                152,
                3,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                175,
                7,
            ),
        ],
    );
}

#[test]
fn unnamed_conditional_displays_boolean() {
    check(
        r###"function f<T>(x: T extends string ? true : false) { let y: true = x; }"###,
        &[(
            2322,
            r###"Type 'boolean' is not assignable to type 'true'."###,
            56,
            1,
        )],
    );
}

#[test]
fn primitive_union_constraint_diagnostics() {
    check(
        r###"type Select<T> = T extends string ? "text" : 42;
function f<T>(x: Select<T>) { let y: string = x; }
function g<T extends object>(x: Select<T>) { let y: string = x; }"###,
        &[
            (
                2322,
                r###"Type 'string | number' is not assignable to type 'string'.
  Type 'number' is not assignable to type 'string'."###,
                83,
                1,
            ),
            (
                2322,
                r###"Type 'Select<T>' is not assignable to type 'string'.
  Type 'number' is not assignable to type 'string'."###,
                149,
                1,
            ),
        ],
    );
}

#[test]
fn augmented_function_and_array_relations_remain_supported() {
    check(
        r###"interface Function extends Array<unknown> {}
type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends Function>(x: IsArray<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn shadowed_function_alias_uses_declared_shape() {
    check(
        r###"type Function = string[];
type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends Function>(x: IsArray<T>) { let yes: true = x; }
export {};"###,
        &[],
    );
}

#[test]
fn augmented_array_relations_remain_supported() {
    check(
        r###"interface Array<T> extends Function {}
type IsFunction<T> = T extends Function ? true : false;
function f<T extends unknown[]>(x: IsFunction<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn never_constraint_falls_back_even_against_unknown() {
    check(
        r###"type Always<T> = T extends unknown ? true : false;
function f<T extends never>(x: Always<T>) { let yes: true = x; let no: false = x; }
function g<T extends any>(x: Always<T>) { let yes: true = x; let no: false = x; }
function h<T extends unknown>(x: Always<T>) { let yes: true = x; let no: false = x; }"###,
        &[
            (2322, "Type 'boolean' is not assignable to type 'true'.", 99, 3),
            (2322, "Type 'boolean' is not assignable to type 'false'.", 118, 2),
            (2322, "Type 'Always<T>' is not assignable to type 'false'.\n  Type 'true' is not assignable to type 'false'.", 200, 2),
            (2322, "Type 'Always<T>' is not assignable to type 'false'.\n  Type 'true' is not assignable to type 'false'.", 286, 2),
        ],
    );
}

#[test]
fn inherited_builtin_constraints_are_not_treated_as_empty_objects() {
    check(
        r###"class MyArray extends Array<unknown> {}
interface ArrayInterface extends Array<unknown> {}
type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends MyArray>(x: IsArray<T>) { let yes: true = x; }
function g<T extends ArrayInterface>(x: IsArray<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn any_and_unknown_constraints_do_not_overlap_never() {
    check(
        r###"type IsNever<T> = T extends never ? true : false;
function f<T extends any>(x: IsNever<T>) { let no: false = x; }
function g<T extends unknown>(x: IsNever<T>) { let no: false = x; }"###,
        &[],
    );
}

#[test]
fn nested_conditionals_preserve_outer_branch_correlation() {
    check(
        r###"type Choice<T> = T extends string ? (T extends string ? true : false) : true;
type Other<T> = T extends string ? (T extends number ? false : true) : true;
function f<T extends string | number>(x: Choice<T>) { let yes: true = x; }
function g<T extends string | number>(x: Other<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn type_parameter_named_function_is_not_the_global_function() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<Function extends unknown[], T extends Function>(x: IsArray<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn array_alias_is_not_the_global_array() {
    check(
        r###"type Array<T> = () => void;
type IsFunction<T> = T extends Function ? true : false;
function f<T extends Array<number>>(x: IsFunction<T>) { let yes: true = x; }
export {};"###,
        &[],
    );
}

#[test]
fn array_class_is_not_the_global_array() {
    check(
        r###"class Array<T> extends Function {}
type IsFunction<T> = T extends Function ? true : false;
function f<T extends Array<number>>(x: IsFunction<T>) { let yes: true = x; }
export {};"###,
        &[],
    );
}
