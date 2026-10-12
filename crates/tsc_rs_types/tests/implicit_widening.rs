// Diagnostic expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(
    source: &str,
    strict_null_checks: bool,
    no_implicit_any: bool,
    expected: &[(u32, &str, u32, u32)],
) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let mut checker = TypeChecker::new();
    let mut output = checker.check_with_options(
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
    output
        .diagnostics
        .sort_by_key(|d| (d.span.map(|s| s.start), d.code));
    let actual = output
        .diagnostics
        .iter()
        .map(|d| {
            let s = d.span.unwrap();
            (d.code, d.message.as_str(), s.start, s.end - s.start)
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn basic_null() {
    verify(
        "const value={item:null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            13,
            9,
        )],
    );
}

#[test]
fn basic_undefined() {
    verify(
        "const value={item:undefined};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            13,
            14,
        )],
    );
}

#[test]
fn basic_empty_array() {
    verify(
        "const value={item:[]};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any[]' type.",
            13,
            7,
        )],
    );
}

#[test]
fn basic_holes() {
    verify(
        "const value={item:[,,]};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any[]' type.",
            13,
            9,
        )],
    );
}

#[test]
fn nested_null_array() {
    verify(
        "const value={item:[[null]]};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any[][]' type.",
            13,
            13,
        )],
    );
}

#[test]
fn nested_object() {
    verify(
        "const value={outer:{item:null}};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            20,
            9,
        )],
    );
}

#[test]
fn array_of_objects() {
    verify(
        "const value=[{item:null}];value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            14,
            9,
        )],
    );
}

#[test]
fn discarded_literal() {
    verify("({item:null});", false, true, &[]);
}

#[test]
fn bare_literal_argument_any() {
    verify(
        "declare function accept(x:any):void;accept({item:null});",
        false,
        true,
        &[],
    );
}

#[test]
fn bare_literal_argument_unknown() {
    verify(
        "declare function accept(x:unknown):void;accept({item:null});",
        false,
        true,
        &[],
    );
}

#[test]
fn bare_literal_argument_object() {
    verify(
        "declare function accept(x:object):void;accept({item:null});",
        false,
        true,
        &[],
    );
}

#[test]
fn bare_literal_argument_typed() {
    verify(
        "declare function accept(x:{item:number}):void;accept({item:null});",
        false,
        true,
        &[],
    );
}

#[test]
fn generic_literal_argument() {
    verify(
        "declare function accept<T>(x:T):T;const value=accept({item:null});value;",
        false,
        true,
        &[],
    );
}

#[test]
fn generic_constrained_argument() {
    verify("declare function accept<T extends {item:number}>(x:T):T;const value=accept({item:null});value;", false, true, &[]);
}

#[test]
fn annotated_object() {
    verify(
        "const value:{item:any}={item:null};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn annotated_unknown() {
    verify("const value:unknown={item:null};value;", false, true, &[]);
}

#[test]
fn annotated_any() {
    verify("const value:any={item:null};value;", false, true, &[]);
}

#[test]
fn annotated_object_keyword() {
    verify("const value:object={item:null};value;", false, true, &[]);
}

#[test]
fn annotated_missing_member() {
    verify("const value:{extra?:number}={item:null};value;", false, true, &[(2353, "Object literal may only specify known properties, and 'item' does not exist in type '{ extra?: number; }'.", 29, 4)]);
}

#[test]
fn asserted_any() {
    verify("const value={item:null} as any;value;", false, true, &[]);
}

#[test]
fn asserted_object() {
    verify(
        "const value={item:null} as {item:number};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn asserted_const() {
    verify(
        "const value={item:null} as const;value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            13,
            9,
        )],
    );
}

#[test]
fn satisfies_unknown() {
    verify(
        "const value={item:null} satisfies unknown;value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            13,
            9,
        )],
    );
}

#[test]
fn satisfies_property() {
    verify(
        "const value={item:null} satisfies {item:number};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            13,
            9,
        )],
    );
}

