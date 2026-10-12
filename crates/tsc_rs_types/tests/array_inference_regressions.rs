// Expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(
    source: &str,
    strict_null_checks: bool,
    no_implicit_any: bool,
    expected_type: &str,
    expected: &[(u32, &str, &str)],
    arrays: &[(u32, u32, &str)],
) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let output = checker.check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2020),
            strict: Some(true),
            strict_null_checks: Some(strict_null_checks),
            no_implicit_any: Some(no_implicit_any),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        output.diagnostics.len(),
        expected.len(),
        "{source}: {:?}",
        output.diagnostics
    );
    for (actual, (code, message, text)) in output.diagnostics.iter().zip(expected) {
        assert_eq!(actual.code, *code);
        assert_eq!(actual.message, *message);
        let span = actual.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], *text);
    }
    for (position, end, expected_type) in arrays {
        assert_eq!(
            output
                .expression_type_spans
                .get(&(*position, *end))
                .map(|entry| entry.type_string.as_str()),
            Some(*expected_type),
            "array at {position}: {source}"
        );
    }
    let position = source.rfind("value;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&position).map(String::as_str),
        Some(expected_type),
        "{source}"
    );
}

#[test]
fn empty_object_assertion() {
    verify("enum Mode{None=\"\",Other=\"x\"}interface Target{items:{id:number}[];mode:Mode;}const value={items:[],mode:\"\"} as Target;value;", true, true, "Target", &[
], &[
(95, 97, "never[]"),
]);
}

#[test]
fn contravariant_empty_inference() {
    verify("type Compare<T>=(a:T,b:T)=>number;declare function create<T>(items:T[],compare:Compare<T>):T[];declare const compare:Compare<number>;const value=create([],compare);value;", true, true, "number[]", &[
], &[
(152, 154, "never[]"),
]);
}

#[test]
fn disjoint_object_assertion() {
    verify("declare const source:{a:number};const value=source as {b:number};value;", true, true, "{\n    b: number;\n}", &[
(2352, "Conversion of type '{ a: number; }' to type '{ b: number; }' may be a mistake because neither type sufficiently overlaps with the other. If this was intentional, convert the expression to 'unknown' first.\n  Property 'b' is missing in type '{ a: number; }' but required in type '{ b: number; }'.", "source as {b:number}"),
], &[
]);
}

#[test]
fn mixed_directional_object_assertion() {
    verify("declare const source:{left:1;right:number};const value=source as {left:number;right:2};value;", true, true, "{\n    left: number;\n    right: 2;\n}", &[
], &[
]);
}

#[test]
fn disjoint_common_object_assertion() {
    verify("declare const source:{common:number;left:number};const value=source as {common:number;right:number};value;", true, true, "{\n    common: number;\n    right: number;\n}", &[
(2352, "Conversion of type '{ common: number; left: number; }' to type '{ common: number; right: number; }' may be a mistake because neither type sufficiently overlaps with the other. If this was intentional, convert the expression to 'unknown' first.\n  Property 'right' is missing in type '{ common: number; left: number; }' but required in type '{ common: number; right: number; }'.", "source as {common:number;right:number}"),
], &[
]);
}

#[test]
fn empty_tuple_interface() {
    verify(
        "type Empty=[];interface Custom extends Empty{extra?:number;}const value:Custom=[];value;",
        true,
        true,
        "Custom",
        &[],
        &[(79, 81, "[]")],
    );
}

#[test]
fn empty_pattern_assignment() {
    verify(
        "const value=[]=[1];value;",
        true,
        true,
        "[number]",
        &[],
        &[(12, 14, "[]"), (15, 18, "[number]")],
    );
}

#[test]
fn invalid_empty_pattern_assignment() {
    verify(
        "const value=[]=42;value;",
        true,
        true,
        "42",
        &[(
            2488,
            "Type '42' must have a '[Symbol.iterator]()' method that returns an iterator.",
            "[]",
        )],
        &[(12, 14, "[]")],
    );
}

#[test]
fn non_strict_empty_for_of() {
    verify(
        "for(const value of []){value;}",
        false,
        true,
        "any",
        &[(
            7005,
            "Variable 'value' implicitly has an 'any' type.",
            "value",
        )],
        &[(19, 21, "undefined[]")],
    );
}

#[test]
fn conditional_empty_strict() {
    verify(
        "declare const flag:boolean;const value=flag?[]:[1];value;",
        true,
        true,
        "number[]",
        &[],
        &[(44, 46, "never[]"), (47, 50, "number[]")],
    );
}

#[test]
fn conditional_empty_nonstrict() {
    verify(
        "declare const flag:boolean;const value=flag?[]:[1];value;",
        false,
        true,
        "number[]",
        &[],
        &[(44, 46, "undefined[]"), (47, 50, "number[]")],
    );
}

#[test]
fn conditional_any_array() {
    verify(
        "declare const flag:boolean;declare const items:any[];const value=flag?items:[1];value;",
        true,
        true,
        "any[]",
        &[],
        &[(76, 79, "number[]")],
    );
}

#[test]
fn conditional_undefined_array() {
    verify(
        "declare const flag:boolean;const value=flag?[undefined]:[1];value;",
        true,
        true,
        "undefined[] | number[]",
        &[],
        &[(44, 55, "undefined[]"), (56, 59, "number[]")],
    );
}

#[test]
fn conditional_never_both() {
    verify(
        "declare const flag:boolean;const value={items:flag?[]:[]};value;",
        true,
        true,
        "{\n    items: never[];\n}",
        &[],
        &[(51, 53, "never[]"), (54, 56, "never[]")],
    );
}

#[test]
fn spread_union_arrays() {
    verify(
        "declare const input:string[]|number[];const value=[...input];value;",
        true,
        true,
        "(string | number)[]",
        &[],
        &[(50, 60, "(string | number)[]")],
    );
}

#[test]
fn spread_contextual_union_arrays() {
    verify(
        "declare const input:string[]|number[];const value:(string|number)[]=[...input];value;",
        true,
        true,
        "(string | number)[]",
        &[],
        &[(68, 78, "(string | number)[]")],
    );
}
