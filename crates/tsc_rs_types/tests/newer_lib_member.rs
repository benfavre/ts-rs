use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::{load_stdlib_sources, TypeChecker};

fn check(source: &str, options: CompilerOptions) -> Option<Vec<Diagnostic>> {
    check_file("newer_lib_member.ts", source, options)
}

fn check_file(file_name: &str, source: &str, options: CompilerOptions) -> Option<Vec<Diagnostic>> {
    // The production checker deliberately stays usable without a Node/
    // TypeScript installation. These declaration-derived tests require the
    // same lib directory used by project checking and the corpus harness.
    if load_stdlib_sources(&CompilerOptions::default()).is_empty() {
        return None;
    }
    let file = tsc_rs_parser::parse(file_name, source);
    let symbols = tsc_rs_symbols::bind(&file);
    Some(
        TypeChecker::new()
            .check_with_options(&file, &symbols, &options)
            .diagnostics,
    )
}

fn es2015_options() -> CompilerOptions {
    CompilerOptions {
        target: Some(ScriptTarget::ES2015),
        lib: vec!["es2015".to_string()],
        strict: Some(false),
        ..CompilerOptions::default()
    }
}

fn es5_options() -> CompilerOptions {
    CompilerOptions {
        target: Some(ScriptTarget::ES5),
        lib: vec!["es5".to_string()],
        strict: Some(false),
        ..CompilerOptions::default()
    }
}

fn ts2550<'a>(source: &'a str, diagnostics: &'a [Diagnostic]) -> Vec<(&'a str, &'a str)> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2550)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS2550 should have a property span");
            (
                &source[span.start as usize..span.end as usize],
                diagnostic.message.as_str(),
            )
        })
        .collect()
}

