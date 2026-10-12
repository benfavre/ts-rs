use tsc_rs_ast::{CompilerOptions, Diagnostic, ModuleKind, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(declarations: &[(&str, &str)], source: &str) -> Vec<Diagnostic> {
    let providers: Vec<_> = declarations
        .iter()
        .map(|(name, text)| tsc_rs_parser::parse(name, text))
        .collect();
    let consumer = tsc_rs_parser::parse("consumer.ts", source);
    let mut files: Vec<_> = providers.iter().collect();
    files.push(&consumer);
    for file in &files {
        assert!(
            file.diagnostics.is_empty(),
            "{}: {:?}",
            file.file_name,
            file.diagnostics
        );
    }
    let mut checker = TypeChecker::new();
    checker.register_available_files(
        &files
            .iter()
            .map(|file| file.file_name.clone())
            .collect::<Vec<_>>(),
    );
    checker.inject_external_types(&files);
    checker.set_current_file_name("consumer.ts");
    checker.enable_module_resolution_diagnostics();
    checker
        .check_with_options(
            &consumer,
            &tsc_rs_symbols::bind(&consumer),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                module: Some(ModuleKind::CommonJS),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<u32> {
    diagnostics.iter().map(|error| error.code).collect()
}

#[test]
fn ambient_imports_preserve_class_and_function_return_types() {
    let errors = check(&[("models.d.ts", "declare module 'models' { class Model { name: string; } function create(): Model; }")],
        "import { Model, create } from 'models'; const factory: () => Model = () => undefined; const name: number = create().name;");
    assert_eq!(codes(&errors), [2322, 2322], "{errors:?}");
    assert_eq!(
        errors[0].message,
        "Type 'undefined' is not assignable to type 'Model'."
    );
}

#[test]
fn ambient_module_members_do_not_leak_into_global_names() {
    let errors = check(
        &[(
            "models.d.ts",
            "declare module 'models' { class Model { name: string; } }",
        )],
        "export {}; Model; let value: Model;",
    );
    assert_eq!(codes(&errors), [2304, 2304], "{errors:?}");
}

#[test]
fn ambient_imports_distinguish_same_named_types_in_different_modules() {
    let errors = check(&[("models.d.ts", "declare module 'text' { interface Value { field: string; } } declare module 'numeric' { interface Value { field: number; } }")],
        "import { Value as Text } from 'text'; import { Value as Numeric } from 'numeric'; const text: Text = { field: 1 }; const numeric: Numeric = { field: 'bad' };");
    assert_eq!(codes(&errors), [2322, 2322], "{errors:?}");
}

#[test]
fn ambient_namespace_imports_preserve_constructed_instance_types() {
    let errors = check(&[("models.d.ts", "declare module 'models' { class Model { name: string; } }")],
        "import * as models from 'models'; const model = new models.Model(); const name: number = model.name;");
    assert_eq!(codes(&errors), [2322], "{errors:?}");
}

#[test]
fn ambient_interfaces_merge_across_declaration_files() {
    let errors = check(&[
        ("text.d.ts", "declare module 'models' { interface Value { text: string; } }"),
        ("number.d.ts", "declare module 'models' { interface Value { count: number; } }"),
    ], "import { Value } from 'models'; declare const value: Value; const text: number = value.text; const count: string = value.count;");
    assert_eq!(codes(&errors), [2322, 2322], "{errors:?}");
}

#[test]
fn explicit_ambient_exports_do_not_expose_private_declarations() {
    let errors = check(&[("models.d.ts", "declare module 'models' { class Hidden { name: string; } export const visible: number; export {}; }")],
        "import * as models from 'models'; models.Hidden; const visible: number = models.visible;");
    assert_eq!(codes(&errors), [2339], "{errors:?}");
}

#[test]
fn ambient_export_assignments_preserve_aliased_class_namespace_values() {
    let errors = check(&[("models.d.ts", "declare module 'models' { namespace Model { export const version: number; } class Model { name: string; } import Alias = Model; export = Alias; }")],
        "import Model = require('models'); const model = new Model(); const name: number = model.name; const version: string = Model.version;");
    assert_eq!(codes(&errors), [2322, 2322], "{errors:?}");
}

#[test]
fn ambient_augmentations_merge_instance_and_namespace_members() {
    let errors = check(&[
        ("models.d.ts", "declare module 'models' { class Model { name: string; } namespace Model { const version: number; } }"),
        ("augmentation.ts", "import { Model } from 'models'; declare module 'models' { interface Model { count: number; } namespace Model { const kind: string; } }"),
    ], "import { Model } from 'models'; declare const value: Model; const count: string = value.count; const name: number = value.name; const kind: number = Model.kind; const prototype: Model = Model.prototype;");
    assert_eq!(codes(&errors), [2322, 2322, 2322], "{errors:?}");
}

#[test]
fn nested_ambient_augmentations_resolve_enclosing_imported_types() {
    let errors = check(&[("models.d.ts", "declare module 'models' { class Model {} } declare module 'values' { class Value { text: string; } } declare module 'augmentation' { import { Value } from 'values'; module 'models' { interface Model { get(): Value; } } }")],
        "import { Model } from 'models'; declare const model: Model; const text: number = model.get().text;");
    assert_eq!(codes(&errors), [2322], "{errors:?}");
}
