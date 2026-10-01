// Full semantic diagnostics verified against TypeScript 6.0.3.
use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;
fn check(source: &str, expected: &[(u32, u32, u32, &str)]) {
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            no_lib: Some(false),
            strict: Some(true),
            target: Some(tsc_rs_ast::ScriptTarget::ES2015),
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| {
            let span = d.span.expect("unexpected global diagnostic");
            (
                d.code,
                span.start,
                span.end - span.start,
                d.message.as_str(),
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn merged_namespace_relations() {
    check("class C { value=1; static n=1; } namespace C { export const s=\"x\"; } class Base { p: undefined; } class D extends Base { p: typeof C; } interface Bad {[key:string]: undefined; c:typeof C;} const n:number=C.n; const s:string=C.s; const Alias=C; const instance:C=new Alias();", &[
(2416,121,1,"Property 'p' in type 'D' is not assignable to the same property in base type 'Base'.\n  Type 'typeof C' is not assignable to type 'undefined'."),
(2564,121,1,"Property 'p' has no initializer and is not definitely assigned in the constructor."),
(2411,176,1,"Property 'c' of type 'typeof C' is not assignable to 'string' index type 'undefined'."),
]);
}

#[test]
fn merged_namespace_ambient_order() {
    check("declare namespace C {const extra: string;} declare class C { static n:number; } const a:string=C.extra; const b:number=C.n;", &[
]);
}

#[test]
fn merged_namespace_constructor() {
    check("class C { static n=1; constructor(public p:number){} } namespace C {export const s=\"x\";} const ctor:new(p:number)=>C=C; const withProps:{n:number;s:string}=C; const alias=C; const value:C=new alias(1);", &[
]);
}

#[test]
fn merged_namespace_enum_index() {
    check("enum E { A } class C {} namespace C { export const extra=1; } interface I { [key:string]:typeof C; member:E; } interface U { [key:string]:string|boolean; member:E; } interface V { [key:string]:number|string; member:E; }", &[
(2411,99,6,"Property 'member' of type 'E' is not assignable to 'string' index type 'typeof C'."),
(2411,154,6,"Property 'member' of type 'E' is not assignable to 'string' index type 'string | boolean'."),
]);
}

#[test]
fn merged_namespace_enum_coverage() {
    check(
        "enum E { A=0, B=1 } interface I { [key:string]:0|1; member:E; } const covered:0|1=0 as E;",
        &[],
    );
}
