// Exact types and diagnostics checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(source: &str, unchecked: bool, expected_type: &str, expected: &[(u32, &str, u32, u32)]) {
    verify_options(source, unchecked, true, expected_type, expected);
}

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
fn array_interface() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;const result=values[0]; result;", false, "string", &[
    ]);
}

#[test]
fn array_interface_numeric_string() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;const result=values[\"0\"]; result;", false, "string", &[
    ]);
}

#[test]
fn array_interface_dynamic() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;declare const index:number;const result=values[index]; result;", false, "string", &[
    ]);
}

#[test]
fn array_interface_inheritance() {
    verify("interface List<T> extends Array<T>{} interface Nested<T> extends List<T[]>{} declare const values:Nested<string>;const result=values[0]; result;", false, "string[]", &[
    ]);
}

#[test]
fn array_like_interface() {
    verify("interface List<T> extends ArrayLike<T>{} declare const values:List<string>;const result=values[0]; result;", false, "string", &[
    ]);
}

#[test]
fn readonly_array_interface() {
    verify("interface List<T> extends ReadonlyArray<T>{} declare const values:List<string>;const result=values[0]; result;", false, "string", &[
    ]);
}

#[test]
fn array_alias_heritage() {
    verify("type Base<T>=T[];interface List<T> extends Base<T>{} declare const values:List<string>;const result=values[0]; result;", false, "string", &[
    ]);
}

#[test]
fn array_class() {
    verify("class List<T> extends Array<T>{} declare const values:List<string>;const result=values[0]; result;", false, "string", &[
    ]);
}

#[test]
fn array_interface_missing() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;const result=values[\"missing\"]; result;", false, "any", &[
        (7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", 91, 9),
    ]);
}

#[test]
fn array_interface_reassignment() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;let result=values[0];result=0; result;", false, "string", &[
        (2322, "Type 'number' is not assignable to type 'string'.", 92, 6),
    ]);
}

#[test]
fn inherited_numeric_and_string() {
    verify("interface Numeric<T>{[key:number]:T}interface Values extends Numeric<string>{[key:string]:string|number}declare const values:Values;const result=values[0]; result;", false, "string", &[
    ]);
}

#[test]
fn inherited_index_union() {
    verify("interface Values{[key:number]:string;[key:string]:string|number}declare const values:Values;declare const key:0|\"x\";const result=values[key]; result;", false, "string | number", &[
    ]);
}

#[test]
fn static_property() {
    verify("class C{static value=1;}declare const value:C;const result=value[\"value\"]; result;", false, "any", &[
        (2576, "Property 'value' does not exist on type 'C'. Did you mean to access the static member 'C[\"value\"]' instead?", 59, 14),
    ]);
}

#[test]
fn static_inherited() {
    verify("class Base{static value=1;}class C extends Base{}declare const value:C;const result=value[\"value\"]; result;", false, "any", &[
        (2576, "Property 'value' does not exist on type 'C'. Did you mean to access the static member 'C[\"value\"]' instead?", 84, 14),
    ]);
}

#[test]
fn static_method() {
    verify("class C{static method(){}}declare const value:C;const result=value[\"method\"]; result;", false, "any", &[
        (2576, "Property 'method' does not exist on type 'C'. Did you mean to access the static member 'C[\"method\"]' instead?", 61, 15),
    ]);
}

#[test]
fn static_quote() {
    verify("class C{static \"\\\"\"(){}}declare const value:C;const result=value[\"\\\"\"]; result;", false, "any", &[
        (2576, "Property '\"' does not exist on type 'C'. Did you mean to access the static member 'C[\"\\\"\"]' instead?", 59, 11),
    ]);
}

#[test]
fn static_valid() {
    verify(
        "class C{static value=1;}const result=C[\"value\"]; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn static_instance_override() {
    verify("class Base{static value=1;}class C extends Base{value=\"yes\";}declare const value:C;const result=value[\"value\"]; result;", false, "string", &[
    ]);
}

#[test]
fn get_method() {
    verify("declare const value:{get(key:string):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get(key: string): number; }' has no index signature. Did you mean to call 'value.get'?", 58, 14),
    ]);
}

