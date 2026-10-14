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

#[test]
fn test_dts_typed_callable_initializers_keep_signatures_and_dependencies() {
    // Complete declaration outputs checked against TypeScript 6.0.3.
    for (source, expected) in [
        ("const f=()=>{};", "declare const f: () => void;\n"),
        ("const f=(x:string)=>{};", "declare const f: (x: string) => void;\n"),
        ("const f=function(x:string){};", "declare const f: (x: string) => void;\n"),
        ("export const f=(((x:string):number=>1));", "export declare const f: (x: string) => number;\n"),
        ("type Named={name:string};let f=({name:alias}:Named)=>{};", "type Named = {\n    name: string;\n};\ndeclare let f: ({ name: alias }: Named) => void;\n"),
        ("type Named={name:string};const f=({name:a,name:b}:Named)=>{};", "type Named = {\n    name: string;\n};\ndeclare const f: ({ name: a, name: b }: Named) => void;\n"),
        ("let value=\"\";const f=({value:alias}:{value:string}):typeof value=>value;", "declare let value: string;\ndeclare const f: ({ value: alias }: {\n    value: string;\n}) => typeof value;\n"),
        ("type Hidden={x:number};export const f=(x:Hidden)=>{};", "type Hidden = {\n    x: number;\n};\nexport declare const f: (x: Hidden) => void;\nexport {};\n"),
        ("type Hidden={x:number};export const f=(x:string):Hidden=>({x:1});", "type Hidden = {\n    x: number;\n};\nexport declare const f: (x: string) => Hidden;\nexport {};\n"),
        ("type A={x:number};type B={y:number};export const f=(x:A):B=>({y:1});", "type A = {\n    x: number;\n};\ntype B = {\n    y: number;\n};\nexport declare const f: (x: A) => B;\nexport {};\n"),
        ("const hidden=\"\";export const f=(x:string):typeof hidden=>hidden;", "declare const hidden = \"\";\nexport declare const f: (x: string) => typeof hidden;\nexport {};\n"),
        ("type T={unused:string};export const f=<T>(x:T):T=>x;", "export declare const f: <T>(x: T) => T;\n"),
        ("type Hidden={x:number};export const f=<T extends Hidden>(x:T):T=>x;", "type Hidden = {\n    x: number;\n};\nexport declare const f: <T extends Hidden>(x: T) => T;\nexport {};\n"),
        ("type Hidden={x:number};export const f=<T=Hidden>(x:T):T=>x;", "type Hidden = {\n    x: number;\n};\nexport declare const f: <T = Hidden>(x: T) => T;\nexport {};\n"),
        ("type T={outer:number};export namespace N{type T={inner:string};export const f=(x:T):T=>x;}", "export declare namespace N {\n    type T = {\n        inner: string;\n    };\n    export const f: (x: T) => T;\n    export {};\n}\n"),
        ("export const f=(x?:string):number=>1;", "export declare const f: (x?: string) => number;\n"),
        ("export const f=(...x:string[]):number=>x.length;", "export declare const f: (...x: string[]) => number;\n"),
        ("export const f=([a,b]:string[]):void=>{};", "export declare const f: ([a, b]: string[]) => void;\n"),
        ("type N={p:{x:string}};export const f=({p:{x:alias}}:N):void=>{};", "type N = {\n    p: {\n        x: string;\n    };\n};\nexport declare const f: ({ p: { x: alias } }: N) => void;\nexport {};\n"),
        ("export const f=({x=\"\"}:{x?:string}):void=>{};", "export declare const f: ({ x }: {\n    x?: string;\n}) => void;\n"),
        ("export const f=({default:alias}:{default:string}):void=>{};", "export declare const f: ({ default: alias }: {\n    default: string;\n}) => void;\n"),
        ("export const f=(x:unknown):x is string=>true;", "export declare const f: (x: unknown) => x is string;\n"),
        ("export const f=(x:unknown):asserts x=>{};", "export declare const f: (x: unknown) => asserts x;\n"),
        ("export const f=function internal(x:string):string{return x;};", "export declare const f: (x: string) => string;\n"),
        ("export const f=function(this:{x:number}):void{};", "export declare const f: (this: {\n    x: number;\n}) => void;\n"),
        ("export const f=(x:string):null=>null;", "export declare const f: (x: string) => null;\n"),
        ("export const f=(x:string):string|undefined=>x;", "export declare const f: (x: string) => string | undefined;\n"),
        ("const f=(x:string):void=>{}, n=1, g=():number=>1;", "declare const f: (x: string) => void, n = 1, g: () => number;\n"),
        ("type Hidden={x:number};export const f:(x:Hidden)=>void=(x)=>{};", "type Hidden = {\n    x: number;\n};\nexport declare const f: (x: Hidden) => void;\nexport {};\n"),
        ("export const f:<T>(x:T)=>T=<T>(x:T):T=>x;", "export declare const f: <T>(x: T) => T;\n"),
        ("export const f=1;export const s=\"hello\";", "export declare const f = 1;\nexport declare const s = \"hello\";\n"),
        ("export const f:any=(x:string):void=>{};", "export declare const f: any;\n"),
        ("export declare const f:(x:string)=>void;", "export declare const f: (x: string) => void;\n"),
        ("type X={x:number};export const f=(X:X):X=>X;", "type X = {\n    x: number;\n};\nexport declare const f: (X: X) => X;\nexport {};\n"),
        ("type Hidden={x:number};interface Wrap<T>{x:T};export const f=(x:Wrap<Hidden>):Hidden=>x.x;", "type Hidden = {\n    x: number;\n};\ninterface Wrap<T> {\n    x: T;\n}\nexport declare const f: (x: Wrap<Hidden>) => Hidden;\nexport {};\n"),
        ("const Hidden=1;export const f=(x:typeof Hidden):void=>{};", "declare const Hidden = 1;\nexport declare const f: (x: typeof Hidden) => void;\nexport {};\n"),
        ("const Hidden=1;export const f=<T>(x:T):typeof Hidden=>Hidden;", "declare const Hidden = 1;\nexport declare const f: <T>(x: T) => typeof Hidden;\nexport {};\n"),
    ] {
        let file = tsc_rs_parser::parse("probe.ts", source);
        for (strict, strict_null_checks) in [
            (None, None), (Some(false), None), (Some(true), None),
            (Some(true), Some(false)), (Some(false), Some(true)),
            (None, Some(false)), (None, Some(true)),
        ] {
            let options = CompilerOptions {
                declaration: Some(true), strict, strict_null_checks, ..Default::default()
            };
            assert_eq!(emit(&file, &options).declaration_file.unwrap(), expected,
                "{source}; strict={strict:?}; strict_null_checks={strict_null_checks:?}");
        }
    }
}

