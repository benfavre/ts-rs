use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::{load_stdlib_sources, TypeChecker};

fn check(source: &str, options: CompilerOptions) -> Option<Vec<Diagnostic>> {
    if load_stdlib_sources(&CompilerOptions::default()).is_empty() {
        return None;
    }
    let file = tsc_rs_parser::parse("missing_lib_global.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    Some(
        TypeChecker::new()
            .check_with_options(&file, &symbols, &options)
            .diagnostics,
    )
}

fn check_with_newer_lib_donor(source: &str, options: CompilerOptions) -> Option<Vec<Diagnostic>> {
    let libraries = load_stdlib_sources(&CompilerOptions {
        target: Some(ScriptTarget::ES2020),
        ..CompilerOptions::default()
    });
    if libraries.is_empty() {
        return None;
    }
    let parsed: Vec<_> = libraries
        .iter()
        .map(|library| tsc_rs_parser::parse(&library.file_name, &library.source))
        .collect();
    let declarations: Vec<_> = parsed.iter().collect();
    let file = tsc_rs_parser::parse("missing_lib_global.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.inject_external_types(&declarations);
    checker.take_diagnostics();
    Some(
        checker
            .check_with_options(&file, &symbols, &options)
            .diagnostics,
    )
}

fn check_with_user_donor(
    source: &str,
    donor_name: &str,
    donor_source: &str,
    options: CompilerOptions,
) -> Option<Vec<Diagnostic>> {
    if load_stdlib_sources(&CompilerOptions::default()).is_empty() {
        return None;
    }
    let donor = tsc_rs_parser::parse(donor_name, donor_source);
    let file = tsc_rs_parser::parse("missing_lib_global.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.inject_external_types(&[&donor]);
    checker.take_diagnostics();
    Some(
        checker
            .check_with_options(&file, &symbols, &options)
            .diagnostics,
    )
}

fn options(target: ScriptTarget, lib: &str) -> CompilerOptions {
    CompilerOptions {
        target: Some(target),
        lib: vec![lib.to_string()],
        strict: Some(false),
        ..CompilerOptions::default()
    }
}

fn ts2583<'a>(source: &'a str, diagnostics: &'a [Diagnostic]) -> Vec<(&'a str, &'a str)> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2583)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS2583 should have a name span");
            (
                &source[span.start as usize..span.end as usize],
                diagnostic.message.as_str(),
            )
        })
        .collect()
}

#[test]
fn reports_declaration_derived_missing_globals_in_value_and_type_positions() {
    let source = r#"
const map = new Map<string, number>();
Reflect.get({}, "key");
let atomics: typeof Atomics;
let shared: SharedArrayBuffer;
let asyncValues: AsyncIterable<number>;
let bigint: BigInt64Array;
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    let actual = ts2583(source, &diagnostics);
    assert_eq!(
        actual.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        [
            "Map",
            "Reflect",
            "Atomics",
            "SharedArrayBuffer",
            "AsyncIterable",
            "BigInt64Array",
        ],
        "diagnostics: {diagnostics:#?}"
    );
    for (name, message) in actual {
        let edition = match name {
            "Map" | "Reflect" => "es2015",
            "Atomics" | "SharedArrayBuffer" => "es2017",
            "AsyncIterable" => "es2018",
            "BigInt64Array" => "es2020",
            _ => unreachable!(),
        };
        assert!(
            message.contains(&format!(
                "Try changing the 'lib' compiler option to '{edition}' or later."
            )),
            "{message}"
        );
    }
}

#[test]
fn selected_libraries_make_only_their_globals_available() {
    let source = r#"
new Map();
Atomics.load(new Int32Array(), 0);
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es2015")) else {
        return;
    };
    assert_eq!(
        ts2583(source, &diagnostics)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["Atomics"],
        "diagnostics: {diagnostics:#?}"
    );
}

#[test]
fn user_declarations_and_type_parameters_shadow_standard_names() {
    let source = r#"
interface Map<K, V> {
    get(key: K): V;
}
declare const Map: new <K, V>() => Map<K, V>;
declare const Reflect: { get(value: object, key: string): unknown };
type AsyncIterable<T> = { next(): T };
function identity<BigInt64Array>(value: BigInt64Array): BigInt64Array {
    return value;
}
class Box<SharedArrayBuffer> {
    value!: SharedArrayBuffer;
    method<Atomics>(value: Atomics): Atomics {
        return value;
    }
}
new Map<string, number>();
Reflect.get({}, "key");
let values: AsyncIterable<number>;
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    assert!(
        ts2583(source, &diagnostics).is_empty(),
        "local declarations must not receive TS2583: {diagnostics:#?}"
    );
}

#[test]
fn type_only_declarations_and_non_ts2583_globals_keep_their_diagnostic_class() {
    let source = r#"
interface Map<K, V> {}
new Map<string, number>();
new Proxy({}, {});
const Reflect = { get() {} };
Reflect.get();
function localShadow(Map: unknown) {
    new Map();
}
function typeShadow<WeakMap>() {
    new WeakMap();
}
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    assert!(
        ts2583(source, &diagnostics).is_empty(),
        "a local type declaration and a generic missing-name global must not receive TS2583: {diagnostics:#?}"
    );
}

