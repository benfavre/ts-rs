use tsc_rs_ast::{CompilerOptions, Diagnostic, JsxEmit, ModuleKind, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(file_name: &str, source: &str, force_module: bool) -> Vec<Diagnostic> {
    check_options(
        file_name,
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(false),
            module_detection: force_module.then(|| "force".into()),
            ..CompilerOptions::default()
        },
        None,
    )
}

fn check_options(
    file_name: &str,
    source: &str,
    options: CompilerOptions,
    format: Option<ModuleKind>,
) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    assert!(
        !tsc_rs_parser::has_syntax_errors(&file.diagnostics),
        "{:?}",
        file.diagnostics
    );
    let mut errors = file.diagnostics.clone();
    let mut checker = TypeChecker::new();
    checker.set_current_file_module_format(format);
    errors.extend(
        checker
            .check_with_options(&file, &tsc_rs_symbols::bind(&file), &options)
            .diagnostics,
    );
    errors.sort_by_key(|error| (error.span.map(|span| span.start), error.code));
    errors
}

fn codes(errors: &[Diagnostic]) -> Vec<u32> {
    errors.iter().map(|error| error.code).collect()
}

#[test]
fn script_augmentations_report_context_and_ambient_requirements() {
    for (source, expected) in [
        ("declare global {}", vec![2669]),
        ("global {}", vec![2669, 2670]),
        ("declare /* comment */ global {}", vec![2669]),
    ] {
        let errors = check("script.ts", source, false);
        assert_eq!(codes(&errors), expected, "{errors:?}");
        for error in errors {
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], "global");
        }
    }
}

#[test]
fn external_modules_allow_declared_global_augmentations() {
    for source in [
        "export {}; declare global {}",
        "export = {}; declare global {}",
        "import A = require('m'); declare global {}",
        "declare module 'm' { global {} }",
        "namespace global {}",
    ] {
        let errors = check("module.ts", source, false);
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
    let errors = check("module.ts", "export {}; global {}", false);
    assert_eq!(codes(&errors), [2670], "{errors:?}");
}

#[test]
fn namespace_and_global_bodies_are_not_external_module_containers() {
    for source in [
        "declare namespace A { global {} }",
        "export {}; namespace A.B { declare global {} }",
        "export {}; declare global { global {} }",
        "namespace N {} import A = N; declare global {}",
    ] {
        let errors = check("nested.ts", source, false);
        assert_eq!(codes(&errors), [2669], "{source}: {errors:?}");
    }
}

#[test]
fn blocks_and_functions_do_not_inherit_module_declaration_context() {
    for source in [
        "export {}; { declare global {} }",
        "export {}; function f() { declare global {} }",
        "export {}; const f = () => { declare global {} };",
    ] {
        let errors = check("nested.ts", source, false);
        assert_eq!(codes(&errors), [1234], "{source}: {errors:?}");
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "declare");
    }
}

#[test]
fn declaration_files_supply_ambient_context_but_still_require_modules() {
    let errors = check("types.d.ts", "global {}", false);
    assert_eq!(codes(&errors), [1046, 2669], "{errors:?}");
    for (source, expected) in [
        ("export {}; global {}", vec![1046]),
        ("declare module 'm' { global {} }", vec![]),
    ] {
        let errors = check("types.d.ts", source, false);
        assert_eq!(codes(&errors), expected, "{source}: {errors:?}");
    }
}

#[test]
fn forced_modules_do_not_turn_declaration_files_into_modules() {
    assert!(check("script.ts", "declare global {}", true).is_empty());
    let errors = check("types.d.ts", "declare global {}", true);
    assert_eq!(codes(&errors), [2669], "{errors:?}");
}

