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

fn assert_return_error(source: &str, code: u32, expected: &str) {
    let errors = check(source);
    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
    let error = &errors[0];
    assert_eq!(error.code, code, "{error:?}");
    assert_eq!(error.message, expected);
    let span = error.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "dec");
}

#[test]
fn decorator_results_are_checked_at_every_legacy_site() {
    for (source, code, expected) in [
        ("declare function dec(a:any): number; @dec class C {}", 1270,
         "Decorator function return type 'number' is not assignable to type 'void | typeof C'."),
        ("declare function dec(a:any,b:any,c:any): number; class C { @dec m() {} }", 1270,
         "Decorator function return type 'number' is not assignable to type 'void | TypedPropertyDescriptor<() => void>'."),
        ("declare function dec(a:any,b:any,c:any): number; class C { @dec get p(): string { return ''; } }", 1270,
         "Decorator function return type 'number' is not assignable to type 'void | TypedPropertyDescriptor<string>'."),
        ("declare function dec(a:any,b:any): number; class C { @dec p: any; }", 1271,
         "Decorator function return type is 'number' but is expected to be 'void' or 'any'."),
        ("declare function dec(a:any,b:any,c:any): number; class C { @dec accessor p = 1; }", 1271,
         "Decorator function return type is 'number' but is expected to be 'void' or 'any'."),
        ("declare function dec(a:any,b:any,c:any): number; class C { constructor(@dec p:any) {} }", 1271,
         "Decorator function return type is 'number' but is expected to be 'void' or 'any'."),
        ("declare function dec(a:any,b:any,c:any): number; class C { m(@dec p:any) {} }", 1271,
         "Decorator function return type is 'number' but is expected to be 'void' or 'any'."),
    ] {
        assert_return_error(source, code, expected);
    }
}

#[test]
fn generic_return_inference_uses_the_decorated_target_even_after_an_arity_error() {
    let errors = check("declare function dec<T>(target:T): T; class C { @dec m() {} }");
    assert_eq!(
        errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        [1241, 1270],
        "{errors:?}"
    );
    assert_eq!(errors[1].message, "Decorator function return type 'C' is not assignable to type 'void | TypedPropertyDescriptor<() => void>'.");
}

#[test]
fn void_any_never_and_identity_class_decorators_are_valid() {
    for returned in ["void", "any", "never", "undefined"] {
        let source = format!("declare function dec(...args:any[]): {returned}; @dec class C {{ @dec p:any; @dec m(@dec p:any) {{}} }}");
        assert!(check(&source).is_empty(), "{source}: {:?}", check(&source));
    }
    let source = "declare function dec<T>(target:T):T; @dec class C { n = 1; }";
    assert!(check(source).is_empty(), "{:?}", check(source));
}

#[test]
fn decorator_overload_selection_uses_runtime_target_key_and_parameter_index() {
    for source in [
        "declare function dec(target:any,key:undefined,index:0):void; declare function dec(target:any,key:undefined,index:1):number; class C { constructor(a:any,@dec b:any) {} }",
        "declare function dec(target:any,key:'a'):void; declare function dec(target:any,key:'b'):number; class C { @dec b:any; }",
        "declare function dec(target:any,key:string):void; declare function dec(target:any,key:'b'):number; class C { @dec b:any; }",
        "declare const tag:unique symbol; declare function dec(target:any,key:typeof tag):number; class C { @dec [tag]:any; }",
        "declare function dec(target:C,key:string):void; declare function dec(target:typeof C,key:string):number; class C { x=1; @dec static b:any; }",
    ] {
        assert_return_error(source, 1271, "Decorator function return type is 'number' but is expected to be 'void' or 'any'.");
    }
}

#[test]
fn descriptor_return_type_uses_static_and_computed_member_types() {
    for source in [
        "declare function dec(...args:any[]):number; class C { @dec static m():number { return 1; } }",
        "declare function dec(...args:any[]):number; class C { @dec ['m']():number { return 1; } }",
    ] {
        assert_return_error(source, 1270, "Decorator function return type 'number' is not assignable to type 'void | TypedPropertyDescriptor<() => number>'.");
    }
}

#[test]
fn descriptor_results_are_inferred_and_structurally_checked() {
    let valid = "declare function dec<T>(a:any,b:any,d:TypedPropertyDescriptor<T>):TypedPropertyDescriptor<T>; class C { @dec m() {} }";
    assert!(check(valid).is_empty(), "{:?}", check(valid));
    assert_return_error(
        "declare function dec(a:any,b:any,c:any): {value: number}; class C { @dec m() {} }",
        1270,
        "Decorator function return type '{ value: number; }' is not assignable to type 'void | TypedPropertyDescriptor<() => void>'.\n  Type '{ value: number; }' is not assignable to type 'TypedPropertyDescriptor<() => void>'.\n    Types of property 'value' are incompatible.\n      Type 'number' is not assignable to type '() => void'.",
    );
}

#[test]
fn uninferred_result_parameters_default_to_unknown() {
    assert_return_error(
        "declare function dec<T>(...args:any[]):T; @dec class C {}",
        1270,
        "Decorator function return type 'unknown' is not assignable to type 'void | typeof C'.",
    );
}

#[test]
fn failed_overload_resolution_does_not_choose_an_arbitrary_return() {
    let errors = check(
        "declare function dec():number; declare function dec(a:any,b:any):string; @dec class C {}",
    );
    assert_eq!(
        errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        [1238],
        "{errors:?}"
    );
}

#[test]
fn parameter_index_excludes_this_and_setters_require_void_results() {
    for source in [
        "declare function dec(a:any,b:any,c:0):number; class C { m(this:C, @dec p:any) {} }",
        "declare function dec(a:any,b:any,c:any):number; class C { set p(@dec value:number) {} }",
    ] {
        assert_return_error(
            source,
            1271,
            "Decorator function return type is 'number' but is expected to be 'void' or 'any'.",
        );
    }
}

#[test]
fn interfaces_inheriting_library_decorator_aliases_retain_their_call_signatures() {
    assert_return_error(
        "interface AllDecorators extends MethodDecorator, ClassDecorator, PropertyDecorator {} declare function dec(...args:any[]):AllDecorators; class C { @dec m() {} }",
        1270,
        "Decorator function return type 'AllDecorators' is not assignable to type 'void | TypedPropertyDescriptor<() => void>'.",
    );
    let valid = "declare const cls:ClassDecorator; declare const prop:PropertyDecorator; declare const method:MethodDecorator; declare const param:ParameterDecorator; @cls class C { @prop p:any; @method m(@param p:any) {} constructor(@param p:any) {} }";
    assert!(check(valid).is_empty(), "{:?}", check(valid));
}
