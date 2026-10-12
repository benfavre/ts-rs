use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str, implicit_any: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("globals.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    TypeChecker::new()
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                no_implicit_any: Some(implicit_any),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

#[test]
fn global_dot_and_bracket_access_report_their_own_errors() {
    let source = "globalThis.ghost; this.ghost; globalThis['ghost']; this['ghost'];";
    assert!(check(source, false).is_empty());
    let errors = check(source, true);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [7017, 7017, 7053, 7053],
        "{errors:?}"
    );
    for (error, span) in
        errors
            .iter()
            .zip(["ghost", "ghost", "globalThis['ghost']", "this['ghost']"])
    {
        let actual = error.span.unwrap();
        assert_eq!(&source[actual.start as usize..actual.end as usize], span);
    }
    assert_eq!(errors[2].message, "Element implicitly has an 'any' type because expression of type '\"ghost\"' can't be used to index type 'typeof globalThis'.\n  Property 'ghost' does not exist on type 'typeof globalThis'.");
}

#[test]
fn global_intersections_keep_numeric_index_restrictions() {
    let source = "declare let view: Window & typeof globalThis; view.ghost; view['ghost']; view[0]; view.document; view.addEventListener;";
    for (implicit, expected) in [(false, vec![2339]), (true, vec![2339, 7015])] {
        let errors = check(source, implicit);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            expected,
            "{errors:?}"
        );
        assert_eq!(
            errors[0].message,
            "Property 'ghost' does not exist on type 'Window & typeof globalThis'."
        );
    }
}

#[test]
fn intersection_members_are_checked_against_numeric_index_domains() {
    let source = "interface Items { [n: number]: number; named: string } declare const values: Items & { tag: boolean }; values.ghost; values['ghost']; values['0']; values.named; values.tag;";
    let errors = check(source, true);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2339, 7015],
        "{errors:?}"
    );
}

#[test]
fn global_reads_ignore_local_shadowing_and_respect_bound_receivers() {
    let source = "var shared: number; function f(shared: string) { const n: number = globalThis.shared; } function g(globalThis: { known: number }) { const n: number = globalThis.known; } const object = { method(this: { field: number }) { const n: number = this.field; } }; class C { field = 1; method() { this.field; } }";
    let errors = check(source, true);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn lexical_globals_are_not_global_object_properties() {
    let source =
        "let local: number; globalThis.local; globalThis['local']; this.local; this['local'];";
    for implicit in [true, false] {
        let errors = check(source, implicit);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [2339, 2339, 2339, 2339],
            "{errors:?}"
        );
    }
}

#[test]
fn library_intersections_include_inherited_members_and_indices() {
    let source = "declare const list: HTMLCollectionOf<Element> & { tag: boolean }; list.length; list.namedItem('x'); list.ghost; list['ghost']; list[0];";
    let errors = check(source, true);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2339, 7015],
        "{errors:?}"
    );
}

#[test]
fn intersection_pattern_indices_accept_only_matching_property_names() {
    let source = "type Pattern = { [key: `data${string}`]: number }; declare const value: Pattern & { tag: boolean }; value.dataValue; value['dataValue']; value.other; value['other'];";
    let errors = check(source, true);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2339, 7053],
        "{errors:?}"
    );
    assert!(
        errors[1]
            .message
            .ends_with("Property 'other' does not exist on type 'Pattern & { tag: boolean; }'."),
        "{:?}",
        errors[1]
    );
}

#[test]
fn numeric_index_names_follow_javascript_number_spelling() {
    let source = "declare const view: Window & typeof globalThis; view['NaN']; view['Infinity']; view['-Infinity']; view['0']; view['01'];";
    let errors = check(source, true);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [7015],
        "{errors:?}"
    );
    let span = errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "'01'");
}

#[test]
fn global_self_property_is_readonly_through_direct_and_aliased_receivers() {
    let source = "globalThis.globalThis = {} as any; globalThis['globalThis'] = {} as any; this.globalThis = {} as any; declare let alias: typeof globalThis; alias.globalThis = {} as any; const inferred = globalThis; inferred.globalThis = {} as any;";
    for implicit in [false, true] {
        let errors = check(source, implicit);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [2540, 2540, 2540, 2540, 2540],
            "{errors:?}"
        );
        for (error, span) in errors.iter().zip([
            "globalThis",
            "'globalThis'",
            "globalThis",
            "globalThis",
            "globalThis",
        ]) {
            assert_eq!(
                error.message,
                "Cannot assign to 'globalThis' because it is a read-only property."
            );
            let actual = error.span.unwrap();
            assert_eq!(&source[actual.start as usize..actual.end as usize], span);
        }
    }
}

