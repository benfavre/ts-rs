//! Comprehensive tests for the declaration file (.d.ts) emitter.
//!
//! These tests parse TypeScript source with the parser, then emit declaration
//! files with `declaration: true` and verify the output.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

/// Helper: parse TypeScript source and emit a .d.ts declaration file.
fn emit_dts(source: &str) -> String {
    emit_dts_named("test.ts", source)
}

fn emit_dts_named(file_name: &str, source: &str) -> String {
    let file = tsc_rs_parser::parse(file_name, source);
    let opts = CompilerOptions {
        declaration: Some(true),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.declaration_file
        .expect("declaration_file should be Some when declaration=true")
}

// ---------------------------------------------------------------
// 1. Function declaration
// ---------------------------------------------------------------

#[test]
fn test_dts_function_declaration() {
    let dts = emit_dts("function greet(name: string): string { return 'Hello ' + name; }");
    assert!(dts.contains("declare function greet(name: string): string;"));
    // Should NOT contain function body
    assert!(!dts.contains("return"));
    assert!(!dts.contains("Hello"));
}

#[test]
fn test_dts_inferred_constructor_types_preserve_explicit_arguments() {
    let source = r#"class Box<T> {}
namespace N { export class Pair<A, B> {} }
class Plain {}
function make<T>() { return new Box<T>(); }
function qualified() { return new N.Pair<string, number>(); }
const instance = new N.Pair<Box<number[]>, [string, boolean]>();
const withoutParens = new Box<string>;
function nongeneric() { return new Plain(); }
function explicit(): Box<unknown> { return new Box<string>(); }
"#;
    assert_eq!(
        emit_dts(source),
        "declare class Box<T> {\n}\n\
         declare namespace N {\n    class Pair<A, B> {\n    }\n}\n\
         declare class Plain {\n}\n\
         declare function make<T>(): Box<T>;\n\
         declare function qualified(): N.Pair<string, number>;\n\
         declare const instance: N.Pair<Box<number[]>, [string, boolean]>;\n\
         declare const withoutParens: Box<string>;\n\
         declare function nongeneric(): Plain;\n\
         declare function explicit(): Box<unknown>;\n"
    );
}

#[test]
fn test_dts_inferred_constructor_arguments_retain_type_dependencies() {
    let source = r#"interface Hidden { value: string; }
class Box<T> { item!: T; }
export function make() { return new Box<Hidden>(); }
export const value = new Box<Hidden>();
"#;
    assert_eq!(
        emit_dts(source),
        "interface Hidden {\n    value: string;\n}\n\
         declare class Box<T> {\n    item: T;\n}\n\
         export declare function make(): Box<Hidden>;\n\
         export declare const value: Box<Hidden>;\n\
         export {};\n"
    );
}

#[test]
fn test_dts_inferred_constructor_arguments_use_declaration_type_formatting() {
    let source = r#"class Box<T> {}
class Pair<A, B> {}
const value = new Pair < (string | number)[] , Box< [string, boolean] > >();
function shape() { return new Box<{ readonly value: string; next?: Box<number>; }>(); }
"#;
    assert_eq!(
        emit_dts(source),
        "declare class Box<T> {\n}\n\
         declare class Pair<A, B> {\n}\n\
         declare const value: Pair<(string | number)[], Box<[string, boolean]>>;\n\
         declare function shape(): Box<{\n    readonly value: string;\n    next?: Box<number>;\n}>;\n"
    );
}

#[test]
fn test_dts_inferred_constructor_types_keep_nongeneric_error_recovery() {
    // These initializers have TS2558 errors, but declaration emit still uses
    // the nongeneric instance types rather than copying the invalid arguments.
    let source = "class Plain {}\nconst local = new Plain<string>();\nconst builtin = new String<string>();\n";
    assert_eq!(
        emit_dts(source),
        "declare class Plain {\n}\ndeclare const local: Plain;\ndeclare const builtin: String;\n"
    );
}

#[test]
fn test_dts_inferred_constructor_types_respect_namespace_class_shadowing() {
    let source = r#"class Box<T> {}
namespace N {
    export class Box {}
    export function make() { return new Box<string>(); }
}
"#;
    assert_eq!(
        emit_dts(source),
        "declare class Box<T> {\n}\ndeclare namespace N {\n    class Box {\n    }\n    function make(): Box;\n}\n"
    );
}

// ---------------------------------------------------------------
// 2. Class declaration
// ---------------------------------------------------------------

#[test]
fn test_dts_class_declaration() {
    let dts = emit_dts(
        r#"
class MyClass {
    x: number;
    constructor(x: number) {
        this.x = x;
    }
    greet(): string {
        return "hello";
    }
}
"#,
    );
    assert!(dts.contains("declare class MyClass"));
    assert!(dts.contains("x: number;"));
    assert!(dts.contains("constructor(x: number);"));
    assert!(dts.contains("greet(): string;"));
    // Should NOT contain method bodies
    assert!(!dts.contains("return"));
    assert!(!dts.contains("this.x"));
}

// ---------------------------------------------------------------
// 3. Interface passthrough
// ---------------------------------------------------------------

#[test]
fn test_dts_interface_passthrough() {
    let dts = emit_dts(
        r#"
interface Greeter {
    name: string;
    greet(message: string): void;
}
"#,
    );
    assert!(dts.contains("interface Greeter"));
    assert!(dts.contains("name: string;"));
    assert!(dts.contains("greet(message: string): void;"));
}

// ---------------------------------------------------------------
// 4. Type alias passthrough
// ---------------------------------------------------------------

#[test]
fn test_dts_type_alias_passthrough() {
    let dts = emit_dts("type StringOrNumber = string | number;");
    assert!(dts.contains("type StringOrNumber = string | number;"));
}

// ---------------------------------------------------------------
// 5. Enum declaration
// ---------------------------------------------------------------

#[test]
fn test_dts_enum_declaration() {
    let dts = emit_dts(
        r#"
enum Direction {
    Up,
    Down,
    Left,
    Right
}
"#,
    );
    assert!(dts.contains("declare enum Direction"));
    assert!(dts.contains("Up"));
    assert!(dts.contains("Down"));
    assert!(dts.contains("Left"));
    assert!(dts.contains("Right"));
}

// ---------------------------------------------------------------
// 6. Const enum declaration
// ---------------------------------------------------------------

#[test]
fn test_dts_const_enum_declaration() {
    let dts = emit_dts(
        r#"
const enum Color {
    Red = 0,
    Green = 1,
    Blue = 2
}
"#,
    );
    assert!(dts.contains("declare const enum Color"));
    assert!(dts.contains("Red"));
    assert!(dts.contains("Green"));
    assert!(dts.contains("Blue"));
}

// ---------------------------------------------------------------
// 7. Variable declarations
// ---------------------------------------------------------------

#[test]
fn test_dts_variable_declarations() {
    let dts = emit_dts(
        r#"
const x: number = 42;
let name: string = "hello";
var count: number = 0;
"#,
    );
    assert!(dts.contains("declare const x: number;"));
    assert!(dts.contains("declare let name: string;"));
    assert!(dts.contains("declare var count: number;"));
    // Should NOT contain initializer values
    assert!(!dts.contains("42"));
    assert!(!dts.contains("hello"));
}

// ---------------------------------------------------------------
// 8. Namespace declarations
// ---------------------------------------------------------------

#[test]
fn test_dts_namespace_declaration() {
    let dts = emit_dts(
        r#"
namespace MyLib {
    export function init(): void {}
    export const version: string = "1.0";
}
"#,
    );
    assert!(dts.contains("declare namespace MyLib"));
    assert!(dts.contains("function init(): void;"));
    assert!(dts.contains("const version: string;"));
}

// ---------------------------------------------------------------
// 9. Export default
// ---------------------------------------------------------------

#[test]
fn test_dts_export_default_function() {
    let dts = emit_dts("export default function foo(x: number): string { return String(x); }");
    assert!(dts.contains("export default function foo(x: number): string;"));
    assert!(!dts.contains("return"));
}

// ---------------------------------------------------------------
// 10. Re-exports
// ---------------------------------------------------------------

#[test]
fn test_dts_reexports() {
    let dts = emit_dts(
        r#"
export { Foo, Bar as Baz } from "./other";
"#,
    );
    assert!(dts.contains("export { Foo, Bar as Baz } from \"./other\";"));
}

// ---------------------------------------------------------------
// 11. Import statements
// ---------------------------------------------------------------

#[test]
fn test_dts_import_statements() {
    let dts = emit_dts(
        r#"
import { Component } from "react";
import type { Props } from "./types";
"#,
    );
    assert!(dts.contains("import { Component } from \"react\";"));
    assert!(dts.contains("import type { Props } from \"./types\";"));
}

// ---------------------------------------------------------------
// 12. Mixed file with types and values
// ---------------------------------------------------------------

#[test]
fn test_dts_mixed_file() {
    let dts = emit_dts(
        r#"
interface Config {
    debug: boolean;
}

type ID = string | number;

function createApp(config: Config): void {
    console.log(config);
}

const DEFAULT_ID: ID = "abc";
"#,
    );
    assert!(dts.contains("interface Config"));
    assert!(dts.contains("debug: boolean;"));
    assert!(dts.contains("type ID = string | number;"));
    assert!(dts.contains("declare function createApp(config: Config): void;"));
    assert!(dts.contains("declare const DEFAULT_ID: ID;"));
    // No runtime code
    assert!(!dts.contains("console.log"));
    assert!(!dts.contains("abc"));
}

// ---------------------------------------------------------------
// 13. Module with only types
// ---------------------------------------------------------------

#[test]
fn test_dts_types_only_module() {
    let dts = emit_dts(
        r#"
interface Foo {
    bar: string;
}
type Baz = number;
"#,
    );
    assert!(dts.contains("interface Foo"));
    assert!(dts.contains("bar: string;"));
    assert!(dts.contains("type Baz = number;"));
}

// ---------------------------------------------------------------
// 14. Generic function/class declarations
// ---------------------------------------------------------------

#[test]
fn test_dts_generic_function() {
    let dts = emit_dts("function identity<T>(value: T): T { return value; }");
    assert!(dts.contains("declare function identity<T>(value: T): T;"));
    assert!(!dts.contains("return"));
}

#[test]
fn test_dts_generic_class() {
    let dts = emit_dts(
        r#"
class Container<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }
    get(): T {
        return this.value;
    }
}
"#,
    );
    assert!(dts.contains("declare class Container<T>"));
    assert!(dts.contains("value: T;"));
    assert!(dts.contains("constructor(value: T);"));
    assert!(dts.contains("get(): T;"));
    assert!(!dts.contains("return"));
}

