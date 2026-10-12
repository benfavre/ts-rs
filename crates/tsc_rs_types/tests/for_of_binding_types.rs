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
fn number_array() {
    verify("for(const value of [1,2]){value;}", "number", &[]);
}

#[test]
fn boolean_array() {
    verify("for(const value of [true,false]){value;}", "boolean", &[]);
}

#[test]
fn string_iterable() {
    verify("for(var value of \"text\"){value;}", "string", &[]);
}

#[test]
fn readonly_array() {
    verify(
        "declare const input:ReadonlyArray<number>;for(const value of input){value;}",
        "number",
        &[],
    );
}

#[test]
fn readonly_tuple() {
    verify(
        "for(const value of [1,\"a\"] as const){value;}",
        "1 | \"a\"",
        &[],
    );
}

#[test]
fn array_union() {
    verify(
        "declare const input:number[]|string[];for(let value of input){value;}",
        "string | number",
        &[],
    );
}

#[test]
fn set() {
    verify(
        "declare const input:Set<string>;for(const value of input){value;}",
        "string",
        &[],
    );
}

#[test]
fn map() {
    verify(
        "declare const input:Map<string,number>;for(const value of input){value;}",
        "[string, number]",
        &[],
    );
}

#[test]
fn map_destructuring() {
    verify(
        "declare const input:Map<string,number>;for(const [key,value] of input){value;}",
        "number",
        &[],
    );
}

#[test]
fn object_destructuring() {
    verify(
        "declare const input:{value:string}[];for(const {value} of input){value;}",
        "string",
        &[],
    );
}

#[test]
fn object_renamed_binding() {
    verify(
        "declare const input:{count:number}[];for(const {count:value} of input){value;}",
        "number",
        &[],
    );
}

#[test]
fn destructuring_default() {
    verify(
        "declare const input:{value?:number}[];for(const {value=0} of input){value;}",
        "number",
        &[],
    );
}

#[test]
fn array_rest_binding() {
    verify(
        "declare const input:[number,string,boolean][];for(const [key,...value] of input){value;}",
        "[string, boolean]",
        &[],
    );
}

#[test]
fn generic_element() {
    verify(
        "function f<T>(input:T[]){for(const value of input){value;}}",
        "T",
        &[],
    );
}

#[test]
fn custom_iterator() {
    verify("declare const input:{[Symbol.iterator]():{next():{value:string;done:false}}};for(const value of input){value;}", "string", &[]);
}

#[test]
fn custom_literal_iterator() {
    verify("declare const input:{[Symbol.iterator]():{next():{value:1;done:false}}};for(const value of input){value;}", "1", &[]);
}

#[test]
fn sync_promise_elements() {
    verify(
        "declare const input:Promise<number>[];for(const value of input){value;}",
        "Promise<number>",
        &[],
    );
}

#[test]
fn await_promise_elements() {
    verify(
        "async function f(input:Promise<number>[]){for await(const value of input){value;}}",
        "number",
        &[],
    );
}

#[test]
fn async_iterable() {
    verify(
        "async function f(input:AsyncIterable<string>){for await(const value of input){value;}}",
        "string",
        &[],
    );
}

#[test]
fn async_generator() {
    verify(
        "async function f(input:AsyncGenerator<string>){for await(const value of input){value;}}",
        "string",
        &[],
    );
}

#[test]
fn wrong_method_diagnostic() {
    verify(
        "for(const value of [1]){value.toUpperCase();value;}",
        "number",
        &[(
            2339,
            "Property 'toUpperCase' does not exist on type 'number'.",
            "toUpperCase",
        )],
    );
}

#[test]
fn wrong_assignment_diagnostic() {
    verify(
        "for(let value of [1]){value=\"bad\";value;}",
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            "value",
        )],
    );
}

#[test]
fn var_after_loop() {
    verify(
        "for(var value of [1]){}value;",
        "number",
        &[(
            2454,
            "Variable 'value' is used before being assigned.",
            "value",
        )],
    );
}

#[test]
fn invalid_iterand() {
    verify(
        "for(const value of 123){value;}",
        "any",
        &[(
            2488,
            "Type '123' must have a '[Symbol.iterator]()' method that returns an iterator.",
            "123",
        )],
    );
}

#[test]
fn custom_inferred_iterator() {
    verify("const input={[Symbol.iterator](){return {next(){return {value:1,done:false}}}}};for(const value of input){value;}", "number", &[]);
}

#[test]
fn initialized_var_after_loop() {
    verify("var value=0;for(var value of [1]){}value;", "number", &[]);
}

#[test]
fn async_iterable_promise() {
    verify("async function f(input:AsyncIterable<Promise<number>>){for await(const value of input){value;}}", "number", &[]);
}

#[test]
fn undefined_element() {
    verify(
        "declare const input:(number|undefined)[];for(const value of input){value;}",
        "number | undefined",
        &[],
    );
}

#[test]
fn lexical_shadowing() {
    verify(
        "let value=\"outer\";for(const value of [1]){value.toFixed();}value;",
        "string",
        &[],
    );
}

#[test]
fn for_await_var_after_loop() {
    verify(
        "async function f(input:Promise<number>[]){for await(var value of input){}value;}",
        "number",
        &[(
            2454,
            "Variable 'value' is used before being assigned.",
            "value",
        )],
    );
}

#[test]
fn array_alias() {
    verify(
        "type Items=number[];declare const input:Items;for(const value of input){value;}",
        "number",
        &[],
    );
}

#[test]
fn constrained_array() {
    verify(
        "function f<T extends number[]>(input:T){for(const value of input){value;}}",
        "number",
        &[],
    );
}

#[test]
fn constrained_iterable() {
    verify(
        "function f<T,I extends Iterable<T>>(input:I){for(const value of input){value;}}",
        "T",
        &[],
    );
}

#[test]
fn custom_async_iterator() {
    verify("declare const input:{[Symbol.asyncIterator]():{next():Promise<{value:string;done:false}>}};async function f(){for await(const value of input){value;}}", "string", &[]);
}

#[test]
fn numeric_object_pattern_reads_tuple_positions() {
    verify("declare const input:Iterable<[key:string,item:number]>;for(const {1:value} of input){value;}", "number", &[]);
}
