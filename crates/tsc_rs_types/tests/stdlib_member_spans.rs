use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

// Original TypeScript 6.0.3 TypeWriterWalker text and synthetic-node masks.
fn assert_declarations(
    source: &str,
    strict: bool,
    library: &str,
    expected: &[(&str, usize, &str, &str)],
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
        assert_eq!(
            actual.underline, underline,
            "{anchor} at {offset}: {source}"
        );
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
fn array_map_value_strict() {
    assert_declarations("declare const values: number[]; const method = values.map;", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values.map", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("map", 54, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_map_value_loose() {
    assert_declarations("declare const values: number[]; const method = values.map;", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values.map", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("map", 54, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_every_value_strict() {
    assert_declarations("declare const values: (string | number)[]; const method = values.every;", true, "es2020", &[
        ("values", 14, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("method", 49, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values.every", 58, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values", 58, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("every", 65, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
    ], &[
    ]);
}

#[test]
fn array_every_value_loose() {
    assert_declarations("declare const values: (string | number)[]; const method = values.every;", false, "es2020", &[
        ("values", 14, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("method", 49, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values.every", 58, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values", 58, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("every", 65, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
    ], &[
    ]);
}

#[test]
fn array_filter_value_strict() {
    assert_declarations("declare const values: number[]; const method = values.filter;", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values.filter", 47, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("filter", 54, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_filter_value_loose() {
    assert_declarations("declare const values: number[]; const method = values.filter;", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values.filter", 47, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("filter", 54, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_find_value_strict() {
    assert_declarations("declare const values: number[]; const method = values.find;", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ <S extends number>(predicate: (value: number, index: number, obj: number[]) => value is S, thisArg?: any): S | undefined; (predicate: (value: number, index: number, obj: number[]) => unknown, thisArg?: any): number | undefined; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values.find", 47, "{ <S extends number>(predicate: (value: number, index: number, obj: number[]) => value is S, thisArg?: any): S | undefined; (predicate: (value: number, index: number, obj: number[]) => unknown, thisArg?: any): number | undefined; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("find", 54, "{ <S extends number>(predicate: (value: number, index: number, obj: number[]) => value is S, thisArg?: any): S | undefined; (predicate: (value: number, index: number, obj: number[]) => unknown, thisArg?: any): number | undefined; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_find_value_loose() {
    assert_declarations("declare const values: number[]; const method = values.find;", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ <S extends number>(predicate: (value: number, index: number, obj: number[]) => value is S, thisArg?: any): S; (predicate: (value: number, index: number, obj: number[]) => unknown, thisArg?: any): number; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^"),
        ("values.find", 47, "{ <S extends number>(predicate: (value: number, index: number, obj: number[]) => value is S, thisArg?: any): S; (predicate: (value: number, index: number, obj: number[]) => unknown, thisArg?: any): number; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("find", 54, "{ <S extends number>(predicate: (value: number, index: number, obj: number[]) => value is S, thisArg?: any): S; (predicate: (value: number, index: number, obj: number[]) => unknown, thisArg?: any): number; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^   ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_reduce_value_strict() {
    assert_declarations("declare const values: number[]; const method = values.reduce;", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("values.reduce", 47, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("reduce", 54, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_reduce_value_loose() {
    assert_declarations("declare const values: number[]; const method = values.reduce;", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("values.reduce", 47, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("reduce", 54, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_concat_value_strict() {
    assert_declarations("declare const values: number[]; const method = values.concat;", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ (...items: ConcatArray<number>[]): number[]; (...items: (number | ConcatArray<number>)[]): number[]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values.concat", 47, "{ (...items: ConcatArray<number>[]): number[]; (...items: (number | ConcatArray<number>)[]): number[]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("concat", 54, "{ (...items: ConcatArray<number>[]): number[]; (...items: (number | ConcatArray<number>)[]): number[]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_concat_value_loose() {
    assert_declarations("declare const values: number[]; const method = values.concat;", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("method", 38, "{ (...items: ConcatArray<number>[]): number[]; (...items: (number | ConcatArray<number>)[]): number[]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values.concat", 47, "{ (...items: ConcatArray<number>[]): number[]; (...items: (number | ConcatArray<number>)[]): number[]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("concat", 54, "{ (...items: ConcatArray<number>[]): number[]; (...items: (number | ConcatArray<number>)[]): number[]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_push_value_strict() {
    assert_declarations(
        "declare const values: number[]; const method = values.push;",
        true,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            (
                "method",
                38,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            (
                "values.push",
                47,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            ("values", 47, "number[]", "^^^^^^^^"),
            (
                "push",
                54,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
        ],
        &[],
    );
}

#[test]
fn array_push_value_loose() {
    assert_declarations(
        "declare const values: number[]; const method = values.push;",
        false,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            (
                "method",
                38,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            (
                "values.push",
                47,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            ("values", 47, "number[]", "^^^^^^^^"),
            (
                "push",
                54,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
        ],
        &[],
    );
}

#[test]
fn array_pop_value_strict() {
    assert_declarations(
        "declare const values: number[]; const method = values.pop;",
        true,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            (
                "method",
                38,
                "() => number | undefined",
                "^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.pop",
                47,
                "() => number | undefined",
                "^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 47, "number[]", "^^^^^^^^"),
            (
                "pop",
                54,
                "() => number | undefined",
                "^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn array_pop_value_loose() {
    assert_declarations(
        "declare const values: number[]; const method = values.pop;",
        false,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            ("method", 38, "() => number", "^^^^^^^^^^^^"),
            ("values.pop", 47, "() => number", "^^^^^^^^^^^^"),
            ("values", 47, "number[]", "^^^^^^^^"),
            ("pop", 54, "() => number", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn array_sort_value_strict() {
    assert_declarations(
        "declare const values: number[]; const method = values.sort;",
        true,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            (
                "method",
                38,
                "(compareFn?: ((a: number, b: number) => number) | undefined) => number[]",
                "^         ^^^^^ ^^^^^^^^^^ ^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.sort",
                47,
                "(compareFn?: ((a: number, b: number) => number) | undefined) => number[]",
                "^         ^^^^^ ^^^^^^^^^^ ^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 47, "number[]", "^^^^^^^^"),
            (
                "sort",
                54,
                "(compareFn?: ((a: number, b: number) => number) | undefined) => number[]",
                "^         ^^^^^ ^^^^^^^^^^ ^^^^^^^^^^^^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn array_sort_value_loose() {
    assert_declarations(
        "declare const values: number[]; const method = values.sort;",
        false,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            (
                "method",
                38,
                "(compareFn?: (a: number, b: number) => number) => number[]",
                "^         ^^^^ ^^^^^^^^^^ ^^^^^^^^^^^^^      ^^^^^^^^^^^^^",
            ),
            (
                "values.sort",
                47,
                "(compareFn?: (a: number, b: number) => number) => number[]",
                "^         ^^^^ ^^^^^^^^^^ ^^^^^^^^^^^^^      ^^^^^^^^^^^^^",
            ),
            ("values", 47, "number[]", "^^^^^^^^"),
            (
                "sort",
                54,
                "(compareFn?: (a: number, b: number) => number) => number[]",
                "^         ^^^^ ^^^^^^^^^^ ^^^^^^^^^^^^^      ^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn array_foreach_callback_strict() {
    assert_declarations("declare const values: number[]; values.forEach((value, index, array) => { const copy = value; });", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.forEach((value, index, array) => { const copy = value; })", 32, "void", "^^^^"),
        ("values.forEach", 32, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("forEach", 39, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("(value, index, array) => { const copy = value; }", 47, "(value: number, index: number, array: number[]) => void", "^     ^^^^^^^^^^     ^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 48, "number", "^^^^^^"),
        ("index", 55, "number", "^^^^^^"),
        ("array", 62, "number[]", "^^^^^^^^"),
        ("copy", 80, "number", "^^^^^^"),
        ("value", 87, "number", "^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_foreach_callback_loose() {
    assert_declarations("declare const values: number[]; values.forEach((value, index, array) => { const copy = value; });", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.forEach((value, index, array) => { const copy = value; })", 32, "void", "^^^^"),
        ("values.forEach", 32, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("forEach", 39, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("(value, index, array) => { const copy = value; }", 47, "(value: number, index: number, array: number[]) => void", "^     ^^^^^^^^^^     ^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 48, "number", "^^^^^^"),
        ("index", 55, "number", "^^^^^^"),
        ("array", 62, "number[]", "^^^^^^^^"),
        ("copy", 80, "number", "^^^^^^"),
        ("value", 87, "number", "^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_every_callback_strict() {
    assert_declarations("declare const values: number[]; const result = values.every(value => value > 0);", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "boolean", "^^^^^^^"),
        ("values.every(value => value > 0)", 47, "boolean", "^^^^^^^"),
        ("values.every", 47, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("every", 54, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("value => value > 0", 60, "(value: number) => boolean", "^     ^^^^^^^^^^^^^^^^^^^^"),
        ("value", 60, "number", "^^^^^^"),
        ("value > 0", 69, "boolean", "^^^^^^^"),
        ("value", 69, "number", "^^^^^^"),
        ("0", 77, "0", "^"),
    ], &[
    ]);
}

#[test]
fn array_every_callback_loose() {
    assert_declarations("declare const values: number[]; const result = values.every(value => value > 0);", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "boolean", "^^^^^^^"),
        ("values.every(value => value > 0)", 47, "boolean", "^^^^^^^"),
        ("values.every", 47, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("every", 54, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): this is S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^        ^  ^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("value => value > 0", 60, "(value: number) => boolean", "^     ^^^^^^^^^^^^^^^^^^^^"),
        ("value", 60, "number", "^^^^^^"),
        ("value > 0", 69, "boolean", "^^^^^^^"),
        ("value", 69, "number", "^^^^^^"),
        ("0", 77, "0", "^"),
    ], &[
    ]);
}

#[test]
fn array_filter_callback_strict() {
    assert_declarations("declare const values: number[]; const result = values.filter(value => value > 0);", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number[]", "^^^^^^^^"),
        ("values.filter(value => value > 0)", 47, "number[]", "^^^^^^^^"),
        ("values.filter", 47, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("filter", 54, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("value => value > 0", 61, "(value: number) => boolean", "^     ^^^^^^^^^^^^^^^^^^^^"),
        ("value", 61, "number", "^^^^^^"),
        ("value > 0", 70, "boolean", "^^^^^^^"),
        ("value", 70, "number", "^^^^^^"),
        ("0", 78, "0", "^"),
    ], &[
    ]);
}

#[test]
fn array_filter_callback_loose() {
    assert_declarations("declare const values: number[]; const result = values.filter(value => value > 0);", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number[]", "^^^^^^^^"),
        ("values.filter(value => value > 0)", 47, "number[]", "^^^^^^^^"),
        ("values.filter", 47, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("filter", 54, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("value => value > 0", 61, "(value: number) => boolean", "^     ^^^^^^^^^^^^^^^^^^^^"),
        ("value", 61, "number", "^^^^^^"),
        ("value > 0", 70, "boolean", "^^^^^^^"),
        ("value", 70, "number", "^^^^^^"),
        ("0", 78, "0", "^"),
    ], &[
    ]);
}

#[test]
fn array_map_callback_strict() {
    assert_declarations("declare const values: number[]; const result = values.map(value => value + 1);", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number[]", "^^^^^^^^"),
        ("values.map(value => value + 1)", 47, "number[]", "^^^^^^^^"),
        ("values.map", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("map", 54, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("value => value + 1", 58, "(value: number) => number", "^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 58, "number", "^^^^^^"),
        ("value + 1", 67, "number", "^^^^^^"),
        ("value", 67, "number", "^^^^^^"),
        ("1", 75, "1", "^"),
    ], &[
    ]);
}

#[test]
fn array_map_callback_loose() {
    assert_declarations("declare const values: number[]; const result = values.map(value => value + 1);", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number[]", "^^^^^^^^"),
        ("values.map(value => value + 1)", 47, "number[]", "^^^^^^^^"),
        ("values.map", 47, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("map", 54, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("value => value + 1", 58, "(value: number) => number", "^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 58, "number", "^^^^^^"),
        ("value + 1", 67, "number", "^^^^^^"),
        ("value", 67, "number", "^^^^^^"),
        ("1", 75, "1", "^"),
    ], &[
    ]);
}

#[test]
fn array_reduce_callback_strict() {
    assert_declarations("declare const values: number[]; const result = values.reduce((previous, current) => previous + current, 0);", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number", "^^^^^^"),
        ("values.reduce((previous, current) => previous + current, 0)", 47, "number", "^^^^^^"),
        ("values.reduce", 47, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("reduce", 54, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("(previous, current) => previous + current", 61, "(previous: number, current: number) => number", "^        ^^^^^^^^^^       ^^^^^^^^^^^^^^^^^^^"),
        ("previous", 62, "number", "^^^^^^"),
        ("current", 72, "number", "^^^^^^"),
        ("previous + current", 84, "number", "^^^^^^"),
        ("previous", 84, "number", "^^^^^^"),
        ("current", 95, "number", "^^^^^^"),
        ("0", 104, "0", "^"),
    ], &[
    ]);
}

#[test]
fn array_reduce_callback_loose() {
    assert_declarations("declare const values: number[]; const result = values.reduce((previous, current) => previous + current, 0);", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number", "^^^^^^"),
        ("values.reduce((previous, current) => previous + current, 0)", 47, "number", "^^^^^^"),
        ("values.reduce", 47, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("reduce", 54, "{ (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number): number; (callbackfn: (previousValue: number, currentValue: number, currentIndex: number, array: number[]) => number, initialValue: number): number; <U>(callbackfn: (previousValue: U, currentValue: number, currentIndex: number, array: number[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^          ^^^             ^^^^^^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^^^^^^^^^^^ ^^          ^^^             ^^^^^            ^^^^^^^^^^            ^^      ^^     ^^^^^^^^^^^^^^^^^^            ^^^^^^^^^^"),
        ("(previous, current) => previous + current", 61, "(previous: number, current: number) => number", "^        ^^^^^^^^^^       ^^^^^^^^^^^^^^^^^^^"),
        ("previous", 62, "number", "^^^^^^"),
        ("current", 72, "number", "^^^^^^"),
        ("previous + current", 84, "number", "^^^^^^"),
        ("previous", 84, "number", "^^^^^^"),
        ("current", 95, "number", "^^^^^^"),
        ("0", 104, "0", "^"),
    ], &[
    ]);
}

#[test]
fn array_callback_type_guard_strict() {
    assert_declarations("declare const values: (string | number)[]; const result = values.filter((value): value is string => typeof value === \"string\");", true, "es2020", &[
        ("values", 14, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("result", 49, "string[]", "^^^^^^^^"),
        ("values.filter((value): value is string => typeof value === \"string\")", 58, "string[]", "^^^^^^^^"),
        ("values.filter", 58, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): (string | number)[]; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 58, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("filter", 65, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): (string | number)[]; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("(value): value is string => typeof value === \"string\"", 72, "(value: string | number) => value is string", "^     ^^^^^^^^^^^^^^^^^^^^^^               "),
        ("value", 73, "string | number", "^^^^^^^^^^^^^^^"),
        ("typeof value === \"string\"", 100, "boolean", "^^^^^^^"),
        ("typeof value", 100, "\"string\" | \"number\" | \"bigint\" | \"boolean\" | \"symbol\" | \"undefined\" | \"object\" | \"function\"", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("value", 107, "string | number", "^^^^^^^^^^^^^^^"),
        ("\"string\"", 117, "\"string\"", "^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_callback_type_guard_loose() {
    assert_declarations("declare const values: (string | number)[]; const result = values.filter((value): value is string => typeof value === \"string\");", false, "es2020", &[
        ("values", 14, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("result", 49, "string[]", "^^^^^^^^"),
        ("values.filter((value): value is string => typeof value === \"string\")", 58, "string[]", "^^^^^^^^"),
        ("values.filter", 58, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): (string | number)[]; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 58, "(string | number)[]", "^^^^^^^^^^^^^^^^^^^"),
        ("filter", 65, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: (string | number)[]) => value is S, thisArg?: any): S[]; (predicate: (value: string | number, index: number, array: (string | number)[]) => unknown, thisArg?: any): (string | number)[]; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("(value): value is string => typeof value === \"string\"", 72, "(value: string | number) => value is string", "^     ^^^^^^^^^^^^^^^^^^^^^^               "),
        ("value", 73, "string | number", "^^^^^^^^^^^^^^^"),
        ("typeof value === \"string\"", 100, "boolean", "^^^^^^^"),
        ("typeof value", 100, "\"string\" | \"number\" | \"bigint\" | \"boolean\" | \"symbol\" | \"undefined\" | \"object\" | \"function\"", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("value", 107, "string | number", "^^^^^^^^^^^^^^^"),
        ("\"string\"", 117, "\"string\"", "^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_contextual_map_result_strict() {
    assert_declarations("declare const values: number[]; const result: { kind: \"value\" }[] = values.map(value => ({ kind: \"value\" }));", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "{ kind: \"value\"; }[]", "^^^^^^^^       ^^^^^"),
        ("kind", 48, "\"value\"", "^^^^^^^"),
        ("values.map(value => ({ kind: \"value\" }))", 68, "{ kind: \"value\"; }[]", "^^^^^^^^^^^^^^^^^^^^"),
        ("values.map", 68, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 68, "number[]", "^^^^^^^^"),
        ("map", 75, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("value => ({ kind: \"value\" })", 79, "(value: number) => { kind: \"value\"; }", "^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("value", 79, "number", "^^^^^^"),
        ("({ kind: \"value\" })", 88, "{ kind: \"value\"; }", "^^^^^^^^^^^^^^^^^^"),
        ("{ kind: \"value\" }", 89, "{ kind: \"value\"; }", "^^^^^^^^^^^^^^^^^^"),
        ("kind", 91, "\"value\"", "^^^^^^^"),
        ("\"value\"", 97, "\"value\"", "^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_contextual_map_result_loose() {
    assert_declarations("declare const values: number[]; const result: { kind: \"value\" }[] = values.map(value => ({ kind: \"value\" }));", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "{ kind: \"value\"; }[]", "^^^^^^^^       ^^^^^"),
        ("kind", 48, "\"value\"", "^^^^^^^"),
        ("values.map(value => ({ kind: \"value\" }))", 68, "{ kind: \"value\"; }[]", "^^^^^^^^^^^^^^^^^^^^"),
        ("values.map", 68, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 68, "number[]", "^^^^^^^^"),
        ("map", 75, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("value => ({ kind: \"value\" })", 79, "(value: number) => { kind: \"value\"; }", "^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("value", 79, "number", "^^^^^^"),
        ("({ kind: \"value\" })", 88, "{ kind: \"value\"; }", "^^^^^^^^^^^^^^^^^^"),
        ("{ kind: \"value\" }", 89, "{ kind: \"value\"; }", "^^^^^^^^^^^^^^^^^^"),
        ("kind", 91, "\"value\"", "^^^^^^^"),
        ("\"value\"", 97, "\"value\"", "^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_generic_owner_strict() {
    assert_declarations("function method<T>(values: T[]) { return values.map; }", true, "es2020", &[
        ("method", 9, "<T>(values: T[]) => <U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any) => U[]", "^ ^^      ^^   ^^^^^^ ^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 19, "T[]", "^^^"),
        ("values.map", 41, "<U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 41, "T[]", "^^^"),
        ("map", 48, "<U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_generic_owner_loose() {
    assert_declarations("function method<T>(values: T[]) { return values.map; }", false, "es2020", &[
        ("method", 9, "<T>(values: T[]) => <U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any) => U[]", "^ ^^      ^^   ^^^^^^ ^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 19, "T[]", "^^^"),
        ("values.map", 41, "<U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 41, "T[]", "^^^"),
        ("map", 48, "<U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_generic_shadow_strict() {
    assert_declarations("function method<U>(values: U[]) { return values.map; }", true, "es2020", &[
        ("method", 9, "<U>(values: U[]) => <U_1>(callbackfn: (value: U, index: number, array: U[]) => U_1, thisArg?: any) => U_1[]", "^ ^^      ^^   ^^^^^^^^^^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^"),
        ("values", 19, "U[]", "^^^"),
        ("values.map", 41, "<U_1>(callbackfn: (value: U, index: number, array: U[]) => U_1, thisArg?: any) => U_1[]", "^^^^^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^"),
        ("values", 41, "U[]", "^^^"),
        ("map", 48, "<U_1>(callbackfn: (value: U, index: number, array: U[]) => U_1, thisArg?: any) => U_1[]", "^^^^^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_generic_shadow_loose() {
    assert_declarations("function method<U>(values: U[]) { return values.map; }", false, "es2020", &[
        ("method", 9, "<U>(values: U[]) => <U_1>(callbackfn: (value: U, index: number, array: U[]) => U_1, thisArg?: any) => U_1[]", "^ ^^      ^^   ^^^^^^^^^^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^"),
        ("values", 19, "U[]", "^^^"),
        ("values.map", 41, "<U_1>(callbackfn: (value: U, index: number, array: U[]) => U_1, thisArg?: any) => U_1[]", "^^^^^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^"),
        ("values", 41, "U[]", "^^^"),
        ("map", 48, "<U_1>(callbackfn: (value: U, index: number, array: U[]) => U_1, thisArg?: any) => U_1[]", "^^^^^^          ^^^     ^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn readonly_array_map_value_strict() {
    assert_declarations("declare const values: readonly number[]; const method = values.map;", true, "es2020", &[
        ("values", 14, "readonly number[]", "^^^^^^^^^^^^^^^^^"),
        ("method", 47, "<U>(callbackfn: (value: number, index: number, array: readonly number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values.map", 56, "<U>(callbackfn: (value: number, index: number, array: readonly number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 56, "readonly number[]", "^^^^^^^^^^^^^^^^^"),
        ("map", 63, "<U>(callbackfn: (value: number, index: number, array: readonly number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn readonly_array_map_value_loose() {
    assert_declarations("declare const values: readonly number[]; const method = values.map;", false, "es2020", &[
        ("values", 14, "readonly number[]", "^^^^^^^^^^^^^^^^^"),
        ("method", 47, "<U>(callbackfn: (value: number, index: number, array: readonly number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values.map", 56, "<U>(callbackfn: (value: number, index: number, array: readonly number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 56, "readonly number[]", "^^^^^^^^^^^^^^^^^"),
        ("map", 63, "<U>(callbackfn: (value: number, index: number, array: readonly number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn readonly_array_every_value_strict() {
    assert_declarations("declare const values: readonly (string | number)[]; const method = values.every;", true, "es2020", &[
        ("values", 14, "readonly (string | number)[]", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("method", 58, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: readonly (string | number)[]) => value is S, thisArg?: any): this is readonly S[]; (predicate: (value: string | number, index: number, array: readonly (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^                 ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values.every", 67, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: readonly (string | number)[]) => value is S, thisArg?: any): this is readonly S[]; (predicate: (value: string | number, index: number, array: readonly (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^                 ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values", 67, "readonly (string | number)[]", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("every", 74, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: readonly (string | number)[]) => value is S, thisArg?: any): this is readonly S[]; (predicate: (value: string | number, index: number, array: readonly (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^                 ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
    ], &[
    ]);
}

#[test]
fn readonly_array_every_value_loose() {
    assert_declarations("declare const values: readonly (string | number)[]; const method = values.every;", false, "es2020", &[
        ("values", 14, "readonly (string | number)[]", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("method", 58, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: readonly (string | number)[]) => value is S, thisArg?: any): this is readonly S[]; (predicate: (value: string | number, index: number, array: readonly (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^                 ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values.every", 67, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: readonly (string | number)[]) => value is S, thisArg?: any): this is readonly S[]; (predicate: (value: string | number, index: number, array: readonly (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^                 ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
        ("values", 67, "readonly (string | number)[]", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("every", 74, "{ <S extends string | number>(predicate: (value: string | number, index: number, array: readonly (string | number)[]) => value is S, thisArg?: any): this is readonly S[]; (predicate: (value: string | number, index: number, array: readonly (string | number)[]) => unknown, thisArg?: any): boolean; }", "^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^                 ^  ^^^         ^^^     ^^^^^^^^^^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^       ^^^   ^^^       ^^^"),
    ], &[
    ]);
}

#[test]
fn tuple_slice_value_strict() {
    assert_declarations(
        "declare const values: [number, string]; const method = values.slice;",
        true,
        "es2020",
        &[
            ("values", 14, "[number, string]", "^^^^^^^^^^^^^^^^"),
            (
                "method",
                46,
                "(start?: number, end?: number) => (string | number)[]",
                "^     ^^^      ^^   ^^^      ^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.slice",
                55,
                "(start?: number, end?: number) => (string | number)[]",
                "^     ^^^      ^^   ^^^      ^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 55, "[number, string]", "^^^^^^^^^^^^^^^^"),
            (
                "slice",
                62,
                "(start?: number, end?: number) => (string | number)[]",
                "^     ^^^      ^^   ^^^      ^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn tuple_slice_value_loose() {
    assert_declarations(
        "declare const values: [number, string]; const method = values.slice;",
        false,
        "es2020",
        &[
            ("values", 14, "[number, string]", "^^^^^^^^^^^^^^^^"),
            (
                "method",
                46,
                "(start?: number, end?: number) => (string | number)[]",
                "^     ^^^      ^^   ^^^      ^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.slice",
                55,
                "(start?: number, end?: number) => (string | number)[]",
                "^     ^^^      ^^   ^^^      ^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 55, "[number, string]", "^^^^^^^^^^^^^^^^"),
            (
                "slice",
                62,
                "(start?: number, end?: number) => (string | number)[]",
                "^     ^^^      ^^   ^^^      ^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn array_optional_context_strict() {
    assert_declarations("declare const values: number[]; values.forEach((value, index, array) => {}, null);", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.forEach((value, index, array) => {}, null)", 32, "void", "^^^^"),
        ("values.forEach", 32, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("forEach", 39, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("(value, index, array) => {}", 47, "(value: number, index: number, array: number[]) => void", "^     ^^^^^^^^^^     ^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 48, "number", "^^^^^^"),
        ("index", 55, "number", "^^^^^^"),
        ("array", 62, "number[]", "^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_optional_context_loose() {
    assert_declarations("declare const values: number[]; values.forEach((value, index, array) => {}, null);", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.forEach((value, index, array) => {}, null)", 32, "void", "^^^^"),
        ("values.forEach", 32, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("forEach", 39, "(callbackfn: (value: number, index: number, array: number[]) => void, thisArg?: any) => void", "^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^    ^^       ^^^   ^^^^^    "),
        ("(value, index, array) => {}", 47, "(value: number, index: number, array: number[]) => void", "^     ^^^^^^^^^^     ^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 48, "number", "^^^^^^"),
        ("index", 55, "number", "^^^^^^"),
        ("array", 62, "number[]", "^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_wrong_push_argument_strict() {
    assert_declarations(
        "declare const values: number[]; values.push(\"wrong\");",
        true,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            ("values.push(\"wrong\")", 32, "number", "^^^^^^"),
            (
                "values.push",
                32,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            ("values", 32, "number[]", "^^^^^^^^"),
            (
                "push",
                39,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            ("\"wrong\"", 44, "\"wrong\"", "^^^^^^^"),
        ],
        &[(2345, 44, 7)],
    );
}

#[test]
fn array_wrong_push_argument_loose() {
    assert_declarations(
        "declare const values: number[]; values.push(\"wrong\");",
        false,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            ("values.push(\"wrong\")", 32, "number", "^^^^^^"),
            (
                "values.push",
                32,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            ("values", 32, "number[]", "^^^^^^^^"),
            (
                "push",
                39,
                "(...items: number[]) => number",
                "^^^^     ^^^^^^^^^^^^^^^      ",
            ),
            ("\"wrong\"", 44, "\"wrong\"", "^^^^^^^"),
        ],
        &[(2345, 44, 7)],
    );
}

#[test]
fn array_wrong_map_argument_strict() {
    assert_declarations("declare const values: number[]; values.map(1);", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.map(1)", 32, "unknown[]", "^^^^^^^^^"),
        ("values.map", 32, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("map", 39, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("1", 43, "1", "^"),
    ], &[
        (2345, 43, 1),
    ]);
}

#[test]
fn array_wrong_map_argument_loose() {
    assert_declarations("declare const values: number[]; values.map(1);", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.map(1)", 32, "unknown[]", "^^^^^^^^^"),
        ("values.map", 32, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("map", 39, "<U>(callbackfn: (value: number, index: number, array: number[]) => U, thisArg?: any) => U[]", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^"),
        ("1", 43, "1", "^"),
    ], &[
        (2345, 43, 1),
    ]);
}

#[test]
fn array_wrong_filter_callback_strict() {
    assert_declarations("declare const values: number[]; values.filter((value: string) => true);", true, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.filter((value: string) => true)", 32, "number[]", "^^^^^^^^"),
        ("values.filter", 32, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("filter", 39, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("(value: string) => true", 46, "(value: string) => true", "^     ^^      ^^^^^^^^^"),
        ("value", 47, "string", "^^^^^^"),
        ("true", 65, "true", "^^^^"),
    ], &[
        (2769, 46, 23),
    ]);
}

#[test]
fn array_wrong_filter_callback_loose() {
    assert_declarations("declare const values: number[]; values.filter((value: string) => true);", false, "es2020", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("values.filter((value: string) => true)", 32, "number[]", "^^^^^^^^"),
        ("values.filter", 32, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("values", 32, "number[]", "^^^^^^^^"),
        ("filter", 39, "{ <S extends number>(predicate: (value: number, index: number, array: number[]) => value is S, thisArg?: any): S[]; (predicate: (value: number, index: number, array: number[]) => unknown, thisArg?: any): number[]; }", "^^^ ^^^^^^^^^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^       ^^^   ^^^^^^^^^         ^^^     ^^^^^^^^^^     ^^      ^^     ^^^^^^^^^^^^^^^       ^^       ^^^   ^^^^^^^^^^^^^^"),
        ("(value: string) => true", 46, "(value: string) => true", "^     ^^      ^^^^^^^^^"),
        ("value", 47, "string", "^^^^^^"),
        ("true", 65, "true", "^^^^"),
    ], &[
        (2769, 46, 23),
    ]);
}

#[test]
fn array_flat_value_strict() {
    assert_declarations(
        "declare const values: number[][]; const method = values.flat;",
        true,
        "es2020",
        &[
            ("values", 14, "number[][]", "^^^^^^^^^^"),
            (
                "method",
                40,
                "<A, D extends number = 1>(this: A, depth?: D | undefined) => FlatArray<A, D>[]",
                "^ ^^ ^^^^^^^^^      ^^^^^^    ^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.flat",
                49,
                "<A, D extends number = 1>(this: A, depth?: D | undefined) => FlatArray<A, D>[]",
                "^ ^^ ^^^^^^^^^      ^^^^^^    ^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 49, "number[][]", "^^^^^^^^^^"),
            (
                "flat",
                56,
                "<A, D extends number = 1>(this: A, depth?: D | undefined) => FlatArray<A, D>[]",
                "^ ^^ ^^^^^^^^^      ^^^^^^    ^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn array_flat_value_loose() {
    assert_declarations(
        "declare const values: number[][]; const method = values.flat;",
        false,
        "es2020",
        &[
            ("values", 14, "number[][]", "^^^^^^^^^^"),
            (
                "method",
                40,
                "<A, D extends number = 1>(this: A, depth?: D) => FlatArray<A, D>[]",
                "^ ^^ ^^^^^^^^^      ^^^^^^    ^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.flat",
                49,
                "<A, D extends number = 1>(this: A, depth?: D) => FlatArray<A, D>[]",
                "^ ^^ ^^^^^^^^^      ^^^^^^    ^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 49, "number[][]", "^^^^^^^^^^"),
            (
                "flat",
                56,
                "<A, D extends number = 1>(this: A, depth?: D) => FlatArray<A, D>[]",
                "^ ^^ ^^^^^^^^^      ^^^^^^    ^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn array_iterator_value_strict() {
    assert_declarations(
        "declare const values: number[]; const result = values.values();",
        true,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            (
                "result",
                38,
                "ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.values()",
                47,
                "ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.values",
                47,
                "() => ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 47, "number[]", "^^^^^^^^"),
            (
                "values",
                54,
                "() => ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn array_iterator_value_loose() {
    assert_declarations(
        "declare const values: number[]; const result = values.values();",
        false,
        "es2020",
        &[
            ("values", 14, "number[]", "^^^^^^^^"),
            (
                "result",
                38,
                "ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.values()",
                47,
                "ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "values.values",
                47,
                "() => ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("values", 47, "number[]", "^^^^^^^^"),
            (
                "values",
                54,
                "() => ArrayIterator<number>",
                "^^^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn iterator_helpers_pipeline_strict() {
    assert_declarations("declare const values: number[]; const result = values.values().map(value => value + 1).toArray();", true, "es2025", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number[]", "^^^^^^^^"),
        ("values.values().map(value => value + 1).toArray()", 47, "number[]", "^^^^^^^^"),
        ("values.values().map(value => value + 1).toArray", 47, "() => number[]", "^^^^^^^^^^^^^^"),
        ("values.values().map(value => value + 1)", 47, "IteratorObject<number, undefined, unknown>", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values.values().map", 47, "<U>(callbackfn: (value: number, index: number) => U) => IteratorObject<U, undefined, unknown>", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values.values()", 47, "ArrayIterator<number>", "^^^^^^^^^^^^^^^^^^^^^"),
        ("values.values", 47, "() => ArrayIterator<number>", "^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("values", 54, "() => ArrayIterator<number>", "^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("map", 63, "<U>(callbackfn: (value: number, index: number) => U) => IteratorObject<U, undefined, unknown>", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("value => value + 1", 67, "(value: number) => number", "^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 67, "number", "^^^^^^"),
        ("value + 1", 76, "number", "^^^^^^"),
        ("value", 76, "number", "^^^^^^"),
        ("1", 84, "1", "^"),
        ("toArray", 87, "() => number[]", "^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn iterator_helpers_pipeline_loose() {
    assert_declarations("declare const values: number[]; const result = values.values().map(value => value + 1).toArray();", false, "es2025", &[
        ("values", 14, "number[]", "^^^^^^^^"),
        ("result", 38, "number[]", "^^^^^^^^"),
        ("values.values().map(value => value + 1).toArray()", 47, "number[]", "^^^^^^^^"),
        ("values.values().map(value => value + 1).toArray", 47, "() => number[]", "^^^^^^^^^^^^^^"),
        ("values.values().map(value => value + 1)", 47, "IteratorObject<number, undefined, unknown>", "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values.values().map", 47, "<U>(callbackfn: (value: number, index: number) => U) => IteratorObject<U, undefined, unknown>", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values.values()", 47, "ArrayIterator<number>", "^^^^^^^^^^^^^^^^^^^^^"),
        ("values.values", 47, "() => ArrayIterator<number>", "^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 47, "number[]", "^^^^^^^^"),
        ("values", 54, "() => ArrayIterator<number>", "^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("map", 63, "<U>(callbackfn: (value: number, index: number) => U) => IteratorObject<U, undefined, unknown>", "^ ^^          ^^^     ^^^^^^^^^^     ^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("value => value + 1", 67, "(value: number) => number", "^     ^^^^^^^^^^^^^^^^^^^"),
        ("value", 67, "number", "^^^^^^"),
        ("value + 1", 76, "number", "^^^^^^"),
        ("value", 76, "number", "^^^^^^"),
        ("1", 84, "1", "^"),
        ("toArray", 87, "() => number[]", "^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn array_reduce_generic_accumulator_strict() {
    assert_declarations("function compose<T>(fns: ((x: T) => T)[], initial: T) { return fns.reduce((previous, fn) => fn(previous), initial); }", true, "es2020", &[
        ("compose", 9, "<T>(fns: ((x: T) => T)[], initial: T) => T", "^ ^^   ^^               ^^       ^^ ^^^^^^"),
        ("fns", 20, "((x: T) => T)[]", "^^ ^^ ^^^^^ ^^^"),
        ("x", 27, "T", "^"),
        ("initial", 42, "T", "^"),
        ("fns.reduce((previous, fn) => fn(previous), initial)", 63, "T", "^"),
        ("fns.reduce", 63, "{ (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T): (x: T) => T; (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T, initialValue: (x: T) => T): (x: T) => T; <U>(callbackfn: (previousValue: U, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^ ^^          ^^^             ^^^^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^^^            ^^^^^^^^^^"),
        ("fns", 63, "((x: T) => T)[]", "^^ ^^ ^^^^^ ^^^"),
        ("reduce", 67, "{ (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T): (x: T) => T; (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T, initialValue: (x: T) => T): (x: T) => T; <U>(callbackfn: (previousValue: U, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^ ^^          ^^^             ^^^^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^^^            ^^^^^^^^^^"),
        ("(previous, fn) => fn(previous)", 74, "(previous: T, fn: (x: T) => T) => T", "^        ^^^^^  ^^^ ^^ ^^^^^ ^^^^^^"),
        ("previous", 75, "T", "^"),
        ("fn", 85, "(x: T) => T", "^ ^^ ^^^^^ "),
        ("fn(previous)", 92, "T", "^"),
        ("fn", 92, "(x: T) => T", "^ ^^ ^^^^^ "),
        ("previous", 95, "T", "^"),
        ("initial", 106, "T", "^"),
    ], &[
    ]);
}

#[test]
fn array_reduce_generic_accumulator_loose() {
    assert_declarations("function compose<T>(fns: ((x: T) => T)[], initial: T) { return fns.reduce((previous, fn) => fn(previous), initial); }", false, "es2020", &[
        ("compose", 9, "<T>(fns: ((x: T) => T)[], initial: T) => T", "^ ^^   ^^               ^^       ^^ ^^^^^^"),
        ("fns", 20, "((x: T) => T)[]", "^^ ^^ ^^^^^ ^^^"),
        ("x", 27, "T", "^"),
        ("initial", 42, "T", "^"),
        ("fns.reduce((previous, fn) => fn(previous), initial)", 63, "T", "^"),
        ("fns.reduce", 63, "{ (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T): (x: T) => T; (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T, initialValue: (x: T) => T): (x: T) => T; <U>(callbackfn: (previousValue: U, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^ ^^          ^^^             ^^^^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^^^            ^^^^^^^^^^"),
        ("fns", 63, "((x: T) => T)[]", "^^ ^^ ^^^^^ ^^^"),
        ("reduce", 67, "{ (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T): (x: T) => T; (callbackfn: (previousValue: (x: T) => T, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => (x: T) => T, initialValue: (x: T) => T): (x: T) => T; <U>(callbackfn: (previousValue: U, currentValue: (x: T) => T, currentIndex: number, array: ((x: T) => T)[]) => U, initialValue: U): U; }", "^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^          ^^^             ^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^ ^^ ^^^^^ ^^            ^^^ ^^ ^^^^^ ^^^^ ^^ ^^^^^ ^^^ ^^          ^^^             ^^^^^            ^^^ ^^ ^^^^^ ^^            ^^      ^^     ^^^^ ^^ ^^^^^ ^^^^^^^^^^^            ^^^^^^^^^^"),
        ("(previous, fn) => fn(previous)", 74, "(previous: T, fn: (x: T) => T) => T", "^        ^^^^^  ^^^ ^^ ^^^^^ ^^^^^^"),
        ("previous", 75, "T", "^"),
        ("fn", 85, "(x: T) => T", "^ ^^ ^^^^^ "),
        ("fn(previous)", 92, "T", "^"),
        ("fn", 92, "(x: T) => T", "^ ^^ ^^^^^ "),
        ("previous", 95, "T", "^"),
        ("initial", 106, "T", "^"),
    ], &[
    ]);
}

#[test]
fn array_concat_contextual_tuple_strict() {
    assert_declarations("declare const values: [number, number][]; const result = values.concat([[1, 2], [3, 4]]);", true, "es2020", &[
        ("values", 14, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("result", 48, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("values.concat([[1, 2], [3, 4]])", 57, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("values.concat", 57, "{ (...items: ConcatArray<[number, number]>[]): [number, number][]; (...items: ([number, number] | ConcatArray<[number, number]>)[]): [number, number][]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 57, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("concat", 64, "{ (...items: ConcatArray<[number, number]>[]): [number, number][]; (...items: ([number, number] | ConcatArray<[number, number]>)[]): [number, number][]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("[[1, 2], [3, 4]]", 71, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("[1, 2]", 72, "[number, number]", "^^^^^^^^^^^^^^^^"),
        ("1", 73, "1", "^"),
        ("2", 76, "2", "^"),
        ("[3, 4]", 80, "[number, number]", "^^^^^^^^^^^^^^^^"),
        ("3", 81, "3", "^"),
        ("4", 84, "4", "^"),
    ], &[
    ]);
}

#[test]
fn array_concat_contextual_tuple_loose() {
    assert_declarations("declare const values: [number, number][]; const result = values.concat([[1, 2], [3, 4]]);", false, "es2020", &[
        ("values", 14, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("result", 48, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("values.concat([[1, 2], [3, 4]])", 57, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("values.concat", 57, "{ (...items: ConcatArray<[number, number]>[]): [number, number][]; (...items: ([number, number] | ConcatArray<[number, number]>)[]): [number, number][]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("values", 57, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("concat", 64, "{ (...items: ConcatArray<[number, number]>[]): [number, number][]; (...items: ([number, number] | ConcatArray<[number, number]>)[]): [number, number][]; }", "^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("[[1, 2], [3, 4]]", 71, "[number, number][]", "^^^^^^^^^^^^^^^^^^"),
        ("[1, 2]", 72, "[number, number]", "^^^^^^^^^^^^^^^^"),
        ("1", 73, "1", "^"),
        ("2", 76, "2", "^"),
        ("[3, 4]", 80, "[number, number]", "^^^^^^^^^^^^^^^^"),
        ("3", 81, "3", "^"),
        ("4", 84, "4", "^"),
    ], &[
    ]);
}

#[test]
fn boolean_callback_literal_strict() {
    assert_declarations(
        "declare function use(callback: (value: number) => boolean): void; use(value => true);",
        true,
        "es2020",
        &[
            (
                "use",
                17,
                "(callback: (value: number) => boolean) => void",
                "^        ^^                          ^^^^^    ",
            ),
            (
                "callback",
                21,
                "(value: number) => boolean",
                "^     ^^      ^^^^^       ",
            ),
            ("value", 32, "number", "^^^^^^"),
            ("use(value => true)", 66, "void", "^^^^"),
            (
                "use",
                66,
                "(callback: (value: number) => boolean) => void",
                "^        ^^                          ^^^^^    ",
            ),
            (
                "value => true",
                70,
                "(value: number) => true",
                "^     ^^^^^^^^^^^^^^^^^",
            ),
            ("value", 70, "number", "^^^^^^"),
            ("true", 79, "true", "^^^^"),
        ],
        &[],
    );
}

#[test]
fn boolean_callback_literal_loose() {
    assert_declarations(
        "declare function use(callback: (value: number) => boolean): void; use(value => true);",
        false,
        "es2020",
        &[
            (
                "use",
                17,
                "(callback: (value: number) => boolean) => void",
                "^        ^^                          ^^^^^    ",
            ),
            (
                "callback",
                21,
                "(value: number) => boolean",
                "^     ^^      ^^^^^       ",
            ),
            ("value", 32, "number", "^^^^^^"),
            ("use(value => true)", 66, "void", "^^^^"),
            (
                "use",
                66,
                "(callback: (value: number) => boolean) => void",
                "^        ^^                          ^^^^^    ",
            ),
            (
                "value => true",
                70,
                "(value: number) => true",
                "^     ^^^^^^^^^^^^^^^^^",
            ),
            ("value", 70, "number", "^^^^^^"),
            ("true", 79, "true", "^^^^"),
        ],
        &[],
    );
}

#[test]
fn boolean_callback_literal_overloads_strict() {
    assert_declarations("declare function use(callback: (value: number) => boolean): number; declare function use(callback: (value: number) => unknown): string; const result = use(value => true);", true, "es2020", &[
        ("use", 17, "{ (callback: (value: number) => boolean): number; (callback: (value: number) => unknown): string; }", "^^^        ^^                          ^^^      ^^^        ^^                          ^^^      ^^^"),
        ("callback", 21, "(value: number) => boolean", "^     ^^      ^^^^^       "),
        ("value", 32, "number", "^^^^^^"),
        ("use", 85, "{ (callback: (value: number) => boolean): number; (callback: (value: number) => unknown): string; }", "^^^        ^^                          ^^^      ^^^        ^^                          ^^^      ^^^"),
        ("callback", 89, "(value: number) => unknown", "^     ^^      ^^^^^       "),
        ("value", 100, "number", "^^^^^^"),
        ("result", 142, "number", "^^^^^^"),
        ("use(value => true)", 151, "number", "^^^^^^"),
        ("use", 151, "{ (callback: (value: number) => boolean): number; (callback: (value: number) => unknown): string; }", "^^^        ^^                          ^^^      ^^^        ^^                          ^^^      ^^^"),
        ("value => true", 155, "(value: number) => true", "^     ^^^^^^^^^^^^^^^^^"),
        ("value", 155, "number", "^^^^^^"),
        ("true", 164, "true", "^^^^"),
    ], &[
    ]);
}

#[test]
fn boolean_callback_literal_overloads_loose() {
    assert_declarations("declare function use(callback: (value: number) => boolean): number; declare function use(callback: (value: number) => unknown): string; const result = use(value => true);", false, "es2020", &[
        ("use", 17, "{ (callback: (value: number) => boolean): number; (callback: (value: number) => unknown): string; }", "^^^        ^^                          ^^^      ^^^        ^^                          ^^^      ^^^"),
        ("callback", 21, "(value: number) => boolean", "^     ^^      ^^^^^       "),
        ("value", 32, "number", "^^^^^^"),
        ("use", 85, "{ (callback: (value: number) => boolean): number; (callback: (value: number) => unknown): string; }", "^^^        ^^                          ^^^      ^^^        ^^                          ^^^      ^^^"),
        ("callback", 89, "(value: number) => unknown", "^     ^^      ^^^^^       "),
        ("value", 100, "number", "^^^^^^"),
        ("result", 142, "number", "^^^^^^"),
        ("use(value => true)", 151, "number", "^^^^^^"),
        ("use", 151, "{ (callback: (value: number) => boolean): number; (callback: (value: number) => unknown): string; }", "^^^        ^^                          ^^^      ^^^        ^^                          ^^^      ^^^"),
        ("value => true", 155, "(value: number) => true", "^     ^^^^^^^^^^^^^^^^^"),
        ("value", 155, "number", "^^^^^^"),
        ("true", 164, "true", "^^^^"),
    ], &[
    ]);
}

#[test]
fn overload_callback_common_base_strict() {
    assert_declarations("interface Base { base: number; } interface Derived extends Base { child: number; } declare const derived: Derived; declare const base: Base; declare function combine<T>(x: T, fn: (arg: T) => T): T; declare function combine<U>(x: U, fn: (arg: U) => U): U; const result = combine(derived, arg => base);", true, "es2020", &[
        ("base", 17, "number", "^^^^^^"),
        ("child", 66, "number", "^^^^^^"),
        ("derived", 97, "Derived", "^^^^^^^"),
        ("base", 129, "Base", "^^^^"),
        ("combine", 158, "{ <T>(x: T, fn: (arg: T) => T): T; <U>(x: U, fn: (arg: U) => U): U; }", "^^^ ^^ ^^ ^^  ^^             ^^^ ^^^ ^^ ^^ ^^  ^^             ^^^ ^^^"),
        ("x", 169, "T", "^"),
        ("fn", 175, "(arg: T) => T", "^   ^^ ^^^^^ "),
        ("arg", 180, "T", "^"),
        ("combine", 215, "{ <T>(x: T, fn: (arg: T) => T): T; <U>(x: U, fn: (arg: U) => U): U; }", "^^^ ^^ ^^ ^^  ^^             ^^^ ^^^ ^^ ^^ ^^  ^^             ^^^ ^^^"),
        ("x", 226, "U", "^"),
        ("fn", 232, "(arg: U) => U", "^   ^^ ^^^^^ "),
        ("arg", 237, "U", "^"),
        ("result", 261, "Derived", "^^^^^^^"),
        ("combine(derived, arg => base)", 270, "Derived", "^^^^^^^"),
        ("combine", 270, "{ <T>(x: T, fn: (arg: T) => T): T; <U>(x: U, fn: (arg: U) => U): U; }", "^^^ ^^ ^^ ^^  ^^             ^^^ ^^^ ^^ ^^ ^^  ^^             ^^^ ^^^"),
        ("derived", 278, "Derived", "^^^^^^^"),
        ("arg => base", 287, "(arg: Derived) => Base", "^   ^^^^^^^^^^^^^^^^^^"),
        ("arg", 287, "Derived", "^^^^^^^"),
        ("base", 294, "Base", "^^^^"),
    ], &[
        (2769, 294, 4),
    ]);
}

#[test]
fn overload_callback_common_base_loose() {
    assert_declarations("interface Base { base: number; } interface Derived extends Base { child: number; } declare const derived: Derived; declare const base: Base; declare function combine<T>(x: T, fn: (arg: T) => T): T; declare function combine<U>(x: U, fn: (arg: U) => U): U; const result = combine(derived, arg => base);", false, "es2020", &[
        ("base", 17, "number", "^^^^^^"),
        ("child", 66, "number", "^^^^^^"),
        ("derived", 97, "Derived", "^^^^^^^"),
        ("base", 129, "Base", "^^^^"),
        ("combine", 158, "{ <T>(x: T, fn: (arg: T) => T): T; <U>(x: U, fn: (arg: U) => U): U; }", "^^^ ^^ ^^ ^^  ^^             ^^^ ^^^ ^^ ^^ ^^  ^^             ^^^ ^^^"),
        ("x", 169, "T", "^"),
        ("fn", 175, "(arg: T) => T", "^   ^^ ^^^^^ "),
        ("arg", 180, "T", "^"),
        ("combine", 215, "{ <T>(x: T, fn: (arg: T) => T): T; <U>(x: U, fn: (arg: U) => U): U; }", "^^^ ^^ ^^ ^^  ^^             ^^^ ^^^ ^^ ^^ ^^  ^^             ^^^ ^^^"),
        ("x", 226, "U", "^"),
        ("fn", 232, "(arg: U) => U", "^   ^^ ^^^^^ "),
        ("arg", 237, "U", "^"),
        ("result", 261, "Base", "^^^^"),
        ("combine(derived, arg => base)", 270, "Base", "^^^^"),
        ("combine", 270, "{ <T>(x: T, fn: (arg: T) => T): T; <U>(x: U, fn: (arg: U) => U): U; }", "^^^ ^^ ^^ ^^  ^^             ^^^ ^^^ ^^ ^^ ^^  ^^             ^^^ ^^^"),
        ("derived", 278, "Derived", "^^^^^^^"),
        ("arg => base", 287, "(arg: Derived) => Base", "^   ^^^^^^^^^^^^^^^^^^"),
        ("arg", 287, "Derived", "^^^^^^^"),
        ("base", 294, "Base", "^^^^"),
    ], &[
    ]);
}

#[test]
fn union_redeclaration_identity_strict() {
    assert_declarations("interface Base { base: number; } interface Derived extends Base { child: number; } var result: Base | Derived; var result: Base;", true, "es2020", &[
        ("base", 17, "number", "^^^^^^"),
        ("child", 66, "number", "^^^^^^"),
        ("result", 87, "Base | Derived", "^^^^^^^^^^^^^^"),
        ("result", 115, "Base | Derived", "^^^^^^^^^^^^^^"),
    ], &[
        (2403, 115, 6),
    ]);
}

#[test]
fn union_redeclaration_identity_loose() {
    assert_declarations("interface Base { base: number; } interface Derived extends Base { child: number; } var result: Base | Derived; var result: Base;", false, "es2020", &[
        ("base", 17, "number", "^^^^^^"),
        ("child", 66, "number", "^^^^^^"),
        ("result", 87, "Base | Derived", "^^^^^^^^^^^^^^"),
        ("result", 115, "Base | Derived", "^^^^^^^^^^^^^^"),
    ], &[
        (2403, 115, 6),
    ]);
}

#[test]
fn keyof_redeclaration_identity_strict() {
    assert_declarations("type Shape = { a: number; b: string }; var result: \"a\" | \"b\"; var result: keyof Shape;", true, "es2020", &[
        ("Shape", 5, "Shape", "^^^^^"),
        ("a", 15, "number", "^^^^^^"),
        ("b", 26, "string", "^^^^^^"),
        ("result", 43, "\"a\" | \"b\"", "^^^^^^^^^"),
        ("result", 66, "\"a\" | \"b\"", "^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn keyof_redeclaration_identity_loose() {
    assert_declarations("type Shape = { a: number; b: string }; var result: \"a\" | \"b\"; var result: keyof Shape;", false, "es2020", &[
        ("Shape", 5, "Shape", "^^^^^"),
        ("a", 15, "number", "^^^^^^"),
        ("b", 26, "string", "^^^^^^"),
        ("result", 43, "\"a\" | \"b\"", "^^^^^^^^^"),
        ("result", 66, "\"a\" | \"b\"", "^^^^^^^^^"),
    ], &[
    ]);
}
