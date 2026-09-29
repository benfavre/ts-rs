use tsc_rs_ast::Diagnostic;
use tsc_rs_parser::{parse, parse_with_jsx};

fn ts1125(source: &str) -> Vec<Diagnostic> {
    parse("escape.ts", source)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code == 1125)
        .collect()
}

fn assert_single_failure(source: &str, expected_start: u32) {
    let parsed = parse("escape.ts", source);
    let diagnostics: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1125)
        .collect();
    assert_eq!(
        diagnostics.len(),
        1,
        "{source:?}: {:#?}",
        parsed.diagnostics
    );
    let diagnostic = diagnostics[0];
    assert_eq!(diagnostic.message, "Hexadecimal digit expected.");
    assert_eq!(
        diagnostic.span,
        Some(tsc_rs_ast::Span::new(expected_start, expected_start)),
        "{source:?}: {:#?}",
        parsed.diagnostics
    );
}

#[test]
fn strings_report_the_first_missing_fixed_width_or_braced_digit() {
    for (source, expected_start) in [
        (r#""\u""#, 3),
        (r#""\u0""#, 4),
        (r#""\u00""#, 5),
        (r#""\u000""#, 6),
        (r#""\x""#, 3),
        (r#""\x0""#, 4),
        (r#""\u{}""#, 4),
        (r#""\u{z}""#, 4),
        (r#""\u{_1}""#, 4),
        (r#""\x_0""#, 3),
        (r#""\u00_0""#, 5),
    ] {
        assert_single_failure(source, expected_start);
    }

    for source in [
        r#""\u0000""#,
        r#""\x00""#,
        r#""\u{0}""#,
        r#""\\u""#,
        r#""\\x""#,
    ] {
        assert!(ts1125(source).is_empty(), "{source:?}");
    }
}

#[test]
fn untagged_template_chunks_report_but_tagged_and_no_substitution_types_do_not() {
    let source = r#"`\u${value}\x${value}\u{}`;"#;
    let starts: Vec<_> = ts1125(source)
        .into_iter()
        .map(|diagnostic| diagnostic.span.unwrap().start)
        .collect();
    assert_eq!(starts, [3, 13, 24]);

    for clean in [
        r#"tag`\u${value}\x${value}\u{}`;"#,
        r#"tag<string>`\x`;"#,
        r#"(tag)`\x`;"#,
        r#"object.tag`\x`;"#,
        r#"makeTag()`\x`;"#,
        "tag\n`\\x`;",
        r#"type Fixed = `\u`;"#,
        r#"type Braced = `\u{}`;"#,
    ] {
        assert!(ts1125(clean).is_empty(), "{clean:?}");
    }

    let template_type = r#"type T = `\u${string}\x`;"#;
    let starts: Vec<_> = ts1125(template_type)
        .into_iter()
        .map(|diagnostic| diagnostic.span.unwrap().start)
        .collect();
    assert_eq!(starts.len(), 2, "{template_type:?}");
}

#[test]
fn empty_hexadecimal_numeric_literals_report_after_leading_separators() {
    for (source, expected_start) in [
        ("const value = 0xn;", 16),
        ("const value = 0x_;", 17),
        ("const value = 0x__n;", 18),
        ("const value = 0xg;", 16),
        ("const value = 0X_n;", 17),
    ] {
        assert_single_failure(source, expected_start);
    }

    for source in [
        "const value = 0x0;",
        "const value = 0x1n;",
        "const value = 0x__1;",
        "const value = 0XFn;",
        "const \\u = 1;",
        "// 0x 0xn \"\\u\"\nconst value = 1;",
    ] {
        assert!(ts1125(source).is_empty(), "{source:?}");
    }
}

#[test]
fn jsx_text_and_attributes_are_literal_but_expression_containers_are_checked() {
    let source = r#"const node = <div title="\u">text \x 0xn {'\u'}</div>;"#;
    let parsed = parse_with_jsx("escape.tsx", source, true);
    let diagnostics: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1125)
        .collect();
    assert_eq!(diagnostics.len(), 1, "{:#?}", parsed.diagnostics);
    let span = diagnostics[0].span.unwrap();
    assert_eq!(span.start, source.rfind(r"\u").unwrap() as u32 + 2);

    for malformed in [
        r#"const node = <div "\x" />;"#,
        r#"const node = <div title "\x" />;"#,
    ] {
        let parsed = parse_with_jsx("escape.tsx", malformed, true);
        let diagnostics: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1125)
            .collect();
        assert_eq!(
            diagnostics.len(),
            1,
            "{malformed:?}: {:#?}",
            parsed.diagnostics
        );
        assert_eq!(
            diagnostics[0].span.unwrap().start,
            malformed.find(r"\x").unwrap() as u32 + 2,
            "{malformed:?}: {:#?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn string_escapes_are_checked_in_property_and_type_positions_in_source_order() {
    let source = r#"const value = { "\u": 1, ["\x"]: 2 };
type Shape = { "\u0": string; ["\x0"]: number };
"#;
    let starts: Vec<_> = ts1125(source)
        .into_iter()
        .map(|diagnostic| diagnostic.span.unwrap().start)
        .collect();
    assert_eq!(starts.len(), 4);
    assert!(
        starts.windows(2).all(|pair| pair[0] < pair[1]),
        "{starts:?}"
    );
}

#[test]
fn repeated_braced_failures_are_reported_once_each() {
    let source = r#"var value = "\u{r}\u{n}\u{t}";"#;
    let observed: Vec<_> = ts1125(source)
        .into_iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1125 should carry a span");
            (span.start, span.end)
        })
        .collect();
    let expected: Vec<_> = source
        .match_indices(r"\u{")
        .map(|(start, _)| {
            let failure = start as u32 + 3;
            (failure, failure)
        })
        .collect();
    assert_eq!(observed, expected);
}

#[test]
fn malformed_hex_escapes_take_precedence_over_octal_at_the_same_position() {
    for (source, expected) in [
        (r#"const value = "\x\01";"#, vec![(1125, 17, 0)]),
        (
            r#"const value = "\xG\01";"#,
            vec![(1125, 17, 0), (1487, 18, 3)],
        ),
        (r#"const value = "\u\01";"#, vec![(1125, 17, 0)]),
        (r#"const value = "\u{\01";"#, vec![(1125, 18, 0)]),
        (
            r#"const value = "\u{}\01";"#,
            vec![(1125, 18, 0), (1487, 19, 3)],
        ),
    ] {
        let diagnostics: Vec<_> = parse("escape.ts", source)
            .diagnostics
            .into_iter()
            .filter(|diagnostic| matches!(diagnostic.code, 1125 | 1487))
            .map(|diagnostic| {
                let span = diagnostic.span.expect("scanner diagnostics carry spans");
                (diagnostic.code, span.start, span.end - span.start)
            })
            .collect();
        assert_eq!(diagnostics, expected, "{source:?}");
    }
}

#[test]
fn scanner_failures_survive_unterminated_literals_and_neighboring_recovery() {
    for (source, expected_start) in [
        ("const value = \"\\x\nnext;", 17),
        ("const value = \"\\x", 17),
        ("const value = 0x; const other = ;", 16),
        ("const value = -0x;", 17),
        ("const value = 0x.foo;", 16),
    ] {
        let parsed = parse("escape.ts", source);
        let diagnostics: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1125)
            .collect();
        assert_eq!(
            diagnostics.len(),
            1,
            "{source:?}: {:#?}",
            parsed.diagnostics
        );
        assert_eq!(
            diagnostics[0].span,
            Some(tsc_rs_ast::Span::new(expected_start, expected_start)),
            "{source:?}: {:#?}",
            parsed.diagnostics
        );
    }

    let combined = parse("escape.ts", "const a = 0x; const b = 00;");
    let scanner_diagnostics: Vec<_> = combined
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code, 1121 | 1125))
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(
        scanner_diagnostics,
        [1125, 1121],
        "{:#?}",
        combined.diagnostics
    );
}
