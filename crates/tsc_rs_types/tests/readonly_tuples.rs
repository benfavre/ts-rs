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
fn dynamic_write() {
    verify_options(
        r###"declare let tuple:readonly [string,number];declare const key:number;tuple[key]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [string, number]' only permits reading."###,
            68,
            10,
        )],
    );
}

#[test]
fn alias_write() {
    verify_options(
        r###"type Tuple=readonly [string,number];declare let tuple:Tuple;tuple[1]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '1' because it is a read-only property."###,
            66,
            1,
        )],
    );
}

#[test]
fn alias_dynamic() {
    verify_options(
        r###"type Tuple=readonly [string,number];declare let tuple:Tuple;declare const key:number;tuple[key]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'Tuple' only permits reading."###,
            85,
            10,
        )],
    );
}

#[test]
fn generic_alias() {
    verify_options(
        r###"type Tuple<T>=readonly [T,number];declare let tuple:Tuple<string>;tuple[0]="";const result=tuple[0]; result;"###,
        false,
        true,
        r###"string"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            72,
            1,
        )],
    );
}

#[test]
fn property_write() {
    verify_options(
        r###"declare const holder:{tuple:readonly [string,number]};holder.tuple[1]=0;const result=holder.tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '1' because it is a read-only property."###,
            67,
            1,
        )],
    );
}

#[test]
fn call_write() {
    verify_options(
        r###"declare function tuple():readonly [string,number];tuple()[1]=0;const result=tuple()[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '1' because it is a read-only property."###,
            58,
            1,
        )],
    );
}

#[test]
fn constraint_write() {
    verify_options(
        r###"function f<T extends readonly [string,number]>(tuple:T){tuple[1]=0;return tuple[1];}declare const tuple:readonly [string,number];const result=f(tuple); result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '1' because it is a read-only property."###,
            62,
            1,
        )],
    );
}

#[test]
fn index_union() {
    verify_options(
        r###"declare const tuple:readonly [string,number];declare const key:0|1;tuple[key]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[
            (
                2540,
                r###"Cannot assign to '0' because it is a read-only property."###,
                73,
                3,
            ),
            (
                2540,
                r###"Cannot assign to '1' because it is a read-only property."###,
                73,
                3,
            ),
        ],
    );
}

#[test]
fn receiver_union() {
    verify_options(
        r###"declare const tuple:readonly [string,number]|[string,number];tuple[1]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '1' because it is a read-only property."###,
            67,
            1,
        )],
    );
}

#[test]
fn receiver_union_dynamic() {
    verify_options(
        r###"declare const tuple:readonly [string,number]|[string,number];declare const key:number;tuple[key]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [string, number] | [string, number]' only permits reading."###,
            86,
            10,
        )],
    );
}

#[test]
fn rest_slot() {
    verify_options(
        r###"declare const tuple:readonly [string,...number[]];tuple[2]=0;const result=tuple[2]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [string, ...number[]]' only permits reading."###,
            50,
            8,
        )],
    );
}

#[test]
fn rest_union() {
    verify_options(
        r###"declare const tuple:readonly [string,...number[]];declare const key:0|2;tuple[key]=0;const result=tuple[2]; result;"###,
        false,
        true,
        r###"number"###,
        &[
            (
                2542,
                r###"Index signature in type 'readonly [string, ...number[]]' only permits reading."###,
                72,
                10,
            ),
            (
                2540,
                r###"Cannot assign to '0' because it is a read-only property."###,
                78,
                3,
            ),
        ],
    );
}

#[test]
fn negative() {
    verify_options(
        r###"declare const tuple:readonly [string,number];tuple[-1]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[
            (
                2322,
                r###"Type '0' is not assignable to type 'undefined'."###,
                45,
                9,
            ),
            (
                2514,
                r###"A tuple type cannot be indexed with a negative value."###,
                51,
                2,
            ),
        ],
    );
}

#[test]
fn out_of_bounds() {
    verify_options(
        r###"declare const tuple:readonly [string,number];tuple[2]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[
            (
                2322,
                r###"Type '0' is not assignable to type 'undefined'."###,
                45,
                8,
            ),
            (
                2542,
                r###"Index signature in type 'readonly [string, number]' only permits reading."###,
                45,
                8,
            ),
            (
                2493,
                r###"Tuple type 'readonly [string, number]' of length '2' has no element at index '2'."###,
                51,
                1,
            ),
        ],
    );
}

#[test]
fn fraction() {
    verify_options(
        r###"declare const tuple:readonly [string,number];tuple[0.5]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[
            (
                2322,
                r###"Type '0' is not assignable to type 'undefined'."###,
                45,
                10,
            ),
            (
                2542,
                r###"Index signature in type 'readonly [string, number]' only permits reading."###,
                45,
                10,
            ),
            (
                2493,
                r###"Tuple type 'readonly [string, number]' of length '2' has no element at index '0.5'."###,
                51,
                3,
            ),
        ],
    );
}

#[test]
fn string_index() {
    verify_options(
        r###"declare const tuple:readonly [string,number];tuple["1"]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '1' because it is a read-only property."###,
            51,
            3,
        )],
    );
}

