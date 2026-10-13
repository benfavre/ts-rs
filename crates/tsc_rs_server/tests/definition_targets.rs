//! Exact definition locations independently captured with the TypeScript
//! language service. These also check every target for merged declarations.
use tsc_rs_ast::{CompilerOptions, Span};
use tsc_rs_query::QueryEngine;
use tsc_rs_server::harness_api::{definitions_at, ProjectState};

fn check(
    files: &[(&str, &str)],
    marker: &str,
    position: u32,
    roots: &[&str],
    experimental: bool,
    expected: &[(&str, u32, u32)],
) {
    let mut options = CompilerOptions::default();
    options.no_lib = Some(false);
    options.allow_js = Some(true);
    options.experimental_decorators = Some(experimental);
    if !roots.is_empty() {
        options.type_roots = Some(roots.iter().map(|s| s.to_string()).collect());
    }
    let mut engine = QueryEngine::with_options(options);
    for (name, source) in files {
        engine
            .add_source(name.to_string(), source.to_string())
            .unwrap();
    }
    let _ = engine.check_all();
    let source = files.iter().find(|(file, _)| *file == marker).unwrap().1;
    let sf = tsc_rs_parser::parse(marker, source);
    let symbols = engine.get_file_symbols(marker).unwrap();
    let output = engine.get_file_check_output(marker).unwrap();
    let project = ProjectState::from_file_map(
        files
            .iter()
            .filter(|(file, _)| *file != marker)
            .map(|(file, source)| {
                (
                    file.to_string(),
                    (
                        source.to_string(),
                        engine.get_file_symbols(file).unwrap().clone(),
                    ),
                )
            })
            .collect(),
    );
    let actual = definitions_at(&engine, symbols, &sf, position, Some(&project), &output);
    let expected: Vec<_> = expected
        .iter()
        .map(|(file, start, end)| (file.to_string(), Span::new(*start, *end)))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn static_blocks() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class C { static {} static x; static {} static {} }"####,
        )],
        r####"/case.ts"####,
        30,
        &[],
        false,
        &[
            (r####"/case.ts"####, 10, 16),
            (r####"/case.ts"####, 30, 36),
            (r####"/case.ts"####, 40, 46),
        ],
    );
}

#[test]
fn inherited_constructor() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class A { constructor() {} } class B extends A {} class C extends B { constructor() { super(); } }"####,
        )],
        r####"/case.ts"####,
        86,
        &[],
        false,
        &[(r####"/case.ts"####, 10, 26), (r####"/case.ts"####, 35, 36)],
    );
}

#[test]
fn super_property_receiver() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class A { x() {} } class B extends A {} class C extends B { f() { super.x(); } }"####,
        )],
        r####"/case.ts"####,
        66,
        &[],
        false,
        &[(r####"/case.ts"####, 25, 26)],
    );
}

#[test]
fn unbound_super() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class C { constructor() { super(); } }"####,
        )],
        r####"/case.ts"####,
        26,
        &[],
        false,
        &[],
    );
}

#[test]
fn class_expression_private_name() {
    check(
        &[(
            r####"/case.ts"####,
            r####"const V = class Hidden {}; new Hidden();"####,
        )],
        r####"/case.ts"####,
        31,
        &[],
        false,
        &[],
    );
}

#[test]
fn class_expression_inner_name() {
    check(
        &[(
            r####"/case.ts"####,
            r####"const V = class Hidden { f() { return new Hidden(); } };"####,
        )],
        r####"/case.ts"####,
        42,
        &[],
        false,
        &[(r####"/case.ts"####, 16, 22)],
    );
}

#[test]
fn class_expression_shadowing() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class Hidden {} const V = class Hidden {}; new Hidden();"####,
        )],
        r####"/case.ts"####,
        47,
        &[],
        false,
        &[(r####"/case.ts"####, 6, 12)],
    );
}

#[test]
fn new_target_function() {
    check(
        &[(r####"/case.ts"####, r####"function f() { new.target; }"####)],
        r####"/case.ts"####,
        19,
        &[],
        false,
        &[(r####"/case.ts"####, 9, 10)],
    );
}

#[test]
fn new_target_constructor() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class C { constructor() { new.target; } }"####,
        )],
        r####"/case.ts"####,
        30,
        &[],
        false,
        &[(r####"/case.ts"####, 6, 7)],
    );
}

#[test]
fn new_target_nested_function() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class C { constructor() { function inner() { new.target; } } }"####,
        )],
        r####"/case.ts"####,
        49,
        &[],
        false,
        &[(r####"/case.ts"####, 35, 40)],
    );
}

