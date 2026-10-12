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
fn array_annotation() {
    verify_options(
        r###"declare const result:readonly number[]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_reference() {
    verify_options(
        r###"declare const result:ReadonlyArray<number>; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_union_element() {
    verify_options(
        r###"declare const result:ReadonlyArray<string|number>; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly (string | number)[]"###,
        &[],
    );
}

#[test]
fn array_utility() {
    verify_options(
        r###"declare const result:Readonly<number[]>; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_read() {
    verify_options(
        r###"declare const array:readonly number[];const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_unchecked() {
    verify_options(
        r###"declare const array:readonly number[];const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        true,
        true,
        r###"number | undefined"###,
        &[],
    );
}

#[test]
fn array_write() {
    verify_options(
        r###"declare const array:readonly number[];array[0]=1;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            38,
            8,
        )],
    );
}

#[test]
fn array_dynamic_write() {
    verify_options(
        r###"declare const array:ReadonlyArray<number>;declare const key:number;array[key]=1;const result=array[key]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            67,
            10,
        )],
    );
}

#[test]
fn array_alias_write() {
    verify_options(
        r###"type List<T>=ReadonlyArray<T>;declare const array:List<number>;array[0]=1;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'List<number>' only permits reading."###,
            63,
            8,
        )],
    );
}

#[test]
fn array_property_write() {
    verify_options(
        r###"declare const value:{array:readonly number[]};value.array[0]=1;const result=value.array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            46,
            14,
        )],
    );
}

#[test]
fn array_return_write() {
    verify_options(
        r###"declare function array():readonly number[];array()[0]=1;const result=array()[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            43,
            10,
        )],
    );
}

#[test]
fn array_generic_write() {
    verify_options(
        r###"function f<T extends readonly number[]>(array:T){array[0]=1;return array[0];}const result=f([1]); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            49,
            8,
        )],
    );
}

#[test]
fn array_string_index() {
    verify_options(
        r###"declare const array:readonly number[];array["0"]=1;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            38,
            10,
        )],
    );
}

#[test]
fn array_length_write() {
    verify_options(
        r###"declare const array:readonly number[];array.length=1;const result=array.length; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to 'length' because it is a read-only property."###,
            44,
            6,
        )],
    );
}

#[test]
fn array_length_index() {
    verify_options(
        r###"declare const array:readonly number[];array["length"]=1;const result=array.length; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2540,
            r###"Cannot assign to 'length' because it is a read-only property."###,
            44,
            8,
        )],
    );
}

#[test]
fn array_delete() {
    verify_options(
        r###"declare const array:readonly number[];delete array[0];const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            45,
            8,
        )],
    );
}

#[test]
fn array_delete_length() {
    verify_options(
        r###"declare const array:readonly number[];delete array.length;const result=array.length; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2704,
            r###"The operand of a 'delete' operator cannot be a read-only property."###,
            45,
            12,
        )],
    );
}

#[test]
fn array_increment() {
    verify_options(
        r###"declare const array:readonly number[];array[0]++;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            38,
            8,
        )],
    );
}

#[test]
fn array_write_wrong() {
    verify_options(
        r###"declare const array:readonly number[];array[0]="";const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[
            (
                2322,
                r###"Type 'string' is not assignable to type 'number'."###,
                38,
                8,
            ),
            (
                2542,
                r###"Index signature in type 'readonly number[]' only permits reading."###,
                38,
                8,
            ),
        ],
    );
}

#[test]
fn array_spread() {
    verify_options(
        r###"declare const array:readonly number[];const result=[...array]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn array_binding() {
    verify_options(
        r###"declare const array:readonly number[];const [result]=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_rest_binding() {
    verify_options(
        r###"declare const array:readonly number[];const [,...result]=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn array_slice() {
    verify_options(
        r###"declare const array:readonly number[];const result=array.slice(); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn array_map() {
    verify_options(
        r###"declare const array:readonly number[];const result=array.map(x=>x.toString()); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"string[]"###,
        &[],
    );
}

#[test]
fn array_push() {
    verify_options(
        r###"declare const array:readonly number[];array.push(1);const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2339,
            r###"Property 'push' does not exist on type 'readonly number[]'."###,
            44,
            4,
        )],
    );
}

#[test]
fn array_pop() {
    verify_options(
        r###"declare const array:ReadonlyArray<number>;array.pop();const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2339,
            r###"Property 'pop' does not exist on type 'readonly number[]'."###,
            48,
            3,
        )],
    );
}