#[test]
fn exporting_global_names_reports_non_local_exports() {
    let source = "export { globalThis, Math, Array };";
    let errors = check("exports.ts", source, false);
    assert_eq!(codes(&errors), [2661, 2661, 2661], "{errors:?}");
    for (error, name) in errors.iter().zip(["globalThis", "Math", "Array"]) {
        assert_eq!(
            error.message,
            format!(
                "Cannot export '{name}'. Only local declarations can be exported from a module."
            )
        );
    }
    let errors = check(
        "exports.ts",
        "const globalThis = 1; export { globalThis };",
        false,
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn automatic_module_detection_uses_file_extensions_and_host_format() {
    for (name, format, expected) in [
        ("source.mts", None, vec![]),
        ("source.cts", None, vec![]),
        ("source.ts", Some(ModuleKind::ESNext), vec![]),
        ("source.ts", Some(ModuleKind::CommonJS), vec![2669]),
        ("source.d.mts", Some(ModuleKind::ESNext), vec![2669]),
    ] {
        for legacy in [false, true] {
            let errors = check_options(
                name,
                "declare global {}",
                CompilerOptions {
                    module: Some(ModuleKind::NodeNext),
                    target: Some(ScriptTarget::ES2015),
                    strict: Some(false),
                    module_detection: legacy.then(|| "legacy".into()),
                    ..CompilerOptions::default()
                },
                format,
            );
            assert_eq!(
                codes(&errors),
                if legacy { vec![2669] } else { expected.clone() },
                "{name}: {errors:?}"
            );
        }
    }
    let errors = check_options(
        "source.ts",
        "declare global {}",
        CompilerOptions {
            module: Some(ModuleKind::ESNext),
            strict: Some(false),
            ..CompilerOptions::default()
        },
        Some(ModuleKind::ESNext),
    );
    assert_eq!(
        codes(&errors),
        [2669],
        "an ES module output option does not by itself make a source file a module: {errors:?}"
    );
}

#[test]
fn automatic_jsx_runtime_makes_actual_jsx_files_modules() {
    for (source, expected) in [
        ("declare global {} const element = <div />;", vec![]),
        ("declare global {} const element = '<div />';", vec![2669]),
    ] {
        let errors = check_options(
            "source.tsx",
            source,
            CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                jsx: Some(JsxEmit::ReactJSX),
                ..CompilerOptions::default()
            },
            None,
        );
        assert_eq!(codes(&errors), expected, "{errors:?}");
    }
}

fn check_program(source: &str, cached: &[&str]) -> Vec<Diagnostic> {
    let base = tsc_rs_parser::parse("a.ts", "export class A {}");
    let dependency = tsc_rs_parser::parse("dep.ts", "export class B {}");
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let mut checker = TypeChecker::new();
    checker.register_available_files(&["a.ts".into(), "dep.ts".into(), "subject.ts".into()]);
    checker.register_resolvable_specifiers(
        "subject.ts",
        &cached
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>(),
    );
    checker.inject_external_types(&[&base, &dependency, &file]);
    checker.set_current_file_name("subject.ts");
    checker.enable_module_resolution_diagnostics();
    let mut errors = checker
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                strict: Some(false),
                target: Some(ScriptTarget::ES2015),
                module: Some(ModuleKind::CommonJS),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    errors.sort_by_key(|error| (error.span.map(|span| span.start), error.code));
    errors
}

#[test]
fn augmentation_imports_and_reexports_report_grammar_and_resolution_errors() {
    let source = "import { A } from './a'; declare module './a' { import { B } from './dep'; export { B } from './dep'; }";
    let errors = check_program(source, &["./a"]);
    assert_eq!(codes(&errors), [2667, 2307, 2666, 2307], "{errors:?}");
    for (error, expected) in errors
        .iter()
        .zip(["import", "'./dep'", "export", "'./dep'"])
    {
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], expected);
    }
}

#[test]
fn an_augmentation_can_reuse_a_top_level_imports_resolution() {
    let source = "import { A } from './a'; import './dep'; declare module './a' { import { B } from './dep'; }";
    let errors = check_program(source, &["./a", "./dep"]);
    assert_eq!(codes(&errors), [2667], "{errors:?}");
}

#[test]
fn global_augmentations_reject_imports_and_export_clauses() {
    let errors = check_program(
        "export {}; declare global { import { B } from './dep'; export {}; }",
        &[],
    );
    assert_eq!(codes(&errors), [2667, 2307, 2666], "{errors:?}");
}

#[test]
fn internal_import_aliases_and_export_modifiers_are_allowed_in_augmentations() {
    let source = "import { A } from './a'; namespace N { export interface V {} } declare module './a' { import V = N.V; export interface A { v: V; } }";
    let errors = check_program(source, &["./a"]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn export_clauses_disable_implicit_augmentation_exports() {
    for (body, expected) in [
        ("interface A { field: number; }", vec![]),
        (
            "export {}; interface A { field: number; }",
            vec![2339, 2666],
        ),
        (
            "export {}; export interface A { field: number; }",
            vec![2666],
        ),
    ] {
        let source = format!("import {{ A }} from './a'; declare const value: A; value.field; declare module './a' {{ {body} }}");
        let errors = check_program(&source, &["./a"]);
        assert_eq!(codes(&errors), expected, "{source}: {errors:?}");
    }
}

#[test]
fn unresolved_augmentation_targets_do_not_report_body_augmentation_errors() {
    let source = "export {}; declare module 'missing' { import { B } from './dep'; export {}; }";
    let errors = check_program(source, &[]);
    assert_eq!(codes(&errors), [2664, 2307], "{errors:?}");
}
