use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

const REGEX_CASE: &str = include_str!("../../../tests/cases/compiler/regularExpressionScanning.ts");

fn check(source: &str) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    check_with_options(source, &CompilerOptions::default())
}

fn check_with_options(
    source: &str,
    options: &CompilerOptions,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let file = tsc_rs_parser::parse("regex.ts", source);
    let parse_diagnostics = file.diagnostics.clone();
    let symbols = tsc_rs_symbols::bind(&file);
    let semantic_diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, options)
        .diagnostics;
    (parse_diagnostics, semantic_diagnostics)
}

fn line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let before = &source.as_bytes()[..offset];
    let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
    let line_start = before
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |newline| newline + 1);
    (line, offset - line_start + 1)
}

#[test]
fn compiler_case_reports_exact_zero_prefixed_regex_octal_escapes() {
    let (parse_diagnostics, semantic_diagnostics) = check(REGEX_CASE);
    assert!(
        parse_diagnostics.is_empty(),
        "regex grammar diagnostics must remain semantic: {parse_diagnostics:#?}"
    );

    let diagnostics: Vec<_> = semantic_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .collect();
    assert_eq!(diagnostics.len(), 10, "{semantic_diagnostics:#?}");

    let observed: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should identify the escape");
            (
                line_and_column(REGEX_CASE, span.start as usize),
                &REGEX_CASE[span.start as usize..span.end as usize],
                diagnostic.message.as_str(),
            )
        })
        .collect();
    let expected_per_line = [
        (26, r"\01", "01"),
        (33, r"\0", "00"),
        (42, r"\03", "03"),
        (48, r"\005", "05"),
        (54, r"\00", "00"),
    ];
    let expected: Vec<_> = [14usize, 15]
        .into_iter()
        .flat_map(|line| {
            expected_per_line.map(move |(column, slice, replacement)| {
                (
                    ((line, column), slice),
                    format!(
                        "Octal escape sequences are not allowed. Use the syntax '\\x{replacement}'."
                    ),
                )
            })
        })
        .map(|((location, slice), message)| (location, slice, message))
        .collect();
    let observed_owned: Vec<_> = observed
        .into_iter()
        .map(|(location, slice, message)| (location, slice, message.to_string()))
        .collect();
    assert_eq!(observed_owned, expected);
}

#[test]
fn regex_backreferences_lone_zero_and_escaped_slashes_do_not_become_ts1487() {
    let source = r#"const regexes = [/\0/, /[\0]/, /\1/, /\123/, /\8/, /\9/, /\\01/];"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    assert!(
        semantic_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1487),
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn regex_hexadecimal_failures_are_zero_width_and_source_ordered() {
    let source =
        r#"const regexes = [/\u/, /\u0/, /\u00/, /\u000/, /\x/, /\x0/, /\u{}/, /[\u\x]/u];"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");

    let diagnostics: Vec<_> = semantic_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1125)
        .collect();
    assert_eq!(diagnostics.len(), 9, "{semantic_diagnostics:#?}");
    let starts: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.message, "Hexadecimal digit expected.");
            let span = diagnostic.span.expect("TS1125 should carry a span");
            assert_eq!(span.start, span.end);
            span.start
        })
        .collect();
    assert!(
        starts.windows(2).all(|pair| pair[0] < pair[1]),
        "{starts:?}"
    );
}

