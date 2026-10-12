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
fn empty_variable_null_false_implicit_false() {
    verify(
        "const value=[];value;",
        false,
        false,
        "any[]",
        &[],
        &[(12, 14, "undefined[]")],
    );
}

#[test]
fn empty_object_property_null_false_implicit_false() {
    verify(
        "const holder={items:[]};const value=holder.items;value;",
        false,
        false,
        "any[]",
        &[],
        &[(20, 22, "undefined[]")],
    );
}

#[test]
fn empty_function_return_null_false_implicit_false() {
    verify(
        "function make(){return [];}const value=make();value;",
        false,
        false,
        "any[]",
        &[],
        &[(23, 25, "undefined[]")],
    );
}

#[test]
fn empty_contextual_array_null_false_implicit_false() {
    verify(
        "const value:number[]=[];value;",
        false,
        false,
        "number[]",
        &[],
        &[(21, 23, "undefined[]")],
    );
}

#[test]
fn holes_only_null_false_implicit_false() {
    verify(
        "const value=[,,];value;",
        false,
        false,
        "any[]",
        &[],
        &[(12, 16, "undefined[]")],
    );
}

#[test]
fn holes_mixed_null_false_implicit_false() {
    verify(
        "const value=[,1,,\"\"];value;",
        false,
        false,
        "(string | number)[]",
        &[],
        &[(12, 20, "(string | number)[]")],
    );
}

#[test]
fn empty_contextual_array_null_false_implicit_true() {
    verify(
        "const value:number[]=[];value;",
        false,
        true,
        "number[]",
        &[],
        &[(21, 23, "undefined[]")],
    );
}

#[test]
fn holes_mixed_null_false_implicit_true() {
    verify(
        "const value=[,1,,\"\"];value;",
        false,
        true,
        "(string | number)[]",
        &[],
        &[(12, 20, "(string | number)[]")],
    );
}

#[test]
fn empty_variable_null_true_implicit_false() {
    verify(
        "const value=[];value;",
        true,
        false,
        "never[]",
        &[],
        &[(12, 14, "never[]")],
    );
}

#[test]
fn empty_object_property_null_true_implicit_false() {
    verify(
        "const holder={items:[]};const value=holder.items;value;",
        true,
        false,
        "never[]",
        &[],
        &[(20, 22, "never[]")],
    );
}

#[test]
fn empty_function_return_null_true_implicit_false() {
    verify(
        "function make(){return [];}const value=make();value;",
        true,
        false,
        "never[]",
        &[],
        &[(23, 25, "never[]")],
    );
}

#[test]
fn empty_contextual_array_null_true_implicit_false() {
    verify(
        "const value:number[]=[];value;",
        true,
        false,
        "number[]",
        &[],
        &[(21, 23, "never[]")],
    );
}

#[test]
fn holes_only_null_true_implicit_false() {
    verify(
        "const value=[,,];value;",
        true,
        false,
        "undefined[]",
        &[],
        &[(12, 16, "undefined[]")],
    );
}

#[test]
fn holes_mixed_null_true_implicit_false() {
    verify(
        "const value=[,1,,\"\"];value;",
        true,
        false,
        "(string | number | undefined)[]",
        &[],
        &[(12, 20, "(string | number | undefined)[]")],
    );
}

#[test]
fn empty_object_property_null_true_implicit_true() {
    verify(
        "const holder={items:[]};const value=holder.items;value;",
        true,
        true,
        "never[]",
        &[],
        &[(20, 22, "never[]")],
    );
}

#[test]
fn empty_function_return_null_true_implicit_true() {
    verify(
        "function make(){return [];}const value=make();value;",
        true,
        true,
        "never[]",
        &[],
        &[(23, 25, "never[]")],
    );
}

#[test]
fn empty_contextual_array_null_true_implicit_true() {
    verify(
        "const value:number[]=[];value;",
        true,
        true,
        "number[]",
        &[],
        &[(21, 23, "never[]")],
    );
}

#[test]
fn holes_only_null_true_implicit_true() {
    verify(
        "const value=[,,];value;",
        true,
        true,
        "undefined[]",
        &[],
        &[(12, 16, "undefined[]")],
    );
}

#[test]
fn holes_mixed_null_true_implicit_true() {
    verify(
        "const value=[,1,,\"\"];value;",
        true,
        true,
        "(string | number | undefined)[]",
        &[],
        &[(12, 20, "(string | number | undefined)[]")],
    );
}

#[test]
fn empty_parameter_default() {
    verify(
        "function use(input=[]){return input;}const value=use();value;",
        true,
        true,
        "never[]",
        &[],
        &[(19, 21, "never[]")],
    );
}

#[test]
fn empty_nested_array() {
    verify(
        "const value=[[]];value;",
        true,
        true,
        "never[][]",
        &[],
        &[(12, 16, "never[][]"), (13, 15, "never[]")],
    );
}

