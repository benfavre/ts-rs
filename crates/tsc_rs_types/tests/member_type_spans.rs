use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

// Original TypeScript 6.0.3 TypeWriterWalker text and synthetic-node masks.
fn assert_methods(source: &str, strict: bool, expected: &[(&str, usize, &str, &str)]) {
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
    let plain = TypeChecker::new().check_with_options(&file, &symbols, &options);
    assert_eq!(output.expression_types, plain.expression_types);
    assert_eq!(
        format!("{:?}", output.diagnostics),
        format!("{:?}", plain.diagnostics)
    );
}

#[test]
fn generic_interface_methods() {
    assert_methods(
        "interface IList<T> { data(): T; next(): string; }",
        false,
        &[
            ("data", 21, "() => T", "^^^^^^ "),
            ("next", 32, "() => string", "^^^^^^      "),
        ],
    );
}

#[test]
fn generic_type_literal_methods() {
    assert_methods(
        "type Box<T> = { map<U extends T = T>(x: U, y?: number, ...z: string[]): U; };",
        false,
        &[(
            "map",
            16,
            "<U extends T = T>(x: U, y?: number, ...z: string[]) => U",
            "^ ^^^^^^^^^ ^^^^^^ ^^ ^^ ^^^      ^^^^^ ^^        ^^^^^ ",
        )],
    );
}

#[test]
fn optional_method_without_strict_nulls() {
    assert_methods(
        "interface I { f?(): number; }",
        false,
        &[("f", 14, "() => number", "^^^^^^      ")],
    );
}

#[test]
fn optional_method_with_strict_nulls() {
    assert_methods(
        "interface I { f?(): number; }",
        true,
        &[(
            "f",
            14,
            "(() => number) | undefined",
            "^^^^^^^      ^^^^^^^^^^^^^",
        )],
    );
}

#[test]
fn overloads_in_one_interface() {
    assert_methods(
        "interface I { f(x:number):string; f(x:string):number; }",
        false,
        &[
            (
                "f",
                14,
                "{ (x: number): string; (x: string): number; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
            (
                "f",
                34,
                "{ (x: number): string; (x: string): number; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
        ],
    );
}

#[test]
fn overloads_in_merged_interfaces() {
    assert_methods(
        "interface I { f(x:number):string; } interface I { f(x:string):number; }",
        false,
        &[
            (
                "f",
                14,
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
    );
}

#[test]
fn literal_and_computed_method_names() {
    assert_methods("declare const key: unique symbol; interface I { \"quoted\"():string; 42():boolean; [key]():number; }", false, &[
        ("\"quoted\"", 48, "() => string", "^^^^^^      "),
        ("42", 67, "() => boolean", "^^^^^^       "),
        ("[key]", 81, "() => number", "^^^^^^      "),
    ]);
}

#[test]
fn namespace_aliases_and_shadowing() {
    assert_methods("namespace N { type Value = string; export interface I<T> { read(x:T): Value; generic<Value>(value:Value):Value; } }", false, &[
        ("read", 59, "(x: T) => Value", "^ ^^ ^^^^^     "),
        ("generic", 77, "<Value>(value: Value) => Value", "^     ^^     ^^     ^^^^^     "),
    ]);
}

#[test]
fn nested_type_literal_methods() {
    assert_methods(
        "type F<T> = { nested: { f(value:T):T }; };",
        false,
        &[("f", 24, "(value: T) => T", "^     ^^ ^^^^^ ")],
    );
}

#[test]
fn predicate_return_annotations() {
    assert_methods(
        "interface I { isString(x:unknown):x is string; assert(x:unknown):asserts x is string; }",
        false,
        &[
            (
                "isString",
                14,
                "(x: unknown) => x is string",
                "^ ^^       ^^^^^           ",
            ),
            (
                "assert",
                47,
                "(x: unknown) => asserts x is string",
                "^ ^^       ^^^^^                   ",
            ),
        ],
    );
}

#[test]
fn destructured_and_this_parameters() {
    assert_methods(
        "interface I { run<T>(this: I, { x }: {x:T}, [y]:[T]):T; }",
        false,
        &[(
            "run",
            14,
            "<T>(this: I, { x }: { x: T; }, [y]: [T]) => T",
            "^ ^^    ^^ ^^     ^^         ^^   ^^   ^^^^^ ",
        )],
    );
}

#[test]
fn generic_method_constraints_and_defaults() {
    assert_methods(
        "interface I { f<T extends number = 1>(value:T):T; }",
        false,
        &[(
            "f",
            14,
            "<T extends number = 1>(value: T) => T",
            "^ ^^^^^^^^^      ^^^^^^     ^^ ^^^^^ ",
        )],
    );
}

#[test]
fn own_method_overrides_inherited_method() {
    assert_methods("interface Base { f(x:number):string; } interface Child extends Base { f(x:string):number; }", false, &[
        ("f", 17, "(x: number) => string", "^ ^^      ^^^^^      "),
        ("f", 70, "(x: string) => number", "^ ^^      ^^^^^      "),
    ]);
}

#[test]
fn type_literal_overloads() {
    assert_methods(
        "type I = { f(x:number):string; f(x:string):number; };",
        false,
        &[
            (
                "f",
                11,
                "{ (x: number): string; (x: string): number; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
            (
                "f",
                31,
                "{ (x: number): string; (x: string): number; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
        ],
    );
}

#[test]
fn optional_type_literal_overloads() {
    assert_methods(
        "type I = { f?(x:number):string; f?(x:string):number; };",
        true,
        &[
            (
                "f",
                11,
                "{ (x: number): string; (x: string): number; } | undefined",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^^^^^^^^^^^^^",
            ),
            (
                "f",
                32,
                "{ (x: number): string; (x: string): number; } | undefined",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^^^^^^^^^^^^^",
            ),
        ],
    );
}

#[test]
fn identical_method_overloads() {
    assert_methods(
        "interface I { f(x:number):string; f(x:number):string; }",
        false,
        &[
            (
                "f",
                14,
                "{ (x: number): string; (x: number): string; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
            (
                "f",
                34,
                "{ (x: number): string; (x: number): string; }",
                "^^^ ^^      ^^^      ^^^ ^^      ^^^      ^^^",
            ),
        ],
    );
}

#[test]
fn separate_namespace_interfaces() {
    assert_methods("namespace A { export interface I { f():number; } } namespace B { export interface I { f():string; } }", false, &[
        ("f", 35, "() => number", "^^^^^^      "),
        ("f", 86, "() => string", "^^^^^^      "),
    ]);
}

#[test]
fn identical_merged_method_overloads() {
    assert_methods(
        "interface I { f(x:number):string; } interface I { f(x:number):string; }",
        false,
        &[
            (
                "f",
                14,
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
    );
}