#[test]
fn get_property() {
    verify("const value={get:(key:string)=>1};const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get: (key: string) => number; }' has no index signature. Did you mean to call 'value.get'?", 47, 14),
    ]);
}

#[test]
fn get_wrong_key() {
    verify("declare const value:{get(key:number):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(key: number): number; }'.\n  Property 'hello' does not exist on type '{ get(key: number): number; }'.", 58, 14),
    ]);
}

#[test]
fn get_optional_parameter() {
    verify("declare const value:{get(key?:string):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(key?: string | undefined): number; }'.\n  Property 'hello' does not exist on type '{ get(key?: string | undefined): number; }'.", 59, 14),
    ]);
}

#[test]
fn get_optional_method() {
    verify("declare const value:{get?:(key:string)=>number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get?: ((key: string) => number) | undefined; }'.\n  Property 'hello' does not exist on type '{ get?: ((key: string) => number) | undefined; }'.", 61, 14),
    ]);
}

#[test]
fn get_non_function() {
    verify("declare const value:{get:number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get: number; }'.\n  Property 'hello' does not exist on type '{ get: number; }'.", 46, 14),
    ]);
}

#[test]
fn get_no_parameters() {
    verify("declare const value:{get():number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(): number; }'.\n  Property 'hello' does not exist on type '{ get(): number; }'.", 48, 14),
    ]);
}

#[test]
fn get_overloads() {
    verify("declare const value:{get(key:string):number;get(key:number):string};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(key: string): number; get(key: number): string; }'.\n  Property 'hello' does not exist on type '{ get(key: string): number; get(key: number): string; }'.", 81, 14),
    ]);
}

#[test]
fn get_nested() {
    verify("declare const value:{nested:{get(key:string):number}};const result=value.nested[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get(key: string): number; }' has no index signature. Did you mean to call 'value.nested.get'?", 67, 21),
    ]);
}

#[test]
fn get_nested_element() {
    verify("declare const value:{nested:{get(key:string):number}};const result=value[\"nested\"][\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get(key: string): number; }' has no index signature. Did you mean to call 'value.nested.get'?", 67, 24),
    ]);
}

#[test]
fn get_parenthesized() {
    verify("declare const value:{get(key:string):number};const result=(value)[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get(key: string): number; }' has no index signature. Did you mean to call 'get'?", 58, 16),
    ]);
}

#[test]
fn get_call_receiver() {
    verify("declare function value():{get(key:string):number};const result=value()[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get(key: string): number; }' has no index signature. Did you mean to call 'get'?", 63, 16),
    ]);
}

#[test]
fn get_fresh_parenthesized() {
    verify(
        "const result=({get:(key:string)=>1})[\"hello\"]; result;",
        false,
        "undefined",
        &[(
            2339,
            "Property 'hello' does not exist on type '{ get: (key: string) => number; }'.",
            13,
            32,
        )],
    );
}

#[test]
fn get_fresh_nested() {
    verify(
        "const result=({nested:{get:(key:string)=>1}}).nested[\"hello\"]; result;",
        false,
        "undefined",
        &[(
            2339,
            "Property 'hello' does not exist on type '{ get: (key: string) => number; }'.",
            13,
            48,
        )],
    );
}

#[test]
fn get_known_property() {
    verify("declare const value:{get(key:string):number;hello:string};const result=value[\"hello\"]; result;", false, "string", &[
    ]);
}

#[test]
fn get_index_signature() {
    verify("declare const value:{get(key:string):number;[key:string]:any};const result=value[\"hello\"]; result;", false, "any", &[
    ]);
}