#[test]
fn regex_valid_and_escaped_sequences_do_not_report_ts1125() {
    let source = r#"const regexes = [/\u0000/, /\x00/, /\u{0}/u, /\\u/, /\\x/, /[\\u]/];"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    assert!(
        semantic_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1125),
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn malformed_regex_hex_escapes_suppress_only_overlapping_octal_diagnostics() {
    for (source, expected) in [
        (r#"const regex = /\x\01/;"#, vec![(1125, 17, 0)]),
        (
            r#"const regex = /\xG\01/;"#,
            vec![(1125, 17, 0), (1487, 18, 3)],
        ),
        (r#"const regex = /\u\01/;"#, vec![(1125, 17, 0)]),
        (r#"const regex = /\u{\01/u;"#, vec![(1125, 18, 0)]),
        (
            r#"const regex = /\u{}\01/u;"#,
            vec![(1125, 18, 0), (1487, 19, 3)],
        ),
    ] {
        let (parse_diagnostics, semantic_diagnostics) = check(source);
        assert!(
            parse_diagnostics.is_empty(),
            "{source:?}: {parse_diagnostics:#?}"
        );
        let diagnostics: Vec<_> = semantic_diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code, 1125 | 1487))
            .map(|diagnostic| {
                let span = diagnostic.span.expect("regex diagnostics carry spans");
                (diagnostic.code, span.start, span.end - span.start)
            })
            .collect();
        assert_eq!(
            diagnostics, expected,
            "{source:?}: {semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn regexp_ts1125_obeys_the_parse_diagnostic_gate() {
    for source in [
        "const broken = ; const regex = /\\u/;",
        "const regex = /\\x/; const broken = ;",
    ] {
        let (parse_diagnostics, semantic_diagnostics) = check(source);
        assert!(!parse_diagnostics.is_empty(), "{source}");
        assert!(
            semantic_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != 1125),
            "{semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn regexp_ts1125_reaches_runtime_property_and_type_computed_contexts() {
    let source = r#"const value = {
    [/\u/]: 1,
    method(arg = /\x/) {}
};
type Shape = {
    [/\u0/]: string;
    [/\x0/](): void;
};
class Container {
    [/\u00/ as any]() {}
}
"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    let diagnostics: Vec<_> = semantic_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1125)
        .collect();
    assert_eq!(diagnostics.len(), 5, "{semantic_diagnostics:#?}");
}

#[test]
fn escaped_slash_and_backslash_parity_preserves_the_regex_boundary() {
    let source = r#"const escapedSlash = /\/\00/;
const threeBackslashes = /\\\/\01/;
const fourBackslashes = /\\\\/;
const escapedBackslash = /\\02/;
"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    let slices: Vec<_> = semantic_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(slices, [r"\00", r"\01"], "{semantic_diagnostics:#?}");
}

#[test]
fn regex_grammar_check_is_skipped_for_unrelated_parse_diagnostics_before_or_after() {
    for source in [
        "const broken = ; const regex = /\\00/;",
        "const regex = /\\00/; const broken = ;",
    ] {
        let (parse_diagnostics, semantic_diagnostics) = check(source);
        assert!(
            !parse_diagnostics.is_empty(),
            "fixture must contain a parse diagnostic: {source}"
        );
        assert!(
            parse_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != 1487),
            "the gating diagnostic must be unrelated to TS1487: {parse_diagnostics:#?}"
        );
        assert!(
            semantic_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != 1487),
            "{semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn regex_grammar_pass_reaches_defaults_decorators_and_computed_names_once() {
    let source = r#"function declared(value = /\00/) {}
const arrow = (value = /\01/) => value;
class Container {
    method(value = /\08/) {}
    [/\09/ as any]() {}
}
@decorate(/\007/)
class Decorated {}
"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    let diagnostics: Vec<_> = semantic_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .collect();
    let slices: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        slices,
        [r"\00", r"\01", r"\0", r"\0", r"\007"],
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn regex_grammar_pass_reaches_relational_shift_and_for_of_operands() {
    let source = r#"const less = value < /\00/.source;
const greater = value > /\01/.source;
const shifted = value >> /\02/.source;
for (const item of /\03/) {}
"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    let slices: Vec<_> = semantic_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        slices,
        [r"\00", r"\01", r"\02", r"\03"],
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn zero_prefixed_regex_octal_escape_is_target_invariant() {
    let source = r#"const regex = /\00/u;"#;
    for target in [
        ScriptTarget::ES3,
        ScriptTarget::ES5,
        ScriptTarget::ES2015,
        ScriptTarget::ESNext,
    ] {
        let options = CompilerOptions {
            target: Some(target),
            ..CompilerOptions::default()
        };
        let (parse_diagnostics, semantic_diagnostics) = check_with_options(source, &options);
        assert!(
            parse_diagnostics.is_empty(),
            "{target:?}: {parse_diagnostics:#?}"
        );
        let ts1487: Vec<_> = semantic_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1487)
            .collect();
        assert_eq!(ts1487.len(), 1, "{target:?}: {semantic_diagnostics:#?}");
        let span = ts1487[0].span.expect("TS1487 should carry a span");
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            r"\00",
            "{target:?}"
        );
    }
}

#[test]
fn unterminated_regex_does_not_run_the_lazy_regex_grammar_check() {
    let source = r#"const regex = /\00"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert!(
        parse_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 1161),
        "{parse_diagnostics:#?}"
    );
    assert!(
        semantic_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1487),
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn escaped_line_break_does_not_turn_an_unterminated_regex_into_a_cached_literal() {
    for source in [
        "const regex = /\\\n\\00/;",
        "const regex = /a\u{2028}\\00/;",
        "const regex = /a\u{2029}\\00/;",
    ] {
        let file = tsc_rs_parser::parse("regex.ts", source);
        assert!(
            file.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == 1161),
            "{source:?}: {:#?}",
            file.diagnostics
        );
        let symbols = tsc_rs_symbols::bind(&file);
        let semantic_diagnostics = TypeChecker::new().check(&file, &symbols).diagnostics;
        assert!(
            semantic_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != 1487),
            "{source:?}: {semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn discarded_or_ineligible_decorators_do_not_report_regex_grammar() {
    for source in [
        r#"@decorate(/\06/) value;"#,
        r#"@decorate(/\06/) const value = 1;"#,
        r#"@decorate(/\06/) function value() {}"#,
        r#"@decorate(/\06/) interface Value {}"#,
        r#"@decorate(/\06/) type Value = {};"#,
        r#"@decorate(/\06/) enum Value {}"#,
    ] {
        let file = tsc_rs_parser::parse("regex.ts", source);
        let symbols = tsc_rs_symbols::bind(&file);
        let semantic_diagnostics = TypeChecker::new().check(&file, &symbols).diagnostics;
        assert!(
            semantic_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != 1487),
            "{source}: {semantic_diagnostics:#?}"
        );
    }

    let valid = r#"@decorate(/\06/) class Value {}"#;
    let file = tsc_rs_parser::parse("regex.ts", valid);
    let symbols = tsc_rs_symbols::bind(&file);
    let semantic_diagnostics = TypeChecker::new().check(&file, &symbols).diagnostics;
    assert!(
        semantic_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 1487),
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn regex_grammar_follows_decorator_semantic_eligibility() {
    let always_invalid = r#"@decorate(/\01/) function top(@param(/\02/) value: unknown) {}
interface Shape { method(@param(/\03/) value: unknown): void; }
class Container {
    @decorate(/\04/) constructor() {}
    @decorate(/\05/) static {}
}
"#;
    for experimental_decorators in [false, true] {
        let options = CompilerOptions {
            experimental_decorators: Some(experimental_decorators),
            ..CompilerOptions::default()
        };
        let (_, diagnostics) = check_with_options(always_invalid, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.code != 1487),
            "{experimental_decorators}: {diagnostics:#?}"
        );
    }

    let class_expression = r#"const Value = @decorate(/\06/) class {};"#;
    let (_, stage_three) = check_with_options(class_expression, &CompilerOptions::default());
    assert!(
        stage_three.iter().any(|diagnostic| diagnostic.code == 1487),
        "{stage_three:#?}"
    );
    let legacy_options = CompilerOptions {
        experimental_decorators: Some(true),
        ..CompilerOptions::default()
    };
    let (_, legacy) = check_with_options(class_expression, &legacy_options);
    assert!(
        legacy.iter().all(|diagnostic| diagnostic.code != 1487),
        "{legacy:#?}"
    );
}

#[test]
fn rejected_signature_defaults_and_for_in_initializers_do_not_run_regex_grammar() {
    let source = r#"declare function declared(value = /\01/): void;
interface Shape { method(value = /\02/): void; }
declare class Container {
    method(value = /\03/): void;
    constructor(value = /\04/);
}
for (var item = /\05/ in source) {}
"#;
    let (parse_diagnostics, semantic_diagnostics) = check(source);
    assert_eq!(
        parse_diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        [2371, 2371, 2371, 2371],
        "{parse_diagnostics:#?}"
    );
    assert!(
        semantic_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1487),
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn computed_name_regex_grammar_matches_type_binding_and_enum_visitation() {
    let source = r#"declare const key: unique symbol;
interface ValidShape { [key]: string; }
interface InterfaceShape {
    [/\01/]: string;
    [/\02/](): void;
}
type LiteralShape = {
    [/\03/]: string;
    [/\04/](): void;
};
type NestedShape = () => { [/\07/]: string };
const { [/\05/]: binding } = source;
enum Values { [/\06/] = 1 }
"#;
    let (_, diagnostics) = check(source);
    let slices: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        slices,
        [r"\01", r"\02", r"\03", r"\04", r"\07", r"\05"],
        "{diagnostics:#?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code != 2304 || !diagnostic.message.contains("'key'") }),
        "{diagnostics:#?}"
    );
}

#[test]
fn overload_signatures_with_runtime_implementations_visit_defaults() {
    let source = r#"function overloaded(value = /\01/): void;
function overloaded(value?: RegExp): void {}
class Container {
    method(value = /\02/): void;
    method(value?: RegExp): void {}
    constructor(value = /\03/);
    constructor(value?: RegExp) {}
}
"#;
    let (_, diagnostics) = check(source);
    let slices: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(slices, [r"\01", r"\02", r"\03"], "{diagnostics:#?}");
}
