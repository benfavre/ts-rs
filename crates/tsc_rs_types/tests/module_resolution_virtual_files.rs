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