#[test]
fn set_assignment() {
    verify("declare const value:{set(key:string,value:number):void};const result=value[\"hello\"]=1; result;", false, "1", &[
        (7052, "Element implicitly has an 'any' type because type '{ set(key: string, value: number): void; }' has no index signature. Did you mean to call 'value.set'?", 69, 14),
    ]);
}

#[test]
fn set_compound() {
    verify("declare const value:{set(key:string,value:number):void};const result=value[\"hello\"]+=1; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ set(key: string, value: number): void; }' has no index signature. Did you mean to call 'value.set'?", 69, 14),
    ]);
}

#[test]
fn set_postfix() {
    verify("declare const value:{set(key:string,value:number):void};const result=value[\"hello\"]++; result;", false, "number", &[
        (7052, "Element implicitly has an 'any' type because type '{ set(key: string, value: number): void; }' has no index signature. Did you mean to call 'value.set'?", 69, 14),
    ]);
}

#[test]
fn set_parenthesized() {
    verify("declare const value:{set(key:string,value:number):void};const result=(value[\"hello\"])=1; result;", false, "1", &[
        (7052, "Element implicitly has an 'any' type because type '{ set(key: string, value: number): void; }' has no index signature. Did you mean to call 'value.set'?", 70, 14),
    ]);
}

#[test]
fn set_fresh() {
    verify("const result=({set:(key:string,value:number)=>{}})[\"hello\"]=1; result;", false, "1", &[
        (7052, "Element implicitly has an 'any' type because type '{ set: (key: string, value: number) => void; }' has no index signature. Did you mean to call 'set'?", 13, 46),
    ]);
}

#[test]
fn set_fresh_nested() {
    verify("const result=({nested:{set:(key:string,value:number)=>{}}}).nested[\"hello\"]=1; result;", false, "1", &[
        (7052, "Element implicitly has an 'any' type because type '{ set: (key: string, value: number) => void; }' has no index signature. Did you mean to call 'set'?", 13, 62),
    ]);
}

#[test]
fn set_no_get_on_read() {
    verify("declare const value:{set(key:string,value:number):void};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ set(key: string, value: number): void; }'.\n  Property 'hello' does not exist on type '{ set(key: string, value: number): void; }'.", 69, 14),
    ]);
}

#[test]
fn get_no_set_on_write() {
    verify("declare const value:{get(key:string):number};const result=value[\"hello\"]=1; result;", false, "1", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(key: string): number; }'.\n  Property 'hello' does not exist on type '{ get(key: string): number; }'.", 58, 14),
    ]);
}

#[test]
fn get_generic_interface() {
    verify("interface Store<T>{get(key:string):T}declare const value:Store<number>;const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type 'Store<number>' has no index signature. Did you mean to call 'value.get'?", 84, 14),
    ]);
}

#[test]
fn get_callable_property() {
    verify("interface Getter{(key:string):number;label:string}declare const value:{get:Getter};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get: Getter; }'.\n  Property 'hello' does not exist on type '{ get: Getter; }'.", 96, 14),
    ]);
}

#[test]
fn get_rest_array() {
    verify("declare const value:{get(...keys:string[]):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(...keys: string[]): number; }'.\n  Property 'hello' does not exist on type '{ get(...keys: string[]): number; }'.", 64, 14),
    ]);
}

#[test]
fn get_rest_tuple() {
    verify("declare const value:{get(...keys:[string]):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get(keys_0: string): number; }' has no index signature. Did you mean to call 'value.get'?", 64, 14),
    ]);
}

#[test]
fn get_void_key() {
    verify("declare const value:{get(key:string|void):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(key: string | void): number; }'.\n  Property 'hello' does not exist on type '{ get(key: string | void): number; }'.", 63, 14),
    ]);
}

#[test]
fn array_interface_unchecked() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;const result=values[0]; result;", true, "string | undefined", &[
    ]);
}

#[test]
fn array_interface_dynamic_unchecked() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;declare const index:number;const result=values[index]; result;", true, "string | undefined", &[
    ]);
}