#[test]
fn test_dts_variable_any_assertions_preserve_boundaries_and_dependencies() {
    // Complete declaration outputs checked against TypeScript 6.0.3 in each
    // strict-option mode, including omitted options and child overrides.
    let cases = [
        ("class_any", "const anyThing=class{} as any;export class Foo extends anyThing{};", "declare const anyThing: any;\nexport declare class Foo extends anyThing {\n}\nexport {};\n"),
        ("angle_any", "const anyThing=<any>class{};export class Foo extends anyThing{};", "declare const anyThing: any;\nexport declare class Foo extends anyThing {\n}\nexport {};\n"),
        ("primitive_any", "export const n=1 as any,s=\"x\" as any,b=true as any;", "export declare const n: any, s: any, b: any;\n"),
        ("var_let", "let n=1 as any;var s=<any>\"x\";", "declare let n: any;\ndeclare var s: any;\n"),
        ("parenthesized", "export const v=(((1 as (any))));", "export declare const v: (any);\n"),
        ("nonnull", "export const v=((null as any))!;", "export declare const v: any;\n"),
        ("satisfies", "export const v=(1 as any) satisfies string;", "export declare const v: any;\n"),
        ("satisfies_under_any", "export const v=(1 satisfies number) as any;", "export declare const v: any;\n"),
        ("outer_any", "export const v=((null as string) as any);", "export declare const v: any;\n"),
        ("outer_null", "export const v=((null as any) as null);", "export declare const v: null;\n"),
        ("outer_number", "export const v=((1 as any) as number);", "export declare const v: number;\n"),
        ("explicit_annotation", "export const v:number=1 as any;", "export declare const v: number;\n"),
        ("explicit_null_annotation", "export const v:null=1 as any;", "export declare const v: null;\n"),
        ("private_constructor", "class Private{};export const v=new Private() as any;", "export declare const v: any;\n"),
        ("private_generic_constructor", "class Box<T>{};class Private{};export const v=new Box<Private>() as any;", "export declare const v: any;\n"),
        ("private_generic_exported", "class Box<T>{};class Private{};export const v=new Box<Private>() as any;export const typed:Box<Private>=v;", "declare class Box<T> {\n}\ndeclare class Private {\n}\nexport declare const v: any;\nexport declare const typed: Box<Private>;\nexport {};\n"),
        ("original_class", "const original=class{};export const cast=original as any;", "export declare const cast: any;\n"),
        ("original_literal", "const original=1;export const cast=original as any;", "export declare const cast: any;\n"),
        ("original_constructor", "class Private{};const original=new Private();export const cast=original as any;", "export declare const cast: any;\n"),
        ("original_generic_constructor", "class Box<T>{};class Private{};const original=new Box<Private>();export const cast=original as any;", "export declare const cast: any;\n"),
        ("cached_number_assertion", "const original=1;const cast=original as any;export const value=cast as number;", "export declare const value: number;\n"),
        ("cached_null_assertion", "const original=null;const cast=original as any;export const value=cast as null;", "export declare const value: null;\n"),
        ("exported_number_cast_assertion", "const original=1;export const cast=original as any;export const value=cast as number;", "export declare const cast: any;\nexport declare const value: number;\n"),
        ("exported_null_cast_assertion", "const original=null;export const cast=original as any;export const value=cast as null;", "export declare const cast: any;\nexport declare const value: null;\n"),
        ("cached_class_assertion", "class C{};const original=new C();const cast=original as any;export const value=cast as C;", "declare class C {\n}\nexport declare const value: C;\nexport {};\n"),
        ("exported_class_cast_assertion", "class C{};const original=new C();export const cast=original as any;export const value=cast as C;", "declare class C {\n}\nexport declare const cast: any;\nexport declare const value: C;\nexport {};\n"),
        ("namespace_private", "export namespace N{class Private{};export const v=new Private() as any;}", "export declare namespace N {\n    const v: any;\n}\n"),
        ("namespace_shadow", "class Box<T>{};export namespace N{class Box{};export const v=new Box() as any;}", "export declare namespace N {\n    const v: any;\n}\n"),
        ("namespace_outer_private", "class Private{};export namespace N{export const v=new Private() as any;}", "export declare namespace N {\n    const v: any;\n}\n"),
        ("private_same_named", "class Hidden{};const original=new Hidden();export const cast=original as any;export const other:Hidden=original;", "declare class Hidden {\n}\nexport declare const cast: any;\nexport declare const other: Hidden;\nexport {};\n"),
        ("explicit_return_assertion", "export function f():null{return null as any;}", "export declare function f(): null;\n"),
        ("constructor_parameter_assertion", "export class C{constructor(public value=null as null){}}", "export declare class C {\n    value: null;\n    constructor(value?: null);\n}\n"),
        ("fresh_null_asserted", "const fresh=null;export const value=fresh as null;", "export declare const value: null;\n"),
        ("fresh_null_angle", "const fresh=null;export const value=<null>fresh;", "export declare const value: null;\n"),
        ("fresh_null_function", "const fresh=null;export function f(){return fresh as null;}", "export declare function f(): null;\n"),
        ("fresh_null_property", "const fresh=null;export class C{value=fresh as null;}", "export declare class C {\n    value: null;\n}\n"),
        ("fresh_null_parameter", "const fresh=null;export class C{constructor(public value=fresh as null){}}", "export declare class C {\n    value: null;\n    constructor(value?: null);\n}\n"),
        ("object_any", "export const v={x:1} as any;", "export declare const v: any;\n"),
        ("array_any", "export const v=[1,2] as any;", "export declare const v: any;\n"),
        ("function_any", "export const f=((x:string):string=>x) as any;", "export declare const f: any;\n"),
        ("function_literal_any", "export const f=function(x:string){return x} as any;", "export declare const f: any;\n"),
        ("array_member", "const arr=[1];export const value=arr[0] as any;", "export declare const value: any;\n"),
        ("satisfies_outside_nonnull", "export const v=((null as any) satisfies null)!;", "export declare const v: any;\n"),
        ("explicit_outer_type_alias", "type N=null;export const v:N=null as any;", "type N = null;\nexport declare const v: N;\nexport {};\n"),
        ("ordinary_literals", "export const n=1,s=\"x\",b=true;", "export declare const n = 1, s = \"x\", b = true;\n"),
        ("parenthesized_nonnull", "export const v=(1 as (any))!;", "export declare const v: any;\n"),
        ("parenthesized_satisfies", "export const v=(1 as (any)) satisfies string;", "export declare const v: any;\n"),
        ("parenthesized_angle", "export const v=<(any)>1;", "export declare const v: (any);\n"),
        ("parenthesized_nested", "export const v=1 as (((any)));", "export declare const v: (((any)));\n"),
        ("parenthesized_variable", "let v=1 as (any);", "declare let v: (any);\n"),
        ("parenthesized_private", "class Hidden{};export const v=new Hidden() as (any);", "export declare const v: (any);\n"),
        ("parenthesized_outer_any", "export const v=(1 as (any)) as any;", "export declare const v: any;\n"),
        ("parenthesized_any_alias", "export const v=1 as any;export const x=v as (any);", "export declare const v: any;\nexport declare const x: (any);\n"),
        ("parenthesized_explicit", "export const v:(any)=1 as any;", "export declare const v: (any);\n"),
    ];
    for (strict, strict_null_checks) in [
        (None, None),
        (Some(false), None),
        (Some(true), None),
        (Some(true), Some(false)),
        (Some(false), Some(true)),
        (None, Some(false)),
        (None, Some(true)),
    ] {
        let options = CompilerOptions {
            declaration: Some(true),
            target: Some(ScriptTarget::ES2015),
            strict,
            strict_null_checks,
            ..Default::default()
        };
        for (name, source, expected) in cases {
            let file = tsc_rs_parser::parse("test.ts", source);
            assert_eq!(
                emit(&file, &options).declaration_file.unwrap(),
                expected,
                "{name}: strict={strict:?}, strictNullChecks={strict_null_checks:?}"
            );
        }
    }
}