#[test]
fn nested_contextual() {
    verify(
        "const value:{outer:{item:any}}={outer:{item:[]}};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn array_contextual() {
    verify(
        "const value:{item:any}[]=[{item:null}];value;",
        false,
        true,
        &[],
    );
}

#[test]
fn mixed_contextual_property() {
    verify("const value:{extra:number}={extra:1,item:null};value;", false, true, &[(2353, "Object literal may only specify known properties, and 'item' does not exist in type '{ extra: number; }'.", 36, 4)]);
}

#[test]
fn function_return() {
    verify(
        "function make(){return {item:null};}const value=make();value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            24,
            9,
        )],
    );
}

#[test]
fn function_annotated_return() {
    verify(
        "function make():{item:any}{return {item:null};}const value=make();value;",
        false,
        true,
        &[],
    );
}

#[test]
fn arrow_return() {
    verify(
        "const make=()=>({item:null});const value=make();value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            17,
            9,
        )],
    );
}

#[test]
fn arrow_contextual_return() {
    verify(
        "const make:()=>{item:any}=()=>({item:null});const value=make();value;",
        false,
        true,
        &[],
    );
}

#[test]
fn property_arrow_return() {
    verify(
        "const value={make:()=>({item:null})};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            24,
            9,
        )],
    );
}

#[test]
fn parameter_default() {
    verify(
        "function make(value={item:null}){return value;}make();",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            21,
            9,
        )],
    );
}

#[test]
fn parameter_annotated_default() {
    verify(
        "function make(value:{item:any}={item:null}){return value;}make();",
        false,
        true,
        &[],
    );
}

#[test]
fn property_quoted() {
    verify(
        "const value={\"item\":null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property '\"item\"' implicitly has an 'any' type.",
            13,
            11,
        )],
    );
}

#[test]
fn property_number() {
    verify(
        "const value={123:null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property '123' implicitly has an 'any' type.",
            13,
            8,
        )],
    );
}

#[test]
fn property_computed_literal() {
    verify(
        "const value={[\"item\"]:null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property '[\"item\"]' implicitly has an 'any' type.",
            13,
            13,
        )],
    );
}

#[test]
fn property_computed_dynamic() {
    verify(
        "declare const key:string;const value={[key]:null};value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an '{ [x: string]: any; }' type.",
            31,
            5,
        )],
    );
}

#[test]
fn property_symbol() {
    verify(
        "const key=Symbol();const value={[key]:null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property '[key]' implicitly has an 'any' type.",
            32,
            10,
        )],
    );
}

#[test]
fn shorthand_null() {
    verify(
        "const item=null;const value={item};value;",
        false,
        true,
        &[(7005, "Variable 'item' implicitly has an 'any' type.", 6, 4)],
    );
}

#[test]
fn shorthand_undefined() {
    verify(
        "const item=undefined;const value={item};value;",
        false,
        true,
        &[(7005, "Variable 'item' implicitly has an 'any' type.", 6, 4)],
    );
}

#[test]
fn property_any_reference() {
    verify(
        "declare const item:any;const value={item};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn property_null_annotation() {
    verify(
        "declare const item:null;const value={item};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn property_undefined_annotation() {
    verify(
        "declare const item:undefined;const value={item};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn property_null_cast() {
    verify("const value={item:null as null};value;", false, true, &[]);
}

#[test]
fn conditional_null_undefined() {
    verify(
        "declare const flag:boolean;const value={item:flag?null:undefined};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            40,
            24,
        )],
    );
}

#[test]
fn conditional_null_string() {
    verify(
        "declare const flag:boolean;const value={item:flag?null:\"x\"};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn conditional_object_union() {
    verify(
        "declare const flag:boolean;const value=flag?{item:null}:{item:1};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn spread_any() {
    verify(
        "declare const input:any;const value={item:null,...input};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn spread_object() {
    verify(
        "declare const input:{item:number};const value={item:null,...input};value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an '{ item: number; }' type.",
            40,
            5,
        )],
    );
}

#[test]
fn spread_reversed() {
    verify(
        "declare const input:{item:number};const value={...input,item:null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            56,
            9,
        )],
    );
}

