use tsc_rs_types::{TypeCheckOutput, TypeChecker};

fn check(source: &str) -> TypeCheckOutput {
    let file = tsc_rs_parser::parse("case.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    checker.check(&file, &symbols)
}
fn assert_node(source: &str, anchor: &str, name: &str, text: &str, underline: &str) {
    let result = check(source);
    let start = source.find(anchor).unwrap() as u32;
    let entry = result
        .expression_type_spans
        .get(&(start, start + name.len() as u32))
        .unwrap();
    assert_eq!(entry.node_text, text, "{source}");
    assert_eq!(entry.underline, underline, "{source}\n{text}");
    assert_eq!(text.encode_utf16().count(), underline.len());
}

// Expected text and masks come from TypeScript 6.0.3's TypeWriterWalker,
// with its original synthetic-node printer, not our baseline renderer.
#[test]
fn declared_annotations_and_parameter_names_retain_source_provenance() {
    let source =
        "declare function f(x: number, y?: string, ...z: boolean[]): number; const g = f; f;";
    for (anchor, name) in [("f(", "f"), ("g =", "g")] {
        assert_node(
            source,
            anchor,
            name,
            "(x: number, y?: string, ...z: boolean[]) => number",
            "^ ^^      ^^ ^^^      ^^^^^ ^^         ^^^^^      ",
        );
    }
}
#[test]
fn inferred_return_types_and_default_parameter_types_are_synthetic() {
    let source = "const f = (x: number) => x; function g(x = 1) { return x; }";
    assert_node(
        source,
        "f =",
        "f",
        "(x: number) => number",
        "^ ^^      ^^^^^^^^^^^",
    );
    assert_node(
        source,
        "g(",
        "g",
        "(x?: number) => number",
        "^ ^^^^^^^^^^^^^^^^^^^^",
    );
}
#[test]
fn generic_constraints_reuse_source_but_defaults_are_synthesized() {
    let source = "declare function f<T extends number = 1>(x:T): T;";
    assert_node(
        source,
        "f<",
        "f",
        "<T extends number = 1>(x: T) => T",
        "^ ^^^^^^^^^      ^^^^^^ ^^ ^^^^^ ",
    );
}
#[test]
fn overloads_preserve_each_declarations_annotations() {
    let source = "declare let f: { (x: number): string; (x: string): number; }; f;";
    assert_node(
        source,
        "f:",
        "f",
        "{ (x: number): string; (x: string): number; }",
        "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
    );
}
#[test]
fn nested_source_type_literals_use_syntax_instead_of_semantic_expansion() {
    let source = "declare let f: (x: { a: number }) => { b: string }; f;";
    assert_node(
        source,
        "f:",
        "f",
        "(x: { a: number; }) => { b: string; }",
        "^ ^^              ^^^^^              ",
    );
}
#[test]
fn destructuring_names_drop_initializers_when_cloned() {
    let source = "function f({x = 1}: {x?:number}, [y]: [number]) { return x + y; } f;";
    assert_node(
        source,
        "f(",
        "f",
        "({ x }: { x?: number; }, [y]: [number]) => number",
        "^     ^^               ^^   ^^        ^^^^^^^^^^^",
    );
}
#[test]
fn source_alias_spelling_and_union_order_are_preserved_in_annotations() {
    assert_node(
        "type X = number; declare function f(x: X): X;",
        "f(",
        "f",
        "(x: X) => X",
        "^ ^^ ^^^^^ ",
    );
    assert_node(
        "function g(x: number | string, y: Array<number>): void {}",
        "g(",
        "g",
        "(x: number | string, y: Array<number>) => void",
        "^ ^^               ^^ ^^             ^^^^^    ",
    );
}
#[test]
fn constructor_signatures_retain_source_annotations() {
    assert_node(
        "declare let f: new (x: number) => {}; f;",
        "f:",
        "f",
        "new (x: number) => {}",
        "^^^^^ ^^      ^^^^^  ",
    );
}
#[test]
fn methods_and_nested_callbacks_preserve_distinct_origins() {
    let source = "const o = { f(x: number) { return x; }, p: (y: string) => y }; o.f;";
    assert_node(
        source,
        "o =",
        "o",
        "{ f(x: number): number; p: (y: string) => string; }",
        "^^^^ ^^      ^^^^^^^^^^^^^^^ ^^      ^^^^^^^^^^^^^^",
    );
}

#[test]
fn instantiated_interface_methods_synthesize_changed_types_but_reuse_names() {
    assert_node(
        "interface Box<T> { f(x:T):T; } declare let b:Box<number>; const g=b.f;",
        "g=",
        "g",
        "(x: number) => number",
        "^ ^^^^^^^^^^^^^^^^^^^",
    );
}
#[test]
fn original_parameter_names_use_utf16_width() {
    assert_node(
        "declare function f(𐐀: number): number;",
        "f(",
        "f",
        "(𐐀: number) => number",
        "^  ^^      ^^^^^      ",
    );
}
#[test]
fn reused_nested_signatures_have_original_punctuation() {
    assert_node(
        "declare function f(x: (n: number) => string): (n: string) => number;",
        "f(",
        "f",
        "(x: (n: number) => string) => (n: string) => number",
        "^ ^^                     ^^^^^                     ",
    );
}

#[test]
fn member_origin_survives_spreads_but_does_not_follow_copied_functions() {
    let source =
        "const o = { f(x: number) { return x; } }; const p = { q: o.f }; const s = { ...o };";
    assert_node(
        source,
        "p =",
        "p",
        "{ q: (x: number) => number; }",
        "^^^^^^ ^^      ^^^^^^^^^^^^^^",
    );
    assert_node(
        source,
        "s =",
        "s",
        "{ f(x: number): number; }",
        "^^^^ ^^      ^^^^^^^^^^^^",
    );
}