#[test]
fn array_reverse() {
    verify_options(
        r###"declare const array:readonly number[];array.reverse();const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2339,
            r###"Property 'reverse' does not exist on type 'readonly number[]'."###,
            44,
            7,
        )],
    );
}

#[test]
fn array_assign_mutable() {
    verify_options(
        r###"declare const array:readonly number[];const result:number[]=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[(
            4104,
            r###"The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            44,
            6,
        )],
    );
}

#[test]
fn array_assign_readonly() {
    verify_options(
        r###"declare const array:number[];const result:readonly number[]=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_assign_readonly_wrong() {
    verify_options(
        r###"declare const array:readonly string[];const result:readonly number[]=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[(
            2322,
            r###"Type 'readonly string[]' is not assignable to type 'readonly number[]'.
  Type 'string' is not assignable to type 'number'."###,
            44,
            6,
        )],
    );
}

#[test]
fn array_argument() {
    verify_options(
        r###"declare const array:readonly number[];function f(x:number[]){return x[0];}const result=f(array); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2345,
            r###"Argument of type 'readonly number[]' is not assignable to parameter of type 'number[]'.
  The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            89,
            5,
        )],
    );
}

#[test]
fn array_return() {
    verify_options(
        r###"declare const array:readonly number[];function f():number[]{return array;}const result=f(); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[(
            4104,
            r###"The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            60,
            6,
        )],
    );
}

#[test]
fn array_assertion() {
    verify_options(
        r###"declare const array:readonly number[];const result=array as number[]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn array_mutable_assertion() {
    verify_options(
        r###"declare const array:number[];const result=array as readonly number[]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_assign_alias() {
    verify_options(
        r###"type List=number[];declare const array:readonly number[];const result:List=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"List"###,
        &[(
            4104,
            r###"The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'List'."###,
            63,
            6,
        )],
    );
}

#[test]
fn array_assign_union() {
    verify_options(
        r###"declare const array:readonly number[];const result:number[]|string=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"string | number[]"###,
        &[(
            2322,
            r###"Type 'readonly number[]' is not assignable to type 'string | number[]'.
  The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            44,
            6,
        )],
    );
}

#[test]
fn array_assign_readonly_union() {
    verify_options(
        r###"declare const array:readonly number[];const result:number[]|ReadonlyArray<number>=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_object() {
    verify_options(
        r###"declare const array:readonly number[];const result:object=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"object"###,
        &[],
    );
}

#[test]
fn array_nested_assignment() {
    verify_options(
        r###"declare const value:{items:readonly number[]};const result:{items:number[]}=value; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"{
    items: number[];
}"###,
        &[(
            2322,
            r###"Type '{ items: readonly number[]; }' is not assignable to type '{ items: number[]; }'.
  Types of property 'items' are incompatible.
    The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            52,
            6,
        )],
    );
}

#[test]
fn array_union_receiver() {
    verify_options(
        r###"declare const array:readonly number[]|number[];array[0]=1;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[] | number[]' only permits reading."###,
            47,
            8,
        )],
    );
}

