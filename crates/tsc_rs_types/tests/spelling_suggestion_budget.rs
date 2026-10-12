use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("suggestions.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(false),
                always_strict: Some(false),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

fn missing_values(count: usize) -> String {
    (0..count).map(|i| format!("totallyAbsent{i};\n")).collect()
}

fn missing_types(count: usize) -> String {
    (0..count)
        .map(|i| format!("type Alias{i} = TotallyAbsent{i};\n"))
        .collect()
}

fn assert_codes(source: &str, expected: &[u32]) -> Vec<Diagnostic> {
    let errors = diagnostics(source);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        expected,
        "{source}: {errors:?}"
    );
    errors
}

#[test]
fn unsuccessful_value_lookups_exhaust_the_suggestion_budget() {
    let source = format!(
        "declare const knownName: number;\n{}knownNmae;",
        missing_values(10)
    );
    let errors = assert_codes(&source, &[2304; 11]);
    assert_eq!(
        errors.last().unwrap().message,
        "Cannot find name 'knownNmae'."
    );
}

#[test]
fn the_tenth_value_lookup_can_suggest_but_the_eleventh_cannot() {
    let source = format!(
        "declare const knownName: number;\n{}knownNmae; knownNmae;",
        missing_values(9)
    );
    let mut expected = vec![2304; 9];
    expected.extend([2552, 2304]);
    let errors = assert_codes(&source, &expected);
    assert_eq!(
        errors[9].message,
        "Cannot find name 'knownNmae'. Did you mean 'knownName'?"
    );
    assert_eq!(
        errors[10].span.unwrap().start as usize,
        source.rfind("knownNmae").unwrap()
    );
}

#[test]
fn unsuccessful_type_lookups_also_consume_suggestion_slots() {
    for count in [9, 10] {
        let source = format!(
            "interface KnownType {{}}\n{}type Last = KnownTyp;",
            missing_types(count)
        );
        let mut expected = vec![2304; count];
        expected.push(if count == 9 { 2552 } else { 2304 });
        assert_codes(&source, &expected);
    }
}

#[test]
fn type_and_value_lookups_share_one_budget() {
    let source = format!("interface KnownType {{}} declare const knownName: number;\n{}{}knownNmae; type Last = KnownTyp;", missing_values(5), missing_types(4));
    let mut expected = vec![2304; 9];
    expected.extend([2552, 2304]);
    assert_codes(&source, &expected);
}
