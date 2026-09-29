use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("readonly_namespace_exports.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn ts2540_slices<'a>(source: &'a str, diagnostics: &'a [Diagnostic]) -> Vec<(&'a str, &'a str)> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2540)
        .map(|diagnostic| {
            let span = diagnostic
                .span
                .expect("TS2540 should point at the immutable namespace property");
            (
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect()
}

fn diagnostic_slices<'a>(
    source: &'a str,
    diagnostics: &'a [Diagnostic],
) -> Vec<(u32, &'a str, &'a str)> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("diagnostic should have a span");
            (
                diagnostic.code,
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect()
}

#[test]
fn exported_const_namespace_member_is_readonly_for_every_write_form() {
    let source = r#"
namespace Runtime {
    export const value = 0;
    export let mutableLet = 0;
    export var mutableVar = 0;
    export function mutableFunction() {}
    export class MutableClass {}
    const hidden = 0;
}

Runtime.value = 1;
Runtime.value += 1;
++Runtime.value;
Runtime.value++;
(Runtime.value) = 2;
Runtime["value"] = 3;

Runtime.mutableLet = 1;
Runtime.mutableVar += 1;
Runtime.mutableFunction = () => {};
Runtime.MutableClass = class {};
Runtime.hidden = 1;

declare const dynamic: string;
Runtime[dynamic] = 1;
Runtime.missing = 1;

const mutableObject = { value: 0 };
mutableObject.value = 1;
mutableObject["value"] += 1;

function mutateShadow(Runtime: { value: number }) {
    Runtime.value = 1;
}
"#;

    let diagnostics = check(source);
    assert_eq!(
        ts2540_slices(source, &diagnostics),
        [
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "\"value\"",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ambient_namespace_const_is_readonly_without_freezing_other_members() {
    let source = r#"
declare namespace Ambient {
    const value: number;
    let mutableLet: number;
    var mutableVar: number;
    function mutableFunction(): void;
    class MutableClass {}
}

Ambient.value = 1;
Ambient["value"] += 1;
Ambient.mutableLet = 1;
Ambient.mutableVar += 1;
Ambient.mutableFunction = () => {};
Ambient.MutableClass = class {};
"#;

    let diagnostics = check(source);
    assert_eq!(
        ts2540_slices(source, &diagnostics),
        [
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "\"value\"",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nested_and_reopened_namespaces_preserve_const_readonly_metadata() {
    let source = r#"
namespace Outer {
    export namespace Inner {
        export const nested = 0;
        export let mutable = 0;
    }
}

namespace Reopened {
    export const first = 0;
}
namespace Reopened {
    export const second = 0;
    export var mutable = 0;
}

namespace Dotted.Inner {
    export const dotted = 0;
    export let mutable = 0;
}

import Alias = Outer.Inner;

Outer.Inner.nested = 1;
Outer.Inner.mutable = 1;
Outer.Inner.missing = 1;
Alias.nested = 1;
Reopened.first = 1;
Reopened["second"]++;
Reopened.mutable = 1;
Dotted.Inner.dotted = 1;
Dotted.Inner.mutable = 1;
"#;

    let diagnostics = check(source);
    assert_eq!(
        ts2540_slices(source, &diagnostics),
        [
            (
                "Cannot assign to 'nested' because it is a read-only property.",
                "nested",
            ),
            (
                "Cannot assign to 'nested' because it is a read-only property.",
                "nested",
            ),
            (
                "Cannot assign to 'first' because it is a read-only property.",
                "first",
            ),
            (
                "Cannot assign to 'second' because it is a read-only property.",
                "\"second\"",
            ),
            (
                "Cannot assign to 'dotted' because it is a read-only property.",
                "dotted",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn readonly_write_suppresses_target_errors_but_still_checks_the_rhs() {
    let source = r#"
namespace Runtime {
    export const value = 0;
    export const object: { a: number } = { a: 0 };
}

function takesString(value: string): number { return 0; }

Runtime.value = 1;
Runtime.value = takesString(123);
Runtime.value -= "bad";
Runtime.object = { a: 1, extra: 2 };

let mutable: number = 0;
mutable = "bad";
"#;

    let diagnostics = check(source);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| {
                let span = diagnostic.span.expect("diagnostic should have a span");
                (
                    diagnostic.code,
                    &source[span.start as usize..span.end as usize],
                )
            })
            .collect::<Vec<_>>(),
        [
            (2540, "value"),
            (2540, "value"),
            (2345, "123"),
            (2540, "value"),
            (2540, "object"),
            (2322, "mutable"),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn import_equals_namespace_alias_uses_its_lexical_binding() {
    let source = r#"
namespace SourceRoot {
    export namespace Inner {
        export const value = 0;
    }
}

import Alias = SourceRoot.Inner;

Alias.value = 1;

function typedAliasShadow(Alias: { value: number }) {
    Alias.value = 1;
}

function anyAliasShadow(Alias: any) {
    Alias.value = 1;
}

function canonicalRootShadow(SourceRoot: { Inner: { value: number } }) {
    Alias.value = 1;
}

namespace Use {
    import NestedAlias = SourceRoot.Inner;

    NestedAlias.value = 1;

    function anyNestedAliasShadow(NestedAlias: any) {
        NestedAlias.value = 1;
    }
}
"#;

    let diagnostics = check(source);
    assert_eq!(
        ts2540_slices(source, &diagnostics),
        [
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reverse_order_import_equals_chains_resolve_to_a_fixed_point() {
    let source = r#"
namespace Root {
    export namespace Inner {
        export const value = 0;
    }
}

import Direct = Later;
import Later = Root.Inner;
Direct.value = 1;

import Member = LaterRoot.Inner;
import LaterRoot = Root;
Member.value = 1;

namespace Use {
    import Nested = NestedRoot.Inner;
    import NestedRoot = Root;
    Nested.value = 1;
}
"#;

    let diagnostics = check(source);
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn cyclic_import_equals_aliases_report_once_without_namespace_evidence() {
    let source = r#"import A = B;
import B = A;
A.value = 1;
B.value = 1;
"#;

    let diagnostics = check(source);
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [(
            2303,
            "Circular definition of import alias 'A'.",
            "import A = B;",
        )],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn cyclic_aliases_do_not_block_an_independent_namespace_alias() {
    let source = r#"namespace Root {
    export const value = 0;
}
import A = B;
import B = A;
import Independent = Root;
A.value = 1;
B.value = 1;
Independent.value = 1;
"#;

    let diagnostics = check(source);
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [
            (
                2303,
                "Circular definition of import alias 'A'.",
                "import A = B;",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn import_equals_alias_preserves_readonly_evidence_through_constant_receiver_paths() {
    let source = r#"
namespace Root {
    export namespace Inner {
        export namespace Deeper {
            export const value = 0;
            export let mutable = 0;
        }
    }
}
import Alias = Root;

(Alias).Inner.Deeper.value = 1;
Alias!.Inner.Deeper.value = 1;
(Alias satisfies typeof Alias).Inner.Deeper.value = 1;
Alias.Inner.Deeper.value = 1;
Alias["Inner"].Deeper.value = 1;
Alias[`Inner`]["Deeper"].value = 1;
Alias.Inner.Deeper["value"] = 1;

(Alias as any).Inner.Deeper.value = 1;
(<any>Alias).Inner.Deeper.value = 1;
(Alias.Inner as any).Deeper.value = 1;
Alias.Inner.Deeper.mutable = 1;
Alias.Inner.Missing.value = 1;
declare const dynamic: string;
Alias[dynamic].Deeper.value = 1;
"#;

    let diagnostics = check(source);
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "\"value\"",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nested_alias_receiver_paths_obey_nearest_binding_and_capture_barriers() {
    let source = r#"
namespace SourceRoot {
    export namespace Inner {
        export const value = 0;
        export let mutable = 0;
    }
}
import Alias = SourceRoot;

function typedShadow(Alias: { Inner: { value: number } }) {
    (Alias).Inner.value = 1;
    Alias["Inner"].value = 1;
}

function anyShadow(Alias: any) {
    Alias!.Inner.value = 1;
}

function sourceRootShadow(SourceRoot: { Inner: { value: number } }) {
    Alias.Inner.value = 1;
}

Alias.Inner.value = 1;
Alias.Inner.mutable = 1;
"#;

    let diagnostics = check(source);
    assert_eq!(
        diagnostic_slices(source, &diagnostics),
        [
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
            (
                2540,
                "Cannot assign to 'value' because it is a read-only property.",
                "value",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}