#[test]
fn empty_union_array() {
    verify(
        "const value=[[],[1]];value;",
        true,
        true,
        "number[][]",
        &[],
        &[
            (12, 20, "number[][]"),
            (13, 15, "never[]"),
            (16, 19, "number[]"),
        ],
    );
}

#[test]
fn empty_export() {
    verify(
        "export const value=[];value;",
        true,
        true,
        "never[]",
        &[],
        &[(19, 21, "never[]")],
    );
}

#[test]
fn array_literal_standalone() {
    verify(
        "[];const value=0;value;",
        true,
        true,
        "0",
        &[],
        &[(0, 2, "never[]")],
    );
}

#[test]
fn empty_generic_inference() {
    verify(
        "function id<T>(input:T[]):T[]{return input;}const value=id([]);value;",
        true,
        true,
        "never[]",
        &[],
        &[(59, 61, "never[]")],
    );
}

#[test]
fn empty_for_of() {
    verify(
        "for(const value of []){value;}",
        true,
        true,
        "never",
        &[],
        &[(19, 21, "never[]")],
    );
}

#[test]
fn holes_contextual_array() {
    verify("const value:number[]=[,1];value;", true, true, "number[]", &[
(2322, "Type '(number | undefined)[]' is not assignable to type 'number[]'.\n  Type 'number | undefined' is not assignable to type 'number'.\n    Type 'undefined' is not assignable to type 'number'.", "value"),
], &[
(21, 25, "(number | undefined)[]"),
]);
}

#[test]
fn holes_array_rest() {
    verify(
        "let [first,...value]=[,1,2];value;",
        true,
        true,
        "[number, number]",
        &[],
        &[(21, 27, "[undefined, number, number]")],
    );
}

#[test]
fn holes_after_spread() {
    verify(
        "declare const input:number[];const value=[...input,,\"a\"];value;",
        true,
        true,
        "(string | number | undefined)[]",
        &[],
        &[(41, 56, "(string | number | undefined)[]")],
    );
}

#[test]
fn holes_positional_context() {
    verify(
        "const value:[undefined,string]=[,\"a\"];value;",
        true,
        true,
        "[undefined, string]",
        &[],
        &[(31, 37, "[undefined, string]")],
    );
}

#[test]
fn nonstrict_contextual_mixed() {
    verify(
        "const value:(string|number)[]=[null,undefined,1,\"\"];value;",
        false,
        false,
        "(string | number)[]",
        &[],
        &[(30, 51, "(string | number)[]")],
    );
}

#[test]
fn holes_required_tuple() {
    verify("const value:[number,string]=[,\"a\"];value;", true, true, "[number, string]", &[
(2322, "Type '[undefined, string]' is not assignable to type '[number, string]'.\n  Type at position 0 in source is not compatible with type at position 0 in target.\n    Type 'undefined' is not assignable to type 'number'.", "value"),
], &[
(28, 34, "[undefined, string]"),
]);
}

#[test]
fn holes_const_element() {
    verify(
        "const tuple=[,1] as const;const value=tuple[1];value;",
        true,
        true,
        "1",
        &[],
        &[(12, 16, "readonly [undefined, 1]")],
    );
}

#[test]
fn empty_push_literal_null_false() {
    verify(
        "[].push(1);const value=0;value;",
        false,
        true,
        "0",
        &[],
        &[(0, 2, "undefined[]")],
    );
}

#[test]
fn empty_map_null_false() {
    verify(
        "const value=[].map(item=>item);value;",
        false,
        true,
        "any[]",
        &[],
        &[(12, 14, "undefined[]")],
    );
}

#[test]
fn undefined_literal_push_null_false() {
    verify(
        "[undefined].push(1);const value=0;value;",
        false,
        true,
        "0",
        &[],
        &[(0, 11, "undefined[]")],
    );
}

#[test]
fn empty_push_literal_null_true() {
    verify(
        "[].push(1);const value=0;value;",
        true,
        true,
        "0",
        &[(
            2345,
            "Argument of type '1' is not assignable to parameter of type 'never'.",
            "1",
        )],
        &[(0, 2, "never[]")],
    );
}

#[test]
fn empty_indexed_read_null_true() {
    verify(
        "const value=[][0];value;",
        true,
        true,
        "never",
        &[],
        &[(12, 14, "never[]")],
    );
}

#[test]
fn empty_map_null_true() {
    verify(
        "const value=[].map(item=>item);value;",
        true,
        true,
        "never[]",
        &[],
        &[(12, 14, "never[]")],
    );
}

#[test]
fn undefined_literal_push_null_true() {
    verify(
        "[undefined].push(1);const value=0;value;",
        true,
        true,
        "0",
        &[(
            2345,
            "Argument of type '1' is not assignable to parameter of type 'undefined'.",
            "1",
        )],
        &[(0, 11, "undefined[]")],
    );
}