#[test]
fn destructuring_initializer() {
    verify(
        "const {item}={item:null};",
        false,
        true,
        &[(
            7031,
            "Binding element 'item' implicitly has an 'any' type.",
            7,
            4,
        )],
    );
}

#[test]
fn destructuring_assignment() {
    verify("let item:number;({item}={item:null});", false, true, &[]);
}

#[test]
fn assignment_any() {
    verify("let value:any;value={item:null};value;", false, true, &[]);
}

#[test]
fn class_property() {
    verify(
        "class Box{value={item:null}}new Box();",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            17,
            9,
        )],
    );
}

#[test]
fn class_annotated_property() {
    verify(
        "class Box{value:{item:any}={item:null}}new Box();",
        false,
        true,
        &[],
    );
}

#[test]
fn getter_return() {
    verify(
        "const value={get item(){return null}};value;",
        false,
        true,
        &[(
            7010,
            "'item', which lacks return-type annotation, implicitly has an 'any' return type.",
            17,
            4,
        )],
    );
}

#[test]
fn method_return() {
    verify(
        "const value={make(){return {item:null}}};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            28,
            9,
        )],
    );
}

#[test]
fn nested_method_array() {
    verify(
        "const value={make(){return [{item:null}]}};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            29,
            9,
        )],
    );
}

#[test]
fn basic_null_strict() {
    verify("const value={item:null};value;", true, true, &[]);
}

#[test]
fn basic_null_no_implicit_disabled() {
    verify("const value={item:null};value;", false, false, &[]);
}

#[test]
fn basic_undefined_strict() {
    verify("const value={item:undefined};value;", true, true, &[]);
}

#[test]
fn basic_undefined_no_implicit_disabled() {
    verify("const value={item:undefined};value;", false, false, &[]);
}

#[test]
fn basic_empty_array_strict() {
    verify("const value={item:[]};value;", true, true, &[]);
}

#[test]
fn basic_empty_array_no_implicit_disabled() {
    verify("const value={item:[]};value;", false, false, &[]);
}

#[test]
fn nested_object_strict() {
    verify("const value={outer:{item:null}};value;", true, true, &[]);
}

#[test]
fn nested_object_no_implicit_disabled() {
    verify("const value={outer:{item:null}};value;", false, false, &[]);
}

#[test]
fn arrow_return_strict() {
    verify(
        "const make=()=>({item:null});const value=make();value;",
        true,
        true,
        &[],
    );
}

#[test]
fn arrow_return_no_implicit_disabled() {
    verify(
        "const make=()=>({item:null});const value=make();value;",
        false,
        false,
        &[],
    );
}

#[test]
fn function_expression_return() {
    verify(
        "const make=function(){return {item:null}};make();",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            30,
            9,
        )],
    );
}

#[test]
fn named_function_expression_return() {
    verify(
        "const make=function named(){return null};make();",
        false,
        true,
        &[(
            7010,
            "'named', which lacks return-type annotation, implicitly has an 'any' return type.",
            20,
            5,
        )],
    );
}

#[test]
fn function_expression_annotated() {
    verify("const make:functionType=function(){return {item:null}};type functionType=()=>{item:any};make();", false, true, &[]);
}

#[test]
fn arrow_scalar_return() {
    verify("const make=()=>null;make();", false, true, &[(7011, "Function expression, which lacks return-type annotation, implicitly has an 'any' return type.", 11, 8)]);
}

#[test]
fn arrow_array_return() {
    verify("const make=()=>[];make();", false, true, &[(7011, "Function expression, which lacks return-type annotation, implicitly has an 'any[]' return type.", 11, 6)]);
}

#[test]
fn function_scalar_return() {
    verify(
        "function make(){return null;}make();",
        false,
        true,
        &[(
            7010,
            "'make', which lacks return-type annotation, implicitly has an 'any' return type.",
            9,
            4,
        )],
    );
}