#[test]
fn array_tuple_union() {
    verify_options(
        r###"declare const array:readonly number[]|[number];array[0]=1;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
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
fn array_keyof() {
    verify_options(
        r###"type Keys=keyof readonly number[];let result:Keys="length"; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###""length""###,
        &[],
    );
}

#[test]
fn array_typeof() {
    verify_options(
        r###"declare const array:readonly number[];declare const result:typeof array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_indexed_type() {
    verify_options(
        r###"type Value=(readonly number[])[number];declare const result:Value; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_inference() {
    verify_options(
        r###"function first<T>(array:ReadonlyArray<T>):T{return array[0];}declare const array:readonly number[];const result=first(array); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn const_tuple_assign() {
    verify_options(
        r###"const tuple=[1,2] as const;const result:number[]=tuple; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[(
            4104,
            r###"The type 'readonly [1, 2]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            33,
            6,
        )],
    );
}

#[test]
fn const_tuple_alias_type() {
    verify_options(
        r###"const tuple=[1,2] as const;const result=tuple; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly [1, 2]"###,
        &[],
    );
}

#[test]
fn readonly_tuple_assign() {
    verify_options(
        r###"declare const tuple:readonly [number];const result:[number]=tuple; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"[number]"###,
        &[(
            4104,
            r###"The type 'readonly [number]' is 'readonly' and cannot be assigned to the mutable type '[number]'."###,
            44,
            6,
        )],
    );
}

#[test]
fn readonly_tuple_argument() {
    verify_options(
        r###"declare const tuple:readonly [number];function f(x:[number]){return x[0];}const result=f(tuple); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2345,
            r###"Argument of type 'readonly [number]' is not assignable to parameter of type '[number]'.
  The type 'readonly [number]' is 'readonly' and cannot be assigned to the mutable type '[number]'."###,
            89,
            5,
        )],
    );
}

#[test]
fn readonly_tuple_to_array() {
    verify_options(
        r###"declare const tuple:readonly [number];const result:number[]=tuple; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[(
            4104,
            r###"The type 'readonly [number]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            44,
            6,
        )],
    );
}

#[test]
fn mutable_tuple_to_readonly_array() {
    verify_options(
        r###"declare const tuple:[number];const result:readonly number[]=tuple; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn readonly_tuple_to_readonly_array() {
    verify_options(
        r###"declare const tuple:readonly [number];const result:readonly number[]=tuple; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn readonly_tuple_push() {
    verify_options(
        r###"declare const tuple:readonly [number];tuple.push(1);const result=tuple[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2339,
            r###"Property 'push' does not exist on type 'readonly [number]'."###,
            44,
            4,
        )],
    );
}

#[test]
fn const_tuple_nonnull_write() {
    verify_options(
        r###"const tuple=[1] as const;tuple![0]=1;const result=tuple![0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"1"###,
        &[(
            2540,
            r###"Cannot assign to '0' because it is a read-only property."###,
            32,
            1,
        )],
    );
}

#[test]
fn array_bad_key() {
    verify_options(
        r###"type Keys=keyof readonly number[];const result:Keys="push"; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"keyof readonly number[]"###,
        &[(
            2322,
            r###"Type '"push"' is not assignable to type 'keyof readonly number[]'."###,
            40,
            6,
        )],
    );
}

#[test]
fn array_numeric_key() {
    verify_options(
        r###"type Keys=keyof readonly number[];let result:Keys=0; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_string_numeric_key() {
    verify_options(
        r###"type Keys=keyof readonly number[];const result:Keys="0"; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"keyof readonly number[]"###,
        &[(
            2322,
            r###"Type '"0"' is not assignable to type 'keyof readonly number[]'."###,
            40,
            6,
        )],
    );
}

#[test]
fn array_concat_readonly() {
    verify_options(
        r###"declare const array:readonly number[];const result=array.concat(array); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn mutable_concat_readonly() {
    verify_options(
        r###"declare const array:number[];declare const other:readonly number[];const result=array.concat(other); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn array_callback_write() {
    verify_options(
        r###"declare const array:readonly number[];array.forEach((value,index,items)=>{items[0]=value;});const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            74,
            8,
        )],
    );
}

#[test]
fn array_callback_third_type() {
    verify_options(
        r###"declare const array:readonly number[];const result=array.map((value,index,items)=>items); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"(readonly number[])[]"###,
        &[],
    );
}

#[test]
fn array_generic_alias() {
    verify_options(
        r###"type List<T>=readonly T[];function first<T>(xs:List<T>):T{return xs[0];}declare const xs:List<string>;const result=first(xs); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"string"###,
        &[],
    );
}

#[test]
fn array_union_source_assignment() {
    verify_options(
        r###"declare const array:readonly number[]|number[];const result:number[]=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[(
            2322,
            r###"Type 'number[] | readonly number[]' is not assignable to type 'number[]'.
  The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            53,
            6,
        )],
    );
}

