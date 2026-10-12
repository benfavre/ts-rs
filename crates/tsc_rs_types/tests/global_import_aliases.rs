use tsc_rs_ast::{CompilerOptions, Diagnostic, ModuleKind, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str, providers: &[(&str, &str)], inject: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("subject.ts", source);
    let providers: Vec<_> = providers
        .iter()
        .map(|(name, source)| tsc_rs_parser::parse(name, source))
        .collect();
    let mut files: Vec<_> = providers.iter().collect();
    files.push(&file);
    for file in &files {
        assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    }
    let mut checker = TypeChecker::new();
    checker.register_available_files(
        &files
            .iter()
            .map(|file| file.file_name.clone())
            .collect::<Vec<_>>(),
    );
    if inject {
        checker.inject_external_types(&files);
    }
    checker.set_current_file_name("subject.ts");
    let mut errors = checker
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                module: Some(ModuleKind::CommonJS),
                strict: Some(false),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    errors.sort_by_key(|error| (error.span.map(|span| span.start), error.code));
    errors
}

fn assert_codes(errors: Vec<Diagnostic>, expected: &[u32]) {
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        expected,
        "{errors:?}"
    );
}

#[test]
fn merged_global_alias_retains_value_and_type_meanings() {
    for inject in [false, true] {
        assert_codes(check("export {}; namespace A { export const y = 34; export interface y { s: string } } declare global { export import x = A.y; } const ok: number = x; const bad: string = x; let good: x = { s: '' }; let badType: x = { s: 0 };", &[], inject), &[2322, 2322]);
    }
}

#[test]
fn non_exported_augmentation_alias_is_private() {
    for inject in [false, true] {
        assert_codes(check("export {}; namespace A { export const y = 34; } declare global { import hidden = A.y; } hidden;", &[], inject), &[2304]);
    }
}

#[test]
fn erased_generic_alias_retains_arguments_and_defaults() {
    for inject in [false, true] {
        assert_codes(check("export {}; namespace A { export interface T<V = string> { item: V } } declare global { export import Global = A.T; } let x: Global = { item: 0 }; let y: Global<number> = { item: 0 }; Global;", &[], inject), &[2322, 2693]);
    }
}

#[test]
fn namespace_alias_retains_member_types() {
    for inject in [false, true] {
        assert_codes(check("export {}; namespace A { export namespace N { export const v = 1; export interface T { s: string } } } declare global { export import Global = A.N; } const x: string = Global.v; const y: Global.T = { s: 0 };", &[], inject), &[2322, 2322]);
    }
}

#[test]
fn class_alias_retains_constructor_and_instance() {
    for inject in [false, true] {
        assert_codes(check("export {}; namespace A { export class C { value = 1; } } declare global { export import Global = A.C; } const c: Global = new Global(); const x: string = c.value;", &[], inject), &[2322]);
    }
}

#[test]
fn global_aliases_resolve_forward_chains() {
    for inject in [false, true] {
        assert_codes(check("export {}; declare global { export import x = Later.y; import Later = A; } namespace A { export const y = 34; } const bad: string = x;", &[], inject), &[2322]);
    }
}

#[test]
fn aliases_keep_provider_identity_across_module_local_namespaces() {
    assert_codes(check("export {}; namespace A { export interface T { value: boolean } } const good: First = { value: 'ok' }; const bad: First = { value: 1 }; const other: Second = { value: 1 }; const badOther: Second = { value: 'no' };", &[
        ("first.ts", "export {}; namespace A { export interface T { value: string } } declare global { export import First = A.T; }"),
        ("second.ts", "export {}; namespace A { export interface T { value: number } } declare global { export import Second = A.T; }"),
    ], true), &[2322, 2322]);
}

#[test]
fn aliases_do_not_expose_private_namespace_members() {
    for inject in [false, true] {
        assert_codes(check("export {}; namespace A { interface T { v: string } } declare global { export import G = A.T; } let g: G = { v: 0 };", &[], inject), &[2694]);
        assert_codes(check("export {}; namespace A { const v = 1; } declare global { export import G = A.v; } G;", &[], inject), &[2694]);
        assert_codes(check("export {}; namespace A { export namespace N { interface T { v: string }; export const x = 1; } } declare global { export import G = A.N; } let x: G.T = { v: 0 };", &[], inject), &[2694]);
    }
}

#[test]
fn class_alias_retains_static_members() {
    for inject in [false, true] {
        assert_codes(check("export {}; namespace A { export class C { static s = 1; v = 1; } } declare global { export import G = A.C; } const bad: string = G.s; const x: G = new G(); const wrong: string = x.v;", &[], inject), &[2322, 2322]);
    }
}

#[test]
fn global_aliases_preserve_value_types_in_other_files() {
    assert_codes(check("export {}; const good: number = Global.value; const bad: string = Global.value; const item: Global.Item = { value: 0 };", &[
        ("provider.ts", "export {}; namespace Local { export const value = 1; export interface Item { value: string } } declare global { export import Global = Local; }"),
    ], true), &[2322, 2322]);
}

#[test]
fn local_declarations_shadow_global_aliases() {
    let provider = "export {}; namespace Local { export interface Item { value: string } } declare global { export import Global = Local; export import Item = Local.Item; }";
    assert_codes(check("export {}; namespace Global { export interface Item { value: number } } interface Item { value: boolean } const a: Global.Item = { value: 0 }; const b: Item = { value: true };", &[("provider.ts", provider)], true), &[]);
}
