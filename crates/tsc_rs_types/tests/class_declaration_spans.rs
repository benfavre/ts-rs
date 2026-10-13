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
fn namespace_members() {
    assert_declarations("namespace N { export class C { value = 1; read() { return this.value; } get size() { return this.value; } set size(value:number) {} } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("value", 31, "number", "^^^^^^"),
        ("read", 42, "() => number", "^^^^^^^^^^^^"),
        ("size", 76, "number", "^^^^^^"),
        ("size", 110, "number", "^^^^^^"),
    ], &[
    ]);
}

#[test]
fn dotted_namespace() {
    assert_declarations(
        "namespace A.B { export class C { text = \"\"; read() { return this.text; } } }",
        true,
        &[
            ("A", 10, "typeof A", "^^^^^^^^"),
            ("B", 12, "typeof B", "^^^^^^^^"),
            ("text", 33, "string", "^^^^^^"),
            ("read", 44, "() => string", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn nested_class() {
    assert_declarations(
        "function outer() { class C { value = 1; read() { return this.value; } } return C; }",
        true,
        &[
            ("value", 29, "number", "^^^^^^"),
            ("read", 40, "() => number", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn block_class() {
    assert_declarations(
        "{ class C { value: number = 1; method(): string { return \"\"; } } }",
        true,
        &[
            ("value", 12, "number", "^^^^^^"),
            ("method", 31, "() => string", "^^^^^^      "),
        ],
        &[],
    );
}

#[test]
fn anonymous_class_expression() {
    assert_declarations(
        "const C = class { value = 1; read() { return this.value; } };",
        true,
        &[
            ("value", 18, "number", "^^^^^^"),
            ("read", 29, "() => number", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn named_class_expression() {
    assert_declarations(
        "const C = class Inner { value = 1; read() { return this.value; } };",
        true,
        &[
            ("value", 24, "number", "^^^^^^"),
            ("read", 35, "() => number", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn generic_namespace_class() {
    assert_declarations("namespace N { export class C<T> { value!:T; read() { return this.value; } map<U extends T = T>(value:U):U { return value; } } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("value", 34, "T", "^"),
        ("read", 44, "() => T", "^^^^^^^"),
        ("map", 74, "<U extends T = T>(value: U) => U", "^ ^^^^^^^^^ ^^^^^^     ^^ ^^^^^ "),
    ], &[
    ]);
}

#[test]
fn static_namespace_members() {
    assert_declarations("namespace N { export class C { static value = 1; static read() { return this.value; } static get size() { return this.value; } } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("value", 38, "number", "^^^^^^"),
        ("read", 56, "() => number", "^^^^^^^^^^^^"),
        ("size", 97, "number", "^^^^^^"),
    ], &[
    ]);
}

#[test]
fn readonly_literal_members() {
    assert_declarations("namespace N { class C { readonly text = \"hello\"; readonly count = 1; readonly flag = true; } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("text", 33, "\"hello\"", "^^^^^^^"),
        ("count", 58, "1", "^"),
        ("flag", 78, "true", "^^^^"),
    ], &[
    ]);
}

#[test]
fn optional_class_members() {
    assert_declarations(
        "class C { field?: string; method?():number; }",
        true,
        &[
            ("field", 10, "string | undefined", "^^^^^^^^^^^^^^^^^^"),
            (
                "method",
                26,
                "(() => number) | undefined",
                "^^^^^^^      ^^^^^^^^^^^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn optional_members_without_strict_nulls() {
    assert_declarations(
        "class C { field?: string; method?():number; }",
        false,
        &[
            ("field", 10, "string", "^^^^^^"),
            ("method", 26, "() => number", "^^^^^^      "),
        ],
        &[],
    );
}

#[test]
fn method_overloads() {
    assert_declarations(
        "class C { f(x:number):string; f(x:string):number; f(x:any):any { return x; } }",
        true,
        &[
            (
                "f",
                10,
                "{ (x: number): string; (x: string): number; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
            (
                "f",
                30,
                "{ (x: number): string; (x: string): number; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
            (
                "f",
                50,
                "{ (x: number): string; (x: string): number; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn identical_method_overloads() {
    assert_declarations(
        "class C { f(x:number):string; f(x:number):string; f(x:any):any { return x; } }",
        true,
        &[
            (
                "f",
                10,
                "{ (x: number): string; (x: number): string; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
            (
                "f",
                30,
                "{ (x: number): string; (x: number): string; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
            (
                "f",
                50,
                "{ (x: number): string; (x: number): string; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
        ],
        &[],
    );
}

#[test]
fn namespace_method_overloads() {
    assert_declarations("namespace N { class C { f(x:number):string; f(x:string):number; f(x:any):any { return x; } } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("f", 24, "{ (x: number): string; (x: string): number; }", "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^"),
        ("f", 44, "{ (x: number): string; (x: string): number; }", "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^"),
        ("f", 64, "{ (x: number): string; (x: string): number; }", "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^"),
    ], &[
    ]);
}

#[test]
fn static_method_overloads() {
    assert_declarations("class C { static f(x:number):string; static f(x:string):number; static f(x:any):any { return x; } }", true, &[
        ("f", 17, "{ (x: number): string; (x: string): number; }", "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^"),
        ("f", 44, "{ (x: number): string; (x: string): number; }", "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^"),
        ("f", 71, "{ (x: number): string; (x: string): number; }", "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^"),
    ], &[
    ]);
}

#[test]
fn single_method_overload() {
    assert_declarations(
        "class C { f(x:number):string; f(x:any):any { return x; } }",
        true,
        &[
            ("f", 10, "(x: number) => string", "^ ^^      ^^^^^      "),
            ("f", 30, "(x: number) => string", "^ ^^      ^^^^^      "),
        ],
        &[],
    );
}

#[test]
fn getter_and_setter() {
    assert_declarations("namespace N { class C { get value():number { return 1; } set value(x:string | number) {} } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("value", 28, "number", "^^^^^^"),
        ("value", 61, "number", "^^^^^^"),
    ], &[
    ]);
}

#[test]
fn setter_only() {
    assert_declarations(
        "namespace N { class C { set value(x:string) {} } }",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("value", 28, "string", "^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn literal_member_names() {
    assert_declarations("namespace N { class C { \"quoted\"():string { return \"\"; } 42():boolean { return true; } [\"value\"] = 1; } }", true, &[
        ("N", 10, "typeof N", "^^^^^^^^"),
        ("\"quoted\"", 24, "() => string", "^^^^^^      "),
        ("42", 57, "() => boolean", "^^^^^^       "),
        ("[\"value\"]", 87, "number", "^^^^^^"),
    ], &[
    ]);
}

#[test]
fn private_member_names() {
    assert_declarations(
        "namespace N { class C { #value = 1; #read() { return this.#value; } } }",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("#value", 24, "number", "^^^^^^"),
            ("#read", 36, "() => number", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_class_collisions() {
    assert_declarations("namespace A { class C { value = 1; read() { return this.value; } } } namespace B { class C { value = \"\"; read() { return this.value; } } }", true, &[
        ("A", 10, "typeof A", "^^^^^^^^"),
        ("value", 24, "number", "^^^^^^"),
        ("read", 35, "() => number", "^^^^^^^^^^^^"),
        ("B", 79, "typeof B", "^^^^^^^^"),
        ("value", 93, "string", "^^^^^^"),
        ("read", 105, "() => string", "^^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn function_namespace_merge() {
    assert_declarations(
        "function f(value:number):string { return \"\"; } namespace f { export const value = 1; }",
        true,
        &[("f", 57, "typeof f", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn class_namespace_merge() {
    assert_declarations(
        "class C {} namespace C { export const value = 1; }",
        true,
        &[("C", 21, "typeof C", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn enum_namespace_merge() {
    assert_declarations(
        "enum E { A } namespace E { export const value = 1; }",
        true,
        &[("E", 23, "typeof E", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn type_only_namespace() {
    assert_declarations(
        "namespace N { export interface I {} }",
        true,
        &[],
        &[("N", 10)],
    );
}

#[test]
fn ambient_external_module() {
    assert_declarations(
        "declare module \"pkg\" { export class C { value:number; read():string; } }",
        true,
        &[
            (
                "\"pkg\"",
                15,
                "typeof import(\"pkg\")",
                "^^^^^^^^^^^^^^^^^^^^",
            ),
            ("value", 40, "number", "^^^^^^"),
            ("read", 54, "() => string", "^^^^^^      "),
        ],
        &[],
    );
}

#[test]
fn namespace_alias() {
    assert_declarations(
        "namespace N { export namespace Inner { export const value = 1; } }",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("Inner", 31, "typeof Inner", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn empty_namespace() {
    assert_declarations("namespace N {}", true, &[], &[("N", 10)]);
}

#[test]
fn semicolon_namespace() {
    assert_declarations(
        "namespace N { ; }",
        true,
        &[("N", 10, "typeof N", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn expression_namespace() {
    assert_declarations(
        "namespace N { 1; }",
        true,
        &[("N", 10, "typeof N", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn const_enum_namespace() {
    assert_declarations(
        "namespace N { export const enum E { A } }",
        true,
        &[],
        &[("N", 10)],
    );
}

#[test]
fn nested_type_namespace() {
    assert_declarations(
        "namespace A.B { export interface I {} }",
        true,
        &[],
        &[("A", 10), ("B", 12)],
    );
}

#[test]
fn type_only_function_namespace_merge() {
    assert_declarations(
        "function f() {} namespace f { export interface I {} }",
        true,
        &[],
        &[("f", 26)],
    );
}

#[test]
fn merged_namespace_blocks() {
    assert_declarations(
        "namespace N { export const value = 1; } namespace N { export interface I {} }",
        true,
        &[("N", 10, "typeof N", "^^^^^^^^")],
        &[("N", 50)],
    );
}

#[test]
fn ambient_type_only_module() {
    assert_declarations(
        "declare module \"pkg\" { export interface I {} }",
        true,
        &[(
            "\"pkg\"",
            15,
            "typeof import(\"pkg\")",
            "^^^^^^^^^^^^^^^^^^^^",
        )],
        &[],
    );
}

#[test]
fn namespace_import_alias() {
    assert_declarations(
        "namespace A { export const value = 1; } namespace B { import C = A; }",
        true,
        &[("A", 10, "typeof A", "^^^^^^^^")],
        &[("B", 50)],
    );
}

#[test]
fn namespace_export_import_alias() {
    assert_declarations(
        "namespace A { export const value = 1; } namespace B { export import C = A; }",
        true,
        &[
            ("A", 10, "typeof A", "^^^^^^^^"),
            ("B", 50, "typeof B", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_exports_local_interface() {
    assert_declarations(
        "namespace N { interface I {} export { I }; }",
        true,
        &[],
        &[("N", 10)],
    );
}

#[test]
fn namespace_exports_outer_interface() {
    assert_declarations(
        "interface I {} namespace N { export { I }; }",
        true,
        &[],
        &[("N", 25)],
    );
}

#[test]
fn namespace_exports_outer_value() {
    assert_declarations(
        "const value = 1; namespace N { export { value }; }",
        true,
        &[("N", 27, "typeof N", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_exports_forward_value() {
    assert_declarations(
        "namespace N { export { value }; const value = 1; }",
        true,
        &[("N", 10, "typeof N", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_exports_import_alias() {
    assert_declarations(
        "namespace N { import x = N; export { x }; }",
        true,
        &[("N", 10, "typeof x", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_exports_cycle() {
    assert_declarations("namespace N { export { N }; }", true, &[], &[("N", 10)]);
}

#[test]
fn namespace_exports_nested_type_module() {
    assert_declarations(
        "namespace N { namespace Inner { interface I {} } export { Inner }; }",
        true,
        &[],
        &[("N", 10), ("Inner", 24)],
    );
}

#[test]
fn namespace_exports_const_enum() {
    assert_declarations(
        "namespace N { const enum E { A } export { E }; }",
        true,
        &[],
        &[("N", 10)],
    );
}

#[test]
fn namespace_export_alias_lexical_shadow() {
    assert_declarations(
        "const Value = 1; namespace A { interface Value {} namespace B { export { Value }; } }",
        true,
        &[],
        &[("A", 27), ("B", 60)],
    );
}

#[test]
fn default_anonymous_class() {
    assert_declarations(
        "export default class { value = 1; read() { return this.value; } }",
        true,
        &[
            ("value", 23, "number", "^^^^^^"),
            ("read", 34, "() => number", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn default_anonymous_generic_class() {
    assert_declarations(
        "export default class<T> { value!:T; read() { return this.value; } }",
        true,
        &[("value", 26, "T", "^"), ("read", 36, "() => T", "^^^^^^^")],
        &[],
    );
}

#[test]
fn named_generic_class_expression() {
    assert_declarations(
        "const C = class Inner<T> { value!:T; read() { return this.value; } };",
        true,
        &[
            ("Inner", 16, "typeof Inner", "^^^^^^^^^^^^"),
            ("value", 27, "T", "^"),
            ("read", 37, "() => T", "^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn named_default_class() {
    assert_declarations(
        "export default class C { value = 1; read() { return this.value; } }",
        true,
        &[
            ("C", 21, "C", "^"),
            ("value", 25, "number", "^^^^^^"),
            ("read", 36, "() => number", "^^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn escaped_namespace_name() {
    assert_declarations(
        "namespace \\u004E { export const value = 1; }",
        true,
        &[("\\u004E", 10, "typeof \\u004E", "^^^^^^^^^^^^^")],
        &[],
    );
}

#[test]
fn global_augmentation() {
    assert_declarations(
        "export {}; declare global { interface I {} }",
        true,
        &[("global", 19, "any", "^^^")],
        &[],
    );
}

#[test]
fn ambient_global_namespace() {
    assert_declarations(
        "declare namespace global { const value: number; }",
        true,
        &[("global", 18, "typeof global", "^^^^^^^^^^^^^")],
        &[],
    );
}

#[test]
fn method_getter_name_conflict() {
    assert_declarations(
        "class C { method() {} get method() { return 1; } }",
        true,
        &[
            ("C", 6, "C", "^"),
            ("method", 10, "() => void", "^^^^^^^^^^"),
            ("method", 26, "number", "^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_self_alias() {
    assert_declarations(
        "namespace N { import Self = N; export const value = 1; }",
        true,
        &[("N", 10, "typeof Self", "^^^^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_chained_self_alias() {
    assert_declarations(
        "namespace N { import Self = Next; import Next = N; export const value = 1; }",
        true,
        &[("N", 10, "typeof Self", "^^^^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_cyclic_alias() {
    assert_declarations(
        "namespace N { import A = B; import B = A; export const value = 1; }",
        true,
        &[("N", 10, "typeof N", "^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_shadowed_self_alias() {
    assert_declarations("namespace N { namespace N { export const value = 1; } import Self = N; export const other = 2; }", true, &[
        ("N", 10, "typeof globalThis.N", "^^^^^^^^^^^^^^^^^^^"),
        ("N", 24, "typeof N", "^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_qualified_self_alias() {
    assert_declarations("namespace Outer { export namespace Inner { import Self = Outer.Inner; export const value = 1; } }", true, &[
        ("Outer", 10, "typeof Outer", "^^^^^^^^^^^^"),
        ("Inner", 35, "typeof Self", "^^^^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_dotted_self_alias() {
    assert_declarations(
        "namespace Outer.Inner { import Self = Outer.Inner; export const value = 1; }",
        true,
        &[
            ("Outer", 10, "typeof Outer", "^^^^^^^^^^^^"),
            ("Inner", 16, "typeof Self", "^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_alias_to_other() {
    assert_declarations(
        "namespace A { export const x = 1; } namespace B { import Other = A; export const y = 2; }",
        true,
        &[
            ("A", 10, "typeof A", "^^^^^^^^"),
            ("B", 46, "typeof B", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_exported_self_alias() {
    assert_declarations(
        "namespace N { export import Self = N; export const value = 1; }",
        true,
        &[("N", 10, "typeof Self", "^^^^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_merged_self_alias() {
    assert_declarations(
        "namespace N { export const x = 1; } namespace N { import Self = N; export const y = 2; }",
        true,
        &[
            ("N", 10, "typeof N", "^^^^^^^^"),
            ("N", 46, "typeof Self", "^^^^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_external_shadow() {
    assert_declarations(
        "export namespace N { namespace N { export const value = 1; } export const other = 2; }",
        true,
        &[
            (
                "N",
                17,
                "typeof import(\"./case\").N",
                "^^^^^^^^^^^^^^^^^^^^^^^^^",
            ),
            ("N", 31, "typeof N", "^^^^^^^^"),
        ],
        &[],
    );
}

#[test]
fn namespace_nested_shadow() {
    assert_declarations("namespace Outer { export namespace N { namespace N { export const value = 1; } export const other = 2; } }", true, &[
        ("Outer", 10, "typeof Outer", "^^^^^^^^^^^^"),
        ("N", 35, "typeof Outer.N", "^^^^^^^^^^^^^^"),
        ("N", 49, "typeof N", "^^^^^^^^"),
    ], &[
    ]);
}

#[test]
fn namespace_value_shadow() {
    assert_declarations(
        "namespace N { const N = 1; export const value = 2; }",
        true,
        &[("N", 10, "typeof globalThis.N", "^^^^^^^^^^^^^^^^^^^")],
        &[],
    );
}

#[test]
fn namespace_type_shadow() {
    assert_declarations(
        "namespace N { interface N {} export const value = 2; }",
        true,
        &[("N", 10, "typeof N", "^^^^^^^^")],
        &[],
    );
}
