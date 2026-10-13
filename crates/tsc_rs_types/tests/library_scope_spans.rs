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
fn declared_function_parameter_strict() {
    assert_declarations(
        "declare function identity<Uint8Array>(value: Uint8Array): Uint8Array;",
        true,
        "es2020",
        false,
        &[
            (
                "identity",
                17,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^          "),
            ),
            ("value", 38, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn declared_function_parameter_loose() {
    assert_declarations(
        "declare function identity<Uint8Array>(value: Uint8Array): Uint8Array;",
        false,
        "es2020",
        false,
        &[
            (
                "identity",
                17,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^          "),
            ),
            ("value", 38, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn function_parameter_strict() {
    assert_declarations(
        "function identity<Uint8Array>(value: Uint8Array) { return value; }",
        true,
        "es2020",
        false,
        &[
            (
                "identity",
                9,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 30, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 58, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn function_parameter_loose() {
    assert_declarations(
        "function identity<Uint8Array>(value: Uint8Array) { return value; }",
        false,
        "es2020",
        false,
        &[
            (
                "identity",
                9,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 30, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 58, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn generic_call_strict() {
    assert_declarations("function identity<Uint8Array>(value: Uint8Array) { return value; } const result = identity(\"text\");", true, "es2020", false, &[
        ("identity", 9, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 30, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 58, "Uint8Array", Some("^^^^^^^^^^")),
        ("result", 73, "\"text\"", Some("^^^^^^")),
        ("identity(\"text\")", 82, "\"text\"", Some("^^^^^^")),
        ("identity", 82, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("\"text\"", 91, "\"text\"", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn generic_call_loose() {
    assert_declarations("function identity<Uint8Array>(value: Uint8Array) { return value; } const result = identity(\"text\");", false, "es2020", false, &[
        ("identity", 9, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 30, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 58, "Uint8Array", Some("^^^^^^^^^^")),
        ("result", 73, "\"text\"", Some("^^^^^^")),
        ("identity(\"text\")", 82, "\"text\"", Some("^^^^^^")),
        ("identity", 82, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("\"text\"", 91, "\"text\"", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn function_type_parameter_strict() {
    assert_declarations("type Identity = <Uint8Array>(value: Uint8Array) => Uint8Array; declare const identity: Identity;", true, "es2020", false, &[
        ("Identity", 5, "Identity", Some("^^^^^^^^")),
        ("value", 29, "Uint8Array", Some("^^^^^^^^^^")),
        ("identity", 77, "Identity", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn function_type_parameter_loose() {
    assert_declarations("type Identity = <Uint8Array>(value: Uint8Array) => Uint8Array; declare const identity: Identity;", false, "es2020", false, &[
        ("Identity", 5, "Identity", Some("^^^^^^^^")),
        ("value", 29, "Uint8Array", Some("^^^^^^^^^^")),
        ("identity", 77, "Identity", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn type_alias_parameter_strict() {
    assert_declarations(
        "type Identity<Uint8Array> = Uint8Array;",
        true,
        "es2020",
        false,
        &[("Identity", 5, "Uint8Array", Some("^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn type_alias_parameter_loose() {
    assert_declarations(
        "type Identity<Uint8Array> = Uint8Array;",
        false,
        "es2020",
        false,
        &[("Identity", 5, "Uint8Array", Some("^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn interface_parameter_strict() {
    assert_declarations(
        "interface Box<Uint8Array> { value: Uint8Array; method(value: Uint8Array): Uint8Array; }",
        true,
        "es2020",
        false,
        &[
            ("value", 28, "Uint8Array", Some("^^^^^^^^^^")),
            (
                "method",
                47,
                "(value: Uint8Array) => Uint8Array",
                Some("^     ^^          ^^^^^          "),
            ),
            ("value", 54, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn interface_parameter_loose() {
    assert_declarations(
        "interface Box<Uint8Array> { value: Uint8Array; method(value: Uint8Array): Uint8Array; }",
        false,
        "es2020",
        false,
        &[
            ("value", 28, "Uint8Array", Some("^^^^^^^^^^")),
            (
                "method",
                47,
                "(value: Uint8Array) => Uint8Array",
                Some("^     ^^          ^^^^^          "),
            ),
            ("value", 54, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn class_parameter_strict() {
    assert_declarations(
        "class Box<Uint8Array> { value!: Uint8Array; method(value: Uint8Array) { return value; } }",
        true,
        "es2020",
        false,
        &[
            ("Box", 6, "Box<Uint8Array>", Some("^^^^^^^^^^^^^^^")),
            ("value", 24, "Uint8Array", Some("^^^^^^^^^^")),
            (
                "method",
                44,
                "(value: Uint8Array) => Uint8Array",
                Some("^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 51, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 79, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn class_parameter_loose() {
    assert_declarations(
        "class Box<Uint8Array> { value!: Uint8Array; method(value: Uint8Array) { return value; } }",
        false,
        "es2020",
        false,
        &[
            ("Box", 6, "Box<Uint8Array>", Some("^^^^^^^^^^^^^^^")),
            ("value", 24, "Uint8Array", Some("^^^^^^^^^^")),
            (
                "method",
                44,
                "(value: Uint8Array) => Uint8Array",
                Some("^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 51, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 79, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn method_parameter_strict() {
    assert_declarations(
        "class Box { method<Uint8Array>(value: Uint8Array) { return value; } }",
        true,
        "es2020",
        false,
        &[
            ("Box", 6, "Box", Some("^^^")),
            (
                "method",
                12,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 31, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 59, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn method_parameter_loose() {
    assert_declarations(
        "class Box { method<Uint8Array>(value: Uint8Array) { return value; } }",
        false,
        "es2020",
        false,
        &[
            ("Box", 6, "Box", Some("^^^")),
            (
                "method",
                12,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 31, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 59, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn interface_method_parameter_strict() {
    assert_declarations(
        "interface Box { method<Uint8Array>(value: Uint8Array): Uint8Array; }",
        true,
        "es2020",
        false,
        &[
            (
                "method",
                16,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^          "),
            ),
            ("value", 35, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn interface_method_parameter_loose() {
    assert_declarations(
        "interface Box { method<Uint8Array>(value: Uint8Array): Uint8Array; }",
        false,
        "es2020",
        false,
        &[
            (
                "method",
                16,
                "<Uint8Array>(value: Uint8Array) => Uint8Array",
                Some("^          ^^     ^^          ^^^^^          "),
            ),
            ("value", 35, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn nested_library_parameter_strict() {
    assert_declarations("function outer<T>(item: T) { return function inner<Uint8Array>(value: Uint8Array) { return value; }; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <Uint8Array>(value: Uint8Array) => Uint8Array", Some("^ ^^    ^^ ^^^^^^          ^^                 ^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<Uint8Array>(value: Uint8Array) { return value; }", 36, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("inner", 45, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 63, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 91, "Uint8Array", Some("^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn nested_library_parameter_loose() {
    assert_declarations("function outer<T>(item: T) { return function inner<Uint8Array>(value: Uint8Array) { return value; }; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <Uint8Array>(value: Uint8Array) => Uint8Array", Some("^ ^^    ^^ ^^^^^^          ^^                 ^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<Uint8Array>(value: Uint8Array) { return value; }", 36, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("inner", 45, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 63, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 91, "Uint8Array", Some("^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn alias_name_parameter_strict() {
    assert_declarations(
        "type T = string; function identity<T>(value: T) { return value; }",
        true,
        "es2020",
        false,
        &[
            ("T", 5, "string", Some("^^^^^^")),
            (
                "identity",
                26,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 38, "T", Some("^")),
            ("value", 57, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn alias_name_parameter_loose() {
    assert_declarations(
        "type T = string; function identity<T>(value: T) { return value; }",
        false,
        "es2020",
        false,
        &[
            ("T", 5, "string", Some("^^^^^^")),
            (
                "identity",
                26,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 38, "T", Some("^")),
            ("value", 57, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn interface_name_parameter_strict() {
    assert_declarations(
        "interface T { field: string; } function identity<T>(value: T) { return value; }",
        true,
        "es2020",
        false,
        &[
            ("field", 14, "string", Some("^^^^^^")),
            (
                "identity",
                40,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 52, "T", Some("^")),
            ("value", 71, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn interface_name_parameter_loose() {
    assert_declarations(
        "interface T { field: string; } function identity<T>(value: T) { return value; }",
        false,
        "es2020",
        false,
        &[
            ("field", 14, "string", Some("^^^^^^")),
            (
                "identity",
                40,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 52, "T", Some("^")),
            ("value", 71, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn class_name_parameter_strict() {
    assert_declarations(
        "class T { field!: string; } function identity<T>(value: T) { return value; }",
        true,
        "es2020",
        false,
        &[
            ("T", 6, "T", Some("^")),
            ("field", 10, "string", Some("^^^^^^")),
            (
                "identity",
                37,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 49, "T", Some("^")),
            ("value", 68, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn class_name_parameter_loose() {
    assert_declarations(
        "class T { field!: string; } function identity<T>(value: T) { return value; }",
        false,
        "es2020",
        false,
        &[
            ("T", 6, "T", Some("^")),
            ("field", 10, "string", Some("^^^^^^")),
            (
                "identity",
                37,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 49, "T", Some("^")),
            ("value", 68, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn local_library_interface_strict() {
    assert_declarations("export {}; interface Uint8Array<T = string> { value: T; } declare const item: Uint8Array<number>; const value = item.value;", true, "es2020", false, &[
        ("value", 46, "T", Some("^")),
        ("item", 72, "Uint8Array<number>", Some("^^^^^^^^^^^^^^^^^^")),
        ("value", 104, "number", Some("^^^^^^")),
        ("item.value", 112, "number", Some("^^^^^^")),
        ("item", 112, "Uint8Array<number>", Some("^^^^^^^^^^^^^^^^^^")),
        ("value", 117, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn local_library_interface_loose() {
    assert_declarations("export {}; interface Uint8Array<T = string> { value: T; } declare const item: Uint8Array<number>; const value = item.value;", false, "es2020", false, &[
        ("value", 46, "T", Some("^")),
        ("item", 72, "Uint8Array<number>", Some("^^^^^^^^^^^^^^^^^^")),
        ("value", 104, "number", Some("^^^^^^")),
        ("item.value", 112, "number", Some("^^^^^^")),
        ("item", 112, "Uint8Array<number>", Some("^^^^^^^^^^^^^^^^^^")),
        ("value", 117, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn constrained_library_parameter_strict() {
    assert_declarations("function identity<Uint8Array extends { value: string }>(item: Uint8Array) { return item.value; }", true, "es2020", false, &[
        ("identity", 9, "<Uint8Array extends { value: string; }>(item: Uint8Array) => string", Some("^          ^^^^^^^^^                  ^^    ^^          ^^^^^^^^^^^")),
        ("value", 39, "string", Some("^^^^^^")),
        ("item", 56, "Uint8Array", Some("^^^^^^^^^^")),
        ("item.value", 83, "string", Some("^^^^^^")),
        ("item", 83, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 88, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn constrained_library_parameter_loose() {
    assert_declarations("function identity<Uint8Array extends { value: string }>(item: Uint8Array) { return item.value; }", false, "es2020", false, &[
        ("identity", 9, "<Uint8Array extends { value: string; }>(item: Uint8Array) => string", Some("^          ^^^^^^^^^                  ^^    ^^          ^^^^^^^^^^^")),
        ("value", 39, "string", Some("^^^^^^")),
        ("item", 56, "Uint8Array", Some("^^^^^^^^^^")),
        ("item.value", 83, "string", Some("^^^^^^")),
        ("item", 83, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 88, "string", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn library_access_after_generic_strict() {
    assert_declarations("function identity<Uint8Array>(value: Uint8Array) { return value; } declare const buffer: Uint8Array; const length = buffer.length;", true, "es2020", false, &[
        ("identity", 9, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 30, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 58, "Uint8Array", Some("^^^^^^^^^^")),
        ("buffer", 81, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 107, "number", Some("^^^^^^")),
        ("buffer.length", 116, "number", Some("^^^^^^")),
        ("buffer", 116, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 123, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn library_access_after_generic_loose() {
    assert_declarations("function identity<Uint8Array>(value: Uint8Array) { return value; } declare const buffer: Uint8Array; const length = buffer.length;", false, "es2020", false, &[
        ("identity", 9, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 30, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 58, "Uint8Array", Some("^^^^^^^^^^")),
        ("buffer", 81, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 107, "number", Some("^^^^^^")),
        ("buffer.length", 116, "number", Some("^^^^^^")),
        ("buffer", 116, "Uint8Array<ArrayBufferLike>", Some("^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("length", 123, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn returned_function_parameter_strict() {
    assert_declarations(
        "function outer() { return function inner(value: number) { return value; }; }",
        true,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "() => (value: number) => number",
                Some("^^^^^^^             ^^^^^^^^^^^"),
            ),
            (
                "function inner(value: number) { return value; }",
                26,
                "(value: number) => number",
                Some("^     ^^      ^^^^^^^^^^^"),
            ),
            (
                "inner",
                35,
                "(value: number) => number",
                Some("^     ^^      ^^^^^^^^^^^"),
            ),
            ("value", 41, "number", Some("^^^^^^")),
            ("value", 65, "number", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn returned_function_parameter_loose() {
    assert_declarations(
        "function outer() { return function inner(value: number) { return value; }; }",
        false,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "() => (value: number) => number",
                Some("^^^^^^^             ^^^^^^^^^^^"),
            ),
            (
                "function inner(value: number) { return value; }",
                26,
                "(value: number) => number",
                Some("^     ^^      ^^^^^^^^^^^"),
            ),
            (
                "inner",
                35,
                "(value: number) => number",
                Some("^     ^^      ^^^^^^^^^^^"),
            ),
            ("value", 41, "number", Some("^^^^^^")),
            ("value", 65, "number", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn returned_optional_parameter_strict() {
    assert_declarations(
        "function outer() { return function inner(value?: number) { return value; }; }",
        true,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "() => (value?: number) => number | undefined",
                Some("^^^^^^^     ^        ^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "function inner(value?: number) { return value; }",
                26,
                "(value?: number) => number | undefined",
                Some("^     ^^^      ^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "inner",
                35,
                "(value?: number) => number | undefined",
                Some("^     ^^^      ^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "value",
                41,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "value",
                66,
                "number | undefined",
                Some("^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn returned_optional_parameter_loose() {
    assert_declarations(
        "function outer() { return function inner(value?: number) { return value; }; }",
        false,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "() => (value?: number) => number",
                Some("^^^^^^^     ^        ^^^^^^^^^^^"),
            ),
            (
                "function inner(value?: number) { return value; }",
                26,
                "(value?: number) => number",
                Some("^     ^^^      ^^^^^^^^^^^"),
            ),
            (
                "inner",
                35,
                "(value?: number) => number",
                Some("^     ^^^      ^^^^^^^^^^^"),
            ),
            ("value", 41, "number", Some("^^^^^^")),
            ("value", 66, "number", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn returned_rest_parameter_strict() {
    assert_declarations(
        "function outer() { return function inner(...values: number[]) { return values; }; }",
        true,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "() => (...values: number[]) => number[]",
                Some("^^^^^^^                   ^^^^^^^^^^^^^"),
            ),
            (
                "function inner(...values: number[]) { return values; }",
                26,
                "(...values: number[]) => number[]",
                Some("^^^^      ^^        ^^^^^^^^^^^^^"),
            ),
            (
                "inner",
                35,
                "(...values: number[]) => number[]",
                Some("^^^^      ^^        ^^^^^^^^^^^^^"),
            ),
            ("values", 44, "number[]", Some("^^^^^^^^")),
            ("values", 71, "number[]", Some("^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn returned_rest_parameter_loose() {
    assert_declarations(
        "function outer() { return function inner(...values: number[]) { return values; }; }",
        false,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "() => (...values: number[]) => number[]",
                Some("^^^^^^^                   ^^^^^^^^^^^^^"),
            ),
            (
                "function inner(...values: number[]) { return values; }",
                26,
                "(...values: number[]) => number[]",
                Some("^^^^      ^^        ^^^^^^^^^^^^^"),
            ),
            (
                "inner",
                35,
                "(...values: number[]) => number[]",
                Some("^^^^      ^^        ^^^^^^^^^^^^^"),
            ),
            ("values", 44, "number[]", Some("^^^^^^^^")),
            ("values", 71, "number[]", Some("^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_parameter_strict() {
    assert_declarations("namespace N { export type T = string; export function identity<T>(value: T) { return value; } }", true, "es2020", false, &[
        ("N", 10, "typeof N", Some("^^^^^^^^")),
        ("T", 26, "string", Some("^^^^^^")),
        ("identity", 54, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 66, "T", Some("^")),
        ("value", 85, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn namespace_parameter_loose() {
    assert_declarations("namespace N { export type T = string; export function identity<T>(value: T) { return value; } }", false, "es2020", false, &[
        ("N", 10, "typeof N", Some("^^^^^^^^")),
        ("T", 26, "string", Some("^^^^^^")),
        ("identity", 54, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 66, "T", Some("^")),
        ("value", 85, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn interface_call_parameter_strict() {
    assert_declarations(
        "interface Callable { <Uint8Array>(value: Uint8Array): Uint8Array; }",
        true,
        "es2020",
        false,
        &[("value", 34, "Uint8Array", Some("^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn interface_call_parameter_loose() {
    assert_declarations(
        "interface Callable { <Uint8Array>(value: Uint8Array): Uint8Array; }",
        false,
        "es2020",
        false,
        &[("value", 34, "Uint8Array", Some("^^^^^^^^^^"))],
        &[],
    );
}

#[test]
fn interface_construct_parameter_strict() {
    assert_declarations(
        "interface Constructor { new<Uint8Array>(value: Uint8Array): { value: Uint8Array }; }",
        true,
        "es2020",
        false,
        &[
            ("value", 40, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 62, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn interface_construct_parameter_loose() {
    assert_declarations(
        "interface Constructor { new<Uint8Array>(value: Uint8Array): { value: Uint8Array }; }",
        false,
        "es2020",
        false,
        &[
            ("value", 40, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 62, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn constrained_alias_parameter_strict() {
    assert_declarations("type T = string; function identity<T extends { value: number }>(item: T) { return item.value; }", true, "es2020", false, &[
        ("T", 5, "string", Some("^^^^^^")),
        ("identity", 26, "<T extends { value: number; }>(item: T) => number", Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^^^^^^")),
        ("value", 47, "number", Some("^^^^^^")),
        ("item", 64, "T", Some("^")),
        ("item.value", 82, "number", Some("^^^^^^")),
        ("item", 82, "T", Some("^")),
        ("value", 87, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn constrained_alias_parameter_loose() {
    assert_declarations("type T = string; function identity<T extends { value: number }>(item: T) { return item.value; }", false, "es2020", false, &[
        ("T", 5, "string", Some("^^^^^^")),
        ("identity", 26, "<T extends { value: number; }>(item: T) => number", Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^^^^^^")),
        ("value", 47, "number", Some("^^^^^^")),
        ("item", 64, "T", Some("^")),
        ("item.value", 82, "number", Some("^^^^^^")),
        ("item", 82, "T", Some("^")),
        ("value", 87, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn defaulted_library_parameter_strict() {
    assert_declarations(
        "function identity<Uint8Array = string>(value: Uint8Array) { return value; }",
        true,
        "es2020",
        false,
        &[
            (
                "identity",
                9,
                "<Uint8Array = string>(value: Uint8Array) => Uint8Array",
                Some("^          ^^^^^^^^^^^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 39, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 67, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn defaulted_library_parameter_loose() {
    assert_declarations(
        "function identity<Uint8Array = string>(value: Uint8Array) { return value; }",
        false,
        "es2020",
        false,
        &[
            (
                "identity",
                9,
                "<Uint8Array = string>(value: Uint8Array) => Uint8Array",
                Some("^          ^^^^^^^^^^^     ^^          ^^^^^^^^^^^^^^^"),
            ),
            ("value", 39, "Uint8Array", Some("^^^^^^^^^^")),
            ("value", 67, "Uint8Array", Some("^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn nested_unconstrained_shadow_strict() {
    assert_declarations("function outer<Uint8Array extends { value: number }>(item: Uint8Array) { return function inner<Uint8Array>(value: Uint8Array) { return value; }; }", true, "es2020", false, &[
        ("outer", 9, "<Uint8Array extends { value: number; }>(item: Uint8Array) => <Uint8Array_1>(value: Uint8Array_1) => Uint8Array_1", Some("^          ^^^^^^^^^                  ^^    ^^          ^^^^^^            ^^                   ^^^^^^^^^^^^^^^^^")),
        ("value", 36, "number", Some("^^^^^^")),
        ("item", 53, "Uint8Array", Some("^^^^^^^^^^")),
        ("function inner<Uint8Array>(value: Uint8Array) { return value; }", 80, "<Uint8Array_1>(value: Uint8Array_1) => Uint8Array_1", Some("^^^^^^^^^^^^^^^     ^^            ^^^^^^^^^^^^^^^^^")),
        ("inner", 89, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 107, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 135, "Uint8Array", Some("^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn nested_unconstrained_shadow_loose() {
    assert_declarations("function outer<Uint8Array extends { value: number }>(item: Uint8Array) { return function inner<Uint8Array>(value: Uint8Array) { return value; }; }", false, "es2020", false, &[
        ("outer", 9, "<Uint8Array extends { value: number; }>(item: Uint8Array) => <Uint8Array_1>(value: Uint8Array_1) => Uint8Array_1", Some("^          ^^^^^^^^^                  ^^    ^^          ^^^^^^            ^^                   ^^^^^^^^^^^^^^^^^")),
        ("value", 36, "number", Some("^^^^^^")),
        ("item", 53, "Uint8Array", Some("^^^^^^^^^^")),
        ("function inner<Uint8Array>(value: Uint8Array) { return value; }", 80, "<Uint8Array_1>(value: Uint8Array_1) => Uint8Array_1", Some("^^^^^^^^^^^^^^^     ^^            ^^^^^^^^^^^^^^^^^")),
        ("inner", 89, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 107, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 135, "Uint8Array", Some("^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn class_shadowed_constraint_strict() {
    assert_declarations("class Box<Uint8Array extends { value: number }> { method<Uint8Array>(value: Uint8Array) { return value; } }", true, "es2020", false, &[
        ("Box", 6, "Box<Uint8Array>", Some("^^^^^^^^^^^^^^^")),
        ("value", 31, "number", Some("^^^^^^")),
        ("method", 50, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 69, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 97, "Uint8Array", Some("^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn class_shadowed_constraint_loose() {
    assert_declarations("class Box<Uint8Array extends { value: number }> { method<Uint8Array>(value: Uint8Array) { return value; } }", false, "es2020", false, &[
        ("Box", 6, "Box<Uint8Array>", Some("^^^^^^^^^^^^^^^")),
        ("value", 31, "number", Some("^^^^^^")),
        ("method", 50, "<Uint8Array>(value: Uint8Array) => Uint8Array", Some("^          ^^     ^^          ^^^^^^^^^^^^^^^")),
        ("value", 69, "Uint8Array", Some("^^^^^^^^^^")),
        ("value", 97, "Uint8Array", Some("^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn captured_outer_parameter_strict() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: T) { return [item, value] as const; }; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: T_1) => readonly [T, T_1]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: T) { return [item, value] as const; }", 36, "<T_1>(value: T_1) => readonly [T, T_1]", Some("^^^^^^     ^^   ^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 45, "<T>(value: T) => readonly [T_1, T]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 54, "T", Some("^")),
        ("[item, value] as const", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("[item, value]", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("item", 74, "T_1", Some("^^^")),
        ("value", 80, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn captured_outer_parameter_loose() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: T) { return [item, value] as const; }; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: T_1) => readonly [T, T_1]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: T) { return [item, value] as const; }", 36, "<T_1>(value: T_1) => readonly [T, T_1]", Some("^^^^^^     ^^   ^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 45, "<T>(value: T) => readonly [T_1, T]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 54, "T", Some("^")),
        ("[item, value] as const", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("[item, value]", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("item", 74, "T_1", Some("^^^")),
        ("value", 80, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn captured_outer_instantiation_strict() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: T) { return [item, value] as const; }; } const inner = outer(1); const pair = inner(\"text\");", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: T_1) => readonly [T, T_1]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: T) { return [item, value] as const; }", 36, "<T_1>(value: T_1) => readonly [T, T_1]", Some("^^^^^^     ^^   ^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 45, "<T>(value: T) => readonly [T_1, T]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 54, "T", Some("^")),
        ("[item, value] as const", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("[item, value]", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("item", 74, "T_1", Some("^^^")),
        ("value", 80, "T", Some("^")),
        ("inner", 108, "<T>(value: T) => readonly [number, T]", Some("^ ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("outer(1)", 116, "<T>(value: T) => readonly [number, T]", Some("^ ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("outer", 116, "<T>(item: T) => <T_1>(value: T_1) => readonly [T, T_1]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^^^^^^^^^^^^^^^^^")),
        ("1", 122, "1", Some("^")),
        ("pair", 132, "readonly [number, string]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner(\"text\")", 139, "readonly [number, string]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 139, "<T>(value: T) => readonly [number, T]", Some("^ ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("\"text\"", 145, "\"text\"", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn captured_outer_instantiation_loose() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: T) { return [item, value] as const; }; } const inner = outer(1); const pair = inner(\"text\");", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: T_1) => readonly [T, T_1]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: T) { return [item, value] as const; }", 36, "<T_1>(value: T_1) => readonly [T, T_1]", Some("^^^^^^     ^^   ^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 45, "<T>(value: T) => readonly [T_1, T]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 54, "T", Some("^")),
        ("[item, value] as const", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("[item, value]", 73, "readonly [T_1, T]", Some("^^^^^^^^^^^^^^^^^")),
        ("item", 74, "T_1", Some("^^^")),
        ("value", 80, "T", Some("^")),
        ("inner", 108, "<T>(value: T) => readonly [number, T]", Some("^ ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("outer(1)", 116, "<T>(value: T) => readonly [number, T]", Some("^ ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("outer", 116, "<T>(item: T) => <T_1>(value: T_1) => readonly [T, T_1]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^^^^^^^^^^^^^^^^^")),
        ("1", 122, "1", Some("^")),
        ("pair", 132, "readonly [number, string]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner(\"text\")", 139, "readonly [number, string]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 139, "<T>(value: T) => readonly [number, T]", Some("^ ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("\"text\"", 145, "\"text\"", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn nested_constrained_parameter_strict() {
    assert_declarations("function outer<T extends { first: string }>(item: T) { return function inner<T extends { second: number }>(value: T) { return [item.first, value.second] as const; }; }", true, "es2020", false, &[
        ("outer", 9, "<T extends { first: string; }>(item: T) => <T_1 extends { second: number; }>(value: T_1) => readonly [string, number]", Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^                               ^^          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("first", 27, "string", Some("^^^^^^")),
        ("item", 44, "T", Some("^")),
        ("function inner<T extends { second: number }>(value: T) { return [item.first, value.second] as const; }", 62, "<T_1 extends { second: number; }>(value: T_1) => readonly [string, number]", Some("^^^^^^^^^^^^^                   ^^     ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 71, "<T extends { second: number; }>(value: T) => readonly [string, number]", Some("^ ^^^^^^^^^                   ^^     ^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("second", 89, "number", Some("^^^^^^")),
        ("value", 107, "T", Some("^")),
        ("[item.first, value.second] as const", 126, "readonly [string, number]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("[item.first, value.second]", 126, "readonly [string, number]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("item.first", 127, "string", Some("^^^^^^")),
        ("item", 127, "T_1", Some("^^^")),
        ("first", 132, "string", Some("^^^^^^")),
        ("value.second", 139, "number", Some("^^^^^^")),
        ("value", 139, "T", Some("^")),
        ("second", 145, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn nested_constrained_parameter_loose() {
    assert_declarations("function outer<T extends { first: string }>(item: T) { return function inner<T extends { second: number }>(value: T) { return [item.first, value.second] as const; }; }", false, "es2020", false, &[
        ("outer", 9, "<T extends { first: string; }>(item: T) => <T_1 extends { second: number; }>(value: T_1) => readonly [string, number]", Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^                               ^^          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("first", 27, "string", Some("^^^^^^")),
        ("item", 44, "T", Some("^")),
        ("function inner<T extends { second: number }>(value: T) { return [item.first, value.second] as const; }", 62, "<T_1 extends { second: number; }>(value: T_1) => readonly [string, number]", Some("^^^^^^^^^^^^^                   ^^     ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 71, "<T extends { second: number; }>(value: T) => readonly [string, number]", Some("^ ^^^^^^^^^                   ^^     ^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("second", 89, "number", Some("^^^^^^")),
        ("value", 107, "T", Some("^")),
        ("[item.first, value.second] as const", 126, "readonly [string, number]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("[item.first, value.second]", 126, "readonly [string, number]", Some("^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("item.first", 127, "string", Some("^^^^^^")),
        ("item", 127, "T_1", Some("^^^")),
        ("first", 132, "string", Some("^^^^^^")),
        ("value.second", 139, "number", Some("^^^^^^")),
        ("value", 139, "T", Some("^")),
        ("second", 145, "number", Some("^^^^^^")),
    ], &[
    ]);
}

#[test]
fn nested_annotated_parameter_strict() {
    assert_declarations(
        "function outer<T>(item: T) { return function inner<T>(value: T): T { return value; }; }",
        true,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "<T>(item: T) => <T_1>(value: T_1) => T_1",
                Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^   "),
            ),
            ("item", 18, "T", Some("^")),
            (
                "function inner<T>(value: T): T { return value; }",
                36,
                "<T_1>(value: T_1) => T_1",
                Some("^^^^^^     ^^   ^^^^^   "),
            ),
            (
                "inner",
                45,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            ("value", 54, "T", Some("^")),
            ("value", 76, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn nested_annotated_parameter_loose() {
    assert_declarations(
        "function outer<T>(item: T) { return function inner<T>(value: T): T { return value; }; }",
        false,
        "es2020",
        false,
        &[
            (
                "outer",
                9,
                "<T>(item: T) => <T_1>(value: T_1) => T_1",
                Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^   "),
            ),
            ("item", 18, "T", Some("^")),
            (
                "function inner<T>(value: T): T { return value; }",
                36,
                "<T_1>(value: T_1) => T_1",
                Some("^^^^^^     ^^   ^^^^^   "),
            ),
            (
                "inner",
                45,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            ("value", 54, "T", Some("^")),
            ("value", 76, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn nested_default_parameter_strict() {
    assert_declarations("function outer<T>(item: T) { return function inner<T = string>(value: T) { return value; }; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1 = string>(value: T_1) => T_1", Some("^ ^^    ^^ ^^^^^^            ^^          ^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T = string>(value: T) { return value; }", 36, "<T_1 = string>(value: T_1) => T_1", Some("^^^^^^^^^^^^^^^     ^^   ^^^^^^^^")),
        ("inner", 45, "<T = string>(value: T) => T", Some("^ ^^^^^^^^^^^     ^^ ^^^^^^")),
        ("value", 63, "T", Some("^")),
        ("value", 82, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn nested_default_parameter_loose() {
    assert_declarations("function outer<T>(item: T) { return function inner<T = string>(value: T) { return value; }; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1 = string>(value: T_1) => T_1", Some("^ ^^    ^^ ^^^^^^            ^^          ^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T = string>(value: T) { return value; }", 36, "<T_1 = string>(value: T_1) => T_1", Some("^^^^^^^^^^^^^^^     ^^   ^^^^^^^^")),
        ("inner", 45, "<T = string>(value: T) => T", Some("^ ^^^^^^^^^^^     ^^ ^^^^^^")),
        ("value", 63, "T", Some("^")),
        ("value", 82, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn nested_object_annotation_strict() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: { nested: T }) { return value; }; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: { nested: T_1; }) => { nested: T_1; }", Some("^ ^^    ^^ ^^^^^^   ^^                       ^^^^^^^^^^^^^^^   ^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: { nested: T }) { return value; }", 36, "<T_1>(value: { nested: T_1; }) => { nested: T_1; }", Some("^^^^^^     ^^                ^^^^^^^^^^^^^^^   ^^^")),
        ("inner", 45, "<T>(value: { nested: T; }) => { nested: T; }", Some("^ ^^     ^^              ^^^^^^^^^^^^^^^ ^^^")),
        ("value", 54, "{ nested: T; }", Some("^^^^^^^^^^ ^^^")),
        ("nested", 63, "T", Some("^")),
        ("value", 85, "{ nested: T; }", Some("^^^^^^^^^^ ^^^")),
    ], &[
    ]);
}

#[test]
fn nested_object_annotation_loose() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: { nested: T }) { return value; }; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: { nested: T_1; }) => { nested: T_1; }", Some("^ ^^    ^^ ^^^^^^   ^^                       ^^^^^^^^^^^^^^^   ^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: { nested: T }) { return value; }", 36, "<T_1>(value: { nested: T_1; }) => { nested: T_1; }", Some("^^^^^^     ^^                ^^^^^^^^^^^^^^^   ^^^")),
        ("inner", 45, "<T>(value: { nested: T; }) => { nested: T; }", Some("^ ^^     ^^              ^^^^^^^^^^^^^^^ ^^^")),
        ("value", 54, "{ nested: T; }", Some("^^^^^^^^^^ ^^^")),
        ("nested", 63, "T", Some("^")),
        ("value", 85, "{ nested: T; }", Some("^^^^^^^^^^ ^^^")),
    ], &[
    ]);
}

#[test]
fn separate_generic_returns_strict() {
    assert_declarations("function outer<T>(item: T) { return [function first<T>(value: T) { return value; }, function second<T>(value: T) { return value; }] as const; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => readonly [<T_1>(value: T_1) => T_1, <T_1>(value: T_1) => T_1]", Some("^ ^^    ^^ ^^^^^^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("[function first<T>(value: T) { return value; }, function second<T>(value: T) { return value; }] as const", 36, "readonly [<T_1>(value: T_1) => T_1, <T_1>(value: T_1) => T_1]", Some("^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^")),
        ("[function first<T>(value: T) { return value; }, function second<T>(value: T) { return value; }]", 36, "readonly [<T_1>(value: T_1) => T_1, <T_1>(value: T_1) => T_1]", Some("^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^")),
        ("function first<T>(value: T) { return value; }", 37, "<T_1>(value: T_1) => T_1", Some("^^^^^^     ^^   ^^^^^^^^")),
        ("first", 46, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 55, "T", Some("^")),
        ("value", 74, "T", Some("^")),
        ("function second<T>(value: T) { return value; }", 84, "<T_1>(value: T_1) => T_1", Some("^^^^^^     ^^   ^^^^^^^^")),
        ("second", 93, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 103, "T", Some("^")),
        ("value", 122, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn separate_generic_returns_loose() {
    assert_declarations("function outer<T>(item: T) { return [function first<T>(value: T) { return value; }, function second<T>(value: T) { return value; }] as const; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => readonly [<T_1>(value: T_1) => T_1, <T_1>(value: T_1) => T_1]", Some("^ ^^    ^^ ^^^^^^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("[function first<T>(value: T) { return value; }, function second<T>(value: T) { return value; }] as const", 36, "readonly [<T_1>(value: T_1) => T_1, <T_1>(value: T_1) => T_1]", Some("^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^")),
        ("[function first<T>(value: T) { return value; }, function second<T>(value: T) { return value; }]", 36, "readonly [<T_1>(value: T_1) => T_1, <T_1>(value: T_1) => T_1]", Some("^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^^^^^^^^     ^^   ^^^^^^^^^")),
        ("function first<T>(value: T) { return value; }", 37, "<T_1>(value: T_1) => T_1", Some("^^^^^^     ^^   ^^^^^^^^")),
        ("first", 46, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 55, "T", Some("^")),
        ("value", 74, "T", Some("^")),
        ("function second<T>(value: T) { return value; }", 84, "<T_1>(value: T_1) => T_1", Some("^^^^^^     ^^   ^^^^^^^^")),
        ("second", 93, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 103, "T", Some("^")),
        ("value", 122, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn nested_distinct_parameters_strict() {
    assert_declarations("function outer<T>(item: T) { return function inner<U>(value: U) { return [item, value] as const; }; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <U>(value: U) => readonly [T, U]", Some("^ ^^    ^^ ^^^^^^ ^^        ^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<U>(value: U) { return [item, value] as const; }", 36, "<U>(value: U) => readonly [T, U]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 45, "<U>(value: U) => readonly [T, U]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^")),
        ("value", 54, "U", Some("^")),
        ("[item, value] as const", 73, "readonly [T, U]", Some("^^^^^^^^^^^^^^^")),
        ("[item, value]", 73, "readonly [T, U]", Some("^^^^^^^^^^^^^^^")),
        ("item", 74, "T", Some("^")),
        ("value", 80, "U", Some("^")),
    ], &[
    ]);
}

#[test]
fn nested_distinct_parameters_loose() {
    assert_declarations("function outer<T>(item: T) { return function inner<U>(value: U) { return [item, value] as const; }; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <U>(value: U) => readonly [T, U]", Some("^ ^^    ^^ ^^^^^^ ^^        ^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<U>(value: U) { return [item, value] as const; }", 36, "<U>(value: U) => readonly [T, U]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 45, "<U>(value: U) => readonly [T, U]", Some("^ ^^     ^^ ^^^^^^^^^^^^^^^^^^^^")),
        ("value", 54, "U", Some("^")),
        ("[item, value] as const", 73, "readonly [T, U]", Some("^^^^^^^^^^^^^^^")),
        ("[item, value]", 73, "readonly [T, U]", Some("^^^^^^^^^^^^^^^")),
        ("item", 74, "T", Some("^")),
        ("value", 80, "U", Some("^")),
    ], &[
    ]);
}

#[test]
fn standalone_generic_expression_strict() {
    assert_declarations(
        "const identity = function<T>(value: T) { return value; }; const value = identity(1);",
        true,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            (
                "function<T>(value: T) { return value; }",
                17,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 29, "T", Some("^")),
            ("value", 48, "T", Some("^")),
            ("value", 64, "1", Some("^")),
            ("identity(1)", 72, "1", Some("^")),
            (
                "identity",
                72,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("1", 81, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn standalone_generic_expression_loose() {
    assert_declarations(
        "const identity = function<T>(value: T) { return value; }; const value = identity(1);",
        false,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            (
                "function<T>(value: T) { return value; }",
                17,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("value", 29, "T", Some("^")),
            ("value", 48, "T", Some("^")),
            ("value", 64, "1", Some("^")),
            ("identity(1)", 72, "1", Some("^")),
            (
                "identity",
                72,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^^"),
            ),
            ("1", 81, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn expression_generic_assignment_strict() {
    assert_declarations(
        "const identity = function<T>(value: T): T { const result: T = value; return result; };",
        true,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            (
                "function<T>(value: T): T { const result: T = value; return result; }",
                17,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            ("value", 29, "T", Some("^")),
            ("result", 50, "T", Some("^")),
            ("value", 62, "T", Some("^")),
            ("result", 76, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn expression_generic_assignment_loose() {
    assert_declarations(
        "const identity = function<T>(value: T): T { const result: T = value; return result; };",
        false,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            (
                "function<T>(value: T): T { const result: T = value; return result; }",
                17,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            ("value", 29, "T", Some("^")),
            ("result", 50, "T", Some("^")),
            ("value", 62, "T", Some("^")),
            ("result", 76, "T", Some("^")),
        ],
        &[],
    );
}

#[test]
fn constrained_generic_expression_strict() {
    assert_declarations(
        "const value = function<T extends { value: number }>(item: T) { return item.value; };",
        true,
        "es2020",
        false,
        &[
            (
                "value",
                6,
                "<T extends { value: number; }>(item: T) => number",
                Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^^^^^^"),
            ),
            (
                "function<T extends { value: number }>(item: T) { return item.value; }",
                14,
                "<T extends { value: number; }>(item: T) => number",
                Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^^^^^^"),
            ),
            ("value", 35, "number", Some("^^^^^^")),
            ("item", 52, "T", Some("^")),
            ("item.value", 70, "number", Some("^^^^^^")),
            ("item", 70, "T", Some("^")),
            ("value", 75, "number", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn constrained_generic_expression_loose() {
    assert_declarations(
        "const value = function<T extends { value: number }>(item: T) { return item.value; };",
        false,
        "es2020",
        false,
        &[
            (
                "value",
                6,
                "<T extends { value: number; }>(item: T) => number",
                Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^^^^^^"),
            ),
            (
                "function<T extends { value: number }>(item: T) { return item.value; }",
                14,
                "<T extends { value: number; }>(item: T) => number",
                Some("^ ^^^^^^^^^                  ^^    ^^ ^^^^^^^^^^^"),
            ),
            ("value", 35, "number", Some("^^^^^^")),
            ("item", 52, "T", Some("^")),
            ("item.value", 70, "number", Some("^^^^^^")),
            ("item", 70, "T", Some("^")),
            ("value", 75, "number", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn three_nested_parameters_strict() {
    assert_declarations("function outer<T>(item: T) { return function middle<T>(value: T) { return function inner<T>(last: T) { return [item, value, last] as const; }; }; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: T_1) => <T_2>(last: T_2) => readonly [T, T_1, T_2]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^   ^^         ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function middle<T>(value: T) { return function inner<T>(last: T) { return [item, value, last] as const; }; }", 36, "<T_1>(value: T_1) => <T_2>(last: T_2) => readonly [T, T_1, T_2]", Some("^^^^^^     ^^   ^^^^^^   ^^         ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("middle", 45, "<T>(value: T) => <T_1>(last: T_1) => readonly [T_2, T, T_1]", Some("^ ^^     ^^ ^^^^^^   ^^         ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 55, "T", Some("^")),
        ("function inner<T>(last: T) { return [item, value, last] as const; }", 74, "<T_1>(last: T_1) => readonly [T_2, T, T_1]", Some("^^^^^^    ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 83, "<T>(last: T) => readonly [T_1, T_2, T]", Some("^ ^^    ^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("last", 92, "T", Some("^")),
        ("[item, value, last] as const", 110, "readonly [T_1, T_2, T]", Some("^^^^^^^^^^^^^^^^^^^^^^")),
        ("[item, value, last]", 110, "readonly [T_1, T_2, T]", Some("^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 111, "T_1", Some("^^^")),
        ("value", 117, "T_1", Some("^^^")),
        ("last", 124, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn three_nested_parameters_loose() {
    assert_declarations("function outer<T>(item: T) { return function middle<T>(value: T) { return function inner<T>(last: T) { return [item, value, last] as const; }; }; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: T_1) => <T_2>(last: T_2) => readonly [T, T_1, T_2]", Some("^ ^^    ^^ ^^^^^^   ^^          ^^^^^^   ^^         ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 18, "T", Some("^")),
        ("function middle<T>(value: T) { return function inner<T>(last: T) { return [item, value, last] as const; }; }", 36, "<T_1>(value: T_1) => <T_2>(last: T_2) => readonly [T, T_1, T_2]", Some("^^^^^^     ^^   ^^^^^^   ^^         ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("middle", 45, "<T>(value: T) => <T_1>(last: T_1) => readonly [T_2, T, T_1]", Some("^ ^^     ^^ ^^^^^^   ^^         ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 55, "T", Some("^")),
        ("function inner<T>(last: T) { return [item, value, last] as const; }", 74, "<T_1>(last: T_1) => readonly [T_2, T, T_1]", Some("^^^^^^    ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 83, "<T>(last: T) => readonly [T_1, T_2, T]", Some("^ ^^    ^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("last", 92, "T", Some("^")),
        ("[item, value, last] as const", 110, "readonly [T_1, T_2, T]", Some("^^^^^^^^^^^^^^^^^^^^^^")),
        ("[item, value, last]", 110, "readonly [T_1, T_2, T]", Some("^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 111, "T_1", Some("^^^")),
        ("value", 117, "T_1", Some("^^^")),
        ("last", 124, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn generic_expression_invalid_return_strict() {
    assert_declarations(
        "const identity = function<T>(value: T): T { return 1; };",
        true,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            (
                "function<T>(value: T): T { return 1; }",
                17,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            ("value", 29, "T", Some("^")),
            ("1", 51, "1", Some("^")),
        ],
        &[(2322, 44, 6)],
    );
}

#[test]
fn generic_expression_invalid_return_loose() {
    assert_declarations(
        "const identity = function<T>(value: T): T { return 1; };",
        false,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            (
                "function<T>(value: T): T { return 1; }",
                17,
                "<T>(value: T) => T",
                Some("^ ^^     ^^ ^^^^^ "),
            ),
            ("value", 29, "T", Some("^")),
            ("1", 51, "1", Some("^")),
        ],
        &[(2322, 44, 6)],
    );
}

#[test]
fn nested_function_declaration_shadow_strict() {
    assert_declarations("const outer = function<T>(value: T) { function inner<T>(item: T) { return item; } return inner(value); };", true, "es2020", false, &[
        ("outer", 6, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("function<T>(value: T) { function inner<T>(item: T) { return item; } return inner(value); }", 14, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 26, "T", Some("^")),
        ("inner", 47, "<T>(item: T) => T", Some("^ ^^    ^^ ^^^^^^")),
        ("item", 56, "T", Some("^")),
        ("item", 74, "T", Some("^")),
        ("inner(value)", 89, "T", Some("^")),
        ("inner", 89, "<T_1>(item: T_1) => T_1", Some("^^^^^^    ^^   ^^^^^^^^")),
        ("value", 95, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn nested_function_declaration_shadow_loose() {
    assert_declarations("const outer = function<T>(value: T) { function inner<T>(item: T) { return item; } return inner(value); };", false, "es2020", false, &[
        ("outer", 6, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("function<T>(value: T) { function inner<T>(item: T) { return item; } return inner(value); }", 14, "<T>(value: T) => T", Some("^ ^^     ^^ ^^^^^^")),
        ("value", 26, "T", Some("^")),
        ("inner", 47, "<T>(item: T) => T", Some("^ ^^    ^^ ^^^^^^")),
        ("item", 56, "T", Some("^")),
        ("item", 74, "T", Some("^")),
        ("inner(value)", 89, "T", Some("^")),
        ("inner", 89, "<T_1>(item: T_1) => T_1", Some("^^^^^^    ^^   ^^^^^^^^")),
        ("value", 95, "T", Some("^")),
    ], &[
    ]);
}

#[test]
fn renamed_parameter_property_name_strict() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: { T: T; label: \"T\" }) { return value; }; }", true, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: { T: T_1; label: \"T\"; }) => { T: T_1; label: \"T\"; }", Some("^ ^^    ^^ ^^^^^^   ^^                              ^^^^^^^^^^   ^^^^^^^^^   ^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: { T: T; label: \"T\" }) { return value; }", 36, "<T_1>(value: { T: T_1; label: \"T\"; }) => { T: T_1; label: \"T\"; }", Some("^^^^^^     ^^                       ^^^^^^^^^^   ^^^^^^^^^   ^^^")),
        ("inner", 45, "<T>(value: { T: T; label: \"T\"; }) => { T: T; label: \"T\"; }", Some("^ ^^     ^^                     ^^^^^^^^^^ ^^^^^^^^^   ^^^")),
        ("value", 54, "{ T: T; label: \"T\"; }", Some("^^^^^ ^^^^^^^^^   ^^^")),
        ("T", 63, "T", Some("^")),
        ("label", 69, "\"T\"", Some("^^^")),
        ("value", 92, "{ T: T; label: \"T\"; }", Some("^^^^^ ^^^^^^^^^   ^^^")),
    ], &[
    ]);
}

#[test]
fn renamed_parameter_property_name_loose() {
    assert_declarations("function outer<T>(item: T) { return function inner<T>(value: { T: T; label: \"T\" }) { return value; }; }", false, "es2020", false, &[
        ("outer", 9, "<T>(item: T) => <T_1>(value: { T: T_1; label: \"T\"; }) => { T: T_1; label: \"T\"; }", Some("^ ^^    ^^ ^^^^^^   ^^                              ^^^^^^^^^^   ^^^^^^^^^   ^^^")),
        ("item", 18, "T", Some("^")),
        ("function inner<T>(value: { T: T; label: \"T\" }) { return value; }", 36, "<T_1>(value: { T: T_1; label: \"T\"; }) => { T: T_1; label: \"T\"; }", Some("^^^^^^     ^^                       ^^^^^^^^^^   ^^^^^^^^^   ^^^")),
        ("inner", 45, "<T>(value: { T: T; label: \"T\"; }) => { T: T; label: \"T\"; }", Some("^ ^^     ^^                     ^^^^^^^^^^ ^^^^^^^^^   ^^^")),
        ("value", 54, "{ T: T; label: \"T\"; }", Some("^^^^^ ^^^^^^^^^   ^^^")),
        ("T", 63, "T", Some("^")),
        ("label", 69, "\"T\"", Some("^^^")),
        ("value", 92, "{ T: T; label: \"T\"; }", Some("^^^^^ ^^^^^^^^^   ^^^")),
    ], &[
    ]);
}

#[test]
fn unicode_shadowed_parameters_strict() {
    assert_declarations("function outer<𝒜>(item: 𝒜) { return function inner<𝒜>(value: 𝒜) { return [item, value] as const; }; }", true, "es2020", false, &[
        ("outer", 9, "<𝒜>(item: 𝒜) => <𝒜_1>(value: 𝒜_1) => readonly [𝒜, 𝒜_1]", Some("^  ^^    ^^  ^^^^^^    ^^           ^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 21, "𝒜", Some("^^")),
        ("function inner<𝒜>(value: 𝒜) { return [item, value] as const; }", 42, "<𝒜_1>(value: 𝒜_1) => readonly [𝒜, 𝒜_1]", Some("^^^^^^^     ^^    ^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 51, "<𝒜>(value: 𝒜) => readonly [𝒜_1, 𝒜]", Some("^  ^^     ^^  ^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 63, "𝒜", Some("^^")),
        ("[item, value] as const", 85, "readonly [𝒜_1, 𝒜]", Some("^^^^^^^^^^^^^^^^^^^")),
        ("[item, value]", 85, "readonly [𝒜_1, 𝒜]", Some("^^^^^^^^^^^^^^^^^^^")),
        ("item", 86, "𝒜_1", Some("^^^^")),
        ("value", 92, "𝒜", Some("^^")),
    ], &[
    ]);
}

#[test]
fn unicode_shadowed_parameters_loose() {
    assert_declarations("function outer<𝒜>(item: 𝒜) { return function inner<𝒜>(value: 𝒜) { return [item, value] as const; }; }", false, "es2020", false, &[
        ("outer", 9, "<𝒜>(item: 𝒜) => <𝒜_1>(value: 𝒜_1) => readonly [𝒜, 𝒜_1]", Some("^  ^^    ^^  ^^^^^^    ^^           ^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("item", 21, "𝒜", Some("^^")),
        ("function inner<𝒜>(value: 𝒜) { return [item, value] as const; }", 42, "<𝒜_1>(value: 𝒜_1) => readonly [𝒜, 𝒜_1]", Some("^^^^^^^     ^^    ^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("inner", 51, "<𝒜>(value: 𝒜) => readonly [𝒜_1, 𝒜]", Some("^  ^^     ^^  ^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("value", 63, "𝒜", Some("^^")),
        ("[item, value] as const", 85, "readonly [𝒜_1, 𝒜]", Some("^^^^^^^^^^^^^^^^^^^")),
        ("[item, value]", 85, "readonly [𝒜_1, 𝒜]", Some("^^^^^^^^^^^^^^^^^^^")),
        ("item", 86, "𝒜_1", Some("^^^^")),
        ("value", 92, "𝒜", Some("^^")),
    ], &[
    ]);
}

#[test]
fn readonly_generic_expression_strict() {
    assert_declarations("const identity = function<T>(item: { readonly value: T }) { item.value = item.value; return item; };", true, "es2020", false, &[
        ("identity", 6, "<T>(item: { readonly value: T; }) => { readonly value: T; }", Some("^ ^^    ^^                      ^^^^^^^^^^^^^^^^^^^^^^^ ^^^")),
        ("function<T>(item: { readonly value: T }) { item.value = item.value; return item; }", 17, "<T>(item: { readonly value: T; }) => { readonly value: T; }", Some("^ ^^    ^^                      ^^^^^^^^^^^^^^^^^^^^^^^ ^^^")),
        ("item", 29, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
        ("value", 46, "T", Some("^")),
        ("item.value = item.value", 60, "T", Some("^")),
        ("item.value", 60, "any", Some("^^^")),
        ("item", 60, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
        ("value", 65, "any", Some("^^^")),
        ("item.value", 73, "T", Some("^")),
        ("item", 73, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
        ("value", 78, "T", Some("^")),
        ("item", 92, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
    ], &[
        (2540, 65, 5),
    ]);
}

#[test]
fn readonly_generic_expression_loose() {
    assert_declarations("const identity = function<T>(item: { readonly value: T }) { item.value = item.value; return item; };", false, "es2020", false, &[
        ("identity", 6, "<T>(item: { readonly value: T; }) => { readonly value: T; }", Some("^ ^^    ^^                      ^^^^^^^^^^^^^^^^^^^^^^^ ^^^")),
        ("function<T>(item: { readonly value: T }) { item.value = item.value; return item; }", 17, "<T>(item: { readonly value: T; }) => { readonly value: T; }", Some("^ ^^    ^^                      ^^^^^^^^^^^^^^^^^^^^^^^ ^^^")),
        ("item", 29, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
        ("value", 46, "T", Some("^")),
        ("item.value = item.value", 60, "T", Some("^")),
        ("item.value", 60, "any", Some("^^^")),
        ("item", 60, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
        ("value", 65, "any", Some("^^^")),
        ("item.value", 73, "T", Some("^")),
        ("item", 73, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
        ("value", 78, "T", Some("^")),
        ("item", 92, "{ readonly value: T; }", Some("^^^^^^^^^^^^^^^^^^ ^^^")),
    ], &[
        (2540, 65, 5),
    ]);
}

#[test]
fn method_generic_expression_strict() {
    assert_declarations(
        "const identity = function<T>(item: { method(value: T): T }) { return item; };",
        true,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(item: { method(value: T): T; }) => { method(value: T): T; }",
                Some("^ ^^    ^^                        ^^^^^^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
            (
                "function<T>(item: { method(value: T): T }) { return item; }",
                17,
                "<T>(item: { method(value: T): T; }) => { method(value: T): T; }",
                Some("^ ^^    ^^                        ^^^^^^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
            (
                "item",
                29,
                "{ method(value: T): T; }",
                Some("^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
            ("method", 37, "(value: T) => T", Some("^     ^^ ^^^^^ ")),
            ("value", 44, "T", Some("^")),
            (
                "item",
                69,
                "{ method(value: T): T; }",
                Some("^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn method_generic_expression_loose() {
    assert_declarations(
        "const identity = function<T>(item: { method(value: T): T }) { return item; };",
        false,
        "es2020",
        false,
        &[
            (
                "identity",
                6,
                "<T>(item: { method(value: T): T; }) => { method(value: T): T; }",
                Some("^ ^^    ^^                        ^^^^^^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
            (
                "function<T>(item: { method(value: T): T }) { return item; }",
                17,
                "<T>(item: { method(value: T): T; }) => { method(value: T): T; }",
                Some("^ ^^    ^^                        ^^^^^^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
            (
                "item",
                29,
                "{ method(value: T): T; }",
                Some("^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
            ("method", 37, "(value: T) => T", Some("^     ^^ ^^^^^ ")),
            ("value", 44, "T", Some("^")),
            (
                "item",
                69,
                "{ method(value: T): T; }",
                Some("^^^^^^^^^     ^^ ^^^ ^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn intersection_mutable_property_strict() {
    assert_declarations("declare const item: { readonly value: number } & { value: number }; item.value = 1; item[\"value\"] = 2;", true, "es2020", false, &[
        ("item", 14, "{ readonly value: number; } & { value: number; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
        ("value", 31, "number", Some("^^^^^^")),
        ("value", 51, "number", Some("^^^^^^")),
        ("item.value = 1", 68, "1", Some("^")),
        ("item.value", 68, "number", Some("^^^^^^")),
        ("item", 68, "{ readonly value: number; } & { value: number; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
        ("value", 73, "number", Some("^^^^^^")),
        ("1", 81, "1", Some("^")),
        ("item[\"value\"] = 2", 84, "2", Some("^")),
        ("item[\"value\"]", 84, "number", Some("^^^^^^")),
        ("item", 84, "{ readonly value: number; } & { value: number; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
        ("\"value\"", 89, "\"value\"", Some("^^^^^^^")),
        ("2", 100, "2", Some("^")),
    ], &[
    ]);
}

#[test]
fn intersection_mutable_property_loose() {
    assert_declarations("declare const item: { readonly value: number } & { value: number }; item.value = 1; item[\"value\"] = 2;", false, "es2020", false, &[
        ("item", 14, "{ readonly value: number; } & { value: number; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
        ("value", 31, "number", Some("^^^^^^")),
        ("value", 51, "number", Some("^^^^^^")),
        ("item.value = 1", 68, "1", Some("^")),
        ("item.value", 68, "number", Some("^^^^^^")),
        ("item", 68, "{ readonly value: number; } & { value: number; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
        ("value", 73, "number", Some("^^^^^^")),
        ("1", 81, "1", Some("^")),
        ("item[\"value\"] = 2", 84, "2", Some("^")),
        ("item[\"value\"]", 84, "number", Some("^^^^^^")),
        ("item", 84, "{ readonly value: number; } & { value: number; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
        ("\"value\"", 89, "\"value\"", Some("^^^^^^^")),
        ("2", 100, "2", Some("^")),
    ], &[
    ]);
}

#[test]
fn intersection_mutable_property_reversed_strict() {
    assert_declarations(
        "declare const item: { value: number } & { readonly value: number }; item.value = 1;",
        true,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 22, "number", Some("^^^^^^")),
            ("value", 51, "number", Some("^^^^^^")),
            ("item.value = 1", 68, "1", Some("^")),
            ("item.value", 68, "number", Some("^^^^^^")),
            (
                "item",
                68,
                "{ value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 73, "number", Some("^^^^^^")),
            ("1", 81, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn intersection_mutable_property_reversed_loose() {
    assert_declarations(
        "declare const item: { value: number } & { readonly value: number }; item.value = 1;",
        false,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 22, "number", Some("^^^^^^")),
            ("value", 51, "number", Some("^^^^^^")),
            ("item.value = 1", 68, "1", Some("^")),
            ("item.value", 68, "number", Some("^^^^^^")),
            (
                "item",
                68,
                "{ value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 73, "number", Some("^^^^^^")),
            ("1", 81, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn intersection_readonly_properties_strict() {
    assert_declarations("declare const item: { readonly value: number } & { readonly value: number; label: string }; item.value = 1;", true, "es2020", false, &[
        ("item", 14, "{ readonly value: number; } & { readonly value: number; label: string; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^^^^^^^      ^^^")),
        ("value", 31, "number", Some("^^^^^^")),
        ("value", 60, "number", Some("^^^^^^")),
        ("label", 75, "string", Some("^^^^^^")),
        ("item.value = 1", 92, "1", Some("^")),
        ("item.value", 92, "any", Some("^^^")),
        ("item", 92, "{ readonly value: number; } & { readonly value: number; label: string; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^^^^^^^      ^^^")),
        ("value", 97, "any", Some("^^^")),
        ("1", 105, "1", Some("^")),
    ], &[
        (2540, 97, 5),
    ]);
}

#[test]
fn intersection_readonly_properties_loose() {
    assert_declarations("declare const item: { readonly value: number } & { readonly value: number; label: string }; item.value = 1;", false, "es2020", false, &[
        ("item", 14, "{ readonly value: number; } & { readonly value: number; label: string; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^^^^^^^      ^^^")),
        ("value", 31, "number", Some("^^^^^^")),
        ("value", 60, "number", Some("^^^^^^")),
        ("label", 75, "string", Some("^^^^^^")),
        ("item.value = 1", 92, "1", Some("^")),
        ("item.value", 92, "any", Some("^^^")),
        ("item", 92, "{ readonly value: number; } & { readonly value: number; label: string; }", Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^^^^^^^      ^^^")),
        ("value", 97, "any", Some("^^^")),
        ("1", 105, "1", Some("^")),
    ], &[
        (2540, 97, 5),
    ]);
}

#[test]
fn intersection_unrelated_property_strict() {
    assert_declarations(
        "declare const item: { readonly value: number } & { label: string }; item.value = 1;",
        true,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } & { label: string; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 31, "number", Some("^^^^^^")),
            ("label", 51, "string", Some("^^^^^^")),
            ("item.value = 1", 68, "1", Some("^")),
            ("item.value", 68, "any", Some("^^^")),
            (
                "item",
                68,
                "{ readonly value: number; } & { label: string; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 73, "any", Some("^^^")),
            ("1", 81, "1", Some("^")),
        ],
        &[(2540, 73, 5)],
    );
}

#[test]
fn intersection_unrelated_property_loose() {
    assert_declarations(
        "declare const item: { readonly value: number } & { label: string }; item.value = 1;",
        false,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } & { label: string; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 31, "number", Some("^^^^^^")),
            ("label", 51, "string", Some("^^^^^^")),
            ("item.value = 1", 68, "1", Some("^")),
            ("item.value", 68, "any", Some("^^^")),
            (
                "item",
                68,
                "{ readonly value: number; } & { label: string; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 73, "any", Some("^^^")),
            ("1", 81, "1", Some("^")),
        ],
        &[(2540, 73, 5)],
    );
}

#[test]
fn union_readonly_property_strict() {
    assert_declarations(
        "declare const item: { readonly value: number } | { value: number }; item.value = 1;",
        true,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } | { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 31, "number", Some("^^^^^^")),
            ("value", 51, "number", Some("^^^^^^")),
            ("item.value = 1", 68, "1", Some("^")),
            ("item.value", 68, "any", Some("^^^")),
            (
                "item",
                68,
                "{ readonly value: number; } | { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 73, "any", Some("^^^")),
            ("1", 81, "1", Some("^")),
        ],
        &[(2540, 73, 5)],
    );
}

#[test]
fn union_readonly_property_loose() {
    assert_declarations(
        "declare const item: { readonly value: number } | { value: number }; item.value = 1;",
        false,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } | { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 31, "number", Some("^^^^^^")),
            ("value", 51, "number", Some("^^^^^^")),
            ("item.value = 1", 68, "1", Some("^")),
            ("item.value", 68, "any", Some("^^^")),
            (
                "item",
                68,
                "{ readonly value: number; } | { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 73, "any", Some("^^^")),
            ("1", 81, "1", Some("^")),
        ],
        &[(2540, 73, 5)],
    );
}

#[test]
fn intersection_getter_and_mutable_strict() {
    assert_declarations(
        "declare const item: { get value(): number } & { value: number }; item.value = 1;",
        true,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } & { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 26, "number", Some("^^^^^^")),
            ("value", 48, "number", Some("^^^^^^")),
            ("item.value = 1", 65, "1", Some("^")),
            ("item.value", 65, "number", Some("^^^^^^")),
            (
                "item",
                65,
                "{ readonly value: number; } & { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 70, "number", Some("^^^^^^")),
            ("1", 78, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn intersection_getter_and_mutable_loose() {
    assert_declarations(
        "declare const item: { get value(): number } & { value: number }; item.value = 1;",
        false,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } & { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 26, "number", Some("^^^^^^")),
            ("value", 48, "number", Some("^^^^^^")),
            ("item.value = 1", 65, "1", Some("^")),
            ("item.value", 65, "number", Some("^^^^^^")),
            (
                "item",
                65,
                "{ readonly value: number; } & { value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 70, "number", Some("^^^^^^")),
            ("1", 78, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn intersection_getter_and_readonly_strict() {
    assert_declarations(
        "declare const item: { get value(): number } & { readonly value: number }; item.value = 1;",
        true,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 26, "number", Some("^^^^^^")),
            ("value", 57, "number", Some("^^^^^^")),
            ("item.value = 1", 74, "1", Some("^")),
            ("item.value", 74, "any", Some("^^^")),
            (
                "item",
                74,
                "{ readonly value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 79, "any", Some("^^^")),
            ("1", 87, "1", Some("^")),
        ],
        &[(2540, 79, 5)],
    );
}

#[test]
fn intersection_getter_and_readonly_loose() {
    assert_declarations(
        "declare const item: { get value(): number } & { readonly value: number }; item.value = 1;",
        false,
        "es2020",
        false,
        &[
            (
                "item",
                14,
                "{ readonly value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 26, "number", Some("^^^^^^")),
            ("value", 57, "number", Some("^^^^^^")),
            ("item.value = 1", 74, "1", Some("^")),
            ("item.value", 74, "any", Some("^^^")),
            (
                "item",
                74,
                "{ readonly value: number; } & { readonly value: number; }",
                Some("^^^^^^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^      ^^^"),
            ),
            ("value", 79, "any", Some("^^^")),
            ("1", 87, "1", Some("^")),
        ],
        &[(2540, 79, 5)],
    );
}

#[test]
fn paired_getter_setter_property_strict() {
    assert_declarations(
        "declare const item: { get value(): number; set value(input: number) }; item.value = 1;",
        true,
        "es2020",
        false,
        &[
            ("item", 14, "{ value: number; }", Some("^^^^^^^^^      ^^^")),
            ("value", 26, "number", Some("^^^^^^")),
            ("value", 47, "number", Some("^^^^^^")),
            ("input", 53, "number", Some("^^^^^^")),
            ("item.value = 1", 71, "1", Some("^")),
            ("item.value", 71, "number", Some("^^^^^^")),
            ("item", 71, "{ value: number; }", Some("^^^^^^^^^      ^^^")),
            ("value", 76, "number", Some("^^^^^^")),
            ("1", 84, "1", Some("^")),
        ],
        &[],
    );
}

#[test]
fn paired_getter_setter_property_loose() {
    assert_declarations(
        "declare const item: { get value(): number; set value(input: number) }; item.value = 1;",
        false,
        "es2020",
        false,
        &[
            ("item", 14, "{ value: number; }", Some("^^^^^^^^^      ^^^")),
            ("value", 26, "number", Some("^^^^^^")),
            ("value", 47, "number", Some("^^^^^^")),
            ("input", 53, "number", Some("^^^^^^")),
            ("item.value = 1", 71, "1", Some("^")),
            ("item.value", 71, "number", Some("^^^^^^")),
            ("item", 71, "{ value: number; }", Some("^^^^^^^^^      ^^^")),
            ("value", 76, "number", Some("^^^^^^")),
            ("1", 84, "1", Some("^")),
        ],
        &[],
    );
}
