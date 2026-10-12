// Exact types and diagnostics checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify_options(
    source: &str,
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
            target: Some(ScriptTarget::ES2015),
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
fn zero() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[0]; result;",
        false,
        true,
        "string",
        &[],
    );
}

#[test]
fn one() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn string() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[\"1\"]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn past_end() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[2]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index '2'.",
            55,
            1,
        )],
    );
}

#[test]
fn large() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[1000]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index '1000'.",
            55,
            4,
        )],
    );
}

#[test]
fn negative() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[-1]; result;",
        false,
        true,
        "undefined",
        &[(
            2514,
            "A tuple type cannot be indexed with a negative value.",
            55,
            2,
        )],
    );
}

#[test]
fn fraction() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[1.5]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index '1.5'.",
            55,
            3,
        )],
    );
}

#[test]
fn negative_fraction() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[-0.5]; result;",
        false,
        true,
        "undefined",
        &[(
            2514,
            "A tuple type cannot be indexed with a negative value.",
            55,
            4,
        )],
    );
}

#[test]
fn nan() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[NaN]; result;",
        false,
        true,
        "string | number",
        &[],
    );
}

#[test]
fn infinity() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[Infinity]; result;",
        false,
        true,
        "string | number",
        &[],
    );
}

#[test]
fn string_negative() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[\"-1\"]; result;",
        false,
        true,
        "undefined",
        &[(
            2514,
            "A tuple type cannot be indexed with a negative value.",
            55,
            4,
        )],
    );
}

#[test]
fn string_fraction() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[\"0.5\"]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index '0.5'.",
            55,
            5,
        )],
    );
}

#[test]
fn noncanonical() {
    verify_options("declare const tuple:[string,number];const result=tuple[\"01\"]; result;", false, true, "any", &[
        (7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", 55, 4),
    ]);
}

#[test]
fn negative_zero() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[-0]; result;",
        false,
        true,
        "string",
        &[],
    );
}

#[test]
fn string_negative_zero() {
    verify_options("declare const tuple:[string,number];const result=tuple[\"-0\"]; result;", false, true, "any", &[
        (7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", 55, 4),
    ]);
}

#[test]
fn optional() {
    verify_options(
        "declare const tuple:[string,number?];const result=tuple[1]; result;",
        false,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn optional_unchecked() {
    verify_options(
        "declare const tuple:[string,number?];const result=tuple[1]; result;",
        true,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn optional_past() {
    verify_options("declare const tuple:[string,number?];const result=tuple[2]; result;", false, true, "undefined", &[
        (2493, "Tuple type '[string, (number | undefined)?]' of length '2' has no element at index '2'.", 56, 1),
    ]);
}

#[test]
fn optional_past_unchecked() {
    verify_options("declare const tuple:[string,number?];const result=tuple[2]; result;", true, true, "undefined", &[
        (2493, "Tuple type '[string, (number | undefined)?]' of length '2' has no element at index '2'.", 56, 1),
    ]);
}

#[test]
fn rest() {
    verify_options(
        "declare const tuple:[string,...number[]];const result=tuple[3]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn rest_unchecked() {
    verify_options(
        "declare const tuple:[string,...number[]];const result=tuple[3]; result;",
        true,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn rest_at() {
    verify_options(
        "declare const tuple:[string,...number[]];const result=tuple[1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn rest_at_unchecked() {
    verify_options(
        "declare const tuple:[string,...number[]];const result=tuple[1]; result;",
        true,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn rest_negative() {
    verify_options(
        "declare const tuple:[string,...number[]];const result=tuple[-1]; result;",
        false,
        true,
        "string | number",
        &[],
    );
}

#[test]
fn rest_negative_unchecked() {
    verify_options(
        "declare const tuple:[string,...number[]];const result=tuple[-1]; result;",
        true,
        true,
        "string | number | undefined",
        &[],
    );
}

#[test]
fn middle_rest() {
    verify_options(
        "declare const tuple:[string,...number[],boolean];const result=tuple[1]; result;",
        false,
        true,
        "number | boolean",
        &[],
    );
}

#[test]
fn middle_rest_unchecked() {
    verify_options(
        "declare const tuple:[string,...number[],boolean];const result=tuple[1]; result;",
        true,
        true,
        "number | boolean",
        &[],
    );
}

#[test]
fn middle_rest_far() {
    verify_options(
        "declare const tuple:[string,...number[],boolean];const result=tuple[100]; result;",
        false,
        true,
        "number | boolean",
        &[],
    );
}

#[test]
fn middle_rest_far_unchecked() {
    verify_options(
        "declare const tuple:[string,...number[],boolean];const result=tuple[100]; result;",
        true,
        true,
        "number | boolean | undefined",
        &[],
    );
}

#[test]
fn empty() {
    verify_options(
        "declare const tuple:[];const result=tuple[0]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[]' of length '0' has no element at index '0'.",
            42,
            1,
        )],
    );
}

#[test]
fn empty_unchecked() {
    verify_options(
        "declare const tuple:[];const result=tuple[0]; result;",
        true,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[]' of length '0' has no element at index '0'.",
            42,
            1,
        )],
    );
}

#[test]
fn readonly() {
    verify_options(
        "declare const tuple:readonly [string,number];const result=tuple[2]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type 'readonly [string, number]' of length '2' has no element at index '2'.",
            64,
            1,
        )],
    );
}

#[test]
fn readonly_unchecked() {
    verify_options(
        "declare const tuple:readonly [string,number];const result=tuple[2]; result;",
        true,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type 'readonly [string, number]' of length '2' has no element at index '2'.",
            64,
            1,
        )],
    );
}

#[test]
fn alias() {
    verify_options(
        "type Pair<T>=[T,number];declare const tuple:Pair<string>;const result=tuple[2]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type 'Pair<string>' of length '2' has no element at index '2'.",
            76,
            1,
        )],
    );
}

#[test]
fn alias_optional() {
    verify_options(
        "type Pair<T>=[T,number?];declare const tuple:Pair<string>;const result=tuple[2]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type 'Pair<string>' of length '2' has no element at index '2'.",
            77,
            1,
        )],
    );
}