#[test]
fn function_array_return() {
    verify(
        "function make(){return [];}make();",
        false,
        true,
        &[(
            7010,
            "'make', which lacks return-type annotation, implicitly has an 'any[]' return type.",
            9,
            4,
        )],
    );
}

#[test]
fn function_mixed_returns() {
    verify(
        "function make(flag:boolean){if(flag)return null;return 1;}make(true);",
        false,
        true,
        &[],
    );
}

#[test]
fn function_mixed_objects() {
    verify(
        "function make(flag:boolean){if(flag)return {item:null};return {item:1};}make(true);",
        false,
        true,
        &[],
    );
}

#[test]
fn function_empty_object_branch() {
    verify(
        "function make(flag:boolean){if(flag)return {item:null};return {};}make(true);",
        false,
        true,
        &[],
    );
}

#[test]
fn nested_function_return_containers() {
    verify("function outer():number{const make=function(){return null};return 1;}outer();", false, true, &[(7011, "Function expression, which lacks return-type annotation, implicitly has an 'any' return type.", 35, 8)]);
}

#[test]
fn contextual_method() {
    verify(
        "const value:{make():{item:any}}={make(){return {item:null}}};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn contextual_getter() {
    verify(
        "const value:{item:any}={get item(){return null}};value;",
        false,
        true,
        &[(
            7010,
            "'item', which lacks return-type annotation, implicitly has an 'any' return type.",
            28,
            4,
        )],
    );
}

#[test]
fn getter_declared_return() {
    verify(
        "const value={get item():any{return null}};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn getter_nested_object() {
    verify(
        "const value={get item(){return {nested:null}}};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'nested' implicitly has an 'any' type.",
            32,
            11,
        )],
    );
}

#[test]
fn getter_array_return() {
    verify(
        "const value={get item(){return []}};value;",
        false,
        true,
        &[(
            7010,
            "'item', which lacks return-type annotation, implicitly has an 'any[]' return type.",
            17,
            4,
        )],
    );
}

#[test]
fn class_method_scalar_return() {
    verify(
        "class Box{make(){return null}}new Box();",
        false,
        true,
        &[(
            7010,
            "'make', which lacks return-type annotation, implicitly has an 'any' return type.",
            10,
            4,
        )],
    );
}

#[test]
fn class_getter_scalar_return() {
    verify(
        "class Box{get item(){return null}}new Box();",
        false,
        true,
        &[(
            7010,
            "'item', which lacks return-type annotation, implicitly has an 'any' return type.",
            14,
            4,
        )],
    );
}

#[test]
fn class_private_field() {
    verify(
        "class Box{private item=null}new Box();",
        false,
        true,
        &[(7008, "Member 'item' implicitly has an 'any' type.", 18, 4)],
    );
}

#[test]
fn const_null_unused() {
    verify(
        "const value=null;",
        false,
        true,
        &[(7005, "Variable 'value' implicitly has an 'any' type.", 6, 5)],
    );
}

#[test]
fn let_null_read() {
    verify("let value=null;value;", false, true, &[]);
}

#[test]
fn var_undefined_read() {
    verify("var value=undefined;value;", false, true, &[]);
}

#[test]
fn exported_null_variable() {
    verify(
        "export let value=null;value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'any' type.",
            11,
            5,
        )],
    );
}

