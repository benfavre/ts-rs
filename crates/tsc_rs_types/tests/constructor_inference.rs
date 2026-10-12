// Expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    TypeChecker::new()
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                experimental_decorators: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

fn assert_error(source: &str, code: u32, message: &str, marked: &str) {
    let errors = check(source);
    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
    assert_eq!(errors[0].code, code, "{:?}", errors[0]);
    assert_eq!(errors[0].message, message);
    let span = errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], marked);
}

#[test]
fn constructor_prototype_provides_the_instance_inference_candidate() {
    assert_error("interface I<T> { prototype:T; m:()=>T; } declare function dec<T>(c:I<T>):void; @dec class C { _brand:any; static m() {} }", 1238,
        "Unable to resolve signature of class decorator when called as an expression.\n  Argument of type 'typeof C' is not assignable to parameter of type 'I<C>'.\n    The types returned by 'm()' are incompatible between these types.\n      Type 'void' is not assignable to type 'C'.", "dec");
}

#[test]
fn prototype_inference_also_applies_to_ordinary_calls() {
    assert_error("declare function extract<T>(c:{prototype:T}):T; class C { value=1; } const instance=extract(C); const wrong:string=instance.value;", 2322,
        "Type 'number' is not assignable to type 'string'.", "wrong");
}

#[test]
fn property_and_callback_candidates_use_a_common_supertype() {
    assert_error("declare function choose<T>(x:{value:T;get:()=>T}):T; declare const input:{value:number;get:()=>string}; const result=choose(input); const n:number=result;", 2345,
        "Argument of type '{ value: number; get: () => string; }' is not assignable to parameter of type '{ value: number; get: () => number; }'.\n  The types returned by 'get()' are incompatible between these types.\n    Type 'string' is not assignable to type 'number'.", "input");
    assert_error("declare function choose<T>(a:()=>T,b:()=>T):T; declare const a:()=>number; declare const b:()=>string; const result=choose(a,b); const n:number=result;", 2345,
        "Argument of type '() => string' is not assignable to parameter of type '() => number'.\n  Type 'string' is not assignable to type 'number'.", "b");
}

#[test]
fn decorator_constructor_comparisons_preserve_private_static_origins() {
    for source in [
        "declare function dec(target:typeof C):void; @dec class C { private static brand:any; }",
        "class Base { private static brand:any; } declare function dec(target:typeof Base):void; @dec class Derived extends Base {}",
    ] {
        assert!(check(source).is_empty(), "{source}: {:?}", check(source));
    }
    assert_error("class Base { private static brand:any; } declare function dec(target:typeof Base):void; @dec class Other { private static brand:any; }", 1238,
        "Unable to resolve signature of class decorator when called as an expression.\n  Argument of type 'typeof Other' is not assignable to parameter of type 'typeof Base'.\n    Types have separate declarations of a private property 'brand'.", "dec");
}

#[test]
fn decorator_constructor_comparisons_check_static_member_types() {
    assert_error("class Base { static tag:number; } declare function dec(target:typeof Base):void; @dec class Other { static tag:string; }", 1238,
        "Unable to resolve signature of class decorator when called as an expression.\n  Argument of type 'typeof Other' is not assignable to parameter of type 'typeof Base'.\n    Types of property 'tag' are incompatible.\n      Type 'string' is not assignable to type 'number'.", "dec");
}

#[test]
fn union_candidates_and_contextual_callbacks_keep_their_inference() {
    for source in [
        "declare function choose<T>(a:(x:T)=>T,b:(x:T)=>T):void; choose(x=>1,x=>\"text\");",
        "interface A { a:string; } interface B extends A { b:string; } interface C extends A { c:string; } declare function choose<T>(value:T|undefined):T; declare const value:B|C; const result:B|C=choose(value);",
    ] {
        let errors = check(source);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn callable_decorator_returns_do_not_report_a_missing_prototype() {
    assert_error("interface Callback { (target:any):void; } declare function dec(target:any):Callback; @dec class C {}", 1270,
        "Decorator function return type 'Callback' is not assignable to type 'void | typeof C'.", "dec");
}

#[test]
fn prototypes_distinguish_classes_from_construct_signatures() {
    let errors = check("declare const ctor:new()=>{value:number}; declare const queried:typeof ctor; declare function extract<T>(c:{prototype:T}):T; const result=extract(queried); const wrong:string=result.value;");
    assert!(errors.is_empty(), "{errors:?}");
    assert_error("class Box<T>{value!:T;} declare function extract<T>(c:{prototype:T}):T; const instance=extract(Box); const check:never=instance;", 2322,
        "Type 'Box<any>' is not assignable to type 'never'.", "check");
}
