use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn reserved_names(source: &str) -> Vec<(u32, String, String)> {
    let file = tsc_rs_parser::parse("reserved_type_names.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut diagnostics: Vec<_> = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
        .into_iter()
        .filter(|d| matches!(d.code, 2368 | 2414 | 2427 | 2431 | 2457))
        .map(|d| {
            let span = d.span.expect("reserved-name diagnostics need a span");
            (
                d.code,
                source[span.start as usize..span.end as usize].to_owned(),
                d.message,
            )
        })
        .collect();
    diagnostics.sort();
    diagnostics
}

#[test]
fn built_in_type_names_are_rejected_in_declarations() {
    for name in [
        "any",
        "unknown",
        "never",
        "number",
        "bigint",
        "boolean",
        "string",
        "symbol",
        "object",
        "undefined",
    ] {
        for (kind, code, source) in [
            ("Class", 2414, format!("class {name} {{}}")),
            ("Interface", 2427, format!("interface {name} {{}}")),
            ("Type alias", 2457, format!("type {name} = 1;")),
            ("Enum", 2431, format!("enum {name} {{}}")),
            ("Type parameter", 2368, format!("function f<{name}>() {{}}")),
        ] {
            assert_eq!(
                reserved_names(&source),
                vec![(
                    code,
                    name.to_owned(),
                    format!("{kind} name cannot be '{name}'.")
                )],
                "{source}"
            );
        }
    }
}

#[test]
fn valid_contextual_and_value_names_remain_allowed() {
    for name in ["intrinsic", "constructor", "Any", "Unknown"] {
        let source =
            format!("class {name} {{}}; interface I<{name}> {{}}; type T<{name}> = {name};");
        assert!(reserved_names(&source).is_empty(), "{source}");
    }
    let source = "const any = 1; function number(string: number) { return string; } namespace boolean {} class C { any = 1; string() {} }";
    assert!(reserved_names(source).is_empty());
}

#[test]
fn nested_exported_ambient_and_expression_declarations_are_checked() {
    let source = "export {}; namespace N { export interface any {} export type number = 1; export enum string {} } declare class boolean {} const C = class unknown {};";
    let got = reserved_names(source);
    assert_eq!(
        got.iter().map(|d| d.0).collect::<Vec<_>>(),
        vec![2414, 2414, 2427, 2431, 2457],
        "{got:?}"
    );
}

#[test]
fn generic_signatures_mapped_and_inferred_parameters_are_checked_once() {
    let source = "class C<any> { method<number>() {} } interface I { m<string>(): void; <boolean>(): void; new <symbol>(): I; } type A = <unknown>() => void; type B = new <never>() => I; type M = { [object in string]: 1 }; type F<T> = T extends infer bigint ? 1 : 2; const f = <undefined>() => 1; const o = { m<any>() {} };";
    let got = reserved_names(source);
    assert_eq!(got.len(), 11, "{got:?}");
    assert!(got.iter().all(|d| d.0 == 2368), "{got:?}");
}

#[test]
fn parameter_spans_exclude_modifiers_constraints_and_defaults() {
    let source = r"function f<const /* any */ any extends string = string>() {} interface I<out number extends string> {} function g<a\u006ey>() {}";
    let got = reserved_names(source);
    assert_eq!(got.len(), 3, "{got:?}");
    let mut spans: Vec<_> = got.iter().map(|d| d.1.as_str()).collect();
    spans.sort();
    assert_eq!(spans, [r"a\u006ey", "any", "number"]);
}