#[test]
fn new_target_arrow() {
    check(
        &[(
            r####"/case.ts"####,
            r####"function f() { const arrow = () => new.target; }"####,
        )],
        r####"/case.ts"####,
        39,
        &[],
        false,
        &[(r####"/case.ts"####, 9, 10)],
    );
}

#[test]
fn import_meta() {
    check(
        &[(r####"/case.ts"####, r####"export {}; import.meta;"####)],
        r####"/case.ts"####,
        18,
        &[],
        false,
        &[],
    );
}

#[test]
fn yield_computed_member() {
    check(
        &[(
            r####"/case.ts"####,
            r####"function* gen() { class C { [yield 10]() {} } }"####,
        )],
        r####"/case.ts"####,
        29,
        &[],
        false,
        &[(r####"/case.ts"####, 28, 38)],
    );
}

#[test]
fn instanceof_hook() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class C { static [Symbol.hasInstance](value: unknown) { return true; } } declare let value: any; value instanceof C;"####,
        )],
        r####"/case.ts"####,
        103,
        &[],
        false,
        &[(r####"/case.ts"####, 17, 37)],
    );
}

#[test]
fn index_pattern_prefix() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface Dict { [k: `a${string}`]: number; [k: `${string}b`]: number } declare let dict: Dict; dict.ax;"####,
        )],
        r####"/case.ts"####,
        101,
        &[],
        false,
        &[(r####"/case.ts"####, 17, 43)],
    );
}

#[test]
fn index_pattern_suffix() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface Dict { [k: `a${string}`]: number; [k: `${string}b`]: number } declare let dict: Dict; dict.xb;"####,
        )],
        r####"/case.ts"####,
        101,
        &[],
        false,
        &[(r####"/case.ts"####, 44, 69)],
    );
}

#[test]
fn index_patterns_both() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface Dict { [k: `a${string}`]: number; [k: `${string}b`]: number } declare let dict: Dict; dict.ab;"####,
        )],
        r####"/case.ts"####,
        101,
        &[],
        false,
        &[(r####"/case.ts"####, 17, 43), (r####"/case.ts"####, 44, 69)],
    );
}

#[test]
fn index_union() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface A { [k: string]: number } interface B { [k: string]: boolean } declare let obj: A | B; obj.x;"####,
        )],
        r####"/case.ts"####,
        101,
        &[],
        false,
        &[(r####"/case.ts"####, 14, 33), (r####"/case.ts"####, 50, 70)],
    );
}

#[test]
fn index_named_member_precedence() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface Dict { named: number; [k: string]: number } declare let dict: Dict; dict.named;"####,
        )],
        r####"/case.ts"####,
        83,
        &[],
        false,
        &[(r####"/case.ts"####, 17, 22)],
    );
}

#[test]
fn index_pattern_miss() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface Dict { [k: `a${string}`]: number } declare let dict: Dict; dict.wrong;"####,
        )],
        r####"/case.ts"####,
        74,
        &[],
        false,
        &[],
    );
}

