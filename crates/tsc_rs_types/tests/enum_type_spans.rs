use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

// Original TypeScript 6.0.3 TypeWriterWalker text and synthetic-node masks.
fn assert_declarations(
    source: &str,
    strict: bool,
    library: &str,
    unchecked: bool,
    expected: &[(&str, usize, &str, Option<&str>)],
    diagnostics: &[(u32, u32, u32)],
) {
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        strict: Some(strict),
        no_unchecked_indexed_access: Some(unchecked),
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
fn numeric_strict() {
    assert_declarations(
        "enum E { A, B, C = 10 } let x: E; const a = E.A; let b = E.B; E; E.A; E[\"B\"];",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("C", 15, "E.C", Some("^^^")),
            ("10", 19, "10", Some("^^")),
            ("x", 28, "E", Some("^")),
            ("a", 40, "E.A", Some("^^^")),
            ("E.A", 44, "E.A", Some("^^^")),
            ("E", 44, "typeof E", Some("^^^^^^^^")),
            ("A", 46, "E.A", Some("^^^")),
            ("b", 53, "E", Some("^")),
            ("E.B", 57, "E.B", Some("^^^")),
            ("E", 57, "typeof E", Some("^^^^^^^^")),
            ("B", 59, "E.B", Some("^^^")),
            ("E", 62, "typeof E", Some("^^^^^^^^")),
            ("E.A", 65, "E.A", Some("^^^")),
            ("E", 65, "typeof E", Some("^^^^^^^^")),
            ("A", 67, "E.A", Some("^^^")),
            ("E[\"B\"]", 70, "E.B", Some("^^^")),
            ("E", 70, "typeof E", Some("^^^^^^^^")),
            ("\"B\"", 72, "\"B\"", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn numeric_loose() {
    assert_declarations(
        "enum E { A, B, C = 10 } let x: E; const a = E.A; let b = E.B; E; E.A; E[\"B\"];",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("C", 15, "E.C", Some("^^^")),
            ("10", 19, "10", Some("^^")),
            ("x", 28, "E", Some("^")),
            ("a", 40, "E.A", Some("^^^")),
            ("E.A", 44, "E.A", Some("^^^")),
            ("E", 44, "typeof E", Some("^^^^^^^^")),
            ("A", 46, "E.A", Some("^^^")),
            ("b", 53, "E", Some("^")),
            ("E.B", 57, "E.B", Some("^^^")),
            ("E", 57, "typeof E", Some("^^^^^^^^")),
            ("B", 59, "E.B", Some("^^^")),
            ("E", 62, "typeof E", Some("^^^^^^^^")),
            ("E.A", 65, "E.A", Some("^^^")),
            ("E", 65, "typeof E", Some("^^^^^^^^")),
            ("A", 67, "E.A", Some("^^^")),
            ("E[\"B\"]", 70, "E.B", Some("^^^")),
            ("E", 70, "typeof E", Some("^^^^^^^^")),
            ("\"B\"", 72, "\"B\"", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn string_strict() {
    assert_declarations(
        "enum E { A = \"a\", B = \"b\" } let x: E; const a = E.A; let b = E.B; E; E.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("\"a\"", 13, "\"a\"", Some("^^^")),
            ("B", 18, "E.B", Some("^^^")),
            ("\"b\"", 22, "\"b\"", Some("^^^")),
            ("x", 32, "E", Some("^")),
            ("a", 44, "E.A", Some("^^^")),
            ("E.A", 48, "E.A", Some("^^^")),
            ("E", 48, "typeof E", Some("^^^^^^^^")),
            ("A", 50, "E.A", Some("^^^")),
            ("b", 57, "E", Some("^")),
            ("E.B", 61, "E.B", Some("^^^")),
            ("E", 61, "typeof E", Some("^^^^^^^^")),
            ("B", 63, "E.B", Some("^^^")),
            ("E", 66, "typeof E", Some("^^^^^^^^")),
            ("E.A", 69, "E.A", Some("^^^")),
            ("E", 69, "typeof E", Some("^^^^^^^^")),
            ("A", 71, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn string_loose() {
    assert_declarations(
        "enum E { A = \"a\", B = \"b\" } let x: E; const a = E.A; let b = E.B; E; E.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("\"a\"", 13, "\"a\"", Some("^^^")),
            ("B", 18, "E.B", Some("^^^")),
            ("\"b\"", 22, "\"b\"", Some("^^^")),
            ("x", 32, "E", Some("^")),
            ("a", 44, "E.A", Some("^^^")),
            ("E.A", 48, "E.A", Some("^^^")),
            ("E", 48, "typeof E", Some("^^^^^^^^")),
            ("A", 50, "E.A", Some("^^^")),
            ("b", 57, "E", Some("^")),
            ("E.B", 61, "E.B", Some("^^^")),
            ("E", 61, "typeof E", Some("^^^^^^^^")),
            ("B", 63, "E.B", Some("^^^")),
            ("E", 66, "typeof E", Some("^^^^^^^^")),
            ("E.A", 69, "E.A", Some("^^^")),
            ("E", 69, "typeof E", Some("^^^^^^^^")),
            ("A", 71, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn single_strict() {
    assert_declarations(
        "enum E { A = 2 } let x: E; const a = E.A; E.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("2", 13, "2", Some("^")),
            ("x", 21, "E", Some("^")),
            ("a", 33, "E.A", Some("^^^")),
            ("E.A", 37, "E", Some("^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("A", 39, "E", Some("^")),
            ("E.A", 42, "E", Some("^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("A", 44, "E", Some("^")),
        ],
        &[],
    );
}

#[test]
fn single_loose() {
    assert_declarations(
        "enum E { A = 2 } let x: E; const a = E.A; E.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("2", 13, "2", Some("^")),
            ("x", 21, "E", Some("^")),
            ("a", 33, "E.A", Some("^^^")),
            ("E.A", 37, "E", Some("^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("A", 39, "E", Some("^")),
            ("E.A", 42, "E", Some("^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("A", 44, "E", Some("^")),
        ],
        &[],
    );
}

#[test]
fn empty_strict() {
    assert_declarations(
        "enum E {} let x: E; E;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("x", 14, "E", Some("^")),
            ("E", 20, "typeof E", Some("^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn empty_loose() {
    assert_declarations(
        "enum E {} let x: E; E;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("x", 14, "E", Some("^")),
            ("E", 20, "typeof E", Some("^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn ambient_strict() {
    assert_declarations(
        "declare enum E { A, B = 3, C } let x: E; E; E.A; E.B; E.C;",
        true,
        "es2020",
        false,
        &[
            ("E", 13, "E", Some("^")),
            ("A", 17, "E.A", Some("^^^")),
            ("B", 20, "E.B", Some("^^^")),
            ("3", 24, "3", Some("^")),
            ("C", 27, "E.C", Some("^^^")),
            ("x", 35, "E", Some("^")),
            ("E", 41, "typeof E", Some("^^^^^^^^")),
            ("E.A", 44, "E.A", Some("^^^")),
            ("E", 44, "typeof E", Some("^^^^^^^^")),
            ("A", 46, "E.A", Some("^^^")),
            ("E.B", 49, "E.B", Some("^^^")),
            ("E", 49, "typeof E", Some("^^^^^^^^")),
            ("B", 51, "E.B", Some("^^^")),
            ("E.C", 54, "E.C", Some("^^^")),
            ("E", 54, "typeof E", Some("^^^^^^^^")),
            ("C", 56, "E.C", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn ambient_loose() {
    assert_declarations(
        "declare enum E { A, B = 3, C } let x: E; E; E.A; E.B; E.C;",
        false,
        "es2020",
        false,
        &[
            ("E", 13, "E", Some("^")),
            ("A", 17, "E.A", Some("^^^")),
            ("B", 20, "E.B", Some("^^^")),
            ("3", 24, "3", Some("^")),
            ("C", 27, "E.C", Some("^^^")),
            ("x", 35, "E", Some("^")),
            ("E", 41, "typeof E", Some("^^^^^^^^")),
            ("E.A", 44, "E.A", Some("^^^")),
            ("E", 44, "typeof E", Some("^^^^^^^^")),
            ("A", 46, "E.A", Some("^^^")),
            ("E.B", 49, "E.B", Some("^^^")),
            ("E", 49, "typeof E", Some("^^^^^^^^")),
            ("B", 51, "E.B", Some("^^^")),
            ("E.C", 54, "E.C", Some("^^^")),
            ("E", 54, "typeof E", Some("^^^^^^^^")),
            ("C", 56, "E.C", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn constant_strict() {
    assert_declarations(
        "const enum E { A, B = A + 1, C = B << 2 } let x = E.C;",
        true,
        "es2020",
        false,
        &[
            ("E", 11, "E", Some("^")),
            ("A", 15, "E.A", Some("^^^")),
            ("B", 18, "E.B", Some("^^^")),
            ("A + 1", 22, "number", Some("^^^^^^")),
            ("A", 22, "E.A", Some("^^^")),
            ("1", 26, "1", Some("^")),
            ("C", 29, "E.C", Some("^^^")),
            ("B << 2", 33, "number", Some("^^^^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("2", 38, "2", Some("^")),
            ("x", 46, "E", Some("^")),
            ("E.C", 50, "E.C", Some("^^^")),
            ("E", 50, "typeof E", Some("^^^^^^^^")),
            ("C", 52, "E.C", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn constant_loose() {
    assert_declarations(
        "const enum E { A, B = A + 1, C = B << 2 } let x = E.C;",
        false,
        "es2020",
        false,
        &[
            ("E", 11, "E", Some("^")),
            ("A", 15, "E.A", Some("^^^")),
            ("B", 18, "E.B", Some("^^^")),
            ("A + 1", 22, "number", Some("^^^^^^")),
            ("A", 22, "E.A", Some("^^^")),
            ("1", 26, "1", Some("^")),
            ("C", 29, "E.C", Some("^^^")),
            ("B << 2", 33, "number", Some("^^^^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("2", 38, "2", Some("^")),
            ("x", 46, "E", Some("^")),
            ("E.C", 50, "E.C", Some("^^^")),
            ("E", 50, "typeof E", Some("^^^^^^^^")),
            ("C", 52, "E.C", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn duplicates_strict() {
    assert_declarations(
        "enum E { A = 1, B = 1, C = \"x\", D = \"x\" } E.A; E.B; E.C; E.D;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("1", 13, "1", Some("^")),
            ("B", 16, "E.A", Some("^^^")),
            ("1", 20, "1", Some("^")),
            ("C", 23, "E.C", Some("^^^")),
            ("\"x\"", 27, "\"x\"", Some("^^^")),
            ("D", 32, "E.C", Some("^^^")),
            ("\"x\"", 36, "\"x\"", Some("^^^")),
            ("E.A", 42, "E.A", Some("^^^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("A", 44, "E.A", Some("^^^")),
            ("E.B", 47, "E.A", Some("^^^")),
            ("E", 47, "typeof E", Some("^^^^^^^^")),
            ("B", 49, "E.A", Some("^^^")),
            ("E.C", 52, "E.C", Some("^^^")),
            ("E", 52, "typeof E", Some("^^^^^^^^")),
            ("C", 54, "E.C", Some("^^^")),
            ("E.D", 57, "E.C", Some("^^^")),
            ("E", 57, "typeof E", Some("^^^^^^^^")),
            ("D", 59, "E.C", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn duplicates_loose() {
    assert_declarations(
        "enum E { A = 1, B = 1, C = \"x\", D = \"x\" } E.A; E.B; E.C; E.D;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("1", 13, "1", Some("^")),
            ("B", 16, "E.A", Some("^^^")),
            ("1", 20, "1", Some("^")),
            ("C", 23, "E.C", Some("^^^")),
            ("\"x\"", 27, "\"x\"", Some("^^^")),
            ("D", 32, "E.C", Some("^^^")),
            ("\"x\"", 36, "\"x\"", Some("^^^")),
            ("E.A", 42, "E.A", Some("^^^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("A", 44, "E.A", Some("^^^")),
            ("E.B", 47, "E.A", Some("^^^")),
            ("E", 47, "typeof E", Some("^^^^^^^^")),
            ("B", 49, "E.A", Some("^^^")),
            ("E.C", 52, "E.C", Some("^^^")),
            ("E", 52, "typeof E", Some("^^^^^^^^")),
            ("C", 54, "E.C", Some("^^^")),
            ("E.D", 57, "E.C", Some("^^^")),
            ("E", 57, "typeof E", Some("^^^^^^^^")),
            ("D", 59, "E.C", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn member_initializers_strict() {
    assert_declarations(
        "enum E { A = 2, B = A, C = E.A + B, D = C * 2 } E.D;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("2", 13, "2", Some("^")),
            ("B", 16, "E.A", Some("^^^")),
            ("A", 20, "E.A", Some("^^^")),
            ("C", 23, "E.C", Some("^^^")),
            ("E.A + B", 27, "number", Some("^^^^^^")),
            ("E.A", 27, "E.A", Some("^^^")),
            ("E", 27, "typeof E", Some("^^^^^^^^")),
            ("A", 29, "E.A", Some("^^^")),
            ("B", 33, "E.A", Some("^^^")),
            ("D", 36, "E.D", Some("^^^")),
            ("C * 2", 40, "number", Some("^^^^^^")),
            ("C", 40, "E.C", Some("^^^")),
            ("2", 44, "2", Some("^")),
            ("E.D", 48, "E.D", Some("^^^")),
            ("E", 48, "typeof E", Some("^^^^^^^^")),
            ("D", 50, "E.D", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn member_initializers_loose() {
    assert_declarations(
        "enum E { A = 2, B = A, C = E.A + B, D = C * 2 } E.D;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("2", 13, "2", Some("^")),
            ("B", 16, "E.A", Some("^^^")),
            ("A", 20, "E.A", Some("^^^")),
            ("C", 23, "E.C", Some("^^^")),
            ("E.A + B", 27, "number", Some("^^^^^^")),
            ("E.A", 27, "E.A", Some("^^^")),
            ("E", 27, "typeof E", Some("^^^^^^^^")),
            ("A", 29, "E.A", Some("^^^")),
            ("B", 33, "E.A", Some("^^^")),
            ("D", 36, "E.D", Some("^^^")),
            ("C * 2", 40, "number", Some("^^^^^^")),
            ("C", 40, "E.C", Some("^^^")),
            ("2", 44, "2", Some("^")),
            ("E.D", 48, "E.D", Some("^^^")),
            ("E", 48, "typeof E", Some("^^^^^^^^")),
            ("D", 50, "E.D", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn computed_strict() {
    assert_declarations(
        "declare function next(): number; enum E { A = next(), B = next() } E.A; E.B;",
        true,
        "es2020",
        false,
        &[
            ("next", 17, "() => number", Some("^^^^^^      ")),
            ("E", 38, "E", Some("^")),
            ("A", 42, "E.A", Some("^^^")),
            ("next()", 46, "number", Some("^^^^^^")),
            ("next", 46, "() => number", Some("^^^^^^      ")),
            ("B", 54, "E.B", Some("^^^")),
            ("next()", 58, "number", Some("^^^^^^")),
            ("next", 58, "() => number", Some("^^^^^^      ")),
            ("E.A", 67, "E.A", Some("^^^")),
            ("E", 67, "typeof E", Some("^^^^^^^^")),
            ("A", 69, "E.A", Some("^^^")),
            ("E.B", 72, "E.B", Some("^^^")),
            ("E", 72, "typeof E", Some("^^^^^^^^")),
            ("B", 74, "E.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn computed_loose() {
    assert_declarations(
        "declare function next(): number; enum E { A = next(), B = next() } E.A; E.B;",
        false,
        "es2020",
        false,
        &[
            ("next", 17, "() => number", Some("^^^^^^      ")),
            ("E", 38, "E", Some("^")),
            ("A", 42, "E.A", Some("^^^")),
            ("next()", 46, "number", Some("^^^^^^")),
            ("next", 46, "() => number", Some("^^^^^^      ")),
            ("B", 54, "E.B", Some("^^^")),
            ("next()", 58, "number", Some("^^^^^^")),
            ("next", 58, "() => number", Some("^^^^^^      ")),
            ("E.A", 67, "E.A", Some("^^^")),
            ("E", 67, "typeof E", Some("^^^^^^^^")),
            ("A", 69, "E.A", Some("^^^")),
            ("E.B", 72, "E.B", Some("^^^")),
            ("E", 72, "typeof E", Some("^^^^^^^^")),
            ("B", 74, "E.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn quoted_strict() {
    assert_declarations(
        "enum E { \"a-b\" = 1, \"with space\" = 2 } E[\"a-b\"]; E[\"with space\"];",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            (
                "\"a-b\"",
                9,
                "(typeof E)[\"a-b\"]",
                Some("^^^^^^^^^^^^^^^^^"),
            ),
            ("1", 17, "1", Some("^")),
            (
                "\"with space\"",
                20,
                "(typeof E)[\"with space\"]",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("2", 35, "2", Some("^")),
            (
                "E[\"a-b\"]",
                39,
                "(typeof E)[\"a-b\"]",
                Some("^^^^^^^^^^^^^^^^^"),
            ),
            ("E", 39, "typeof E", Some("^^^^^^^^")),
            ("\"a-b\"", 41, "\"a-b\"", Some("^^^^^")),
            (
                "E[\"with space\"]",
                49,
                "(typeof E)[\"with space\"]",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("E", 49, "typeof E", Some("^^^^^^^^")),
            ("\"with space\"", 51, "\"with space\"", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn quoted_loose() {
    assert_declarations(
        "enum E { \"a-b\" = 1, \"with space\" = 2 } E[\"a-b\"]; E[\"with space\"];",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            (
                "\"a-b\"",
                9,
                "(typeof E)[\"a-b\"]",
                Some("^^^^^^^^^^^^^^^^^"),
            ),
            ("1", 17, "1", Some("^")),
            (
                "\"with space\"",
                20,
                "(typeof E)[\"with space\"]",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("2", 35, "2", Some("^")),
            (
                "E[\"a-b\"]",
                39,
                "(typeof E)[\"a-b\"]",
                Some("^^^^^^^^^^^^^^^^^"),
            ),
            ("E", 39, "typeof E", Some("^^^^^^^^")),
            ("\"a-b\"", 41, "\"a-b\"", Some("^^^^^")),
            (
                "E[\"with space\"]",
                49,
                "(typeof E)[\"with space\"]",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("E", 49, "typeof E", Some("^^^^^^^^")),
            ("\"with space\"", 51, "\"with space\"", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn merged_strict() {
    assert_declarations(
        "enum E { A = 1 } enum E { B = 2 } E; E.A; E.B;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("1", 13, "1", Some("^")),
            ("E", 22, "E", Some("^")),
            ("B", 26, "E.B", Some("^^^")),
            ("2", 30, "2", Some("^")),
            ("E", 34, "typeof E", Some("^^^^^^^^")),
            ("E.A", 37, "E.A", Some("^^^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("A", 39, "E.A", Some("^^^")),
            ("E.B", 42, "E.B", Some("^^^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("B", 44, "E.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn merged_loose() {
    assert_declarations(
        "enum E { A = 1 } enum E { B = 2 } E; E.A; E.B;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("1", 13, "1", Some("^")),
            ("E", 22, "E", Some("^")),
            ("B", 26, "E.B", Some("^^^")),
            ("2", 30, "2", Some("^")),
            ("E", 34, "typeof E", Some("^^^^^^^^")),
            ("E.A", 37, "E.A", Some("^^^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("A", 39, "E.A", Some("^^^")),
            ("E.B", 42, "E.B", Some("^^^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("B", 44, "E.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_strict() {
    assert_declarations(
        "namespace N { export enum E { A, B } E; E.A; } N.E; N.E.A; let x: N.E;",
        true,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("E", 26, "E", Some("^")),
            ("A", 30, "E.A", Some("^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("E.A", 40, "E.A", Some("^^^")),
            ("E", 40, "typeof E", Some("^^^^^^^^")),
            ("A", 42, "E.A", Some("^^^")),
            ("N.E", 47, "typeof N.E", Some("^^^^^^^^^^")),
            ("N", 47, "typeof N", Some("^^^^^^^^")),
            ("E", 49, "typeof N.E", Some("^^^^^^^^^^")),
            ("N.E.A", 52, "N.E.A", Some("^^^^^")),
            ("N.E", 52, "typeof N.E", Some("^^^^^^^^^^")),
            ("N", 52, "typeof N", Some("^^^^^^^^")),
            ("E", 54, "typeof N.E", Some("^^^^^^^^^^")),
            ("A", 56, "N.E.A", Some("^^^^^")),
            ("x", 63, "N.E", Some("^^^")),
            ("N", 66, "any", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_loose() {
    assert_declarations(
        "namespace N { export enum E { A, B } E; E.A; } N.E; N.E.A; let x: N.E;",
        false,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("E", 26, "E", Some("^")),
            ("A", 30, "E.A", Some("^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("E.A", 40, "E.A", Some("^^^")),
            ("E", 40, "typeof E", Some("^^^^^^^^")),
            ("A", 42, "E.A", Some("^^^")),
            ("N.E", 47, "typeof N.E", Some("^^^^^^^^^^")),
            ("N", 47, "typeof N", Some("^^^^^^^^")),
            ("E", 49, "typeof N.E", Some("^^^^^^^^^^")),
            ("N.E.A", 52, "N.E.A", Some("^^^^^")),
            ("N.E", 52, "typeof N.E", Some("^^^^^^^^^^")),
            ("N", 52, "typeof N", Some("^^^^^^^^")),
            ("E", 54, "typeof N.E", Some("^^^^^^^^^^")),
            ("A", 56, "N.E.A", Some("^^^^^")),
            ("x", 63, "N.E", Some("^^^")),
            ("N", 66, "any", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn nested_namespace_strict() {
    assert_declarations(
        "namespace N.M { export enum E { A, B } E.A; } N.M.E; N.M.E.A;",
        true,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("M", 12, "typeof M", Some("^^^^^^^^")),
            ("E", 28, "E", Some("^")),
            ("A", 32, "E.A", Some("^^^")),
            ("B", 35, "E.B", Some("^^^")),
            ("E.A", 39, "E.A", Some("^^^")),
            ("E", 39, "typeof E", Some("^^^^^^^^")),
            ("A", 41, "E.A", Some("^^^")),
            ("N.M.E", 46, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("N.M", 46, "typeof N.M", Some("^^^^^^^^^^")),
            ("N", 46, "typeof N", Some("^^^^^^^^")),
            ("M", 48, "typeof N.M", Some("^^^^^^^^^^")),
            ("E", 50, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("N.M.E.A", 53, "N.M.E.A", Some("^^^^^^^")),
            ("N.M.E", 53, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("N.M", 53, "typeof N.M", Some("^^^^^^^^^^")),
            ("N", 53, "typeof N", Some("^^^^^^^^")),
            ("M", 55, "typeof N.M", Some("^^^^^^^^^^")),
            ("E", 57, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("A", 59, "N.M.E.A", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn nested_namespace_loose() {
    assert_declarations(
        "namespace N.M { export enum E { A, B } E.A; } N.M.E; N.M.E.A;",
        false,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("M", 12, "typeof M", Some("^^^^^^^^")),
            ("E", 28, "E", Some("^")),
            ("A", 32, "E.A", Some("^^^")),
            ("B", 35, "E.B", Some("^^^")),
            ("E.A", 39, "E.A", Some("^^^")),
            ("E", 39, "typeof E", Some("^^^^^^^^")),
            ("A", 41, "E.A", Some("^^^")),
            ("N.M.E", 46, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("N.M", 46, "typeof N.M", Some("^^^^^^^^^^")),
            ("N", 46, "typeof N", Some("^^^^^^^^")),
            ("M", 48, "typeof N.M", Some("^^^^^^^^^^")),
            ("E", 50, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("N.M.E.A", 53, "N.M.E.A", Some("^^^^^^^")),
            ("N.M.E", 53, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("N.M", 53, "typeof N.M", Some("^^^^^^^^^^")),
            ("N", 53, "typeof N", Some("^^^^^^^^")),
            ("M", 55, "typeof N.M", Some("^^^^^^^^^^")),
            ("E", 57, "typeof N.M.E", Some("^^^^^^^^^^^^")),
            ("A", 59, "N.M.E.A", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn local_shadow_strict() {
    assert_declarations(
        "enum E { A, B } function f(E: number) { return E; } E; E.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            (
                "f",
                25,
                "(E: number) => number",
                Some("^ ^^      ^^^^^^^^^^^"),
            ),
            ("E", 27, "number", Some("^^^^^^")),
            ("E", 47, "number", Some("^^^^^^")),
            ("E", 52, "typeof E", Some("^^^^^^^^")),
            ("E.A", 55, "E.A", Some("^^^")),
            ("E", 55, "typeof E", Some("^^^^^^^^")),
            ("A", 57, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn local_shadow_loose() {
    assert_declarations(
        "enum E { A, B } function f(E: number) { return E; } E; E.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            (
                "f",
                25,
                "(E: number) => number",
                Some("^ ^^      ^^^^^^^^^^^"),
            ),
            ("E", 27, "number", Some("^^^^^^")),
            ("E", 47, "number", Some("^^^^^^")),
            ("E", 52, "typeof E", Some("^^^^^^^^")),
            ("E.A", 55, "E.A", Some("^^^")),
            ("E", 55, "typeof E", Some("^^^^^^^^")),
            ("A", 57, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn nested_enum_strict() {
    assert_declarations(
        "enum E { A = 1 } function f() { enum E { A = \"a\", B = \"b\" } return E.A; } E.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("1", 13, "1", Some("^")),
            ("f", 26, "() => E", Some("^^^^^^^")),
            ("E", 37, "E", Some("^")),
            ("A", 41, "E.A", Some("^^^")),
            ("\"a\"", 45, "\"a\"", Some("^^^")),
            ("B", 50, "E.B", Some("^^^")),
            ("\"b\"", 54, "\"b\"", Some("^^^")),
            ("E.A", 67, "E.A", Some("^^^")),
            ("E", 67, "typeof E", Some("^^^^^^^^")),
            ("A", 69, "E.A", Some("^^^")),
            ("E.A", 74, "E", Some("^")),
            ("E", 74, "typeof E", Some("^^^^^^^^")),
            ("A", 76, "E", Some("^")),
        ],
        &[],
    );
}

#[test]
fn nested_enum_loose() {
    assert_declarations(
        "enum E { A = 1 } function f() { enum E { A = \"a\", B = \"b\" } return E.A; } E.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("1", 13, "1", Some("^")),
            ("f", 26, "() => E", Some("^^^^^^^")),
            ("E", 37, "E", Some("^")),
            ("A", 41, "E.A", Some("^^^")),
            ("\"a\"", 45, "\"a\"", Some("^^^")),
            ("B", 50, "E.B", Some("^^^")),
            ("\"b\"", 54, "\"b\"", Some("^^^")),
            ("E.A", 67, "E.A", Some("^^^")),
            ("E", 67, "typeof E", Some("^^^^^^^^")),
            ("A", 69, "E.A", Some("^^^")),
            ("E.A", 74, "E", Some("^")),
            ("E", 74, "typeof E", Some("^^^^^^^^")),
            ("A", 76, "E", Some("^")),
        ],
        &[],
    );
}

#[test]
fn object_alias_strict() {
    assert_declarations(
        "enum E { A, B } const Alias = E; Alias; Alias.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("Alias", 22, "typeof E", Some("^^^^^^^^")),
            ("E", 30, "typeof E", Some("^^^^^^^^")),
            ("Alias", 33, "typeof E", Some("^^^^^^^^")),
            ("Alias.A", 40, "E.A", Some("^^^")),
            ("Alias", 40, "typeof E", Some("^^^^^^^^")),
            ("A", 46, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn object_alias_loose() {
    assert_declarations(
        "enum E { A, B } const Alias = E; Alias; Alias.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("Alias", 22, "typeof E", Some("^^^^^^^^")),
            ("E", 30, "typeof E", Some("^^^^^^^^")),
            ("Alias", 33, "typeof E", Some("^^^^^^^^")),
            ("Alias.A", 40, "E.A", Some("^^^")),
            ("Alias", 40, "typeof E", Some("^^^^^^^^")),
            ("A", 46, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_alias_strict() {
    assert_declarations(
        "namespace N { export enum E { A, B } } import Alias = N.E; Alias; Alias.A;",
        true,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("E", 26, "E", Some("^")),
            ("A", 30, "E.A", Some("^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("Alias", 46, "typeof Alias", Some("^^^^^^^^^^^^")),
            ("N", 54, "typeof N", Some("^^^^^^^^")),
            ("E", 56, "Alias", Some("^^^^^")),
            ("Alias", 59, "typeof Alias", Some("^^^^^^^^^^^^")),
            ("Alias.A", 66, "Alias.A", Some("^^^^^^^")),
            ("Alias", 66, "typeof Alias", Some("^^^^^^^^^^^^")),
            ("A", 72, "Alias.A", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_alias_loose() {
    assert_declarations(
        "namespace N { export enum E { A, B } } import Alias = N.E; Alias; Alias.A;",
        false,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("E", 26, "E", Some("^")),
            ("A", 30, "E.A", Some("^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("Alias", 46, "typeof Alias", Some("^^^^^^^^^^^^")),
            ("N", 54, "typeof N", Some("^^^^^^^^")),
            ("E", 56, "Alias", Some("^^^^^")),
            ("Alias", 59, "typeof Alias", Some("^^^^^^^^^^^^")),
            ("Alias.A", 66, "Alias.A", Some("^^^^^^^")),
            ("Alias", 66, "typeof Alias", Some("^^^^^^^^^^^^")),
            ("A", 72, "Alias.A", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn typeof_strict() {
    assert_declarations(
        "enum E { A, B } type Constructor = typeof E; let x: typeof E; type A = typeof E.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("Constructor", 21, "typeof E", Some("^^^^^^^^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("x", 49, "typeof E", Some("^^^^^^^^")),
            ("E", 59, "typeof E", Some("^^^^^^^^")),
            ("A", 67, "E.A", Some("^^^")),
            ("E.A", 78, "E.A", Some("^^^")),
            ("E", 78, "typeof E", Some("^^^^^^^^")),
            ("A", 80, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn typeof_loose() {
    assert_declarations(
        "enum E { A, B } type Constructor = typeof E; let x: typeof E; type A = typeof E.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("Constructor", 21, "typeof E", Some("^^^^^^^^")),
            ("E", 42, "typeof E", Some("^^^^^^^^")),
            ("x", 49, "typeof E", Some("^^^^^^^^")),
            ("E", 59, "typeof E", Some("^^^^^^^^")),
            ("A", 67, "E.A", Some("^^^")),
            ("E.A", 78, "E.A", Some("^^^")),
            ("E", 78, "typeof E", Some("^^^^^^^^")),
            ("A", 80, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn member_annotation_strict() {
    assert_declarations(
        "enum E { A, B } let x: E.A = E.A; let y: E.B;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("x", 20, "E.A", Some("^^^")),
            ("E", 23, "any", Some("^^^")),
            ("E.A", 29, "E.A", Some("^^^")),
            ("E", 29, "typeof E", Some("^^^^^^^^")),
            ("A", 31, "E.A", Some("^^^")),
            ("y", 38, "E.B", Some("^^^")),
            ("E", 41, "any", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn member_annotation_loose() {
    assert_declarations(
        "enum E { A, B } let x: E.A = E.A; let y: E.B;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("x", 20, "E.A", Some("^^^")),
            ("E", 23, "any", Some("^^^")),
            ("E.A", 29, "E.A", Some("^^^")),
            ("E", 29, "typeof E", Some("^^^^^^^^")),
            ("A", 31, "E.A", Some("^^^")),
            ("y", 38, "E.B", Some("^^^")),
            ("E", 41, "any", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn enum_union_strict() {
    assert_declarations(
        "enum E { A, B, C } let x: E.A | E.B; let y: E.A | E.B | E.C;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("C", 15, "E.C", Some("^^^")),
            ("x", 23, "E.A | E.B", Some("^^^^^^^^^")),
            ("E", 26, "any", Some("^^^")),
            ("E", 32, "any", Some("^^^")),
            ("y", 41, "E", Some("^")),
            ("E", 44, "any", Some("^^^")),
            ("E", 50, "any", Some("^^^")),
            ("E", 56, "any", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn enum_union_loose() {
    assert_declarations(
        "enum E { A, B, C } let x: E.A | E.B; let y: E.A | E.B | E.C;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("C", 15, "E.C", Some("^^^")),
            ("x", 23, "E.A | E.B", Some("^^^^^^^^^")),
            ("E", 26, "any", Some("^^^")),
            ("E", 32, "any", Some("^^^")),
            ("y", 41, "E", Some("^")),
            ("E", 44, "any", Some("^^^")),
            ("E", 50, "any", Some("^^^")),
            ("E", 56, "any", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_merge_strict() {
    assert_declarations(
        "enum E { A, B } namespace E { export const extra = 1; } E; E.A; E.extra;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("E", 26, "typeof E", Some("^^^^^^^^")),
            ("extra", 43, "1", Some("^")),
            ("1", 51, "1", Some("^")),
            ("E", 56, "typeof E", Some("^^^^^^^^")),
            ("E.A", 59, "E.A", Some("^^^")),
            ("E", 59, "typeof E", Some("^^^^^^^^")),
            ("A", 61, "E.A", Some("^^^")),
            ("E.extra", 64, "1", Some("^")),
            ("E", 64, "typeof E", Some("^^^^^^^^")),
            ("extra", 66, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn namespace_merge_loose() {
    assert_declarations(
        "enum E { A, B } namespace E { export const extra = 1; } E; E.A; E.extra;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("E", 26, "typeof E", Some("^^^^^^^^")),
            ("extra", 43, "1", Some("^")),
            ("1", 51, "1", Some("^")),
            ("E", 56, "typeof E", Some("^^^^^^^^")),
            ("E.A", 59, "E.A", Some("^^^")),
            ("E", 59, "typeof E", Some("^^^^^^^^")),
            ("A", 61, "E.A", Some("^^^")),
            ("E.extra", 64, "1", Some("^")),
            ("E", 64, "typeof E", Some("^^^^^^^^")),
            ("extra", 66, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn enum_as_property_strict() {
    assert_declarations(
        "enum E { A, B } const obj = { E }; obj.E; obj.E.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("obj", 22, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("{ E }", 28, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("E", 30, "typeof E", Some("^^^^^^^^")),
            ("obj.E", 35, "typeof E", Some("^^^^^^^^")),
            ("obj", 35, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("E", 39, "typeof E", Some("^^^^^^^^")),
            ("obj.E.A", 42, "E.A", Some("^^^")),
            ("obj.E", 42, "typeof E", Some("^^^^^^^^")),
            ("obj", 42, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("E", 46, "typeof E", Some("^^^^^^^^")),
            ("A", 48, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn enum_as_property_loose() {
    assert_declarations(
        "enum E { A, B } const obj = { E }; obj.E; obj.E.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("obj", 22, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("{ E }", 28, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("E", 30, "typeof E", Some("^^^^^^^^")),
            ("obj.E", 35, "typeof E", Some("^^^^^^^^")),
            ("obj", 35, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("E", 39, "typeof E", Some("^^^^^^^^")),
            ("obj.E.A", 42, "E.A", Some("^^^")),
            ("obj.E", 42, "typeof E", Some("^^^^^^^^")),
            ("obj", 42, "{ E: typeof E; }", Some("^^^^^^^^^^^^^^^^")),
            ("E", 46, "typeof E", Some("^^^^^^^^")),
            ("A", 48, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn enum_parameters_strict() {
    assert_declarations(
        "enum E { A, B } function f(x: E, y = E.A) { return x; } f(E.B);",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("f", 25, "(x: E, y?: E) => E", Some("^ ^^ ^^ ^^^^^^^^^^")),
            ("x", 27, "E", Some("^")),
            ("y", 33, "E", Some("^")),
            ("E.A", 37, "E.A", Some("^^^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("A", 39, "E.A", Some("^^^")),
            ("x", 51, "E", Some("^")),
            ("f(E.B)", 56, "E", Some("^")),
            ("f", 56, "(x: E, y?: E) => E", Some("^ ^^ ^^ ^^^^^^^^^^")),
            ("E.B", 58, "E.B", Some("^^^")),
            ("E", 58, "typeof E", Some("^^^^^^^^")),
            ("B", 60, "E.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn enum_parameters_loose() {
    assert_declarations(
        "enum E { A, B } function f(x: E, y = E.A) { return x; } f(E.B);",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("f", 25, "(x: E, y?: E) => E", Some("^ ^^ ^^ ^^^^^^^^^^")),
            ("x", 27, "E", Some("^")),
            ("y", 33, "E", Some("^")),
            ("E.A", 37, "E.A", Some("^^^")),
            ("E", 37, "typeof E", Some("^^^^^^^^")),
            ("A", 39, "E.A", Some("^^^")),
            ("x", 51, "E", Some("^")),
            ("f(E.B)", 56, "E", Some("^")),
            ("f", 56, "(x: E, y?: E) => E", Some("^ ^^ ^^ ^^^^^^^^^^")),
            ("E.B", 58, "E.B", Some("^^^")),
            ("E", 58, "typeof E", Some("^^^^^^^^")),
            ("B", 60, "E.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn enum_returns_strict() {
    assert_declarations(
        "enum E { A, B } function f() { return E; } function g() { return E.A; } f(); g();",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("f", 25, "() => typeof E", Some("^^^^^^^^^^^^^^")),
            ("E", 38, "typeof E", Some("^^^^^^^^")),
            ("g", 52, "() => E", Some("^^^^^^^")),
            ("E.A", 65, "E.A", Some("^^^")),
            ("E", 65, "typeof E", Some("^^^^^^^^")),
            ("A", 67, "E.A", Some("^^^")),
            ("f()", 72, "typeof E", Some("^^^^^^^^")),
            ("f", 72, "() => typeof E", Some("^^^^^^^^^^^^^^")),
            ("g()", 77, "E", Some("^")),
            ("g", 77, "() => E", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn enum_returns_loose() {
    assert_declarations(
        "enum E { A, B } function f() { return E; } function g() { return E.A; } f(); g();",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("f", 25, "() => typeof E", Some("^^^^^^^^^^^^^^")),
            ("E", 38, "typeof E", Some("^^^^^^^^")),
            ("g", 52, "() => E", Some("^^^^^^^")),
            ("E.A", 65, "E.A", Some("^^^")),
            ("E", 65, "typeof E", Some("^^^^^^^^")),
            ("A", 67, "E.A", Some("^^^")),
            ("f()", 72, "typeof E", Some("^^^^^^^^")),
            ("f", 72, "() => typeof E", Some("^^^^^^^^^^^^^^")),
            ("g()", 77, "E", Some("^")),
            ("g", 77, "() => E", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn multiple_aliases_strict() {
    assert_declarations(
        "namespace N { export enum E { A, B } } import Z = N.E; import A = N.E; N.E; Z.A; A.B;",
        true,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("E", 26, "E", Some("^")),
            ("A", 30, "E.A", Some("^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("Z", 46, "typeof Z", Some("^^^^^^^^")),
            ("N", 50, "typeof N", Some("^^^^^^^^")),
            ("E", 52, "Z", Some("^")),
            ("A", 62, "typeof Z", Some("^^^^^^^^")),
            ("N", 66, "typeof N", Some("^^^^^^^^")),
            ("E", 68, "Z", Some("^")),
            ("N.E", 71, "typeof Z", Some("^^^^^^^^")),
            ("N", 71, "typeof N", Some("^^^^^^^^")),
            ("E", 73, "typeof Z", Some("^^^^^^^^")),
            ("Z.A", 76, "Z.A", Some("^^^")),
            ("Z", 76, "typeof Z", Some("^^^^^^^^")),
            ("A", 78, "Z.A", Some("^^^")),
            ("A.B", 81, "Z.B", Some("^^^")),
            ("A", 81, "typeof Z", Some("^^^^^^^^")),
            ("B", 83, "Z.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn multiple_aliases_loose() {
    assert_declarations(
        "namespace N { export enum E { A, B } } import Z = N.E; import A = N.E; N.E; Z.A; A.B;",
        false,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("E", 26, "E", Some("^")),
            ("A", 30, "E.A", Some("^^^")),
            ("B", 33, "E.B", Some("^^^")),
            ("Z", 46, "typeof Z", Some("^^^^^^^^")),
            ("N", 50, "typeof N", Some("^^^^^^^^")),
            ("E", 52, "Z", Some("^")),
            ("A", 62, "typeof Z", Some("^^^^^^^^")),
            ("N", 66, "typeof N", Some("^^^^^^^^")),
            ("E", 68, "Z", Some("^")),
            ("N.E", 71, "typeof Z", Some("^^^^^^^^")),
            ("N", 71, "typeof N", Some("^^^^^^^^")),
            ("E", 73, "typeof Z", Some("^^^^^^^^")),
            ("Z.A", 76, "Z.A", Some("^^^")),
            ("Z", 76, "typeof Z", Some("^^^^^^^^")),
            ("A", 78, "Z.A", Some("^^^")),
            ("A.B", 81, "Z.B", Some("^^^")),
            ("A", 81, "typeof Z", Some("^^^^^^^^")),
            ("B", 83, "Z.B", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn local_alias_strict() {
    assert_declarations(
        "enum E { A, B } import Alias = E; E; Alias; Alias.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("Alias", 23, "typeof E", Some("^^^^^^^^")),
            ("E", 31, "E", Some("^")),
            ("E", 34, "typeof E", Some("^^^^^^^^")),
            ("Alias", 37, "typeof E", Some("^^^^^^^^")),
            ("Alias.A", 44, "E.A", Some("^^^")),
            ("Alias", 44, "typeof E", Some("^^^^^^^^")),
            ("A", 50, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn local_alias_loose() {
    assert_declarations(
        "enum E { A, B } import Alias = E; E; Alias; Alias.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("Alias", 23, "typeof E", Some("^^^^^^^^")),
            ("E", 31, "E", Some("^")),
            ("E", 34, "typeof E", Some("^^^^^^^^")),
            ("Alias", 37, "typeof E", Some("^^^^^^^^")),
            ("Alias.A", 44, "E.A", Some("^^^")),
            ("Alias", 44, "typeof E", Some("^^^^^^^^")),
            ("A", 50, "E.A", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn alias_shadow_strict() {
    assert_declarations("namespace N { export enum E { A, B } } import Alias = N.E; function f(Alias: number) { return N.E.A; } Alias.A;", true, "es2020", false, &[
        ("N", 10, "typeof N", Some("^^^^^^^^")),
        ("E", 26, "E", Some("^")),
        ("A", 30, "E.A", Some("^^^")),
        ("B", 33, "E.B", Some("^^^")),
        ("Alias", 46, "typeof Alias", Some("^^^^^^^^^^^^")),
        ("N", 54, "typeof N", Some("^^^^^^^^")),
        ("E", 56, "Alias", Some("^^^^^")),
        ("f", 68, "(Alias: number) => Alias", Some("^     ^^      ^^^^^^^^^^")),
        ("Alias", 70, "number", Some("^^^^^^")),
        ("N.E.A", 94, "Alias.A", Some("^^^^^^^")),
        ("N.E", 94, "typeof globalThis.Alias", Some("^^^^^^^^^^^^^^^^^^^^^^^")),
        ("N", 94, "typeof N", Some("^^^^^^^^")),
        ("E", 96, "typeof globalThis.Alias", Some("^^^^^^^^^^^^^^^^^^^^^^^")),
        ("A", 98, "Alias.A", Some("^^^^^^^")),
        ("Alias.A", 103, "Alias.A", Some("^^^^^^^")),
        ("Alias", 103, "typeof Alias", Some("^^^^^^^^^^^^")),
        ("A", 109, "Alias.A", Some("^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn alias_shadow_loose() {
    assert_declarations("namespace N { export enum E { A, B } } import Alias = N.E; function f(Alias: number) { return N.E.A; } Alias.A;", false, "es2020", false, &[
        ("N", 10, "typeof N", Some("^^^^^^^^")),
        ("E", 26, "E", Some("^")),
        ("A", 30, "E.A", Some("^^^")),
        ("B", 33, "E.B", Some("^^^")),
        ("Alias", 46, "typeof Alias", Some("^^^^^^^^^^^^")),
        ("N", 54, "typeof N", Some("^^^^^^^^")),
        ("E", 56, "Alias", Some("^^^^^")),
        ("f", 68, "(Alias: number) => Alias", Some("^     ^^      ^^^^^^^^^^")),
        ("Alias", 70, "number", Some("^^^^^^")),
        ("N.E.A", 94, "Alias.A", Some("^^^^^^^")),
        ("N.E", 94, "typeof globalThis.Alias", Some("^^^^^^^^^^^^^^^^^^^^^^^")),
        ("N", 94, "typeof N", Some("^^^^^^^^")),
        ("E", 96, "typeof globalThis.Alias", Some("^^^^^^^^^^^^^^^^^^^^^^^")),
        ("A", 98, "Alias.A", Some("^^^^^^^")),
        ("Alias.A", 103, "Alias.A", Some("^^^^^^^")),
        ("Alias", 103, "typeof Alias", Some("^^^^^^^^^^^^")),
        ("A", 109, "Alias.A", Some("^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn enum_value_assignment_strict() {
    assert_declarations(
        "enum E { A, B } let n: number = E; let e: E = E; let o: typeof E = E.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("n", 20, "number", Some("^^^^^^")),
            ("E", 32, "typeof E", Some("^^^^^^^^")),
            ("e", 39, "E", Some("^")),
            ("E", 46, "typeof E", Some("^^^^^^^^")),
            ("o", 53, "typeof E", Some("^^^^^^^^")),
            ("E", 63, "typeof E", Some("^^^^^^^^")),
            ("E.A", 67, "E.A", Some("^^^")),
            ("E", 67, "typeof E", Some("^^^^^^^^")),
            ("A", 69, "E.A", Some("^^^")),
        ],
        &[(2322, 20, 1), (2322, 39, 1), (2322, 53, 1)],
    );
}

#[test]
fn enum_value_assignment_loose() {
    assert_declarations(
        "enum E { A, B } let n: number = E; let e: E = E; let o: typeof E = E.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("n", 20, "number", Some("^^^^^^")),
            ("E", 32, "typeof E", Some("^^^^^^^^")),
            ("e", 39, "E", Some("^")),
            ("E", 46, "typeof E", Some("^^^^^^^^")),
            ("o", 53, "typeof E", Some("^^^^^^^^")),
            ("E", 63, "typeof E", Some("^^^^^^^^")),
            ("E.A", 67, "E.A", Some("^^^")),
            ("E", 67, "typeof E", Some("^^^^^^^^")),
            ("A", 69, "E.A", Some("^^^")),
        ],
        &[(2322, 20, 1), (2322, 39, 1), (2322, 53, 1)],
    );
}

#[test]
fn enum_object_keys_strict() {
    assert_declarations(
        "enum E { A, B } E.A = 2; E.missing; E[0]; const copy = { ...E }; copy.A;",
        true,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("E.A = 2", 16, "2", Some("^")),
            ("E.A", 16, "any", Some("^^^")),
            ("E", 16, "typeof E", Some("^^^^^^^^")),
            ("A", 18, "any", Some("^^^")),
            ("2", 22, "2", Some("^")),
            ("E.missing", 25, "any", Some("^^^")),
            ("E", 25, "typeof E", Some("^^^^^^^^")),
            ("missing", 27, "any", Some("^^^")),
            ("E[0]", 36, "string", Some("^^^^^^")),
            ("E", 36, "typeof E", Some("^^^^^^^^")),
            ("0", 38, "0", Some("^")),
            (
                "copy",
                48,
                "{ [x: number]: string; A: E.A; B: E.B; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "{ ...E }",
                55,
                "{ [x: number]: string; A: E.A; B: E.B; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("E", 60, "typeof E", Some("^^^^^^^^")),
            ("copy.A", 65, "E.A", Some("^^^")),
            (
                "copy",
                65,
                "{ [x: number]: string; A: E.A; B: E.B; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("A", 70, "E.A", Some("^^^")),
        ],
        &[(2540, 18, 1), (2339, 27, 7)],
    );
}

#[test]
fn enum_object_keys_loose() {
    assert_declarations(
        "enum E { A, B } E.A = 2; E.missing; E[0]; const copy = { ...E }; copy.A;",
        false,
        "es2020",
        false,
        &[
            ("E", 5, "E", Some("^")),
            ("A", 9, "E.A", Some("^^^")),
            ("B", 12, "E.B", Some("^^^")),
            ("E.A = 2", 16, "2", Some("^")),
            ("E.A", 16, "any", Some("^^^")),
            ("E", 16, "typeof E", Some("^^^^^^^^")),
            ("A", 18, "any", Some("^^^")),
            ("2", 22, "2", Some("^")),
            ("E.missing", 25, "any", Some("^^^")),
            ("E", 25, "typeof E", Some("^^^^^^^^")),
            ("missing", 27, "any", Some("^^^")),
            ("E[0]", 36, "string", Some("^^^^^^")),
            ("E", 36, "typeof E", Some("^^^^^^^^")),
            ("0", 38, "0", Some("^")),
            (
                "copy",
                48,
                "{ [x: number]: string; A: E.A; B: E.B; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "{ ...E }",
                55,
                "{ [x: number]: string; A: E.A; B: E.B; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("E", 60, "typeof E", Some("^^^^^^^^")),
            ("copy.A", 65, "E.A", Some("^^^")),
            (
                "copy",
                65,
                "{ [x: number]: string; A: E.A; B: E.B; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("A", 70, "E.A", Some("^^^")),
        ],
        &[(2540, 18, 1), (2339, 27, 7)],
    );
}

#[test]
fn merged_reverse_strict() {
    assert_declarations(
        "namespace E { export const extra = 1; } enum E { A, B } E; E.A; E.extra;",
        true,
        "es2020",
        false,
        &[
            ("E", 10, "typeof E", Some("^^^^^^^^")),
            ("extra", 27, "1", Some("^")),
            ("1", 35, "1", Some("^")),
            ("E", 45, "E", Some("^")),
            ("A", 49, "E.A", Some("^^^")),
            ("B", 52, "E.B", Some("^^^")),
            ("E", 56, "typeof E", Some("^^^^^^^^")),
            ("E.A", 59, "E.A", Some("^^^")),
            ("E", 59, "typeof E", Some("^^^^^^^^")),
            ("A", 61, "E.A", Some("^^^")),
            ("E.extra", 64, "1", Some("^")),
            ("E", 64, "typeof E", Some("^^^^^^^^")),
            ("extra", 66, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn merged_reverse_loose() {
    assert_declarations(
        "namespace E { export const extra = 1; } enum E { A, B } E; E.A; E.extra;",
        false,
        "es2020",
        false,
        &[
            ("E", 10, "typeof E", Some("^^^^^^^^")),
            ("extra", 27, "1", Some("^")),
            ("1", 35, "1", Some("^")),
            ("E", 45, "E", Some("^")),
            ("A", 49, "E.A", Some("^^^")),
            ("B", 52, "E.B", Some("^^^")),
            ("E", 56, "typeof E", Some("^^^^^^^^")),
            ("E.A", 59, "E.A", Some("^^^")),
            ("E", 59, "typeof E", Some("^^^^^^^^")),
            ("A", 61, "E.A", Some("^^^")),
            ("E.extra", 64, "1", Some("^")),
            ("E", 64, "typeof E", Some("^^^^^^^^")),
            ("extra", 66, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn scoped_distinct_values_strict() {
    assert_declarations("enum E { A = 1 } function f() { enum E { A = \"a\", B = \"b\" } const x = E.B; return x; } E.A; f();", true, "es2020", false, &[
        ("E", 5, "E", Some("^")),
        ("A", 9, "E.A", Some("^^^")),
        ("1", 13, "1", Some("^")),
        ("f", 26, "() => E", Some("^^^^^^^")),
        ("E", 37, "E", Some("^")),
        ("A", 41, "E.A", Some("^^^")),
        ("\"a\"", 45, "\"a\"", Some("^^^")),
        ("B", 50, "E.B", Some("^^^")),
        ("\"b\"", 54, "\"b\"", Some("^^^")),
        ("x", 66, "E.B", Some("^^^")),
        ("E.B", 70, "E.B", Some("^^^")),
        ("E", 70, "typeof E", Some("^^^^^^^^")),
        ("B", 72, "E.B", Some("^^^")),
        ("x", 82, "E.B", Some("^^^")),
        ("E.A", 87, "E", Some("^")),
        ("E", 87, "typeof E", Some("^^^^^^^^")),
        ("A", 89, "E", Some("^")),
        ("f()", 92, "E", Some("^")),
        ("f", 92, "() => E", Some("^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn scoped_distinct_values_loose() {
    assert_declarations("enum E { A = 1 } function f() { enum E { A = \"a\", B = \"b\" } const x = E.B; return x; } E.A; f();", false, "es2020", false, &[
        ("E", 5, "E", Some("^")),
        ("A", 9, "E.A", Some("^^^")),
        ("1", 13, "1", Some("^")),
        ("f", 26, "() => E", Some("^^^^^^^")),
        ("E", 37, "E", Some("^")),
        ("A", 41, "E.A", Some("^^^")),
        ("\"a\"", 45, "\"a\"", Some("^^^")),
        ("B", 50, "E.B", Some("^^^")),
        ("\"b\"", 54, "\"b\"", Some("^^^")),
        ("x", 66, "E.B", Some("^^^")),
        ("E.B", 70, "E.B", Some("^^^")),
        ("E", 70, "typeof E", Some("^^^^^^^^")),
        ("B", 72, "E.B", Some("^^^")),
        ("x", 82, "E.B", Some("^^^")),
        ("E.A", 87, "E", Some("^")),
        ("E", 87, "typeof E", Some("^^^^^^^^")),
        ("A", 89, "E", Some("^")),
        ("f()", 92, "E", Some("^")),
        ("f", 92, "() => E", Some("^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn merged_class_strict() {
    assert_declarations("enum E { A, B } namespace E { export class Point { constructor(public x: number) {} } } const p = new E.Point(1); E.Point;", true, "es2020", false, &[
        ("E", 5, "E", Some("^")),
        ("A", 9, "E.A", Some("^^^")),
        ("B", 12, "E.B", Some("^^^")),
        ("E", 26, "typeof E", Some("^^^^^^^^")),
        ("Point", 43, "Point", Some("^^^^^")),
        ("x", 70, "number", Some("^^^^^^")),
        ("p", 94, "E.Point", Some("^^^^^^^")),
        ("new E.Point(1)", 98, "E.Point", Some("^^^^^^^")),
        ("E.Point", 102, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 102, "typeof E", Some("^^^^^^^^")),
        ("Point", 104, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("1", 110, "1", Some("^")),
        ("E.Point", 114, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 114, "typeof E", Some("^^^^^^^^")),
        ("Point", 116, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn merged_class_loose() {
    assert_declarations("enum E { A, B } namespace E { export class Point { constructor(public x: number) {} } } const p = new E.Point(1); E.Point;", false, "es2020", false, &[
        ("E", 5, "E", Some("^")),
        ("A", 9, "E.A", Some("^^^")),
        ("B", 12, "E.B", Some("^^^")),
        ("E", 26, "typeof E", Some("^^^^^^^^")),
        ("Point", 43, "Point", Some("^^^^^")),
        ("x", 70, "number", Some("^^^^^^")),
        ("p", 94, "E.Point", Some("^^^^^^^")),
        ("new E.Point(1)", 98, "E.Point", Some("^^^^^^^")),
        ("E.Point", 102, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 102, "typeof E", Some("^^^^^^^^")),
        ("Point", 104, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("1", 110, "1", Some("^")),
        ("E.Point", 114, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 114, "typeof E", Some("^^^^^^^^")),
        ("Point", 116, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn merged_class_reverse_strict() {
    assert_declarations("namespace E { export class Point { constructor(public x: number) {} } } enum E { A, B } const p = new E.Point(1); E.Point;", true, "es2020", false, &[
        ("E", 10, "typeof E", Some("^^^^^^^^")),
        ("Point", 27, "Point", Some("^^^^^")),
        ("x", 54, "number", Some("^^^^^^")),
        ("E", 77, "E", Some("^")),
        ("A", 81, "E.A", Some("^^^")),
        ("B", 84, "E.B", Some("^^^")),
        ("p", 94, "E.Point", Some("^^^^^^^")),
        ("new E.Point(1)", 98, "E.Point", Some("^^^^^^^")),
        ("E.Point", 102, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 102, "typeof E", Some("^^^^^^^^")),
        ("Point", 104, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("1", 110, "1", Some("^")),
        ("E.Point", 114, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 114, "typeof E", Some("^^^^^^^^")),
        ("Point", 116, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn merged_class_reverse_loose() {
    assert_declarations("namespace E { export class Point { constructor(public x: number) {} } } enum E { A, B } const p = new E.Point(1); E.Point;", false, "es2020", false, &[
        ("E", 10, "typeof E", Some("^^^^^^^^")),
        ("Point", 27, "Point", Some("^^^^^")),
        ("x", 54, "number", Some("^^^^^^")),
        ("E", 77, "E", Some("^")),
        ("A", 81, "E.A", Some("^^^")),
        ("B", 84, "E.B", Some("^^^")),
        ("p", 94, "E.Point", Some("^^^^^^^")),
        ("new E.Point(1)", 98, "E.Point", Some("^^^^^^^")),
        ("E.Point", 102, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 102, "typeof E", Some("^^^^^^^^")),
        ("Point", 104, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("1", 110, "1", Some("^")),
        ("E.Point", 114, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
        ("E", 114, "typeof E", Some("^^^^^^^^")),
        ("Point", 116, "typeof E.Point", Some("^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn javascript_enum_expando_declarations_preserve_readonly_members() {
    // Original TypeScript 6.0.3 semantic diagnostics. Qualified declarations
    // accept inferred initializers; writes to enum members remain readonly.
    for (source, expected) in [
        ("N.E = {}; N.E.A = 2;", vec![(2540, 14, 1)]),
        ("N.E = {A: 1};", vec![]),
        ("N.E = 1;", vec![]),
        ("N.E = \"x\";", vec![]),
        ("N.E = {A: 0, B: 1};", vec![]),
    ] {
        let declaration =
            tsc_rs_parser::parse("decl.d.ts", "declare namespace N { enum E { A, B } }");
        let file = tsc_rs_parser::parse("case.js", source);
        let mut checker = TypeChecker::new();
        checker.inject_external_types(&[&declaration, &file]);
        let output = checker.check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                allow_js: Some(true),
                check_js: Some(true),
                target: Some(tsc_rs_ast::ScriptTarget::ES2020),
                ..Default::default()
            },
        );
        let actual: Vec<_> = output
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.span.unwrap();
                (d.code, span.start, span.end - span.start)
            })
            .collect();
        assert_eq!(actual, expected, "{source}");
    }
}