// ---------------------------------------------------------------
// 15. Overloaded function signatures
// ---------------------------------------------------------------

#[test]
fn test_dts_overloaded_function() {
    let dts = emit_dts(
        r#"
function process(x: string): string;
function process(x: number): number;
function process(x: string | number): string | number {
    return x;
}
"#,
    );
    assert_eq!(dts, "declare function process(x: string): string;\ndeclare function process(x: number): number;\n");
}

// ---------------------------------------------------------------
// 16. Export star
// ---------------------------------------------------------------

#[test]
fn test_dts_export_star() {
    let dts = emit_dts("export * from './module';");
    assert!(dts.contains("export * from \"./module\";"));
}

#[test]
fn test_dts_export_star_as() {
    let dts = emit_dts("export * as ns from './module';");
    assert!(dts.contains("export * as ns from \"./module\";"));
}

// ---------------------------------------------------------------
// 17. Export = (CommonJS-style)
// ---------------------------------------------------------------

#[test]
fn test_dts_export_assign() {
    let dts = emit_dts(
        r#"
class MyClass {}
export = MyClass;
"#,
    );
    assert!(dts.contains("declare class MyClass"));
    assert!(dts.contains("export = MyClass;"));
}

// ---------------------------------------------------------------
// 18. Exported declarations
// ---------------------------------------------------------------

#[test]
fn test_dts_exported_function() {
    let dts = emit_dts("export function add(a: number, b: number): number { return a + b; }");
    assert!(dts.contains("export declare function add(a: number, b: number): number;"));
    assert!(!dts.contains("return"));
}