#[test]
fn shadowed_global_self_properties_remain_writable() {
    let source = "function f(globalThis: { globalThis: number }) { globalThis.globalThis = 1; globalThis['globalThis'] = 2; } const local = { globalThis: 0 }; local.globalThis = 3;";
    let errors = check(source, true);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn a_local_function_named_global_this_keeps_its_own_properties() {
    let source = "function outer() { function globalThis() {} globalThis.globalThis = 1; const alias: typeof globalThis = globalThis; alias.globalThis = 2; }";
    let errors = check(source, true);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn global_indexed_types_reject_lexical_and_missing_properties() {
    let source = "var visible: number; const hidden = 1; type G = typeof globalThis; type A = G['hidden']; type B = (typeof globalThis)['missing']; type C = G['visible' | 'missing'];";
    for implicit in [false, true] {
        let errors = check(source, implicit);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [2339, 2339, 2339],
            "{errors:?}"
        );
        for (error, expected) in
            errors
                .iter()
                .zip(["'hidden'", "'missing'", "'visible' | 'missing'"])
        {
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], expected);
        }
    }
}

#[test]
fn global_indexed_types_preserve_known_property_types() {
    let source = "var visible: number; type G = typeof globalThis; const typed: G['visible'] = 'bad'; const globalSelf: G['globalThis'] = globalThis; const inferred = globalThis; const n: number = inferred.visible; const recursive: number = globalThis.globalThis.visible;";
    let errors = check(source, true);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2322],
        "{errors:?}"
    );
    assert_eq!(
        errors[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

#[test]
fn global_indexed_types_report_broad_and_invalid_indices() {
    let source = "type G = typeof globalThis; type A = G[string]; type B = G[number]; type C = G[boolean]; type D = G[any]; type E = G[symbol]; type F = G[never];";
    let errors = check(source, true);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2537, 2537, 2538, 2538, 2538],
        "{errors:?}"
    );
}

#[test]
fn import_equals_global_aliases_keep_property_types_and_readonly_members() {
    for alias in [
        "import G = globalThis;",
        "import H = globalThis; import G = H;",
        "import G = H; import H = globalThis;",
        "import G = globalThis.globalThis;",
    ] {
        let source = format!("var shared: number; {alias} const bad: string = G.shared; G.globalThis = 1; G.missing;");
        let errors = check(&source, true);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [2322, 2540, 7017],
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn local_global_this_declarations_shadow_import_alias_targets() {
    let errors = check(
        "export {}; const globalThis = 1; import G = globalThis;",
        true,
    );
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2437],
        "{errors:?}"
    );
    let errors = check("export {}; namespace globalThis { export const field = 1; } import G = globalThis; const n: number = G.field;", true);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn exported_global_object_aliases_keep_their_types_across_files() {
    for (source, import) in [
        (
            "import G = globalThis; export { G as value };",
            "import { value } from './provider';",
        ),
        (
            "import G = H; import H = globalThis; export { G as value };",
            "import { value } from './provider';",
        ),
        (
            "export import value = globalThis;",
            "import { value } from './provider';",
        ),
        (
            "import G = globalThis; export default G;",
            "import value from './provider';",
        ),
        (
            "import G = globalThis; export = G;",
            "import value = require('./provider');",
        ),
    ] {
        let provider = tsc_rs_parser::parse("provider.ts", source);
        let consumer = tsc_rs_parser::parse(
            "consumer.ts",
            &format!("{import} value.globalThis = 1; value.missing;"),
        );
        assert!(
            provider.diagnostics.is_empty(),
            "{:?}",
            provider.diagnostics
        );
        assert!(
            consumer.diagnostics.is_empty(),
            "{:?}",
            consumer.diagnostics
        );
        let mut checker = TypeChecker::new();
        checker.register_available_files(&["provider.ts".into(), "consumer.ts".into()]);
        checker.register_virtual_module_paths(
            "consumer.ts",
            &[("./provider".into(), "provider.ts".into())],
        );
        checker.inject_external_types(&[&consumer, &provider]);
        checker.set_current_file_name("consumer.ts");
        let errors = checker
            .check_with_options(
                &consumer,
                &tsc_rs_symbols::bind(&consumer),
                &CompilerOptions {
                    target: Some(ScriptTarget::ES2015),
                    strict: Some(false),
                    no_implicit_any: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .diagnostics;
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [2540, 7017],
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn global_object_import_aliases_have_value_and_namespace_meanings() {
    let errors = check("import G = globalThis; var G;", false);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2440],
        "{errors:?}"
    );
    let errors = check("import G = globalThis; type T = G;", false);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2709],
        "{errors:?}"
    );
}

#[test]
fn global_object_alias_resolution_preserves_errors_and_lexical_shadowing() {
    for (source, expected) in [
        ("import G = H; import H = G;", vec![2303]),
        ("import G = Absent;", vec![2503]),
        ("export {}; namespace globalThis { export const own=1; } import G = globalThis; G.globalThis;", vec![2339]),
        ("import G = globalThis; function f<G>() { let x: G; }", vec![]),
    ] {
        let errors = check(source, false);
        assert_eq!(errors.iter().map(|error| error.code).collect::<Vec<_>>(), expected, "{source}: {errors:?}");
    }
}
