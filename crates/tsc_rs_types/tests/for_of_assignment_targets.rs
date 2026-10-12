// Expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(source: &str, expected_type: &str, expected: &[(u32, &str, &str)]) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let output = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2018),
            strict: Some(true),
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
    let position = source.rfind("value;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&position).map(String::as_str),
        Some(expected_type),
        "{source}"
    );
}

#[test]
fn plain_string_target() {
    verify(
        "let value:string;for(value of [\"a\"]){value;}",
        "string",
        &[],
    );
}

#[test]
fn union_target_narrows() {
    verify(
        "let value:string|number=0;for(value of [\"a\"]){value;}",
        "string",
        &[],
    );
}

#[test]
fn union_target_after_loop() {
    verify(
        "let value:string|number=0;for(value of [\"a\"]){}value;",
        "string | number",
        &[],
    );
}

#[test]
fn literal_source_broad_target() {
    verify(
        "let value:string=\"previous\";for(value of [\"a\"] as const){value;}",
        "string",
        &[],
    );
}

#[test]
fn incompatible_target() {
    verify(
        "let value:number;for(value of [\"a\"]){value;}",
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            "value",
        )],
    );
}

#[test]
fn explicit_any_target() {
    verify("let value:any;for(value of [1]){value;}", "any", &[]);
}

#[test]
fn unknown_target() {
    verify(
        "let value:unknown;for(value of [1]){value;}",
        "unknown",
        &[],
    );
}

#[test]
fn undeclared_target() {
    verify(
        "for(value of [1]){value;}",
        "any",
        &[
            (2304, "Cannot find name 'value'.", "value"),
            (2304, "Cannot find name 'value'.", "value"),
        ],
    );
}

#[test]
fn constant_target() {
    verify(
        "const value=0;for(value of [1]){value;}",
        "0",
        &[(
            2588,
            "Cannot assign to 'value' because it is a constant.",
            "value",
        )],
    );
}

#[test]
fn after_possibly_empty_loop() {
    verify(
        "let value:string;for(value of [\"a\"]){}value;",
        "string",
        &[(
            2454,
            "Variable 'value' is used before being assigned.",
            "value",
        )],
    );
}

#[test]
fn property_target() {
    verify("declare const holder:{item:string|number};for(holder.item of [\"a\"]){const value=holder.item;value;}", "string", &[]);
}

#[test]
fn element_target() {
    verify("declare const holder:{item:string|number};for(holder[\"item\"] of [\"a\"]){const value=holder.item;value;}", "string", &[]);
}

#[test]
fn readonly_target() {
    verify("declare const holder:{readonly item:number};for(holder.item of [1]){const value=holder.item;value;}", "number", &[(2540, "Cannot assign to 'item' because it is a read-only property.", "item")]);
}

#[test]
fn optional_property_target() {
    verify("declare const holder:{item:number};for(holder?.item of [1]){const value=holder.item;value;}", "number", &[(2781, "The left-hand side of a 'for...of' statement may not be an optional property access.", "holder?.item")]);
}

#[test]
fn object_pattern() {
    verify(
        "let value:string;for({value} of [{value:\"a\"}]){value;}",
        "string",
        &[],
    );
}

#[test]
fn object_pattern_narrowing() {
    verify(
        "let value:string|number=0;for({value} of [{value:\"a\"}]){value;}",
        "string",
        &[],
    );
}

#[test]
fn object_pattern_mismatch() {
    verify(
        "let value:number;for({value} of [{value:\"a\"}]){value;}",
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            "value",
        )],
    );
}

#[test]
fn object_renamed_binding() {
    verify(
        "let value:number;for({item:value} of [{item:1}]){value;}",
        "number",
        &[],
    );
}

#[test]
fn object_default() {
    verify(
        "let value:number;declare const input:{value?:number}[];for({value=1} of input){value;}",
        "number",
        &[],
    );
}

#[test]
fn array_pattern() {
    verify(
        "let value:string;declare const input:[string][];for([value] of input){value;}",
        "string",
        &[],
    );
}

#[test]
fn array_pattern_mismatch() {
    verify(
        "let value:number;declare const input:[string][];for([value] of input){value;}",
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            "value",
        )],
    );
}

#[test]
fn array_rest() {
    verify("let value:string[];declare const input:[number,...string[]][];let head:number;for([head,...value] of input){value;}", "string[]", &[]);
}

#[test]
fn object_rest() {
    verify("let value:{item:number};let head:string;declare const input:{head:string,item:number}[];for({head,...value} of input){value;}", "{\n    item: number;\n}", &[]);
}

#[test]
fn async_target() {
    verify("async function f(input:Promise<number>[]){let value:number;for await(value of input){value;}}", "number", &[]);
}

#[test]
fn async_mismatch() {
    verify("async function f(input:Promise<number>[]){let value:string;for await(value of input){value;}}", "string", &[(2322, "Type 'number' is not assignable to type 'string'.", "value")]);
}