#[test]
fn array_interface_numeric_string_unchecked() {
    verify("interface List<T> extends Array<T>{} declare const values:List<string>;const result=values[\"0\"]; result;", true, "string | undefined", &[
    ]);
}

#[test]
fn fresh_missing_string() {
    verify(
        "const result = {}[\"missing\"]; result;",
        false,
        "undefined",
        &[(
            2339,
            "Property 'missing' does not exist on type '{}'.",
            15,
            13,
        )],
    );
}

#[test]
fn fresh_missing_number() {
    verify(
        "const result = {}[3]; result;",
        false,
        "undefined",
        &[(2339, "Property '3' does not exist on type '{}'.", 15, 5)],
    );
}

#[test]
fn fresh_misspelled() {
    verify(
        "const result={present:1}[\"presnt\"]; result;",
        false,
        "undefined",
        &[(
            2339,
            "Property 'presnt' does not exist on type '{ present: number; }'.",
            13,
            21,
        )],
    );
}

#[test]
fn fresh_regular_property() {
    verify(
        "const result={present:1 as const}[\"missing\"]; result;",
        false,
        "undefined",
        &[(
            2339,
            "Property 'missing' does not exist on type '{ present: 1; }'.",
            13,
            31,
        )],
    );
}

#[test]
fn unique_missing() {
    verify("declare const key:unique symbol;const object={present:1};const result=object[key]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'unique symbol' can't be used to index type '{ present: number; }'.\n  Property '[key]' does not exist on type '{ present: number; }'.", 70, 11),
    ]);
}

#[test]
fn unique_present() {
    verify("declare const key:unique symbol;declare const object:{[key]:string};const result=object[key]; result;", false, "string", &[
    ]);
}

#[test]
fn enum_number_missing() {
    verify("enum E{A,B}declare const key:E;const object={present:1};const result=object[key]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'E' can't be used to index type '{ present: number; }'.\n  Property '[E.A]' does not exist on type '{ present: number; }'.", 69, 11),
    ]);
}

#[test]
fn enum_number_present() {
    verify("enum E{A,B}declare const key:E;declare const object:{0:string;1:number};const result=object[key]; result;", false, "string | number", &[
    ]);
}

#[test]
fn enum_string_partial() {
    verify("enum E{A=\"a\",B=\"b\"}declare const key:E;const object={a:1};const result=object[key]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'E' can't be used to index type '{ a: number; }'.\n  Property '[E.B]' does not exist on type '{ a: number; }'.", 71, 11),
    ]);
}

#[test]
fn enum_string_present() {
    verify("enum E{A=\"a\",B=\"b\"}declare const key:E;declare const object:{a:string;b:number};const result=object[key]; result;", false, "string | number", &[
    ]);
}

#[test]
fn enum_member_missing() {
    verify("enum E{A=\"a\",B=\"b\"}declare const key:E.B;const object={a:1};const result=object[key]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'E.B' can't be used to index type '{ a: number; }'.\n  Property '[E.B]' does not exist on type '{ a: number; }'.", 73, 11),
    ]);
}

#[test]
fn enum_array_index() {
    verify(
        "enum E{A,B}declare const key:E;const object=[\"a\"];const result=object[key]; result;",
        false,
        "string",
        &[],
    );
}

#[test]
fn invalid_interface_key() {
    verify("interface Key{method():void}declare const key:Key;declare const object:{[key:string]:number};const result=object[key]; result;", false, "any", &[
        (2538, "Type 'Key' cannot be used as an index type.", 113, 3),
    ]);
}

#[test]
fn invalid_class_key() {
    verify("class Key{method(){}}declare const key:Key;declare const object:{[key:string]:number};const result=object[key]; result;", false, "any", &[
        (2538, "Type 'Key' cannot be used as an index type.", 106, 3),
    ]);
}

#[test]
fn valid_primitive_alias_key() {
    verify("type Key=string;declare const key:Key;declare const object:{[key:string]:number};const result=object[key]; result;", false, "number", &[
    ]);
}

