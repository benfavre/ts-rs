// Expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(source: &str, strict: bool, expected_type: &str, expected: &[(u32, &str, &str)]) {
    let source = format!("{source} result;");
    let file = tsc_rs_parser::parse("subject.ts", &source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let output = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2015),
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
    let position = source.rfind("result;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&position).map(String::as_str),
        Some(expected_type)
    );
}

#[test]
fn object_literal_missing_string() {
    verify(
        "const result = {}[\"missing\"];",
        true,
        "undefined",
        &[(
            2339,
            "Property 'missing' does not exist on type '{}'.",
            "{}[\"missing\"]",
        )],
    );
}

#[test]
fn object_literal_missing_number() {
    verify(
        "const result = {}[3];",
        true,
        "undefined",
        &[(2339, "Property '3' does not exist on type '{}'.", "{}[3]")],
    );
}

#[test]
fn named_object_missing_string() {
    verify("declare const value:{present:number}; const result=value[\"missing\"];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type '\"missing\"' can't be used to index type '{ present: number; }'.\n  Property 'missing' does not exist on type '{ present: number; }'.", "value[\"missing\"]")]);
}

#[test]
fn interface_missing_string() {
    verify("interface I { present:number; } declare const value:I; const result=value[\"missing\"];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type '\"missing\"' can't be used to index type 'I'.\n  Property 'missing' does not exist on type 'I'.", "value[\"missing\"]")]);
}

#[test]
fn any_key() {
    verify("declare const key:any; const value={}; const result=value[key];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type 'any' can't be used to index type '{}'.", "value[key]")]);
}

#[test]
fn string_key() {
    verify("declare const key:string; const value={present:1}; const result=value[key];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type 'string' can't be used to index type '{ present: number; }'.\n  No index signature with a parameter of type 'string' was found on type '{ present: number; }'.", "value[key]")]);
}

#[test]
fn number_key() {
    verify("declare const key:number; const value={present:1}; const result=value[key];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type 'number' can't be used to index type '{ present: number; }'.\n  No index signature with a parameter of type 'number' was found on type '{ present: number; }'.", "value[key]")]);
}

#[test]
fn symbol_key() {
    verify("declare const key:symbol; const value={present:1}; const result=value[key];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type 'symbol' can't be used to index type '{ present: number; }'.", "value[key]")]);
}

#[test]
fn array_string_key() {
    verify("declare const key:string; const value=[1,2]; const result=value[key];", true, "any", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "key")]);
}

#[test]
fn array_missing_literal() {
    verify("const value=[1,2]; const result=value[\"missing\"];", true, "any", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "\"missing\"")]);
}

#[test]
fn known_numeric_string() {
    verify(
        "const value=[1,2]; const result=value[\"0\"];",
        true,
        "number",
        &[],
    );
}

#[test]
fn valid_union_keys() {
    verify(
        "declare const key:\"one\"|\"two\"; const value={one:1,two:2}; const result=value[key];",
        true,
        "number",
        &[],
    );
}

#[test]
fn union_keys_with_missing() {
    verify("declare const key:\"one\"|\"missing\"; const value={one:1,two:2}; const result=value[key];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type '\"one\" | \"missing\"' can't be used to index type '{ one: number; two: number; }'.\n  Property 'missing' does not exist on type '{ one: number; two: number; }'.", "value[key]")]);
}

#[test]
fn numeric_index_only() {
    verify("declare const value:{[key:number]:string}; declare const key:string; const result=value[key];", true, "any", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "key")]);
}

#[test]
fn numeric_enum_reverse_map_and_missing_key() {
    verify("enum E { A }; const result=E[0];", true, "string", &[]);
    verify("enum E { A }; const result=E[\"missing\"];", true, "any", &[(7015,
        "Element implicitly has an 'any' type because index expression is not of type 'number'.", "\"missing\"")]);
}

#[test]
fn index_errors_respect_no_implicit_any() {
    verify("const result = {}[\"missing\"];", false, "any", &[]);
    verify(
        "declare const key:any; const value={}; const result=value[key];",
        false,
        "any",
        &[],
    );
}

#[test]
fn dynamic_index_on_fresh_object_is_permitted() {
    verify(
        "declare const key:string;const result = {}[key];",
        true,
        "undefined",
        &[],
    );
}

#[test]
fn string_index_signature_accepts_string_number_and_any_keys() {
    for key_type in ["string", "number", "any"] {
        verify(&format!("declare const key:{key_type}; declare const value:{{[key:string]:number}}; const result=value[key];"), true, "number", &[]);
    }
}

#[test]
fn array_any_key() {
    verify(
        "declare const key:any;const result = [1][key];",
        true,
        "number",
        &[],
    );
}

#[test]
fn array_noncanonical_numeric_string() {
    verify("const result = [1][\"01\"];", true, "any", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "\"01\"")]);
}

#[test]
fn array_symbol_key() {
    verify("declare const key:symbol;const result = [1][key];", true, "any", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "key")]);
}

#[test]
fn array_known_length() {
    verify("const result = [1][\"length\"];", true, "number", &[]);
}

#[test]
fn inherited_string_domain() {
    verify("interface Base {[key:string]:number} interface Child extends Base {[key:number]:number} declare const value:Child;declare const key:string;const result=value[key];", true, "number", &[]);
}

#[test]
fn interface_both_domains() {
    verify("interface I {[key:string]:number;[key:number]:number} declare const value:I;declare const key:string;const result=value[key];", true, "number", &[]);
}

#[test]
fn class_missing_property() {
    verify("class C { present=1; } declare const value:C;const result=value[\"missing\"];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type '\"missing\"' can't be used to index type 'C'.\n  Property 'missing' does not exist on type 'C'.", "value[\"missing\"]")]);
}

