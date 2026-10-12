use tsc_rs_types::TypeChecker;

// TypeScript 6.0.3 TypeWriterWalker outputs, including synthetic-node masks.
fn assert_node(source: &str, anchor: &str, name: &str, text: &str, underline: &str) {
    assert_node_with_options(source, anchor, name, text, underline, false);
}
fn assert_node_with_options(
    source: &str,
    anchor: &str,
    name: &str,
    text: &str,
    underline: &str,
    strict: bool,
) {
    let file = tsc_rs_parser::parse("case.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let options = tsc_rs_ast::CompilerOptions {
        strict: Some(strict),
        ..Default::default()
    };
    let result = checker.check_with_options(&file, &symbols, &options);
    let start = source.find(anchor).unwrap() as u32;
    let entry = result
        .expression_type_spans
        .get(&(start, start + name.len() as u32))
        .unwrap();
    assert_eq!(entry.node_text, text, "{source}");
    assert_eq!(entry.underline, underline, "{source}\n{text}");
    assert_eq!(text.encode_utf16().count(), underline.len());
}

#[test]
fn optional_and_readonly_properties() {
    let source = "declare const x: { a: number; b?: string; readonly c: boolean }; x;";
    assert_node(
        source,
        "x:",
        "x",
        "{ a: number; b?: string; readonly c: boolean; }",
        "^^^^^      ^^^^^^      ^^^^^^^^^^^^^^       ^^^",
    );
}

#[test]
fn nested_property_annotations_and_aliases() {
    let source = "type N = number; declare let x: { a: N; nested: { b: string }; fn: (n:number) => string }; x;";
    assert_node(
        source,
        "x:",
        "x",
        "{ a: N; nested: { b: string; }; fn: (n: number) => string; }",
        "^^^^^ ^^^^^^^^^^              ^^^^^^                     ^^^",
    );
}

#[test]
fn string_index_signatures_are_synthetic() {
    let source = "declare const x: { [key: string]: number }; x;";
    assert_node(
        source,
        "x:",
        "x",
        "{ [key: string]: number; }",
        "^^^^^^^^^^^^^^^^^^^^^^^^^^",
    );
}

#[test]
fn number_index_signatures_are_synthetic() {
    let source = "declare const x: { [index: number]: string }; x;";
    assert_node(
        source,
        "x:",
        "x",
        "{ [index: number]: string; }",
        "^^^^^^^^^^^^^^^^^^^^^^^^^^^^",
    );
}

#[test]
fn accessor_signatures_reuse_annotations() {
    let source = "declare const x: { get a(): number; set a(value: number); }; x;";
    assert_node(source, "x:", "x", "{ a: number; }", "^^^^^      ^^^");
}

#[test]
fn object_accessors_reuse_annotations() {
    let source = "const x = { get a(): number { return 1; }, set a(value: number) {} }; x;";
    assert_node(source, "x =", "x", "{ a: number; }", "^^^^^      ^^^");
}

#[test]
fn instantiated_properties_synthesize_changed_annotations() {
    let source = "type Box<T> = { value: T }; declare const x: Box<number>; const y = { ...x };";
    assert_node(
        source,
        "y =",
        "y",
        "{ value: number; }",
        "^^^^^^^^^^^^^^^^^^",
    );
}

#[test]
fn quoted_property_names_preserve_original_quote_style() {
    let source = "declare const x: { 'one': number; \"two words\": string; 3: boolean }; x;";
    assert_node(
        source,
        "x:",
        "x",
        "{ one: number; \"two words\": string; 3: boolean; }",
        "^^^^^^^      ^^^^^^^^^^^^^^^      ^^^^^       ^^^",
    );
}

#[test]
fn interface_spreads_reuse_member_annotations() {
    let source = "interface I { a: number; b: string; } declare let x: I; const y = { ...x }; y;";
    assert_node(
        source,
        "y =",
        "y",
        "{ a: number; b: string; }",
        "^^^^^      ^^^^^      ^^^",
    );
}

#[test]
fn annotated_spreads_reuse_member_annotations() {
    let source = "const x: { a: number } = {a: 1}; const y = { ...x }; y;";
    assert_node(source, "y =", "y", "{ a: number; }", "^^^^^      ^^^");
}

#[test]
fn returned_objects_reuse_member_annotations() {
    let source = "function f(): { a: number } { return { a: 1 }; } const x = f();";
    assert_node(source, "x =", "x", "{ a: number; }", "^^^^^      ^^^");
}

#[test]
fn multiline_members_print_flat() {
    let source = "declare const x: {\n a: number;\n b: string;\n}; x;";
    assert_node(
        source,
        "x:",
        "x",
        "{ a: number; b: string; }",
        "^^^^^      ^^^^^      ^^^",
    );
}

#[test]
fn spread_overrides_replace_annotation_provenance() {
    let source = "declare const a: { value: number }; const x = { ...a, value: 1 }; const y = {value: 2, ...a};";
    assert_node(
        source,
        "x =",
        "x",
        "{ value: number; }",
        "^^^^^^^^^^^^^^^^^^",
    );
    assert_node(
        source,
        "y =",
        "y",
        "{ value: number; }",
        "^^^^^^^^^      ^^^",
    );
}

