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
    assert!(dts.contains("import * as path from \"path\";"));
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
