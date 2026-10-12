// Expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::{TypeCheckOutput, TypeChecker};

fn check(source: &str) -> TypeCheckOutput {
    check_with_variance(source, true)
}

fn check_with_variance(source: &str, strict_function_types: bool) -> TypeCheckOutput {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(true),
            strict_function_types: Some(strict_function_types),
            ..CompilerOptions::default()
        },
    )
}

fn assert_result_type(source: &str, output: &TypeCheckOutput, expected: &str) {
    let start = source.rfind("result;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&start).map(String::as_str),
        Some(expected),
        "{source}: {:?}",
        output.expression_types
    );
}

#[test]
fn overloaded_constructor_parameters_choose_a_common_subtype() {
    let source = "declare function choose<T>(ctor:{new(x:T):string;new(x:T,y?:T):string}):T; declare const ctor:{new(x:boolean):string;new(x:number):boolean}; const result=choose(ctor); result;";
    let output = check(source);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_result_type(source, &output, "boolean");
}

#[test]
fn overload_order_preserves_the_selected_type_and_reports_real_errors() {
    let source = "declare function choose<T>(ctor:{new(x:T):string;new(x:T,y?:T):string}):T; declare const ctor:{new(x:number):boolean;new(x:boolean):string}; const result=choose(ctor); result;";
    let output = check(source);
    assert_result_type(source, &output, "number");
    assert_eq!(output.diagnostics.len(), 1, "{:?}", output.diagnostics);
    let error = &output.diagnostics[0];
    assert_eq!(error.code, 2345);
    assert_eq!(error.message, "Argument of type '{ new (x: number): boolean; new (x: boolean): string; }' is not assignable to parameter of type '{ new (x: number): string; new (x: number, y?: number | undefined): string; }'.\n  Type 'boolean' is not assignable to type 'string'.");
    let span = error.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "ctor");
}

#[test]
fn overloaded_constructor_returns_choose_a_common_supertype() {
    let source = "declare function choose<T>(ctor:{new():T;new(x:number):T}):T; declare const ctor:{new():number;new(x:number):string}; const result=choose(ctor); result;";
    let output = check(source);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_result_type(source, &output, "number");
}

#[test]
fn union_candidates_within_a_constructor_signature_stay_unions() {
    for source in [
        "declare function choose<T>(ctor:new(x:T)=>object):T; declare const ctor:new(x:string|number)=>object; const result=choose(ctor); result;",
        "declare function choose<T>(ctor:new()=>T):T; declare const ctor:new()=>string|number; const result=choose(ctor); result;",
    ] {
        let output = check(source);
        assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
        assert_result_type(source, &output, "string | number");
    }
}

#[test]
fn constructor_parameter_inference_respects_strict_function_types() {
    for (first, second) in [("Base", "Derived"), ("Derived", "Base")] {
        let source = format!("interface Base {{ a:string; }} interface Derived extends Base {{ b:string; }} declare function choose<T>(ctor:{{new(x:T):object;new(x:T,y?:T):object}}):T; declare const ctor:{{new(x:{first}):object;new(x:{second}):object}}; const result=choose(ctor); result;");
        for (strict, expected) in [(true, "Derived"), (false, "Base")] {
            let output = check_with_variance(&source, strict);
            assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
            assert_result_type(&source, &output, expected);
        }
    }
}

#[test]
fn constructor_return_inference_must_satisfy_parameter_candidates() {
    let source = "interface Base { a:string; } interface Derived extends Base { b:string; } declare function choose<T>(ctor:new(x:T)=>T):T; declare const ctor:new(x:Base)=>Derived; const result=choose(ctor); result;";
    let output = check(source);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_result_type(source, &output, "Derived");

    let source = "interface Base { a:string; } interface Derived extends Base { b:string; } declare function choose<T>(ctor:new(x:T)=>T):T; declare const ctor:new(x:Derived)=>Base; const result=choose(ctor); result;";
    let output = check(source);
    assert_result_type(source, &output, "Derived");
    assert_eq!(output.diagnostics.len(), 1, "{:?}", output.diagnostics);
    let error = &output.diagnostics[0];
    assert_eq!(error.code, 2345);
    assert_eq!(error.message, "Argument of type 'new (x: Derived) => Base' is not assignable to parameter of type 'new (x: Derived) => Derived'.\n  Property 'b' is missing in type 'Base' but required in type 'Derived'.");
    let span = error.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "ctor");
}

#[test]
fn constructor_parameter_candidates_preserve_never_and_literal_types() {
    for (first, second, expected) in [
        ("never", "number", "never"),
        ("number", "never", "never"),
        ("number", "42", "42"),
    ] {
        let source = format!("declare function choose<T>(ctor:{{new(x:T):object;new(x:T,y?:T):object}}):T; declare const ctor:{{new(x:{first}):object;new(x:{second}):object}}; const result=choose(ctor); result;");
        let output = check(&source);
        assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
        assert_result_type(&source, &output, expected);
    }
}

#[test]
fn constructor_inference_keeps_context_sensitive_callbacks_unchanged() {
    let source = "declare function f<T,U>(t:T,u:U,a:(u:U)=>T,b:(t:T)=>U):[T,U]; interface A { a:A; } interface B extends A { b:any; } declare const a:A,b:B; const result=f(a,b,u=>u.b,t=>t); result;";
    let output = check_with_variance(source, false);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}