#[test]
fn any_index() {
    verify_options(
        r###"declare const tuple:readonly [string,number];declare const key:any;tuple[key]=0;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [string, number]' only permits reading."###,
            67,
            10,
        )],
    );
}

#[test]
fn increment() {
    verify_options(
        r###"declare const tuple:readonly [number];tuple[0]++;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            44,
            1,
        )],
    );
}

#[test]
fn compound() {
    verify_options(
        r###"declare const tuple:readonly [number];tuple[0]+=1;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            44,
            1,
        )],
    );
}

#[test]
fn length() {
    verify_options(
        r###"declare const tuple:readonly [number];tuple.length=1;const result=tuple.length; result;"###,
        false,
        true,
        r###"1"###,
        &[(
            2540,
            r###"Cannot assign to 'length' because it is a read-only property."###,
            44,
            6,
        )],
    );
}

#[test]
fn length_index() {
    verify_options(
        r###"declare const tuple:readonly [number];tuple["length"]=1;const result=tuple["length"]; result;"###,
        false,
        true,
        r###"1"###,
        &[(
            2540,
            r###"Cannot assign to 'length' because it is a read-only property."###,
            44,
            8,
        )],
    );
}

#[test]
fn delete_fixed() {
    verify_options(
        r###"declare const tuple:readonly [number];delete tuple[0];const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2704,
            r###"The operand of a 'delete' operator cannot be a read-only property."###,
            45,
            8,
        )],
    );
}

#[test]
fn delete_rest() {
    verify_options(
        r###"declare const tuple:readonly [number,...number[]];delete tuple[1];const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [number, ...number[]]' only permits reading."###,
            57,
            8,
        )],
    );
}

#[test]
fn delete_dynamic() {
    verify_options(
        r###"declare const tuple:readonly [number];declare const key:number;delete tuple[key];const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [number]' only permits reading."###,
            70,
            10,
        )],
    );
}

#[test]
fn alias_read() {
    verify_options(
        r###"type Tuple=readonly [string,number];declare const tuple:Tuple;const result=tuple[1]; result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn nested_read() {
    verify_options(
        r###"declare const tuple:readonly [readonly [number]];const result=tuple[0][0]; result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn destructure() {
    verify_options(
        r###"declare const tuple:readonly [string,number];const [,result]=tuple; result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn rest_binding() {
    verify_options(
        r###"declare const tuple:readonly [string,number];const [,...result]=tuple; result;"###,
        false,
        true,
        r###"[number]"###,
        &[],
    );
}

#[test]
fn rest_parameter() {
    verify_options(
        r###"function f(...tuple:readonly [string,number]){return tuple[1];}const result=f("",0); result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn spread() {
    verify_options(
        r###"declare const tuple:readonly [number,number];const result=[...tuple]; result;"###,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn for_of() {
    verify_options(
        r###"declare const tuple:readonly [number,number];let result=0;for(const element of tuple){result=element;} result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn method() {
    verify_options(
        r###"declare const tuple:readonly [number,number];const result=tuple.slice(); result;"###,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn optional() {
    verify_options(
        r###"declare const tuple:readonly [number?];const result=tuple[0]; result;"###,
        false,
        true,
        r###"number | undefined"###,
        &[],
    );
}

#[test]
fn optional_unchecked() {
    verify_options(
        r###"declare const tuple:readonly [string,number];declare const key:number;const result=tuple[key]; result;"###,
        true,
        true,
        r###"string | number | undefined"###,
        &[],
    );
}

#[test]
fn mutable_shadow() {
    verify_options(
        r###"declare let tuple:readonly [string,number];function f(){let tuple:[string,number]=["",0];tuple[1]=1;return tuple[1];}const result=f(); result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn dynamic_incompatible() {
    verify_options(
        r###"declare const tuple:readonly [number];declare const key:number;tuple[key]="";const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[
            (
                2322,
                r###"Type 'string' is not assignable to type 'number'."###,
                63,
                10,
            ),
            (
                2542,
                r###"Index signature in type 'readonly [number]' only permits reading."###,
                63,
                10,
            ),
        ],
    );
}

#[test]
fn fixed_incompatible() {
    verify_options(
        r###"declare const tuple:readonly [number];tuple[0]="";const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            44,
            1,
        )],
    );
}

#[test]
fn const_alias() {
    verify_options(
        r###"const tuple=[1,2] as const;const alias=tuple;alias[0]=1;const result=alias[0]; result;"###,
        false,
        true,
        r###"1"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            51,
            1,
        )],
    );
}

#[test]
fn const_dynamic() {
    verify_options(
        r###"const tuple=[1,2] as const;declare const key:number;tuple[key]=1;const result=tuple[0]; result;"###,
        false,
        true,
        r###"1"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [1, 2]' only permits reading."###,
            52,
            10,
        )],
    );
}

#[test]
fn parenthesized() {
    verify_options(
        r###"declare const tuple:readonly [number];(tuple[0])=1;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            45,
            1,
        )],
    );
}

#[test]
fn assertion() {
    verify_options(
        r###"declare const tuple:[number];(tuple as readonly [number])[0]=1;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            58,
            1,
        )],
    );
}

