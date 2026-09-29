use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check_import_use(use_source: &str) -> Vec<Diagnostic> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after Unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("tsc-rs-readonly-imports-{unique}"));
    std::fs::create_dir_all(&directory).expect("create readonly import fixture directory");

    let exports_path = directory.join("exports.ts");
    let use_path = directory.join("use.ts");
    let exports_source = r#"
export var variable = 0;
export let lexical = 0;
export const constant = 0;
"#;

    std::fs::write(&exports_path, exports_source).expect("write readonly export fixture");
    std::fs::write(&use_path, use_source).expect("write readonly import fixture");

    let exports_file = tsc_rs_parser::parse(
        exports_path.to_str().expect("utf-8 exports path"),
        exports_source,
    );
    let use_file = tsc_rs_parser::parse(use_path.to_str().expect("utf-8 use path"), use_source);
    let symbols = tsc_rs_symbols::bind(&use_file);
    let mut checker = TypeChecker::new();
    checker.inject_external_types(&[&exports_file, &use_file]);
    checker.take_diagnostics();
    let diagnostics = checker
        .check_with_options(&use_file, &symbols, &CompilerOptions::default())
        .diagnostics;

    std::fs::remove_dir_all(&directory).expect("remove readonly import fixture directory");
    diagnostics
}

fn check_virtual_import_use(
    exports_name: &str,
    use_name: &str,
    use_source: &str,
) -> Vec<Diagnostic> {
    let exports_source = "export const constant = 0;\n";
    let exports_file = tsc_rs_parser::parse(exports_name, exports_source);
    let use_file = tsc_rs_parser::parse(use_name, use_source);
    let symbols = tsc_rs_symbols::bind(&use_file);
    let mut checker = TypeChecker::new();
    checker.inject_external_types(&[&exports_file, &use_file]);
    checker.take_diagnostics();
    checker
        .check_with_options(&use_file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn readonly_diagnostic_slices<'a>(
    source: &'a str,
    diagnostics: &'a [Diagnostic],
) -> Vec<(&'a str, &'a str)> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2540)
        .map(|diagnostic| {
            let span = diagnostic
                .span
                .expect("TS2540 should point at the immutable property");
            (
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect()
}

#[test]
fn namespace_import_members_are_immutable_for_every_export_binding_kind() {
    let source = r#"
import * as ns from "./exports";
ns.variable = 1;
ns.lexical += 1;
++ns.constant;
ns.variable++;
(ns.lexical) = 2;
ns["constant"] = 3;

declare const dynamic: string;
ns[dynamic] = 4;
ns.missing = 5;

const mutable = { variable: 0, lexical: 0, constant: 0 };
mutable.variable = 1;
mutable.lexical += 1;
++mutable.constant;
mutable.variable++;
(mutable.lexical) = 2;
mutable["constant"] = 3;
"#;

    let diagnostics = check_import_use(source);
    assert_eq!(
        readonly_diagnostic_slices(source, &diagnostics),
        [
            (
                "Cannot assign to 'variable' because it is a read-only property.",
                "variable",
            ),
            (
                "Cannot assign to 'lexical' because it is a read-only property.",
                "lexical",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "constant",
            ),
            (
                "Cannot assign to 'variable' because it is a read-only property.",
                "variable",
            ),
            (
                "Cannot assign to 'lexical' because it is a read-only property.",
                "lexical",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "\"constant\"",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn import_equals_require_members_are_immutable() {
    let source = r#"
import ns = require("./exports");
ns.variable -= 1;
--ns.lexical;
(ns.constant)++;
ns["variable"] = 2;
"#;

    let diagnostics = check_import_use(source);
    assert_eq!(
        readonly_diagnostic_slices(source, &diagnostics),
        [
            (
                "Cannot assign to 'variable' because it is a read-only property.",
                "variable",
            ),
            (
                "Cannot assign to 'lexical' because it is a read-only property.",
                "lexical",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "constant",
            ),
            (
                "Cannot assign to 'variable' because it is a read-only property.",
                "\"variable\"",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn virtual_compiler_fixture_import_members_are_immutable() {
    let source = r#"
import ns = require("./exports");
ns.constant = 1;
ns.missing = 2;
"#;
    let diagnostics = check_virtual_import_use("exports.ts", "use.ts", source);

    assert_eq!(
        readonly_diagnostic_slices(source, &diagnostics),
        [(
            "Cannot assign to 'constant' because it is a read-only property.",
            "constant",
        )],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn namespace_members_are_immutable_for_for_in_and_for_of_write_targets() {
    let source = r#"
import * as ns from "./exports";
declare const key: string;

for (ns.constant in {}) {}
for (ns.constant of []) {}
for (ns["constant"] in {}) {}
for (ns["constant"] of []) {}
for ((ns.constant) in {}) {}
for ((ns.constant) of []) {}
for ((ns["constant"]) in {}) {}
for ((ns["constant"]) of []) {}

for (ns.missing in {}) {}
for (ns.missing of []) {}
for (ns[key] in {}) {}
for (ns[key] of []) {}
"#;

    let diagnostics = check_import_use(source);
    assert_eq!(
        readonly_diagnostic_slices(source, &diagnostics),
        [
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "constant",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "constant",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "\"constant\"",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "\"constant\"",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "constant",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "constant",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "\"constant\"",
            ),
            (
                "Cannot assign to 'constant' because it is a read-only property.",
                "\"constant\"",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}