#[test]
fn checks_interface_alias_and_signature_types_without_leaking_generic_names() {
    let source = r#"
interface Container<Map> {
    present(value: Map): Map;
    missing: SharedArrayBuffer;
    method<Reflect>(value: Reflect): Reflect;
    new <Atomics>(value: Atomics): Atomics;
}
type Generic<BigInt64Array> = {
    (value: BigInt64Array): BigInt64Array;
    method<AsyncIterable>(value: AsyncIterable): AsyncIterable;
    missing: AsyncIterator;
};
type MissingMap = Map<string, number>;
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    assert_eq!(
        ts2583(source, &diagnostics)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["SharedArrayBuffer", "AsyncIterator", "Map"],
        "only unbound newer-library names should receive TS2583: {diagnostics:#?}"
    );
}

#[test]
fn newer_donor_declarations_do_not_override_the_selected_library() {
    let source = "new SharedArrayBuffer(1024);";
    let Some(diagnostics) = check_with_newer_lib_donor(source, options(ScriptTarget::ES5, "es5"))
    else {
        return;
    };
    assert_eq!(
        ts2583(source, &diagnostics)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["SharedArrayBuffer"],
        "project-wide donor declarations must not make an omitted library global active: {diagnostics:#?}"
    );
}

#[test]
fn no_lib_user_globals_remain_authoritative() {
    let source = r#"
interface Map<K, V> {
    get(key: K): V;
}
declare const Map: new <K, V>() => Map<K, V>;
new Map<string, number>();
"#;
    let Some(diagnostics) = check(
        source,
        CompilerOptions {
            no_lib: Some(true),
            strict: Some(false),
            ..CompilerOptions::default()
        },
    ) else {
        return;
    };
    assert!(
        ts2583(source, &diagnostics).is_empty(),
        "a user-provided noLib global must suppress TS2583: {diagnostics:#?}"
    );
}

#[test]
fn arbitrary_missing_names_keep_the_general_diagnostic_without_cascades() {
    let source = r#"
new Map().clear();
MissingGlobal.call();
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    assert_eq!(
        ts2583(source, &diagnostics)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["Map"],
        "diagnostics: {diagnostics:#?}"
    );
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == 2304),
        "the unrelated missing global should remain TS2304: {diagnostics:#?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code, 2583 | 2339 | 2351))
            .count(),
        1,
        "the authoritative missing-global diagnostic must not cascade: {diagnostics:#?}"
    );
}

#[test]
fn respects_infer_mapped_expression_and_nested_lexical_scopes() {
    let source = r#"
type InferShadowed<T> = T extends infer Map ? Map : never;
type InferMissing<T> = T extends infer U ? Set : never;
type MappedShadow<T> = { [Reflect in keyof T]: Reflect };
type MappedMissing<T> = { [U in WeakMap]: U };

const shadowArrow = <Promise>(value: Promise): Promise => value;
const missingArrow = <U>(value: Map): Map => value;
const shadowFunction = function <BigInt>(value: BigInt): BigInt { return value; };
const missingFunction = function <U>(value: AsyncIterator): AsyncIterator { return value; };
const shadowClass = class<SharedArrayBuffer> { value!: SharedArrayBuffer };
const missingClass = class<U> { value!: Atomics };
class MethodScopes {
    shadow<WeakSet>() { new WeakSet(); }
    missing<U>() { new Map(); }
}
function parameterValueShadow(
    Map: unknown,
    query: typeof Map,
    typePosition: Map,
) {}
function missingParameter(value: Set) {}
const arrowParameterShadow = (
    WeakMap: unknown,
    query: typeof WeakMap,
    typePosition: WeakMap,
) => query;

function localType() {
    type AsyncIterable = string;
    let local: AsyncIterable;
    new AsyncIterable();
}
function separateScope() {
    let missing: AsyncIterableIterator<unknown>;
}

const object = {
    shadow<AsyncGenerator>(value: AsyncGenerator): AsyncGenerator { return value; },
    missing<U>(value: AsyncGeneratorFunction): AsyncGeneratorFunction { return value; },
};

namespace LocalNamespace {
    export type BigInt64Array = string;
    export let local: BigInt64Array;
}
namespace SeparateNamespace {
    export let missing: BigUint64Array;
}
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    assert_eq!(
        ts2583(source, &diagnostics)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        [
            "Set",
            "WeakMap",
            "Map",
            "Map",
            "AsyncIterator",
            "AsyncIterator",
            "Atomics",
            "Map",
            "Set",
            "AsyncIterableIterator",
            "AsyncGeneratorFunction",
            "AsyncGeneratorFunction",
            "BigUint64Array",
        ],
        "only names outside their precise lexical type scope should receive TS2583: {diagnostics:#?}"
    );
}