#[test]
fn non_null() {
    verify_options(
        r###"declare const tuple:readonly [number]|undefined;tuple![0]=1;const result=tuple![0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            55,
            1,
        )],
    );
}

#[test]
fn destructuring_target() {
    verify_options(
        r###"declare const tuple:readonly [number];[tuple[0]]=[1];const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            45,
            1,
        )],
    );
}

#[test]
fn for_of_target() {
    verify_options(
        r###"declare const tuple:readonly [number];for(tuple[0] of [1]){}const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            48,
            1,
        )],
    );
}

#[test]
fn optional_write() {
    verify_options(
        r###"declare const tuple:readonly [number?];tuple[0]=0;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            45,
            1,
        )],
    );
}

#[test]
fn variadic_negative() {
    verify_options(
        r###"declare const tuple:readonly [string,...number[]];tuple[-1]=0;const result=tuple[0]; result;"###,
        false,
        true,
        r###"string"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [string, ...number[]]' only permits reading."###,
            50,
            9,
        )],
    );
}

#[test]
fn union_negative() {
    verify_options(
        r###"declare const tuple:readonly [string]|[number];tuple[-1]=0;const result=tuple[0]; result;"###,
        false,
        true,
        r###"string | number"###,
        &[
            (
                2542,
                r###"Index signature in type 'readonly [string] | [number]' only permits reading."###,
                47,
                9,
            ),
            (
                2339,
                r###"Property '-1' does not exist on type 'readonly [string] | [number]'."###,
                53,
                2,
            ),
        ],
    );
}

#[test]
fn index_constraint() {
    verify_options(
        r###"function f<K extends 0|1>(tuple:readonly [number,number],key:K){tuple[key]=0;return tuple[0];}const result=f([1,2],0); result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn constraint_dynamic() {
    verify_options(
        r###"function f<T extends readonly [number]>(tuple:T,key:number){tuple[key]=0;return tuple[0];}const result=f([1],0); result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [number]' only permits reading."###,
            60,
            10,
        )],
    );
}

#[test]
fn recursive_alias() {
    verify_options(
        r###"type Tuple=readonly [Tuple];declare const tuple:Tuple;tuple[0]=tuple;const result=tuple[0][0]; result;"###,
        false,
        true,
        r###"Tuple"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            60,
            1,
        )],
    );
}

#[test]
fn annotation_type() {
    verify_options(
        r###"declare const result:readonly [number]; result;"###,
        false,
        true,
        r###"readonly [number]"###,
        &[],
    );
}

#[test]
fn delete_optional() {
    verify_options(
        r###"declare const tuple:readonly [number?];delete tuple[0];const result=tuple[0]; result;"###,
        false,
        true,
        r###"number | undefined"###,
        &[(
            2704,
            r###"The operand of a 'delete' operator cannot be a read-only property."###,
            46,
            8,
        )],
    );
}

#[test]
fn delete_fixed_nonstrict() {
    verify_options(
        r###"declare const tuple:readonly [number];delete tuple[0];const result=tuple[0]; result;"###,
        false,
        false,
        r###"number"###,
        &[(
            2704,
            r###"The operand of a 'delete' operator cannot be a read-only property."###,
            45,
            8,
        )],
    );
}

#[test]
fn delete_const() {
    verify_options(
        r###"const tuple=[1] as const;delete tuple[0];const result=tuple[0]; result;"###,
        false,
        true,
        r###"1"###,
        &[(
            2704,
            r###"The operand of a 'delete' operator cannot be a read-only property."###,
            32,
            8,
        )],
    );
}

#[test]
fn delete_union_key() {
    verify_options(
        r###"declare const tuple:readonly [number,...number[]];declare const key:0|2;delete tuple[key];const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [number, ...number[]]' only permits reading."###,
            79,
            10,
        )],
    );
}

#[test]
fn array_union_dynamic() {
    verify_options(
        r###"declare const tuple:readonly [number]|number[];declare const key:number;tuple[key]=0;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly [number] | number[]' only permits reading."###,
            72,
            10,
        )],
    );
}

#[test]
fn array_union_fixed() {
    verify_options(
        r###"declare const tuple:readonly [number]|number[];tuple[0]=0;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            53,
            1,
        )],
    );
}

#[test]
fn mutable_assertion() {
    verify_options(
        r###"declare const tuple:readonly [number];(tuple as [number])[0]=0;const result=tuple[0]; result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn length_optional() {
    verify_options(
        r###"declare const tuple:readonly [number?];const result=tuple.length; result;"###,
        false,
        true,
        r###"0 | 1"###,
        &[],
    );
}

#[test]
fn method_alias() {
    verify_options(
        r###"type Tuple=readonly [number,number];declare const tuple:Tuple;const result=tuple.slice(); result;"###,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn destructured_return() {
    verify_options(
        r###"function f([first]:readonly [number]){return first;}const result=f([1]); result;"###,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn optional_chain() {
    verify_options(
        r###"declare const tuple:readonly [number]|undefined;const result=tuple?.[0]; result;"###,
        false,
        true,
        r###"number | undefined"###,
        &[],
    );
}