#[test]
fn generic_identity_origin() {
    check(
        &[(
            r####"/case.ts"####,
            r####"type C = { foo: string }; declare function identity<T extends C>(arg: T): T; const result = identity({ foo: "x" }); result.foo;"####,
        )],
        r####"/case.ts"####,
        123,
        &[],
        false,
        &[(r####"/case.ts"####, 103, 106)],
    );
}

#[test]
fn mapped_origin() {
    check(
        &[(
            r####"/case.ts"####,
            r####"const original = { a: 1, b: 2 }; const filtered: { [K in keyof typeof original as K extends 'b' ? never : K]: 0 } = { a: 0 }; filtered.a;"####,
        )],
        r####"/case.ts"####,
        135,
        &[],
        false,
        &[(r####"/case.ts"####, 19, 20)],
    );
}

#[test]
fn mapped_removed_property() {
    check(
        &[(
            r####"/case.ts"####,
            r####"const original = { a: 1, b: 2 }; const filtered: { [K in keyof typeof original as K extends 'b' ? never : K]: 0 } = { a: 0 }; filtered.b;"####,
        )],
        r####"/case.ts"####,
        135,
        &[],
        false,
        &[],
    );
}

#[test]
fn generic_mapped_origin() {
    check(
        &[(
            r####"/case.ts"####,
            r####"type Exclude<T, U> = T extends U ? never : T; const original = { get id() { return 1; }, name: "x" }; type Omit2<T, K> = { [P in keyof T as Exclude<P, K>]: T[P] }; declare function omit<O, M>(o: O, m: M): Omit2<O, keyof M>; const result = omit(original, { name: true }); result.id;"####,
        )],
        r####"/case.ts"####,
        278,
        &[],
        false,
        &[(r####"/case.ts"####, 69, 71)],
    );
}

#[test]
fn object_shadow_outer() {
    check(
        &[(
            r####"/case.ts"####,
            r####"const obj = { x: 1 }; function inner() { const obj = { x: 2 }; } obj.x;"####,
        )],
        r####"/case.ts"####,
        69,
        &[],
        false,
        &[(r####"/case.ts"####, 14, 15)],
    );
}

#[test]
fn object_shadow_inner() {
    check(
        &[(
            r####"/case.ts"####,
            r####"const obj = { x: 1 }; function inner() { const obj = { x: 2 }; obj.x; }"####,
        )],
        r####"/case.ts"####,
        67,
        &[],
        false,
        &[(r####"/case.ts"####, 55, 56)],
    );
}

#[test]
fn expando_class() {
    check(
        &[(
            r####"/case.js"####,
            r####"const core = {}; core.Widget = class {}; new core.Widget();"####,
        )],
        r####"/case.js"####,
        50,
        &[],
        false,
        &[(r####"/case.js"####, 22, 28)],
    );
}

#[test]
fn merged_interfaces() {
    check(
        &[
            (r####"/a.ts"####, r####"interface Shared { x: number }"####),
            (
                r####"/b.ts"####,
                r####"interface Shared { y: number } interface Shared { z: number } let value: Shared;"####,
            ),
        ],
        r####"/b.ts"####,
        73,
        &[],
        false,
        &[
            (r####"/a.ts"####, 10, 16),
            (r####"/b.ts"####, 10, 16),
            (r####"/b.ts"####, 41, 47),
        ],
    );
}

#[test]
fn merged_namespaces() {
    check(
        &[
            (
                r####"/a.ts"####,
                r####"namespace N { export interface I { x: number } }"####,
            ),
            (
                r####"/b.ts"####,
                r####"namespace N { export interface I { y: number } let v: I; }"####,
            ),
        ],
        r####"/b.ts"####,
        54,
        &[],
        false,
        &[(r####"/a.ts"####, 31, 32), (r####"/b.ts"####, 31, 32)],
    );
}

#[test]
fn unrelated_namespace() {
    check(
        &[
            (
                r####"/a.ts"####,
                r####"namespace Other { export interface I { x: number } }"####,
            ),
            (
                r####"/b.ts"####,
                r####"namespace N { export interface I { y: number } let v: I; }"####,
            ),
        ],
        r####"/b.ts"####,
        54,
        &[],
        false,
        &[(r####"/b.ts"####, 31, 32)],
    );
}

#[test]
fn jsdoc_enum_lexical() {
    check(
        &[
            (r####"/a.ts"####, r####"interface Target {}"####),
            (
                r####"/b.ts"####,
                r####"enum E {
    /** {@link Target} */
    Target
}"####,
            ),
        ],
        r####"/b.ts"####,
        24,
        &[],
        false,
        &[(r####"/a.ts"####, 10, 16)],
    );
}

#[test]
fn reference_path() {
    check(
        &[
            (r####"/a.ts"####, r####"/// <reference path="b.ts" />"####),
            (r####"/b.ts"####, r####""####),
        ],
        r####"/a.ts"####,
        21,
        &[],
        false,
        &[(r####"/b.ts"####, 0, 0)],
    );
}

#[test]
fn missing_reference_path() {
    check(
        &[(
            r####"/a.ts"####,
            r####"/// <reference path="missing.ts" />"####,
        )],
        r####"/a.ts"####,
        21,
        &[],
        false,
        &[],
    );
}

#[test]
fn reference_types() {
    check(
        &[
            (
                r####"/src/a.ts"####,
                r####"/// <reference types="custom" />"####,
            ),
            (
                r####"/types/custom/index.d.ts"####,
                r####"declare const value: number;"####,
            ),
        ],
        r####"/src/a.ts"####,
        22,
        &[r####"/types"####],
        false,
        &[(r####"/types/custom/index.d.ts"####, 0, 0)],
    );
}

#[test]
fn ambient_wildcard() {
    check(
        &[
            (
                r####"/types.d.ts"####,
                r####"declare module "*.css" { const css: string; export = css; }"####,
            ),
            (r####"/a.ts"####, r####"import css from "./a.css";"####),
            (r####"/a.css"####, r####"body {}"####),
        ],
        r####"/a.ts"####,
        21,
        &[],
        false,
        &[(r####"/a.css"####, 0, 0), (r####"/types.d.ts"####, 15, 22)],
    );
}

#[test]
fn require_module() {
    check(
        &[
            (r####"/a.js"####, r####"exports.Widget = class {};"####),
            (
                r####"/b.js"####,
                r####"const widgets = require("./a"); widgets;"####,
            ),
        ],
        r####"/b.js"####,
        32,
        &[],
        false,
        &[(r####"/a.js"####, 0, 26)],
    );
}

#[test]
fn require_reexport() {
    check(
        &[
            (
                r####"/a.js"####,
                r####"class Widget {} module.exports = { Widget };"####,
            ),
            (
                r####"/b.js"####,
                r####"const { Widget } = require("./a"); module.exports = { Widget };"####,
            ),
            (
                r####"/c.js"####,
                r####"const { Widget } = require("./b"); new Widget();"####,
            ),
        ],
        r####"/c.js"####,
        39,
        &[],
        false,
        &[(r####"/a.js"####, 6, 12)],
    );
}

#[test]
fn import_equals_module() {
    check(
        &[
            (r####"/a.ts"####, r####"export class C {}"####),
            (r####"/b.ts"####, r####"import alias = require("./a");"####),
        ],
        r####"/b.ts"####,
        7,
        &[],
        false,
        &[(r####"/a.ts"####, 0, 17)],
    );
}

#[test]
fn import_namespace_alias() {
    check(
        &[
            (r####"/a.ts"####, r####"export class C {}"####),
            (
                r####"/b.ts"####,
                r####"import * as alias from "./a"; alias;"####,
            ),
        ],
        r####"/b.ts"####,
        30,
        &[],
        false,
        &[(r####"/b.ts"####, 12, 17)],
    );
}

#[test]
fn import_shadowing() {
    check(
        &[
            (r####"/a.ts"####, r####"export const value = 1;"####),
            (
                r####"/b.ts"####,
                r####"import { value } from "./a"; function f(value: number) { value; }"####,
            ),
        ],
        r####"/b.ts"####,
        57,
        &[],
        false,
        &[(r####"/b.ts"####, 40, 45)],
    );
}

#[test]
fn require_shadowing() {
    check(
        &[
            (
                r####"/a.js"####,
                r####"class Widget {} module.exports = { Widget };"####,
            ),
            (
                r####"/b.js"####,
                r####"const { Widget } = require("./a"); function f(Widget) { Widget; }"####,
            ),
        ],
        r####"/b.js"####,
        56,
        &[],
        false,
        &[(r####"/b.js"####, 46, 52)],
    );
}

#[test]
fn quoted_export_alias() {
    check(
        &[(
            r####"/a.ts"####,
            r####"type Value = string; export { Value as "<>" };"####,
        )],
        r####"/a.ts"####,
        40,
        &[],
        false,
        &[(r####"/a.ts"####, 5, 10)],
    );
}

#[test]
fn quoted_import_alias() {
    check(
        &[
            (
                r####"/a.ts"####,
                r####"type Value = string; export { Value as "<>" };"####,
            ),
            (
                r####"/b.ts"####,
                r####"import { "<>" as Local } from "./a";"####,
            ),
        ],
        r####"/b.ts"####,
        10,
        &[],
        false,
        &[(r####"/a.ts"####, 5, 10)],
    );
}

#[test]
fn quoted_reexport_alias() {
    check(
        &[
            (
                r####"/a.ts"####,
                r####"const value = 1; export { value as "<>" };"####,
            ),
            (
                r####"/b.ts"####,
                r####"export { "<>" as "other" } from "./a";"####,
            ),
        ],
        r####"/b.ts"####,
        10,
        &[],
        false,
        &[(r####"/a.ts"####, 6, 11)],
    );
}

#[test]
fn function_overload_string() {
    check(
        &[(
            r####"/case.ts"####,
            r####"function run(x: number): void; function run(x: string): void; function run(x: any) {} run("test");"####,
        )],
        r####"/case.ts"####,
        86,
        &[],
        false,
        &[(r####"/case.ts"####, 40, 43)],
    );
}

#[test]
fn constructor_overload_string() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class Box { constructor(x: number); constructor(x: string); constructor(x: any) {} } new Box("test");"####,
        )],
        r####"/case.ts"####,
        89,
        &[],
        false,
        &[(r####"/case.ts"####, 6, 9), (r####"/case.ts"####, 36, 59)],
    );
}

#[test]
fn inherited_constructor_overload() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class Base { constructor(x: number); constructor(x: string); constructor(x: any) {} } class Child extends Base {} new Child("test");"####,
        )],
        r####"/case.ts"####,
        118,
        &[],
        false,
        &[(r####"/case.ts"####, 92, 97), (r####"/case.ts"####, 37, 60)],
    );
}

#[test]
fn instance_method_overload() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class C { static method(): void; static method(x: string): void; static method(x?: any) {} method(): void; method(x: string): void; method(x?: any) {} } const c = new C(); c.method("test");"####,
        )],
        r####"/case.ts"####,
        174,
        &[],
        false,
        &[(r####"/case.ts"####, 107, 113)],
    );
}

#[test]
fn call_alias_signature() {
    check(
        &[(
            r####"/case.ts"####,
            r####"function original() {} const first = original; const second = first; second();"####,
        )],
        r####"/case.ts"####,
        69,
        &[],
        false,
        &[(r####"/case.ts"####, 53, 59), (r####"/case.ts"####, 9, 17)],
    );
}

#[test]
fn private_accessor_pair() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class C { get #value() { return 0; } set #value(x: number) {} f() { this.#value; } }"####,
        )],
        r####"/case.ts"####,
        73,
        &[],
        false,
        &[(r####"/case.ts"####, 14, 20), (r####"/case.ts"####, 41, 47)],
    );
}

#[test]
fn contextual_generic_constraint() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface Shape { value: string } declare function take<T extends Shape>(x: T): T; take({ value: "hello" });"####,
        )],
        r####"/case.ts"####,
        90,
        &[],
        false,
        &[(r####"/case.ts"####, 18, 23)],
    );
}

#[test]
fn contextual_union_both() {
    check(
        &[(
            r####"/case.ts"####,
            r####"type Shape = { kind: "a"; value: number } | { kind: "b"; value: string }; const item: Shape = { kind: "wrong", value: 1 };"####,
        )],
        r####"/case.ts"####,
        111,
        &[],
        false,
        &[(r####"/case.ts"####, 26, 31), (r####"/case.ts"####, 57, 62)],
    );
}

#[test]
fn nested_union_property() {
    check(
        &[(
            r####"/case.ts"####,
            r####"interface A { nested: { value: number } } interface B { nested: { value: string } } declare const both: A | B; both.nested.value;"####,
        )],
        r####"/case.ts"####,
        123,
        &[],
        false,
        &[(r####"/case.ts"####, 24, 29), (r####"/case.ts"####, 66, 71)],
    );
}

#[test]
fn typed_binding_property() {
    check(
        &[(
            r####"/case.ts"####,
            r####"function f({ value }: { value: string }) {}"####,
        )],
        r####"/case.ts"####,
        13,
        &[],
        false,
        &[(r####"/case.ts"####, 24, 29)],
    );
}

#[test]
fn inferred_callback_binding() {
    check(
        &[(
            r####"/case.ts"####,
            r####"declare function each<T>(callback: (item: T) => void): void; interface Item { value: number } each<Item>(({ value }) => {});"####,
        )],
        r####"/case.ts"####,
        108,
        &[],
        false,
        &[(r####"/case.ts"####, 78, 83)],
    );
}

#[test]
fn dynamic_import_binding() {
    check(
        &[
            (r####"/a.ts"####, r####"export const value = 1;"####),
            (
                r####"/b.ts"####,
                r####"import("./a").then(({ value }) => {});"####,
            ),
        ],
        r####"/b.ts"####,
        22,
        &[],
        false,
        &[(r####"/a.ts"####, 13, 18)],
    );
}

#[test]
fn global_variable_merge() {
    check(
        &[
            (r####"/a.ts"####, r####"var shared: number;"####),
            (r####"/b.ts"####, r####"var shared: number;"####),
            (r####"/c.ts"####, r####"shared++;"####),
        ],
        r####"/c.ts"####,
        0,
        &[],
        false,
        &[(r####"/a.ts"####, 4, 10), (r####"/b.ts"####, 4, 10)],
    );
}

#[test]
fn global_variable_local_shadow() {
    check(
        &[
            (r####"/a.ts"####, r####"var shared: number;"####),
            (
                r####"/b.ts"####,
                r####"var shared: number; function f() { const shared = 0; shared; }"####,
            ),
        ],
        r####"/b.ts"####,
        53,
        &[],
        false,
        &[(r####"/b.ts"####, 41, 47)],
    );
}

#[test]
fn expando_binding_shadow() {
    check(
        &[(
            r####"/case.js"####,
            r####"const obj = {}; obj.value = 1; function f() { const obj = {}; obj.value = 2; } obj.value;"####,
        )],
        r####"/case.js"####,
        83,
        &[],
        false,
        &[(r####"/case.js"####, 20, 25)],
    );
}

#[test]
fn anonymous_parent_override() {
    check(
        &[(
            r####"/case.ts"####,
            r####"class Child extends (class { value() {} }) { override value() {} }"####,
        )],
        r####"/case.ts"####,
        45,
        &[],
        false,
        &[(r####"/case.ts"####, 29, 34)],
    );
}

#[test]
fn decorator_symbol_overload() {
    check(
        &[(
            r####"/case.ts"####,
            r####"function decorate(target: any, key: string): void; function decorate(target: any, key: symbol): void; function decorate(target: any, key: any) {} declare const key: symbol; class C { @decorate [key]() {} }"####,
        )],
        r####"/case.ts"####,
        184,
        &[],
        true,
        &[(r####"/case.ts"####, 60, 68)],
    );
}

#[test]
fn jsx_function_overload() {
    check(
        &[(
            r####"/case.tsx"####,
            r####"namespace JSX { export interface Element {} } interface First { first: string } interface Second { second: string } declare function View(props: First): JSX.Element; declare function View(props: Second): JSX.Element; < View second="yes" />;"####,
        )],
        r####"/case.tsx"####,
        219,
        &[],
        false,
        &[(r####"/case.tsx"####, 183, 187)],
    );
}

#[test]
fn standard_decorator_overload() {
    check(
        &[(
            r####"/case.ts"####,
            r####"function decorate(target: any, key: string): void; function decorate(target: any, key: symbol): void; function decorate(target: any, key: any) {} declare const key: symbol; class C { @decorate [key]() {} }"####,
        )],
        r####"/case.ts"####,
        184,
        &[],
        false,
        &[(r####"/case.ts"####, 9, 17)],
    );
}

fn protocol_definitions(source: &str, cursor: &str) -> serde_json::Value {
    use serde_json::json;
    let (line, character) =
        tsc_rs_server::offset_to_position(source, source.find(cursor).unwrap() as u32);
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///case.ts","languageId":"typescript","version":1,"text":source}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///case.ts"},"position":{"line":line,"character":character}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit","params":null}),
    ];
    let mut input = vec![];
    for message in messages {
        tsc_rs_server::write_message(&mut input, &message).unwrap();
    }
    let mut output = vec![];
    tsc_rs_server::run_server(&mut std::io::Cursor::new(input), &mut output).unwrap();
    let mut reader = std::io::Cursor::new(output);
    while let Some(message) = tsc_rs_server::read_message(&mut reader).unwrap() {
        if message["id"] == 2 {
            return message["result"].clone();
        }
    }
    panic!("server did not respond to definition request");
}

#[test]
fn protocol_keyword_returns_all_static_blocks() {
    let source = "class C { static {} static {} }";
    let result = protocol_definitions(source, "static");
    assert_eq!(result.as_array().unwrap().len(), 2);
    for (target, pos) in result.as_array().unwrap().iter().zip([10, 20]) {
        assert_eq!(target["uri"], "file:///case.ts");
        assert_eq!(target["range"]["start"]["character"], pos);
        assert_eq!(target["range"]["end"]["character"], pos + 6);
    }
}

#[test]
fn protocol_jsdoc_link_is_not_filtered_out() {
    let source = "interface Target {}\nenum E {\n/** {@link Target} */\nMember\n}";
    let result = protocol_definitions(source, "Target}");
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["range"]["start"]["line"], 0);
    assert_eq!(result[0]["range"]["start"]["character"], 10);
    assert_eq!(result[0]["range"]["end"]["character"], 16);
}