#[test]
fn valid_generic_key() {
    verify("function read<K extends string>(key:K,object:{[key:string]:number}){return object[key]}const result=read(\"x\",{}); result;", false, "number", &[
    ]);
}

#[test]
fn get_undefined_key() {
    verify("declare const value:{get(key:string|undefined):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get(key: string | undefined): number; }' has no index signature. Did you mean to call 'value.get'?", 68, 14),
    ]);
}

#[test]
fn get_generic_method() {
    verify("declare const value:{get<K extends string>(key:K):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get<K extends string>(key: K): number; }'.\n  Property 'hello' does not exist on type '{ get<K extends string>(key: K): number; }'.", 71, 14),
    ]);
}

#[test]
fn get_optional_rest_tuple() {
    verify("declare const value:{get(...keys:[string?]):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ get(keys_0?: string | undefined): number; }'.\n  Property 'hello' does not exist on type '{ get(keys_0?: string | undefined): number; }'.", 65, 14),
    ]);
}

#[test]
fn get_function_alias() {
    verify("type Getter=(key:string)=>number;declare const value:{get:Getter};const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get: Getter; }' has no index signature. Did you mean to call 'value.get'?", 79, 14),
    ]);
}

#[test]
fn get_method_spelling() {
    verify("declare const value:{get(key:string):number;hello:string};const result=value[\"helllo\"]; result;", false, "any", &[
        (2551, "Property 'helllo' does not exist on type '{ get(key: string): number; hello: string; }'. Did you mean 'hello'?", 77, 8),
    ]);
}

#[test]
fn set_tuple_rest() {
    verify("declare const value:{set(...keys:[string,number]):void};const result=value[\"hello\"]=1; result;", false, "1", &[
        (7052, "Element implicitly has an 'any' type because type '{ set(keys_0: string, keys_1: number): void; }' has no index signature. Did you mean to call 'value.set'?", 69, 14),
    ]);
}

#[test]
fn add_assign_number() {
    verify(
        "let value=2;const result=value+=3; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn add_assign_string() {
    verify(
        "let value=\"a\";const result=value+=3; result;",
        false,
        "string",
        &[],
    );
}

#[test]
fn add_assign_any() {
    verify(
        "declare let value:any;const result=value+=3; result;",
        false,
        "any",
        &[],
    );
}

#[test]
fn add_assign_unknown() {
    verify(
        "declare let value:unknown;const result=value+=3; result;",
        false,
        "any",
        &[(18046, "'value' is of type 'unknown'.", 39, 5)],
    );
}

#[test]
fn fresh_overwrite_property() {
    verify("const result=({nested:{get:(key:string)=>1},...{nested:{present:1}}}).nested[\"hello\"]; result;", false, "undefined", &[
        (2339, "Property 'hello' does not exist on type '{ present: number; }'.", 13, 72),
        (2783, "'nested' is specified more than once, so this usage will be overwritten.", 15, 28),
    ]);
}

#[test]
fn fresh_dynamic_empty() {
    verify(
        "declare const key:string;const result={}[key]; result;",
        false,
        "undefined",
        &[],
    );
}

#[test]
fn fresh_dynamic_property() {
    verify(
        "declare const key:string;const result=({present:1})[key]; result;",
        false,
        "number | undefined",
        &[],
    );
}

#[test]
fn fresh_dynamic_union() {
    verify(
        "declare const key:number;const result=({present:1,other:\"text\"})[key]; result;",
        false,
        "string | number | undefined",
        &[],
    );
}

#[test]
fn fresh_known_property() {
    verify(
        "const result=({present:1})[\"present\"]; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn fresh_unique_missing() {
    verify("declare const key:unique symbol;const result=({present:1})[key]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'unique symbol' can't be used to index type '{ present: number; }'.\n  Property '[key]' does not exist on type '{ present: number; }'.", 45, 18),
    ]);
}