#[test]
fn parenthesized_target() {
    verify(
        "let value:string;for((value) of [\"a\"]){value;}",
        "string",
        &[],
    );
}

#[test]
fn body_reassignment() {
    verify(
        "let value:string|number=0;for(value of [\"a\"]){value=1;value;}",
        "number",
        &[],
    );
}

#[test]
fn any_iterand_typed_target() {
    verify(
        "declare const input:any;let value:string;for(value of input){value;}",
        "string",
        &[],
    );
}

#[test]
fn getter_and_setter_types() {
    verify("class Holder{get item():number{return 1;}set item(input:string){}}const holder=new Holder();for(holder.item of [\"a\"]){const value=holder.item;value;}", "number", &[]);
}

#[test]
fn getter_union_and_setter() {
    verify("class Holder{get item():string|number{return 1;}set item(input:string){}}const holder=new Holder();for(holder.item of [\"a\"]){const value=holder.item;value;}", "string", &[]);
}

#[test]
fn element_target_then_property_write() {
    verify("declare const holder:{item:string|number};for(holder[\"item\"] of [\"a\"]){holder.item=1;const value=holder[\"item\"];value;}", "number", &[]);
}

#[test]
fn unicode_element_target() {
    verify("declare const holder:{élément:string|number};for(holder[\"élément\"] of [\"a\"]){const value=holder.élément;value;}", "string", &[]);
}

#[test]
fn dotted_element_key_is_distinct() {
    verify("declare const holder:{\"inner.item\":string|number;inner:{item:string|number}};for(holder[\"inner.item\"] of [\"a\"]){const value=holder.inner.item;value;}", "string | number", &[]);
}

#[test]
fn destructuring_constant() {
    verify(
        "const value=0;for([value] of [[1]]){value;}",
        "0",
        &[(
            2588,
            "Cannot assign to 'value' because it is a constant.",
            "value",
        )],
    );
}

#[test]
fn default_mismatch() {
    verify(
        "let value:number;declare const input:{value?:string}[];for({value=0} of input){value;}",
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            "value",
        )],
    );
}

#[test]
fn shorthand_unknown_with_default() {
    verify("declare const input:any[];for({value=missing} of input){value;}", "any", &[(18004, "No value exists in scope for the shorthand property 'value'. Either declare one or provide an initializer.", "value"), (18004, "No value exists in scope for the shorthand property 'missing'. Either declare one or provide an initializer.", "missing"), (2304, "Cannot find name 'value'.", "value")]);
}

#[test]
fn default_only_mismatch() {
    verify("let value:number;declare const input:{value?:number}[];for({value=\"bad\"} of input){value;}", "number", &[
(2322, "Type 'string' is not assignable to type 'number'.", "value"),
]);
}

#[test]
fn both_source_and_default_mismatch() {
    verify("let value:number;declare const input:{value?:string}[];for({value=false} of input){value;}", "number", &[
(2322, "Type 'boolean' is not assignable to type 'number'.", "value"),
(2322, "Type 'string' is not assignable to type 'number'.", "value"),
]);
}

#[test]
fn array_default_mismatch() {
    verify(
        "let value:number;declare const input:[string?][];for([value=0] of input){value;}",
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            "value",
        )],
    );
}

#[test]
fn repeated_element_targets() {
    verify("declare const holder:{item:string|number};for(holder[\"item\"] of [\"a\"]){}for(holder[\"item\"] of [1]){const value=holder[\"item\"];value;}", "number", &[
]);
}

#[test]
fn property_target_then_element_write() {
    verify("declare const holder:{item:string|number};for(holder.item of [\"a\"]){holder[\"item\"]=1;const value=holder.item;value;}", "number", &[
]);
}

#[test]
fn repeated_missing_property() {
    verify("declare const holder:{x:number};for(holder.missing of []){}for((holder.missing) of []){}const value=0;value;", "0", &[
(2339, "Property 'missing' does not exist on type '{ x: number; }'.", "missing"),
(2339, "Property 'missing' does not exist on type '{ x: number; }'.", "missing"),
]);
}

#[test]
fn required_property_invalid_default() {
    verify("let value:number;declare const input:{value:number}[];for({value=\"bad\"} of input){value;}", "number", &[
(2322, "Type 'string' is not assignable to type 'number'.", "value"),
]);
}

#[test]
fn nested_property_default() {
    verify("let value:number;declare const input:{nest?:{value?:string}}[];for({nest:{value=0}={}} of input){value;}", "number", &[
(2322, "Type 'string' is not assignable to type 'number'.", "value"),
]);
}

#[test]
fn property_assignment_flow_after_loop() {
    verify("declare const holder:{item:string|number};holder.item=1;for(holder.item of [1]){holder.item=\"a\";}const value=holder.item;value;", "string | number", &[
]);
}
