use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("operator_diagnostics.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn diagnostic_codes(source: &str) -> Vec<u32> {
    diagnostics(source)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn reports_syntactically_fixed_truthiness_in_logical_and_conditional_expressions() {
    let codes = diagnostic_codes(
        r#"
declare const fallback: unknown;
const fromObject = ({ value: 1 }) || fallback;
const fromNull = (null as any) ? 1 : 2;
const fromString = "present" && fallback;
"#,
    );

    assert_eq!(codes.iter().filter(|&&code| code == 2872).count(), 2);
    assert_eq!(codes.iter().filter(|&&code| code == 2873).count(), 1);
}

#[test]
fn truthiness_diagnostics_preserve_control_flow_literal_exemptions() {
    let codes = diagnostic_codes(
        r#"
declare const fallback: unknown;
const zero = 0 || fallback;
const one = 1 && fallback;
const yes = true ? fallback : fallback;
const no = false ? fallback : fallback;
"#,
    );

    assert!(!codes.iter().any(|code| matches!(code, 2872 | 2873)));
}

#[test]
fn reports_number_bigint_mismatches_for_binary_arithmetic_operators() {
    let source = r#"
let number: number = 1;
let bigint: bigint = 1n;
number + bigint;
bigint + number;
number - bigint;
bigint - number;
number * bigint;
bigint * number;
number / bigint;
bigint / number;
number % bigint;
bigint % number;
number ** bigint;
bigint ** number;
number & bigint;
bigint & number;
number | bigint;
bigint | number;
number ^ bigint;
bigint ^ number;
number << bigint;
bigint << number;
number >> bigint;
bigint >> number;
bigint >>> bigint;
"#;
    let diagnostics = diagnostics(source);
    let mismatches: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2365)
        .collect();

    assert_eq!(mismatches.len(), 23, "diagnostics: {diagnostics:?}");
    assert_eq!(
        mismatches
            .iter()
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS2365 should have a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        [
            "number + bigint",
            "bigint + number",
            "number - bigint",
            "bigint - number",
            "number * bigint",
            "bigint * number",
            "number / bigint",
            "bigint / number",
            "number % bigint",
            "bigint % number",
            "number ** bigint",
            "bigint ** number",
            "number & bigint",
            "bigint & number",
            "number | bigint",
            "bigint | number",
            "number ^ bigint",
            "bigint ^ number",
            "number << bigint",
            "bigint << number",
            "number >> bigint",
            "bigint >> number",
            "bigint >>> bigint",
        ]
    );
    assert_eq!(
        mismatches[0].message,
        "Operator '+' cannot be applied to types 'number' and 'bigint'."
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, 2362 | 2363)),
        "pair incompatibility should not also emit per-operand diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_number_bigint_mismatches_for_compound_arithmetic_operators() {
    let source = r#"
let number: number = 1;
let bigint: bigint = 1n;
number += bigint;
bigint += number;
number -= bigint;
bigint -= number;
number *= bigint;
bigint *= number;
number /= bigint;
bigint /= number;
number %= bigint;
bigint %= number;
number **= bigint;
bigint **= number;
number &= bigint;
bigint &= number;
number |= bigint;
bigint |= number;
number ^= bigint;
bigint ^= number;
number <<= bigint;
bigint <<= number;
number >>= bigint;
bigint >>= number;
bigint >>>= bigint;
"#;
    let diagnostics = diagnostics(source);
    let mismatches: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2365)
        .collect();

    assert_eq!(mismatches.len(), 23, "diagnostics: {diagnostics:?}");
    assert_eq!(
        mismatches
            .iter()
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS2365 should have a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        [
            "number += bigint",
            "bigint += number",
            "number -= bigint",
            "bigint -= number",
            "number *= bigint",
            "bigint *= number",
            "number /= bigint",
            "bigint /= number",
            "number %= bigint",
            "bigint %= number",
            "number **= bigint",
            "bigint **= number",
            "number &= bigint",
            "bigint &= number",
            "number |= bigint",
            "bigint |= number",
            "number ^= bigint",
            "bigint ^= number",
            "number <<= bigint",
            "bigint <<= number",
            "number >>= bigint",
            "bigint >>= number",
            "bigint >>>= bigint",
        ]
    );
    assert_eq!(
        mismatches[0].message,
        "Operator '+=' cannot be applied to types 'number' and 'bigint'."
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, 2362 | 2363)),
        "pair incompatibility should not also emit per-operand diagnostics: {diagnostics:?}"
    );
}

#[test]
fn number_bigint_pair_check_preserves_valid_operator_cases() {
    let codes = diagnostic_codes(
        r#"
let number: number = 1;
let otherNumber: number = 2;
let bigint: bigint = 1n;
let otherBigint: bigint = 2n;
declare let anyValue: any;

number + otherNumber;
number - otherNumber;
number * otherNumber;
number / otherNumber;
number % otherNumber;
number ** otherNumber;
number & otherNumber;
number | otherNumber;
number ^ otherNumber;
number << otherNumber;
number >> otherNumber;

bigint + otherBigint;
bigint - otherBigint;
bigint * otherBigint;
bigint / otherBigint;
bigint % otherBigint;
bigint ** otherBigint;
bigint & otherBigint;
bigint | otherBigint;
bigint ^ otherBigint;
bigint << otherBigint;
bigint >> otherBigint;

"prefix" + bigint;
bigint + "suffix";
number < bigint;
bigint >= number;
anyValue + bigint;
bigint * anyValue;
"#,
    );

    assert!(
        !codes.contains(&2365),
        "valid same-domain, string, relational, and any cases must not report TS2365: {codes:?}"
    );
}
