use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str, strict: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    TypeChecker::new()
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                experimental_decorators: Some(true),
                strict: Some(strict),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

fn assert_argument(source: &str, code: u32, kind: &str, detail: &str, marked: &str) -> Diagnostic {
    let errors = check(source, true);
    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
    let error = errors.into_iter().next().unwrap();
    assert_eq!(error.code, code);
    assert_eq!(
        error.message,
        format!(
            "Unable to resolve signature of {kind} decorator when called as an expression.\n  {}",
            detail.replace('\n', "\n  ")
        )
    );
    let span = error.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], marked);
    error
}

#[test]
fn instance_targets_do_not_satisfy_function_parameters() {
    assert_argument("declare function dec(a:Function,b:number):void; class C { @dec m() {} }", 1241, "method",
        "Argument of type 'C' is not assignable to parameter of type 'Function'.\n  Type 'C' is missing the following properties from type 'Function': apply, call, bind, prototype, and 5 more.", "dec");
    let source = "declare function dec(a:Function,b:string):void; class C { @dec static m() {} }";
    assert!(check(source, true).is_empty(), "{:?}", check(source, true));
}

#[test]
fn constructor_keys_are_undefined_and_parameter_indices_are_literals() {
    let source = "declare function dec(a:Object,b:string|symbol,c:number):void; class C { constructor(@dec p:any) {} }";
    assert_argument(
        source,
        1239,
        "parameter",
        "Argument of type 'undefined' is not assignable to parameter of type 'string | symbol'.",
        "dec",
    );
    assert!(
        check(source, false).is_empty(),
        "{:?}",
        check(source, false)
    );
    assert_argument(
        "declare function dec(a:any,b:any,c:0):void; class C { constructor(a:any,@dec b:any) {} }",
        1239,
        "parameter",
        "Argument of type '1' is not assignable to parameter of type '0'.",
        "dec",
    );
}

#[test]
fn property_keys_preserve_literal_context_and_widen_for_other_targets() {
    for (source, detail) in [
        (
            "declare function dec(a:any,b:'other'):void; class C { @dec p:any; }",
            "Argument of type '\"p\"' is not assignable to parameter of type '\"other\"'.",
        ),
        (
            "declare function dec(a:any,b:'other'):void; class C { @dec ['p']:any; }",
            "Argument of type 'string' is not assignable to parameter of type '\"other\"'.",
        ),
        (
            "declare function dec(a:any,b?:number):void; class C { @dec p:any; }",
            "Argument of type 'string' is not assignable to parameter of type 'number'.",
        ),
    ] {
        assert_argument(source, 1240, "property", detail, "dec");
    }
}

#[test]
fn descriptor_arguments_retain_the_member_type() {
    assert_argument("declare function dec(a:any,b:any,c:TypedPropertyDescriptor<number>):void; class C { @dec m() {} }", 1241, "method",
        "Argument of type 'TypedPropertyDescriptor<() => void>' is not assignable to parameter of type 'TypedPropertyDescriptor<number>'.\n  Types of property 'value' are incompatible.\n    Type '(() => void) | undefined' is not assignable to type 'number | undefined'.\n      Type '() => void' is not assignable to type 'number'.", "dec");
}

#[test]
fn decorators_are_called_with_a_void_this_context() {
    assert_argument(
        "declare function dec(this:number,a:any,b:any):void; class C { @dec p:any; }",
        1240,
        "property",
        "The 'this' context of type 'void' is not assignable to method's 'this' of type 'number'.",
        "@dec",
    );
}

#[test]
fn overload_errors_show_only_candidates_of_the_right_arity() {
    assert_argument("declare function dec(a:number):number; declare function dec(a:string,b:string):string; @dec class C {}", 1238, "class",
        "Argument of type 'typeof C' is not assignable to parameter of type 'number'.", "dec");
    assert_argument("declare function dec(a:number):number; declare function dec(a:string):string; @dec class C {}", 1238, "class",
        "No overload matches this call.\n  Overload 1 of 2, '(a: number): number', gave the following error.\n    Argument of type 'typeof C' is not assignable to parameter of type 'number'.\n  Overload 2 of 2, '(a: string): string', gave the following error.\n    Argument of type 'typeof C' is not assignable to parameter of type 'string'.", "dec");
}

#[test]
fn large_overload_sets_point_to_the_last_declaration() {
    let source = "declare function dec(a:number):number; declare function dec(a:string):string; declare function dec(a:boolean):boolean; declare function dec(a:symbol):symbol; @dec class C {}";
    let error = assert_argument(source, 1238, "class", "No overload matches this call.\n  The last overload gave the following error.\n    Argument of type 'typeof C' is not assignable to parameter of type 'symbol'.", "dec");
    let related = error.related.unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].code, 2771);
    assert_eq!(related[0].message, "The last overload is declared here.");
    let span = related[0].span.unwrap();
    assert_eq!(span.start as usize, source.rfind("dec(a:symbol)").unwrap());
    assert_eq!(&source[span.start as usize..span.end as usize], "dec");
}

#[test]
fn generic_constraints_control_arguments_and_recovery_returns() {
    let errors = check(
        "declare function dec<T extends number>(a:T):T; @dec class C {}",
        true,
    );
    assert_eq!(
        errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        [1238, 1270],
        "{errors:?}"
    );
    assert_eq!(errors[0].message, "Unable to resolve signature of class decorator when called as an expression.\n  Argument of type 'typeof C' is not assignable to parameter of type 'number'.");
    assert_eq!(
        errors[1].message,
        "Decorator function return type 'number' is not assignable to type 'void | typeof C'."
    );
}

#[test]
fn generic_library_and_tuple_rest_decorators_accept_valid_arguments() {
    for source in [
        "declare const dec:MethodDecorator; class C { @dec m() {} }",
        "declare function dec<T>(a:T,b:string,c:TypedPropertyDescriptor<()=>void>):void; class C { @dec m() {} }",
        "declare function dec(...args:[any,string]):void; class C { @dec p:any; }",
        "declare function dec(a:number,b:string):void; declare function dec(a:object,b:string):void; class C { @dec p:any; }",
    ] {
        assert!(check(source, true).is_empty(), "{source}: {:?}", check(source,true));
    }
}
