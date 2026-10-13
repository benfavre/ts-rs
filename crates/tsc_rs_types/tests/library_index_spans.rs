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
fn match_array_literal_strict() {
    assert_declarations(
        "declare const values: RegExpMatchArray; const first = values[0]; const next = values[1];",
        true,
        "es2020",
        false,
        &[
            ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("first", 46, "string", Some("^^^^^^")),
            ("values[0]", 54, "string", Some("^^^^^^")),
            ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("0", 61, "0", Some("^")),
            ("next", 71, "string", Some("^^^^^^")),
            ("values[1]", 78, "string", Some("^^^^^^")),
            ("values", 78, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("1", 85, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn match_array_literal_strict_unchecked() {
    assert_declarations(
        "declare const values: RegExpMatchArray; const first = values[0]; const next = values[1];",
        true,
        "es2020",
        true,
        &[
            ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("first", 46, "string", Some("^^^^^^")),
            ("values[0]", 54, "string", Some("^^^^^^")),
            ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("0", 61, "0", Some("^")),
            ("next", 71, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
            (
                "values[1]",
                78,
                "string | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            ("values", 78, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("1", 85, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn match_array_literal_loose() {
    assert_declarations(
        "declare const values: RegExpMatchArray; const first = values[0]; const next = values[1];",
        false,
        "es2020",
        false,
        &[
            ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("first", 46, "string", Some("^^^^^^")),
            ("values[0]", 54, "string", Some("^^^^^^")),
            ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("0", 61, "0", Some("^")),
            ("next", 71, "string", Some("^^^^^^")),
            ("values[1]", 78, "string", Some("^^^^^^")),
            ("values", 78, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("1", 85, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn match_array_literal_loose_unchecked() {
    assert_declarations(
        "declare const values: RegExpMatchArray; const first = values[0]; const next = values[1];",
        false,
        "es2020",
        true,
        &[
            ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("first", 46, "string", Some("^^^^^^")),
            ("values[0]", 54, "string", Some("^^^^^^")),
            ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("0", 61, "0", Some("^")),
            ("next", 71, "string", Some("^^^^^^")),
            ("values[1]", 78, "string", Some("^^^^^^")),
            ("values", 78, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
            ("1", 85, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn match_array_dynamic_strict() {
    assert_declarations("declare const values: RegExpMatchArray; declare const index: number; const result = values[index];", true, "es2020", false, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 54, "number", Some("^^^^^^")),
        ("result", 75, "string", Some("^^^^^^")),
        ("values[index]", 84, "string", Some("^^^^^^")),
        ("values", 84, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 91, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_dynamic_strict_unchecked() {
    assert_declarations("declare const values: RegExpMatchArray; declare const index: number; const result = values[index];", true, "es2020", true, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 54, "number", Some("^^^^^^")),
        ("result", 75, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values[index]", 84, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 84, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 91, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_dynamic_loose() {
    assert_declarations("declare const values: RegExpMatchArray; declare const index: number; const result = values[index];", false, "es2020", false, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 54, "number", Some("^^^^^^")),
        ("result", 75, "string", Some("^^^^^^")),
        ("values[index]", 84, "string", Some("^^^^^^")),
        ("values", 84, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 91, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_dynamic_loose_unchecked() {
    assert_declarations("declare const values: RegExpMatchArray; declare const index: number; const result = values[index];", false, "es2020", true, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 54, "number", Some("^^^^^^")),
        ("result", 75, "string", Some("^^^^^^")),
        ("values[index]", 84, "string", Some("^^^^^^")),
        ("values", 84, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 91, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_string_key_strict() {
    assert_declarations("declare const values: RegExpMatchArray; const first = values[\"0\"]; const next = values[\"1\"];", true, "es2020", false, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("first", 46, "string", Some("^^^^^^")),
        ("values[\"0\"]", 54, "string", Some("^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"0\"", 61, "\"0\"", Some("^^^")),
        ("next", 73, "string", Some("^^^^^^")),
        ("values[\"1\"]", 80, "string", Some("^^^^^^")),
        ("values", 80, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"1\"", 87, "\"1\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_string_key_strict_unchecked() {
    assert_declarations("declare const values: RegExpMatchArray; const first = values[\"0\"]; const next = values[\"1\"];", true, "es2020", true, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("first", 46, "string", Some("^^^^^^")),
        ("values[\"0\"]", 54, "string", Some("^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"0\"", 61, "\"0\"", Some("^^^")),
        ("next", 73, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values[\"1\"]", 80, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 80, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"1\"", 87, "\"1\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_string_key_loose() {
    assert_declarations("declare const values: RegExpMatchArray; const first = values[\"0\"]; const next = values[\"1\"];", false, "es2020", false, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("first", 46, "string", Some("^^^^^^")),
        ("values[\"0\"]", 54, "string", Some("^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"0\"", 61, "\"0\"", Some("^^^")),
        ("next", 73, "string", Some("^^^^^^")),
        ("values[\"1\"]", 80, "string", Some("^^^^^^")),
        ("values", 80, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"1\"", 87, "\"1\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_string_key_loose_unchecked() {
    assert_declarations("declare const values: RegExpMatchArray; const first = values[\"0\"]; const next = values[\"1\"];", false, "es2020", true, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("first", 46, "string", Some("^^^^^^")),
        ("values[\"0\"]", 54, "string", Some("^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"0\"", 61, "\"0\"", Some("^^^")),
        ("next", 73, "string", Some("^^^^^^")),
        ("values[\"1\"]", 80, "string", Some("^^^^^^")),
        ("values", 80, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("\"1\"", 87, "\"1\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_properties_strict() {
    assert_declarations("declare const values: RegExpMatchArray; const index = values.index; const input = values.input; const length = values.length;", true, "es2020", false, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 46, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values.index", 54, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 61, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("input", 74, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values.input", 82, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 82, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("input", 89, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("length", 102, "number", Some("^^^^^^")),
        ("values.length", 111, "number", Some("^^^^^^")),
        ("values", 111, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("length", 118, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_properties_strict_unchecked() {
    assert_declarations("declare const values: RegExpMatchArray; const index = values.index; const input = values.input; const length = values.length;", true, "es2020", true, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 46, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values.index", 54, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 61, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("input", 74, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values.input", 82, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 82, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("input", 89, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("length", 102, "number", Some("^^^^^^")),
        ("values.length", 111, "number", Some("^^^^^^")),
        ("values", 111, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("length", 118, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_properties_loose() {
    assert_declarations("declare const values: RegExpMatchArray; const index = values.index; const input = values.input; const length = values.length;", false, "es2020", false, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 46, "number", Some("^^^^^^")),
        ("values.index", 54, "number", Some("^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 61, "number", Some("^^^^^^")),
        ("input", 74, "string", Some("^^^^^^")),
        ("values.input", 82, "string", Some("^^^^^^")),
        ("values", 82, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("input", 89, "string", Some("^^^^^^")),
        ("length", 102, "number", Some("^^^^^^")),
        ("values.length", 111, "number", Some("^^^^^^")),
        ("values", 111, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("length", 118, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn match_array_properties_loose_unchecked() {
    assert_declarations("declare const values: RegExpMatchArray; const index = values.index; const input = values.input; const length = values.length;", false, "es2020", true, &[
        ("values", 14, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 46, "number", Some("^^^^^^")),
        ("values.index", 54, "number", Some("^^^^^^")),
        ("values", 54, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("index", 61, "number", Some("^^^^^^")),
        ("input", 74, "string", Some("^^^^^^")),
        ("values.input", 82, "string", Some("^^^^^^")),
        ("values", 82, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("input", 89, "string", Some("^^^^^^")),
        ("length", 102, "number", Some("^^^^^^")),
        ("values.length", 111, "number", Some("^^^^^^")),
        ("values", 111, "RegExpMatchArray", Some("^^^^^^^^^^^^^^^^")),
        ("length", 118, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn exec_array_properties_strict() {
    assert_declarations("declare const values: RegExpExecArray; const first = values[0]; const index = values.index; const input = values.input;", true, "es2020", false, &[
        ("values", 14, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("first", 45, "string", Some("^^^^^^")),
        ("values[0]", 53, "string", Some("^^^^^^")),
        ("values", 53, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("0", 60, "0", Some("^")),
        ("index", 70, "number", Some("^^^^^^")),
        ("values.index", 78, "number", Some("^^^^^^")),
        ("values", 78, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("index", 85, "number", Some("^^^^^^")),
        ("input", 98, "string", Some("^^^^^^")),
        ("values.input", 106, "string", Some("^^^^^^")),
        ("values", 106, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("input", 113, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn exec_array_properties_strict_unchecked() {
    assert_declarations("declare const values: RegExpExecArray; const first = values[0]; const index = values.index; const input = values.input;", true, "es2020", true, &[
        ("values", 14, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("first", 45, "string", Some("^^^^^^")),
        ("values[0]", 53, "string", Some("^^^^^^")),
        ("values", 53, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("0", 60, "0", Some("^")),
        ("index", 70, "number", Some("^^^^^^")),
        ("values.index", 78, "number", Some("^^^^^^")),
        ("values", 78, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("index", 85, "number", Some("^^^^^^")),
        ("input", 98, "string", Some("^^^^^^")),
        ("values.input", 106, "string", Some("^^^^^^")),
        ("values", 106, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("input", 113, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn exec_array_properties_loose() {
    assert_declarations("declare const values: RegExpExecArray; const first = values[0]; const index = values.index; const input = values.input;", false, "es2020", false, &[
        ("values", 14, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("first", 45, "string", Some("^^^^^^")),
        ("values[0]", 53, "string", Some("^^^^^^")),
        ("values", 53, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("0", 60, "0", Some("^")),
        ("index", 70, "number", Some("^^^^^^")),
        ("values.index", 78, "number", Some("^^^^^^")),
        ("values", 78, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("index", 85, "number", Some("^^^^^^")),
        ("input", 98, "string", Some("^^^^^^")),
        ("values.input", 106, "string", Some("^^^^^^")),
        ("values", 106, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("input", 113, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn exec_array_properties_loose_unchecked() {
    assert_declarations("declare const values: RegExpExecArray; const first = values[0]; const index = values.index; const input = values.input;", false, "es2020", true, &[
        ("values", 14, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("first", 45, "string", Some("^^^^^^")),
        ("values[0]", 53, "string", Some("^^^^^^")),
        ("values", 53, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("0", 60, "0", Some("^")),
        ("index", 70, "number", Some("^^^^^^")),
        ("values.index", 78, "number", Some("^^^^^^")),
        ("values", 78, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("index", 85, "number", Some("^^^^^^")),
        ("input", 98, "string", Some("^^^^^^")),
        ("values.input", 106, "string", Some("^^^^^^")),
        ("values", 106, "RegExpExecArray", Some("^^^^^^^^^^^^^^^")),
        ("input", 113, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_like_index_strict() {
    assert_declarations("declare const values: ArrayLike<string>; const result = values[0]; const length = values.length;", true, "es2020", false, &[
        ("values", 14, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("result", 47, "string", Some("^^^^^^")),
        ("values[0]", 56, "string", Some("^^^^^^")),
        ("values", 56, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("0", 63, "0", Some("^")),
        ("length", 73, "number", Some("^^^^^^")),
        ("values.length", 82, "number", Some("^^^^^^")),
        ("values", 82, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("length", 89, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_like_index_strict_unchecked() {
    assert_declarations("declare const values: ArrayLike<string>; const result = values[0]; const length = values.length;", true, "es2020", true, &[
        ("values", 14, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("result", 47, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values[0]", 56, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 56, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("0", 63, "0", Some("^")),
        ("length", 73, "number", Some("^^^^^^")),
        ("values.length", 82, "number", Some("^^^^^^")),
        ("values", 82, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("length", 89, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_like_index_loose() {
    assert_declarations("declare const values: ArrayLike<string>; const result = values[0]; const length = values.length;", false, "es2020", false, &[
        ("values", 14, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("result", 47, "string", Some("^^^^^^")),
        ("values[0]", 56, "string", Some("^^^^^^")),
        ("values", 56, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("0", 63, "0", Some("^")),
        ("length", 73, "number", Some("^^^^^^")),
        ("values.length", 82, "number", Some("^^^^^^")),
        ("values", 82, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("length", 89, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_like_index_loose_unchecked() {
    assert_declarations("declare const values: ArrayLike<string>; const result = values[0]; const length = values.length;", false, "es2020", true, &[
        ("values", 14, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("result", 47, "string", Some("^^^^^^")),
        ("values[0]", 56, "string", Some("^^^^^^")),
        ("values", 56, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("0", 63, "0", Some("^")),
        ("length", 73, "number", Some("^^^^^^")),
        ("values.length", 82, "number", Some("^^^^^^")),
        ("values", 82, "ArrayLike<string>", Some("^^^^^^^^^^^^^^^^^")),
        ("length", 89, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn typed_array_index_strict() {
    assert_declarations(
        "declare const values: Uint8Array; const first = values[0]; const next = values[\"1\"];",
        true,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 40, "number", Some("^^^^^^")),
            ("values[0]", 48, "number", Some("^^^^^^")),
            (
                "values",
                48,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 55, "0", Some("^")),
            ("next", 65, "number", Some("^^^^^^")),
            ("values[\"1\"]", 72, "number", Some("^^^^^^")),
            (
                "values",
                72,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("\"1\"", 79, "\"1\"", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn typed_array_index_strict_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array; const first = values[0]; const next = values[\"1\"];",
        true,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "first",
                40,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "values[0]",
                48,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "values",
                48,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 55, "0", Some("^")),
            ("next", 65, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
            (
                "values[\"1\"]",
                72,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "values",
                72,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("\"1\"", 79, "\"1\"", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn typed_array_index_loose() {
    assert_declarations(
        "declare const values: Uint8Array; const first = values[0]; const next = values[\"1\"];",
        false,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 40, "number", Some("^^^^^^")),
            ("values[0]", 48, "number", Some("^^^^^^")),
            (
                "values",
                48,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 55, "0", Some("^")),
            ("next", 65, "number", Some("^^^^^^")),
            ("values[\"1\"]", 72, "number", Some("^^^^^^")),
            (
                "values",
                72,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("\"1\"", 79, "\"1\"", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn typed_array_index_loose_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array; const first = values[0]; const next = values[\"1\"];",
        false,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 40, "number", Some("^^^^^^")),
            ("values[0]", 48, "number", Some("^^^^^^")),
            (
                "values",
                48,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 55, "0", Some("^")),
            ("next", 65, "number", Some("^^^^^^")),
            ("values[\"1\"]", 72, "number", Some("^^^^^^")),
            (
                "values",
                72,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("\"1\"", 79, "\"1\"", Some("^^^")),
        ],
        &[],
    );
}

#[test]
fn bigint_typed_array_index_strict() {
    assert_declarations(
        "declare const values: BigInt64Array; const first = values[0];",
        true,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 43, "bigint", Some("^^^^^^")),
            ("values[0]", 51, "bigint", Some("^^^^^^")),
            (
                "values",
                51,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 58, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn bigint_typed_array_index_strict_unchecked() {
    assert_declarations(
        "declare const values: BigInt64Array; const first = values[0];",
        true,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "first",
                43,
                "bigint | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "values[0]",
                51,
                "bigint | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "values",
                51,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 58, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn bigint_typed_array_index_loose() {
    assert_declarations(
        "declare const values: BigInt64Array; const first = values[0];",
        false,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 43, "bigint", Some("^^^^^^")),
            ("values[0]", 51, "bigint", Some("^^^^^^")),
            (
                "values",
                51,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 58, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn bigint_typed_array_index_loose_unchecked() {
    assert_declarations(
        "declare const values: BigInt64Array; const first = values[0];",
        false,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 43, "bigint", Some("^^^^^^")),
            ("values[0]", 51, "bigint", Some("^^^^^^")),
            (
                "values",
                51,
                "BigInt64Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 58, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn generic_array_like_strict() {
    assert_declarations(
        "function get<T>(values: ArrayLike<T>) { return values[0]; }",
        true,
        "es2020",
        false,
        &[
            (
                "get",
                9,
                "<T>(values: ArrayLike<T>) => T",
                Some("^ ^^      ^^            ^^^^^^"),
            ),
            ("values", 16, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("values[0]", 47, "T", Some("^")),
            ("values", 47, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("0", 54, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn generic_array_like_strict_unchecked() {
    assert_declarations(
        "function get<T>(values: ArrayLike<T>) { return values[0]; }",
        true,
        "es2020",
        true,
        &[
            (
                "get",
                9,
                "<T>(values: ArrayLike<T>) => T | undefined",
                Some("^ ^^      ^^            ^^^^^^^^^^^^^^^^^^"),
            ),
            ("values", 16, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("values[0]", 47, "T | undefined", Some("^^^^^^^^^^^^^")),
            ("values", 47, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("0", 54, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn generic_array_like_loose() {
    assert_declarations(
        "function get<T>(values: ArrayLike<T>) { return values[0]; }",
        false,
        "es2020",
        false,
        &[
            (
                "get",
                9,
                "<T>(values: ArrayLike<T>) => T",
                Some("^ ^^      ^^            ^^^^^^"),
            ),
            ("values", 16, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("values[0]", 47, "T", Some("^")),
            ("values", 47, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("0", 54, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn generic_array_like_loose_unchecked() {
    assert_declarations(
        "function get<T>(values: ArrayLike<T>) { return values[0]; }",
        false,
        "es2020",
        true,
        &[
            (
                "get",
                9,
                "<T>(values: ArrayLike<T>) => T",
                Some("^ ^^      ^^            ^^^^^^"),
            ),
            ("values", 16, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("values[0]", 47, "T", Some("^")),
            ("values", 47, "ArrayLike<T>", Some("^^^^^^^^^^^^")),
            ("0", 54, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn inherited_match_array_strict() {
    assert_declarations("interface Matches extends RegExpMatchArray {} declare const values: Matches; const first = values[0]; const next = values[1];", true, "es2020", false, &[
        ("values", 60, "Matches", Some("^^^^^^^")),
        ("first", 83, "string", Some("^^^^^^")),
        ("values[0]", 91, "string", Some("^^^^^^")),
        ("values", 91, "Matches", Some("^^^^^^^")),
        ("0", 98, "0", Some("^")),
        ("next", 108, "string", Some("^^^^^^")),
        ("values[1]", 115, "string", Some("^^^^^^")),
        ("values", 115, "Matches", Some("^^^^^^^")),
        ("1", 122, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn inherited_match_array_strict_unchecked() {
    assert_declarations("interface Matches extends RegExpMatchArray {} declare const values: Matches; const first = values[0]; const next = values[1];", true, "es2020", true, &[
        ("values", 60, "Matches", Some("^^^^^^^")),
        ("first", 83, "string", Some("^^^^^^")),
        ("values[0]", 91, "string", Some("^^^^^^")),
        ("values", 91, "Matches", Some("^^^^^^^")),
        ("0", 98, "0", Some("^")),
        ("next", 108, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values[1]", 115, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 115, "Matches", Some("^^^^^^^")),
        ("1", 122, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn inherited_match_array_loose() {
    assert_declarations("interface Matches extends RegExpMatchArray {} declare const values: Matches; const first = values[0]; const next = values[1];", false, "es2020", false, &[
        ("values", 60, "Matches", Some("^^^^^^^")),
        ("first", 83, "string", Some("^^^^^^")),
        ("values[0]", 91, "string", Some("^^^^^^")),
        ("values", 91, "Matches", Some("^^^^^^^")),
        ("0", 98, "0", Some("^")),
        ("next", 108, "string", Some("^^^^^^")),
        ("values[1]", 115, "string", Some("^^^^^^")),
        ("values", 115, "Matches", Some("^^^^^^^")),
        ("1", 122, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn inherited_match_array_loose_unchecked() {
    assert_declarations("interface Matches extends RegExpMatchArray {} declare const values: Matches; const first = values[0]; const next = values[1];", false, "es2020", true, &[
        ("values", 60, "Matches", Some("^^^^^^^")),
        ("first", 83, "string", Some("^^^^^^")),
        ("values[0]", 91, "string", Some("^^^^^^")),
        ("values", 91, "Matches", Some("^^^^^^^")),
        ("0", 98, "0", Some("^")),
        ("next", 108, "string", Some("^^^^^^")),
        ("values[1]", 115, "string", Some("^^^^^^")),
        ("values", 115, "Matches", Some("^^^^^^^")),
        ("1", 122, "1", Some("^")),
    ], &[
    ]);
}

#[test]
fn inherited_generic_array_like_strict() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; const result = values[0];", true, "es2020", false, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("result", 84, "string", Some("^^^^^^")),
        ("values[0]", 93, "string", Some("^^^^^^")),
        ("values", 93, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 100, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn inherited_generic_array_like_strict_unchecked() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; const result = values[0];", true, "es2020", true, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("result", 84, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values[0]", 93, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 93, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 100, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn inherited_generic_array_like_loose() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; const result = values[0];", false, "es2020", false, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("result", 84, "string", Some("^^^^^^")),
        ("values[0]", 93, "string", Some("^^^^^^")),
        ("values", 93, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 100, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn inherited_generic_array_like_loose_unchecked() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; const result = values[0];", false, "es2020", true, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("result", 84, "string", Some("^^^^^^")),
        ("values[0]", 93, "string", Some("^^^^^^")),
        ("values", 93, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 100, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn numeric_index_override_strict() {
    assert_declarations("interface List extends ArrayLike<string | number> { [index: number]: string; } declare const values: List; const result = values[0];", true, "es2020", false, &[
        ("index", 53, "number", Some("^^^^^^")),
        ("values", 93, "List", Some("^^^^")),
        ("result", 113, "string", Some("^^^^^^")),
        ("values[0]", 122, "string", Some("^^^^^^")),
        ("values", 122, "List", Some("^^^^")),
        ("0", 129, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn numeric_index_override_strict_unchecked() {
    assert_declarations("interface List extends ArrayLike<string | number> { [index: number]: string; } declare const values: List; const result = values[0];", true, "es2020", true, &[
        ("index", 53, "number", Some("^^^^^^")),
        ("values", 93, "List", Some("^^^^")),
        ("result", 113, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values[0]", 122, "string | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("values", 122, "List", Some("^^^^")),
        ("0", 129, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn numeric_index_override_loose() {
    assert_declarations("interface List extends ArrayLike<string | number> { [index: number]: string; } declare const values: List; const result = values[0];", false, "es2020", false, &[
        ("index", 53, "number", Some("^^^^^^")),
        ("values", 93, "List", Some("^^^^")),
        ("result", 113, "string", Some("^^^^^^")),
        ("values[0]", 122, "string", Some("^^^^^^")),
        ("values", 122, "List", Some("^^^^")),
        ("0", 129, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn numeric_index_override_loose_unchecked() {
    assert_declarations("interface List extends ArrayLike<string | number> { [index: number]: string; } declare const values: List; const result = values[0];", false, "es2020", true, &[
        ("index", 53, "number", Some("^^^^^^")),
        ("values", 93, "List", Some("^^^^")),
        ("result", 113, "string", Some("^^^^^^")),
        ("values[0]", 122, "string", Some("^^^^^^")),
        ("values", 122, "List", Some("^^^^")),
        ("0", 129, "0", Some("^")),
    ], &[
    ]);
}

#[test]
fn indexed_type_numeric_strict() {
    assert_declarations(
        "type Element = RegExpMatchArray[number]; declare const value: Element;",
        true,
        "es2020",
        false,
        &[
            ("Element", 5, "string", Some("^^^^^^")),
            ("value", 55, "string", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn indexed_type_numeric_strict_unchecked() {
    assert_declarations(
        "type Element = RegExpMatchArray[number]; declare const value: Element;",
        true,
        "es2020",
        true,
        &[
            ("Element", 5, "string", Some("^^^^^^")),
            ("value", 55, "string", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn indexed_type_numeric_loose() {
    assert_declarations(
        "type Element = RegExpMatchArray[number]; declare const value: Element;",
        false,
        "es2020",
        false,
        &[
            ("Element", 5, "string", Some("^^^^^^")),
            ("value", 55, "string", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn indexed_type_numeric_loose_unchecked() {
    assert_declarations(
        "type Element = RegExpMatchArray[number]; declare const value: Element;",
        false,
        "es2020",
        true,
        &[
            ("Element", 5, "string", Some("^^^^^^")),
            ("value", 55, "string", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn indexed_type_known_property_strict() {
    assert_declarations("type First = RegExpMatchArray[0]; type Index = RegExpMatchArray[\"index\"]; declare const first: First; declare const index: Index;", true, "es2020", false, &[
        ("First", 5, "string", Some("^^^^^^")),
        ("Index", 39, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("first", 88, "string", Some("^^^^^^")),
        ("index", 116, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn indexed_type_known_property_strict_unchecked() {
    assert_declarations("type First = RegExpMatchArray[0]; type Index = RegExpMatchArray[\"index\"]; declare const first: First; declare const index: Index;", true, "es2020", true, &[
        ("First", 5, "string", Some("^^^^^^")),
        ("Index", 39, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
        ("first", 88, "string", Some("^^^^^^")),
        ("index", 116, "number | undefined", Some("^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn indexed_type_known_property_loose() {
    assert_declarations("type First = RegExpMatchArray[0]; type Index = RegExpMatchArray[\"index\"]; declare const first: First; declare const index: Index;", false, "es2020", false, &[
        ("First", 5, "string", Some("^^^^^^")),
        ("Index", 39, "number", Some("^^^^^^")),
        ("first", 88, "string", Some("^^^^^^")),
        ("index", 116, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn indexed_type_known_property_loose_unchecked() {
    assert_declarations("type First = RegExpMatchArray[0]; type Index = RegExpMatchArray[\"index\"]; declare const first: First; declare const index: Index;", false, "es2020", true, &[
        ("First", 5, "string", Some("^^^^^^")),
        ("Index", 39, "number", Some("^^^^^^")),
        ("first", 88, "string", Some("^^^^^^")),
        ("index", 116, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_bracket_method_strict() {
    assert_declarations("declare const values: number[]; const method = values[\"map\"];", true, "es2020", false, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 38, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 47, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 54, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_bracket_method_strict_unchecked() {
    assert_declarations("declare const values: number[]; const method = values[\"map\"];", true, "es2020", true, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 38, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 47, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 54, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_bracket_method_loose() {
    assert_declarations("declare const values: number[]; const method = values[\"map\"];", false, "es2020", false, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 38, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 47, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 54, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn array_bracket_method_loose_unchecked() {
    assert_declarations("declare const values: number[]; const method = values[\"map\"];", false, "es2020", true, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 38, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 47, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 54, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn named_array_bracket_method_strict() {
    assert_declarations("declare const values: Array<number>; const method = values[\"map\"];", true, "es2020", false, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 43, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 52, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 52, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 59, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn named_array_bracket_method_strict_unchecked() {
    assert_declarations("declare const values: Array<number>; const method = values[\"map\"];", true, "es2020", true, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 43, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 52, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 52, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 59, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn named_array_bracket_method_loose() {
    assert_declarations("declare const values: Array<number>; const method = values[\"map\"];", false, "es2020", false, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 43, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 52, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 52, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 59, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn named_array_bracket_method_loose_unchecked() {
    assert_declarations("declare const values: Array<number>; const method = values[\"map\"];", false, "es2020", true, &[
        ("values", 14, "number[]", Some("^^^^^^^^")),
        ("method", 43, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values[\"map\"]", 52, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", Some("^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^")),
        ("values", 52, "number[]", Some("^^^^^^^^")),
        ("\"map\"", 59, "\"map\"", Some("^^^^^")),
    ], &[
    ]);
}

#[test]
fn typed_array_assignment_strict() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = \"wrong\";",
        true,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = \"wrong\"", 34, "\"wrong\"", Some("^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("\"wrong\"", 46, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2322, 34, 9)],
    );
}

#[test]
fn typed_array_assignment_strict_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = \"wrong\";",
        true,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = \"wrong\"", 34, "\"wrong\"", Some("^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("\"wrong\"", 46, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2322, 34, 9)],
    );
}

#[test]
fn typed_array_assignment_loose() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = \"wrong\";",
        false,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = \"wrong\"", 34, "\"wrong\"", Some("^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("\"wrong\"", 46, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2322, 34, 9)],
    );
}

#[test]
fn typed_array_assignment_loose_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = \"wrong\";",
        false,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = \"wrong\"", 34, "\"wrong\"", Some("^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("\"wrong\"", 46, "\"wrong\"", Some("^^^^^^^")),
        ],
        &[(2322, 34, 9)],
    );
}

#[test]
fn array_like_readonly_assignment_strict() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[0] = 1;",
        true,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[0] = 1", 41, "1", Some("^")),
            ("values[0]", 41, "number", Some("^^^^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 48, "0", Some("^")),
            ("1", 53, "1", Some("^")),
        ],
        &[(2542, 41, 9)],
    );
}

#[test]
fn array_like_readonly_assignment_strict_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[0] = 1;",
        true,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[0] = 1", 41, "1", Some("^")),
            ("values[0]", 41, "number", Some("^^^^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 48, "0", Some("^")),
            ("1", 53, "1", Some("^")),
        ],
        &[(2542, 41, 9)],
    );
}

#[test]
fn array_like_readonly_assignment_loose() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[0] = 1;",
        false,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[0] = 1", 41, "1", Some("^")),
            ("values[0]", 41, "number", Some("^^^^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 48, "0", Some("^")),
            ("1", 53, "1", Some("^")),
        ],
        &[(2542, 41, 9)],
    );
}

#[test]
fn array_like_readonly_assignment_loose_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[0] = 1;",
        false,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[0] = 1", 41, "1", Some("^")),
            ("values[0]", 41, "number", Some("^^^^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 48, "0", Some("^")),
            ("1", 53, "1", Some("^")),
        ],
        &[(2542, 41, 9)],
    );
}

#[test]
fn array_like_readonly_length_strict() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values.length = 1;",
        true,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values.length = 1", 41, "1", Some("^")),
            ("values.length", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 48, "any", Some("^^^")),
            ("1", 57, "1", Some("^")),
        ],
        &[(2540, 48, 6)],
    );
}

#[test]
fn array_like_readonly_length_strict_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values.length = 1;",
        true,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values.length = 1", 41, "1", Some("^")),
            ("values.length", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 48, "any", Some("^^^")),
            ("1", 57, "1", Some("^")),
        ],
        &[(2540, 48, 6)],
    );
}

#[test]
fn array_like_readonly_length_loose() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values.length = 1;",
        false,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values.length = 1", 41, "1", Some("^")),
            ("values.length", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 48, "any", Some("^^^")),
            ("1", 57, "1", Some("^")),
        ],
        &[(2540, 48, 6)],
    );
}

#[test]
fn array_like_readonly_length_loose_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values.length = 1;",
        false,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values.length = 1", 41, "1", Some("^")),
            ("values.length", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 48, "any", Some("^^^")),
            ("1", 57, "1", Some("^")),
        ],
        &[(2540, 48, 6)],
    );
}

#[test]
fn no_unchecked_assignment_strict() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = undefined;",
        true,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = undefined", 34, "undefined", Some("^^^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("undefined", 46, "undefined", Some("^^^^^^^^^")),
        ],
        &[(2322, 34, 9)],
    );
}

#[test]
fn no_unchecked_assignment_strict_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = undefined;",
        true,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = undefined", 34, "undefined", Some("^^^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("undefined", 46, "undefined", Some("^^^^^^^^^")),
        ],
        &[(2322, 34, 9)],
    );
}

#[test]
fn no_unchecked_assignment_loose() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = undefined;",
        false,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = undefined", 34, "undefined", Some("^^^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("undefined", 46, "undefined", Some("^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn no_unchecked_assignment_loose_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array; values[0] = undefined;",
        false,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("values[0] = undefined", 34, "undefined", Some("^^^^^^^^^")),
            ("values[0]", 34, "number", Some("^^^^^^")),
            (
                "values",
                34,
                "Uint8Array<ArrayBufferLike>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 41, "0", Some("^")),
            ("undefined", 46, "undefined", Some("^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn inherited_readonly_write_strict() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values[0] = \"x\";", true, "es2020", false, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values[0] = \"x\"", 78, "\"x\"", Some("^^^")),
        ("values[0]", 78, "string", Some("^^^^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 85, "0", Some("^")),
        ("\"x\"", 90, "\"x\"", Some("^^^")),
    ], &[
        (2542, 78, 9),
    ]);
}

#[test]
fn inherited_readonly_write_strict_unchecked() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values[0] = \"x\";", true, "es2020", true, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values[0] = \"x\"", 78, "\"x\"", Some("^^^")),
        ("values[0]", 78, "string", Some("^^^^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 85, "0", Some("^")),
        ("\"x\"", 90, "\"x\"", Some("^^^")),
    ], &[
        (2542, 78, 9),
    ]);
}

#[test]
fn inherited_readonly_write_loose() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values[0] = \"x\";", false, "es2020", false, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values[0] = \"x\"", 78, "\"x\"", Some("^^^")),
        ("values[0]", 78, "string", Some("^^^^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 85, "0", Some("^")),
        ("\"x\"", 90, "\"x\"", Some("^^^")),
    ], &[
        (2542, 78, 9),
    ]);
}

#[test]
fn inherited_readonly_write_loose_unchecked() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values[0] = \"x\";", false, "es2020", true, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values[0] = \"x\"", 78, "\"x\"", Some("^^^")),
        ("values[0]", 78, "string", Some("^^^^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("0", 85, "0", Some("^")),
        ("\"x\"", 90, "\"x\"", Some("^^^")),
    ], &[
        (2542, 78, 9),
    ]);
}

#[test]
fn inherited_readonly_length_strict() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values.length = 2;", true, "es2020", false, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values.length = 2", 78, "2", Some("^")),
        ("values.length", 78, "any", Some("^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("length", 85, "any", Some("^^^")),
        ("2", 94, "2", Some("^")),
    ], &[
        (2540, 85, 6),
    ]);
}

#[test]
fn inherited_readonly_length_strict_unchecked() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values.length = 2;", true, "es2020", true, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values.length = 2", 78, "2", Some("^")),
        ("values.length", 78, "any", Some("^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("length", 85, "any", Some("^^^")),
        ("2", 94, "2", Some("^")),
    ], &[
        (2540, 85, 6),
    ]);
}

#[test]
fn inherited_readonly_length_loose() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values.length = 2;", false, "es2020", false, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values.length = 2", 78, "2", Some("^")),
        ("values.length", 78, "any", Some("^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("length", 85, "any", Some("^^^")),
        ("2", 94, "2", Some("^")),
    ], &[
        (2540, 85, 6),
    ]);
}

#[test]
fn inherited_readonly_length_loose_unchecked() {
    assert_declarations("interface List<T> extends ArrayLike<T> {} declare const values: List<string>; values.length = 2;", false, "es2020", true, &[
        ("values", 56, "List<string>", Some("^^^^^^^^^^^^")),
        ("values.length = 2", 78, "2", Some("^")),
        ("values.length", 78, "any", Some("^^^")),
        ("values", 78, "List<string>", Some("^^^^^^^^^^^^")),
        ("length", 85, "any", Some("^^^")),
        ("2", 94, "2", Some("^")),
    ], &[
        (2540, 85, 6),
    ]);
}

#[test]
fn override_writable_index_strict() {
    assert_declarations("interface List extends ArrayLike<string> { [index: number]: string; } declare const values: List; values[0] = \"x\";", true, "es2020", false, &[
        ("index", 44, "number", Some("^^^^^^")),
        ("values", 84, "List", Some("^^^^")),
        ("values[0] = \"x\"", 98, "\"x\"", Some("^^^")),
        ("values[0]", 98, "string", Some("^^^^^^")),
        ("values", 98, "List", Some("^^^^")),
        ("0", 105, "0", Some("^")),
        ("\"x\"", 110, "\"x\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn override_writable_index_strict_unchecked() {
    assert_declarations("interface List extends ArrayLike<string> { [index: number]: string; } declare const values: List; values[0] = \"x\";", true, "es2020", true, &[
        ("index", 44, "number", Some("^^^^^^")),
        ("values", 84, "List", Some("^^^^")),
        ("values[0] = \"x\"", 98, "\"x\"", Some("^^^")),
        ("values[0]", 98, "string", Some("^^^^^^")),
        ("values", 98, "List", Some("^^^^")),
        ("0", 105, "0", Some("^")),
        ("\"x\"", 110, "\"x\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn override_writable_index_loose() {
    assert_declarations("interface List extends ArrayLike<string> { [index: number]: string; } declare const values: List; values[0] = \"x\";", false, "es2020", false, &[
        ("index", 44, "number", Some("^^^^^^")),
        ("values", 84, "List", Some("^^^^")),
        ("values[0] = \"x\"", 98, "\"x\"", Some("^^^")),
        ("values[0]", 98, "string", Some("^^^^^^")),
        ("values", 98, "List", Some("^^^^")),
        ("0", 105, "0", Some("^")),
        ("\"x\"", 110, "\"x\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn override_writable_index_loose_unchecked() {
    assert_declarations("interface List extends ArrayLike<string> { [index: number]: string; } declare const values: List; values[0] = \"x\";", false, "es2020", true, &[
        ("index", 44, "number", Some("^^^^^^")),
        ("values", 84, "List", Some("^^^^")),
        ("values[0] = \"x\"", 98, "\"x\"", Some("^^^")),
        ("values[0]", 98, "string", Some("^^^^^^")),
        ("values", 98, "List", Some("^^^^")),
        ("0", 105, "0", Some("^")),
        ("\"x\"", 110, "\"x\"", Some("^^^")),
    ], &[
    ]);
}

#[test]
fn override_writable_length_strict() {
    assert_declarations("interface List extends ArrayLike<string> { length: number; } declare const values: List; values.length = 2;", true, "es2020", false, &[
        ("length", 43, "number", Some("^^^^^^")),
        ("values", 75, "List", Some("^^^^")),
        ("values.length = 2", 89, "2", Some("^")),
        ("values.length", 89, "number", Some("^^^^^^")),
        ("values", 89, "List", Some("^^^^")),
        ("length", 96, "number", Some("^^^^^^")),
        ("2", 105, "2", Some("^")),
    ], &[
    ]);
}

#[test]
fn override_writable_length_strict_unchecked() {
    assert_declarations("interface List extends ArrayLike<string> { length: number; } declare const values: List; values.length = 2;", true, "es2020", true, &[
        ("length", 43, "number", Some("^^^^^^")),
        ("values", 75, "List", Some("^^^^")),
        ("values.length = 2", 89, "2", Some("^")),
        ("values.length", 89, "number", Some("^^^^^^")),
        ("values", 89, "List", Some("^^^^")),
        ("length", 96, "number", Some("^^^^^^")),
        ("2", 105, "2", Some("^")),
    ], &[
    ]);
}

#[test]
fn override_writable_length_loose() {
    assert_declarations("interface List extends ArrayLike<string> { length: number; } declare const values: List; values.length = 2;", false, "es2020", false, &[
        ("length", 43, "number", Some("^^^^^^")),
        ("values", 75, "List", Some("^^^^")),
        ("values.length = 2", 89, "2", Some("^")),
        ("values.length", 89, "number", Some("^^^^^^")),
        ("values", 89, "List", Some("^^^^")),
        ("length", 96, "number", Some("^^^^^^")),
        ("2", 105, "2", Some("^")),
    ], &[
    ]);
}

#[test]
fn override_writable_length_loose_unchecked() {
    assert_declarations("interface List extends ArrayLike<string> { length: number; } declare const values: List; values.length = 2;", false, "es2020", true, &[
        ("length", 43, "number", Some("^^^^^^")),
        ("values", 75, "List", Some("^^^^")),
        ("values.length = 2", 89, "2", Some("^")),
        ("values.length", 89, "number", Some("^^^^^^")),
        ("values", 89, "List", Some("^^^^")),
        ("length", 96, "number", Some("^^^^^^")),
        ("2", 105, "2", Some("^")),
    ], &[
    ]);
}

#[test]
fn readonly_delete_index_strict() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values[0];",
        true,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values[0]", 41, "boolean", Some("^^^^^^^")),
            ("values[0]", 48, "number", Some("^^^^^^")),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 55, "0", Some("^")),
        ],
        &[(2542, 48, 9)],
    );
}

#[test]
fn readonly_delete_index_strict_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values[0];",
        true,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values[0]", 41, "boolean", Some("^^^^^^^")),
            (
                "values[0]",
                48,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 55, "0", Some("^")),
        ],
        &[(2542, 48, 9)],
    );
}

#[test]
fn readonly_delete_index_loose() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values[0];",
        false,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values[0]", 41, "boolean", Some("^^^^^^^")),
            ("values[0]", 48, "number", Some("^^^^^^")),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 55, "0", Some("^")),
        ],
        &[(2542, 48, 9)],
    );
}

#[test]
fn readonly_delete_index_loose_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values[0];",
        false,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values[0]", 41, "boolean", Some("^^^^^^^")),
            ("values[0]", 48, "number", Some("^^^^^^")),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("0", 55, "0", Some("^")),
        ],
        &[(2542, 48, 9)],
    );
}

#[test]
fn readonly_delete_length_strict() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values.length;",
        true,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values.length", 41, "boolean", Some("^^^^^^^")),
            ("values.length", 48, "number", Some("^^^^^^")),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 55, "number", Some("^^^^^^")),
        ],
        &[(2704, 48, 13)],
    );
}

#[test]
fn readonly_delete_length_strict_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values.length;",
        true,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values.length", 41, "boolean", Some("^^^^^^^")),
            ("values.length", 48, "number", Some("^^^^^^")),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 55, "number", Some("^^^^^^")),
        ],
        &[(2704, 48, 13)],
    );
}

#[test]
fn readonly_delete_length_loose() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values.length;",
        false,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values.length", 41, "boolean", Some("^^^^^^^")),
            ("values.length", 48, "number", Some("^^^^^^")),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 55, "number", Some("^^^^^^")),
        ],
        &[(2704, 48, 13)],
    );
}

#[test]
fn readonly_delete_length_loose_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; delete values.length;",
        false,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("delete values.length", 41, "boolean", Some("^^^^^^^")),
            ("values.length", 48, "number", Some("^^^^^^")),
            ("values", 48, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("length", 55, "number", Some("^^^^^^")),
        ],
        &[(2704, 48, 13)],
    );
}

#[test]
fn readonly_bracket_length_strict() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[\"length\"] = 2;",
        true,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[\"length\"] = 2", 41, "2", Some("^")),
            ("values[\"length\"]", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("\"length\"", 48, "\"length\"", Some("^^^^^^^^")),
            ("2", 60, "2", Some("^")),
        ],
        &[(2540, 48, 8)],
    );
}

#[test]
fn readonly_bracket_length_strict_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[\"length\"] = 2;",
        true,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[\"length\"] = 2", 41, "2", Some("^")),
            ("values[\"length\"]", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("\"length\"", 48, "\"length\"", Some("^^^^^^^^")),
            ("2", 60, "2", Some("^")),
        ],
        &[(2540, 48, 8)],
    );
}

#[test]
fn readonly_bracket_length_loose() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[\"length\"] = 2;",
        false,
        "es2020",
        false,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[\"length\"] = 2", 41, "2", Some("^")),
            ("values[\"length\"]", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("\"length\"", 48, "\"length\"", Some("^^^^^^^^")),
            ("2", 60, "2", Some("^")),
        ],
        &[(2540, 48, 8)],
    );
}

#[test]
fn readonly_bracket_length_loose_unchecked() {
    assert_declarations(
        "declare const values: ArrayLike<number>; values[\"length\"] = 2;",
        false,
        "es2020",
        true,
        &[
            ("values", 14, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("values[\"length\"] = 2", 41, "2", Some("^")),
            ("values[\"length\"]", 41, "any", Some("^^^")),
            ("values", 41, "ArrayLike<number>", Some("^^^^^^^^^^^^^^^^^")),
            ("\"length\"", 48, "\"length\"", Some("^^^^^^^^")),
            ("2", 60, "2", Some("^")),
        ],
        &[(2540, 48, 8)],
    );
}

#[test]
fn typed_array_explicit_argument_strict() {
    assert_declarations(
        "declare const values: Uint8Array<ArrayBuffer>; const first = values[0];",
        true,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 53, "number", Some("^^^^^^")),
            ("values[0]", 61, "number", Some("^^^^^^")),
            (
                "values",
                61,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 68, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn typed_array_explicit_argument_strict_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array<ArrayBuffer>; const first = values[0];",
        true,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "first",
                53,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "values[0]",
                61,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "values",
                61,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 68, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn typed_array_explicit_argument_loose() {
    assert_declarations(
        "declare const values: Uint8Array<ArrayBuffer>; const first = values[0];",
        false,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 53, "number", Some("^^^^^^")),
            ("values[0]", 61, "number", Some("^^^^^^")),
            (
                "values",
                61,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 68, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn typed_array_explicit_argument_loose_unchecked() {
    assert_declarations(
        "declare const values: Uint8Array<ArrayBuffer>; const first = values[0];",
        false,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("first", 53, "number", Some("^^^^^^")),
            ("values[0]", 61, "number", Some("^^^^^^")),
            (
                "values",
                61,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("0", 68, "0", Some("^")),
        ],
        &[],
    );
}

#[test]
fn typed_array_properties_strict() {
    assert_declarations("declare const values: Uint8Array; const length = values.length; const bytes = values.byteLength; const buffer = values.buffer;", true, "es2020", false, &[
        ("values", 14, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 40, "number", Some("^^^^^^")),
        ("values.length", 49, "number", Some("^^^^^^")),
        ("values", 49, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 56, "number", Some("^^^^^^")),
        ("bytes", 70, "number", Some("^^^^^^")),
        ("values.byteLength", 78, "number", Some("^^^^^^")),
        ("values", 78, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("byteLength", 85, "number", Some("^^^^^^")),
        ("buffer", 103, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values.buffer", 112, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values", 112, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("buffer", 119, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn typed_array_properties_strict_unchecked() {
    assert_declarations("declare const values: Uint8Array; const length = values.length; const bytes = values.byteLength; const buffer = values.buffer;", true, "es2020", true, &[
        ("values", 14, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 40, "number", Some("^^^^^^")),
        ("values.length", 49, "number", Some("^^^^^^")),
        ("values", 49, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 56, "number", Some("^^^^^^")),
        ("bytes", 70, "number", Some("^^^^^^")),
        ("values.byteLength", 78, "number", Some("^^^^^^")),
        ("values", 78, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("byteLength", 85, "number", Some("^^^^^^")),
        ("buffer", 103, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values.buffer", 112, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values", 112, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("buffer", 119, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn typed_array_properties_loose() {
    assert_declarations("declare const values: Uint8Array; const length = values.length; const bytes = values.byteLength; const buffer = values.buffer;", false, "es2020", false, &[
        ("values", 14, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 40, "number", Some("^^^^^^")),
        ("values.length", 49, "number", Some("^^^^^^")),
        ("values", 49, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 56, "number", Some("^^^^^^")),
        ("bytes", 70, "number", Some("^^^^^^")),
        ("values.byteLength", 78, "number", Some("^^^^^^")),
        ("values", 78, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("byteLength", 85, "number", Some("^^^^^^")),
        ("buffer", 103, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values.buffer", 112, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values", 112, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("buffer", 119, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn typed_array_properties_loose_unchecked() {
    assert_declarations("declare const values: Uint8Array; const length = values.length; const bytes = values.byteLength; const buffer = values.buffer;", false, "es2020", true, &[
        ("values", 14, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 40, "number", Some("^^^^^^")),
        ("values.length", 49, "number", Some("^^^^^^")),
        ("values", 49, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 56, "number", Some("^^^^^^")),
        ("bytes", 70, "number", Some("^^^^^^")),
        ("values.byteLength", 78, "number", Some("^^^^^^")),
        ("values", 78, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("byteLength", 85, "number", Some("^^^^^^")),
        ("buffer", 103, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values.buffer", 112, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
        ("values", 112, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("buffer", 119, "ArrayBufferLike", Some("^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterable_defaults_strict() {
    assert_declarations("declare const values: Iterable<number>; declare const explicit: Iterable<number, any, any>;", true, "es2020", false, &[
        ("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
        ("explicit", 54, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterable_defaults_strict_unchecked() {
    assert_declarations("declare const values: Iterable<number>; declare const explicit: Iterable<number, any, any>;", true, "es2020", true, &[
        ("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
        ("explicit", 54, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterable_defaults_loose() {
    assert_declarations("declare const values: Iterable<number>; declare const explicit: Iterable<number, any, any>;", false, "es2020", false, &[
        ("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
        ("explicit", 54, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterable_defaults_loose_unchecked() {
    assert_declarations("declare const values: Iterable<number>; declare const explicit: Iterable<number, any, any>;", false, "es2020", true, &[
        ("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
        ("explicit", 54, "Iterable<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn async_iterable_defaults_strict() {
    assert_declarations("declare const values: AsyncIterable<number>; declare const explicit: AsyncIterable<number, any, any>;", true, "es2020", false, &[
        ("values", 14, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
        ("explicit", 59, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn async_iterable_defaults_strict_unchecked() {
    assert_declarations("declare const values: AsyncIterable<number>; declare const explicit: AsyncIterable<number, any, any>;", true, "es2020", true, &[
        ("values", 14, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
        ("explicit", 59, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn async_iterable_defaults_loose() {
    assert_declarations("declare const values: AsyncIterable<number>; declare const explicit: AsyncIterable<number, any, any>;", false, "es2020", false, &[
        ("values", 14, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
        ("explicit", 59, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn async_iterable_defaults_loose_unchecked() {
    assert_declarations("declare const values: AsyncIterable<number>; declare const explicit: AsyncIterable<number, any, any>;", false, "es2020", true, &[
        ("values", 14, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
        ("explicit", 59, "AsyncIterable<number>", Some("^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterator_defaults_strict() {
    assert_declarations(
        "declare const values: Iterator<number>; declare const iterable: IterableIterator<number>;",
        true,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Iterator<number, any, any>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "iterable",
                54,
                "IterableIterator<number>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn iterator_defaults_strict_unchecked() {
    assert_declarations(
        "declare const values: Iterator<number>; declare const iterable: IterableIterator<number>;",
        true,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Iterator<number, any, any>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "iterable",
                54,
                "IterableIterator<number>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn iterator_defaults_loose() {
    assert_declarations(
        "declare const values: Iterator<number>; declare const iterable: IterableIterator<number>;",
        false,
        "es2020",
        false,
        &[
            (
                "values",
                14,
                "Iterator<number, any, any>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "iterable",
                54,
                "IterableIterator<number>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn iterator_defaults_loose_unchecked() {
    assert_declarations(
        "declare const values: Iterator<number>; declare const iterable: IterableIterator<number>;",
        false,
        "es2020",
        true,
        &[
            (
                "values",
                14,
                "Iterator<number, any, any>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "iterable",
                54,
                "IterableIterator<number>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn iterable_explicit_defaults_strict() {
    assert_declarations(
        "declare const values: Iterable<number, any, any>;",
        true,
        "es2020",
        false,
        &[("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn iterable_explicit_defaults_strict_unchecked() {
    assert_declarations(
        "declare const values: Iterable<number, any, any>;",
        true,
        "es2020",
        true,
        &[("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn iterable_explicit_defaults_loose() {
    assert_declarations(
        "declare const values: Iterable<number, any, any>;",
        false,
        "es2020",
        false,
        &[("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn iterable_explicit_defaults_loose_unchecked() {
    assert_declarations(
        "declare const values: Iterable<number, any, any>;",
        false,
        "es2020",
        true,
        &[("values", 14, "Iterable<number>", Some("^^^^^^^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn iterable_nondefault_arguments_strict() {
    assert_declarations("declare const values: Iterable<number, string, boolean>; declare const partial: AsyncIterable<number, string>;", true, "es2020", false, &[
        ("values", 14, "Iterable<number, string, boolean>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("partial", 71, "AsyncIterable<number, string>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterable_nondefault_arguments_strict_unchecked() {
    assert_declarations("declare const values: Iterable<number, string, boolean>; declare const partial: AsyncIterable<number, string>;", true, "es2020", true, &[
        ("values", 14, "Iterable<number, string, boolean>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("partial", 71, "AsyncIterable<number, string>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterable_nondefault_arguments_loose() {
    assert_declarations("declare const values: Iterable<number, string, boolean>; declare const partial: AsyncIterable<number, string>;", false, "es2020", false, &[
        ("values", 14, "Iterable<number, string, boolean>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("partial", 71, "AsyncIterable<number, string>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn iterable_nondefault_arguments_loose_unchecked() {
    assert_declarations("declare const values: Iterable<number, string, boolean>; declare const partial: AsyncIterable<number, string>;", false, "es2020", true, &[
        ("values", 14, "Iterable<number, string, boolean>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("partial", 71, "AsyncIterable<number, string>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}
