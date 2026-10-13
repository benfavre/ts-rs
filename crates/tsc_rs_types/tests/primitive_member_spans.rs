use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

// Original TypeScript 6.0.3 TypeWriterWalker text and synthetic-node masks.
fn assert_declarations(
    source: &str,
    strict: bool,
    library: &str,
    expected: &[(&str, usize, &str, Option<&str>)],
    diagnostics: &[(u32, u32, u32)],
) {
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        strict: Some(strict),
        target: Some(tsc_rs_ast::ScriptTarget::ES2020),
        lib: vec![library.into()],
        ..Default::default()
    };
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let output = checker.check_with_options(&file, &symbols, &options);
    for &(anchor, offset, text, underline) in expected {
        let span = (offset as u32, (offset + anchor.len()) as u32);
        let actual = output
            .expression_type_spans
            .get(&span)
            .unwrap_or_else(|| panic!("missing {anchor} at {offset}: {source}"));
        assert_eq!(actual.node_text, text, "{anchor} at {offset}: {source}");
        if let Some(underline) = underline {
            assert_eq!(
                actual.underline, underline,
                "{anchor} at {offset}: {source}"
            );
        }
        assert_eq!(
            actual.node_text.encode_utf16().count(),
            actual.underline.len()
        );
    }
    let actual: Vec<_> = output
        .diagnostics
        .iter()
        .map(|d| {
            let span = d.span.unwrap();
            (d.code, span.start, span.end - span.start)
        })
        .collect();
    assert_eq!(actual, diagnostics, "{source}");
    let plain = TypeChecker::new().check_with_options(&file, &symbols, &options);
    assert_eq!(output.expression_types, plain.expression_types);
    assert_eq!(
        format!("{:?}", output.diagnostics),
        format!("{:?}", plain.diagnostics)
    );
}