#[test]
fn union_index() {
    verify_options("declare const tuple:[string,number];declare const key:0|2;const result=tuple[key]; result;", false, true, "string | undefined", &[
        (2493, "Tuple type '[string, number]' of length '2' has no element at index '2'.", 77, 3),
    ]);
}

#[test]
fn union_index_valid() {
    verify_options("declare const tuple:[string,number];declare const key:0|1;const result=tuple[key]; result;", false, true, "string | number", &[
    ]);
}

#[test]
fn union_receiver() {
    verify_options(
        "declare const tuple:[string]|[number,boolean];const result=tuple[1]; result;",
        false,
        true,
        "boolean | undefined",
        &[],
    );
}

#[test]
fn union_receiver_missing() {
    verify_options(
        "declare const tuple:[string]|[number,boolean];const result=tuple[2]; result;",
        false,
        true,
        "undefined",
        &[(
            2339,
            "Property '2' does not exist on type '[string] | [number, boolean]'.",
            65,
            1,
        )],
    );
}

#[test]
fn union_array() {
    verify_options(
        "declare const tuple:[string]|number[];const result=tuple[2]; result;",
        false,
        true,
        "string | number",
        &[],
    );
}

#[test]
fn generic() {
    verify_options("function f<T extends [string,number]>(tuple:T){const result=tuple[2];result;}const result=1; result;", false, true, "1", &[
        (2493, "Tuple type '[string, number]' of length '2' has no element at index '2'.", 66, 1),
    ]);
}

#[test]
fn dynamic_fixed() {
    verify_options("declare const tuple:[string,number];declare const key:number;const result=tuple[key]; result;", false, true, "string | number", &[
    ]);
}

#[test]
fn dynamic_fixed_unchecked() {
    verify_options("declare const tuple:[string,number];declare const key:number;const result=tuple[key]; result;", true, true, "string | number | undefined", &[
    ]);
}

