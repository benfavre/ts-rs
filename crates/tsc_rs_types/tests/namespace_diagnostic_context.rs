// Complete diagnostic codes and source spans from TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(source: &str, expected: &[(u32, u32, u32)]) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    for spans in [false, true] {
        let mut checker = TypeChecker::new();
        if spans {
            checker.enable_expression_type_spans();
        }
        let output = checker.check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(true),
                target: Some(ScriptTarget::ES2015),
                ..Default::default()
            },
        );
        let mut actual = output
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.span.unwrap();
                (d.code, span.start, span.end - span.start)
            })
            .collect::<Vec<_>>();
        actual.sort_by_key(|d| (d.1, d.0));
        assert_eq!(actual, expected, "{source}: {:?}", output.diagnostics);
    }
}

#[test]
fn template_string_segments() {
    verify(
        r###"type D = `${string}.${string}.${string}`; declare function spread<T extends D>(...args:T[]):T; function use<T extends string>(t:T,u:Uppercase<T>) { spread(`1.${t}.3`, `1.${u}.4`); }"###,
        &[],
    );
}

#[test]
fn template_number_segments() {
    verify(
        r###"declare let value:`${number}.${number}.${number}`; declare const part:number; value=`1.${part}.3`; value=`${part}.1.${part}`;"###,
        &[],
    );
}

#[test]
fn template_fixed_text_rejected() {
    verify(
        r###"declare let value:`left.${string}.right`; declare const part:string; value=`wrong.${part}.right`;"###,
        &[(2322, 69, 5)],
    );
}

#[test]
fn template_union_placeholder() {
    verify(
        r###"declare function parse<T extends string|number>(value:`**${T}**`):T; parse("**123**"); parse("**é漢**");"###,
        &[],
    );
}

#[test]
fn template_wrong_numeric_placeholder() {
    verify(
        r###"declare let value:`${number}.${number}.${number}`; declare const part:string; value=`1.${part}.3`;"###,
        &[(2322, 78, 5)],
    );
}

#[test]
fn readonly_variadic_arguments() {
    verify(
        r###"declare function middle<const T extends readonly any[]>(...args:readonly [unknown,...T,unknown]):T; const values=middle(1,2,3,4,5);"###,
        &[],
    );
}

#[test]
fn readonly_variadic_array_argument() {
    verify(
        r###"declare function middle<const T extends readonly any[]>(args:readonly [unknown,...T,unknown]):T; const values=middle([1,2,3,4,5]);"###,
        &[],
    );
}

#[test]
fn readonly_nested_rest_arguments() {
    verify(
        r###"declare function tail<S extends readonly [string,...string[]]>(...args:readonly [...S,number]):void; tail("a",1); tail("a","b",1);"###,
        &[],
    );
}

#[test]
fn readonly_nested_rest_wrong_suffix() {
    verify(
        r###"declare function tail<S extends readonly [string,...string[]]>(...args:readonly [...S,number]):void; tail("a","b");"###,
        &[(2345, 110, 3)],
    );
}

#[test]
fn readonly_tuple_suffix_assignment() {
    verify(
        r###"declare let target:readonly [string,...readonly number[],boolean]; target=["a",1,2,true]; target=["a",1,2,"bad"];"###,
        &[(2322, 90, 6)],
    );
}

#[test]
fn namespace_literal_callback_context() {
    verify(
        r###"namespace Severity { export const Error=1; } interface Item { severity?:1|2; message:string; } function build():Item[] { const values:any[]=[]; return values.map(value=>({severity:Severity.Error,message:"ok"})); }"###,
        &[],
    );
}

#[test]
fn namespace_literal_callback_error() {
    verify(
        r###"namespace Severity { export const Error=3; } interface Item { severity?:1|2; message:string; } function build():Item[] { const values:any[]=[]; return values.map(value=>({severity:Severity.Error,message:"ok"})); }"###,
        &[(2322, 144, 6)],
    );
}

#[test]
fn namespace_literal_block_callback() {
    verify(
        r###"namespace Severity { export const Error=1; } interface Item { severity?:1|2; message:string; } function build():Item[] { const values:any[]=[]; return values.map(value=>{ return {severity:Severity.Error,message:"ok"}; }); }"###,
        &[],
    );
}

#[test]
fn namespace_literal_block_callback_error() {
    verify(
        r###"namespace Severity { export const Error=3; } interface Item { severity?:1|2; message:string; } function build():Item[] { const values:any[]=[]; return values.map(value=>{ return {severity:Severity.Error,message:"ok"}; }); }"###,
        &[(2322, 144, 6)],
    );
}

#[test]
fn generic_tuple_suffix_inference() {
    verify(
        r###"declare function last<T,U>(value:[...unknown[],T,U]):[T,U]; const result:[number,string]=last([1,"a"]); const more:[string,boolean]=last([1,"a",true]);"###,
        &[],
    );
}

#[test]
fn tuple_literal_suffix_context() {
    verify(
        r###"declare const start:readonly ["x","y"]; const one:[...string[],"!"]=["!"]; const two:[...string[],"!"]=["a","!"]; const spread:[...string[],"!"]=[...start,"!"];"###,
        &[],
    );
}

#[test]
fn generic_variadic_parameter_identity() {
    verify(
        r###"function assign<T extends string[],U extends T>(t:[string,...T],u:[string,...U]) { u=t; }"###,
        &[(2322, 83, 1)],
    );
}

#[test]
fn generic_variadic_constraint_rejection() {
    verify(
        r###"function assign<T extends unknown[]>(target:[number,...T],fixed:[number,number],rest:[number,...number[]]) { target=fixed; target=rest; }"###,
        &[(2322, 109, 6), (2322, 123, 6)],
    );
}