#[test]
fn array_union_source_readonly() {
    verify_options(
        r###"declare const array:readonly number[]|number[];const result:readonly number[]=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_optional_target() {
    verify_options(
        r###"declare const array:readonly number[];const result:number[]|undefined=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[] | undefined"###,
        &[(
            4104,
            r###"The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            44,
            6,
        )],
    );
}

#[test]
fn array_empty_object() {
    verify_options(
        r###"declare const array:readonly number[];const result:{}=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"{}"###,
        &[],
    );
}

#[test]
fn array_readonly_utility_alias() {
    verify_options(
        r###"type List=number[];declare const result:Readonly<List>; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_utility_write() {
    verify_options(
        r###"declare const array:Readonly<number[]>;array[0]=0;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2542,
            r###"Index signature in type 'readonly number[]' only permits reading."###,
            39,
            8,
        )],
    );
}

#[test]
fn array_shadowed_readonly_utility() {
    verify_options(
        r###"export {};type Readonly<T>={value:T};declare const value:Readonly<number[]>;value.value[0]=0;const result=value.value[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_rest_parameter() {
    verify_options(
        r###"function f(...values:readonly number[]){return values[0];}const result=f(1,2); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_rest_spread() {
    verify_options(
        r###"declare const values:readonly number[];function f(...xs:number[]){return xs[0];}const result=f(...values); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_for_of() {
    verify_options(
        r###"declare const values:readonly number[];let result=0;for(const value of values){result=value;} result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_spread_es5() {
    verify_options(
        r###"declare const values:readonly number[];const result=[...values]; result;"###,
        ScriptTarget::ES5,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn array_for_of_es5() {
    verify_options(
        r###"declare const values:readonly number[];let result=0;for(const value of values){result=value;} result;"###,
        ScriptTarget::ES5,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_inferred_return() {
    verify_options(
        r###"declare const values:readonly number[];function f(){return values;}const result=f(); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly number[]"###,
        &[],
    );
}

#[test]
fn array_nested_destructuring() {
    verify_options(
        r###"declare const values:readonly (readonly number[])[];const [[result]]=values; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_assertion_return() {
    verify_options(
        r###"function f(){return [1,2] as const;}const result=f(); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly [1, 2]"###,
        &[],
    );
}

#[test]
fn const_tuple_referenced_array() {
    verify_options(
        r###"declare const mutable:number[];const result=[mutable] as const; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly [number[]]"###,
        &[],
    );
}

#[test]
fn const_object_referenced_array() {
    verify_options(
        r###"declare const mutable:number[];const result={mutable} as const; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"{
    readonly mutable: number[];
}"###,
        &[],
    );
}

#[test]
fn const_array_spread() {
    verify_options(
        r###"const tuple=[1,2] as const;const result=[...tuple]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"(1 | 2)[]"###,
        &[],
    );
}

#[test]
fn const_tuple_nested() {
    verify_options(
        r###"const result=[[1]] as const; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly [readonly [1]]"###,
        &[],
    );
}

#[test]
fn array_index_length_read() {
    verify_options(
        r###"declare const array:readonly number[];const result=array["length"]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[],
    );
}

#[test]
fn array_index_slice() {
    verify_options(
        r###"declare const array:readonly number[];const result=array["slice"](); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn array_index_push() {
    verify_options(
        r###"declare const array:readonly number[];array["push"](1);const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            7015,
            r###"Element implicitly has an 'any' type because index expression is not of type 'number'."###,
            44,
            6,
        )],
    );
}

#[test]
fn array_missing_member() {
    verify_options(
        r###"declare const array:readonly number[];array.missing;const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[(
            2339,
            r###"Property 'missing' does not exist on type 'readonly number[]'."###,
            44,
            7,
        )],
    );
}

#[test]
fn array_find_last_es2015() {
    verify_options(
        r###"declare const array:readonly number[];array.findLast(x=>true);const result=array[0]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number"###,
        &[
            (
                2339,
                r###"Property 'findLast' does not exist on type 'readonly number[]'."###,
                44,
                8,
            ),
            (
                7006,
                r###"Parameter 'x' implicitly has an 'any' type."###,
                53,
                1,
            ),
        ],
    );
}

#[test]
fn readonly_tuple_infer() {
    verify_options(
        r###"declare function first<T>(values:readonly T[]):T;const result=first([1,2] as const); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"1 | 2"###,
        &[],
    );
}

#[test]
fn readonly_array_intersection() {
    verify_options(
        r###"declare const array:readonly number[];const result:number[] & {readonly length:number}=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[] & {
    readonly length: number;
}"###,
        &[(
            2322,
            r###"Type 'readonly number[]' is not assignable to type 'number[] & { readonly length: number; }'.
  The type 'readonly number[]' is 'readonly' and cannot be assigned to the mutable type 'number[]'."###,
            44,
            6,
        )],
    );
}

#[test]
fn readonly_array_object_method() {
    verify_options(
        r###"declare const array:readonly number[];const result:{push(...items:number[]):number}=array; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"{
    push(...items: number[]): number;
}"###,
        &[(
            2741,
            r###"Property 'push' is missing in type 'readonly number[]' but required in type '{ push(...items: number[]): number; }'."###,
            44,
            6,
        )],
    );
}

#[test]
fn const_satisfies_mutable() {
    verify_options(
        r###"const result=[1,2] as const satisfies number[]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"[1, 2]"###,
        &[],
    );
}

#[test]
fn const_satisfies_readonly() {
    verify_options(
        r###"const result=[1,2] as const satisfies readonly number[]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly [1, 2]"###,
        &[],
    );
}

#[test]
fn const_context_mutable() {
    verify_options(
        r###"const result:number[]=[1,2] as const; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn readonly_context_spread() {
    verify_options(
        r###"declare const values:readonly ["x","y"];const result:[...string[],"!"]=[...values,"!"]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"[...string[], "!"]"###,
        &[],
    );
}

#[test]
fn readonly_flat_map() {
    verify_options(
        r###"declare const array:readonly number[];const result=array.flatMap(():readonly number[]=>[]); result;"###,
        ScriptTarget::ES2019,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn readonly_assertion_slice() {
    verify_options(
        r###"declare const values:[number,string]|[number,string,string];const result=values.slice(1) as readonly string[]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly string[]"###,
        &[],
    );
}

#[test]
fn const_union_context() {
    verify_options(
        r###"const result=[1,2] as const satisfies number[]|string; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"[1, 2]"###,
        &[],
    );
}

#[test]
fn const_nested_context() {
    verify_options(
        r###"const result=[[1]] as const satisfies number[][]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"[[1]]"###,
        &[],
    );
}

#[test]
fn const_mixed_nested_context() {
    verify_options(
        r###"const result=[[1]] as const satisfies readonly number[][]; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"readonly [[1]]"###,
        &[],
    );
}

#[test]
fn const_return_context() {
    verify_options(
        r###"function f():number[]{return [1,2] as const;}const result=f(); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"number[]"###,
        &[],
    );
}

#[test]
fn tuple_noncanonical_key() {
    verify_options(
        r###"type Keys=keyof readonly [number];const result:Keys="00"; result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"keyof readonly [number]"###,
        &[(
            2322,
            r###"Type '"00"' is not assignable to type 'keyof readonly [number]'."###,
            40,
            6,
        )],
    );
}

#[test]
fn const_generic_context() {
    verify_options(
        r###"declare function f<T extends number[]>(x:T):T;const result=f([1,2] as const); result;"###,
        ScriptTarget::ES2015,
        false,
        true,
        r###"[1, 2]"###,
        &[],
    );
}