#[test]
fn fresh_get_dynamic() {
    verify(
        "declare const key:string;const result=({get:(key:string)=>1})[key]; result;",
        false,
        "((key: string) => number) | undefined",
        &[],
    );
}

#[test]
fn fresh_spread_variable() {
    verify("declare const value:{nested:{present:number}};const result=({nested:{get:(key:string)=>1},...value}).nested[\"hello\"]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type '\"hello\"' can't be used to index type '{ present: number; }'.\n  Property 'hello' does not exist on type '{ present: number; }'.", 59, 57),
        (2783, "'nested' is specified more than once, so this usage will be overwritten.", 61, 28),
    ]);
}

#[test]
fn get_generic_any() {
    verify("declare const value:{get<K extends string>(key:K):number};declare const key:any;const result=value[key]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get<K extends string>(key: K): number; }' has no index signature. Did you mean to call 'value.get'?", 93, 10),
    ]);
}

#[test]
fn get_generic_concrete_key() {
    verify("declare const value:{get<T>(key:string):T};const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get<T>(key: string): T; }' has no index signature. Did you mean to call 'value.get'?", 56, 14),
    ]);
}

#[test]
fn get_generic_union_key() {
    verify("declare const value:{get<K>(key:K|string):number};const result=value[\"hello\"]; result;", false, "any", &[
        (7052, "Element implicitly has an 'any' type because type '{ get<K>(key: string | K): number; }' has no index signature. Did you mean to call 'value.get'?", 63, 14),
    ]);
}

#[test]
fn enum_single_missing() {
    verify("enum E{Only=1}declare const key:E;const object={present:1};const result=object[key]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'E' can't be used to index type '{ present: number; }'.\n  Property '[E]' does not exist on type '{ present: number; }'.", 72, 11),
    ]);
}

#[test]
fn enum_single_member_missing() {
    verify("enum E{A,B}const object={present:1};const result=object[E.B]; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'E.B' can't be used to index type '{ present: number; }'.\n  Property '[E.B]' does not exist on type '{ present: number; }'.", 49, 11),
    ]);
}

#[test]
fn unique_array_index() {
    verify("declare const key:unique symbol;const result=[1][key]; result;", false, "any", &[
        (7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", 49, 3),
    ]);
}

#[test]
fn array_inherited_any_index() {
    verify("interface List<T> extends Array<T>{}declare const value:List<string>;declare const key:any;const result=value[key]; result;", false, "string", &[
    ]);
}

#[test]
fn array_inherited_property() {
    verify("interface List<T> extends Array<T>{}declare const value:List<string>;const result=value[\"length\"]; result;", false, "number", &[
    ]);
}

#[test]
fn array_inherited_write_unchecked() {
    verify("interface List<T> extends Array<T>{}declare const value:List<string>;const result=value[0]=\"hello\"; result;", true, "\"hello\"", &[
    ]);
}

#[test]
fn fresh_dynamic_write() {
    verify("declare const key:string;const result=({present:1})[key]=2; result;", false, "2", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'string' can't be used to index type '{ present: number; }'.\n  No index signature with a parameter of type 'string' was found on type '{ present: number; }'.", 38, 18),
    ]);
}

#[test]
fn fresh_dynamic_compound() {
    verify("declare const key:string;const result=({present:1})[key]+=2; result;", false, "any", &[
        (7053, "Element implicitly has an 'any' type because expression of type 'string' can't be used to index type '{ present: number; }'.\n  No index signature with a parameter of type 'string' was found on type '{ present: number; }'.", 38, 18),
    ]);
}

#[test]
fn fresh_enum_missing() {
    verify(
        "enum E{A,B}declare const key:E;const result=({present:1})[key]; result;",
        false,
        "undefined",
        &[
            (
                2339,
                "Property '0' does not exist on type '{ present: number; }'.",
                44,
                18,
            ),
            (
                2339,
                "Property '1' does not exist on type '{ present: number; }'.",
                44,
                18,
            ),
        ],
    );
}

