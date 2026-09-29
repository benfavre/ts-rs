use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::{TypeCheckOutput, TypeChecker};

fn check(source: &str) -> TypeCheckOutput {
    let file = tsc_rs_parser::parse("update_operands.ts", source);
    assert!(
        file.diagnostics.is_empty(),
        "unexpected parser diagnostics: {:#?}",
        file.diagnostics
    );
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default())
}

fn diagnostics_with_code(source: &str, code: u32) -> Vec<Diagnostic> {
    check(source)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code == code)
        .collect()
}

fn check_es2015(source: &str) -> TypeCheckOutput {
    let file = tsc_rs_parser::parse("update_operands.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        ..CompilerOptions::default()
    };
    TypeChecker::new().check_with_options(&file, &symbols, &options)
}

#[test]
fn invalid_update_operands_report_ts2356_on_the_operand_for_all_four_forms() {
    let source = r#"
declare let text: string;
declare let flag: boolean;
++text;
text--;
--flag;
flag++;
"#;
    let diagnostics = diagnostics_with_code(source, 2356);
    assert_eq!(diagnostics.len(), 4, "{diagnostics:#?}");
    let observed: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS2356 has an operand span");
            (
                &source[span.start as usize..span.end as usize],
                diagnostic.message.as_str(),
            )
        })
        .collect();
    assert_eq!(
        observed,
        vec![
            (
                "text",
                "An arithmetic operand must be of type 'any', 'number', 'bigint' or an enum type."
            ),
            (
                "text",
                "An arithmetic operand must be of type 'any', 'number', 'bigint' or an enum type."
            ),
            (
                "flag",
                "An arithmetic operand must be of type 'any', 'number', 'bigint' or an enum type."
            ),
            (
                "flag",
                "An arithmetic operand must be of type 'any', 'number', 'bigint' or an enum type."
            ),
        ]
    );
}

#[test]
fn update_operand_acceptance_matches_typescript_6_0_3() {
    let source = r#"
declare let anyValue: any;
declare let neverValue: never;
declare let numberValue: number;
declare let bigintValue: bigint;
declare let numberLiteral: 1;
declare let bigintLiteral: 1n;
declare enum NumericEnum { A }
declare let numericEnum: NumericEnum;

++anyValue;
neverValue--;
++numberValue;
bigintValue--;
++numberLiteral;
bigintLiteral--;
++numericEnum;

function numeric<T extends number>(value: T) { ++value; }
function bigint<T extends bigint>(value: T) { value--; }
function both<T extends number | bigint>(value: T) { ++value; }
function chained<T extends U, U extends bigint>(value: T) { value--; }
"#;
    let output = check(source);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 2356),
        "{:#?}",
        output.diagnostics
    );
}

#[test]
fn invalid_unions_string_enums_and_unconstrained_generics_report_ts2356() {
    let source = r#"
declare let objectValue: {};
declare let numberOrString: number | string;
declare enum StringEnum { A = "a" }
declare let stringEnum: StringEnum;

++objectValue;
numberOrString--;
++stringEnum;
function unconstrained<T>(value: T) { value++; }
function unknownConstraint<T extends unknown>(value: T) { --value; }
function anyConstraint<T extends any>(value: T) { value++; }
"#;
    let diagnostics = diagnostics_with_code(source, 2356);
    assert_eq!(diagnostics.len(), 6, "{diagnostics:#?}");
    let slices: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        slices,
        vec![
            "objectValue",
            "numberOrString",
            "stringEnum",
            "value",
            "value",
            "value"
        ]
    );
}

#[test]
fn unknown_operands_keep_the_more_specific_ts18046() {
    let source = r#"
declare let unknownValue: unknown;
declare const holder: { value: unknown };
++unknownValue;
holder.value--;
"#;
    let output = check(source);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 2356),
        "{:#?}",
        output.diagnostics
    );
    let diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 18046)
        .collect();
    assert_eq!(diagnostics.len(), 2, "{:#?}", output.diagnostics);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        vec![
            "'unknownValue' is of type 'unknown'.",
            "'holder.value' is of type 'unknown'."
        ]
    );
}