#[test]
fn alias_missing_property() {
    verify("type Shape={present:number};declare const value:Shape;const result=value[\"missing\"];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type '\"missing\"' can't be used to index type 'Shape'.\n  Property 'missing' does not exist on type 'Shape'.", "value[\"missing\"]")]);
}

#[test]
fn numeric_string_enum() {
    verify("enum E {A}; const result=E[\"0\"];", true, "string", &[]);
}

#[test]
fn string_enum_missing() {
    verify("enum E {A=\"a\"};const result=E[\"missing\"];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type '\"missing\"' can't be used to index type 'typeof E'.\n  Property 'missing' does not exist on type 'typeof E'.", "E[\"missing\"]")]);
}

#[test]
fn misspelled_property() {
    verify("declare const value:{present:number};const result=value[\"presnt\"];", true, "any", &[(2551, "Property 'presnt' does not exist on type '{ present: number; }'. Did you mean 'present'?", "\"presnt\"")]);
}

#[test]
fn numeric_object_key() {
    verify("declare const value:{present:number};const result=value[3];", true, "any", &[(7053, "Element implicitly has an 'any' type because expression of type '3' can't be used to index type '{ present: number; }'.\n  Property '3' does not exist on type '{ present: number; }'.", "value[3]")]);
}

#[test]
fn for_in_numeric_keys() {
    verify(
        "const values=[1];let result=0;for(const key in values){result=values[key];}",
        true,
        "number",
        &[],
    );
}

#[test]
fn for_in_key_stays_string() {
    verify(
        "const values=[1];let result=\"\";for(const key in values){result=key;values[key];}",
        true,
        "string",
        &[],
    );
}

#[test]
fn for_in_shadowed_key() {
    verify("const values=[1];for(const key in values){function read(key:string){return values[key];}}const result=0;", true, "0", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "key")]);
}

#[test]
fn for_in_existing_variable() {
    verify(
        "const values=[1];let key:string;for(key in values){values[key];}const result=0;",
        true,
        "0",
        &[],
    );
}

#[test]
fn for_in_object_has_string_keys() {
    verify("const values=[1];for(const key in {item:1}){values[key];}const result=0;", true, "0", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "key")]);
}

#[test]
fn class_string_domain() {
    verify("class Values {[key:string]:number} declare const value:Values;value[\"item\"];const result=0;", true, "0", &[]);
}

#[test]
fn merged_index_domains() {
    verify("interface Values {[key:number]:number} interface Values {[key:string]:number} declare const value:Values;value[\"item\"];const result=0;", true, "0", &[]);
}

#[test]
fn computed_enum_literal_member() {
    verify(
        "enum E {[\"bar\"]=6} E[\"bar\"];const result=0;",
        true,
        "0",
        &[],
    );
}

#[test]
fn static_super_key() {
    verify("declare class Base {static method():void} class Derived extends Base {static run(){super[\"method\"]();}}const result=0;", true, "0", &[]);
}

#[test]
fn predicate_retains_original_index() {
    verify("declare function isObject(value:unknown):value is {};declare const value:Record<string,unknown>;if(isObject(value)){value[\"attr\"];}const result=0;", true, "0", &[]);
}

#[test]
fn function_expando_index() {
    verify(
        "function fn(){}const key=\"member\";fn[key]=1;fn[key];const result=0;",
        true,
        "0",
        &[],
    );
}

#[test]
fn fresh_property_suggestion() {
    verify(
        "const result={present:1}[\"presnt\"];",
        true,
        "undefined",
        &[(
            2339,
            "Property 'presnt' does not exist on type '{ present: number; }'.",
            "{present:1}[\"presnt\"]",
        )],
    );
}

#[test]
fn array_property_suggestion() {
    verify("const result=[1][\"lenght\"];", true, "any", &[(7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.", "\"lenght\"")]);
}

#[test]
fn for_of_mapped_keys() {
    verify("interface API {0:(x:0)=>string;1:(x:1)=>string} type Optional<T>={[K in keyof T]?:T[K]|null};const value:Optional<API>={};declare const keys:(keyof API)[];for(const key of keys){value[key]=()=>\"ok\";}const result=0;", true, "0", &[]);
}

#[test]
fn for_of_invalid_literal_key() {
    verify("declare const keys:\"missing\"[];const value={present:1};for(const key of keys){value[key];}const result=0;", true, "0", &[(7053, "Element implicitly has an 'any' type because expression of type '\"missing\"' can't be used to index type '{ present: number; }'.\n  Property 'missing' does not exist on type '{ present: number; }'.", "value[key]")]);
}

#[test]
fn for_of_any_key() {
    verify("declare const keys:any[];const value={};for(const key of keys){value[key];}const result=0;", true, "0", &[(7053, "Element implicitly has an 'any' type because expression of type 'any' can't be used to index type '{}'.", "value[key]")]);
}

#[test]
fn for_of_shadowed_key() {
    verify("declare const keys:\"present\"[];const value={present:1};for(const key of keys){function read(key:string){return value[key];}}const result=0;", true, "0", &[(7053, "Element implicitly has an 'any' type because expression of type 'string' can't be used to index type '{ present: number; }'.\n  No index signature with a parameter of type 'string' was found on type '{ present: number; }'.", "value[key]")]);
}

#[test]
fn fresh_const_property() {
    verify(
        "const result={present:1 as const}[\"missing\"];",
        true,
        "undefined",
        &[(
            2339,
            "Property 'missing' does not exist on type '{ present: 1; }'.",
            "{present:1 as const}[\"missing\"]",
        )],
    );
}
