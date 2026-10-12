// Expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(
    source: &str,
    strict: bool,
    expected_type: &str,
    expected: &[(u32, &str, &str)],
    arrays: &[(u32, &str)],
) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let output = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2020),
            strict: Some(strict),
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
    for (position, expected_type) in arrays {
        assert_eq!(
            output.expression_types.get(position).map(String::as_str),
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
fn number_then_string() {
    verify(
        "const value=[0, \"\"];value;",
        true,
        "(string | number)[]",
        &[],
        &[(12, "(string | number)[]")],
    );
}

#[test]
fn string_then_number() {
    verify(
        "const value=[\"\", 0];value;",
        true,
        "(string | number)[]",
        &[],
        &[(12, "(string | number)[]")],
    );
}

#[test]
fn boolean_number_string() {
    verify(
        "const value=[true, 0, \"\"];value;",
        true,
        "(string | number | boolean)[]",
        &[],
        &[(12, "(string | number | boolean)[]")],
    );
}

#[test]
fn primitive_order() {
    verify(
        "declare const sym:symbol;const value=[sym,true,1n,0,\"\"];value;",
        true,
        "(string | number | bigint | boolean | symbol)[]",
        &[],
        &[(37, "(string | number | bigint | boolean | symbol)[]")],
    );
}

#[test]
fn nullish_primitive_order() {
    verify(
        "const value=[undefined,null,true,0,\"\"];value;",
        true,
        "(string | number | boolean | null | undefined)[]",
        &[],
        &[(12, "(string | number | boolean | null | undefined)[]")],
    );
}

#[test]
fn nullish_only() {
    verify(
        "const value=[undefined,null];value;",
        true,
        "(null | undefined)[]",
        &[],
        &[(12, "(null | undefined)[]")],
    );
}

#[test]
fn nested_union_elements() {
    verify(
        "declare const input:string|number;const value=[true,input];value;",
        true,
        "(string | number | boolean)[]",
        &[],
        &[(46, "(string | number | boolean)[]")],
    );
}

#[test]
fn boolean_union() {
    verify(
        "declare const input:true|false;const value=[0,input];value;",
        true,
        "(number | boolean)[]",
        &[],
        &[(43, "(number | boolean)[]")],
    );
}

#[test]
fn const_literal_order() {
    verify(
        "const value=[2 as const,\"b\" as const,1 as const,\"a\" as const];value;",
        true,
        "(2 | \"b\" | 1 | \"a\")[]",
        &[],
        &[(12, "(2 | \"b\" | 1 | \"a\")[]")],
    );
}

#[test]
fn boolean_consts() {
    verify(
        "const value=[true as const,false as const];value;",
        true,
        "boolean[]",
        &[],
        &[(12, "boolean[]")],
    );
}

#[test]
fn broad_and_literal() {
    verify(
        "const value=[1,2 as const,\"a\"];value;",
        true,
        "(string | number)[]",
        &[],
        &[(12, "(string | number)[]")],
    );
}

#[test]
fn unknown_absorbs() {
    verify(
        "declare const input:unknown;const value=[1,input,\"a\"];value;",
        true,
        "unknown[]",
        &[],
        &[(40, "unknown[]")],
    );
}

#[test]
fn any_absorbs() {
    verify(
        "declare const input:any;const value=[1,input,\"a\"];value;",
        true,
        "any[]",
        &[],
        &[(36, "any[]")],
    );
}

#[test]
fn never_omitted() {
    verify(
        "declare const input:never;const value=[input,1,\"a\"];value;",
        true,
        "(string | number)[]",
        &[],
        &[(38, "(string | number)[]")],
    );
}

#[test]
fn nested_arrays() {
    verify(
        "const value=[[0, \"\"],[true,\"\"]];value;",
        true,
        "((string | number)[] | (string | boolean)[])[]",
        &[],
        &[
            (12, "((string | number)[] | (string | boolean)[])[]"),
            (13, "(string | number)[]"),
            (21, "(string | boolean)[]"),
        ],
    );
}

#[test]
fn array_spread() {
    verify(
        "declare const input:(number|string)[];const value=[true,...input];value;",
        true,
        "(string | number | boolean)[]",
        &[],
        &[(50, "(string | number | boolean)[]")],
    );
}

#[test]
fn const_tuple_spread() {
    verify(
        "const input=[0,\"a\"] as const;const value=[true,...input];value;",
        true,
        "(boolean | 0 | \"a\")[]",
        &[],
        &[(12, "readonly [0, \"a\"]"), (41, "(boolean | 0 | \"a\")[]")],
    );
}

#[test]
fn number_string_return() {
    verify(
        "function make(){return [0,\"\"];}const value=make();value;",
        true,
        "(string | number)[]",
        &[],
        &[(23, "(string | number)[]")],
    );
}

#[test]
fn nested_property_array() {
    verify(
        "const holder={items:[0, \"\"]};const value=holder.items;value;",
        true,
        "(string | number)[]",
        &[],
        &[(20, "(string | number)[]")],
    );
}

#[test]
fn generic_contextual_array() {
    verify(
        "function id<T>(input:T[]):T[]{return input;}const value=id([0,\"\"]);value;",
        true,
        "(string | number)[]",
        &[],
        &[(59, "(string | number)[]")],
    );
}

#[test]
fn contextual_any_array() {
    verify("function id<T extends any[]>(input:T):T{return input;}const value=id([true,0,\"\"]);value;", true, "(string | number | boolean)[]", &[
], &[
(69, "(string | number | true)[]"),
]);
}

#[test]
fn disjoint_interfaces() {
    verify("interface Left{left:number}interface Right{right:number}declare const left:Left;declare const right:Right;const value=[right,left];value;", true, "(Right | Left)[]", &[
], &[
(118, "(Right | Left)[]"),
]);
}

#[test]
fn object_primitive_order() {
    verify(
        "interface Item{id:number}declare const item:Item;const value=[item,0,\"\"];value;",
        true,
        "(string | number | Item)[]",
        &[],
        &[(61, "(string | number | Item)[]")],
    );
}

#[test]
fn function_primitive_order() {
    verify(
        "declare const fn:()=>void;const value=[fn,0,\"\"];value;",
        true,
        "(string | number | (() => void))[]",
        &[],
        &[(38, "(string | number | (() => void))[]")],
    );
}

#[test]
fn non_strict_nullish() {
    verify(
        "const value=[undefined,null,0,\"\"];value;",
        false,
        "(string | number)[]",
        &[],
        &[(12, "(string | number)[]")],
    );
}

#[test]
fn primitive_diagnostic() {
    verify(
        "const value:number[]=[true,0,\"\"];value;",
        true,
        "number[]",
        &[
            (
                2322,
                "Type 'boolean' is not assignable to type 'number'.",
                "true",
            ),
            (
                2322,
                "Type 'string' is not assignable to type 'number'.",
                "\"\"",
            ),
        ],
        &[(21, "(string | number | boolean)[]")],
    );
}

#[test]
fn direct_any_context() {
    verify(
        "const value:any[]=[true,0,\"\"];value;",
        true,
        "any[]",
        &[],
        &[(18, "(string | number | boolean)[]")],
    );
}

#[test]
fn unknown_context() {
    verify(
        "const value:unknown[]=[true,0,\"\"];value;",
        true,
        "unknown[]",
        &[],
        &[(22, "(string | number | boolean)[]")],
    );
}

#[test]
fn unknown_generic_context() {
    verify("function id<T extends unknown[]>(arg:T):T{return arg;}const value=id([true,0,\"\"]);value;", true, "(string | number | boolean)[]", &[], &[
(69, "(string | number | true)[]"),
]);
}

#[test]
fn fresh_const_identifier() {
    verify(
        "const input=\"b\";const value=[\"a\" as const,input];value;",
        true,
        "string[]",
        &[],
        &[(28, "string[]")],
    );
}

#[test]
fn regular_const_identifier() {
    verify(
        "const input=\"b\" as const;const value=[1,input];value;",
        true,
        "(number | \"b\")[]",
        &[],
        &[(37, "(number | \"b\")[]")],
    );
}

#[test]
fn annotated_literal_identifier() {
    verify(
        "let input:\"b\"=\"b\";const value=[1,input];value;",
        true,
        "(number | \"b\")[]",
        &[],
        &[(30, "(number | \"b\")[]")],
    );
}

#[test]
fn fresh_const_alias() {
    verify(
        "const original=\"b\";const input=original;const value=[input];value;",
        true,
        "string[]",
        &[],
        &[(52, "string[]")],
    );
}

#[test]
fn fresh_conditional() {
    verify(
        "declare const flag:boolean;const value=[flag?\"a\":\"b\"];value;",
        true,
        "string[]",
        &[],
        &[(39, "string[]")],
    );
}

#[test]
fn regular_conditional() {
    verify(
        "declare const flag:boolean;const value=[flag?(\"a\" as const):\"b\"];value;",
        true,
        "string[]",
        &[],
        &[(39, "string[]")],
    );
}

#[test]
fn nullish_only_non_strict() {
    verify(
        "const value=[undefined,null];value;",
        false,
        "any[]",
        &[],
        &[(12, "null[]")],
    );
}

#[test]
fn unknown_before_any() {
    verify(
        "declare const a:unknown;declare const b:any;const value=[a,b];value;",
        true,
        "any[]",
        &[],
        &[(56, "any[]")],
    );
}