#[test]
fn test_dts_exported_class() {
    let dts = emit_dts(
        r#"
export class Point {
    x: number;
    y: number;
    constructor(x: number, y: number) {
        this.x = x;
        this.y = y;
    }
}
"#,
    );
    assert!(dts.contains("export declare class Point"));
    assert!(dts.contains("x: number;"));
    assert!(dts.contains("y: number;"));
    assert!(dts.contains("constructor(x: number, y: number);"));
}

#[test]
fn test_dts_exported_interface() {
    let dts = emit_dts(
        r#"
export interface HasId {
    id: number;
}
"#,
    );
    assert!(dts.contains("export interface HasId"));
    assert!(dts.contains("id: number;"));
}

#[test]
fn test_dts_exported_type_alias() {
    let dts = emit_dts("export type Result<T> = { ok: boolean; value: T; };");
    assert!(dts.contains("export type Result<T>"));
}

#[test]
fn test_dts_exported_enum() {
    let dts = emit_dts(
        r#"
export enum Status {
    Active,
    Inactive
}
"#,
    );
    assert!(dts.contains("export declare enum Status"));
    assert!(dts.contains("Active"));
    assert!(dts.contains("Inactive"));
}

#[test]
fn test_dts_exported_variable() {
    let dts = emit_dts("export const PI: number = 3.14;");
    assert!(dts.contains("export declare const PI: number;"));
    assert!(!dts.contains("3.14"));
}

#[test]
fn test_dts_private_declaration_in_external_module_keeps_module_boundary() {
    let dts = emit_dts(
        r#"
type PrivateShape = { value: number };
export interface PublicShape {
    nested: PrivateShape;
}
"#,
    );

    assert_eq!(
        dts,
        "type PrivateShape = {\n    value: number;\n};\nexport interface PublicShape {\n    nested: PrivateShape;\n}\nexport {};\n"
    );
}

#[test]
fn test_dts_named_exports_do_not_add_redundant_module_boundary() {
    let dts = emit_dts(
        r#"
interface Left { right: Right; }
interface Right { left: Left; }
export { Left, Right };
"#,
    );

    assert_eq!(
        dts,
        "interface Left {\n    right: Right;\n}\ninterface Right {\n    left: Left;\n}\nexport { Left, Right };\n"
    );
}

#[test]
fn test_dts_prunes_unused_private_declarations_without_a_boundary_marker() {
    let dts = emit_dts("type Private = {}; export interface Public {}");

    assert_eq!(dts, "export interface Public {\n}\n");
}

#[test]
fn test_dts_property_names_do_not_retain_same_named_private_types() {
    let dts = emit_dts("type Unused = {}; export interface Q { Unused: number }");

    assert_eq!(dts, "export interface Q {\n    Unused: number;\n}\n");
}

