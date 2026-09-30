use tsc_rs_symbols::{bind, generate_symbols_baseline};

#[test]
fn member_symbols_include_their_lexically_visible_owner() {
    let source = "namespace N {\n export class C { p: number; #secret = 1; }\n export interface I { readonly q: number; 'x-y': string; }\n export enum E { A, B = A }\n}";
    let file = tsc_rs_parser::parse("members.ts", source);
    assert!(file.diagnostics.is_empty());
    let table = bind(&file);
    let baseline = generate_symbols_baseline(&file, &table);
    // Display strings independently checked with TypeScript 6.0.3.
    for expected in [
        ">C : Symbol(C,",
        ">p : Symbol(C.p,",
        ">#secret : Symbol(C.#secret,",
        ">q : Symbol(I.q,",
        ">'x-y' : Symbol(I[\"x-y\"],",
        ">A : Symbol(E.A,",
        ">B : Symbol(E.B,",
    ] {
        assert!(baseline.contains(expected), "{expected}: {baseline}");
    }
    assert_eq!(baseline.matches(">A : Symbol(E.A,").count(), 2);
    for name in ["q", "A", "B"] {
        let symbol = table
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap();
        let declaration = &symbol.declarations[0];
        assert_eq!(
            &source[declaration.span.start as usize..declaration.span.end as usize],
            name
        );
    }
    let reference = table
        .scope_graph
        .reference_at(source.rfind("A }").unwrap() as u32)
        .unwrap();
    assert_eq!(table.symbols[reference.symbol_id as usize].name, "A");
}

#[test]
fn overload_baselines_limit_declarations_and_report_the_remainder() {
    let source = "function f(x: 1): void;\nfunction f(x: 2): void;\nfunction f(x: 3): void;\nfunction f(x: 4): void;\nfunction f(x: 5): void;\nfunction f(x: number) {}";
    let file = tsc_rs_parser::parse("overloads.ts", source);
    let baseline = generate_symbols_baseline(&file, &bind(&file));
    let annotation = baseline
        .lines()
        .find(|line| line.starts_with(">f :"))
        .unwrap();
    assert_eq!(annotation.matches("Decl(").count(), 5);
    assert!(annotation.ends_with(" ... and 1 more)"), "{annotation}");
}

#[test]
fn namespace_exports_use_the_recorded_scope_when_spans_coincide() {
    let source = "namespace N { export const x = 1; x; }";
    let file = tsc_rs_parser::parse("namespace.ts", source);
    let table = bind(&file);
    let baseline = generate_symbols_baseline(&file, &table);
    assert_eq!(baseline.matches(">x : Symbol(x,").count(), 2, "{baseline}");
    assert!(!baseline.contains("Symbol(N.x,"), "{baseline}");
}

#[test]
fn string_import_names_do_not_replace_the_local_binding_label() {
    let source = "import { 'missing' as x } from 'package'; x;";
    let file = tsc_rs_parser::parse("imports.ts", source);
    let baseline = generate_symbols_baseline(&file, &bind(&file));
    assert_eq!(baseline.matches(">x : Symbol(x,").count(), 2, "{baseline}");
    assert!(!baseline.contains(">'missing' as x"), "{baseline}");
}