#[test]
fn dynamic_optional() {
    verify_options("declare const tuple:[string,number?];declare const key:number;const result=tuple[key]; result;", false, true, "string | number | undefined", &[
    ]);
}

#[test]
fn dynamic_optional_unchecked() {
    verify_options("declare const tuple:[string,number?];declare const key:number;const result=tuple[key]; result;", true, true, "string | number | undefined", &[
    ]);
}

#[test]
fn dynamic_rest() {
    verify_options("declare const tuple:[string,...number[]];declare const key:number;const result=tuple[key]; result;", false, true, "string | number", &[
    ]);
}

#[test]
fn dynamic_rest_unchecked() {
    verify_options("declare const tuple:[string,...number[]];declare const key:number;const result=tuple[key]; result;", true, true, "string | number | undefined", &[
    ]);
}

#[test]
fn dynamic_middle_rest() {
    verify_options("declare const tuple:[string,...number[],boolean];declare const key:number;const result=tuple[key]; result;", false, true, "string | number | boolean", &[
    ]);
}

#[test]
fn dynamic_middle_rest_unchecked() {
    verify_options("declare const tuple:[string,...number[],boolean];declare const key:number;const result=tuple[key]; result;", true, true, "string | number | boolean | undefined", &[
    ]);
}

#[test]
fn dynamic_empty() {
    verify_options(
        "declare const tuple:[];declare const key:number;const result=tuple[key]; result;",
        false,
        true,
        "never",
        &[],
    );
}

#[test]
fn dynamic_empty_unchecked() {
    verify_options(
        "declare const tuple:[];declare const key:number;const result=tuple[key]; result;",
        true,
        true,
        "undefined",
        &[],
    );
}

#[test]
fn type_past_end() {
    verify_options(
        "type Value=[string,number][2];declare const result:Value; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index '2'.",
            27,
            1,
        )],
    );
}

#[test]
fn type_negative() {
    verify_options(
        "type Value=[string,number][-1];declare const result:Value; result;",
        false,
        true,
        "undefined",
        &[(
            2514,
            "A tuple type cannot be indexed with a negative value.",
            27,
            2,
        )],
    );
}

#[test]
fn type_string() {
    verify_options(
        "type Value=[string,number][\"1\"];declare const result:Value; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn type_optional() {
    verify_options(
        "type Value=[string,number?][1];declare const result:Value; result;",
        false,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn write_past() {
    verify_options(
        "declare let tuple:[string,number];tuple[2]=true;const result=tuple[2]; result;",
        false,
        true,
        "undefined",
        &[
            (
                2322,
                "Type 'true' is not assignable to type 'undefined'.",
                34,
                8,
            ),
            (
                2493,
                "Tuple type '[string, number]' of length '2' has no element at index '2'.",
                40,
                1,
            ),
            (
                2493,
                "Tuple type '[string, number]' of length '2' has no element at index '2'.",
                67,
                1,
            ),
        ],
    );
}

#[test]
fn write_valid() {
    verify_options(
        "declare let tuple:[string,number];tuple[1]=0;const result=tuple[1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn write_union() {
    verify_options("declare let tuple:[string,number];declare const key:0|1;tuple[key]=0;const result=tuple[0]; result;", false, true, "string", &[
        (2322, "Type '0' is not assignable to type 'never'.", 56, 10),
    ]);
}

#[test]
fn optional_nonstrict() {
    verify_options(
        "declare const tuple:[string,number?];const result=tuple[1]; result;",
        false,
        false,
        "number",
        &[],
    );
}

#[test]
fn optional_past_nonstrict() {
    verify_options(
        "declare const tuple:[string,number?];const result=tuple[2]; result;",
        false,
        false,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number?]' of length '2' has no element at index '2'.",
            56,
            1,
        )],
    );
}

#[test]
fn negative_nonstrict() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[-1]; result;",
        false,
        false,
        "undefined",
        &[(
            2514,
            "A tuple type cannot be indexed with a negative value.",
            55,
            2,
        )],
    );
}

#[test]
fn past_end_nonstrict() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[2]; result;",
        false,
        false,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index '2'.",
            55,
            1,
        )],
    );
}

