use tsc_rs_ast::{CompilerOptions, Diagnostic, JsxEmit, ModuleKind, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(providers: &[(&str, &str)], source: &str) -> Vec<Diagnostic> {
    check_file(providers, "/project/consumer.js", source)
}

fn check_file(providers: &[(&str, &str)], name: &str, source: &str) -> Vec<Diagnostic> {
    let declarations: Vec<_> = providers
        .iter()
        .map(|(name, text)| tsc_rs_parser::parse(name, text))
        .collect();
    let file = tsc_rs_parser::parse(name, source);
    let mut files: Vec<_> = declarations.iter().collect();
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
    checker.inject_external_types(&files);
    checker.set_current_file_name(&file.file_name);
    checker.enable_module_resolution_diagnostics();
    checker
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                module: Some(ModuleKind::ESNext),
                module_resolution: Some("node".into()),
                allow_js: Some(true),
                check_js: Some(true),
                jsx: Some(JsxEmit::Preserve),
                strict: Some(false),
                ..Default::default()
            },
        )
        .diagnostics
}

fn codes(errors: &[Diagnostic]) -> Vec<u32> {
    errors.iter().map(|error| error.code).collect()
}

#[test]
fn erased_imports_cannot_be_used_as_runtime_values() {
    let source = "Shape; import {Shape} from './types'; Shape; typeof Shape; new Shape(); Shape(); export default Shape;";
    let errors = check(
        &[("/project/types.d.ts", "export interface Shape {}")],
        source,
    );
    assert_eq!(
        codes(&errors),
        [2693, 18042, 2693, 2693, 2693, 2693],
        "{errors:?}"
    );
    for error in errors.iter().filter(|error| error.code == 2693) {
        assert_eq!(
            error.message,
            "'Shape' only refers to a type, but is being used as a value here."
        );
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "Shape");
    }
}

#[test]
fn local_values_shadow_erased_imports() {
    for declaration in [
        "export interface Shape {}",
        "export namespace Shape { interface Member {} }",
    ] {
        for source in [
            "import {Shape} from './types'; function read(Shape) { return Shape; }",
            "import {Shape} from './types'; const Shape = 1; Shape;",
            "import {Shape} from './types'; let Shape; Shape;",
            "import {Shape} from './types'; function Shape() {} Shape;",
            "import {Shape} from './types'; class Shape {} Shape;",
            "import {Shape} from './types'; { const Shape = 1; Shape; }",
        ] {
            let errors = check(&[("/project/types.d.ts", declaration)], source);
            assert_eq!(codes(&errors), [18042], "{source}: {errors:?}");
        }
    }
}

#[test]
fn erased_default_and_renamed_imports_can_share_a_local_value_name() {
    for source in [
        "import Local from './types'; const Local = 1; Local;",
        "import {Shape as Local} from './types'; const Local = 1; Local;",
    ] {
        let errors = check(
            &[(
                "/project/types.d.ts",
                "export default interface Shape {} export {Shape};",
            )],
            source,
        );
        assert_eq!(codes(&errors), [18042], "{source}: {errors:?}");
    }
}

#[test]
fn runtime_imports_still_conflict_with_local_value_declarations() {
    for declaration in [
        "const Model = 1;",
        "let Model;",
        "function Model() {}",
        "class Model {}",
    ] {
        let source = format!("import {{Model}} from './types'; {declaration} Model;");
        let errors = check(&[("/project/types.d.ts", "export class Model {}")], &source);
        assert_eq!(codes(&errors), [2440], "{source}: {errors:?}");
    }
}

#[test]
fn jsx_tags_require_runtime_values_for_imported_types() {
    let source = "import {Shape} from './types'; const x = <Shape/>;";
    let errors = check_file(
        &[("/project/types.d.ts", "export interface Shape {}")],
        "/project/consumer.jsx",
        source,
    );
    assert_eq!(codes(&errors), [18042, 2693], "{errors:?}");
    let span = errors[1].span.unwrap();
    assert_eq!(span.start as usize, source.rfind("Shape").unwrap());
    assert_eq!(&source[span.start as usize..span.end as usize], "Shape");
}

#[test]
fn named_imports_distinguish_types_from_runtime_values() {
    let source = "import { Shape, Shape as Renamed, Model, create, value } from './models';";
    let errors = check(&[("/project/models.d.ts", "export interface Shape { x: number; } export class Model {} export function create(): Shape; export const value: Shape;")], source);
    assert_eq!(codes(&errors), [18042, 18042], "{errors:?}");
    for error in errors {
        assert_eq!(error.message, "'Shape' is a type and cannot be imported in JavaScript files. Use 'import(\"./models\").Shape' in a JSDoc type annotation.");
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "Shape");
    }
}