#[test]
fn string_substring_strict() {
    assert_declarations("declare const value: string; const method = value.substring; const result = value.substring(1);", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("value.substring", 44, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("value", 44, "string", Some("^^^^^^")),
        ("substring", 50, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("result", 67, "string", Some("^^^^^^")),
        ("value.substring(1)", 76, "string", Some("^^^^^^")),
        ("value.substring", 76, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("value", 76, "string", Some("^^^^^^")),
        ("substring", 82, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("1", 92, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn string_substring_loose() {
    assert_declarations("declare const value: string; const method = value.substring; const result = value.substring(1);", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("value.substring", 44, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("value", 44, "string", Some("^^^^^^")),
        ("substring", 50, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("result", 67, "string", Some("^^^^^^")),
        ("value.substring(1)", 76, "string", Some("^^^^^^")),
        ("value.substring", 76, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("value", 76, "string", Some("^^^^^^")),
        ("substring", 82, "(start: number, end?: number) => string", Some("^     ^^      ^^   ^^^      ^^^^^      ")),
        ("1", 92, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn string_starts_with_strict() {
    assert_declarations("declare const value: string; const method = value.startsWith; const result = value.startsWith(\"a\", 1);", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("value.startsWith", 44, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("value", 44, "string", Some("^^^^^^")),
        ("startsWith", 50, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("result", 68, "boolean", Some("^^^^^^^")),
        ("value.startsWith(\"a\", 1)", 77, "boolean", Some("^^^^^^^")),
        ("value.startsWith", 77, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("value", 77, "string", Some("^^^^^^")),
        ("startsWith", 83, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("\"a\"", 94, "\"a\"", Some("^^^")),
        ("1", 99, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn string_starts_with_loose() {
    assert_declarations("declare const value: string; const method = value.startsWith; const result = value.startsWith(\"a\", 1);", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("value.startsWith", 44, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("value", 44, "string", Some("^^^^^^")),
        ("startsWith", 50, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("result", 68, "boolean", Some("^^^^^^^")),
        ("value.startsWith(\"a\", 1)", 77, "boolean", Some("^^^^^^^")),
        ("value.startsWith", 77, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("value", 77, "string", Some("^^^^^^")),
        ("startsWith", 83, "(searchString: string, position?: number) => boolean", Some("^            ^^      ^^        ^^^      ^^^^^       ")),
        ("\"a\"", 94, "\"a\"", Some("^^^")),
        ("1", 99, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn string_ends_with_strict() {
    assert_declarations(
        "declare const value: string; const method = value.endsWith;",
        true,
        "es2020",
        &[
            ("value", 14, "string", Some("^^^^^^")),
            (
                "method",
                35,
                "(searchString: string, endPosition?: number) => boolean",
                Some("^            ^^      ^^           ^^^      ^^^^^       "),
            ),
            (
                "value.endsWith",
                44,
                "(searchString: string, endPosition?: number) => boolean",
                Some("^            ^^      ^^           ^^^      ^^^^^       "),
            ),
            ("value", 44, "string", Some("^^^^^^")),
            (
                "endsWith",
                50,
                "(searchString: string, endPosition?: number) => boolean",
                Some("^            ^^      ^^           ^^^      ^^^^^       "),
            ),
        ],
        &[],
    );
}

#[test]
fn string_ends_with_loose() {
    assert_declarations(
        "declare const value: string; const method = value.endsWith;",
        false,
        "es2020",
        &[
            ("value", 14, "string", Some("^^^^^^")),
            (
                "method",
                35,
                "(searchString: string, endPosition?: number) => boolean",
                Some("^            ^^      ^^           ^^^      ^^^^^       "),
            ),
            (
                "value.endsWith",
                44,
                "(searchString: string, endPosition?: number) => boolean",
                Some("^            ^^      ^^           ^^^      ^^^^^       "),
            ),
            ("value", 44, "string", Some("^^^^^^")),
            (
                "endsWith",
                50,
                "(searchString: string, endPosition?: number) => boolean",
                Some("^            ^^      ^^           ^^^      ^^^^^       "),
            ),
        ],
        &[],
    );
}

#[test]
fn string_split_strict() {
    assert_declarations("declare const value: string; const method = value.split; const result = value.split(\",\");", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("value.split", 44, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("split", 50, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("result", 63, "string[]", Some("^^^^^^^^")),
        ("value.split(\",\")", 72, "string[]", Some("^^^^^^^^")),
        ("value.split", 72, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("value", 72, "string", Some("^^^^^^")),
        ("split", 78, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("\",\"", 84, "\",\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn string_split_loose() {
    assert_declarations("declare const value: string; const method = value.split; const result = value.split(\",\");", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("value.split", 44, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("split", 50, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("result", 63, "string[]", Some("^^^^^^^^")),
        ("value.split(\",\")", 72, "string[]", Some("^^^^^^^^")),
        ("value.split", 72, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("value", 72, "string", Some("^^^^^^")),
        ("split", 78, "{ (separator: string | RegExp, limit?: number): string[]; (splitter: { [Symbol.split](string: string, limit?: number): string[]; }, limit?: number): string[]; }", Some("^^^         ^^               ^^     ^^^      ^^^        ^^^        ^^                                                             ^^     ^^^      ^^^        ^^^")),
        ("\",\"", 84, "\",\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn string_match_strict() {
    assert_declarations("declare const value: string; const method = value.match; const result = value.match(/a/);", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("value.match", 44, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("match", 50, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("result", 63, "RegExpMatchArray | null", Some("^^^^^^^^^^^^^^^^^^^^^^^")),
        ("value.match(/a/)", 72, "RegExpMatchArray | null", Some("^^^^^^^^^^^^^^^^^^^^^^^")),
        ("value.match", 72, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("value", 72, "string", Some("^^^^^^")),
        ("match", 78, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("/a/", 84, "RegExp", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_match_loose() {
    assert_declarations("declare const value: string; const method = value.match; const result = value.match(/a/);", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("value.match", 44, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("match", 50, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("result", 63, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("value.match(/a/)", 72, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("value.match", 72, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("value", 72, "string", Some("^^^^^^")),
        ("match", 78, "{ (regexp: string | RegExp): RegExpMatchArray | null; (matcher: { [Symbol.match](string: string): RegExpMatchArray | null; }): RegExpMatchArray | null; }", Some("^^^      ^^               ^^^                       ^^^       ^^                                                            ^^^                       ^^^")),
        ("/a/", 84, "RegExp", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_search_strict() {
    assert_declarations("declare const value: string; const method = value.search; const result = value.search(/a/);", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("value.search", 44, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("search", 50, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("result", 64, "number", Some("^^^^^^")),
        ("value.search(/a/)", 73, "number", Some("^^^^^^")),
        ("value.search", 73, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("value", 73, "string", Some("^^^^^^")),
        ("search", 79, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("/a/", 86, "RegExp", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_search_loose() {
    assert_declarations("declare const value: string; const method = value.search; const result = value.search(/a/);", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("value.search", 44, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("search", 50, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("result", 64, "number", Some("^^^^^^")),
        ("value.search(/a/)", 73, "number", Some("^^^^^^")),
        ("value.search", 73, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("value", 73, "string", Some("^^^^^^")),
        ("search", 79, "{ (regexp: string | RegExp): number; (searcher: { [Symbol.search](string: string): number; }): number; }", Some("^^^      ^^               ^^^      ^^^        ^^                                            ^^^      ^^^")),
        ("/a/", 86, "RegExp", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_replace_strict() {
    assert_declarations("declare const value: string; const method = value.replace; const result = value.replace(/a/, \"b\");", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value.replace", 44, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("replace", 50, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("result", 65, "string", Some("^^^^^^")),
        ("value.replace(/a/, \"b\")", 74, "string", Some("^^^^^^")),
        ("value.replace", 74, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 74, "string", Some("^^^^^^")),
        ("replace", 80, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 88, "RegExp", Some("^^^^^^")),
        ("\"b\"", 93, "\"b\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn string_replace_loose() {
    assert_declarations("declare const value: string; const method = value.replace; const result = value.replace(/a/, \"b\");", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value.replace", 44, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("replace", 50, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("result", 65, "string", Some("^^^^^^")),
        ("value.replace(/a/, \"b\")", 74, "string", Some("^^^^^^")),
        ("value.replace", 74, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 74, "string", Some("^^^^^^")),
        ("replace", 80, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 88, "RegExp", Some("^^^^^^")),
        ("\"b\"", 93, "\"b\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn string_replace_callback_strict() {
    assert_declarations("declare const value: string; const result = value.replace(/a/, (match, index) => match);", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("result", 35, "string", Some("^^^^^^")),
        ("value.replace(/a/, (match, index) => match)", 44, "string", Some("^^^^^^")),
        ("value.replace", 44, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("replace", 50, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 58, "RegExp", Some("^^^^^^")),
        ("(match, index) => match", 63, "(match: string, index: any) => string", Some("^     ^^^^^^^^^^     ^^^^^^^^^^^^^^^^")),
        ("match", 64, "string", Some("^^^^^^")),
        ("index", 71, "any", None),
        ("match", 81, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_replace_callback_loose() {
    assert_declarations("declare const value: string; const result = value.replace(/a/, (match, index) => match);", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("result", 35, "string", Some("^^^^^^")),
        ("value.replace(/a/, (match, index) => match)", 44, "string", Some("^^^^^^")),
        ("value.replace", 44, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("replace", 50, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 58, "RegExp", Some("^^^^^^")),
        ("(match, index) => match", 63, "(match: string, index: any) => string", Some("^     ^^^^^^^^^^     ^^^^^^^^^^^^^^^^")),
        ("match", 64, "string", Some("^^^^^^")),
        ("index", 71, "any", None),
        ("match", 81, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_locale_compare_strict() {
    assert_declarations("declare const value: string; const method = value.localeCompare;", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (that: string): number; (that: string, locales?: string | string[], options?: Intl.CollatorOptions): number; (that: string, locales?: Intl.LocalesArgument, options?: Intl.CollatorOptions): number; }", Some("^^^    ^^      ^^^      ^^^    ^^      ^^       ^^^                 ^^       ^^^                    ^^^      ^^^    ^^      ^^       ^^^                    ^^       ^^^                    ^^^      ^^^")),
        ("value.localeCompare", 44, "{ (that: string): number; (that: string, locales?: string | string[], options?: Intl.CollatorOptions): number; (that: string, locales?: Intl.LocalesArgument, options?: Intl.CollatorOptions): number; }", Some("^^^    ^^      ^^^      ^^^    ^^      ^^       ^^^                 ^^       ^^^                    ^^^      ^^^    ^^      ^^       ^^^                    ^^       ^^^                    ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("localeCompare", 50, "{ (that: string): number; (that: string, locales?: string | string[], options?: Intl.CollatorOptions): number; (that: string, locales?: Intl.LocalesArgument, options?: Intl.CollatorOptions): number; }", Some("^^^    ^^      ^^^      ^^^    ^^      ^^       ^^^                 ^^       ^^^                    ^^^      ^^^    ^^      ^^       ^^^                    ^^       ^^^                    ^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn string_locale_compare_loose() {
    assert_declarations("declare const value: string; const method = value.localeCompare;", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (that: string): number; (that: string, locales?: string | string[], options?: Intl.CollatorOptions): number; (that: string, locales?: Intl.LocalesArgument, options?: Intl.CollatorOptions): number; }", Some("^^^    ^^      ^^^      ^^^    ^^      ^^       ^^^                 ^^       ^^^                    ^^^      ^^^    ^^      ^^       ^^^                    ^^       ^^^                    ^^^      ^^^")),
        ("value.localeCompare", 44, "{ (that: string): number; (that: string, locales?: string | string[], options?: Intl.CollatorOptions): number; (that: string, locales?: Intl.LocalesArgument, options?: Intl.CollatorOptions): number; }", Some("^^^    ^^      ^^^      ^^^    ^^      ^^       ^^^                 ^^       ^^^                    ^^^      ^^^    ^^      ^^       ^^^                    ^^       ^^^                    ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("localeCompare", 50, "{ (that: string): number; (that: string, locales?: string | string[], options?: Intl.CollatorOptions): number; (that: string, locales?: Intl.LocalesArgument, options?: Intl.CollatorOptions): number; }", Some("^^^    ^^      ^^^      ^^^    ^^      ^^       ^^^                 ^^       ^^^                    ^^^      ^^^    ^^      ^^       ^^^                    ^^       ^^^                    ^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn string_trim_strict() {
    assert_declarations(
        "declare const value: string; const method = value.trim; const result = value.trim();",
        true,
        "es2020",
        &[
            ("value", 14, "string", Some("^^^^^^")),
            ("method", 35, "() => string", Some("^^^^^^      ")),
            ("value.trim", 44, "() => string", Some("^^^^^^      ")),
            ("value", 44, "string", Some("^^^^^^")),
            ("trim", 50, "() => string", Some("^^^^^^      ")),
            ("result", 62, "string", Some("^^^^^^")),
            ("value.trim()", 71, "string", Some("^^^^^^")),
            ("value.trim", 71, "() => string", Some("^^^^^^      ")),
            ("value", 71, "string", Some("^^^^^^")),
            ("trim", 77, "() => string", Some("^^^^^^      ")),
        ],
        &[],
    );
}

#[test]
fn string_trim_loose() {
    assert_declarations(
        "declare const value: string; const method = value.trim; const result = value.trim();",
        false,
        "es2020",
        &[
            ("value", 14, "string", Some("^^^^^^")),
            ("method", 35, "() => string", Some("^^^^^^      ")),
            ("value.trim", 44, "() => string", Some("^^^^^^      ")),
            ("value", 44, "string", Some("^^^^^^")),
            ("trim", 50, "() => string", Some("^^^^^^      ")),
            ("result", 62, "string", Some("^^^^^^")),
            ("value.trim()", 71, "string", Some("^^^^^^")),
            ("value.trim", 71, "() => string", Some("^^^^^^      ")),
            ("value", 71, "string", Some("^^^^^^")),
            ("trim", 77, "() => string", Some("^^^^^^      ")),
        ],
        &[],
    );
}

#[test]
fn string_normalize_strict() {
    assert_declarations("declare const value: string; const method = value.normalize;", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (form: \"NFC\" | \"NFD\" | \"NFKC\" | \"NFKD\"): string; (form?: string): string; }", Some("^^^    ^^                               ^^^      ^^^    ^^^      ^^^      ^^^")),
        ("value.normalize", 44, "{ (form: \"NFC\" | \"NFD\" | \"NFKC\" | \"NFKD\"): string; (form?: string): string; }", Some("^^^    ^^                               ^^^      ^^^    ^^^      ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("normalize", 50, "{ (form: \"NFC\" | \"NFD\" | \"NFKC\" | \"NFKD\"): string; (form?: string): string; }", Some("^^^    ^^                               ^^^      ^^^    ^^^      ^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn string_normalize_loose() {
    assert_declarations("declare const value: string; const method = value.normalize;", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "{ (form: \"NFC\" | \"NFD\" | \"NFKC\" | \"NFKD\"): string; (form?: string): string; }", Some("^^^    ^^                               ^^^      ^^^    ^^^      ^^^      ^^^")),
        ("value.normalize", 44, "{ (form: \"NFC\" | \"NFD\" | \"NFKC\" | \"NFKD\"): string; (form?: string): string; }", Some("^^^    ^^                               ^^^      ^^^    ^^^      ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("normalize", 50, "{ (form: \"NFC\" | \"NFD\" | \"NFKC\" | \"NFKD\"): string; (form?: string): string; }", Some("^^^    ^^                               ^^^      ^^^    ^^^      ^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn string_concat_strict() {
    assert_declarations("declare const value: string; const method = value.concat; const result = value.concat(\"a\", \"b\");", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("value.concat", 44, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("value", 44, "string", Some("^^^^^^")),
        ("concat", 50, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("result", 64, "string", Some("^^^^^^")),
        ("value.concat(\"a\", \"b\")", 73, "string", Some("^^^^^^")),
        ("value.concat", 73, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("value", 73, "string", Some("^^^^^^")),
        ("concat", 79, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("\"a\"", 86, "\"a\"", Some("^^^")),
        ("\"b\"", 91, "\"b\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn string_concat_loose() {
    assert_declarations("declare const value: string; const method = value.concat; const result = value.concat(\"a\", \"b\");", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("method", 35, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("value.concat", 44, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("value", 44, "string", Some("^^^^^^")),
        ("concat", 50, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("result", 64, "string", Some("^^^^^^")),
        ("value.concat(\"a\", \"b\")", 73, "string", Some("^^^^^^")),
        ("value.concat", 73, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("value", 73, "string", Some("^^^^^^")),
        ("concat", 79, "(...strings: string[]) => string", Some("^^^^       ^^        ^^^^^      ")),
        ("\"a\"", 86, "\"a\"", Some("^^^")),
        ("\"b\"", 91, "\"b\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn string_literal_strict() {
    assert_declarations(
        "const value = \"hello\"; const method = value.charAt; const result = value.toUpperCase();",
        true,
        "es2020",
        &[
            ("value", 6, "\"hello\"", Some("^^^^^^^")),
            ("\"hello\"", 14, "\"hello\"", Some("^^^^^^^")),
            (
                "method",
                29,
                "(pos: number) => string",
                Some("^   ^^      ^^^^^      "),
            ),
            (
                "value.charAt",
                38,
                "(pos: number) => string",
                Some("^   ^^      ^^^^^      "),
            ),
            ("value", 38, "\"hello\"", Some("^^^^^^^")),
            (
                "charAt",
                44,
                "(pos: number) => string",
                Some("^   ^^      ^^^^^      "),
            ),
            ("result", 58, "string", Some("^^^^^^")),
            ("value.toUpperCase()", 67, "string", Some("^^^^^^")),
            (
                "value.toUpperCase",
                67,
                "() => string",
                Some("^^^^^^      "),
            ),
            ("value", 67, "\"hello\"", Some("^^^^^^^")),
            ("toUpperCase", 73, "() => string", Some("^^^^^^      ")),
        ],
        &[],
    );
}

#[test]
fn string_literal_loose() {
    assert_declarations(
        "const value = \"hello\"; const method = value.charAt; const result = value.toUpperCase();",
        false,
        "es2020",
        &[
            ("value", 6, "\"hello\"", Some("^^^^^^^")),
            ("\"hello\"", 14, "\"hello\"", Some("^^^^^^^")),
            (
                "method",
                29,
                "(pos: number) => string",
                Some("^   ^^      ^^^^^      "),
            ),
            (
                "value.charAt",
                38,
                "(pos: number) => string",
                Some("^   ^^      ^^^^^      "),
            ),
            ("value", 38, "\"hello\"", Some("^^^^^^^")),
            (
                "charAt",
                44,
                "(pos: number) => string",
                Some("^   ^^      ^^^^^      "),
            ),
            ("result", 58, "string", Some("^^^^^^")),
            ("value.toUpperCase()", 67, "string", Some("^^^^^^")),
            (
                "value.toUpperCase",
                67,
                "() => string",
                Some("^^^^^^      "),
            ),
            ("value", 67, "\"hello\"", Some("^^^^^^^")),
            ("toUpperCase", 73, "() => string", Some("^^^^^^      ")),
        ],
        &[],
    );
}

#[test]
fn number_fixed_strict() {
    assert_declarations("declare const value: number; const method = value.toFixed; const result = value.toFixed();", true, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value.toFixed", 44, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toFixed", 50, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("result", 65, "string", Some("^^^^^^")),
        ("value.toFixed()", 74, "string", Some("^^^^^^")),
        ("value.toFixed", 74, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 74, "number", Some("^^^^^^")),
        ("toFixed", 80, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn number_fixed_loose() {
    assert_declarations("declare const value: number; const method = value.toFixed; const result = value.toFixed();", false, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value.toFixed", 44, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toFixed", 50, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("result", 65, "string", Some("^^^^^^")),
        ("value.toFixed()", 74, "string", Some("^^^^^^")),
        ("value.toFixed", 74, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 74, "number", Some("^^^^^^")),
        ("toFixed", 80, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn number_exponential_strict() {
    assert_declarations("declare const value: number; const method = value.toExponential; const result = value.toExponential();", true, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value.toExponential", 44, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toExponential", 50, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("result", 71, "string", Some("^^^^^^")),
        ("value.toExponential()", 80, "string", Some("^^^^^^")),
        ("value.toExponential", 80, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 80, "number", Some("^^^^^^")),
        ("toExponential", 86, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn number_exponential_loose() {
    assert_declarations("declare const value: number; const method = value.toExponential; const result = value.toExponential();", false, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value.toExponential", 44, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toExponential", 50, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("result", 71, "string", Some("^^^^^^")),
        ("value.toExponential()", 80, "string", Some("^^^^^^")),
        ("value.toExponential", 80, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
        ("value", 80, "number", Some("^^^^^^")),
        ("toExponential", 86, "(fractionDigits?: number) => string", Some("^              ^^^      ^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn number_precision_strict() {
    assert_declarations("declare const value: number; const method = value.toPrecision; const result = value.toPrecision();", true, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("value.toPrecision", 44, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toPrecision", 50, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("result", 69, "string", Some("^^^^^^")),
        ("value.toPrecision()", 78, "string", Some("^^^^^^")),
        ("value.toPrecision", 78, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("value", 78, "number", Some("^^^^^^")),
        ("toPrecision", 84, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn number_precision_loose() {
    assert_declarations("declare const value: number; const method = value.toPrecision; const result = value.toPrecision();", false, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("value.toPrecision", 44, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toPrecision", 50, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("result", 69, "string", Some("^^^^^^")),
        ("value.toPrecision()", 78, "string", Some("^^^^^^")),
        ("value.toPrecision", 78, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
        ("value", 78, "number", Some("^^^^^^")),
        ("toPrecision", 84, "(precision?: number) => string", Some("^         ^^^      ^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn number_string_strict() {
    assert_declarations("declare const value: number; const method = value.toString; const result = value.toString(16);", true, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value.toString", 44, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toString", 50, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("result", 66, "string", Some("^^^^^^")),
        ("value.toString(16)", 75, "string", Some("^^^^^^")),
        ("value.toString", 75, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 75, "number", Some("^^^^^^")),
        ("toString", 81, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("16", 90, "16", Some("^^")),
    ], &[
    ]);
}

#[test]
fn number_string_loose() {
    assert_declarations("declare const value: number; const method = value.toString; const result = value.toString(16);", false, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value.toString", 44, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toString", 50, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("result", 66, "string", Some("^^^^^^")),
        ("value.toString(16)", 75, "string", Some("^^^^^^")),
        ("value.toString", 75, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 75, "number", Some("^^^^^^")),
        ("toString", 81, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("16", 90, "16", Some("^^")),
    ], &[
    ]);
}

#[test]
fn number_locale_string_strict() {
    assert_declarations("declare const value: number; const method = value.toLocaleString;", true, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "{ (locales?: string | string[], options?: Intl.NumberFormatOptions): string; (locales?: Intl.LocalesArgument, options?: Intl.NumberFormatOptions): string; }", Some("^^^       ^^^                 ^^       ^^^                        ^^^      ^^^       ^^^                    ^^       ^^^                        ^^^      ^^^")),
        ("value.toLocaleString", 44, "{ (locales?: string | string[], options?: Intl.NumberFormatOptions): string; (locales?: Intl.LocalesArgument, options?: Intl.NumberFormatOptions): string; }", Some("^^^       ^^^                 ^^       ^^^                        ^^^      ^^^       ^^^                    ^^       ^^^                        ^^^      ^^^")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toLocaleString", 50, "{ (locales?: string | string[], options?: Intl.NumberFormatOptions): string; (locales?: Intl.LocalesArgument, options?: Intl.NumberFormatOptions): string; }", Some("^^^       ^^^                 ^^       ^^^                        ^^^      ^^^       ^^^                    ^^       ^^^                        ^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn number_locale_string_loose() {
    assert_declarations("declare const value: number; const method = value.toLocaleString;", false, "es2020", &[
        ("value", 14, "number", Some("^^^^^^")),
        ("method", 35, "{ (locales?: string | string[], options?: Intl.NumberFormatOptions): string; (locales?: Intl.LocalesArgument, options?: Intl.NumberFormatOptions): string; }", Some("^^^       ^^^                 ^^       ^^^                        ^^^      ^^^       ^^^                    ^^       ^^^                        ^^^      ^^^")),
        ("value.toLocaleString", 44, "{ (locales?: string | string[], options?: Intl.NumberFormatOptions): string; (locales?: Intl.LocalesArgument, options?: Intl.NumberFormatOptions): string; }", Some("^^^       ^^^                 ^^       ^^^                        ^^^      ^^^       ^^^                    ^^       ^^^                        ^^^      ^^^")),
        ("value", 44, "number", Some("^^^^^^")),
        ("toLocaleString", 50, "{ (locales?: string | string[], options?: Intl.NumberFormatOptions): string; (locales?: Intl.LocalesArgument, options?: Intl.NumberFormatOptions): string; }", Some("^^^       ^^^                 ^^       ^^^                        ^^^      ^^^       ^^^                    ^^       ^^^                        ^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn number_literal_strict() {
    assert_declarations(
        "const value = 12; const method = value.valueOf; const result = value.toFixed();",
        true,
        "es2020",
        &[
            ("value", 6, "12", Some("^^")),
            ("12", 14, "12", Some("^^")),
            ("method", 24, "() => number", Some("^^^^^^      ")),
            ("value.valueOf", 33, "() => number", Some("^^^^^^      ")),
            ("value", 33, "12", Some("^^")),
            ("valueOf", 39, "() => number", Some("^^^^^^      ")),
            ("result", 54, "string", Some("^^^^^^")),
            ("value.toFixed()", 63, "string", Some("^^^^^^")),
            (
                "value.toFixed",
                63,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("value", 63, "12", Some("^^")),
            (
                "toFixed",
                69,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
        ],
        &[],
    );
}

#[test]
fn number_literal_loose() {
    assert_declarations(
        "const value = 12; const method = value.valueOf; const result = value.toFixed();",
        false,
        "es2020",
        &[
            ("value", 6, "12", Some("^^")),
            ("12", 14, "12", Some("^^")),
            ("method", 24, "() => number", Some("^^^^^^      ")),
            ("value.valueOf", 33, "() => number", Some("^^^^^^      ")),
            ("value", 33, "12", Some("^^")),
            ("valueOf", 39, "() => number", Some("^^^^^^      ")),
            ("result", 54, "string", Some("^^^^^^")),
            ("value.toFixed()", 63, "string", Some("^^^^^^")),
            (
                "value.toFixed",
                63,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("value", 63, "12", Some("^^")),
            (
                "toFixed",
                69,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
        ],
        &[],
    );
}

#[test]
fn boolean_value_strict() {
    assert_declarations("declare const value: boolean; const method = value.valueOf; const result = value.valueOf();", true, "es2020", &[
        ("value", 14, "boolean", Some("^^^^^^^")),
        ("method", 36, "() => boolean", Some("^^^^^^       ")),
        ("value.valueOf", 45, "() => boolean", Some("^^^^^^       ")),
        ("value", 45, "boolean", Some("^^^^^^^")),
        ("valueOf", 51, "() => boolean", Some("^^^^^^       ")),
        ("result", 66, "boolean", Some("^^^^^^^")),
        ("value.valueOf()", 75, "boolean", Some("^^^^^^^")),
        ("value.valueOf", 75, "() => boolean", Some("^^^^^^       ")),
        ("value", 75, "boolean", Some("^^^^^^^")),
        ("valueOf", 81, "() => boolean", Some("^^^^^^       ")),
    ], &[
    ]);
}

#[test]
fn boolean_value_loose() {
    assert_declarations("declare const value: boolean; const method = value.valueOf; const result = value.valueOf();", false, "es2020", &[
        ("value", 14, "boolean", Some("^^^^^^^")),
        ("method", 36, "() => boolean", Some("^^^^^^       ")),
        ("value.valueOf", 45, "() => boolean", Some("^^^^^^       ")),
        ("value", 45, "boolean", Some("^^^^^^^")),
        ("valueOf", 51, "() => boolean", Some("^^^^^^       ")),
        ("result", 66, "boolean", Some("^^^^^^^")),
        ("value.valueOf()", 75, "boolean", Some("^^^^^^^")),
        ("value.valueOf", 75, "() => boolean", Some("^^^^^^       ")),
        ("value", 75, "boolean", Some("^^^^^^^")),
        ("valueOf", 81, "() => boolean", Some("^^^^^^       ")),
    ], &[
    ]);
}

#[test]
fn boolean_literal_strict() {
    assert_declarations(
        "const value = true; const method = value.valueOf; const result = value.valueOf();",
        true,
        "es2020",
        &[
            ("value", 6, "true", Some("^^^^")),
            ("true", 14, "true", Some("^^^^")),
            ("method", 26, "() => boolean", Some("^^^^^^       ")),
            ("value.valueOf", 35, "() => boolean", Some("^^^^^^       ")),
            ("value", 35, "true", Some("^^^^")),
            ("valueOf", 41, "() => boolean", Some("^^^^^^       ")),
            ("result", 56, "boolean", Some("^^^^^^^")),
            ("value.valueOf()", 65, "boolean", Some("^^^^^^^")),
            ("value.valueOf", 65, "() => boolean", Some("^^^^^^       ")),
            ("value", 65, "true", Some("^^^^")),
            ("valueOf", 71, "() => boolean", Some("^^^^^^       ")),
        ],
        &[],
    );
}

#[test]
fn boolean_literal_loose() {
    assert_declarations(
        "const value = true; const method = value.valueOf; const result = value.valueOf();",
        false,
        "es2020",
        &[
            ("value", 6, "true", Some("^^^^")),
            ("true", 14, "true", Some("^^^^")),
            ("method", 26, "() => boolean", Some("^^^^^^       ")),
            ("value.valueOf", 35, "() => boolean", Some("^^^^^^       ")),
            ("value", 35, "true", Some("^^^^")),
            ("valueOf", 41, "() => boolean", Some("^^^^^^       ")),
            ("result", 56, "boolean", Some("^^^^^^^")),
            ("value.valueOf()", 65, "boolean", Some("^^^^^^^")),
            ("value.valueOf", 65, "() => boolean", Some("^^^^^^       ")),
            ("value", 65, "true", Some("^^^^")),
            ("valueOf", 71, "() => boolean", Some("^^^^^^       ")),
        ],
        &[],
    );
}

#[test]
fn bigint_string_strict() {
    assert_declarations("declare const value: bigint; const method = value.toString; const result = value.toString(16);", true, "es2020", &[
        ("value", 14, "bigint", Some("^^^^^^")),
        ("method", 35, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value.toString", 44, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 44, "bigint", Some("^^^^^^")),
        ("toString", 50, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("result", 66, "string", Some("^^^^^^")),
        ("value.toString(16)", 75, "string", Some("^^^^^^")),
        ("value.toString", 75, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 75, "bigint", Some("^^^^^^")),
        ("toString", 81, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("16", 90, "16", Some("^^")),
    ], &[
    ]);
}

#[test]
fn bigint_string_loose() {
    assert_declarations("declare const value: bigint; const method = value.toString; const result = value.toString(16);", false, "es2020", &[
        ("value", 14, "bigint", Some("^^^^^^")),
        ("method", 35, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value.toString", 44, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 44, "bigint", Some("^^^^^^")),
        ("toString", 50, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("result", 66, "string", Some("^^^^^^")),
        ("value.toString(16)", 75, "string", Some("^^^^^^")),
        ("value.toString", 75, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("value", 75, "bigint", Some("^^^^^^")),
        ("toString", 81, "(radix?: number) => string", Some("^     ^^^      ^^^^^      ")),
        ("16", 90, "16", Some("^^")),
    ], &[
    ]);
}

#[test]
fn bigint_literal_strict() {
    assert_declarations(
        "const value = 12n; const method = value.valueOf; const result = value.valueOf();",
        true,
        "es2020",
        &[
            ("value", 6, "12n", Some("^^^")),
            ("12n", 14, "12n", Some("^^^")),
            ("method", 25, "() => bigint", Some("^^^^^^      ")),
            ("value.valueOf", 34, "() => bigint", Some("^^^^^^      ")),
            ("value", 34, "12n", Some("^^^")),
            ("valueOf", 40, "() => bigint", Some("^^^^^^      ")),
            ("result", 55, "bigint", Some("^^^^^^")),
            ("value.valueOf()", 64, "bigint", Some("^^^^^^")),
            ("value.valueOf", 64, "() => bigint", Some("^^^^^^      ")),
            ("value", 64, "12n", Some("^^^")),
            ("valueOf", 70, "() => bigint", Some("^^^^^^      ")),
        ],
        &[],
    );
}

#[test]
fn bigint_literal_loose() {
    assert_declarations(
        "const value = 12n; const method = value.valueOf; const result = value.valueOf();",
        false,
        "es2020",
        &[
            ("value", 6, "12n", Some("^^^")),
            ("12n", 14, "12n", Some("^^^")),
            ("method", 25, "() => bigint", Some("^^^^^^      ")),
            ("value.valueOf", 34, "() => bigint", Some("^^^^^^      ")),
            ("value", 34, "12n", Some("^^^")),
            ("valueOf", 40, "() => bigint", Some("^^^^^^      ")),
            ("result", 55, "bigint", Some("^^^^^^")),
            ("value.valueOf()", 64, "bigint", Some("^^^^^^")),
            ("value.valueOf", 64, "() => bigint", Some("^^^^^^      ")),
            ("value", 64, "12n", Some("^^^")),
            ("valueOf", 70, "() => bigint", Some("^^^^^^      ")),
        ],
        &[],
    );
}

#[test]
fn symbol_value_strict() {
    assert_declarations("declare const value: symbol; const method = value.valueOf; const result = value.valueOf();", true, "es2020", &[
        ("value", 14, "symbol", Some("^^^^^^")),
        ("method", 35, "() => symbol", Some("^^^^^^      ")),
        ("value.valueOf", 44, "() => symbol", Some("^^^^^^      ")),
        ("value", 44, "symbol", Some("^^^^^^")),
        ("valueOf", 50, "() => symbol", Some("^^^^^^      ")),
        ("result", 65, "symbol", Some("^^^^^^")),
        ("value.valueOf()", 74, "symbol", Some("^^^^^^")),
        ("value.valueOf", 74, "() => symbol", Some("^^^^^^      ")),
        ("value", 74, "symbol", Some("^^^^^^")),
        ("valueOf", 80, "() => symbol", Some("^^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn symbol_value_loose() {
    assert_declarations("declare const value: symbol; const method = value.valueOf; const result = value.valueOf();", false, "es2020", &[
        ("value", 14, "symbol", Some("^^^^^^")),
        ("method", 35, "() => symbol", Some("^^^^^^      ")),
        ("value.valueOf", 44, "() => symbol", Some("^^^^^^      ")),
        ("value", 44, "symbol", Some("^^^^^^")),
        ("valueOf", 50, "() => symbol", Some("^^^^^^      ")),
        ("result", 65, "symbol", Some("^^^^^^")),
        ("value.valueOf()", 74, "symbol", Some("^^^^^^")),
        ("value.valueOf", 74, "() => symbol", Some("^^^^^^      ")),
        ("value", 74, "symbol", Some("^^^^^^")),
        ("valueOf", 80, "() => symbol", Some("^^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn unique_symbol_value_strict() {
    assert_declarations("declare const value: unique symbol; const method = value.toString; const result = value.toString();", true, "es2020", &[
        ("value", 14, "unique symbol", Some("^^^^^^^^^^^^^")),
        ("method", 42, "() => string", Some("^^^^^^      ")),
        ("value.toString", 51, "() => string", Some("^^^^^^      ")),
        ("value", 51, "unique symbol", Some("^^^^^^^^^^^^^")),
        ("toString", 57, "() => string", Some("^^^^^^      ")),
        ("result", 73, "string", Some("^^^^^^")),
        ("value.toString()", 82, "string", Some("^^^^^^")),
        ("value.toString", 82, "() => string", Some("^^^^^^      ")),
        ("value", 82, "unique symbol", Some("^^^^^^^^^^^^^")),
        ("toString", 88, "() => string", Some("^^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn unique_symbol_value_loose() {
    assert_declarations("declare const value: unique symbol; const method = value.toString; const result = value.toString();", false, "es2020", &[
        ("value", 14, "unique symbol", Some("^^^^^^^^^^^^^")),
        ("method", 42, "() => string", Some("^^^^^^      ")),
        ("value.toString", 51, "() => string", Some("^^^^^^      ")),
        ("value", 51, "unique symbol", Some("^^^^^^^^^^^^^")),
        ("toString", 57, "() => string", Some("^^^^^^      ")),
        ("result", 73, "string", Some("^^^^^^")),
        ("value.toString()", 82, "string", Some("^^^^^^")),
        ("value.toString", 82, "() => string", Some("^^^^^^      ")),
        ("value", 82, "unique symbol", Some("^^^^^^^^^^^^^")),
        ("toString", 88, "() => string", Some("^^^^^^      ")),
    ], &[
    ]);
}

#[test]
fn string_wrong_argument_strict() {
    assert_declarations(
        "declare const value: string; value.substring(\"wrong\");",
        true,
        "es2020",
        &[
            ("value", 14, "string", Some("^^^^^^")),
            ("value.substring(\"wrong\")", 29, "string", Some("^^^^^^")),
            (
                "value.substring",
                29,
                "(start: number, end?: number) => string",
                Some("^     ^^      ^^   ^^^      ^^^^^      "),
            ),
            ("value", 29, "string", Some("^^^^^^")),
            (
                "substring",
                35,
                "(start: number, end?: number) => string",
                Some("^     ^^      ^^   ^^^      ^^^^^      "),
            ),
            ("\"wrong\"", 45, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2345, 45, 7)],
    );
}

#[test]
fn string_wrong_argument_loose() {
    assert_declarations(
        "declare const value: string; value.substring(\"wrong\");",
        false,
        "es2020",
        &[
            ("value", 14, "string", Some("^^^^^^")),
            ("value.substring(\"wrong\")", 29, "string", Some("^^^^^^")),
            (
                "value.substring",
                29,
                "(start: number, end?: number) => string",
                Some("^     ^^      ^^   ^^^      ^^^^^      "),
            ),
            ("value", 29, "string", Some("^^^^^^")),
            (
                "substring",
                35,
                "(start: number, end?: number) => string",
                Some("^     ^^      ^^   ^^^      ^^^^^      "),
            ),
            ("\"wrong\"", 45, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2345, 45, 7)],
    );
}

#[test]
fn number_wrong_argument_strict() {
    assert_declarations(
        "declare const value: number; value.toFixed(\"wrong\");",
        true,
        "es2020",
        &[
            ("value", 14, "number", Some("^^^^^^")),
            ("value.toFixed(\"wrong\")", 29, "string", Some("^^^^^^")),
            (
                "value.toFixed",
                29,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("value", 29, "number", Some("^^^^^^")),
            (
                "toFixed",
                35,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("\"wrong\"", 43, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2345, 43, 7)],
    );
}

#[test]
fn number_wrong_argument_loose() {
    assert_declarations(
        "declare const value: number; value.toFixed(\"wrong\");",
        false,
        "es2020",
        &[
            ("value", 14, "number", Some("^^^^^^")),
            ("value.toFixed(\"wrong\")", 29, "string", Some("^^^^^^")),
            (
                "value.toFixed",
                29,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("value", 29, "number", Some("^^^^^^")),
            (
                "toFixed",
                35,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("\"wrong\"", 43, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2345, 43, 7)],
    );
}

#[test]
fn number_too_many_arguments_strict() {
    assert_declarations(
        "declare const value: number; value.toFixed(1, 2);",
        true,
        "es2020",
        &[
            ("value", 14, "number", Some("^^^^^^")),
            ("value.toFixed(1, 2)", 29, "string", Some("^^^^^^")),
            (
                "value.toFixed",
                29,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("value", 29, "number", Some("^^^^^^")),
            (
                "toFixed",
                35,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("1", 43, "1", Some("^")),
            ("2", 46, "2", Some("^")),
        ],
        &[(2554, 46, 1)],
    );
}

#[test]
fn number_too_many_arguments_loose() {
    assert_declarations(
        "declare const value: number; value.toFixed(1, 2);",
        false,
        "es2020",
        &[
            ("value", 14, "number", Some("^^^^^^")),
            ("value.toFixed(1, 2)", 29, "string", Some("^^^^^^")),
            (
                "value.toFixed",
                29,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("value", 29, "number", Some("^^^^^^")),
            (
                "toFixed",
                35,
                "(fractionDigits?: number) => string",
                Some("^              ^^^      ^^^^^      "),
            ),
            ("1", 43, "1", Some("^")),
            ("2", 46, "2", Some("^")),
        ],
        &[(2554, 46, 1)],
    );
}

#[test]
fn string_replace_rest_function_strict() {
    assert_declarations("declare const value: string; declare const args: any[]; const result = value.replace(/a/, function(match, ...rest) { const index = rest[0]; return typeof args[index] !== \"undefined\" ? args[index] : match; });", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("args", 43, "any[]", Some("^^^^^")),
        ("result", 62, "string", Some("^^^^^^")),
        ("value.replace(/a/, function(match, ...rest) { const index = rest[0]; return typeof args[index] !== \"undefined\" ? args[index] : match; })", 71, "string", Some("^^^^^^")),
        ("value.replace", 71, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 71, "string", Some("^^^^^^")),
        ("replace", 77, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 85, "RegExp", Some("^^^^^^")),
        ("function(match, ...rest) { const index = rest[0]; return typeof args[index] !== \"undefined\" ? args[index] : match; }", 90, "(match: string, ...rest: any[]) => any", Some("^     ^^^^^^^^^^^^^    ^^^^^^^^^^^^^^^")),
        ("match", 99, "string", Some("^^^^^^")),
        ("rest", 109, "any[]", Some("^^^^^")),
        ("index", 123, "any", None),
        ("rest[0]", 131, "any", None),
        ("rest", 131, "any[]", Some("^^^^^")),
        ("0", 136, "0", Some("^")),
        ("typeof args[index] !== \"undefined\" ? args[index] : match", 147, "any", None),
        ("typeof args[index] !== \"undefined\"", 147, "boolean", Some("^^^^^^^")),
        ("typeof args[index]", 147, "\"string\" | \"number\" | \"bigint\" | \"boolean\" | \"symbol\" | \"undefined\" | \"object\" | \"function\"", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("args[index]", 154, "any", None),
        ("args", 154, "any[]", Some("^^^^^")),
        ("index", 159, "any", None),
        ("\"undefined\"", 170, "\"undefined\"", Some("^^^^^^^^^^^")),
        ("args[index]", 184, "any", None),
        ("args", 184, "any[]", Some("^^^^^")),
        ("index", 189, "any", None),
        ("match", 198, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_replace_rest_function_loose() {
    assert_declarations("declare const value: string; declare const args: any[]; const result = value.replace(/a/, function(match, ...rest) { const index = rest[0]; return typeof args[index] !== \"undefined\" ? args[index] : match; });", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("args", 43, "any[]", Some("^^^^^")),
        ("result", 62, "string", Some("^^^^^^")),
        ("value.replace(/a/, function(match, ...rest) { const index = rest[0]; return typeof args[index] !== \"undefined\" ? args[index] : match; })", 71, "string", Some("^^^^^^")),
        ("value.replace", 71, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 71, "string", Some("^^^^^^")),
        ("replace", 77, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 85, "RegExp", Some("^^^^^^")),
        ("function(match, ...rest) { const index = rest[0]; return typeof args[index] !== \"undefined\" ? args[index] : match; }", 90, "(match: string, ...rest: any[]) => any", Some("^     ^^^^^^^^^^^^^    ^^^^^^^^^^^^^^^")),
        ("match", 99, "string", Some("^^^^^^")),
        ("rest", 109, "any[]", Some("^^^^^")),
        ("index", 123, "any", None),
        ("rest[0]", 131, "any", None),
        ("rest", 131, "any[]", Some("^^^^^")),
        ("0", 136, "0", Some("^")),
        ("typeof args[index] !== \"undefined\" ? args[index] : match", 147, "any", None),
        ("typeof args[index] !== \"undefined\"", 147, "boolean", Some("^^^^^^^")),
        ("typeof args[index]", 147, "\"string\" | \"number\" | \"bigint\" | \"boolean\" | \"symbol\" | \"undefined\" | \"object\" | \"function\"", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("args[index]", 154, "any", None),
        ("args", 154, "any[]", Some("^^^^^")),
        ("index", 159, "any", None),
        ("\"undefined\"", 170, "\"undefined\"", Some("^^^^^^^^^^^")),
        ("args[index]", 184, "any", None),
        ("args", 184, "any[]", Some("^^^^^")),
        ("index", 189, "any", None),
        ("match", 198, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_replace_rest_arrow_strict() {
    assert_declarations("declare const value: string; const result = value.replace(/a/, (match, ...rest) => { const first = rest[0]; return match; });", true, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("result", 35, "string", Some("^^^^^^")),
        ("value.replace(/a/, (match, ...rest) => { const first = rest[0]; return match; })", 44, "string", Some("^^^^^^")),
        ("value.replace", 44, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("replace", 50, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 58, "RegExp", Some("^^^^^^")),
        ("(match, ...rest) => { const first = rest[0]; return match; }", 63, "(match: string, ...rest: any[]) => string", Some("^     ^^^^^^^^^^^^^    ^^^^^^^^^^^^^^^^^^")),
        ("match", 64, "string", Some("^^^^^^")),
        ("rest", 74, "any[]", Some("^^^^^")),
        ("first", 91, "any", None),
        ("rest[0]", 99, "any", None),
        ("rest", 99, "any[]", Some("^^^^^")),
        ("0", 104, "0", Some("^")),
        ("match", 115, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn string_replace_rest_arrow_loose() {
    assert_declarations("declare const value: string; const result = value.replace(/a/, (match, ...rest) => { const first = rest[0]; return match; });", false, "es2020", &[
        ("value", 14, "string", Some("^^^^^^")),
        ("result", 35, "string", Some("^^^^^^")),
        ("value.replace(/a/, (match, ...rest) => { const first = rest[0]; return match; })", 44, "string", Some("^^^^^^")),
        ("value.replace", 44, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("value", 44, "string", Some("^^^^^^")),
        ("replace", 50, "{ (searchValue: string | RegExp, replaceValue: string): string; (searchValue: string | RegExp, replacer: (substring: string, ...args: any[]) => string): string; (searchValue: { [Symbol.replace](string: string, replaceValue: string): string; }, replaceValue: string): string; (searchValue: { [Symbol.replace](string: string, replacer: (substring: string, ...args: any[]) => string): string; }, replacer: (substring: string, ...args: any[]) => string): string; }", Some("^^^           ^^               ^^            ^^      ^^^      ^^^           ^^               ^^        ^^                                             ^^^      ^^^           ^^                                                                   ^^            ^^      ^^^      ^^^           ^^                                                                                                      ^^        ^^                                             ^^^      ^^^")),
        ("/a/", 58, "RegExp", Some("^^^^^^")),
        ("(match, ...rest) => { const first = rest[0]; return match; }", 63, "(match: string, ...rest: any[]) => string", Some("^     ^^^^^^^^^^^^^    ^^^^^^^^^^^^^^^^^^")),
        ("match", 64, "string", Some("^^^^^^")),
        ("rest", 74, "any[]", Some("^^^^^")),
        ("first", 91, "any", None),
        ("rest[0]", 99, "any", None),
        ("rest", 99, "any[]", Some("^^^^^")),
        ("0", 104, "0", Some("^")),
        ("match", 115, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn contextual_rest_after_position_strict() {
    assert_declarations("declare function use(callback: (first: string, ...items: number[]) => number): void; use((first, second, ...remaining) => remaining[0]);", true, "es2020", &[
        ("use", 17, "(callback: (first: string, ...items: number[]) => number) => void", Some("^        ^^                                             ^^^^^    ")),
        ("callback", 21, "(first: string, ...items: number[]) => number", Some("^     ^^      ^^^^^     ^^        ^^^^^      ")),
        ("first", 32, "string", Some("^^^^^^")),
        ("items", 50, "number[]", Some("^^^^^^^^")),
        ("use((first, second, ...remaining) => remaining[0])", 85, "void", Some("^^^^")),
        ("use", 85, "(callback: (first: string, ...items: number[]) => number) => void", Some("^        ^^                                             ^^^^^    ")),
        ("(first, second, ...remaining) => remaining[0]", 89, "(first: string, second: number, ...remaining: number[]) => number", Some("^     ^^^^^^^^^^      ^^^^^^^^^^^^^         ^^^^^^^^^^^^^^^^^^^^^")),
        ("first", 90, "string", Some("^^^^^^")),
        ("second", 97, "number", Some("^^^^^^")),
        ("remaining", 108, "number[]", Some("^^^^^^^^")),
        ("remaining[0]", 122, "number", Some("^^^^^^")),
        ("remaining", 122, "number[]", Some("^^^^^^^^")),
        ("0", 132, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn contextual_rest_after_position_loose() {
    assert_declarations("declare function use(callback: (first: string, ...items: number[]) => number): void; use((first, second, ...remaining) => remaining[0]);", false, "es2020", &[
        ("use", 17, "(callback: (first: string, ...items: number[]) => number) => void", Some("^        ^^                                             ^^^^^    ")),
        ("callback", 21, "(first: string, ...items: number[]) => number", Some("^     ^^      ^^^^^     ^^        ^^^^^      ")),
        ("first", 32, "string", Some("^^^^^^")),
        ("items", 50, "number[]", Some("^^^^^^^^")),
        ("use((first, second, ...remaining) => remaining[0])", 85, "void", Some("^^^^")),
        ("use", 85, "(callback: (first: string, ...items: number[]) => number) => void", Some("^        ^^                                             ^^^^^    ")),
        ("(first, second, ...remaining) => remaining[0]", 89, "(first: string, second: number, ...remaining: number[]) => number", Some("^     ^^^^^^^^^^      ^^^^^^^^^^^^^         ^^^^^^^^^^^^^^^^^^^^^")),
        ("first", 90, "string", Some("^^^^^^")),
        ("second", 97, "number", Some("^^^^^^")),
        ("remaining", 108, "number[]", Some("^^^^^^^^")),
        ("remaining[0]", 122, "number", Some("^^^^^^")),
        ("remaining", 122, "number[]", Some("^^^^^^^^")),
        ("0", 132, "0", Some("^")),
    ], &[
    ]);
}