#[test]
fn reports_declaration_derived_edition_and_exact_property_span() {
    let source = r#"
const array: number[] = [];
array.includes(1);
array.at(0);
"".replaceAll("a", "b");
Object.values({});
new Intl.DateTimeFormat("en").formatToParts();
"#;
    let Some(diagnostics) = check(source, es5_options()) else {
        return;
    };
    let actual = ts2550(source, &diagnostics);
    assert_eq!(
        actual.iter().map(|(span, _)| *span).collect::<Vec<_>>(),
        ["includes", "at", "replaceAll", "values", "formatToParts"],
        "diagnostics: {diagnostics:?}"
    );
    for (property, message) in actual {
        let edition = match property {
            "includes" => "es2016",
            "values" | "formatToParts" => "es2017",
            "replaceAll" => "es2021",
            "at" => "es2022",
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
fn explicit_lib_union_controls_member_availability_independently_of_target() {
    let source = r#"
declare const array: number[];
array.at(0);
"x".at(0);
"#;
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        lib: vec!["es5".to_string(), "es2022.array".to_string()],
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let Some(diagnostics) = check(source, options) else {
        return;
    };
    assert_eq!(
        ts2550(source, &diagnostics)
            .iter()
            .map(|(span, _)| *span)
            .collect::<Vec<_>>(),
        ["at"],
        "the array feature lib should not make String.at available: {diagnostics:?}"
    );
}

#[test]
fn local_shadows_and_custom_lookalikes_are_not_standard_library_receivers() {
    let source = r#"
interface Mine {}
declare const mine: Mine;
mine.at(0);

function local(
    Array: { from(value: unknown): unknown },
    Intl: { PluralRules: new () => { select(value: number): string } },
    Symbol: { matchAll: symbol },
) {
    Array.from([]);
    new Intl.PluralRules().select(0);
    Symbol.matchAll;
}
"#;
    let Some(diagnostics) = check(source, es5_options()) else {
        return;
    };
    assert!(
        ts2550(source, &diagnostics).is_empty(),
        "local values and unrelated types must not receive TS2550: {diagnostics:?}"
    );
}

#[test]
fn global_aliases_keep_standard_constructor_identity() {
    let source = r#"
const ArrayAlias = Array;
const ObjectAlias = Object;
ArrayAlias.from([]);
ObjectAlias.values({});
"#;
    let Some(diagnostics) = check(source, es5_options()) else {
        return;
    };
    let actual = ts2550(source, &diagnostics);
    assert_eq!(
        actual.iter().map(|(span, _)| *span).collect::<Vec<_>>(),
        ["from", "values"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(actual[0].1.contains("type 'ArrayConstructor'"));
    assert!(actual[1].1.contains("type 'ObjectConstructor'"));
}

#[test]
fn interface_augmentations_suppress_future_lib_diagnostics() {
    let source = r#"
interface Array<T> {
    at(index: number): T | undefined;
}
interface ArrayConstructor {
    from(value: unknown): unknown[];
}
[1].at(0);
Array.from([]);
"#;
    let Some(diagnostics) = check(source, es2015_options()) else {
        return;
    };
    assert!(
        ts2550(source, &diagnostics).is_empty(),
        "declared augmentations make the members available: {diagnostics:?}"
    );
}

#[test]
fn optional_member_access_is_checked_but_element_access_is_not_ts2550() {
    let source = r#"
declare const array: number[] | undefined;
array?.at(0);
array!["at"](0);
"#;
    let Some(diagnostics) = check(source, es2015_options()) else {
        return;
    };
    assert_eq!(
        ts2550(source, &diagnostics)
            .iter()
            .map(|(span, _)| *span)
            .collect::<Vec<_>>(),
        ["at"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn no_property_diagnostic_is_cascaded_below_an_unavailable_global() {
    let source = r#"
new Map().clear();
Atomics.add(new Int32Array(), 0, 1);
new Intl.PluralRules().select(1);
"#;
    let Some(diagnostics) = check(source, es2015_options()) else {
        return;
    };
    assert_eq!(
        ts2550(source, &diagnostics)
            .iter()
            .map(|(span, _)| *span)
            .collect::<Vec<_>>(),
        ["PluralRules"],
        "only the member of the available Intl namespace is TS2550: {diagnostics:?}"
    );
}

#[test]
fn unavailable_promise_value_does_not_cascade_to_its_result_members() {
    let source = r#"
Promise.resolve(1).finally(() => {});
const Future = Promise;
Future.resolve(1).finally(() => {});
"#;
    let Some(diagnostics) = check(source, es5_options()) else {
        return;
    };
    assert!(
        ts2550(source, &diagnostics).is_empty(),
        "the unavailable Promise value is the authoritative failure: {diagnostics:?}"
    );
}

#[test]
fn no_lib_user_global_can_still_receive_the_future_member_suggestion() {
    let source = r#"
interface Array<T> {
    length: number;
}
declare const array: Array<number>;
array.at(0);
"#;
    let options = CompilerOptions {
        no_lib: Some(true),
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let Some(diagnostics) = check(source, options) else {
        return;
    };
    let actual = ts2550(source, &diagnostics);
    assert_eq!(
        actual.iter().map(|(span, _)| *span).collect::<Vec<_>>(),
        ["at"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(actual[0].1.contains("'es2022'"));
}

#[test]
fn standard_receiver_families_report_on_reads_and_calls() {
    let source = r#"
const values = Object.values;
Object.values({});
Number.isNaN(1);
Math.trunc(1);
declare const promise: Promise<number>;
promise.finally(() => {});
"x".padStart(2);
[1].flat();
"#;
    let Some(diagnostics) = check(source, es5_options()) else {
        return;
    };
    let actual = ts2550(source, &diagnostics);
    assert_eq!(
        actual.iter().map(|(span, _)| *span).collect::<Vec<_>>(),
        ["values", "values", "isNaN", "trunc", "finally", "padStart", "flat"],
        "diagnostics: {diagnostics:?}"
    );
    for (property, message) in actual {
        let edition = match property {
            "isNaN" | "trunc" => "es2015",
            "values" | "padStart" => "es2017",
            "finally" => "es2018",
            "flat" => "es2019",
            _ => unreachable!(),
        };
        assert!(message.contains(&format!("'{edition}'")), "{message}");
    }
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 2339),
        "TS2550 should replace the generic missing-property diagnostic: {diagnostics:?}"
    );
}

#[test]
fn constrained_generics_use_their_apparent_standard_type() {
    let source = r#"
function text<T extends string>(value: T) {
    value.at(0);
}
function list<T extends number[]>(value: T) {
    value.at(0);
}
function eventual<T extends Promise<unknown>>(value: T) {
    value.finally(() => {});
}
"#;
    let Some(diagnostics) = check(source, es2015_options()) else {
        return;
    };
    let actual = ts2550(source, &diagnostics);
    assert_eq!(
        actual.iter().map(|(span, _)| *span).collect::<Vec<_>>(),
        ["at", "at", "finally"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(actual[0].1.contains("'es2022'"));
    assert!(actual[1].1.contains("'es2022'"));
    assert!(actual[2].1.contains("'es2018'"));
}

#[test]
fn inactive_language_owners_are_distinct_from_host_library_lookalikes() {
    let source = r#"
interface Map<K, V> {
    clear(): void;
}
declare const map: Map<string, string>;
map.entries();

interface MapConstructor {}
declare const LocalMap: MapConstructor;
LocalMap.groupBy([], value => value);

interface Node {}
declare const node: Node;
node.textContent;
"#;
    let Some(diagnostics) = check(source, es5_options()) else {
        return;
    };
    let actual = ts2550(source, &diagnostics);
    assert_eq!(
        actual.iter().map(|(span, _)| *span).collect::<Vec<_>>(),
        ["entries", "groupBy"],
        "ECMAScript declarations get edition guidance, DOM lookalikes do not: {diagnostics:?}"
    );
    assert!(actual[0].1.contains("'es2015'"));
    assert!(actual[1].1.contains("'es2024'"));
}

#[test]
fn baseline_members_omitted_by_a_partial_lib_are_not_newer_lib_features() {
    let source = r#"
"".charAt(0);
Object.toString();
"".includes("x");
Object.assign({}, {});
"#;
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        lib: vec!["es2015.core".to_string()],
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let Some(diagnostics) = check(source, options) else {
        return;
    };
    assert!(
        ts2550(source, &diagnostics).is_empty(),
        "ES5 members are not newer-lib suggestions, while ES2015 members are active: {diagnostics:?}"
    );
}

#[test]
fn javascript_receivers_follow_check_js_without_reclassifying_shadows() {
    let source = r#"
"x".replaceAll("x", "y");
Object.values({});
function local(Object) {
    Object.values({});
}
"#;
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        lib: vec!["es5".to_string()],
        allow_js: Some(true),
        check_js: Some(true),
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let Some(diagnostics) = check_file("newer_lib_member.js", source, options) else {
        return;
    };
    assert_eq!(
        ts2550(source, &diagnostics)
            .iter()
            .map(|(span, _)| *span)
            .collect::<Vec<_>>(),
        ["replaceAll", "values"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn dynamic_unknown_and_mixed_union_receivers_do_not_get_edition_guidance() {
    let source = r#"
declare const dynamic: any;
declare const opaque: unknown;
declare const mixed: string | number[];
dynamic.at(0);
opaque.at(0);
mixed.at(0);
"#;
    let Some(diagnostics) = check(source, es2015_options()) else {
        return;
    };
    assert!(
        ts2550(source, &diagnostics).is_empty(),
        "TS2550 requires one identifiable standard receiver family: {diagnostics:?}"
    );
}