#[test]
fn ambient_types_and_jsdoc_types_cannot_be_reexported_as_javascript_values() {
    let source = "import { Options, Options as Other, create } from 'models';\n/** @typedef {{ x: any }} Local */\nexport { Local }; export { Local as Renamed }; export { Options };";
    let errors = check(
        &[(
            "/project/models.d.ts",
            "declare module 'models' { interface Options {} function create(): Options; }",
        )],
        source,
    );
    assert_eq!(
        codes(&errors),
        [18042, 18042, 18043, 18043, 18043],
        "{errors:?}"
    );
    for error in &errors[2..4] {
        let related = error.related.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].code, 18044);
        assert_eq!(
            related[0].message,
            "'Local' is automatically exported here."
        );
        let span = related[0].span.unwrap();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "@typedef {{ x: any }} Local"
        );
    }
    assert!(errors[4].related.is_none());
}

#[test]
fn aliases_and_star_reexports_preserve_both_type_and_value_meanings() {
    let errors = check(
        &[
            (
                "/project/model.ts",
                "export interface Shape {} export class Model {}",
            ),
            (
                "/project/first.ts",
                "export { Shape as Data, Model } from './model';",
            ),
            ("/project/second.ts", "export * from './first';"),
            (
                "/project/third.ts",
                "import {Data as Local, Model} from './second'; export {Local as Final, Model};",
            ),
        ],
        "import { Final, Model } from './third';",
    );
    assert_eq!(codes(&errors), [18042], "{errors:?}");
    assert!(errors[0].message.starts_with("'Final' is a type"));
}

#[test]
fn default_interface_imports_report_the_local_binding_name() {
    let source = "import /* Shape */ Data from './types'; import Model from './model';";
    let errors = check(
        &[
            ("/project/types.d.ts", "export default interface Shape {}"),
            ("/project/model.d.ts", "export default class Model {}"),
        ],
        source,
    );
    assert_eq!(codes(&errors), [18042], "{errors:?}");
    assert_eq!(errors[0].message, "'Data' is a type and cannot be imported in JavaScript files. Use 'import(\"./types\").Data' in a JSDoc type annotation.");
    let span = errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "Data");
}

#[test]
fn unresolved_exports_do_not_acquire_type_only_meaning() {
    let errors = check(
        &[("/project/model.ts", "export class Model {}")],
        "import { Missing } from './model'; import { Another } from './missing';",
    );
    assert_eq!(codes(&errors), [2305, 2307], "{errors:?}");
}

#[test]
fn namespace_imports_and_merged_runtime_declarations_are_values() {
    let errors = check(&[("/project/model.d.ts", "export interface Shape {} export interface Mixed {} export const Mixed: number;")],
        "import * as model from './model'; import './model'; import { Mixed } from './model'; export { Mixed };");
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn javascript_reexports_report_types_at_the_remote_name() {
    let source = "export { Shape as Data, Model } from './model';";
    let errors = check(
        &[(
            "/project/model.d.ts",
            "export interface Shape {} export class Model {}",
        )],
        source,
    );
    assert_eq!(codes(&errors), [18043], "{errors:?}");
    let span = errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "Shape");
}

#[test]
fn erased_namespace_imports_report_value_uses_but_allow_lexical_shadows() {
    let errors = check(&[("/project/types.d.ts", "declare module 'types' { namespace Types { interface Value {} } export default Types; }")],
        "Types; import Types from 'types'; typeof Types; Types; function read(Types) { return Types; }");
    assert_eq!(codes(&errors), [2708, 18042, 2708, 2708], "{errors:?}");
}

#[test]
fn jsdoc_namespace_exports_do_not_capture_private_javascript_values() {
    let errors = check(
        &[(
            "/project/types.js",
            "const Types = {};\n/** @typedef {string} Types.Value */\nexport {Types};",
        )],
        "import { Types } from './types';",
    );
    assert_eq!(codes(&errors), [18042], "{errors:?}");
}

#[test]
fn quoted_import_names_use_the_local_name_and_original_token_span() {
    let source = "import { 'shape-type' as Local } from './types';";
    let errors = check(
        &[(
            "/project/types.d.ts",
            "interface Shape {} export { Shape as 'shape-type' };",
        )],
        source,
    );
    assert_eq!(codes(&errors), [18042], "{errors:?}");
    assert_eq!(errors[0].message, "'Local' is a type and cannot be imported in JavaScript files. Use 'import(\"./types\").Local' in a JSDoc type annotation.");
    let span = errors[0].span.unwrap();
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        "'shape-type'"
    );
}

#[test]
fn escaped_default_bindings_keep_the_full_identifier_span() {
    let source = "import /* Data */ \\u0044ata from './types';";
    let errors = check(
        &[("/project/types.d.ts", "export default interface Shape {}")],
        source,
    );
    assert_eq!(codes(&errors), [18042], "{errors:?}");
    assert!(errors[0].message.starts_with("'Data' is a type"));
    let span = errors[0].span.unwrap();
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        "\\u0044ata"
    );
}
