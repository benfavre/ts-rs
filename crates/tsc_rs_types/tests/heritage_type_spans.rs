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
fn simple_base_strict() {
    assert_declarations("class Base {} class Derived extends Base {} const ctor = Base; const value = new Derived();", true, "es2020", false, &[
        ("Base", 6, "Base", Some("^^^^")),
        ("Derived", 20, "Derived", Some("^^^^^^^")),
        ("Base", 36, "Base", Some("^^^^")),
        ("ctor", 50, "typeof Base", Some("^^^^^^^^^^^")),
        ("Base", 57, "typeof Base", Some("^^^^^^^^^^^")),
        ("value", 69, "Derived", Some("^^^^^^^")),
        ("new Derived()", 77, "Derived", Some("^^^^^^^")),
        ("Derived", 81, "typeof Derived", Some("^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn simple_base_loose() {
    assert_declarations("class Base {} class Derived extends Base {} const ctor = Base; const value = new Derived();", false, "es2020", false, &[
        ("Base", 6, "Base", Some("^^^^")),
        ("Derived", 20, "Derived", Some("^^^^^^^")),
        ("Base", 36, "Base", Some("^^^^")),
        ("ctor", 50, "typeof Base", Some("^^^^^^^^^^^")),
        ("Base", 57, "typeof Base", Some("^^^^^^^^^^^")),
        ("value", 69, "Derived", Some("^^^^^^^")),
        ("new Derived()", 77, "Derived", Some("^^^^^^^")),
        ("Derived", 81, "typeof Derived", Some("^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn generic_base_strict() {
    assert_declarations(
        "class Base<T> {} class Derived extends Base<string> {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "Derived", Some("^^^^^^^")),
            ("Base", 39, "Base<string>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn generic_base_loose() {
    assert_declarations(
        "class Base<T> {} class Derived extends Base<string> {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "Derived", Some("^^^^^^^")),
            ("Base", 39, "Base<string>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn generic_parameter_base_strict() {
    assert_declarations(
        "class Base<T> {} class Derived<U> extends Base<U> {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "Derived<U>", Some("^^^^^^^^^^")),
            ("Base", 42, "Base<U>", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn generic_parameter_base_loose() {
    assert_declarations(
        "class Base<T> {} class Derived<U> extends Base<U> {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "Derived<U>", Some("^^^^^^^^^^")),
            ("Base", 42, "Base<U>", Some("^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn defaulted_base_strict() {
    assert_declarations(
        "class Base<T = string> {} class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 32, "Derived", Some("^^^^^^^")),
            ("Base", 48, "Base<string>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn defaulted_base_loose() {
    assert_declarations(
        "class Base<T = string> {} class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 32, "Derived", Some("^^^^^^^")),
            ("Base", 48, "Base<string>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn dependent_default_base_strict() {
    assert_declarations(
        "class Base<T, U = T[]> {} class Derived extends Base<string> {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T, U>", Some("^^^^^^^^^^")),
            ("Derived", 32, "Derived", Some("^^^^^^^")),
            (
                "Base",
                48,
                "Base<string, string[]>",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn dependent_default_base_loose() {
    assert_declarations(
        "class Base<T, U = T[]> {} class Derived extends Base<string> {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T, U>", Some("^^^^^^^^^^")),
            ("Derived", 32, "Derived", Some("^^^^^^^")),
            (
                "Base",
                48,
                "Base<string, string[]>",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn missing_base_argument_strict() {
    assert_declarations(
        "class Base<T> {} class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "Derived", Some("^^^^^^^")),
            ("Base", 39, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[(2314, 39, 4)],
    );
}

#[test]
fn missing_base_argument_loose() {
    assert_declarations(
        "class Base<T> {} class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "Derived", Some("^^^^^^^")),
            ("Base", 39, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[(2314, 39, 4)],
    );
}

#[test]
fn excess_base_argument_strict() {
    assert_declarations(
        "class Base {} class Derived extends Base<string> {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            ("Derived", 20, "Derived", Some("^^^^^^^")),
            ("Base", 36, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[(2315, 36, 12)],
    );
}

#[test]
fn excess_base_argument_loose() {
    assert_declarations(
        "class Base {} class Derived extends Base<string> {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            ("Derived", 20, "Derived", Some("^^^^^^^")),
            ("Base", 36, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[(2315, 36, 12)],
    );
}

#[test]
fn invalid_base_constraint_strict() {
    assert_declarations(
        "class Base<T extends string> {} class Derived extends Base<number> {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 38, "Derived", Some("^^^^^^^")),
            ("Base", 54, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[(2344, 59, 6)],
    );
}

#[test]
fn invalid_base_constraint_loose() {
    assert_declarations(
        "class Base<T extends string> {} class Derived extends Base<number> {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 38, "Derived", Some("^^^^^^^")),
            ("Base", 54, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[(2344, 59, 6)],
    );
}

#[test]
fn qualified_base_strict() {
    assert_declarations(
        "namespace N { export class Base<T> {} } class Derived extends N.Base<string> {}",
        true,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("Base", 27, "Base<T>", Some("^^^^^^^")),
            ("Derived", 46, "Derived", Some("^^^^^^^")),
            ("N.Base", 62, "N.Base<string>", Some("^^^^^^^^^^^^^^")),
            ("N", 62, "typeof N", Some("^^^^^^^^")),
            ("Base", 64, "typeof N.Base", Some("^^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn qualified_base_loose() {
    assert_declarations(
        "namespace N { export class Base<T> {} } class Derived extends N.Base<string> {}",
        false,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("Base", 27, "Base<T>", Some("^^^^^^^")),
            ("Derived", 46, "Derived", Some("^^^^^^^")),
            ("N.Base", 62, "N.Base<string>", Some("^^^^^^^^^^^^^^")),
            ("N", 62, "typeof N", Some("^^^^^^^^")),
            ("Base", 64, "typeof N.Base", Some("^^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_base_strict() {
    assert_declarations(
        "namespace N { export class Base<T> {} export class Derived extends Base<number> {} }",
        true,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("Base", 27, "Base<T>", Some("^^^^^^^")),
            ("Derived", 51, "Derived", Some("^^^^^^^")),
            ("Base", 67, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn namespace_base_loose() {
    assert_declarations(
        "namespace N { export class Base<T> {} export class Derived extends Base<number> {} }",
        false,
        "es2020",
        false,
        &[
            ("N", 10, "typeof N", Some("^^^^^^^^")),
            ("Base", 27, "Base<T>", Some("^^^^^^^")),
            ("Derived", 51, "Derived", Some("^^^^^^^")),
            ("Base", 67, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn constructor_value_alias_strict() {
    assert_declarations(
        "class Base<T> {} const Alias = Base; class Derived extends Alias<string> {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Alias", 23, "typeof Base", Some("^^^^^^^^^^^")),
            ("Base", 31, "typeof Base", Some("^^^^^^^^^^^")),
            ("Derived", 43, "Derived", Some("^^^^^^^")),
            ("Alias", 59, "Base<string>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn constructor_value_alias_loose() {
    assert_declarations(
        "class Base<T> {} const Alias = Base; class Derived extends Alias<string> {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Alias", 23, "typeof Base", Some("^^^^^^^^^^^")),
            ("Base", 31, "typeof Base", Some("^^^^^^^^^^^")),
            ("Derived", 43, "Derived", Some("^^^^^^^")),
            ("Alias", 59, "Base<string>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn parenthesized_base_strict() {
    assert_declarations(
        "class Base {} class Derived extends (Base) {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            ("Derived", 20, "Derived", Some("^^^^^^^")),
            ("(Base)", 36, "Base", Some("^^^^")),
            ("Base", 37, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn parenthesized_base_loose() {
    assert_declarations(
        "class Base {} class Derived extends (Base) {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            ("Derived", 20, "Derived", Some("^^^^^^^")),
            ("(Base)", 36, "Base", Some("^^^^")),
            ("Base", 37, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn element_base_strict() {
    assert_declarations(
        "class Base {} const group = { Base }; class Derived extends group[\"Base\"] {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            (
                "group",
                20,
                "{ Base: typeof Base; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "{ Base }",
                28,
                "{ Base: typeof Base; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("Base", 30, "typeof Base", Some("^^^^^^^^^^^")),
            ("Derived", 44, "Derived", Some("^^^^^^^")),
            ("group[\"Base\"]", 60, "Base", Some("^^^^")),
            (
                "group",
                60,
                "{ Base: typeof Base; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("\"Base\"", 66, "\"Base\"", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn element_base_loose() {
    assert_declarations(
        "class Base {} const group = { Base }; class Derived extends group[\"Base\"] {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            (
                "group",
                20,
                "{ Base: typeof Base; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            (
                "{ Base }",
                28,
                "{ Base: typeof Base; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("Base", 30, "typeof Base", Some("^^^^^^^^^^^")),
            ("Derived", 44, "Derived", Some("^^^^^^^")),
            ("group[\"Base\"]", 60, "Base", Some("^^^^")),
            (
                "group",
                60,
                "{ Base: typeof Base; }",
                Some("^^^^^^^^^^^^^^^^^^^^^^"),
            ),
            ("\"Base\"", 66, "\"Base\"", Some("^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn called_base_strict() {
    assert_declarations(
        "class Base {} declare function getBase(): typeof Base; class Derived extends getBase() {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            (
                "getBase",
                31,
                "() => typeof Base",
                Some("^^^^^^           "),
            ),
            ("Base", 49, "typeof Base", Some("^^^^^^^^^^^")),
            ("Derived", 61, "Derived", Some("^^^^^^^")),
            ("getBase()", 77, "Base", Some("^^^^")),
            (
                "getBase",
                77,
                "() => typeof Base",
                Some("^^^^^^           "),
            ),
        ],
        &[],
    );
}

#[test]
fn called_base_loose() {
    assert_declarations(
        "class Base {} declare function getBase(): typeof Base; class Derived extends getBase() {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            (
                "getBase",
                31,
                "() => typeof Base",
                Some("^^^^^^           "),
            ),
            ("Base", 49, "typeof Base", Some("^^^^^^^^^^^")),
            ("Derived", 61, "Derived", Some("^^^^^^^")),
            ("getBase()", 77, "Base", Some("^^^^")),
            (
                "getBase",
                77,
                "() => typeof Base",
                Some("^^^^^^           "),
            ),
        ],
        &[],
    );
}

#[test]
fn class_expression_base_strict() {
    assert_declarations(
        "class Base<T> {} const Derived = class extends Base<number> {};",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "typeof Derived", Some("^^^^^^^^^^^^^^")),
            (
                "class extends Base<number> {}",
                33,
                "typeof Derived",
                Some("^^^^^^^^^^^^^^"),
            ),
            ("Base", 47, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn class_expression_base_loose() {
    assert_declarations(
        "class Base<T> {} const Derived = class extends Base<number> {};",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "typeof Derived", Some("^^^^^^^^^^^^^^")),
            (
                "class extends Base<number> {}",
                33,
                "typeof Derived",
                Some("^^^^^^^^^^^^^^"),
            ),
            ("Base", 47, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn named_class_expression_base_strict() {
    assert_declarations(
        "class Base<T> {} const Derived = class Inner extends Base<number> {};",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "typeof Inner", Some("^^^^^^^^^^^^")),
            (
                "class Inner extends Base<number> {}",
                33,
                "typeof Inner",
                Some("^^^^^^^^^^^^"),
            ),
            ("Inner", 39, "typeof Inner", Some("^^^^^^^^^^^^")),
            ("Base", 53, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn named_class_expression_base_loose() {
    assert_declarations(
        "class Base<T> {} const Derived = class Inner extends Base<number> {};",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base<T>", Some("^^^^^^^")),
            ("Derived", 23, "typeof Inner", Some("^^^^^^^^^^^^")),
            (
                "class Inner extends Base<number> {}",
                33,
                "typeof Inner",
                Some("^^^^^^^^^^^^"),
            ),
            ("Inner", 39, "typeof Inner", Some("^^^^^^^^^^^^")),
            ("Base", 53, "Base<number>", Some("^^^^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn any_base_strict() {
    assert_declarations(
        "declare const Base: any; class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 14, "any", None),
            ("Derived", 31, "Derived", Some("^^^^^^^")),
            ("Base", 47, "any", None),
        ],
        &[],
    );
}

#[test]
fn any_base_loose() {
    assert_declarations(
        "declare const Base: any; class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 14, "any", None),
            ("Derived", 31, "Derived", Some("^^^^^^^")),
            ("Base", 47, "any", None),
        ],
        &[],
    );
}

#[test]
fn null_base_strict() {
    assert_declarations(
        "class Derived extends null {}",
        true,
        "es2020",
        false,
        &[("Derived", 6, "Derived", Some("^^^^^^^"))],
        &[],
    );
}

#[test]
fn null_base_loose() {
    assert_declarations(
        "class Derived extends null {}",
        false,
        "es2020",
        false,
        &[("Derived", 6, "Derived", Some("^^^^^^^"))],
        &[],
    );
}

#[test]
fn unknown_base_strict() {
    assert_declarations(
        "declare const Base: unknown; class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 14, "unknown", Some("^^^^^^^")),
            ("Derived", 35, "Derived", Some("^^^^^^^")),
            ("Base", 51, "unknown", Some("^^^^^^^")),
        ],
        &[(2507, 51, 4)],
    );
}

#[test]
fn unknown_base_loose() {
    assert_declarations(
        "declare const Base: unknown; class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 14, "unknown", Some("^^^^^^^")),
            ("Derived", 35, "Derived", Some("^^^^^^^")),
            ("Base", 51, "unknown", Some("^^^^^^^")),
        ],
        &[(2507, 51, 4)],
    );
}

#[test]
fn number_base_strict() {
    assert_declarations(
        "declare const Base: number; class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 14, "number", Some("^^^^^^")),
            ("Derived", 34, "Derived", Some("^^^^^^^")),
            ("Base", 50, "number", Some("^^^^^^")),
        ],
        &[(2507, 50, 4)],
    );
}

#[test]
fn number_base_loose() {
    assert_declarations(
        "declare const Base: number; class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 14, "number", Some("^^^^^^")),
            ("Derived", 34, "Derived", Some("^^^^^^^")),
            ("Base", 50, "number", Some("^^^^^^")),
        ],
        &[(2507, 50, 4)],
    );
}

#[test]
fn constructor_signature_base_strict() {
    assert_declarations("interface Instance { value: string; } declare const Base: new () => Instance; class Derived extends Base {}", true, "es2020", false, &[
        ("value", 21, "string", Some("^^^^^^")),
        ("Base", 52, "new () => Instance", Some("^^^^^^^^^^        ")),
        ("Derived", 84, "Derived", Some("^^^^^^^")),
        ("Base", 100, "Instance", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn constructor_signature_base_loose() {
    assert_declarations("interface Instance { value: string; } declare const Base: new () => Instance; class Derived extends Base {}", false, "es2020", false, &[
        ("value", 21, "string", Some("^^^^^^")),
        ("Base", 52, "new () => Instance", Some("^^^^^^^^^^        ")),
        ("Derived", 84, "Derived", Some("^^^^^^^")),
        ("Base", 100, "Instance", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn generic_constructor_signature_strict() {
    assert_declarations("interface Instance<T> { value: T; } declare const Base: new <T>() => Instance<T>; class Derived extends Base<number> {}", true, "es2020", false, &[
        ("value", 24, "T", Some("^")),
        ("Base", 50, "new <T>() => Instance<T>", Some("^^^^^ ^^^^^^^           ")),
        ("Derived", 88, "Derived", Some("^^^^^^^")),
        ("Base", 104, "Instance<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn generic_constructor_signature_loose() {
    assert_declarations("interface Instance<T> { value: T; } declare const Base: new <T>() => Instance<T>; class Derived extends Base<number> {}", false, "es2020", false, &[
        ("value", 24, "T", Some("^")),
        ("Base", 50, "new <T>() => Instance<T>", Some("^^^^^ ^^^^^^^           ")),
        ("Derived", 88, "Derived", Some("^^^^^^^")),
        ("Base", 104, "Instance<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn defaulted_constructor_signature_strict() {
    assert_declarations("interface Instance<T> { value: T; } declare const Base: new <T = string>() => Instance<T>; class Derived extends Base {}", true, "es2020", false, &[
        ("value", 24, "T", Some("^")),
        ("Base", 50, "new <T = string>() => Instance<T>", Some("^^^^^ ^^^^^^^^^^^^^^^^           ")),
        ("Derived", 97, "Derived", Some("^^^^^^^")),
        ("Base", 113, "Instance<string>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn defaulted_constructor_signature_loose() {
    assert_declarations("interface Instance<T> { value: T; } declare const Base: new <T = string>() => Instance<T>; class Derived extends Base {}", false, "es2020", false, &[
        ("value", 24, "T", Some("^")),
        ("Base", 50, "new <T = string>() => Instance<T>", Some("^^^^^ ^^^^^^^^^^^^^^^^           ")),
        ("Derived", 97, "Derived", Some("^^^^^^^")),
        ("Base", 113, "Instance<string>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn overloaded_constructor_base_strict() {
    assert_declarations("interface Instance<T> { value: T; } interface Factory { new (): Instance<string>; new<T>(): Instance<T>; } declare const Base: Factory; class Derived extends Base<number> {}", true, "es2020", false, &[
        ("value", 24, "T", Some("^")),
        ("Base", 121, "Factory", Some("^^^^^^^")),
        ("Derived", 142, "Derived", Some("^^^^^^^")),
        ("Base", 158, "Instance<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn overloaded_constructor_base_loose() {
    assert_declarations("interface Instance<T> { value: T; } interface Factory { new (): Instance<string>; new<T>(): Instance<T>; } declare const Base: Factory; class Derived extends Base<number> {}", false, "es2020", false, &[
        ("value", 24, "T", Some("^")),
        ("Base", 121, "Factory", Some("^^^^^^^")),
        ("Derived", 142, "Derived", Some("^^^^^^^")),
        ("Base", 158, "Instance<number>", Some("^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn object_constructor_signature_strict() {
    assert_declarations("interface Instance { value: string; } declare const Base: { new(): Instance }; class Derived extends Base {}", true, "es2020", false, &[
        ("value", 21, "string", Some("^^^^^^")),
        ("Base", 52, "new () => Instance", Some("^^^^^^^^^^        ")),
        ("Derived", 85, "Derived", Some("^^^^^^^")),
        ("Base", 101, "Instance", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn object_constructor_signature_loose() {
    assert_declarations("interface Instance { value: string; } declare const Base: { new(): Instance }; class Derived extends Base {}", false, "es2020", false, &[
        ("value", 21, "string", Some("^^^^^^")),
        ("Base", 52, "new () => Instance", Some("^^^^^^^^^^        ")),
        ("Derived", 85, "Derived", Some("^^^^^^^")),
        ("Base", 101, "Instance", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn object_return_base_strict() {
    assert_declarations(
        "declare const Base: new () => { value: string }; class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            (
                "Base",
                14,
                "new () => { value: string; }",
                Some("^^^^^^^^^^                  "),
            ),
            ("value", 32, "string", Some("^^^^^^")),
            ("Derived", 55, "Derived", Some("^^^^^^^")),
            ("Base", 71, "{ value: string; }", Some("^^^^^^^^^      ^^^")),
        ],
        &[],
    );
}

#[test]
fn object_return_base_loose() {
    assert_declarations(
        "declare const Base: new () => { value: string }; class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            (
                "Base",
                14,
                "new () => { value: string; }",
                Some("^^^^^^^^^^                  "),
            ),
            ("value", 32, "string", Some("^^^^^^")),
            ("Derived", 55, "Derived", Some("^^^^^^^")),
            ("Base", 71, "{ value: string; }", Some("^^^^^^^^^      ^^^")),
        ],
        &[],
    );
}

#[test]
fn invalid_return_base_strict() {
    assert_declarations(
        "declare const Base: new () => number; class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 14, "new () => number", Some("^^^^^^^^^^      ")),
            ("Derived", 44, "Derived", Some("^^^^^^^")),
            ("Base", 60, "new () => number", Some("^^^^^^^^^^      ")),
        ],
        &[(2509, 60, 4)],
    );
}

#[test]
fn invalid_return_base_loose() {
    assert_declarations(
        "declare const Base: new () => number; class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 14, "new () => number", Some("^^^^^^^^^^      ")),
            ("Derived", 44, "Derived", Some("^^^^^^^")),
            ("Base", 60, "new () => number", Some("^^^^^^^^^^      ")),
        ],
        &[(2509, 60, 4)],
    );
}

#[test]
fn union_return_base_strict() {
    assert_declarations("declare const Base: new () => ({ left: string } | { right: number }); class Derived extends Base {}", true, "es2020", false, &[
        ("Base", 14, "new () => ({ left: string; } | { right: number; })", Some("^^^^^^^^^^                                        ")),
        ("left", 33, "string", Some("^^^^^^")),
        ("right", 52, "number", Some("^^^^^^")),
        ("Derived", 76, "Derived", Some("^^^^^^^")),
        ("Base", 92, "new () => ({ left: string; } | { right: number; })", Some("^^^^^^^^^^                                        ")),
    ], &[
        (2509, 92, 4),
    ]);
}

#[test]
fn union_return_base_loose() {
    assert_declarations("declare const Base: new () => ({ left: string } | { right: number }); class Derived extends Base {}", false, "es2020", false, &[
        ("Base", 14, "new () => ({ left: string; } | { right: number; })", Some("^^^^^^^^^^                                        ")),
        ("left", 33, "string", Some("^^^^^^")),
        ("right", 52, "number", Some("^^^^^^")),
        ("Derived", 76, "Derived", Some("^^^^^^^")),
        ("Base", 92, "new () => ({ left: string; } | { right: number; })", Some("^^^^^^^^^^                                        ")),
    ], &[
        (2509, 92, 4),
    ]);
}

#[test]
fn intersection_return_base_strict() {
    assert_declarations("declare const Base: new () => ({ left: string } & { right: number }); class Derived extends Base {}", true, "es2020", false, &[
        ("Base", 14, "new () => ({ left: string; } & { right: number; })", Some("^^^^^^^^^^                                        ")),
        ("left", 33, "string", Some("^^^^^^")),
        ("right", 52, "number", Some("^^^^^^")),
        ("Derived", 76, "Derived", Some("^^^^^^^")),
        ("Base", 92, "{ left: string; } & { right: number; }", Some("^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn intersection_return_base_loose() {
    assert_declarations("declare const Base: new () => ({ left: string } & { right: number }); class Derived extends Base {}", false, "es2020", false, &[
        ("Base", 14, "new () => ({ left: string; } & { right: number; })", Some("^^^^^^^^^^                                        ")),
        ("left", 33, "string", Some("^^^^^^")),
        ("right", 52, "number", Some("^^^^^^")),
        ("Derived", 76, "Derived", Some("^^^^^^^")),
        ("Base", 92, "{ left: string; } & { right: number; }", Some("^^^^^^^^      ^^^^^^^^^^^^^^^      ^^^")),
    ], &[
    ]);
}

#[test]
fn abstract_base_strict() {
    assert_declarations(
        "abstract class Base {} class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 15, "Base", Some("^^^^")),
            ("Derived", 29, "Derived", Some("^^^^^^^")),
            ("Base", 45, "Base", Some("^^^^")),
        ],
        &[],
    );
}

#[test]
fn abstract_base_loose() {
    assert_declarations(
        "abstract class Base {} class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 15, "Base", Some("^^^^")),
            ("Derived", 29, "Derived", Some("^^^^^^^")),
            ("Base", 45, "Base", Some("^^^^")),
        ],
        &[],
    );
}

#[test]
fn abstract_constructor_base_strict() {
    assert_declarations("interface Instance { value: string; } declare const Base: abstract new () => Instance; class Derived extends Base {}", true, "es2020", false, &[
        ("value", 21, "string", Some("^^^^^^")),
        ("Base", 52, "abstract new () => Instance", Some("^^^^^^^^^^^^^^^^^^^        ")),
        ("Derived", 93, "Derived", Some("^^^^^^^")),
        ("Base", 109, "Instance", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn abstract_constructor_base_loose() {
    assert_declarations("interface Instance { value: string; } declare const Base: abstract new () => Instance; class Derived extends Base {}", false, "es2020", false, &[
        ("value", 21, "string", Some("^^^^^^")),
        ("Base", 52, "abstract new () => Instance", Some("^^^^^^^^^^^^^^^^^^^        ")),
        ("Derived", 93, "Derived", Some("^^^^^^^")),
        ("Base", 109, "Instance", Some("^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn self_base_strict() {
    assert_declarations(
        "class Base extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            ("Base", 19, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[(2506, 6, 4)],
    );
}

#[test]
fn self_base_loose() {
    assert_declarations(
        "class Base extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Base", 6, "Base", Some("^^^^")),
            ("Base", 19, "typeof Base", Some("^^^^^^^^^^^")),
        ],
        &[(2506, 6, 4)],
    );
}

#[test]
fn circular_base_strict() {
    assert_declarations(
        "class A extends B {} class B extends A {}",
        true,
        "es2020",
        false,
        &[
            ("A", 6, "A", Some("^")),
            ("B", 16, "typeof B", Some("^^^^^^^^")),
            ("B", 27, "B", Some("^")),
            ("A", 37, "typeof A", Some("^^^^^^^^")),
        ],
        &[(2506, 6, 1), (2449, 16, 1), (2506, 27, 1)],
    );
}

#[test]
fn circular_base_loose() {
    assert_declarations(
        "class A extends B {} class B extends A {}",
        false,
        "es2020",
        false,
        &[
            ("A", 6, "A", Some("^")),
            ("B", 16, "typeof B", Some("^^^^^^^^")),
            ("B", 27, "B", Some("^")),
            ("A", 37, "typeof A", Some("^^^^^^^^")),
        ],
        &[(2506, 6, 1), (2449, 16, 1), (2506, 27, 1)],
    );
}

#[test]
fn builtin_error_base_strict() {
    assert_declarations(
        "class Derived extends Error {}",
        true,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            ("Error", 22, "Error", Some("^^^^^")),
        ],
        &[],
    );
}

#[test]
fn builtin_error_base_loose() {
    assert_declarations(
        "class Derived extends Error {}",
        false,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            ("Error", 22, "Error", Some("^^^^^")),
        ],
        &[],
    );
}

#[test]
fn builtin_array_base_strict() {
    assert_declarations(
        "class Derived extends Array<string> {}",
        true,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            ("Array", 22, "string[]", Some("^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn builtin_array_base_loose() {
    assert_declarations(
        "class Derived extends Array<string> {}",
        false,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            ("Array", 22, "string[]", Some("^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn builtin_map_base_strict() {
    assert_declarations(
        "class Derived extends Map<string, number> {}",
        true,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            (
                "Map",
                22,
                "Map<string, number>",
                Some("^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn builtin_map_base_loose() {
    assert_declarations(
        "class Derived extends Map<string, number> {}",
        false,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            (
                "Map",
                22,
                "Map<string, number>",
                Some("^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn builtin_uint8_base_strict() {
    assert_declarations(
        "class Derived extends Uint8Array {}",
        true,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            (
                "Uint8Array",
                22,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn builtin_uint8_base_loose() {
    assert_declarations(
        "class Derived extends Uint8Array {}",
        false,
        "es2020",
        false,
        &[
            ("Derived", 6, "Derived", Some("^^^^^^^")),
            (
                "Uint8Array",
                22,
                "Uint8Array<ArrayBuffer>",
                Some("^^^^^^^^^^^^^^^^^^^^^^^"),
            ),
        ],
        &[],
    );
}

#[test]
fn interface_type_only_base_strict() {
    assert_declarations(
        "interface Base {} class Derived extends Base {}",
        true,
        "es2020",
        false,
        &[
            ("Derived", 24, "Derived", Some("^^^^^^^")),
            ("Base", 40, "any", Some("^^^")),
        ],
        &[(2689, 40, 4)],
    );
}

#[test]
fn interface_type_only_base_loose() {
    assert_declarations(
        "interface Base {} class Derived extends Base {}",
        false,
        "es2020",
        false,
        &[
            ("Derived", 24, "Derived", Some("^^^^^^^")),
            ("Base", 40, "any", Some("^^^")),
        ],
        &[(2689, 40, 4)],
    );
}

#[test]
fn generic_mixin_parameter_strict() {
    assert_declarations("function derive<T extends new (...args: any[]) => {}>(Base: T) { return class extends Base {}; }", true, "es2020", false, &[
        ("derive", 9, "<T extends new (...args: any[]) => {}>(Base: T) => { new (...args: any[]): (Anonymous class); prototype: derive<any>.(Anonymous class); } & T", Some("^ ^^^^^^^^^                          ^^    ^^ ^^^^^^^^^^^^^^^    ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("args", 34, "any[]", Some("^^^^^")),
        ("Base", 54, "T", Some("^")),
        ("class extends Base {}", 72, "{ new (...args: any[]): (Anonymous class); prototype: derive<any>.(Anonymous class); } & T", Some("^^^^^^^^^^    ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("Base", 86, "{}", Some("^^")),
    ], &[
    ]);
}

#[test]
fn generic_mixin_parameter_loose() {
    assert_declarations("function derive<T extends new (...args: any[]) => {}>(Base: T) { return class extends Base {}; }", false, "es2020", false, &[
        ("derive", 9, "<T extends new (...args: any[]) => {}>(Base: T) => { new (...args: any[]): (Anonymous class); prototype: derive<any>.(Anonymous class); } & T", Some("^ ^^^^^^^^^                          ^^    ^^ ^^^^^^^^^^^^^^^    ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("args", 34, "any[]", Some("^^^^^")),
        ("Base", 54, "T", Some("^")),
        ("class extends Base {}", 72, "{ new (...args: any[]): (Anonymous class); prototype: derive<any>.(Anonymous class); } & T", Some("^^^^^^^^^^    ^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^")),
        ("Base", 86, "{}", Some("^^")),
    ], &[
    ]);
}

#[test]
fn type_query_heritage_strict() {
    assert_declarations("class Base {} type Constructor = typeof Base; declare const Alias: Constructor; class Derived extends Alias {}", true, "es2020", false, &[
        ("Base", 6, "Base", Some("^^^^")),
        ("Constructor", 19, "typeof Base", Some("^^^^^^^^^^^")),
        ("Base", 40, "typeof Base", Some("^^^^^^^^^^^")),
        ("Alias", 60, "typeof Base", Some("^^^^^^^^^^^")),
        ("Derived", 86, "Derived", Some("^^^^^^^")),
        ("Alias", 102, "Base", Some("^^^^")),
    ], &[
    ]);
}

#[test]
fn type_query_heritage_loose() {
    assert_declarations("class Base {} type Constructor = typeof Base; declare const Alias: Constructor; class Derived extends Alias {}", false, "es2020", false, &[
        ("Base", 6, "Base", Some("^^^^")),
        ("Constructor", 19, "typeof Base", Some("^^^^^^^^^^^")),
        ("Base", 40, "typeof Base", Some("^^^^^^^^^^^")),
        ("Alias", 60, "typeof Base", Some("^^^^^^^^^^^")),
        ("Derived", 86, "Derived", Some("^^^^^^^")),
        ("Alias", 102, "Base", Some("^^^^")),
    ], &[
    ]);
}

#[test]
fn parameter_default_scope_strict() {
    assert_declarations(
        "var outer = 1; function scoped(x = outer, y = inner) { var inner = \"\"; return y; }",
        true,
        "es2020",
        false,
        &[
            ("outer", 4, "number", Some("^^^^^^")),
            ("1", 12, "1", Some("^")),
            (
                "scoped",
                24,
                "(x?: number, y?: any) => any",
                Some("^ ^^^^^^^^^^^ ^^^^^^^^^^^^^^"),
            ),
            ("x", 31, "number", Some("^^^^^^")),
            ("outer", 35, "number", Some("^^^^^^")),
            ("y", 42, "any", Some("^^^")),
            ("inner", 46, "any", Some("^^^")),
            ("inner", 59, "string", Some("^^^^^^")),
            ("\"\"", 67, "\"\"", Some("^^")),
            ("y", 78, "any", Some("^^^")),
        ],
        &[(2304, 46, 5)],
    );
}

#[test]
fn parameter_default_scope_loose() {
    assert_declarations(
        "var outer = 1; function scoped(x = outer, y = inner) { var inner = \"\"; return y; }",
        false,
        "es2020",
        false,
        &[
            ("outer", 4, "number", Some("^^^^^^")),
            ("1", 12, "1", Some("^")),
            (
                "scoped",
                24,
                "(x?: number, y?: any) => any",
                Some("^ ^^^^^^^^^^^ ^^^^^^^^^^^^^^"),
            ),
            ("x", 31, "number", Some("^^^^^^")),
            ("outer", 35, "number", Some("^^^^^^")),
            ("y", 42, "any", Some("^^^")),
            ("inner", 46, "any", Some("^^^")),
            ("inner", 59, "string", Some("^^^^^^")),
            ("\"\"", 67, "\"\"", Some("^^")),
            ("y", 78, "any", Some("^^^")),
        ],
        &[(2304, 46, 5)],
    );
}

#[test]
fn undefined_query_scope_strict() {
    assert_declarations(
        "var x: typeof undefined; var x: any; var y: typeof undefined;",
        true,
        "es2020",
        false,
        &[
            ("x", 4, "undefined", Some("^^^^^^^^^")),
            ("undefined", 14, "undefined", Some("^^^^^^^^^")),
            ("x", 29, "undefined", Some("^^^^^^^^^")),
            ("y", 41, "undefined", Some("^^^^^^^^^")),
            ("undefined", 51, "undefined", Some("^^^^^^^^^")),
        ],
        &[(2403, 29, 1)],
    );
}

#[test]
fn undefined_query_scope_loose() {
    assert_declarations(
        "var x: typeof undefined; var x: any; var y: typeof undefined;",
        false,
        "es2020",
        false,
        &[
            ("x", 4, "any", None),
            ("undefined", 14, "undefined", Some("^^^^^^^^^")),
            ("x", 29, "any", None),
            ("y", 41, "any", None),
            ("undefined", 51, "undefined", Some("^^^^^^^^^")),
        ],
        &[],
    );
}

#[test]
fn mapped_key_intersection_base_strict() {
    assert_declarations("type Shape<T> = { [K in keyof T & string]: T[K] }; declare const Base: new () => Shape<{ x: string }>; class Derived extends Base {}", true, "es2020", false, &[
        ("Shape", 5, "Shape<T>", Some("^^^^^^^^")),
        ("Base", 65, "new () => Shape<{ x: string; }>", Some("^^^^^^^^^^                     ")),
        ("x", 89, "string", Some("^^^^^^")),
        ("Derived", 109, "Derived", Some("^^^^^^^")),
        ("Base", 125, "Shape<{ x: string; }>", Some("^^^^^^^^^^^      ^^^^")),
    ], &[
    ]);
}

#[test]
fn mapped_key_intersection_base_loose() {
    assert_declarations("type Shape<T> = { [K in keyof T & string]: T[K] }; declare const Base: new () => Shape<{ x: string }>; class Derived extends Base {}", false, "es2020", false, &[
        ("Shape", 5, "Shape<T>", Some("^^^^^^^^")),
        ("Base", 65, "new () => Shape<{ x: string; }>", Some("^^^^^^^^^^                     ")),
        ("x", 89, "string", Some("^^^^^^")),
        ("Derived", 109, "Derived", Some("^^^^^^^")),
        ("Base", 125, "Shape<{ x: string; }>", Some("^^^^^^^^^^^      ^^^^")),
    ], &[
    ]);
}

#[test]
fn library_mapped_base_strict() {
    assert_declarations("type Instance = { x: string }; declare const Base: new () => Partial<Instance>; class Derived extends Base {}", true, "es2020", false, &[
        ("Instance", 5, "Instance", Some("^^^^^^^^")),
        ("x", 18, "string", Some("^^^^^^")),
        ("Base", 45, "new () => Partial<Instance>", Some("^^^^^^^^^^                 ")),
        ("Derived", 86, "Derived", Some("^^^^^^^")),
        ("Base", 102, "Partial<Instance>", Some("^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}

#[test]
fn library_mapped_base_loose() {
    assert_declarations("type Instance = { x: string }; declare const Base: new () => Partial<Instance>; class Derived extends Base {}", false, "es2020", false, &[
        ("Instance", 5, "Instance", Some("^^^^^^^^")),
        ("x", 18, "string", Some("^^^^^^")),
        ("Base", 45, "new () => Partial<Instance>", Some("^^^^^^^^^^                 ")),
        ("Derived", 86, "Derived", Some("^^^^^^^")),
        ("Base", 102, "Partial<Instance>", Some("^^^^^^^^^^^^^^^^^")),
    ], &[
    ]);
}