#[test]
fn fresh_enum_string_missing() {
    verify(
        "enum E{A=\"a\",B=\"b\"}declare const key:E;const result=({a:1})[key]; result;",
        false,
        "number | undefined",
        &[(
            2339,
            "Property 'b' does not exist on type '{ a: number; }'.",
            52,
            12,
        )],
    );
}

#[test]
fn spread_optional() {
    verify(
        "declare const value:{key?:number};const result={key:1,...value}.key; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn spread_any() {
    verify(
        "declare const value:any;const result={key:1,...value}.key; result;",
        false,
        "any",
        &[],
    );
}

#[test]
fn spread_twice() {
    verify(
        "const result={key:1,...{key:2},...{key:3}}.key; result;",
        false,
        "number",
        &[(
            2783,
            "'key' is specified more than once, so this usage will be overwritten.",
            14,
            5,
        )],
    );
}

#[test]
fn spread_undefined_required() {
    verify(
        "declare const value:{key:undefined};const result={key:1,...value}.key; result;",
        false,
        "undefined",
        &[(
            2783,
            "'key' is specified more than once, so this usage will be overwritten.",
            50,
            5,
        )],
    );
}

#[test]
fn spread_method() {
    verify(
        "const result={method(){},...{method:1}}.method; result;",
        false,
        "number",
        &[(
            2783,
            "'method' is specified more than once, so this usage will be overwritten.",
            14,
            6,
        )],
    );
}

#[test]
fn spread_shorthand() {
    verify(
        "const key=1;const result={key,...{key:2}}.key; result;",
        false,
        "number",
        &[(
            2783,
            "'key' is specified more than once, so this usage will be overwritten.",
            26,
            3,
        )],
    );
}

#[test]
fn conditional_spread_any() {
    verify(
        "declare const flag:any;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "any",
        &[],
    );
}

#[test]
fn conditional_spread_unknown() {
    verify(
        "declare const flag:unknown;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "any",
        &[(
            2698,
            "Spread types may only be created from object types.",
            47,
            20,
        )],
    );
}

#[test]
fn conditional_spread_boolean() {
    verify(
        "declare const flag:boolean;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn conditional_spread_true() {
    verify(
        "declare const flag:true;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "number",
        &[(
            2783,
            "'key' is specified more than once, so this usage will be overwritten.",
            38,
            5,
        )],
    );
}

#[test]
fn conditional_spread_false() {
    verify(
        "declare const flag:false;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "any",
        &[(
            2698,
            "Spread types may only be created from object types.",
            45,
            20,
        )],
    );
}

#[test]
fn conditional_spread_string() {
    verify(
        "declare const flag:string;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn conditional_spread_number() {
    verify(
        "declare const flag:number;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn conditional_spread_optional() {
    verify(
        "declare const flag:true|undefined;const result={key:1,...(flag && {key:2})}.key; result;",
        false,
        "number",
        &[],
    );
}

#[test]
fn conditional_spread_alias() {
    verify("declare const flag:boolean;const spread=flag && {key:2};const result={key:1,...spread}.key; result;", false, "number", &[
    ]);
}

#[test]
fn loose_logical_any() {
    verify_options(
        "declare const flag:any;const result=flag && 42; result;",
        false,
        false,
        "0 | 42",
        &[],
    );
}

#[test]
fn loose_logical_unknown() {
    verify_options(
        "declare const flag:unknown;const result=flag && 42; result;",
        false,
        false,
        "0 | 42",
        &[],
    );
}

#[test]
fn loose_logical_boolean() {
    verify_options(
        "declare const flag:boolean;const result=flag && 42; result;",
        false,
        false,
        "0 | 42",
        &[],
    );
}

#[test]
fn loose_logical_false() {
    verify_options(
        "declare const flag:false;const result=flag && 42; result;",
        false,
        false,
        "false",
        &[],
    );
}

#[test]
fn loose_logical_true() {
    verify_options(
        "declare const flag:true;const result=flag && 42; result;",
        false,
        false,
        "0 | 42",
        &[],
    );
}

#[test]
fn loose_logical_number() {
    verify_options(
        "declare const flag:number;const result=flag && 42; result;",
        false,
        false,
        "0 | 42",
        &[],
    );
}

#[test]
fn loose_logical_string() {
    verify_options(
        "declare const flag:string;const result=flag && 42; result;",
        false,
        false,
        "0 | 42",
        &[],
    );
}

#[test]
fn loose_logical_nullable() {
    verify_options(
        "declare const flag:number|null;const result=flag && 42; result;",
        false,
        false,
        "0 | 42",
        &[],
    );
}

#[test]
fn loose_logical_null() {
    verify_options(
        "declare const flag:null;const result=flag && 42; result;",
        false,
        false,
        "null",
        &[],
    );
}

#[test]
fn boolean_flow_true() {
    verify(
        "let flag=true;const result=flag && 42; result;",
        false,
        "42",
        &[],
    );
}

#[test]
fn boolean_flow_false() {
    verify(
        "let flag=false;const result=flag && 42; result;",
        false,
        "false",
        &[],
    );
}

#[test]
fn boolean_flow_reassign_false() {
    verify(
        "let flag=true;flag=false;const result=flag && 42; result;",
        false,
        "false",
        &[],
    );
}

#[test]
fn boolean_flow_reassign_true() {
    verify(
        "let flag=false;flag=true;const result=flag && 42; result;",
        false,
        "42",
        &[],
    );
}

#[test]
fn boolean_flow_declared() {
    verify(
        "let flag:boolean=true;const result=flag && 42; result;",
        false,
        "42",
        &[],
    );
}

#[test]
fn boolean_flow_any() {
    verify(
        "let flag:any=true;const result=flag && 42; result;",
        false,
        "any",
        &[],
    );
}

#[test]
fn boolean_flow_function() {
    verify("let flag=true;function read(){const result=flag && 42;return result;}const result=read(); result;", false, "false | 42", &[
    ]);
}

#[test]
fn boolean_flow_contextual() {
    verify(
        "let fn:(a:string)=>string;let flag=true;fn=flag && (a=>a);const result=fn; result;",
        false,
        "(a: string) => string",
        &[],
    );
}

#[test]
fn duplicate_interface() {
    verify("interface I{[x:number]:string;[x:number]:number;}declare const value:I;const result=value[1]; result;", false, "string", &[
        (2374, "Duplicate index signature for type 'number'.", 12, 18),
        (2374, "Duplicate index signature for type 'number'.", 30, 18),
    ]);
}

#[test]
fn duplicate_alias() {
    verify("interface I{[x:number]:string;[x:number]:number;}type Alias=I;declare const value:Alias;const result=value[1]; result;", false, "string", &[
        (2374, "Duplicate index signature for type 'number'.", 12, 18),
        (2374, "Duplicate index signature for type 'number'.", 30, 18),
    ]);
}

#[test]
fn inherited_same_index() {
    verify("interface A{[x:number]:string;}interface B{[x:number]:number;}interface I extends A,B{}declare const value:I;const result=value[1]; result;", false, "string", &[
        (2430, "Interface 'I' incorrectly extends interface 'B'.\n  'number' index signatures are incompatible.\n    Type 'string' is not assignable to type 'number'.", 72, 1),
    ]);
}

#[test]
fn intersection_indices() {
    verify("interface A{[x:number]:string;}interface B{[x:number]:number;}declare const value:A&B;const result=value[1]; result;", false, "never", &[
    ]);
}

#[test]
fn intersection_index_alias() {
    verify("interface A{[x:number]:string;}interface B{[x:number]:number;}type Both=A&B;declare const value:Both;const result=value[1]; result;", false, "never", &[
    ]);
}

#[test]
fn overriding_index() {
    verify("interface A{[x:number]:number;}interface I extends A{[x:number]:1;}declare const value:I;const result=value[1]; result;", false, "1", &[
    ]);
}
