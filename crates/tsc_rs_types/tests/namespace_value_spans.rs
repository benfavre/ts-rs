use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

// Original TypeScript 6.0.3 TypeWriterWalker text and synthetic-node masks.
fn assert_declarations(
    source: &str,
    strict: bool,
    expected: &[(&str, usize, &str, &str)],
    omitted: &[(&str, usize)],
) {
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        strict: Some(strict),
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
    for &(anchor, offset) in omitted {
        let span = (offset as u32, (offset + anchor.len()) as u32);
        assert!(
            !output.expression_type_spans.contains_key(&span),
            "unexpected {anchor} at {offset}: {source}"
        );
    }
    let plain = TypeChecker::new().check_with_options(&file, &symbols, &options);
    assert_eq!(output.expression_types, plain.expression_types);
    assert_eq!(
        format!("{:?}", output.diagnostics),
        format!("{:?}", plain.diagnostics)
    );
}

#[test]
fn namespace_value_reference() {
    assert_declarations(
        "namespace N { export const value = 1; } const copy = N;",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("value", 27, "1", "^"),
            ("1", 35, "1", "^"),
            ("copy", 46, "typeof N", "^^^^^^^^"),
            ("N", 53, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn nested_namespace_value_reference() {
    assert_declarations("namespace Outer { export namespace Inner { export const value = 1; } } const copy = Outer.Inner;", true, &[
        ("Outer", 10, "typeof Outer", "^^^^^^^^^^^^"),
        ("Inner", 35, "typeof Inner", "^^^^^^^^^^^^"),
        ("value", 56, "1", "^"),
        ("1", 64, "1", "^"),
        ("copy", 77, "typeof Outer.Inner", "^^^^^^^^^^^^^^^^^^"),
        ("Outer.Inner", 84, "typeof Outer.Inner", "^^^^^^^^^^^^^^^^^^"),
        ("Outer", 84, "typeof Outer", "^^^^^^^^^^^^"),
        ("Inner", 90, "typeof Outer.Inner", "^^^^^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_import_alias_value() {
    assert_declarations(
        "namespace N { export const value = 1; } import Alias = N; const copy = Alias;",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("value", 27, "1", "^"),
            ("1", 35, "1", "^"),
            ("Alias", 47, "typeof N", "^^^^^^^^"),
            ("N", 55, "typeof N", "^^^^^^^^"),
            ("copy", 64, "typeof N", "^^^^^^^^"),
            ("Alias", 71, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn empty_namespace_value() {
    assert_declarations(
        "namespace N {} const copy = N;",
        true,
        &[("copy", 21, "any", "^^^"), ("N", 28, "any", "^^^")],
        &[("N", 10)],
    );
}

#[test]
fn script_global_augmentation() {
    assert_declarations(
        "declare global { interface I {} }",
        false,
        &[("global", 8, "typeof global", "^^^^^^^^^^^^^")],
        &[],
    );
}

#[test]
fn module_global_augmentation() {
    assert_declarations(
        "export {}; declare global { interface I {} }",
        false,
        &[("global", 19, "any", "^^^")],
        &[],
    );
}

#[test]
fn dotted_namespace_value() {
    assert_declarations(
        "namespace A.B { export const x = 1; } const value = A.B;",
        true,
        &[
            ("A", 10, "typeof A", "^^^^^^^^"),
            ("B", 12, "typeof B", "^^^^^^^^"),
            ("x", 29, "1", "^"),
            ("1", 33, "1", "^"),
            ("value", 44, "typeof A.B", "^^^^^^^^^^"),
            ("A.B", 52, "typeof A.B", "^^^^^^^^^^"),
            ("A", 52, "typeof A", "^^^^^^^^"),
            ("B", 54, "typeof A.B", "^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn nested_namespace_local_value() {
    assert_declarations("namespace A { export namespace B { export const x = 1; } export const value = B; } const value = A.value;", true, &[
        ("A", 10, "typeof A", "^^^^^^^^"),
        ("B", 31, "typeof B", "^^^^^^^^"),
        ("x", 48, "1", "^"),
        ("1", 52, "1", "^"),
        ("value", 70, "typeof B", "^^^^^^^^"),
        ("B", 78, "typeof B", "^^^^^^^^"),
        ("value", 89, "typeof A.B", "^^^^^^^^^^"),
        ("A.value", 97, "typeof A.B", "^^^^^^^^^^"),
        ("A", 97, "typeof A", "^^^^^^^^"),
        ("value", 99, "typeof A.B", "^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_self_value() {
    assert_declarations(
        "namespace N { export const x = 1; export const self = N; }",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("x", 27, "1", "^"),
            ("1", 31, "1", "^"),
            ("self", 47, "typeof N", "^^^^^^^^"),
            ("N", 54, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_inner_self_value() {
    assert_declarations(
        "namespace A { export namespace B { export const x = 1; export const self = B; } }",
        true,
        &[
            ("A", 10, "typeof A", "^^^^^^^^"),
            ("B", 31, "typeof B", "^^^^^^^^"),
            ("x", 48, "1", "^"),
            ("1", 52, "1", "^"),
            ("self", 68, "typeof B", "^^^^^^^^"),
            ("B", 75, "typeof B", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_returned_value() {
    assert_declarations(
        "namespace N { export const x = 1; } function value() { return N; } const alias = value;",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("x", 27, "1", "^"),
            ("1", 31, "1", "^"),
            ("value", 45, "() => typeof N", "^^^^^^^^^^^^^^"),
            ("N", 62, "typeof N", "^^^^^^^^"),
            ("alias", 73, "() => typeof N", "^^^^^^^^^^^^^^"),
            ("value", 81, "() => typeof N", "^^^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_nested_return() {
    assert_declarations("namespace A { export namespace B { export const x = 1; } } function value() { return A.B; }", true, &[
        ("A", 10, "typeof A", "^^^^^^^^"),
        ("B", 31, "typeof B", "^^^^^^^^"),
        ("x", 48, "1", "^"),
        ("1", 52, "1", "^"),
        ("value", 68, "() => typeof A.B", "^^^^^^^^^^^^^^^^"),
        ("A.B", 85, "typeof A.B", "^^^^^^^^^^"),
        ("A", 85, "typeof A", "^^^^^^^^"),
        ("B", 87, "typeof A.B", "^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_local_return() {
    assert_declarations("namespace A { export namespace B { export const x = 1; } export function value() { return B; } } const alias = A.value;", true, &[
        ("A", 10, "typeof A", "^^^^^^^^"),
        ("B", 31, "typeof B", "^^^^^^^^"),
        ("x", 48, "1", "^"),
        ("1", 52, "1", "^"),
        ("value", 73, "() => typeof B", "^^^^^^^^^^^^^^"),
        ("B", 90, "typeof B", "^^^^^^^^"),
        ("alias", 103, "() => typeof A.B", "^^^^^^^^^^^^^^^^"),
        ("A.value", 111, "() => typeof A.B", "^^^^^^^^^^^^^^^^"),
        ("A", 111, "typeof A", "^^^^^^^^"),
        ("value", 113, "() => typeof A.B", "^^^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_self_import_value() {
    assert_declarations(
        "namespace N { import Self = N; export const x = 1; export const value = N; }",
        true,
        &[
            ("N", 10, "typeof Self", "^^^^^^^^^^^"),
            ("Self", 21, "typeof Self", "^^^^^^^^^^^"),
            ("N", 28, "typeof Self", "^^^^^^^^^^^"),
            ("x", 44, "1", "^"),
            ("1", 48, "1", "^"),
            ("value", 64, "typeof Self", "^^^^^^^^^^^"),
            ("N", 72, "typeof Self", "^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_ordered_self_aliases() {
    assert_declarations(
        "namespace N { import Z = N; import A = N; export const x = 1; export const value = N; }",
        true,
        &[
            ("N", 10, "typeof Z", "^^^^^^^^"),
            ("Z", 21, "typeof Z", "^^^^^^^^"),
            ("N", 25, "typeof Z", "^^^^^^^^"),
            ("A", 35, "typeof Z", "^^^^^^^^"),
            ("N", 39, "typeof Z", "^^^^^^^^"),
            ("x", 55, "1", "^"),
            ("1", 59, "1", "^"),
            ("value", 75, "typeof Z", "^^^^^^^^"),
            ("N", 83, "typeof Z", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_alias_chain_value() {
    assert_declarations(
        "namespace N { export const x = 1; } import A = B; import B = N; const value = A;",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("x", 27, "1", "^"),
            ("1", 31, "1", "^"),
            ("A", 43, "typeof N", "^^^^^^^^"),
            ("B", 47, "typeof N", "^^^^^^^^"),
            ("B", 57, "typeof N", "^^^^^^^^"),
            ("N", 61, "typeof N", "^^^^^^^^"),
            ("value", 70, "typeof N", "^^^^^^^^"),
            ("A", 78, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_equal_exports() {
    assert_declarations("namespace A { export namespace N { export const x = 1; } } namespace B { export namespace N { export const x = 1; } } const a = A.N; const b = B.N;", true, &[
        ("A", 10, "typeof A", "^^^^^^^^"),
        ("N", 31, "typeof N", "^^^^^^^^"),
        ("x", 48, "1", "^"),
        ("1", 52, "1", "^"),
        ("B", 69, "typeof B", "^^^^^^^^"),
        ("N", 90, "typeof N", "^^^^^^^^"),
        ("x", 107, "1", "^"),
        ("1", 111, "1", "^"),
        ("a", 124, "typeof A.N", "^^^^^^^^^^"),
        ("A.N", 128, "typeof A.N", "^^^^^^^^^^"),
        ("A", 128, "typeof A", "^^^^^^^^"),
        ("N", 130, "typeof A.N", "^^^^^^^^^^"),
        ("b", 139, "typeof B.N", "^^^^^^^^^^"),
        ("B.N", 143, "typeof B.N", "^^^^^^^^^^"),
        ("B", 143, "typeof B", "^^^^^^^^"),
        ("N", 145, "typeof B.N", "^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_array_value() {
    assert_declarations(
        "namespace N { export const x = 1; } const values = [N];",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("x", 27, "1", "^"),
            ("1", 31, "1", "^"),
            ("values", 42, "(typeof N)[]", "^^^^^^^^^^^^"),
            ("[N]", 51, "(typeof N)[]", "^^^^^^^^^^^^"),
            ("N", 52, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_object_value() {
    assert_declarations(
        "namespace N { export const x = 1; } const value = { namespace: N };",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("x", 27, "1", "^"),
            ("1", 31, "1", "^"),
            (
                "value",
                42,
                "{ namespace: typeof N; }",
                "^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            (
                "{ namespace: N }",
                50,
                "{ namespace: typeof N; }",
                "^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("namespace", 52, "typeof N", "^^^^^^^^"),
            ("N", 63, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_shadowed_parameter() {
    assert_declarations("namespace N { export const x = 1; } const value = N; function f(N: number) { return value; }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("x", 27, "1", "^"),
        ("1", 31, "1", "^"),
        ("value", 42, "typeof N", "^^^^^^^^"),
        ("N", 50, "typeof N", "^^^^^^^^"),
        ("f", 62, "(N: number) => typeof globalThis.N", "^ ^^      ^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("N", 64, "number", "^^^^^^"),
        ("value", 84, "typeof globalThis.N", "^^^^^^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_shadowed_external() {
    assert_declarations("export namespace N { export const x = 1; } const value = N; function f(N: number) { return value; }", true, &[
        ("N", 17, "typeof N", "^^^^^^^^"),
        ("x", 34, "1", "^"),
        ("1", 38, "1", "^"),
        ("value", 49, "typeof N", "^^^^^^^^"),
        ("N", 57, "typeof N", "^^^^^^^^"),
        ("f", 69, "(N: number) => typeof import(\"./case\").N", "^ ^^      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"),
        ("N", 71, "number", "^^^^^^"),
        ("value", 91, "typeof import(\"./case\").N", "^^^^^^^^^^^^^^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_type_query() {
    assert_declarations(
        "namespace N { export const x = 1; } let value: typeof N = N;",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("x", 27, "1", "^"),
            ("1", 31, "1", "^"),
            ("value", 40, "typeof N", "^^^^^^^^"),
            ("N", 54, "typeof N", "^^^^^^^^"),
            ("N", 58, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn import_equals_invalid_type() {
    assert_declarations(
        "interface I {} import Alias = I; const copy = Alias;",
        true,
        &[
            ("Alias", 22, "any", "^^^"),
            ("I", 30, "any", "^^^"),
            ("copy", 39, "any", "^^^"),
            ("Alias", 46, "any", "^^^"),
        ],
        &[],
    );
}

#[test]
fn import_equals_class() {
    assert_declarations(
        "class C {} import Alias = C; const copy = Alias;",
        true,
        &[
            ("C", 6, "C", "^"),
            ("Alias", 18, "any", "^^^"),
            ("C", 26, "any", "^^^"),
            ("copy", 35, "any", "^^^"),
            ("Alias", 42, "any", "^^^"),
        ],
        &[],
    );
}

#[test]
fn import_equals_required_unknown() {
    assert_declarations(
        "import Alias = require(\"missing\"); const copy = Alias;",
        true,
        &[
            ("Alias", 7, "any", "^^^"),
            ("copy", 41, "any", "^^^"),
            ("Alias", 48, "any", "^^^"),
        ],
        &[],
    );
}

#[test]
fn import_equals_exported_namespace() {
    assert_declarations(
        "namespace N { export const x = 1; } export import Alias = N;",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("x", 27, "1", "^"),
            ("1", 31, "1", "^"),
            ("Alias", 50, "typeof N", "^^^^^^^^"),
            ("N", 58, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn nested_global_augmentation() {
    assert_declarations(
        "namespace N { declare global { interface I {} } }",
        false,
        &[("global", 22, "typeof global", "^^^^^^^^^^^^^")],
        &[("N", 10)],
    );
}

#[test]
fn ambient_global_augmentation() {
    assert_declarations(
        "declare module \"m\" { global { interface I {} } }",
        false,
        &[
            ("\"m\"", 15, "typeof import(\"m\")", "^^^^^^^^^^^^^^^^^^"),
            ("global", 21, "any", "^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_function_variable_conflict() {
    assert_declarations(
        "namespace N { var f; function f() {} }",
        false,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("f", 18, "any", "^^^"),
            ("f", 30, "() => void", "^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn local_function_variable_conflict() {
    assert_declarations(
        "function outer() { var f; function f() {} }",
        false,
        &[
            ("outer", 9, "() => void", "^^^^^^^^^^"),
            ("f", 23, "any", "^^^"),
            ("f", 35, "() => void", "^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn function_annotation_provenance() {
    assert_declarations(
        "function declared(): number { return 1; } function inferred() { return 2; }",
        true,
        &[
            ("declared", 9, "() => number", "^^^^^^      "),
            ("1", 37, "1", "^"),
            ("inferred", 51, "() => number", "^^^^^^^^^^^^"),
            ("2", 71, "2", "^"),
        ],
        &[],
    );
}

#[test]
fn namespace_function_annotation_provenance() {
    assert_declarations("namespace N { export function declared(): number { return 1; } export function inferred() { return 2; } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("declared", 30, "() => number", "^^^^^^      "),
        ("1", 58, "1", "^"),
        ("inferred", 79, "() => number", "^^^^^^^^^^^^"),
        ("2", 99, "2", "^"),
    ], &[
    ]);
}
