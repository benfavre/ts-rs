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

fn recovery_diagnostic_tuples(
    source: &str,
    options: CompilerOptions,
) -> Vec<(u32, u32, u32, String)> {
    let file = tsc_rs_parser::parse("input.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let checked = TypeChecker::new().check_with_options(&file, &symbols, &options);
    let mut diagnostics: Vec<_> = file
        .diagnostics
        .into_iter()
        .chain(checked.diagnostics)
        .map(|d| {
            let span = d.span.unwrap();
            (d.code, span.start, span.end, d.message)
        })
        .collect();
    diagnostics.sort();
    diagnostics
}

#[test]
fn missing_binary_rhs_uses_error_type_but_preserves_operator_results() {
    // Exact TypeScript 6.0.3 parser + checker diagnostics, including outer
    // boolean/number incompatibilities and assignment/call result types.
    for (source, expected) in [
        (
            "1 > > 2;",
            vec![
                (1109, 4, 5, "Expression expected."),
                (
                    2365,
                    0,
                    7,
                    "Operator '>' cannot be applied to types 'boolean' and 'number'.",
                ),
            ],
        ),
        ("1 >> > 2;", vec![(1109, 5, 6, "Expression expected.")]),
        ("1 + > 2;", vec![(1109, 4, 5, "Expression expected.")]),
        ("1 - > 2;", vec![(1109, 4, 5, "Expression expected.")]),
        ("1 * > 2;", vec![(1109, 4, 5, "Expression expected.")]),
        (
            "function f(x:number){} f(1 > > 2);",
            vec![
                (1109, 29, 30, "Expression expected."),
                (
                    2345,
                    25,
                    32,
                    "Argument of type 'boolean' is not assignable to parameter of type 'number'.",
                ),
                (
                    2365,
                    25,
                    32,
                    "Operator '>' cannot be applied to types 'boolean' and 'number'.",
                ),
            ],
        ),
        (
            "const x=[1 > > 2];",
            vec![
                (1109, 13, 14, "Expression expected."),
                (
                    2365,
                    9,
                    16,
                    "Operator '>' cannot be applied to types 'boolean' and 'number'.",
                ),
            ],
        ),
        (
            "const x=(1 > > 2);",
            vec![
                (1109, 13, 14, "Expression expected."),
                (
                    2365,
                    9,
                    16,
                    "Operator '>' cannot be applied to types 'boolean' and 'number'.",
                ),
            ],
        ),
        (
            "const x:number=1 > > 2;",
            vec![
                (1109, 19, 20, "Expression expected."),
                (
                    2322,
                    6,
                    7,
                    "Type 'boolean' is not assignable to type 'number'.",
                ),
                (
                    2365,
                    15,
                    22,
                    "Operator '>' cannot be applied to types 'boolean' and 'number'.",
                ),
            ],
        ),
        (
            "const x:boolean=1 > > 2;",
            vec![
                (1109, 20, 21, "Expression expected."),
                (
                    2365,
                    16,
                    23,
                    "Operator '>' cannot be applied to types 'boolean' and 'number'.",
                ),
            ],
        ),
        (
            "const x:number=1 >> > 2;",
            vec![
                (1109, 20, 21, "Expression expected."),
                (
                    2322,
                    6,
                    7,
                    "Type 'boolean' is not assignable to type 'number'.",
                ),
            ],
        ),
        (
            "const x:boolean=1 >> > 2;",
            vec![(1109, 21, 22, "Expression expected.")],
        ),
    ] {
        let mut expected: Vec<_> = expected
            .into_iter()
            .map(|(code, start, end, message)| (code, start, end, message.to_string()))
            .collect();
        expected.sort();
        for (strict, strict_null_checks) in [
            (None, None),
            (Some(true), None),
            (Some(false), None),
            (Some(true), Some(false)),
            (Some(false), Some(true)),
        ] {
            let actual = recovery_diagnostic_tuples(
                source,
                CompilerOptions {
                    strict,
                    strict_null_checks,
                    target: Some(ScriptTarget::ES2015),
                    ..Default::default()
                },
            );
            assert_eq!(
                actual, expected,
                "{source}, strict={strict:?}, strictNullChecks={strict_null_checks:?}"
            );
        }
    }
}

#[test]
fn genuine_nullish_unknown_and_array_elision_operands_keep_diagnostics() {
    for (source, expected) in [
        (
            "1 > undefined;",
            vec![(18050, 4, 13, "The value 'undefined' cannot be used here.")],
        ),
        (
            "1 > null;",
            vec![(18050, 4, 8, "The value 'null' cannot be used here.")],
        ),
        (
            "declare let x:number|undefined;1 > x;",
            vec![(18048, 35, 36, "'x' is possibly 'undefined'.")],
        ),
        (
            "declare function f():undefined;1 > f();",
            vec![(2532, 35, 38, "Object is possibly 'undefined'.")],
        ),
        (
            "declare let x:unknown;1 > x;",
            vec![(18046, 26, 27, "'x' is of type 'unknown'.")],
        ),
        (
            "const x=[,];1 > x[0];",
            vec![(2532, 16, 20, "Object is possibly 'undefined'.")],
        ),
        (
            "const x=[...[,,]];1 > x[0];",
            vec![(2532, 22, 26, "Object is possibly 'undefined'.")],
        ),
        (
            "const x=[,];1 >> x[0];",
            vec![(2532, 17, 21, "Object is possibly 'undefined'.")],
        ),
        (
            "const a=[,]; const x:number=a[0];",
            vec![(
                2322,
                19,
                20,
                "Type 'undefined' is not assignable to type 'number'.",
            )],
        ),
    ] {
        let mut expected: Vec<_> = expected
            .into_iter()
            .map(|(code, start, end, message)| (code, start, end, message.to_string()))
            .collect();
        expected.sort();
        let actual = recovery_diagnostic_tuples(
            source,
            CompilerOptions {
                strict: Some(true),
                target: Some(ScriptTarget::ES2015),
                ..Default::default()
            },
        );
        assert_eq!(actual, expected, "{source}");
    }
    for source in ["const x=[,];1 > x[0];", "const x=[...[,,]];1 > x[0];"] {
        assert_eq!(
            recovery_diagnostic_tuples(
                source,
                CompilerOptions {
                    strict: Some(false),
                    target: Some(ScriptTarget::ES2015),
                    ..Default::default()
                }
            ),
            vec![],
            "{source}"
        );
    }
    // Disabling strict null checks does not disable noImplicitAny inherited
    // from strict: this real elision still gets TypeScript's TS7005.
    assert_eq!(
        recovery_diagnostic_tuples(
            "const x=[,];1 > x[0];",
            CompilerOptions {
                strict: Some(true),
                strict_null_checks: Some(false),
                target: Some(ScriptTarget::ES2015),
                ..Default::default()
            }
        ),
        vec![(
            7005,
            6,
            7,
            "Variable 'x' implicitly has an 'any[]' type.".to_string()
        )]
    );
}