#[test]
fn test_dts_explicit_exports_preserve_private_dependencies_without_extra_marker() {
    let cases = [
        (
            "class Foo {}; export default Foo;",
            "declare class Foo {\n}\nexport default Foo;\n",
        ),
        (
            "class Foo {}; export = Foo;",
            "declare class Foo {\n}\nexport = Foo;\n",
        ),
        (
            "type Private = {}; export interface Public { value: Private }; export * from './other';",
            "type Private = {};\nexport interface Public {\n    value: Private;\n}\nexport * from \"./other\";\n",
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(emit_dts(source), expected, "source: {source}");
    }
}

#[test]
fn test_dts_existing_empty_export_is_not_duplicated() {
    assert_eq!(emit_dts("type Private = {}; export {};"), "export {};\n");
}

#[test]
fn test_dts_global_augmentation_does_not_duplicate_existing_boundary_marker() {
    let dts = emit_dts("export {}; declare global { interface Window { value: number } }");

    assert_eq!(
        dts,
        "export {};\ndeclare global {\n    interface Window {\n        value: number;\n    }\n}\n"
    );
}

#[test]
fn test_dts_module_file_extensions_preserve_external_module_identity() {
    for file_name in ["only.mts", "only.cts"] {
        assert_eq!(
            emit_dts_named(file_name, "interface Private {}"),
            "export {};\n",
            "file: {file_name}"
        );
    }
}

// ---------------------------------------------------------------
// 19. No declaration when option is false/absent
// ---------------------------------------------------------------

#[test]
fn test_no_dts_when_not_requested() {
    let file = tsc_rs_parser::parse("test.ts", "function foo(): void {}");
    let opts = CompilerOptions::default();
    let out = emit(&file, &opts);
    assert!(out.declaration_file.is_none());
}

// ---------------------------------------------------------------
// 20. Abstract class
// ---------------------------------------------------------------

#[test]
fn test_dts_abstract_class() {
    let dts = emit_dts(
        r#"
abstract class Shape {
    abstract area(): number;
    name: string;
}
"#,
    );
    assert!(dts.contains("declare abstract class Shape"));
    assert!(dts.contains("abstract area(): number;"));
    assert!(dts.contains("name: string;"));
}

// ---------------------------------------------------------------
// 21. Private members stripped
// ---------------------------------------------------------------

#[test]
fn test_dts_private_members_stripped() {
    let dts = emit_dts(
        r#"
class Foo {
    public x: number;
    private y: number;
    protected z: number;
    getName(): string { return ""; }
}
"#,
    );
    assert!(dts.contains("x: number;"));
    assert!(dts.contains("private y;"));
    assert!(dts.contains("protected z: number;"));
    assert!(dts.contains("getName(): string;"));
}

#[test]
fn test_dts_private_methods_and_accessors_are_preserved_as_stripped_members() {
    let dts = emit_dts(
        r#"
class Foo {
    private foo(a: string): number { return 1; }
    private get bar(): string { return ""; }
    private set baz(v: string) {}
}
"#,
    );
    assert!(dts.contains("private foo;"));
    assert!(dts.contains("private get bar();"));
    assert!(dts.contains("private set baz(value);"));
}

#[test]
fn test_dts_ecma_private_members_collapse_to_private_placeholder() {
    let dts = emit_dts(
        r#"
class Foo {
    #x: number;
    #y(): void {}
    get #z(): string { return ""; }
}
"#,
    );
    assert!(dts.contains("#private;"));
    assert!(!dts.contains("#x"));
    assert!(!dts.contains("#y"));
    assert!(!dts.contains("#z"));
}

// ---------------------------------------------------------------
// 22. Optional and rest parameters
// ---------------------------------------------------------------

#[test]
fn test_dts_optional_and_rest_params() {
    let dts = emit_dts("function foo(x: number, y?: string, ...rest: boolean[]): void {}");
    assert!(dts.contains("declare function foo(x: number, y?: string, ...rest: boolean[]): void;"));
}

// ---------------------------------------------------------------
// 23. Import namespace
// ---------------------------------------------------------------

#[test]
fn test_dts_import_namespace() {
    let dts = emit_dts("import * as path from 'path';");
    assert!(dts.contains("import * as path from 'path';"));
}

// ---------------------------------------------------------------
// 24. Export named without source
// ---------------------------------------------------------------

#[test]
fn test_dts_export_named_local() {
    let dts = emit_dts(
        r#"
const x: number = 1;
export { x };
"#,
    );
    assert!(dts.contains("export { x };"));
}

#[test]
fn enum_declarations_evaluate_constants_and_omit_runtime_initializers() {
    let output = emit_dts(
        r#"
declare function dynamic(): number;
enum First { A, B = A + 2, Text = "x", More = Text + "y", Runtime = dynamic(), Unknown, Reset = 8, Next }
enum Second { A = 40, B = First.B, C = Second.A + 1, D = First["More"] }
declare enum Ambient { A, B = 2, C }
declare const enum AmbientConst { A, B }
"#,
    );
    assert_eq!(output, "declare function dynamic(): number;\ndeclare enum First {\n    A = 0,\n    B = 2,\n    Text = \"x\",\n    More = \"xy\",\n    Runtime,\n    Unknown,\n    Reset = 8,\n    Next = 9\n}\ndeclare enum Second {\n    A = 40,\n    B = 2,\n    C = 41,\n    D = \"xy\"\n}\ndeclare enum Ambient {\n    A,\n    B = 2,\n    C\n}\ndeclare const enum AmbientConst {\n    A = 0,\n    B = 1\n}\n");
}

#[test]
fn enum_declaration_references_respect_namespace_ownership() {
    let output = emit_dts(
        r#"
namespace Left { export enum E { A = 3 } export enum F { A = E.A } }
namespace Right { export enum E { A = 7 } export enum F { A = E.A, B = Left.E.A } }
declare namespace Ambient { enum E { A, B } }
"#,
    );
    // Test the evaluated members independently of namespace export formatting.
    for expected in [
        "enum E {\n        A = 3\n    }",
        "enum F {\n        A = 3\n    }",
        "enum E {\n        A = 7\n    }",
        "enum F {\n        A = 7,\n        B = 3\n    }",
        "enum E {\n        A,\n        B\n    }",
    ] {
        assert_eq!(output.matches(expected).count(), 1, "{output}");
    }
}

#[test]
fn enum_declarations_preserve_nonfinite_values_and_bitwise_constants() {
    let output = emit_dts("enum E { Positive = 1 / 0, Negative = -1 / 0, NotANumber = 0 / 0, Bits = ~1, Unsigned = -1 >>> 1 }");
    assert_eq!(output, "declare enum E {\n    Positive = Infinity,\n    Negative = -Infinity,\n    NotANumber = NaN,\n    Bits = -2,\n    Unsigned = 2147483647\n}\n");
}

#[test]
fn const_declarations_preserve_direct_literal_types() {
    let output = emit_dts(
        r#"const n = 0x10, s = 'abc', yes = true, minus = -123, plus = +12, template = `hi`, unicode = "é😀";
const typed: number = 1;
const cast = 1 as number;
let mutable = 1;
var text = "abc";
"#,
    );
    assert_eq!(output, "declare const n = 16, s = \"abc\", yes = true, minus = -123, plus = 12, template = \"hi\", unicode = \"\\u00E9\\uD83D\\uDE00\";\ndeclare const typed: number;\ndeclare const cast: number;\ndeclare let mutable: number;\ndeclare var text: string;\n");
}

#[test]
fn bigint_const_declarations_keep_arbitrary_precision() {
    let output = emit_dts("const hex = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFn, binary = 0b101n, octal = -0o17n, separated = 1_234n;");
    assert_eq!(output, "declare const hex = 340282366920938463463374607431768211455n, binary = 5n, octal = -15n, separated = 1234n;\n");
}

#[test]
fn class_declarations_preserve_readonly_literals_and_erase_definite_assertions() {
    let output = emit_dts(
        r#"
class C {
    readonly x = 1;
    private readonly y = "foo";
    protected readonly z = true;
    readonly typed: number = 2;
    private a: number;
    field!: number;
    static readonly b = 3;
    mutable = "text";
}
"#,
    );
    assert_eq!(output, "declare class C {\n    readonly x = 1;\n    private readonly y;\n    protected readonly z = true;\n    readonly typed: number;\n    private a;\n    field: number;\n    static readonly b = 3;\n    mutable: string;\n}\n");
}

#[test]
fn constructor_parameter_properties_become_fields_and_plain_parameters() {
    let output = emit_dts(
        r#"
class C {
    constructor(public x: number, private y: string, protected readonly z?: boolean) {}
}
class Defaults { constructor(readonly value = 1) {} }
class Private { private constructor(public id: number) {} }
"#,
    );
    assert_eq!(output, "declare class C {\n    x: number;\n    private y;\n    protected readonly z?: boolean;\n    constructor(x: number, y: string, z?: boolean);\n}\ndeclare class Defaults {\n    readonly value: number;\n    constructor(value?: number);\n}\ndeclare class Private {\n    id: number;\n    private constructor();\n}\n");
}

#[test]
fn namespace_declarations_retain_public_types_and_private_dependencies() {
    let output = emit_dts(
        r#"
namespace A { class Private {} export function f(): Private { return new Private(); } }
namespace B { class Hidden {} export class Visible {} }
declare namespace D { class Implicit {} export class Explicit {} }
declare namespace E { class Hidden {} class Visible {} export { Visible }; }
namespace Outer.Inner { export enum E { A, B } }
"#,
    );
    assert_eq!(output, "declare namespace A {\n    class Private {\n    }\n    export function f(): Private;\n    export {};\n}\ndeclare namespace B {\n    class Visible {\n    }\n}\ndeclare namespace D {\n    class Implicit {\n    }\n    class Explicit {\n    }\n}\ndeclare namespace E {\n    class Hidden {\n    }\n    class Visible {\n    }\n    export { Visible };\n}\ndeclare namespace Outer.Inner {\n    enum E {\n        A = 0,\n        B = 1\n    }\n}\n");
}

#[test]
fn declaration_functions_without_value_returns_emit_void_signatures() {
    let output = emit_dts(
        r#"
function outer() { function nested() { return 1; } const arrow = () => 2; }
async function work() { return; }
class C { empty() {} async work() {} *items(): Generator<number, void, unknown> { yield 1; } }
"#,
    );
    assert_eq!(output, "declare function outer(): void;\ndeclare function work(): Promise<void>;\ndeclare class C {\n    empty(): void;\n    work(): Promise<void>;\n    items(): Generator<number, void, unknown>;\n}\n");
}

#[test]
fn declaration_void_inference_does_not_miss_nested_control_flow_returns() {
    for body in [
        "while (true) { return value; }",
        "do { return value; } while (true);",
        "for (;;) { return value; }",
        "for (const x in {}) { return value; }",
        "for (const x of []) { return value; }",
        "switch (0) { default: return value; }",
        "try { return value; } catch {}",
        "try {} catch { return value; }",
        "try {} finally { return value; }",
        "label: { return value; }",
        "if (true) { return value; } else { return; }",
    ] {
        let source = format!("function f(value: any) {{ {body} }}");
        let output = emit_dts(&source);
        assert!(!output.contains(": void;"), "{source}: {output}");
    }
}

#[test]
fn test_dts_overload_implementations_are_hidden_in_each_scope() {
    let dts = emit_dts(
        r#"
class Hidden {}
export function f(x: string): string;
export function f(x: Hidden): Hidden { return x; }
export namespace N {
    export function f(x: number): number;
    export function f(x: number): number { return x; }
}
export class C {
    constructor(x: number);
    constructor(public x: number) {}
    method(x: string): string;
    method(x: number): number;
    method(x: any): any { return x; }
    static method() {}
    "quoted"(x: string): string;
    quoted(x: any): any { return x; }
    standalone() {}
}
"#,
    );
    assert_eq!(dts, "export declare function f(x: string): string;\nexport declare namespace N {\n    function f(x: number): number;\n}\nexport declare class C {\n    x: number;\n    constructor(x: number);\n    method(x: string): string;\n    method(x: number): number;\n    static method(): void;\n    \"quoted\"(x: string): string;\n    standalone(): void;\n}\n");
}

#[test]
fn test_dts_default_function_overload() {
    assert_eq!(emit_dts("export default function (x: string): string; export default function (x: any): any {return x;}"), "export default function (x: string): string;\n");
}

#[test]
fn test_dts_implicit_any_signatures() {
    assert_eq!(emit_dts("interface I { p; m(x); (x); new(x); } type T = { p; m(x); }; declare function f(x, ...rest); abstract class C { p; abstract m(x); }"), "interface I {\n    p: any;\n    m(x: any): any;\n    (x: any): any;\n    new (x: any): any;\n}\ntype T = {\n    p: any;\n    m(x: any): any;\n};\ndeclare function f(x: any, ...rest: any[]): any;\ndeclare abstract class C {\n    p: any;\n    abstract m(x: any): any;\n}\n");
}

#[test]
fn test_dts_private_overloads_emit_one_member_per_static_scope() {
    assert_eq!(emit_dts("class C { private f(x: string); private f(x: number); private f(x: any) {} private static f(x: string); private static f(x: any) {} } declare class D { private f(x: string); private f(x: number); }"), "declare class C {\n    private f;\n    private static f;\n}\ndeclare class D {\n    private f;\n}\n");
}

#[test]
fn test_dts_default_parameters_keep_types_and_optionality() {
    assert_eq!(emit_dts("function f(a = 1, b = 'x', ...rest) {} class C { method(a = 0, b) {} optional(a = 0, b?) {} }"), "declare function f(a?: number, b?: string, ...rest: any[]): void;\ndeclare class C {\n    method(a: number, b: any): void;\n    optional(a?: number, b?: any): void;\n}\n");
}

#[test]
fn test_dts_required_defaults_respect_strict_null_checks() {
    let source = "function f(a: string = '', b: string | undefined = '', c: any = 1, d: (() => void) = () => {}, required: number) {} class C { constructor(a = '', required: number) {} method(a = true, required: number) {} }";
    let file = tsc_rs_parser::parse("test.ts", source);
    let options = CompilerOptions {
        declaration: Some(true),
        strict: Some(true),
        ..Default::default()
    };
    assert_eq!(emit(&file, &options).declaration_file.unwrap(), "declare function f(a: string | undefined, b: string | undefined, c: any, d: (() => void) | undefined, required: number): void;\ndeclare class C {\n    constructor(a: string | undefined, required: number);\n    method(a: boolean | undefined, required: number): void;\n}\n");
    let options = CompilerOptions {
        strict_null_checks: Some(false),
        ..options
    };
    assert_eq!(emit(&file, &options).declaration_file.unwrap(), "declare function f(a: string, b: string | undefined, c: any, d: (() => void), required: number): void;\ndeclare class C {\n    constructor(a: string, required: number);\n    method(a: boolean, required: number): void;\n}\n");
}

#[test]
fn test_dts_default_parameter_assertions() {
    assert_eq!(
        emit_dts("function f(a = 'x' as const, b = 1 as number | string) {}"),
        "declare function f(a?: \"x\", b?: number | string): void;\n"
    );
}

#[test]
fn test_dts_single_returns_use_parameter_scope_and_literal_types() {
    assert_eq!(emit_dts("let x = 1; function f(x: string) { return x; } function identity<T>(x: T) { return x; } class C { method(x: number) { return x; } literal() { return 1; } async work(x: string) { return x; } }"), "declare let x: number;\ndeclare function f(x: string): string;\ndeclare function identity<T>(x: T): T;\ndeclare class C {\n    method(x: number): number;\n    literal(): number;\n    work(x: string): Promise<string>;\n}\n");
}

#[test]
fn test_dts_optional_parameter_returns_preserve_undefined() {
    let file = tsc_rs_parser::parse(
        "test.ts",
        "function f(x?: string) {return x;} class C { method(x?: number | undefined) {return x;} }",
    );
    let opts = CompilerOptions {
        declaration: Some(true),
        strict_null_checks: Some(true),
        ..Default::default()
    };
    assert_eq!(emit(&file, &opts).declaration_file.unwrap(), "declare function f(x?: string): string | undefined;\ndeclare class C {\n    method(x?: number | undefined): number | undefined;\n}\n");
}

#[test]
fn test_dts_namespace_alias_retains_private_dependency_chain() {
    let dts = emit_dts(
        "namespace Outer { namespace Inner { class Hidden {} } import Middle = Inner; export import Public = Middle; import Unused = Inner; }",
    );
    assert_eq!(
        dts,
        "declare namespace Outer {\n    namespace Inner {\n    }\n    import Middle = Inner;\n    export import Public = Middle;\n    export {};\n}\n"
    );
}

#[test]
fn test_dts_namespace_alias_keeps_explicit_export() {
    let dts = emit_dts(
        "namespace Outer { export namespace Inner { export interface Value { name: string } } export import Public = Inner; }",
    );
    assert_eq!(
        dts,
        "declare namespace Outer {\n    namespace Inner {\n        interface Value {\n            name: string;\n        }\n    }\n    export import Public = Inner;\n}\n"
    );
}

#[test]
fn test_dts_exported_qualified_alias_preserves_module_boundary() {
    let dts = emit_dts(
        "namespace Hidden { export namespace Inner { export interface Value { name: string } } } import Unused = Hidden.Inner; export import Public = Hidden.Inner;",
    );
    assert_eq!(
        dts,
        "declare namespace Hidden {\n    namespace Inner {\n        interface Value {\n            name: string;\n        }\n    }\n}\nexport import Public = Hidden.Inner;\nexport {};\n"
    );
}

#[test]
fn test_dts_internal_alias_used_by_exported_signature_is_retained() {
    let dts = emit_dts(
        "namespace Hidden { export interface Value { name: string } } import Alias = Hidden.Value; export function use(value: Alias): Alias { return value; }",
    );
    assert_eq!(
        dts,
        "declare namespace Hidden {\n    interface Value {\n        name: string;\n    }\n}\nimport Alias = Hidden.Value;\nexport declare function use(value: Alias): Alias;\nexport {};\n"
    );
}

#[test]
fn test_dts_empty_binding_patterns_preserve_external_module_identity() {
    for source in [
        "export let [,,[,[],,[],]] = undefined as any;",
        "export const {} = sideEffect();",
        "export const { item: [,{ nested: {} }] = [] } = sideEffect();",
        "export let [...[]] = sideEffect();",
    ] {
        assert_eq!(emit_dts(source), "export {};\n", "{source}");
    }
}

#[test]
fn test_dts_empty_binding_patterns_emit_no_script_declarations() {
    let dts = emit_dts(
        "const {} = sideEffect(); let [,,[[]]] = sideEffect(); var { item: {} } = sideEffect();",
    );
    assert_eq!(dts, "");
}

#[test]
fn test_dts_empty_bindings_do_not_leave_commas_or_export_prefixes() {
    assert_eq!(
        emit_dts(
            "export const [] = [], first = 1, {} = {}, second: string = '', [,,] = [] as any;"
        ),
        "export declare const first = 1, second: string;\n"
    );
    assert_eq!(
        emit_dts(
            "namespace Outer { export const {} = {}; } declare namespace Ambient { const {}: {}; }"
        ),
        "declare namespace Outer {\n}\ndeclare namespace Ambient {\n}\n"
    );
}

#[test]
fn test_dts_empty_bindings_keep_existing_module_markers() {
    assert_eq!(
        emit_dts("import 'side-effect'; export const {} = {};"),
        "import 'side-effect';\n"
    );
    assert_eq!(emit_dts("export const {} = {}; export {};"), "export {};\n");
    assert_eq!(
        emit_dts_named("empty.mts", "const {} = {};"),
        "export {};\n"
    );
}

#[test]
fn test_dts_leading_documentation_on_exported_declarations() {
    assert_eq!(
        emit_dts("// ordinary\n/** Public value */\n/* ordinary between */\nexport const value = 1;\n/** Public type */\nexport type Value = number;"),
        "/** Public value */\nexport declare const value = 1;\n/** Public type */\nexport type Value = number;\n"
    );
    assert_eq!(
        emit_dts("/** Default class */\nexport default class C {}"),
        "/** Default class */\nexport default class C {\n}\n"
    );
}

#[test]
fn test_dts_documentation_preserves_inline_layout_and_relative_indent() {
    assert_eq!(
        emit_dts("/** first */ /** second */ const x = 1;"),
        "/** first */ /** second */ declare const x = 1;\n"
    );
    assert_eq!(
        emit_dts("/** namespace */\nnamespace N {\n      /** value\n       * explanation\n       */\n      export const value = 1;\n}"),
        "/** namespace */\ndeclare namespace N {\n    /** value\n     * explanation\n     */\n    const value = 1;\n}\n"
    );
    assert_eq!(
        emit_dts("/** windows\r\n * lines\r\n */\r\nconst x = 1;"),
        "/** windows\n * lines\n */\ndeclare const x = 1;\n"
    );
}

#[test]
fn test_dts_documentation_is_not_collected_from_erased_statements() {
    assert_eq!(
        emit_dts("/** hidden */\nconst hidden = 0;\n/** expression */\nvoid hidden;\n/** visible */\nexport function visible(): void { /** body */ void hidden; }"),
        "/** visible */\nexport declare function visible(): void;\n"
    );
    assert_eq!(emit_dts("/** expression */\nvoid 0;"), "");
    assert_eq!(
        emit_dts("/**/\nconst value = 1;"),
        "declare const value = 1;\n"
    );
}

#[test]
fn test_dts_documentation_respects_overload_elision() {
    assert_eq!(
        emit_dts("/** number input */\nfunction f(value: number): void;\n/** string input */\nfunction f(value: string): void;\n/** implementation only */\nfunction f(value: any): void {}"),
        "/** number input */\ndeclare function f(value: number): void;\n/** string input */\ndeclare function f(value: string): void;\n"
    );
}

#[test]
fn test_dts_leading_documentation_honors_remove_comments() {
    let file = tsc_rs_parser::parse("test.ts", "/** public */\nexport const value = 1;");
    let options = CompilerOptions {
        declaration: Some(true),
        remove_comments: Some(true),
        ..Default::default()
    };
    assert_eq!(
        emit(&file, &options).declaration_file.unwrap(),
        "export declare const value = 1;\n"
    );
}

#[test]
fn test_dts_uninitialized_variable_declarators_have_explicit_any_types() {
    let source = "var x, y, z = 1; let a, b: string; declare const ambient;";
    let expected = "declare var x: any, y: any, z: number;\ndeclare let a: any, b: string;\ndeclare const ambient: any;\n";
    for strict in [false, true] {
        let file = tsc_rs_parser::parse("test.ts", source);
        let options = CompilerOptions {
            declaration: Some(true),
            strict: Some(strict),
            ..Default::default()
        };
        assert_eq!(emit(&file, &options).declaration_file.unwrap(), expected);
    }
}

#[test]
fn test_dts_exported_and_namespace_variables_keep_implicit_any_types() {
    assert_eq!(
        emit_dts("export let value; export declare const ambient; export namespace N { export var member, typed: number; }"),
        "export declare let value: any;\nexport declare const ambient: any;\nexport declare namespace N {\n    var member: any, typed: number;\n}\n"
    );
}

#[test]
fn test_dts_generic_constructor_lookup_stops_at_nearer_values() {
    // Exact TypeScript 6.0.3 declarations, including erroneous type arguments.
    let cases = [
        ("class Box<T> {} namespace N { export type Box={}; export declare const Box:new()=>Box; export const value=new Box<string>(); }", &[false][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    type Box = {};\n    const Box: new () => Box;\n    const value: Box;\n}\n"),
        ("class Box<T> {} export namespace N { export type Box={}; export declare const Box:new()=>Box; export const value=new Box<string>(); }", &[false][..], "export declare namespace N {\n    type Box = {};\n    const Box: new () => Box;\n    const value: Box;\n}\n"),
        ("class Box<T> {} namespace N { export interface Box {} export declare const Box:new()=>Box; export const value=new Box<string>(); }", &[false][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    const Box: new () => Box;\n    const value: Box;\n}\n"),
        ("class Box<T> {} namespace N { export type Box={}; export declare const Box:new()=>Box; export namespace Deep{export const value=new Box<string>();} }", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    type Box = {};\n    const Box: new () => Box;\n    namespace Deep {\n        const value: Box;\n    }\n}\n"),
        ("class Box<T>{} interface Box<T>{value:T} export const value=new Box<string>();", &[false, true][..], "declare class Box<T> {\n}\ninterface Box<T> {\n    value: T;\n}\nexport declare const value: Box<string>;\nexport {};\n"),
        ("class Box<T>{} namespace Box{export class Inner<T>{}} export const a=new Box<string>();export const b=new Box.Inner<number>();", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace Box {\n    class Inner<T> {\n    }\n}\nexport declare const a: Box<string>;\nexport declare const b: Box.Inner<number>;\nexport {};\n"),
        ("namespace N{export class Box<T>{}} namespace N{export const value=new Box<string>();}", &[false, true][..], "declare namespace N {\n    class Box<T> {\n    }\n}\ndeclare namespace N {\n    const value: Box<string>;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export function make(Box:new()=>Box){return new Box<string>();}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    function make(Box: new () => Box): Box;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export function make({Box}:{Box:new()=>Box}){return new Box<string>();}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    function make({ Box }: {\n        Box: new () => Box;\n    }): Box;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export function make(){const Box:new()=>Box=null!;return new Box<string>();}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    function make(): Box;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export function make(source:{Box:new()=>Box}){const {Box}=source;return new Box<string>();}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    function make(source: {\n        Box: new () => Box;\n    }): Box;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export function make(){ {const Box:new()=>Box=null!;return new Box<string>();} }}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    function make(): Box;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export function make(){if(false){var Box:new()=>Box;}return new Box<string>();}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    function make(): Box;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export function make(Box:new()=>Box,value=new Box<string>()){}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    function make(Box: new () => Box, value?: Box): void;\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export class C{constructor(Box:new()=>Box,public value=new Box<string>()){}}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    class C {\n        value: Box;\n        constructor(Box: new () => Box, value?: Box);\n    }\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export class C{make(Box:new()=>Box,value=new Box<string>()){}}}", &[false, true][..], "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    class C {\n        make(Box: new () => Box, value?: Box): void;\n    }\n}\n"),
    ];
    for (source, strict_modes, expected) in cases {
        let file = tsc_rs_parser::parse("probe.ts", source);
        for &strict in strict_modes {
            let options = CompilerOptions {
                declaration: Some(true),
                strict: Some(strict),
                ..Default::default()
            };
            assert_eq!(
                emit(&file, &options).declaration_file.unwrap(),
                expected,
                "{source}, strict={strict}"
            );
        }
    }
}

#[test]
fn test_dts_signature_parameter_values_shadow_generic_constructors() {
    // Erroneous signature initializers still have declaration emit baselines.
    for (source, expected) in [
        ("class Box<T>{} namespace N{export interface Box{} export interface I{make(Box:new()=>Box,value=new Box<string>()):void}}", "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    interface I {\n        make(Box: new () => Box, value?: Box): void;\n    }\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export interface I{(Box:new()=>Box,value=new Box<string>()):void}}", "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    interface I {\n        (Box: new () => Box, value?: Box): void;\n    }\n}\n"),
        ("class Box<T>{} namespace N{export interface Box{} export type Fn=(Box:new()=>Box,value=new Box<string>())=>void;}", "declare class Box<T> {\n}\ndeclare namespace N {\n    interface Box {\n    }\n    type Fn = (Box: new () => Box, value?: Box) => void;\n}\n"),
    ] {
        let file = tsc_rs_parser::parse("probe.ts", source);
        for strict in [false, true] {
            let options = CompilerOptions { declaration: Some(true), strict: Some(strict), ..Default::default() };
            assert_eq!(emit(&file, &options).declaration_file.unwrap(), expected, "{source}, strict={strict}");
        }
    }
}

#[test]
fn test_dts_fresh_null_initializers_widen_without_strict_null_checks() {
    let emit_non_strict = |source| {
        let file = tsc_rs_parser::parse("test.ts", source);
        emit(
            &file,
            &CompilerOptions {
                declaration: Some(true),
                strict: Some(false),
                ..Default::default()
            },
        )
        .declaration_file
        .unwrap()
    };
    assert_eq!(
        emit_non_strict("const c = null, p = (null), s = null satisfies null, n = null!; let l = null; var v = null;"),
        "declare const c: any, p: any, s: any, n: any;\ndeclare let l: any;\ndeclare var v: any;\n"
    );
    assert_eq!(
        emit_non_strict("namespace N { export const value = null; } class C { value = null; readonly immutable = null; }"),
        "declare namespace N {\n    const value: any;\n}\ndeclare class C {\n    value: any;\n    readonly immutable: any;\n}\n"
    );
}

#[test]
fn test_dts_explicit_null_types_do_not_widen() {
    assert_eq!(
        emit_dts("const typed: null = null, asserted = null as null, cast = <null>null, wrapped = (null as null) satisfies null; class C { value: null = null; asserted = null as null; }"),
        "declare const typed: null, asserted: null, cast: null, wrapped: null;\ndeclare class C {\n    value: null;\n    asserted: null;\n}\n"
    );
}

#[test]
fn test_dts_null_initializer_widening_respects_strict_option_precedence() {
    let file = tsc_rs_parser::parse("test.ts", "const c = null; class C { value = null; }");
    for (strict, strict_null_checks, ty) in [
        (None, None, "null"),
        (None, Some(true), "null"),
        (None, Some(false), "any"),
        (Some(false), None, "any"),
        (Some(true), None, "null"),
        (Some(false), Some(true), "null"),
        (Some(true), Some(false), "any"),
    ] {
        let options = CompilerOptions {
            declaration: Some(true),
            strict,
            strict_null_checks,
            ..Default::default()
        };
        assert_eq!(
            emit(&file, &options).declaration_file.unwrap(),
            format!("declare const c: {ty};\ndeclare class C {{\n    value: {ty};\n}}\n")
        );
    }
}

#[test]
fn test_dts_assertions_over_widened_null_preserve_all_public_positions() {
    // Exact declaration output from TypeScript 6.0.3.
    for (source, expected) in [
        (
            "const fresh=null; export const asserted=fresh as null;",
            "export declare const asserted: null;\n",
        ),
        (
            "const fresh=null; export const cast=<null>fresh;",
            "export declare const cast: null;\n",
        ),
        (
            "const fresh=null; export function f(){return fresh as null;}",
            "export declare function f(): null;\n",
        ),
        (
            "const fresh=null; export class C{value=fresh as null;}",
            "export declare class C {\n    value: null;\n}\n",
        ),
        (
            "const fresh=null; export class C{constructor(public value=fresh as null){}}",
            "export declare class C {\n    value: null;\n    constructor(value?: null);\n}\n",
        ),
    ] {
        let file = tsc_rs_parser::parse("probe.ts", source);
        for (strict, strict_null_checks) in [
            (None, None),
            (Some(false), None),
            (Some(true), None),
            (Some(true), Some(false)),
            (Some(false), Some(true)),
        ] {
            let options = CompilerOptions {
                declaration: Some(true),
                strict,
                strict_null_checks,
                ..Default::default()
            };
            assert_eq!(
                emit(&file, &options).declaration_file.unwrap(),
                expected,
                "{source}; strict={strict:?}; strict_null_checks={strict_null_checks:?}"
            );
        }
    }
}

#[test]
fn test_dts_null_assertion_wrappers_keep_existing_declaration_inference() {
    // TypeScript 6.0.3 emits these declarations even for invalid as-const inputs.
    for source in [
        "const fresh=null; export const v=(fresh as null) as const;",
        "const fresh=null; export const v=(fresh as string) as null;",
        "const fresh=null; export const v=((fresh as null) as const);",
    ] {
        let file = tsc_rs_parser::parse("probe.ts", source);
        for (strict, strict_null_checks) in [
            (false, None),
            (true, None),
            (true, Some(false)),
            (false, Some(true)),
        ] {
            let options = CompilerOptions {
                declaration: Some(true),
                strict: Some(strict),
                strict_null_checks,
                ..Default::default()
            };
            assert_eq!(
                emit(&file, &options).declaration_file.unwrap(),
                "export declare const v: null;\n",
                "{source}, strict={strict}, strict_null_checks={strict_null_checks:?}"
            );
        }
    }
}

#[test]
fn test_dts_nullish_assertions_keep_existing_non_strict_inference() {
    // Existing non-null alias narrowing with strict null checks disabled.
    for source in [
        "const fresh=null; export const v=(fresh as (null))!;",
        "const fresh=null; export const v=(fresh as (null|undefined))!;",
        "type N=null; const fresh=null; export const v=(fresh as N)!;",
        "type N=null|undefined; const fresh=null; export const v=(fresh as N)!;",
        "type N=undefined|null; const fresh=null; export const v=(fresh as N)!;",
        "const fresh=null;export const v=(fresh as ((null)|(undefined)))!;",
    ] {
        let file = tsc_rs_parser::parse("probe.ts", source);
        for (strict, strict_null_checks) in [(false, None), (true, Some(false))] {
            let options = CompilerOptions {
                declaration: Some(true),
                strict: Some(strict),
                strict_null_checks,
                ..Default::default()
            };
            assert_eq!(
                emit(&file, &options).declaration_file.unwrap(),
                "export declare const v: null;\n",
                "{source}, strict={strict}, strict_null_checks={strict_null_checks:?}"
            );
        }
    }
}

#[test]
fn test_dts_omitted_strict_options_preserve_fresh_null() {
    // Exact TS6 output. A later for-in binding does not affect the declaration.
    let source = "let l9=0,l10:string=\"\",l11=null;for(let l11 in {}){} const c=null;var v=null;namespace N{export const value=null;}class C{value=null;readonly immutable=null;}";
    assert_eq!(emit_dts(source), "declare let l9: number, l10: string, l11: null;\ndeclare const c: null;\ndeclare var v: null;\ndeclare namespace N {\n    const value: null;\n}\ndeclare class C {\n    value: null;\n    readonly immutable: null;\n}\n");
}