#[test]
fn properties_override_spread_method_origin() {
    let source =
        "const a = { f(x: number) { return x; } }; const x = { ...a, f: (y: string) => y };";
    assert_node(
        source,
        "x =",
        "x",
        "{ f: (y: string) => string; }",
        "^^^^^^ ^^      ^^^^^^^^^^^^^^",
    );
}

#[test]
fn spreads_override_direct_method_origin() {
    let source =
        "const a = { f: (x: number) => x }; const x = { f(y: string) { return y; }, ...a };";
    assert_node(
        source,
        "x =",
        "x",
        "{ f: (x: number) => number; }",
        "^^^^^^ ^^      ^^^^^^^^^^^^^^",
    );
}

#[test]
fn readonly_index_signatures_are_synthetic() {
    let source = "declare const a: { readonly [s: string]: number }; a;";
    assert_node(
        source,
        "a:",
        "a",
        "{ readonly [s: string]: number; }",
        "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^",
    );
}

#[test]
fn setter_annotation_can_supply_inferred_getter() {
    let source = "const x = { get a() { return 1; }, set a(value: number) {} };";
    assert_node(source, "x =", "x", "{ a: number; }", "^^^^^      ^^^");
}

#[test]
fn setter_only_signature_reuses_parameter_annotation() {
    let source = "declare const x: { set a(value: number); };";
    assert_node(source, "x:", "x", "{ a: number; }", "^^^^^      ^^^");
}

#[test]
fn generic_substitution_preserves_only_unchanged_annotations() {
    let source =
        "type Box<T> = { a: number; b: T }; declare const box: Box<string>; const x = { ...box };";
    assert_node(
        source,
        "x =",
        "x",
        "{ a: number; b: string; }",
        "^^^^^      ^^^^^^^^^^^^^^",
    );
}

#[test]
fn string_literals_normalize_quotes_and_escape_unicode() {
    let source = "declare const x: { 'bad-name': 'hello'; '😀': number; }; x;";
    assert_node(
        source,
        "x:",
        "x",
        "{ 'bad-name': \"hello\"; '\\uD83D\\uDE00': number; }",
        "^^^^^^^^^^^^^^       ^^^^^^^^^^^^^^^^^^      ^^^",
    );
}

#[test]
fn inherited_annotations_survive_partial_instantiation() {
    assert_node_with_options("interface Base { a: number; } interface Child<T> extends Base { b: T; } declare const child: Child<string>; const x = {...child};", "x =", "x", "{ b: string; a: number; }", "^^^^^^^^^^^^^^^^      ^^^", false);
}

#[test]
fn own_interface_member_replaces_inherited_annotation() {
    assert_node_with_options("interface Base { a: number; } interface Child extends Base { a: string; } declare const child: Child; const x = {...child};", "x =", "x", "{ a: string; }", "^^^^^      ^^^", false);
}

#[test]
fn strict_optional_properties_reuse_annotations() {
    assert_node_with_options(
        "declare const x: { a?: number; b?: string | undefined; c?: number | string }; x;",
        "x:",
        "x",
        "{ a?: number; b?: string | undefined; c?: number | string; }",
        "^^^^^^      ^^^^^^                  ^^^^^^               ^^^",
        true,
    );
}

#[test]
fn duplicate_direct_declarations_retain_method_origin() {
    assert_node_with_options(
        "const x = { f(x:number) { return x; }, f: (y: number) => y };",
        "x =",
        "x",
        "{ f(y: number): number; }",
        "^^^^ ^^      ^^^^^^^^^^^^",
        false,
    );
}

#[test]
fn later_spread_restores_original_annotation() {
    assert_node_with_options(
        "declare const a: { value: number }; const x = { ...a, value: 2, ...a };",
        "x =",
        "x",
        "{ value: number; }",
        "^^^^^^^^^      ^^^",
        false,
    );
}

#[test]
fn reused_strings_escape_controls_and_non_ascii() {
    assert_node_with_options(
        "declare const x: { \"line\\nbreak\": \"a\\nb\"; \"\\u00e9\": \"\\u00e9\"; };",
        "x:",
        "x",
        "{ \"line\\nbreak\": \"a\\nb\"; é: \"\\u00E9\"; }",
        "^^^^^^^^^^^^^^^^^      ^^^^^        ^^^",
        false,
    );
}

#[test]
fn reused_strings_preserve_typescript_control_escape_spelling() {
    assert_node(
        "declare const x: { a: \"\\0\"; b: \"\\u00001\"; c: \"\\v\"; d: \"\\u007F\" };",
        "x:",
        "x",
        "{ a: \"\\0\"; b: \"\\x001\"; c: \"\\v\"; d: \"\u{7f}\"; }",
        "^^^^^    ^^^^^       ^^^^^    ^^^^^   ^^^",
    );
}

#[test]
fn inferred_accessor_discards_spread_property_annotation() {
    assert_node(
        "declare const a: { value: number }; const x = {...a, get value() { return 1; }};",
        "x =",
        "x",
        "{ value: number; }",
        "^^^^^^^^^^^^^^^^^^",
    );
}

#[test]
fn inferred_accessor_discards_spread_accessor_annotation() {
    assert_node(
        "const a = {get value(): number {return 1;}}; const x = {...a, get value() {return 1;}};",
        "x =",
        "x",
        "{ value: number; }",
        "^^^^^^^^^^^^^^^^^^",
    );
}