#[test]
fn test_dts_primitive_default_exports_preserve_types_and_unique_names() {
    // Complete outputs verified against TypeScript 6.0.3. The source includes
    // collisions in nested scopes, Unicode names, literals and arithmetic.
    let cases = [
        ("number", "export default 3.14159;", "declare const _default: 3.14159;\nexport default _default;\n"),
        ("string", "export default \"a\\n\\\"b\";", "declare const _default: \"a\\n\\\"b\";\nexport default _default;\n"),
        ("boolean", "export default true;", "declare const _default: true;\nexport default _default;\n"),
        ("false", "export default false;", "declare const _default: false;\nexport default _default;\n"),
        ("bigint", "export default 0x12n;", "declare const _default: 0x12n;\nexport default _default;\n"),
        ("negative", "export default -12;", "declare const _default: -12;\nexport default _default;\n"),
        ("positive", "export default +12;", "declare const _default: 12;\nexport default _default;\n"),
        ("negative_bigint", "export default -12n;", "declare const _default: -12n;\nexport default _default;\n"),
        ("hex", "export default 0x12;", "declare const _default: 18;\nexport default _default;\n"),
        ("separators", "export default 1_234;", "declare const _default: 1234;\nexport default _default;\n"),
        ("parenthesized", "export default (((1)));", "declare const _default: number;\nexport default _default;\n"),
        ("arithmetic", "export default 1+2;", "declare const _default: number;\nexport default _default;\n"),
        ("arithmetic_nested", "export default (1+2)*(3-4)/5;", "declare const _default: number;\nexport default _default;\n"),
        ("shift", "export default 1<<2;", "declare const _default: number;\nexport default _default;\n"),
        ("bitwise", "export default 1&2|3^4;", "declare const _default: number;\nexport default _default;\n"),
        ("remainder", "export default 5%2;", "declare const _default: number;\nexport default _default;\n"),
        ("exponent", "export default 5**2;", "declare const _default: number;\nexport default _default;\n"),
        ("bigint_arithmetic", "export default 1n+2n;", "declare const _default: bigint;\nexport default _default;\n"),
        ("string_concat", "export default \"a\"+\"b\";", "declare const _default: string;\nexport default _default;\n"),
        ("mixed_concat", "export default \"a\"+1;", "declare const _default: string;\nexport default _default;\n"),
        ("undefined", "export default undefined;", "export default undefined;\n"),
        ("identifier", "const value=1;export default value;", "declare const value = 1;\nexport default value;\n"),
        ("function_decl", "export default function f(x:number):number{return x;}", "export default function f(x: number): number;\n"),
        ("class_decl", "export default class C{}", "export default class C {\n}\n"),
        ("collision", "var _default=1;export {_default as d};export default 1+2;", "declare var _default: number;\nexport { _default as d };\ndeclare const _default_1: number;\nexport default _default_1;\n"),
        ("collision_suffix", "const _default=1,_default_1=2;export default 3;", "declare const _default_2: 3;\nexport default _default_2;\n"),
        ("collision_type", "type _default=number;export default 1;", "declare const _default_1: 1;\nexport default _default_1;\n"),
        ("collision_parameter", "function f(_default:number){};export default 1;", "declare const _default_1: 1;\nexport default _default_1;\n"),
        ("collision_namespace", "namespace N{const _default=1;}export default 1;", "declare const _default_1: 1;\nexport default _default_1;\n"),
        ("collision_property", "const v={_default:1};export default 1;", "declare const _default_1: 1;\nexport default _default_1;\n"),
        ("collision_reference", "const v=_default;export default 1;", "declare const _default_1: 1;\nexport default _default_1;\n"),
        ("collision_unicode", "const \\u005fdefault=1;export default 2;", "declare const _default_1: 2;\nexport default _default_1;\n"),
        ("collision_unicode_suffix", "const _default=1,\\u005fdefault_1=2;export default 3;", "declare const _default_2: 3;\nexport default _default_2;\n"),
        ("collision_exportalias", "const v=1;export {v as _default};export default 2;", "declare const v = 1;\nexport { v as _default };\ndeclare const _default_1: 2;\nexport default _default_1;\n"),
        ("ordinary_regex", "const text=/_default/;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("ordinary_comment", "/* _default */ export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("collision_template_substitution", "const text=`x${_default}`;export default 1;", "declare const _default_1: 1;\nexport default _default_1;\n"),
        ("duplicate_default", "export default 1;export default 2;", "declare const _default: 1;\nexport default _default;\ndeclare const _default_1: 2;\nexport default _default_1;\n"),
        ("namespace_default", "export namespace N{export default 1;}", "export declare namespace N {\n    const _default: 1;\n    export default _default;\n}\n"),
        ("ambient_module", "declare module \"pkg\"{export default 1;}", "declare module \"pkg\" {\n    const _default: 1;\n    export default _default;\n}\n"),
        ("sibling_module", "declare module \"a\"{export default 1;}declare module \"b\"{export default 2;}", "declare module \"a\" {\n    const _default: 1;\n    export default _default;\n}\ndeclare module \"b\" {\n    const _default_1: 2;\n    export default _default_1;\n}\n"),
        ("shadow_nested", "export namespace N{const _default=1;export default 2;}", "export declare namespace N {\n    const _default_1: 2;\n    export default _default_1;\n}\n"),
        ("explicit_outer_annotation", "const value:number=1;export default value;", "declare const value: number;\nexport default value;\n"),
        ("private_signature", "interface Hidden{x:1};export default function f(x:Hidden):Hidden{return x;}", "interface Hidden {\n    x: 1;\n}\nexport default function f(x: Hidden): Hidden;\nexport {};\n"),
        ("bigint_upper", "export default 0XFn;", "declare const _default: 0xfn;\nexport default _default;\n"),
        ("bigint_separators", "export default 1_234n;", "declare const _default: 1234n;\nexport default _default;\n"),
        ("bigint_hex_separators", "export default 0x1_2n;", "declare const _default: 0x12n;\nexport default _default;\n"),
        ("bigint_negative_hex", "export default -0x12n;", "declare const _default: -0x12n;\nexport default _default;\n"),
        ("bigint_zero", "export default -0n;", "declare const _default: -0n;\nexport default _default;\n"),
        ("bigint_parenthesized", "export default (12n);", "declare const _default: bigint;\nexport default _default;\n"),
        ("parenthesized_string", "export default (\"a\");", "declare const _default: string;\nexport default _default;\n"),
        ("parenthesized_bool", "export default (true);", "declare const _default: boolean;\nexport default _default;\n"),
        ("parenthesized_negative", "export default (-12);", "declare const _default: number;\nexport default _default;\n"),
        ("parenthesized_unary", "export default -(12);", "declare const _default: number;\nexport default _default;\n"),
        ("template_literal", "export default `abc`;", "declare const _default: \"abc\";\nexport default _default;\n"),
    ];
    for (strict, strict_null_checks) in [
        (None, None),
        (Some(false), None),
        (Some(true), None),
        (Some(true), Some(false)),
        (Some(false), Some(true)),
        (None, Some(false)),
        (None, Some(true)),
    ] {
        let options = CompilerOptions {
            declaration: Some(true),
            target: Some(ScriptTarget::ES2015),
            strict,
            strict_null_checks,
            ..Default::default()
        };
        for (name, source, expected) in cases {
            let file = tsc_rs_parser::parse("test.ts", source);
            assert_eq!(
                emit(&file, &options).declaration_file.unwrap(),
                expected,
                "{name}: strict={strict:?}, strictNullChecks={strict_null_checks:?}"
            );
        }
    }
}

#[test]
fn test_dts_default_export_lexical_names_and_signed_zero() {
    // TypeScript 6.0.3 outputs. JSX-mode comments and literal text must not be
    // treated as JSX syntax; regex bodies must not reserve identifier names.
    let cases = [
        ("regex_text", "probe.ts", "const r=/_default/;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("regex_class", "probe.ts", "const r=/[_default]/;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("regex_unicode", "probe.ts", "const r=/\\u005fdefault/;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("regex_escaped", "probe.ts", "const r=/\\x5fdefault/;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("regex_nested", "probe.ts", "const r=/(?:_default|[_default])/;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("interpolated_template", "probe.ts", "const x=`${_default}`;export default 1;", "declare const _default_1: 1;\nexport default _default_1;\n"),
        ("raw_template_substitution", "probe.ts", "const x=`_default${1}`;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("interleaved_function", "probe.ts", "export default 1;export default function(){};export default 2;", "declare const _default: 1;\nexport default _default;\nexport default function (): void;\ndeclare const _default_1: 2;\nexport default _default_1;\n"),
        ("interleaved_class", "probe.ts", "export default 1;export default class{};export default 2;", "declare const _default: 1;\nexport default _default;\nexport default class {\n}\ndeclare const _default_1: 2;\nexport default _default_1;\n"),
        ("function_before", "probe.ts", "export default function(){};export default 1;", "export default function (): void;\ndeclare const _default: 1;\nexport default _default;\n"),
        ("class_before", "probe.ts", "export default class{};export default 1;", "export default class {\n}\ndeclare const _default: 1;\nexport default _default;\n"),
        ("number_negative_zero", "probe.ts", "export default -0;", "declare const _default: -0;\nexport default _default;\n"),
        ("number_exp", "probe.ts", "export default 1e3;", "declare const _default: 1000;\nexport default _default;\n"),
        ("number_fraction", "probe.ts", "export default .5;", "declare const _default: 0.5;\nexport default _default;\n"),
        ("number_parenthesized_zero", "probe.ts", "export default (-0);", "declare const _default: number;\nexport default _default;\n"),
        ("number_positive_zero", "probe.ts", "export default +0;", "declare const _default: 0;\nexport default _default;\n"),
        ("bigint_zero", "probe.ts", "export default 0n;", "declare const _default: 0n;\nexport default _default;\n"),
        ("bigint_negative_zero", "probe.ts", "export default -0n;", "declare const _default: -0n;\nexport default _default;\n"),
        ("bigint_parenthesized_zero", "probe.ts", "export default (-0n);", "declare const _default: bigint;\nexport default _default;\n"),
        ("jsx_plain_literal", "probe.tsx", "export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("jsx_angle_comment", "probe.tsx", "/* < _default */export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("jsx_angle_string", "probe.tsx", "const s=\"< _default\";export default 1;", "declare const _default: 1;\nexport default _default;\n"),
        ("jsx_angle_template", "probe.tsx", "const s=`< _default`;export default 1;", "declare const _default: 1;\nexport default _default;\n"),
    ];
    for (strict, strict_null_checks) in [
        (None, None),
        (Some(false), None),
        (Some(true), None),
        (Some(true), Some(false)),
        (Some(false), Some(true)),
        (None, Some(false)),
        (None, Some(true)),
    ] {
        let options = CompilerOptions {
            declaration: Some(true),
            target: Some(ScriptTarget::ES2015),
            strict,
            strict_null_checks,
            ..Default::default()
        };
        for (name, file_name, source, expected) in cases {
            let file = tsc_rs_parser::parse(file_name, source);
            assert_eq!(
                emit(&file, &options).declaration_file.unwrap(),
                expected,
                "{name}: strict={strict:?}, strictNullChecks={strict_null_checks:?}"
            );
        }
    }
}

// TypeScript 6.0.3 still emits declarations for invalid binding-pattern
// parameter properties (TS1187). These expectations include that recovery.
fn assert_constructor_pattern_declarations(cases: &[(&str, &str)]) {
    for (source, expected) in cases {
        let file = tsc_rs_parser::parse("probe.ts", source);
        for (strict, strict_null_checks) in [
            (None, None),
            (Some(false), None),
            (Some(true), None),
            (Some(true), Some(false)),
            (Some(false), Some(true)),
            (None, Some(false)),
            (None, Some(true)),
        ] {
            let options = CompilerOptions {
                declaration: Some(true),
                strict,
                strict_null_checks,
                ..Default::default()
            };
            assert_eq!(
                emit(&file, &options).declaration_file.as_deref(),
                Some(*expected),
                "{source}; strict={strict:?}, strictNullChecks={strict_null_checks:?}"
            );
        }
    }
}

#[test]
fn test_dts_constructor_patterns_structural() {
    assert_constructor_pattern_declarations(&[
        ("class C { constructor(public [x,y]: string[]) {} }", "declare class C {\n    x: string;\n    y: string;\n    constructor([x, y]: string[]);\n}\n"),
        ("class C { constructor(public {x,y}: {x:number;y:string}) {} }", "declare class C {\n    x: number;\n    y: string;\n    constructor({ x, y }: {\n        x: number;\n        y: string;\n    });\n}\n"),
        ("class C { constructor(public [,x,,y]:[boolean,string,object,number]) {} }", "declare class C {\n    x: string;\n    y: number;\n    constructor([, x, , y]: [boolean, string, object, number]);\n}\n"),
        ("class C { constructor(public {x:[a,b],y:{z}}:{x:[string,number];y:{z:boolean}}) {} }", "declare class C {\n    a: string;\n    b: number;\n    z: boolean;\n    constructor({ x: [a, b], y: { z } }: {\n        x: [string, number];\n        y: {\n            z: boolean;\n        };\n    });\n}\n"),
        ("class C{constructor(public {\"a-b\":x}:{\"a-b\":number}){}}", "declare class C {\n    x: number;\n    constructor({ \"a-b\": x }: {\n        \"a-b\": number;\n    });\n}\n"),
        ("class C{constructor(public {1:x}:{1:number}){}}", "declare class C {\n    x: number;\n    constructor({ 1: x }: {\n        1: number;\n    });\n}\n"),
        ("class C{constructor(public[x]:{a:number}[]){}}", "declare class C {\n    x: {\n        a: number;\n    };\n    constructor([x]: {\n        a: number;\n    }[]);\n}\n"),
        ("class C{constructor(public[x]:[number,string][]){}}", "declare class C {\n    x: [number, string];\n    constructor([x]: [number, string][]);\n}\n"),
        ("class C { constructor(public {x,y}:{x:1;y:\"a\"}) {} }", "declare class C {\n    x: 1;\n    y: \"a\";\n    constructor({ x, y }: {\n        x: 1;\n        y: \"a\";\n    });\n}\n"),
        ("class C { constructor(public {x,y}:{x:null;y:undefined}) {} }", "declare class C {\n    x: null;\n    y: undefined;\n    constructor({ x, y }: {\n        x: null;\n        y: undefined;\n    });\n}\n"),
    ]);
}

#[test]
fn test_dts_constructor_patterns_aliases() {
    assert_constructor_pattern_declarations(&[
        ("type A=string[]; class C { constructor(public [x,y]: A) {} }", "type A = string[];\ndeclare class C {\n    x: string;\n    y: string;\n    constructor([x, y]: A);\n}\n"),
        ("type B=A; type A=[string,number]; class C { constructor(public [x,y]: B) {} }", "type B = A;\ntype A = [string, number];\ndeclare class C {\n    x: string;\n    y: number;\n    constructor([x, y]: B);\n}\n"),
        ("class C { constructor(public [x,y]: A) {} } type A=[string,number];", "declare class C {\n    x: string;\n    y: number;\n    constructor([x, y]: A);\n}\ntype A = [string, number];\n"),
        ("namespace N { type A=[string,number]; export class C { constructor(public [x,y]: A) {} } }", "declare namespace N {\n    type A = [string, number];\n    export class C {\n        x: string;\n        y: number;\n        constructor([x, y]: A);\n    }\n    export {};\n}\n"),
        ("type A=[string,number]; namespace N { type A=[boolean,bigint]; export class C { constructor(public [x,y]: A) {} } }", "type A = [string, number];\ndeclare namespace N {\n    type A = [boolean, bigint];\n    export class C {\n        x: boolean;\n        y: bigint;\n        constructor([x, y]: A);\n    }\n    export {};\n}\n"),
        ("namespace A{type T=[number];export class C{constructor(public[x]:T){}}} namespace B{type T=[string];export class C{constructor(public[x]:T){}}}", "declare namespace A {\n    type T = [number];\n    export class C {\n        x: number;\n        constructor([x]: T);\n    }\n    export {};\n}\ndeclare namespace B {\n    type T = [string];\n    export class C {\n        x: string;\n        constructor([x]: T);\n    }\n    export {};\n}\n"),
        ("type A=[number];const A=1;class C{constructor(public[x]:A){}}", "type A = [number];\ndeclare const A = 1;\ndeclare class C {\n    x: number;\n    constructor([x]: A);\n}\n"),
        ("type P=number; type A={x:P}; class C { constructor(public {x}:A) {} }", "type P = number;\ntype A = {\n    x: P;\n};\ndeclare class C {\n    x: number;\n    constructor({ x }: A);\n}\n"),
    ]);
}

#[test]
fn test_dts_constructor_patterns_generic_scope() {
    assert_constructor_pattern_declarations(&[
        ("class C<T> { constructor(public [x,y]: T[]) {} }", "declare class C<T> {\n    x: T;\n    y: T;\n    constructor([x, y]: T[]);\n}\n"),
        ("type T=number; type A={x:T}; class C<T> { constructor(public {x}:A) {} }", "type T = number;\ntype A = {\n    x: T;\n};\ndeclare class C<T> {\n    x: number;\n    constructor({ x }: A);\n}\n"),
        ("class H{};class C<H>{constructor(public[x]:H[]){}}", "declare class H {\n}\ndeclare class C<H> {\n    x: H;\n    constructor([x]: H[]);\n}\n"),
        ("class Hidden{};namespace N{class Hidden{};type A={x:Hidden};export class C{constructor(public{x}:A){}}}", "declare class Hidden {\n}\ndeclare namespace N {\n    class Hidden {\n    }\n    type A = {\n        x: Hidden;\n    };\n    export class C {\n        x: Hidden;\n        constructor({ x }: A);\n    }\n    export {};\n}\n"),
        ("type T=number;type A=T[];class C<T>{constructor(public[x]:A){}}", "type T = number;\ntype A = T[];\ndeclare class C<T> {\n    x: number;\n    constructor([x]: A);\n}\n"),
    ]);
}

#[test]
fn test_dts_constructor_patterns_dependencies() {
    assert_constructor_pattern_declarations(&[
        ("class Hidden{}; type A={x:Hidden}; export class C { constructor(public {x}:A) {} }", "declare class Hidden {\n}\ntype A = {\n    x: Hidden;\n};\nexport declare class C {\n    x: Hidden;\n    constructor({ x }: A);\n}\nexport {};\n"),
        ("interface Hidden{x:number}; type A={x:Hidden}; export class C { constructor(public {x}:A) {} }", "interface Hidden {\n    x: number;\n}\ntype A = {\n    x: Hidden;\n};\nexport declare class C {\n    x: Hidden;\n    constructor({ x }: A);\n}\nexport {};\n"),
        ("import {Hidden} from \"./other\"; type A={x:Hidden}; export class C { constructor(public {x}:A) {} }", "import { Hidden } from \"./other\";\ntype A = {\n    x: Hidden;\n};\nexport declare class C {\n    x: Hidden;\n    constructor({ x }: A);\n}\nexport {};\n"),
        ("class Hidden{}; type A={x:{y:Hidden}}; export class C { constructor(public {x}:A) {} }", "declare class Hidden {\n}\ntype A = {\n    x: {\n        y: Hidden;\n    };\n};\nexport declare class C {\n    x: {\n        y: Hidden;\n    };\n    constructor({ x }: A);\n}\nexport {};\n"),
        ("export namespace N { class Hidden{}; type A={x:Hidden}; export class C { constructor(public {x}:A) {} } }", "export declare namespace N {\n    class Hidden {\n    }\n    type A = {\n        x: Hidden;\n    };\n    export class C {\n        x: Hidden;\n        constructor({ x }: A);\n    }\n    export {};\n}\n"),
        ("class H{};type A={x:H};export class C{constructor(private{x}:A){}}", "declare class H {\n}\ntype A = {\n    x: H;\n};\nexport declare class C {\n    private x: H;\n    constructor({ x }: A);\n}\nexport {};\n"),
    ]);
}

#[test]
fn test_dts_constructor_patterns_rest_and_defaults() {
    assert_constructor_pattern_declarations(&[
        ("class C { constructor(public [x,...rest]:string[]) {} }", "declare class C {\n    x: string;\n    rest: string[];\n    constructor([x, ...rest]: string[]);\n}\n"),
        ("class C { constructor(public [x,...rest]:[string,number,boolean]) {} }", "declare class C {\n    x: string;\n    rest: [number, boolean];\n    constructor([x, ...rest]: [string, number, boolean]);\n}\n"),
        ("class C { constructor(public {x,...rest}:{x:number;y:string}) {} }", "declare class C {\n    x: number;\n    rest: {\n        y: string;\n    };\n    constructor({ x, ...rest }: {\n        x: number;\n        y: string;\n    });\n}\n"),
        ("class C { constructor(public [x,...rest]:readonly string[]) {} }", "declare class C {\n    x: string;\n    rest: string[];\n    constructor([x, ...rest]: readonly string[]);\n}\n"),
        ("class C { constructor(public {x,...rest}:{readonly x:number;readonly y:string}) {} }", "declare class C {\n    x: number;\n    rest: {\n        y: string;\n    };\n    constructor({ x, ...rest }: {\n        readonly x: number;\n        readonly y: string;\n    });\n}\n"),
        ("class C{constructor(public[x,...rest]:[number]){}}", "declare class C {\n    x: number;\n    rest: [];\n    constructor([x, ...rest]: [number]);\n}\n"),
        ("class C{constructor(public{x,...rest}:{x:number}){}}", "declare class C {\n    x: number;\n    rest: {};\n    constructor({ x, ...rest }: {\n        x: number;\n    });\n}\n"),
        ("class C{constructor(public{...rest,x}:{x:number;y:string}){}}", "declare class C {\n    rest: {\n        y: string;\n    };\n    x: number;\n    constructor({ ...rest, x }: {\n        x: number;\n        y: string;\n    });\n}\n"),
        ("class C{constructor(public[...rest,x]:number[]){}}", "declare class C {\n    rest: number[];\n    x: number;\n    constructor([...rest, x]: number[]);\n}\n"),
        ("class C { constructor(public {x=1,y=\"a\"}:{x:number;y:string}) {} }", "declare class C {\n    x: number;\n    y: string;\n    constructor({ x, y }: {\n        x: number;\n        y: string;\n    });\n}\n"),
        ("class C { constructor(public {x}:{x:number}={x:1}) {} }", "declare class C {\n    x: number;\n    constructor({ x }?: {\n        x: number;\n    });\n}\n"),
        ("class C{constructor(public ...[x,y]:number[]){}}", "declare class C {\n    x: number;\n    y: number;\n    constructor(...[x, y]: number[]);\n}\n"),
    ]);
}

#[test]
fn test_dts_constructor_patterns_duplicates() {
    assert_constructor_pattern_declarations(&[
        ("class C { constructor(private [x,y]: string[]) {} }", "declare class C {\n    private x: string;\n    private y: string;\n    constructor([x, y]: string[]);\n}\n"),
        ("class C { constructor(public [x,x]:[string,number]) {} }", "declare class C {\n    x: string;\n    x: number;\n    constructor([x, x]: [string, number]);\n}\n"),
        ("class C { constructor(public {x:a,x:b}:{x:number}) {} }", "declare class C {\n    a: number;\n    b: number;\n    constructor({ x: a, x: b }: {\n        x: number;\n    });\n}\n"),
        ("class C { x:string; constructor(public [x]:number[]) {} }", "declare class C {\n    x: number;\n    x: string;\n    constructor([x]: number[]);\n}\n"),
        ("class C { constructor(public x:number,public [x,y]:string[]) {} }", "declare class C {\n    x: number;\n    x: string;\n    y: string;\n    constructor(x: number, [x, y]: string[]);\n}\n"),
    ]);
}

#[test]
fn test_dts_constructor_patterns_ordinary_controls() {
    assert_constructor_pattern_declarations(&[
        ("class C { constructor(public x:number, private y:string,readonly z:boolean) {} }", "declare class C {\n    x: number;\n    private y;\n    readonly z: boolean;\n    constructor(x: number, y: string, z: boolean);\n}\n"),
        ("type T=number; export namespace N{type T=string;export class C{constructor(public x:T,private y:T){}}}", "export declare namespace N {\n    type T = string;\n    export class C {\n        x: T;\n        private y;\n        constructor(x: T, y: T);\n    }\n    export {};\n}\n"),
        ("class Hidden{}; export class C{constructor(public x:Hidden,private y:Hidden){}}", "declare class Hidden {\n}\nexport declare class C {\n    x: Hidden;\n    private y;\n    constructor(x: Hidden, y: Hidden);\n}\nexport {};\n"),
        ("type T=number; export class C<T>{constructor(public x:T){}}", "export declare class C<T> {\n    x: T;\n    constructor(x: T);\n}\n"),
        ("class C{constructor(public []:number[]){}}", "declare class C {\n    constructor([]: number[]);\n}\n"),
        ("export function f({x}:{x:number}):number{return x;}", "export declare function f({ x }: {\n    x: number;\n}): number;\n"),
        ("class C{constructor([x,y]:[string,number]){}}", "declare class C {\n    constructor([x, y]: [string, number]);\n}\n"),
    ]);
}

#[test]
fn test_dts_constructor_patterns_do_not_emit_partial_unsupported_types() {
    for source in [
        "class C{constructor(public{x,y}:{x:number;y:number|string}){}}",
        "type A<T>=[T,T]; class C<T>{constructor(public[x,y]:A<T>){}}",
        "class C{constructor(public{x=1}:{x:undefined}){}}",
        "class C{constructor(public{x:y=1}:{x:void}){}}",
        "type U=unknown;class C{constructor(public[x=1]:U[]){}}",
        "class C<T extends number|undefined>{constructor(public{x=1}:{x:T}){}}",
    ] {
        let output = emit_dts(source);
        assert!(output.contains("class C"), "{output}");
        assert!(!output.contains("\n    x:"), "{output}");
        assert!(!output.contains("\n    y:"), "{output}");
    }
    let aliases = (0..36)
        .map(|i| format!("type A{i}=A{};", i + 1))
        .collect::<String>();
    let source = format!(
        "{aliases}type A36=number;class C{{constructor(public{{x,y}}:{{x:number;y:A0}}){{}}}}"
    );
    let output = emit_dts(&source);
    assert!(!output.contains("\n    x:"), "{output}");
    assert!(!output.contains("\n    y:"), "{output}");
}

#[test]
fn test_dts_constructor_patterns_defer_ambiguous_declarations() {
    let output =
        emit_dts("class C{constructor(public[x]:number[]){} constructor(public[x]:string[]){}}");
    assert!(!output.contains("\n    x: string;"), "{output}");
    let output = emit_dts("namespace N{export type A=[number];} namespace N{export type A=[string];export class C{constructor(public[x]:A){}}}");
    assert!(!output.contains("\n        x: string;"), "{output}");
}

#[test]
fn test_dts_constructor_patterns_defaults_without_semantic_narrowing() {
    assert_constructor_pattern_declarations(&[
        ("class C{constructor(public {x=1}:{x:any}){}}", "declare class C {\n    x: any;\n    constructor({ x }: {\n        x: any;\n    });\n}\n"),
        ("class C{constructor(public {x:y=1}:{x:number}){}}", "declare class C {\n    y: number;\n    constructor({ x: y }: {\n        x: number;\n    });\n}\n"),
        ("class C{constructor(public [x=1]:null[]){}}", "declare class C {\n    x: null;\n    constructor([x]: null[]);\n}\n"),
        ("class C{constructor(public[{x}={x:1}]:{x:number}[]){}}", "declare class C {\n    x: number;\n    constructor([{ x }]: {\n        x: number;\n    }[]);\n}\n"),
        ("class C{constructor(public[[x]=[1]]:number[][]){}}", "declare class C {\n    x: number;\n    constructor([[x]]: number[][]);\n}\n"),
    ]);
}

#[test]
fn shorthand_function_properties_preserve_generic_declaration_identity() {
    assert_eq!(
        emit_dts("function keys<T>(...names: (keyof T)[]) {} const utility = { keys };"),
        "declare function keys<T>(...names: (keyof T)[]): void;\ndeclare const utility: {\n    keys: typeof keys;\n};\n"
    );
}

#[test]
fn exported_shorthand_objects_retain_private_overloads_and_dependencies() {
    assert_eq!(
        emit_dts("interface Item { value: string; } function f(x: Item): Item; function f(x: string): string; function f(x: any): any { return x; } export const utility = { f };"),
        "interface Item {\n    value: string;\n}\ndeclare function f(x: Item): Item;\ndeclare function f(x: string): string;\nexport declare const utility: {\n    f: typeof f;\n};\nexport {};\n"
    );
}

#[test]
fn shorthand_function_objects_keep_property_and_declaration_order() {
    assert_eq!(
        emit_dts("const utility = { g, f }; function f() {} function g<T>(x: T): T { return x; } let other = { f };"),
        "declare const utility: {\n    g: typeof g;\n    f: typeof f;\n};\ndeclare function f(): void;\ndeclare function g<T>(x: T): T;\ndeclare let other: {\n    f: typeof f;\n};\n"
    );
}

#[test]
fn shorthand_inference_respects_explicit_types_and_nested_scopes() {
    assert_eq!(
        emit_dts("function f() {} const typed: { f: () => void } = { f }; namespace N { export const utility: number = 1; } function g(f: number): number { const utility = { f }; return utility.f; } const utility = { f };"),
        "declare function f(): void;\ndeclare const typed: {\n    f: () => void;\n};\ndeclare namespace N {\n    const utility: number;\n}\ndeclare function g(f: number): number;\ndeclare const utility: {\n    f: typeof f;\n};\n"
    );
}

#[test]
fn shorthand_objects_do_not_expose_uncertain_private_return_inference() {
    // These unannotated returns are not modeled precisely yet. Retaining the
    // private function would newly publish an incorrect widened/partial type.
    for source in [
        "function f() { return 1 as const; } export const utility = { f };",
        "function f() { if (Math.random()) return 1; return 'x'; } export const utility = { f };",
        "function f(x = { value: 1 }) {} export const utility = { f };",
        "type Promise<T> = { value: T }; async function f() {} export const utility = { f };",
    ] {
        assert_eq!(emit_dts(source), "export declare const utility;\n");
    }
    assert_eq!(
        emit_dts("function f(): 1 { return 1 as const; } export const utility = { f };"),
        "declare function f(): 1;\nexport declare const utility: {\n    f: typeof f;\n};\nexport {};\n"
    );
}

#[test]
fn interface_declarations_omit_parenthesized_heritage_expressions() {
    assert_eq!(
        emit_dts("class A {} interface I extends (typeof A) { value: number; } interface J extends (A) {}"),
        "declare class A {\n}\ninterface I {\n    value: number;\n}\ninterface J {\n}\n"
    );
    assert_eq!(
        emit_dts("interface I extends (Ignored), Valid<string>, Namespace.Base<number>, (typeof Missing) {}"),
        "interface I extends Valid<string>, Namespace.Base<number> {\n}\n"
    );
}

#[test]
fn omitted_interface_heritage_does_not_retain_private_dependencies() {
    assert_eq!(
        emit_dts(
            "class Hidden {} class Kept {} export interface I extends (Hidden) { value: Kept; }"
        ),
        "declare class Kept {\n}\nexport interface I {\n    value: Kept;\n}\nexport {};\n"
    );
    assert_eq!(
        emit_dts("namespace N { class Hidden {} export interface I extends (typeof Hidden) {} }"),
        "declare namespace N {\n    interface I {\n    }\n}\n"
    );
}

#[test]
fn interface_heritage_keeps_keyword_names_and_default_export_spelling() {
    assert_eq!(
        emit_dts("export default interface Named extends (Missing), any, string {}"),
        "export default interface Named extends any, string {\n}\n"
    );
}

#[test]
fn interface_heritage_defers_incomplete_parenthesized_recovery() {
    // A recovered type node has no closing delimiter. Keep its existing emit
    // behavior and parser diagnostic until recovery is modeled independently.
    let source = "interface I extends (A { value: number; }";
    let file = tsc_rs_parser::parse("test.ts", source);
    assert!(file
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == 1005));
    assert_eq!(
        emit_dts(source),
        "interface I extends (A) {\n    value: number;\n}\n"
    );
    for source in [
        "interface I extends ((A) { x?: number; }",
        "interface I extends (((A)) { x?: number; }",
    ] {
        let file = tsc_rs_parser::parse("test.ts", source);
        assert!(file
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 1005));
        let parentheses = if source.contains("(((A)") {
            "(((A)))"
        } else {
            "((A))"
        };
        assert_eq!(
            emit_dts(source),
            format!("interface I extends {parentheses} {{\n    x?: number;\n}}\n")
        );
    }
}
