use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn diagnostics_with_options(
    file_name: &str,
    source: &str,
    options: CompilerOptions,
) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    assert!(
        file.diagnostics.is_empty(),
        "unexpected parser diagnostics: {:#?}",
        file.diagnostics
    );
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    diagnostics_with_options(
        "binary_operator_compatibility.ts",
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            ..CompilerOptions::default()
        },
    )
}

fn ts2365(source: &str) -> Vec<Diagnostic> {
    diagnostics(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == 2365)
        .collect()
}

fn diagnostic_slices<'a>(source: &'a str, diagnostics: &[Diagnostic]) -> Vec<&'a str> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS2365 should have a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect()
}

#[test]
fn invalid_addition_and_compound_addition_report_whole_expression_ts2365() {
    let source = r#"
declare let numberValue: number;
declare let booleanValue: boolean;
declare let symbolValue: symbol;
declare let objectValue: {};
declare let mixed: number | string;
numberValue + booleanValue;
symbolValue + symbolValue;
objectValue + numberValue;
mixed + numberValue;
booleanValue += booleanValue;
objectValue += numberValue;
function generic<T, U>(left: T, right: U) {
    left + right;
    left += right;
}
"#;
    let diagnostics = ts2365(source);
    assert_eq!(diagnostics.len(), 8, "{diagnostics:#?}");
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [
            "numberValue + booleanValue",
            "symbolValue + symbolValue",
            "objectValue + numberValue",
            "mixed + numberValue",
            "booleanValue += booleanValue",
            "objectValue += numberValue",
            "left + right",
            "left += right",
        ]
    );
    assert_eq!(
        diagnostics[0].message,
        "Operator '+' cannot be applied to types 'number' and 'boolean'."
    );
    assert_eq!(
        diagnostics[4].message,
        "Operator '+=' cannot be applied to types 'boolean' and 'boolean'."
    );
}

#[test]
fn addition_preserves_dynamic_string_enum_numeric_and_constrained_domains() {
    let source = r#"
declare let anyValue: any;
declare let neverValue: never;
declare let objectValue: {};
declare let stringValue: string;
declare let numberValue: number;
declare let bigintValue: bigint;
enum NumericEnum { A, B }
enum StringEnum { A = "a", B = "b" }
declare let numericEnum: NumericEnum;
declare let stringEnum: StringEnum;

anyValue + objectValue;
neverValue + objectValue;
stringValue + objectValue;
objectValue + stringValue;
numberValue + numberValue;
bigintValue + bigintValue;
numericEnum + numberValue;
stringEnum + numberValue;
objectValue += stringValue;
stringValue += objectValue;

function numbers<T extends number, U extends number>(left: T, right: U) {
    left + right;
}
function bigints<T extends bigint, U extends bigint>(left: T, right: U) {
    left + right;
}
function strings<T extends string>(left: T, right: number) {
    left + right;
}
"#;
    let diagnostics = diagnostics(source);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 2365),
        "{diagnostics:#?}"
    );
}

#[test]
fn relational_compatibility_reports_unrelated_primitives_unions_and_parameters() {
    let source = r#"
declare let numberValue: number;
declare let stringValue: string;
declare let booleanValue: boolean;
declare let mixed: number | string;
numberValue < stringValue;
booleanValue >= numberValue;
mixed > numberValue;
function generic<T, U>(left: T, right: U) {
    left < right;
    right >= left;
}
"#;
    let diagnostics = ts2365(source);
    assert_eq!(diagnostics.len(), 5, "{diagnostics:#?}");
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [
            "numberValue < stringValue",
            "booleanValue >= numberValue",
            "mixed > numberValue",
            "left < right",
            "right >= left",
        ]
    );
    assert_eq!(
        diagnostics[0].message,
        "Operator '<' cannot be applied to types 'number' and 'string'."
    );
}

#[test]
fn distinct_non_numeric_constraints_and_heterogeneous_enums_remain_incompatible() {
    let source = r#"
enum Heterogeneous { Number = 1, String = "string" }
declare let heterogeneous: Heterogeneous;
heterogeneous + 1;

function strings<T extends string, U extends string>(left: T, right: U) {
    left < right;
}
function booleans<T extends boolean, U extends boolean>(left: T, right: U) {
    left < right;
}
function objects<T extends {}, U extends {}>(left: T, right: U) {
    left < right;
}
"#;
    let diagnostics = ts2365(source);
    assert_eq!(diagnostics.len(), 4, "{diagnostics:#?}");
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [
            "heterogeneous + 1",
            "left < right",
            "left < right",
            "left < right",
        ]
    );
}

#[test]
fn relational_compatibility_preserves_numeric_enum_dynamic_and_structural_cases() {
    let source = r#"
declare let anyValue: any;
declare let neverValue: never;
declare let numberValue: number;
declare let bigintValue: bigint;
declare let stringValue: string;
declare let booleanValue: boolean;
enum NumericEnum { A, B }
declare let numericEnum: NumericEnum;
declare let numericUnion: number | bigint;
declare let base: { value: number };
declare let derived: { value: number, extra: string };

numberValue < bigintValue;
numericUnion >= numberValue;
numericEnum < numberValue;
stringValue < stringValue;
booleanValue < booleanValue;
anyValue < derived;
neverValue < derived;
base < derived;
derived < base;

function same<T>(left: T, right: T) { left < right; }
function numbers<T extends number, U extends bigint>(left: T, right: U) {
    left < right;
}
function topObject<T>(left: T, right: {}) {
    left < right;
    right < left;
}
"#;
    let diagnostics = diagnostics(source);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 2365),
        "{diagnostics:#?}"
    );
}

#[test]
fn strict_nullish_and_unknown_precedence_only_leaves_invalid_non_nullish_pairs() {
    let source = r#"
declare let numberOrNull: number | null;
declare let unknownOrNumber: unknown | number;
null + 1;
undefined < 1;
numberOrNull + 1;
numberOrNull + false;
unknownOrNumber + 1;
unknownOrNumber < 1;
"#;
    let diagnostics = diagnostics_with_options(
        "binary_operator_compatibility.ts",
        source,
        CompilerOptions {
            strict_null_checks: Some(true),
            target: Some(ScriptTarget::ESNext),
            ..CompilerOptions::default()
        },
    );
    let incompatibilities: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2365)
        .cloned()
        .collect();
    assert_eq!(incompatibilities.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostic_slices(source, &incompatibilities),
        ["numberOrNull + false"]
    );
    assert_eq!(
        incompatibilities[0].message,
        "Operator '+' cannot be applied to types 'number' and 'boolean'."
    );
}

#[test]
fn checked_javascript_reports_known_incompatibility_but_preserves_recovery_any() {
    let source = r#"
// @ts-check
/** @type {number} */
let numberValue = 1;
/** @type {boolean} */
let booleanValue = true;
let recovered;
numberValue + booleanValue;
recovered + booleanValue;
"#;
    let diagnostics = diagnostics_with_options(
        "binary_operator_compatibility.js",
        source,
        CompilerOptions {
            check_js: Some(true),
            target: Some(ScriptTarget::ESNext),
            ..CompilerOptions::default()
        },
    );
    let incompatibilities: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2365)
        .cloned()
        .collect();
    assert_eq!(incompatibilities.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostic_slices(source, &incompatibilities),
        ["numberValue + booleanValue"]
    );
}
