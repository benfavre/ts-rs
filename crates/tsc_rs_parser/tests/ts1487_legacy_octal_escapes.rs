use std::collections::BTreeMap;

use tsc_rs_parser::{parse, parse_with_jsx};

const OCTAL_CASE: &str =
    include_str!("../../../tests/cases/compiler/octalLiteralAndEscapeSequence.ts");
const TAGGED_CASE: &str =
    include_str!("../../../tests/cases/compiler/templateLiteralEscapeSequence.ts");

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
fn compiler_case_reports_all_string_and_untagged_template_octal_escapes() {
    let parsed = parse("octalLiteralAndEscapeSequence.ts", OCTAL_CASE);
    let diagnostics: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .collect();
    assert_eq!(diagnostics.len(), 86, "{:#?}", parsed.diagnostics);

    let mut expected_virtual_lines = Vec::new();
    for (start, end) in [
        (18, 24),
        (26, 32),
        (34, 51),
        (54, 55),
        (58, 77),
        (80, 81),
        (84, 85),
        (88, 94),
        (96, 102),
        (104, 110),
        (112, 118),
    ] {
        expected_virtual_lines.extend(start..=end);
    }

    let observed_locations: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should identify the escape");
            let (physical_line, column) = line_and_column(OCTAL_CASE, span.start as usize);
            // The checked compiler case's leading @target directive is omitted
            // from the virtual baseline source.
            (physical_line - 1, column)
        })
        .collect();
    let expected_locations: Vec<_> = expected_virtual_lines
        .iter()
        .map(|line| {
            let column = if (96..=102).contains(line) || (112..=118).contains(line) {
                6
            } else {
                2
            };
            (*line, column)
        })
        .collect();
    assert_eq!(observed_locations, expected_locations);

    let mut span_lengths = BTreeMap::new();
    let mut replacements = BTreeMap::new();
    for diagnostic in diagnostics {
        let span = diagnostic.span.expect("TS1487 should identify the escape");
        *span_lengths.entry(span.end - span.start).or_insert(0usize) += 1;
        let slice = &OCTAL_CASE[span.start as usize..span.end as usize];
        assert!(
            slice.starts_with('\\'),
            "unexpected TS1487 slice: {slice:?}"
        );
        let replacement = diagnostic
            .message
            .strip_prefix("Octal escape sequences are not allowed. Use the syntax '\\x")
            .and_then(|suffix| suffix.strip_suffix("'."))
            .expect("exact TS1487 replacement message");
        *replacements.entry(replacement).or_insert(0usize) += 1;
    }
    assert_eq!(span_lengths, BTreeMap::from([(2, 18), (3, 34), (4, 34)]));
    assert_eq!(
        replacements,
        BTreeMap::from([
            ("00", 20),
            ("01", 14),
            ("04", 10),
            ("05", 18),
            ("0f", 4),
            ("27", 6),
            ("2d", 12),
            ("7f", 2),
        ])
    );
}

#[test]
fn tagged_templates_jsx_attributes_and_non_octal_escapes_stay_clean() {
    let tagged = parse(
        "tagged.ts",
        r#"tag`\1${value}\08${value}\477`;
tag`\005`;
const valid = "\0";
const escaped = "\\01";
const decimal = "\8";
"#,
    );
    assert!(
        tagged
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1487),
        "{:#?}",
        tagged.diagnostics
    );

    let jsx = parse_with_jsx(
        "attribute.tsx",
        r#"const element = <div value="\01" other={'\1'} />;"#,
        true,
    );
    let ts1487: Vec<_> = jsx
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .collect();
    assert_eq!(ts1487.len(), 1, "{:#?}", jsx.diagnostics);
    let span = ts1487[0].span.expect("TS1487 should carry a span");
    assert_eq!(
        &r#"const element = <div value="\01" other={'\1'} />;"#
            [span.start as usize..span.end as usize],
        r"\1",
        "the JavaScript expression escape should be diagnosed, not JSX text"
    );

    let checked_tagged_case = parse("templateLiteralEscapeSequence.ts", TAGGED_CASE);
    assert!(
        checked_tagged_case
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 1487),
        "{:#?}",
        checked_tagged_case.diagnostics
    );
}

#[test]
fn tagged_template_chunks_suppress_ts1487_but_substitution_strings_do_not() {
    let source = r#"tag`\1${'\2'}\3`;"#;
    let parsed = parse("tagged-substitution.ts", source);
    let diagnostics: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .collect();

    assert_eq!(diagnostics.len(), 1, "{:#?}", parsed.diagnostics);
    let span = diagnostics[0]
        .span
        .expect("TS1487 should identify the substitution string escape");
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        r"\2",
        "raw tagged-template chunks should stay suppressed"
    );
}

#[test]
fn import_attribute_and_import_type_option_strings_are_checked() {
    let source = r#"import value from "module" with { type: "\1" };
export * from "module" with { type: "\12" };
type Imported = import("module", { with: { type: "\123" } });
import tagged from "module" with { type: tag`\1` };
type Tagged = import("module", { with: { type: foo.bar`\1` } });
import templated from "module" with { type: `\4${value}\5` };
"#;
    let parsed = parse("attributes.ts", source);
    let diagnostics: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .collect();
    assert_eq!(diagnostics.len(), 5, "{:#?}", parsed.diagnostics);
    let slices: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(slices, [r"\1", r"\12", r"\123", r"\4", r"\5"]);
}

#[test]
fn malformed_hex_and_unicode_recovery_suppresses_only_the_same_start_octal() {
    let source = r#""\x\1\2";
"\xG\3";
"\x1\4\5";
"\u000\6\7";
"\u{\1}\2";
"\u{1\3}\4";
"\u{}\5";
"\u{G\6}";
"\c\7";
"#;
    let parsed = parse("recovery.ts", source);
    let slices: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        slices,
        [r"\2", r"\3", r"\5", r"\7", r"\2", r"\4", r"\5", r"\6", r"\7"],
        "{:#?}",
        parsed.diagnostics
    );
}

#[test]
fn no_substitution_template_literal_type_suppresses_invalid_escape_diagnostics() {
    let source = r#"type CleanOctal = `\1`;
type CleanDecimal = `\8`;
type ReportsInHead = `\01${string}`;
type ReportsInTail = `${string}\02`;
"#;
    let parsed = parse("template-types.ts", source);
    let slices: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1487)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS1487 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(slices, [r"\01", r"\02"], "{:#?}", parsed.diagnostics);
}