#[test]
fn update_result_types_match_direct_bigint_and_generic_oracle_behavior() {
    let source = r#"
declare let bigintValue: bigint;
declare let bigintLiteral: 1n;
const preBigint = ++bigintValue;
const postBigintLiteral = bigintLiteral--;
function genericBigint<T extends bigint>(value: T) { return ++value; }
function genericBoth<T extends number | bigint>(value: T) { return value--; }
"#;
    let output = check(source);
    assert!(output.diagnostics.is_empty(), "{:#?}", output.diagnostics);
    for (needle, expected) in [
        ("++bigintValue", "bigint"),
        ("bigintLiteral--", "bigint"),
        ("++value", "number"),
        ("value--", "number"),
    ] {
        let start = source.find(needle).unwrap() as u32;
        assert_eq!(
            output.expression_types.get(&start).map(String::as_str),
            Some(expected),
            "{needle}: {:?}",
            output.expression_types
        );
    }
}

#[test]
fn symbol_factory_results_are_invalid_update_operands() {
    let output = check_es2015(
        "var value = Symbol();\nvar registered = Symbol.for('key');\n++value;\nvalue--;\n++registered;\nregistered--;",
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2356)
            .count(),
        4,
        "{:#?}; types: {:?}",
        output.diagnostics,
        output.expression_types
    );
}

#[test]
fn numeric_enum_members_aliases_and_reverse_maps_follow_update_rules() {
    let source = r#"
const enum Choice { Unknown, Yes, No }
type YesNo = Choice.Yes | Choice.No;
function valid(member: Choice.Yes, union: YesNo) {
    member++;
    union--;
}

enum Numeric { A, B }
declare let missing: any;
++Numeric[1];
Numeric[missing]--;
"#;
    let output = check(source);
    let diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2356)
        .collect();
    assert_eq!(diagnostics.len(), 2, "{:#?}", output.diagnostics);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| {
                let span = diagnostic.span.unwrap();
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        vec!["Numeric[1]", "Numeric[missing]"]
    );
}

#[test]
fn nullish_only_operands_do_not_gain_ts2356() {
    let source = r#"
declare let nullValue: null;
declare let undefinedValue: undefined;
++nullValue;
nullValue--;
++undefinedValue;
undefinedValue--;
"#;
    assert!(
        diagnostics_with_code(source, 2356).is_empty(),
        "nullish operands receive separate diagnostics in tsc"
    );
}

#[test]
fn unknown_intersections_use_their_concrete_constituent() {
    let source = r#"
declare let numeric: unknown & number;
declare let textual: unknown & string;
++numeric;
textual--;
"#;
    let output = check(source);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 18046),
        "{:#?}",
        output.diagnostics
    );
    let diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2356)
        .collect();
    assert_eq!(diagnostics.len(), 1, "{:#?}", output.diagnostics);
    let span = diagnostics[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "textual");
}

#[test]
fn enum_indexes_and_specific_assignment_errors_do_not_gain_ts2356() {
    let source = r#"
enum Numeric { A, B }
enum Empty {}
enum StringOnly { A = "a", B = "b" }
enum Mixed { A = 0, B = "b" }
declare let anyKey: any;
declare let stringKey: string;
declare let memberKey: "A";
declare let mixedMember: Mixed.A;
declare const constantText: string;
declare const holder: { readonly textual: string };
++Numeric[anyKey];
++Numeric[stringKey];
++Numeric[memberKey];
++Numeric["A"];
++StringOnly["A"];
++mixedMember;
++constantText;
++holder.textual;
++(Numeric[0] + Numeric[1]);
++(Empty[0] + Empty[1]);
++Numeric[missingIndex];
"#;
    let output = check(source);
    let diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2356)
        .collect();
    assert_eq!(diagnostics.len(), 5, "{diagnostics:#?}");
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| {
                let span = diagnostic.span.unwrap();
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        [
            "Numeric[anyKey]",
            "mixedMember",
            "(Numeric[0] + Numeric[1])",
            "(Empty[0] + Empty[1])",
            "Numeric[missingIndex]"
        ]
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2540)
            .count(),
        4,
        "{:#?}",
        output.diagnostics
    );
}

#[test]
fn inline_readonly_precedence_obeys_nearest_binding_shadows() {
    let source = r#"
declare const holder: { readonly textual: string };
{
    let holder: { textual: string };
    ++holder.textual;
}
++holder.textual;
"#;
    let output = check(source);
    let relevant: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code, 2356 | 2540))
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(relevant, [(2356, "holder.textual"), (2540, "textual")]);
}