#[test]
fn const_null_array() {
    verify(
        "const value=[null];value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'any[]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn const_mixed_nullable_array() {
    verify(
        "const value=[null,undefined];value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'any[]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn const_array_with_typed_element() {
    verify("const value=[null,1];value;", false, true, &[]);
}

#[test]
fn empty_const_tuple() {
    verify("const value=[] as const;value;", false, true, &[]);
}

#[test]
fn nullable_const_tuple() {
    verify(
        "const value=[null,undefined] as const;value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'readonly [any, any]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn void_expression() {
    verify(
        "const value=void 0;value;",
        false,
        true,
        &[(7005, "Variable 'value' implicitly has an 'any' type.", 6, 5)],
    );
}

#[test]
fn void_property() {
    verify(
        "const value={item:void 0};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            13,
            11,
        )],
    );
}

#[test]
fn parameter_null_default() {
    verify(
        "function make(value=null){return value;}make();",
        false,
        true,
        &[(
            7006,
            "Parameter 'value' implicitly has an 'any' type.",
            14,
            10,
        )],
    );
}

#[test]
fn parameter_empty_default() {
    verify(
        "function make(value=[]){return value;}make();",
        false,
        true,
        &[(
            7006,
            "Parameter 'value' implicitly has an 'any[]' type.",
            14,
            8,
        )],
    );
}

#[test]
fn duplicate_properties_after_spread() {
    verify(
        "const value={...{item:null},item:1};value;",
        false,
        true,
        &[],
    );
}

#[test]
fn spread_nested_literal() {
    verify("const value={...{item:null}};value;", false, true, &[]);
}

#[test]
fn optional_spread_overwrite() {
    verify(
        "declare const input:{item?:number};const value={item:null,...input};value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an '{ item: number; }' type.",
            41,
            5,
        )],
    );
}

#[test]
fn computed_number_key() {
    verify(
        "declare const key:number;const value={[key]:null};value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an '{ [x: number]: any; }' type.",
            31,
            5,
        )],
    );
}

#[test]
fn computed_symbol_key() {
    verify(
        "declare const key:symbol;const value={[key]:null};value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an '{ [x: symbol]: any; }' type.",
            31,
            5,
        )],
    );
}

#[test]
fn computed_key_and_other_property() {
    verify(
        "declare const key:string;const value={[key]:null,other:1};value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an '{ [x: string]: number; other: number; }' type.",
            31,
            5,
        )],
    );
}

#[test]
fn conditional_two_widening_objects() {
    verify(
        "declare const flag:boolean;const value=flag?{left:null}:{right:null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'left' implicitly has an 'any' type.",
            45,
            9,
        )],
    );
}

#[test]
fn conditional_same_widening_objects() {
    verify(
        "declare const flag:boolean;const value=flag?{item:null}:{item:null};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            45,
            9,
        )],
    );
}

#[test]
fn array_two_widening_objects() {
    verify(
        "const value=[{left:null},{right:null}];value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'left' implicitly has an 'any' type.",
            14,
            9,
        )],
    );
}

#[test]
fn nested_sibling_properties() {
    verify(
        "const value={left:{nested:null},right:null};value;",
        false,
        true,
        &[
            (
                7018,
                "Object literal's property 'nested' implicitly has an 'any' type.",
                19,
                11,
            ),
            (
                7018,
                "Object literal's property 'right' implicitly has an 'any' type.",
                32,
                10,
            ),
        ],
    );
}

#[test]
fn contextual_arrow_default_array() {
    verify(
        "const make:(value?:number[])=>void=(value=[])=>{};make();",
        false,
        true,
        &[],
    );
}

#[test]
fn contextual_arrow_default_null() {
    verify(
        "const make:(value?:number)=>void=(value=null)=>{};make();",
        false,
        true,
        &[],
    );
}

#[test]
fn contextual_function_default_array() {
    verify(
        "const make:(value?:number[])=>void=function(value=[]){};make();",
        false,
        true,
        &[],
    );
}

#[test]
fn contextual_method_default_array() {
    verify(
        "const object:{make(value?:number[]):void}={make(value=[]){}};object.make();",
        false,
        true,
        &[],
    );
}

#[test]
fn let_void_expression() {
    verify(
        "let value=void 0;value;",
        false,
        true,
        &[(7005, "Variable 'value' implicitly has an 'any' type.", 4, 5)],
    );
}

#[test]
fn shadowed_undefined_parameter() {
    verify("function make(undefined:undefined){const value={item:undefined};return value;}make(void 0);", false, true, &[]);
}