#[test]
fn dynamic_optional_nonstrict() {
    verify_options("declare const tuple:[string,number?];declare const key:number;const result=tuple[key]; result;", false, false, "string | number", &[
    ]);
}

#[test]
fn extra_const_tuple() {
    verify_options(
        "const tuple=[1,2] as const;const result=tuple[2]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type 'readonly [1, 2]' of length '2' has no element at index '2'.",
            46,
            1,
        )],
    );
}

#[test]
fn extra_const_tuple_valid() {
    verify_options(
        "const tuple=[1,2] as const;const result=tuple[1]; result;",
        false,
        true,
        "2",
        &[],
    );
}

#[test]
fn extra_named_tuple() {
    verify_options("declare const tuple:[first:string,second:number];const result=tuple[2]; result;", false, true, "undefined", &[
        (2493, "Tuple type '[first: string, second: number]' of length '2' has no element at index '2'.", 68, 1),
    ]);
}

#[test]
fn extra_named_optional_tuple() {
    verify_options("declare const tuple:[first:string,second?:number];const result=tuple[2]; result;", false, true, "undefined", &[
        (2493, "Tuple type '[first: string, second?: number | undefined]' of length '2' has no element at index '2'.", 69, 1),
    ]);
}

#[test]
fn extra_named_rest_tuple() {
    verify_options(
        "declare const tuple:[first:string,...tail:number[]];const result=tuple[5]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn extra_readonly_valid() {
    verify_options(
        "declare const tuple:readonly [string,number];const result=tuple[1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn extra_readonly_write() {
    verify_options(
        "declare let tuple:readonly [string,number];tuple[1]=0;const result=tuple[1]; result;",
        false,
        true,
        "number",
        &[(
            2540,
            "Cannot assign to '1' because it is a read-only property.",
            49,
            1,
        )],
    );
}

#[test]
fn extra_nested_rest_tuple() {
    verify_options(
        "declare const tuple:[string,...[number,boolean]];const result=tuple[2]; result;",
        false,
        true,
        "boolean",
        &[],
    );
}

#[test]
fn extra_nested_rest_past() {
    verify_options(
        "declare const tuple:[string,...[number,boolean]];const result=tuple[3]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number, boolean]' of length '3' has no element at index '3'.",
            68,
            1,
        )],
    );
}

#[test]
fn extra_nested_alias_rest() {
    verify_options("type Tail=[number,boolean];declare const tuple:[string,...Tail];const result=tuple[3]; result;", false, true, "undefined", &[
        (2493, "Tuple type '[string, number, boolean]' of length '3' has no element at index '3'.", 83, 1),
    ]);
}

#[test]
fn extra_rest_array_alias() {
    verify_options(
        "type Tail=number[];declare const tuple:[string,...Tail];const result=tuple[3]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn extra_type_alias_index() {
    verify_options(
        "type Tuple=[string,number];type Value=Tuple[2];declare const result:Value; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type 'Tuple' of length '2' has no element at index '2'.",
            44,
            1,
        )],
    );
}

#[test]
fn extra_type_optional_nonstrict() {
    verify_options(
        "type Value=[string,number?][1];declare const result:Value; result;",
        false,
        false,
        "number",
        &[],
    );
}