#[test]
fn distinguishes_user_script_globals_from_module_and_stdlib_donors() {
    let source = "new Map<string, number>(); let value: Map<string, number>;";
    let Some(script_diagnostics) = check_with_user_donor(
        source,
        "globals.d.ts",
        r#"
interface Map<K, V> {}
declare const Map: new <K, V>() => Map<K, V>;
"#,
        options(ScriptTarget::ES5, "es5"),
    ) else {
        return;
    };
    assert!(
        ts2583(source, &script_diagnostics).is_empty(),
        "a user script's cross-file globals must remain authoritative: {script_diagnostics:#?}"
    );

    let Some(module_diagnostics) = check_with_user_donor(
        source,
        "scoped.d.ts",
        r#"
export interface Map<K, V> {}
export declare const Map: new <K, V>() => Map<K, V>;
"#,
        options(ScriptTarget::ES5, "es5"),
    ) else {
        return;
    };
    assert_eq!(
        ts2583(source, &module_diagnostics)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["Map", "Map"],
        "module-local donors must not leak into the global namespace: {module_diagnostics:#?}"
    );

    let Some(augmentation_diagnostics) = check_with_user_donor(
        source,
        "augmentation.d.ts",
        r#"
export {};
declare global {
    interface Map<K, V> {}
    const Map: new <K, V>() => Map<K, V>;
}
"#,
        options(ScriptTarget::ES5, "es5"),
    ) else {
        return;
    };
    assert!(
        ts2583(source, &augmentation_diagnostics).is_empty(),
        "declare-global donors must suppress missing-lib diagnostics: {augmentation_diagnostics:#?}"
    );
}

#[test]
fn imported_bindings_shadow_missing_lib_names_in_their_own_file() {
    let source = r#"
import type { Map } from "./types";
import { Set } from "./values";
let map: Map<string, number>;
new Map<string, number>();
let set: Set<unknown>;
new Set();
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    assert!(
        ts2583(source, &diagnostics).is_empty(),
        "type-only and value imports should retain their own diagnostic classes: {diagnostics:#?}"
    );
}

#[test]
fn reports_missing_globals_in_shorthand_exports_and_qualified_type_queries() {
    for (source, expected) in [
        ("const value = { Map };", "Map"),
        ("({ Set } = value);", "Set"),
        ("export { Map as Renamed };", "Map"),
        ("export type { Map };", "Map"),
        ("export { type WeakMap as Renamed };", "WeakMap"),
        ("export = Reflect;", "Reflect"),
        ("type Query = typeof Atomics.load;", "Atomics"),
        (
            "type ElementQuery = typeof BigInt64Array[\"prototype\"];",
            "BigInt64Array",
        ),
    ] {
        let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
            return;
        };
        assert_eq!(
            ts2583(source, &diagnostics)
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            [expected],
            "{source}: {diagnostics:#?}"
        );
    }
}

#[test]
fn switch_and_infer_scopes_follow_their_lexical_boundaries() {
    for source in [
        r#"
switch (1) {
    case 1: type Map = string; break;
    case 2: let inside: Map; break;
}
let outside: Map;
"#,
        r#"
switch (true) {
    case true: type Map = string; break;
    case false: let inside: Map; break;
}
let outside: Map;
"#,
    ] {
        let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
            return;
        };
        assert_eq!(
            ts2583(source, &diagnostics)
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            ["Map"],
            "{diagnostics:#?}"
        );
    }

    let source = r#"
type Circular<T> = T extends infer Set extends Set ? Set : never;
type Missing<T> = T extends infer U ? U : Set;
"#;
    let Some(diagnostics) = check(source, options(ScriptTarget::ES5, "es5")) else {
        return;
    };
    assert_eq!(
        ts2583(source, &diagnostics)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["Set"],
        "an infer name is in scope in its own constraint and true branch only: {diagnostics:#?}"
    );
}

#[test]
fn user_lib_named_declaration_files_are_not_typescript_standard_libraries() {
    let source = "new Map(); let value: Map<unknown, unknown>;";
    let Some(diagnostics) = check_with_user_donor(
        source,
        "lib.custom.d.ts",
        "interface Map<K, V> {}\ndeclare const Map: new () => Map<unknown, unknown>;",
        options(ScriptTarget::ES5, "es5"),
    ) else {
        return;
    };
    assert!(
        ts2583(source, &diagnostics).is_empty(),
        "a user file's basename must not make its globals look like omitted stdlib declarations: {diagnostics:#?}"
    );
}

#[test]
fn explicit_library_selection_takes_precedence_over_target_defaults() {
    let source = "new Map(); let value: Map<unknown, unknown>;";
    for (target, library, expected_count) in [
        (ScriptTarget::ESNext, "es5", 2),
        (ScriptTarget::ES5, "es2015.core", 2),
        (ScriptTarget::ES5, "es2015.collection", 0),
    ] {
        let Some(diagnostics) = check(source, options(target, library)) else {
            return;
        };
        assert_eq!(
            ts2583(source, &diagnostics).len(),
            expected_count,
            "{target:?}/{library}: {diagnostics:#?}"
        );
    }

    let Some(default_target_diagnostics) = check(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(false),
            ..CompilerOptions::default()
        },
    ) else {
        return;
    };
    assert!(
        ts2583(source, &default_target_diagnostics).is_empty(),
        "an omitted lib should use the target's default library bundle: {default_target_diagnostics:#?}"
    );
}
