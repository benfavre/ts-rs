use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("flags.ts", source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

#[test]
fn flag_availability_tracks_each_target_boundary() {
    for (flag, before, minimum, name) in [
        ('u', ScriptTarget::ES5, ScriptTarget::ES2015, "es6"),
        ('y', ScriptTarget::ES5, ScriptTarget::ES2015, "es6"),
        ('s', ScriptTarget::ES2017, ScriptTarget::ES2018, "es2018"),
        ('d', ScriptTarget::ES2021, ScriptTarget::ES2022, "es2022"),
        ('v', ScriptTarget::ES2023, ScriptTarget::ES2024, "es2024"),
    ] {
        let source = format!("const pattern = /é/{flag};");
        for target in [before, minimum, ScriptTarget::ESNext] {
            let diagnostics = check(
                &source,
                CompilerOptions {
                    target: Some(target),
                    ..CompilerOptions::default()
                },
            );
            if target == before {
                assert_eq!(diagnostics.len(), 1, "{flag}, {target:?}: {diagnostics:#?}");
                let diagnostic = &diagnostics[0];
                assert_eq!(diagnostic.code, 1501);
                assert_eq!(diagnostic.message, format!("This regular expression flag is only available when targeting '{name}' or later."));
                let span = diagnostic.span.unwrap();
                assert_eq!(
                    &source[span.start as usize..span.end as usize],
                    flag.to_string()
                );
            } else {
                assert!(
                    diagnostics.is_empty(),
                    "{flag}, {target:?}: {diagnostics:#?}"
                );
            }
        }
    }
}

#[test]
fn default_target_accepts_current_flags_and_es5_accepts_legacy_flags() {
    for (source, target) in [
        ("const patterns = [/a/dgimsuy, /a/v];", None),
        ("const patterns = [/a/gim];", Some(ScriptTarget::ES5)),
    ] {
        let diagnostics = check(
            source,
            CompilerOptions {
                target,
                ..CompilerOptions::default()
            },
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }
}

#[test]
fn flags_match_upstream_scanning_case_precedence_and_spans() {
    let source = "const pattern = /foo/visualstudiocode;";
    let diagnostics = check(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                span.start as usize - source.find("visualstudiocode").unwrap(),
                diagnostic.code,
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (0, 1501),
            (2, 1501),
            (3, 1502),
            (4, 1499),
            (5, 1499),
            (6, 1500),
            (7, 1499),
            (8, 1502),
            (9, 1501),
            (10, 1500),
            (11, 1499),
            (12, 1499),
            (13, 1499),
            (14, 1500),
            (15, 1499),
        ]
    );
}

#[test]
fn repeated_unicode_conflicts_are_not_recorded_as_duplicates() {
    let source = "const patterns = [/a/uvvu, /a/vuuv, /a/ss];";
    let diagnostics = check(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            ..CompilerOptions::default()
        },
    );
    let codes: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(
        codes,
        [1501, 1502, 1502, 1500, 1501, 1502, 1502, 1500, 1501, 1500]
    );
}

#[test]
fn unknown_non_bmp_flags_span_whole_code_points() {
    let source = "const pattern = /é/𝘨𝘮𝘶;";
    let diagnostics = check(source, CompilerOptions::default());
    let flags: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.code, 1499);
            assert_eq!(diagnostic.message, "Unknown regular expression flag.");
            let span = diagnostic.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(flags, ["𝘨", "𝘮", "𝘶"]);
}

#[test]
fn unknown_identifier_parts_remain_flags() {
    let source = "const pattern = /a/g1_$;";
    let diagnostics = check(source, CompilerOptions::default());
    let flags: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.code, 1499);
            let span = diagnostic.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(flags, ["1", "_", "$"]);
}

#[test]
fn unicode_trivia_after_flags_is_not_an_unknown_flag() {
    for whitespace in ['\u{00a0}', '\u{2028}', '\u{2029}', '\u{feff}'] {
        let source = format!("const pattern = /a/g{whitespace};");
        let diagnostics = check(&source, CompilerOptions::default());
        assert!(diagnostics.is_empty(), "{whitespace:?}: {diagnostics:#?}");
    }
}

#[test]
fn no_check_skips_flag_diagnostics() {
    let diagnostics = check(
        "const pattern = /foo/visualstudiocode;",
        CompilerOptions {
            no_check: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn misplaced_shebang_suppresses_recovered_regex_flag_errors() {
    let source = "const value = 1;\n#!/usr/bin/env node";
    let file = tsc_rs_parser::parse("flags.ts", source);
    let shebang = file
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == 18026)
        .expect("misplaced shebang must remain a scanner diagnostic");
    let span = shebang.span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "#!");
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, 1499..=1502 | 1509)),
        "{diagnostics:#?}"
    );
}

#[test]
fn shebang_text_in_literals_or_at_file_start_is_valid() {
    for source in [
        "#!/usr/bin/env node\nconst value = 1;",
        "const pattern = /#!/;",
        "const text = '#!';",
        "const text = `#!`;",
        "const value = 1; // #!\n",
    ] {
        let diagnostics = check(source, CompilerOptions::default());
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:#?}");
    }
    let jsx = tsc_rs_parser::parse("flags.tsx", "const node = <div>#!</div>;");
    assert!(jsx.diagnostics.is_empty(), "{:#?}", jsx.diagnostics);
}

#[test]
fn subpattern_flags_check_unknown_duplicate_and_non_toggleable_flags() {
    let source = "const pattern = /(?med-ium:bar)/;";
    let diagnostics = check(source, CompilerOptions::default());
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                &source[span.start as usize..span.end as usize],
                diagnostic.code,
            )
        })
        .collect();
    assert_eq!(actual, [("e", 1499), ("d", 1509), ("u", 1509), ("m", 1500)]);
}

#[test]
fn modifier_groups_respect_escapes_classes_and_group_boundaries() {
    let source = r"const patterns = [/\(?a/, /[(?a]/, /(?=a)(?!b)(?<=c)(?<!d)/, /(?<name>a)/, /(?i:a(?i:b))/, /(?i:a)(?i:b)/, /[[(?a]]/v];";
    let diagnostics = check(source, CompilerOptions::default());
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn subpattern_dot_all_flags_obey_target_availability() {
    let source = "const patterns = [/(?s:a)/, /(?-s:a)/, /(?s-s:a)/];";
    let diagnostics = check(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2017),
            ..CompilerOptions::default()
        },
    );
    let codes: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(codes, [1501, 1501, 1501, 1500]);
}

#[test]
fn upstream_non_bmp_flags_report_six_whole_characters() {
    let source = include_str!("../../../tests/cases/compiler/regularExpressionWithNonBMPFlags.ts");
    let diagnostics = check(source, CompilerOptions::default());
    assert_eq!(diagnostics.len(), 6, "{diagnostics:#?}");
    let mut flags: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.code, 1499);
            let span = diagnostic.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    flags.sort_unstable();
    assert_eq!(flags, ["𝘨", "𝘪", "𝘮", "𝘮", "𝘴", "𝘶"]);
}
