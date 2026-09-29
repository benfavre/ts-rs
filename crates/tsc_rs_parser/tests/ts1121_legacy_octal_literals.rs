use tsc_rs_ast::Span;

fn nth_span(source: &str, needle: &str, occurrence: usize) -> Span {
    let (start, _) = source
        .match_indices(needle)
        .nth(occurrence)
        .unwrap_or_else(|| panic!("missing occurrence {occurrence} of {needle:?}"));
    Span::new(start as u32, (start + needle.len()) as u32)
}

#[test]
fn reports_legacy_octal_literals_in_every_numeric_syntax_position() {
    let source = "\
const ordinary = 001;
enum E { negative = -07, positive = 010 }
const object = { 03: `value ${04}` };
type NumericNames = { 05: string };
const recovery = 0123n;
declare const x: number;
const spaced = x - 06;
";
    let parsed = tsc_rs_parser::parse("legacy.ts", source);
    let actual: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1121)
        .map(|diagnostic| {
            (
                diagnostic.span.expect("TS1121 span"),
                diagnostic.message.as_str(),
            )
        })
        .collect();

    let numeric_06 = nth_span(source, "06", 0);
    let expected = vec![
        (
            nth_span(source, "001", 0),
            "Octal literals are not allowed. Use the syntax '0o1'.",
        ),
        (
            nth_span(source, "-07", 0),
            "Octal literals are not allowed. Use the syntax '-0o7'.",
        ),
        (
            nth_span(source, "010", 0),
            "Octal literals are not allowed. Use the syntax '0o10'.",
        ),
        (
            nth_span(source, "03", 0),
            "Octal literals are not allowed. Use the syntax '0o3'.",
        ),
        (
            nth_span(source, "04", 0),
            "Octal literals are not allowed. Use the syntax '0o4'.",
        ),
        (
            nth_span(source, "05", 0),
            "Octal literals are not allowed. Use the syntax '0o5'.",
        ),
        (
            nth_span(source, "0123", 0),
            "Octal literals are not allowed. Use the syntax '0o123'.",
        ),
        (
            Span::new(numeric_06.start - 1, numeric_06.end),
            "Octal literals are not allowed. Use the syntax '-0o6'.",
        ),
    ];

    assert_eq!(actual, expected, "{:#?}", parsed.diagnostics);

    let recovery_start = nth_span(source, "0123n", 0).start;
    assert!(
        parsed.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == 1005
                && diagnostic.span == Some(Span::new(recovery_start + 4, recovery_start + 5))
        }),
        "the trailing bigint suffix must remain available for parser recovery: {:#?}",
        parsed.diagnostics
    );
}

#[test]
fn excludes_modern_bases_decimals_separators_and_literal_text() {
    let source = r#"
const modern = [0o7, 0O10, 0x17, 0X20, 0b01, 0B10];
const decimal = [0, 7, 08, 09, 1.5, 1e3, 0_1, 1_000];
const text = ["01", '02', `03`, /04/];
const tagged = tag`05`;
const relational = value < /06/.source;
const shifted = value >> /07/.source;
for (const item of /010/) {}
"#;
    let parsed = tsc_rs_parser::parse("modern.ts", source);

    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1121),
        "{:#?}",
        parsed.diagnostics
    );
}

#[test]
fn jsx_text_is_not_numeric_but_expression_containers_are() {
    let source = "<>01 - 02 {03} <X value={04}>05</X></>";
    let parsed = tsc_rs_parser::parse("text.tsx", source);
    let actual: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1121)
        .map(|diagnostic| {
            (
                diagnostic.span.expect("TS1121 span"),
                diagnostic.message.as_str(),
            )
        })
        .collect();

    assert_eq!(
        actual,
        [
            (
                nth_span(source, "03", 0),
                "Octal literals are not allowed. Use the syntax '0o3'.",
            ),
            (
                nth_span(source, "04", 0),
                "Octal literals are not allowed. Use the syntax '0o4'.",
            ),
        ],
        "{:#?}",
        parsed.diagnostics
    );
}

#[test]
fn legacy_octal_stops_before_separator_and_suffixes_recover() {
    let source = "01_2; 0123n;";
    let parsed = tsc_rs_parser::parse("suffixes.ts", source);
    let actual: Vec<_> = parsed
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code,
                diagnostic.span.expect("diagnostic span"),
                diagnostic.message.as_str(),
            )
        })
        .collect();

    assert_eq!(
        actual,
        [
            (
                1121,
                nth_span(source, "01", 0),
                "Octal literals are not allowed. Use the syntax '0o1'.",
            ),
            (1005, nth_span(source, "_2", 0), "';' expected."),
            (
                1121,
                nth_span(source, "0123", 0),
                "Octal literals are not allowed. Use the syntax '0o123'.",
            ),
            (1005, nth_span(source, "n", 0), "';' expected."),
        ],
        "{:#?}",
        parsed.diagnostics
    );
}

#[test]
fn grammar_error_at_signed_start_suppresses_scanner_diagnostic() {
    let invalid_negative_properties = [
        "const value = { -01: 1 };",
        "type Value = { -01: string };",
        "interface Value { -01: string }",
        "class Value { -01 = 1 }",
        "enum Value { -01 = 1 }",
        "const { -01: value } = source;",
    ];

    for source in invalid_negative_properties {
        let parsed = tsc_rs_parser::parse("invalid-property.ts", source);
        assert!(
            parsed
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != 1121),
            "{source}: {:#?}",
            parsed.diagnostics
        );
    }

    for source in ["const value = { +01: 1 };", "const value = [-01];"] {
        let parsed = tsc_rs_parser::parse("still-scanned.ts", source);
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 1121)
                .count(),
            1,
            "{source}: {:#?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn preceding_minus_scanner_state_matches_typescript_across_trivia_and_grammar() {
    let cases = [
        (
            "x - 01;",
            Span::new(3, 6),
            "Octal literals are not allowed. Use the syntax '-0o1'.",
        ),
        (
            "x -/*c*/01;",
            Span::new(7, 10),
            "Octal literals are not allowed. Use the syntax '-0o1'.",
        ),
        (
            "- 01;",
            Span::new(1, 4),
            "Octal literals are not allowed. Use the syntax '-0o1'.",
        ),
        (
            "function f(){ return -01; }",
            Span::new(21, 24),
            "Octal literals are not allowed. Use the syntax '-0o1'.",
        ),
        (
            "--x; 01;",
            Span::new(5, 7),
            "Octal literals are not allowed. Use the syntax '0o1'.",
        ),
    ];

    for (source, expected_span, expected_message) in cases {
        let parsed = tsc_rs_parser::parse("minus.ts", source);
        let actual: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1121)
            .map(|diagnostic| {
                (
                    diagnostic.span.expect("TS1121 span"),
                    diagnostic.message.as_str(),
                )
            })
            .collect();

        assert_eq!(
            actual,
            [(expected_span, expected_message)],
            "{source}: {:#?}",
            parsed.diagnostics
        );
    }
}
