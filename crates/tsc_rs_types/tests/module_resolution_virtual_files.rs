use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check_import(
    file_name: &str,
    source: &str,
    available_files: &[&str],
    ambient_modules: &[&str],
) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.set_current_file_name(file_name);
    checker.register_available_files(
        &available_files
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>(),
    );
    checker.register_available_modules(
        &ambient_modules
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>(),
    );
    checker.enable_module_resolution_diagnostics();
    checker
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn diagnostic_codes(diagnostics: &[Diagnostic]) -> Vec<u32> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn virtual_runtime_extensions_resolve_to_typescript_partners() {
    let diagnostics = check_import(
        "/project/index.mts",
        r#"
import "./source.js";
import "./module.mjs";
import "./common.cjs";
import "./component.jsx";
import "./source.d.ts";
import "./component.ts";
import "./missing.js";
"#,
        &[
            "/project/source.ts",
            "/project/module.mts",
            "/project/common.cts",
            "/project/component.tsx",
        ],
        &[],
    );

    assert_eq!(diagnostic_codes(&diagnostics), [2882], "{diagnostics:#?}");
}

#[test]
fn ambient_module_patterns_match_prefix_and_suffix() {
    let diagnostics = check_import(
        "/project/user.ts",
        r#"
import "foo-middle-baz";
import "./document!text";
import "foo-middle-nope";
"#,
        &["/project/user.ts"],
        &["foo*baz", "*!text"],
    );

    assert_eq!(diagnostic_codes(&diagnostics), [2882], "{diagnostics:#?}");
}

#[test]
fn extension_substitution_does_not_resolve_through_a_derived_stem_alias() {
    let diagnostics = check_import(
        "/project/main.ts",
        r#"
import "./file.js";
import "./file.jsx";
import "./file.ts";
import "./file.tsx";
import "./file.mjs";
import "./file.cjs";
import "./file.mts";
import "./file.cts";
import "./file.d.ts";
import "./file.d.mts";
import "./file.d.cts";
"#,
        &[
            "/project/main.ts",
            "/project/file.d.js.ts",
            "/project/file.d.jsx.ts",
            "/project/file.d.ts.ts",
            "/project/file.d.tsx.ts",
            "/project/file.d.mjs.ts",
            "/project/file.d.cjs.ts",
            "/project/file.d.mts.ts",
            "/project/file.d.cts.ts",
            "/project/file.d.d.ts.ts",
            "/project/file.d.d.mts.ts",
            "/project/file.d.d.cts.ts",
        ],
        &[],
    );

    assert_eq!(
        diagnostic_codes(&diagnostics),
        [2882; 11],
        "{diagnostics:#?}"
    );
}

#[test]
fn side_effect_import_error_is_independent_of_module_resolution_kind() {
    let source = "import './missing'; import value from './missing';";
    for resolution in ["classic", "node", "node16", "nodenext", "bundler"] {
        let file = tsc_rs_parser::parse("/project/index.ts", source);
        let symbols = tsc_rs_symbols::bind(&file);
        let mut checker = TypeChecker::new();
        checker.set_current_file_name("/project/index.ts");
        checker.register_available_files(&["/project/index.ts".into()]);
        checker.enable_module_resolution_diagnostics();
        let output = checker.check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                module_resolution: Some(resolution.into()),
                ..Default::default()
            },
        );
        let expected_bound = if resolution == "classic" { 2792 } else { 2307 };
        assert_eq!(
            diagnostic_codes(&output.diagnostics),
            [2882, expected_bound],
            "{resolution}: {:?}",
            output.diagnostics
        );
        let side_effect = &output.diagnostics[0];
        assert_eq!(
            side_effect.message,
            "Cannot find module or type declarations for side-effect import of './missing'."
        );
        let span = side_effect.span.unwrap();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "'./missing'"
        );
    }
}

fn check_missing_modules(source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("/project/index.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let mut checker = TypeChecker::new();
    checker.set_current_file_name(&file.file_name);
    checker.register_available_files(&[file.file_name.clone()]);
    checker.enable_module_resolution_diagnostics();
    checker
        .check_with_options(&file, &tsc_rs_symbols::bind(&file), &options)
        .diagnostics
}

#[test]
fn missing_node_imports_suggest_node_types_in_every_resolution_mode() {
    let source = "import * as fs from 'fs'; import path = require('node:path'); import type * as dns from 'dns/promises';";
    for resolution in ["classic", "node", "node16", "nodenext", "bundler"] {
        let errors = check_missing_modules(
            source,
            CompilerOptions {
                module_resolution: Some(resolution.into()),
                ..Default::default()
            },
        );
        assert_eq!(
            diagnostic_codes(&errors),
            [2591, 2591, 2591],
            "{resolution}: {errors:?}"
        );
        for (error, module) in errors.iter().zip(["fs", "node:path", "dns/promises"]) {
            assert_eq!(error.message, format!("Cannot find name '{module}'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig."));
            let span = error.span.unwrap();
            assert_eq!(
                &source[span.start as usize..span.end as usize],
                format!("'{module}'")
            );
        }
    }
}

#[test]
fn wildcard_types_omit_the_explicit_node_types_advice() {
    let errors = check_missing_modules(
        "import * as fs from 'node:fs';",
        CompilerOptions {
            types: Some(vec!["*".into()]),
            ..Default::default()
        },
    );
    assert_eq!(diagnostic_codes(&errors), [2580], "{errors:?}");
    assert_eq!(errors[0].message, "Cannot find name 'node:fs'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node`.");
}

#[test]
fn node_prefix_does_not_make_unknown_or_relative_modules_builtins() {
    let source = "import a from 'node:test'; import b from 'node:test/reporters'; import c from 'test'; import d from 'node:missing'; import e from './fs'; import f from 'fs/custom';";
    for (resolution, generic_code) in [("classic", 2792), ("node", 2307)] {
        let errors = check_missing_modules(
            source,
            CompilerOptions {
                module_resolution: Some(resolution.into()),
                ..Default::default()
            },
        );
        assert_eq!(
            diagnostic_codes(&errors),
            [
                2591,
                2591,
                generic_code,
                generic_code,
                generic_code,
                generic_code
            ],
            "{errors:?}"
        );
    }
}

#[test]
fn missing_node_side_effect_imports_keep_the_side_effect_diagnostic() {
    let errors =
        check_missing_modules("import 'fs'; import 'node:fs';", CompilerOptions::default());
    assert_eq!(diagnostic_codes(&errors), [2882, 2882], "{errors:?}");
}

#[test]
fn resolved_node_ambient_modules_do_not_suggest_installing_types() {
    let errors = check_import(
        "/project/index.ts",
        "import * as fs from 'fs'; import path = require('node:path');",
        &["/project/index.ts"],
        &["fs", "node:path"],
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn missing_node_modules_inside_augmentations_keep_both_diagnostics() {
    let source =
        "export {}; declare global { export {readFile} from 'fs'; import fs = require('fs'); }";
    let errors = check_missing_modules(source, CompilerOptions::default());
    assert_eq!(
        diagnostic_codes(&errors),
        [2666, 2591, 2667, 2591],
        "{errors:?}"
    );
}