#[test]
fn extra_type_rest() {
    verify_options(
        "type Value=[string,...number[]][3];declare const result:Value; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn extra_type_nested_rest() {
    verify_options(
        "type Value=[string,...[number,boolean]][2];declare const result:Value; result;",
        false,
        true,
        "boolean",
        &[],
    );
}

#[test]
fn extra_type_union() {
    verify_options(
        "type Value=([string]|[number,boolean])[2];declare const result:Value; result;",
        false,
        true,
        "undefined",
        &[(
            2339,
            "Property '2' does not exist on type '[string] | [number, boolean]'.",
            39,
            1,
        )],
    );
}

#[test]
fn extra_union_negative() {
    verify_options(
        "declare const tuple:[string]|[number,boolean];const result=tuple[-1]; result;",
        false,
        true,
        "string | number | boolean",
        &[(
            2339,
            "Property '-1' does not exist on type '[string] | [number, boolean]'.",
            65,
            2,
        )],
    );
}

#[test]
fn extra_optional_chain() {
    verify_options(
        "declare const tuple:[string,number]|undefined;const result=tuple?.[1]; result;",
        false,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn extra_any_index() {
    verify_options("declare const tuple:[string,number];declare const key:any;const result=tuple[key]; result;", false, true, "string | number", &[
    ]);
}

#[test]
fn extra_length() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[\"length\"]; result;",
        false,
        true,
        "2",
        &[],
    );
}

#[test]
fn extra_optional_length() {
    verify_options(
        "declare const tuple:[string,number?];const result=tuple[\"length\"]; result;",
        false,
        true,
        "1 | 2",
        &[],
    );
}

#[test]
fn extra_rest_length() {
    verify_options(
        "declare const tuple:[string,...number[]];const result=tuple[\"length\"]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn extra_negative_zero_float() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[-0.0]; result;",
        false,
        true,
        "string",
        &[],
    );
}

#[test]
fn extra_hex_index() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[0x1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn extra_infinity_string() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[\"Infinity\"]; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index 'Infinity'.",
            55,
            10,
        )],
    );
}

#[test]
fn extra_nan_string() {
    verify_options(
        "declare const tuple:[string,number];const result=tuple[\"NaN\"]; result;",
        false,
        true,
        "string | number",
        &[(
            2493,
            "Tuple type '[string, number]' of length '2' has no element at index 'NaN'.",
            55,
            5,
        )],
    );
}

#[test]
fn extra_write_rest() {
    verify_options(
        "declare let tuple:[string,...number[]];tuple[2]=\"bad\";const result=tuple[2]; result;",
        false,
        true,
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            39,
            8,
        )],
    );
}

#[test]
fn extra_write_optional() {
    verify_options(
        "declare let tuple:[string,number?];tuple[1]=undefined;const result=tuple[1]; result;",
        false,
        true,
        "undefined",
        &[],
    );
}

fn verify_merged_import(
    other: &str,
    source: &str,
    expected_type: &str,
    expected: &[(u32, &str, u32, u32)],
) {
    let dependency = tsc_rs_parser::parse("/other.ts", other);
    let file = tsc_rs_parser::parse("/subject.ts", source);
    assert!(dependency.diagnostics.is_empty());
    assert!(file.diagnostics.is_empty());
    let mut checker = TypeChecker::new();
    checker.register_available_files(&["/other.ts".into(), "/subject.ts".into()]);
    checker.register_virtual_module_paths("/subject.ts", &[("./other".into(), "/other.ts".into())]);
    checker.inject_external_types(&[&dependency]);
    checker.set_current_file_name("/subject.ts");
    let output = checker.check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    let actual = output
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
    assert_eq!(actual, expected, "{other}: {source}");
    let position = source.rfind("result;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&position).map(String::as_str),
        Some(expected_type),
        "{source}: {:?}",
        output.expression_types
    );
}

#[test]
fn merged_import_type_first() {
    verify_merged_import("export type A=string;function A(n:number){return n;}export {A};export type B=string;const B=10;export {B};","import {A,B} from \"./other\";const result=A(B); result;","number",&[
 ]);
}

#[test]
fn merged_import_value_first() {
    verify_merged_import("function A(n:number){return n;}export {A};export type A=string;const B=10;export {B};export type B=string;","import {A,B} from \"./other\";const result=A(B); result;","number",&[
 ]);
}

#[test]
fn merged_import_namespace() {
    verify_merged_import("export type A=string;function A(n:number){return n;}export {A};export type B=string;const B=10;export {B};","import * as N from \"./other\";const result=N.A(N.B); result;","number",&[
 ]);
}

#[test]
fn merged_import_reject_argument() {
    verify_merged_import(
        "export type A=string;function A(n:number){return n;}export {A};",
        "import {A} from \"./other\";const result=A(\"bad\"); result;",
        "number",
        &[(
            2345,
            "Argument of type 'string' is not assignable to parameter of type 'number'.",
            41,
            5,
        )],
    );
}
