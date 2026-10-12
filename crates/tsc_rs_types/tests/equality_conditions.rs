use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(file_name: &str, source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let mut checker = TypeChecker::new();
    checker.set_current_file_name(file_name);
    let mut errors = checker
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                ..options
            },
        )
        .diagnostics;
    errors.sort_by_key(|error| (error.span.map(|span| span.start), error.code));
    errors
}

fn check_ts(source: &str) -> Vec<Diagnostic> {
    check("subject.ts", source, CompilerOptions::default())
}

fn text<'a>(source: &'a str, error: &Diagnostic) -> &'a str {
    let span = error.span.unwrap();
    &source[span.start as usize..span.end as usize]
}

#[test]
fn nan_equalities_report_each_operator_and_suggest_the_other_operand() {
    for (operator, result, negation) in [
        ("==", false, ""),
        ("===", false, ""),
        ("!=", true, "!"),
        ("!==", true, "!"),
    ] {
        for comparison in [
            format!("value {operator} NaN"),
            format!("((NaN)) {operator} value"),
        ] {
            let source = format!("declare const value: number; {comparison};");
            let errors = check_ts(&source);
            assert_eq!(errors.len(), 1, "{errors:?}");
            let error = &errors[0];
            assert_eq!(error.code, 2845);
            assert_eq!(text(&source, error), comparison);
            assert_eq!(
                error.message,
                format!("This condition will always return '{result}'.")
            );
            let related = error.related.as_ref().unwrap();
            assert_eq!(related.len(), 1);
            assert_eq!(related[0].code, 1369);
            assert_eq!(
                related[0].message,
                format!("Did you mean '{negation}Number.isNaN(value)'?")
            );
            assert_eq!(related[0].file_name.as_deref(), Some("subject.ts"));
            let span = related[0].span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], "value");
        }
    }
}

#[test]
fn comparing_nan_to_itself_has_no_redundant_suggestion() {
    let errors = check_ts("NaN === NaN; ((NaN)) != (NaN);");
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2845, 2845]
    );
    assert!(errors.iter().all(|error| error.related.is_none()));
}

#[test]
fn suggestion_names_and_locations_follow_entity_name_syntax() {
    let source="declare const obj: {x: number}; NaN === ((obj.x)); NaN !== obj['x']; NaN === obj?.x; NaN === (obj).x; NaN === (() => 1)();";
    let errors = check_ts(source);
    assert_eq!(errors.len(), 5, "{errors:?}");
    for (error, (span, message)) in errors.iter().zip([
        ("((obj.x))", "Did you mean 'Number.isNaN(obj.x)'?"),
        ("obj['x']", "Did you mean '!Number.isNaN(...)'?"),
        ("obj?.x", "Did you mean 'Number.isNaN(obj.x)'?"),
        ("(obj).x", "Did you mean 'Number.isNaN(...)'?"),
        ("(() => 1)()", "Did you mean 'Number.isNaN(...)'?"),
    ]) {
        assert_eq!(error.code, 2845, "{error:?}");
        let related = &error.related.as_ref().unwrap()[0];
        assert_eq!(related.message, message);
        let actual = related.span.unwrap();
        assert_eq!(&source[actual.start as usize..actual.end as usize], span);
    }
}

#[test]
fn lexical_nan_bindings_shadow_the_global_and_type_parameters_do_not() {
    let source="declare const n: number; function f(NaN: number) { return n === NaN; } { const NaN = 1; n === NaN; } function g() { n === NaN; var NaN = 1; } function h({NaN}: {NaN: number}) { n !== NaN; } try {} catch(NaN) { n === NaN; } namespace Local { const NaN = 1; n === NaN; } function generic<NaN>() { n === NaN; } n === NaN;";
    let errors = check_ts(source);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2845, 2845],
        "{errors:?}"
    );
    assert!(errors
        .iter()
        .all(|error| text(source, error) == "n === NaN"));
    assert!(check_ts("export {}; const NaN = 1; 1 === NaN;").is_empty());
    let forced = check(
        "subject.ts",
        "const NaN = 1; 1 === NaN;",
        CompilerOptions {
            module_detection: Some("force".into()),
            ..CompilerOptions::default()
        },
    );
    assert!(forced.is_empty(), "{forced:?}");
}

#[test]
fn only_parenthesized_global_identifiers_trigger_the_nan_warning() {
    let source="declare const n: number; (NaN as number) === n; NaN! === n; Number.NaN === n; globalThis.NaN === n; n < NaN; n >= NaN; n === (NaN);";
    let errors = check_ts(source);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2845],
        "{errors:?}"
    );
    assert_eq!(text(source, &errors[0]), "n === (NaN)");
    let errors = check_ts("var NaN: number; 1 === NaN;");
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2845],
        "{errors:?}"
    );
}

#[test]
fn nan_warning_preserves_incompatible_operand_diagnostics() {
    let errors = check_ts("NaN === 'text';");
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2367, 2845],
        "{errors:?}"
    );
}

#[test]
fn object_literal_comparisons_follow_reference_syntax() {
    let source="declare const x: any; x === {}; [] != x; x === /a/; x !== function() {}; x === class {}; x === (() => 1); x === new Object(); x === ({}); x === ([]); (x === {});";
    let errors = check_ts(source);
    let expected = [
        ("x === {}", false),
        ("[] != x", true),
        ("x === /a/", false),
        ("x !== function() {}", true),
        ("x === class {}", false),
        ("x === {}", false),
    ];
    assert_eq!(errors.len(), expected.len(), "{errors:?}");
    for (error, (span, result)) in errors.iter().zip(expected) {
        assert_eq!(error.code, 2839);
        assert_eq!(text(source, error), span);
        assert_eq!(error.message,format!("This condition will always return '{result}' since JavaScript compares objects by reference, not value."));
    }
}

#[test]
fn javascript_allows_loose_object_comparisons_but_reports_nan_equality() {
    let source = "function test(x) { x == []; x != {}; x === []; x !== {}; x == NaN; }";
    let errors = check(
        "subject.js",
        source,
        CompilerOptions {
            allow_js: Some(true),
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        errors
            .iter()
            .map(|error| (error.code, text(source, error)))
            .collect::<Vec<_>>(),
        [(2839, "x === []"), (2839, "x !== {}"), (2845, "x == NaN")],
        "{errors:?}"
    );
}

#[test]
fn no_lib_requires_a_declared_global_nan() {
    let prelude="interface Array<T> {} interface Boolean {} interface Function {} interface CallableFunction {} interface NewableFunction {} interface IArguments {} interface Number {} interface Object {} interface RegExp {} interface String {}";
    let missing = check(
        "subject.ts",
        &format!("{prelude} 1 === NaN;"),
        CompilerOptions {
            no_lib: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        missing.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2304],
        "{missing:?}"
    );
    let declared = check(
        "subject.ts",
        &format!("{prelude} declare const NaN: number; 1 === NaN;"),
        CompilerOptions {
            no_lib: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        declared.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2845],
        "{declared:?}"
    );
}

#[test]
fn escaped_identifiers_keep_their_spelling_in_suggestions() {
    let source = r"declare const object: {item: number}; N\u0061N === object.it\u0065m;";
    let errors = check_ts(source);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2845);
    assert_eq!(
        errors[0].related.as_ref().unwrap()[0].message,
        r"Did you mean 'Number.isNaN(object.it\u0065m)'?"
    );
    assert!(check_ts(r"export {}; const N\u0061N = 1; 1 === N\u0061N;").is_empty());
    assert!(check_ts(r"function f(N\u0061N: number) { return 1 === N\u0061N; }").is_empty());
}