#[test]
fn mixed_readonly_tuple() {
    verify(
        "const value=[null,1] as const;value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'readonly [any, 1]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn nested_readonly_tuple() {
    verify(
        "const value=[[null,1]] as const;value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'readonly [readonly [any, 1]]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn mixed_tuple_assignment() {
    verify(
        "let target:[any,number];const value=target=[null,1];value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an '[any, number]' type.",
            30,
            5,
        )],
    );
}

#[test]
fn mixed_readonly_tuple_return() {
    verify("function make(){return [null,1] as const;}make();", false, true, &[(7010, "'make', which lacks return-type annotation, implicitly has an 'readonly [any, 1]' return type.", 9, 4)]);
}

#[test]
fn empty_readonly_tuple() {
    verify(
        "function make(){return [] as const;}make();",
        false,
        true,
        &[],
    );
}

#[test]
fn const_tuple_with_regular_null() {
    verify(
        "const value=[null as null,1] as const;value;",
        false,
        true,
        &[],
    );
}

#[test]
fn const_tuple_with_holes() {
    verify(
        "const value=[,1] as const;value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'readonly [any, 1]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn const_tuple_property() {
    verify(
        "const value={item:[null,1]} as const;value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'readonly [any, 1]' type.",
            13,
            13,
        )],
    );
}

#[test]
fn const_tuple_with_regular_nested_tuple() {
    verify(
        "const value=[null,[1]] as const;value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'readonly [any, readonly [1]]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn const_tuple_with_regular_null_sibling() {
    verify(
        "const value=[null,null as null] as const;value;",
        false,
        true,
        &[(
            7005,
            "Variable 'value' implicitly has an 'readonly [any, null]' type.",
            6,
            5,
        )],
    );
}

#[test]
fn tuple_with_nested_nullable_object() {
    verify(
        "const value=[{item:null},1] as const;value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            14,
            9,
        )],
    );
}

#[test]
fn const_tuple_property_with_own_assertion() {
    verify(
        "const value={item:[null,1] as const};value;",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'readonly [any, 1]' type.",
            13,
            22,
        )],
    );
}

#[test]
fn const_tuple_union_return() {
    verify("function make(flag:boolean){if(flag)return [null,1] as const;return [null,2] as const;}make(true);", false, true, &[(7010, "'make', which lacks return-type annotation, implicitly has an 'readonly [any, 1] | readonly [any, 2]' return type.", 9, 4)]);
}

#[test]
fn tuple_parameter_default() {
    verify(
        "function make(value=[null,1] as const){return value;}make();",
        false,
        true,
        &[(
            7006,
            "Parameter 'value' implicitly has an 'readonly [any, 1]' type.",
            14,
            23,
        )],
    );
}

#[test]
fn tuple_class_property() {
    verify(
        "class Example{value=[null,1] as const;}",
        false,
        true,
        &[(
            7008,
            "Member 'value' implicitly has an 'readonly [any, 1]' type.",
            14,
            5,
        )],
    );
}

#[test]
fn async_function_return() {
    verify(
        "async function make(){return null;}make();",
        false,
        true,
        &[(
            7010,
            "'make', which lacks return-type annotation, implicitly has an 'any' return type.",
            15,
            4,
        )],
    );
}

#[test]
fn async_arrow_return() {
    verify("const make=async()=>[null];make();", false, true, &[(7011, "Function expression, which lacks return-type annotation, implicitly has an 'any[]' return type.", 11, 15)]);
}

#[test]
fn async_function_expression_return() {
    verify(
        "const make=async function(){return {item:null};};make();",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            36,
            9,
        )],
    );
}

#[test]
fn async_contextual_function_return() {
    verify(
        "const make:()=>Promise<any>=async()=>null;make();",
        false,
        true,
        &[],
    );
}

#[test]
fn annotated_function_expression_return() {
    verify(
        "const make=function():number{return \"bad\";};make();",
        false,
        true,
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            29,
            6,
        )],
    );
}

#[test]
fn annotated_getter_return() {
    verify(
        "const value={get item():number{return \"bad\";}};value;",
        false,
        true,
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            31,
            6,
        )],
    );
}
