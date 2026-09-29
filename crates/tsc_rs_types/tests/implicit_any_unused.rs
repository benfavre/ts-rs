use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source_name: &str, source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(source_name, source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

#[test]
fn reports_implicit_any_for_unannotated_class_implementation_parameters() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
class Example {
    constructor(first, inferred = 1, typed: string) {}
    method(value, optional?: number) {}
    rest(...items) {}
}
class RestConstructor {
    constructor(...args) {}
}
"#;
    let diagnostics = check("class_parameters.ts", source, options);

    let implicit_any: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code, 7006 | 7019))
        .collect();
    let actual: Vec<_> = implicit_any
        .iter()
        .map(|diagnostic| {
            let span = diagnostic
                .span
                .expect("implicit-any parameters should have an exact span");
            (
                diagnostic.code,
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (
                7006,
                "Parameter 'first' implicitly has an 'any' type.",
                "first",
            ),
            (
                7006,
                "Parameter 'value' implicitly has an 'any' type.",
                "value",
            ),
            (
                7019,
                "Rest parameter 'items' implicitly has an 'any[]' type.",
                "...items",
            ),
            (
                7019,
                "Rest parameter 'args' implicitly has an 'any[]' type.",
                "...args",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_runtime_destructured_binding_elements_without_context() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
function declaration([first, ...arrayRest], { source: local, shorthand, preset = 1, ...objectRest }) {}
class Container {
    method({ outer: { nested } }, [withDefault = true]) {}
    constructor(...[ctorRest]) {}
}
const free = ([arrowValue], { key: arrowLocal }) => {};
const contextual: ([value]: [number]) => void = ([value]) => {};
const initialized = ({ value } = { value: 1 }) => {};
const annotated = ({ value }: { value: number }) => {};
"#;
    let diagnostics = check("runtime_binding_elements.ts", source, options);

    let mut actual: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 7031)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS7031 should carry an exact span");
            (
                span.start,
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    actual.sort_by_key(|(start, _, _)| *start);
    assert_eq!(
        actual
            .into_iter()
            .map(|(_, message, slice)| (message, slice))
            .collect::<Vec<_>>(),
        [
            (
                "Binding element 'first' implicitly has an 'any' type.",
                "first",
            ),
            (
                "Binding element 'arrayRest' implicitly has an 'any' type.",
                "arrayRest",
            ),
            (
                "Binding element 'local' implicitly has an 'any' type.",
                "local",
            ),
            (
                "Binding element 'shorthand' implicitly has an 'any' type.",
                "shorthand",
            ),
            (
                "Binding element 'nested' implicitly has an 'any' type.",
                "nested",
            ),
            (
                "Binding element 'ctorRest' implicitly has an 'any' type.",
                "ctorRest",
            ),
            (
                "Binding element 'arrowValue' implicitly has an 'any' type.",
                "arrowValue",
            ),
            (
                "Binding element 'arrowLocal' implicitly has an 'any' type.",
                "arrowLocal",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn parse_recovery_does_not_invent_implicit_any_binding_elements() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    for source in [
        "function broken { static method() }",
        "export default function getThing( { return 'thing'; }",
    ] {
        let parsed = tsc_rs_parser::parse("recovered_parameter.ts", source);
        assert!(
            !parsed.diagnostics.is_empty(),
            "the regression needs parser recovery: {source}"
        );
        let symbols = tsc_rs_symbols::bind(&parsed);
        let diagnostics = TypeChecker::new()
            .check_with_options(&parsed, &symbols, &options)
            .diagnostics;
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.code != 7031),
            "diagnostics for {source}: {diagnostics:?}"
        );
    }
}

#[test]
fn reports_uninitialized_destructuring_declaration_bindings() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
var [first, initialized = 1, ...arrayRest],
    { renamed: local, shorthand, preset = true, ...objectRest };
var [typed]: [any], { typedObject }: { typedObject: any };
var [inferred] = [1], { inferredObject } = { inferredObject: true };
"#;
    let diagnostics = check("destructuring_declarations.ts", source, options);
    let mut actual: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 7031)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS7031 should carry an exact span");
            (span.start, &source[span.start as usize..span.end as usize])
        })
        .collect();
    actual.sort_by_key(|(start, _)| *start);
    assert_eq!(
        actual
            .into_iter()
            .map(|(_, slice)| slice)
            .collect::<Vec<_>>(),
        ["first", "arrayRest", "local", "shorthand"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn unrelated_parse_errors_do_not_hide_ts7031() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
function before({ beforeValue }) {}
const broken = ;
function after({ afterValue }) {}
"#;
    let diagnostics = check("local_parse_recovery.ts", source, options);
    let slices: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 7031)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS7031 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        slices,
        ["beforeValue", "afterValue"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn sole_array_rest_is_inferred_but_trailing_rest_is_implicit_any() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
function sole([...clean]) {}
function trailing([head, ...reported]) {}
function nested([...[nestedClean]]) {}
var [...cleanVar];
var [, ...reportedVar];
"#;
    let diagnostics = check("array_rest_binding.ts", source, options);
    let slices: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 7031)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS7031 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        slices,
        ["head", "reported", "reportedVar"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn array_defaults_only_infer_positions_the_initializer_supplies() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
function empty([first, { nested }] = []) {}
function partial([supplied, missing] = [1]) {}
function elided([explicitUndefined, afterElision] = [,]) {}
function complete([supplied, nested] = [1, { nested: true }]) {}
function rest([...inferredRest] = []) {}
"#;
    let parsed = tsc_rs_parser::parse("array_default_bindings.ts", source);
    assert!(
        parsed.diagnostics.is_empty(),
        "unexpected parse diagnostics: {:?}",
        parsed.diagnostics
    );
    let diagnostics = check("array_default_bindings.ts", source, options);
    let actual: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 7031)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS7031 should carry a span");
            (
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (
                "Binding element 'first' implicitly has an 'any' type.",
                "first",
            ),
            (
                "Binding element '{ nested }' implicitly has an 'any' type.",
                "{ nested }",
            ),
            (
                "Binding element 'missing' implicitly has an 'any' type.",
                "missing",
            ),
            (
                "Binding element 'afterElision' implicitly has an 'any' type.",
                "afterElision",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn recovered_missing_binding_uses_typescript_name_and_zero_width_span() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = "function missing({ value: }) {}";
    let diagnostics = check("missing_binding.ts", source, options);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == 7031)
        .expect("missing binding should receive TS7031");
    assert_eq!(
        diagnostic.message,
        "Binding element '(Missing)' implicitly has an 'any' type."
    );
    let span = diagnostic.span.expect("TS7031 should carry a span");
    assert_eq!(span.start, span.end);
    assert_eq!(span.start as usize, source.find('}').unwrap());
}

#[test]
fn reports_uncontextualized_arrow_and_function_expression_parameters() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
const arrow = (value, optional?, inferred = 1, typed: string, ...rest) => value;
const expression = function (other, initialized = true, ...items) {};
"#;
    let diagnostics = check("expression_parameters.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(code, _, message, slice)| (code, message, slice))
            .collect::<Vec<_>>(),
        [
            (
                7006,
                "Parameter 'value' implicitly has an 'any' type.",
                "value",
            ),
            (
                7006,
                "Parameter 'optional' implicitly has an 'any' type.",
                "optional?",
            ),
            (
                7019,
                "Rest parameter 'rest' implicitly has an 'any[]' type.",
                "...rest",
            ),
            (
                7006,
                "Parameter 'other' implicitly has an 'any' type.",
                "other",
            ),
            (
                7019,
                "Rest parameter 'items' implicitly has an 'any[]' type.",
                "...items",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn contextual_function_slots_suppress_implicit_any_expression_diagnostics() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
let arrow: (value: string, count?: number) => void = (value, count) => {};
let expression: (value: string) => void = function (value) {};
let rest: (head: string, ...tail: number[]) => void = (head, ...tail) => {};
"#;
    let diagnostics = check("contextual_expression_parameters.ts", source, options);

    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, 7006 | 7019)),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn contextual_binding_diagnostics_follow_actual_callable_slots() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
declare function takesAny(value: any): void;
declare function takesUnknown(value: unknown): void;
declare function takesCallback(value: (arg: { x: number }) => void): void;

let untyped;
let anyTarget: any;
let unknownTarget: unknown;
let callbackTarget: (arg: { x: number }) => void;
untyped = ({ assignUntyped }) => {};
anyTarget = ({ assignAny }) => {};
unknownTarget = ({ assignUnknown }) => {};
callbackTarget = ({ x }) => {};

const freeArray = [({ arrayFree }) => {}];
const anyArray: any[] = [({ arrayAny }) => {}];
const unknownArray: unknown[] = [({ arrayUnknown }) => {}];
const callbackArray: ((arg: { x: number }) => void)[] = [({ x }) => {}];

const assertedAny = (({ asserted }) => {}) as any;
const satisfiedAny = (({ satisfied }) => {}) satisfies any;
const assertedCallback = (({ x }) => {}) as (arg: { x: number }) => void;
const satisfiedCallback = (({ x }) => {}) satisfies (arg: { x: number }) => void;

takesAny(({ callAny }) => {});
takesUnknown(({ callUnknown }) => {});
takesCallback(({ x }) => {});

const freeObject = { method({ objectMethod }) {} };
const typedObject: { method(arg: { x: number }): void } = { method({ x }) {} };
"#;
    let diagnostics = check("contextual_binding_slots.ts", source, options);
    let actual: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 7031)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS7031 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        actual,
        [
            "assignUntyped",
            "assignAny",
            "assignUnknown",
            "arrayFree",
            "arrayAny",
            "arrayUnknown",
            "asserted",
            "satisfied",
            "callAny",
            "callUnknown",
            "objectMethod",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn expression_parameter_context_follows_logical_and_iife_rules() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
const or = ((value: string) => value) || (right => right);
const coal = ((value: string) => value) ?? (right => right);
const and = ((value: string) => value) && (untyped => untyped);
((item) => item)("contextual argument");
const { inferred = (item) => item } = {};
"#;
    let diagnostics = check("expression_parameter_context.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["untyped", "item"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn checks_unused_locals_in_checked_commonjs_modules() {
    let mut options = CompilerOptions::default();
    options.no_unused_locals = Some(true);
    let diagnostics = check(
        "/commonjs.js",
        "const unused = 0;\nexports.answer = 42;\n",
        options,
    );

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == 6133 && diagnostic.message.contains("'unused'")
        }),
        "diagnostics: {diagnostics:?}"
    );
}

fn implicit_any_slices<'a>(
    source: &'a str,
    diagnostics: &'a [Diagnostic],
) -> Vec<(u32, &'a str, &'a str)> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 7006)
        .map(|diagnostic| {
            let span = diagnostic
                .span
                .expect("TS7006 should carry the parameter span");
            (
                span.start,
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect()
}

fn parameter_implicit_any_slices<'a>(
    source: &'a str,
    diagnostics: &'a [Diagnostic],
) -> Vec<(u32, u32, &'a str, &'a str)> {
    diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code, 7006 | 7019))
        .map(|diagnostic| {
            let span = diagnostic
                .span
                .expect("implicit-any parameter should carry an exact span");
            (
                diagnostic.code,
                span.start,
                diagnostic.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect()
}

#[test]
fn uses_ts7019_for_declarations_ambient_members_and_missing_names() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
function implemented(value, ...items) {}
declare function ambient(other, ...rest): void;
declare class PublicApi {
    method(methodValue, ...methodItems): void;
    constructor(ctorValue, ...ctorItems);
    private hidden(hiddenValue, ...hiddenItems): void;
}
function malformed(enum) {}
function missing(...) {}
function missingAfterComment(.../* gap */) {}
function missingKeyword(...enum) {}
"#;
    let diagnostics = check("declarations.ts", source, options);
    let mut parameter_diagnostics = parameter_implicit_any_slices(source, &diagnostics);
    parameter_diagnostics.sort_by_key(|(_, start, _, _)| *start);

    assert_eq!(
        parameter_diagnostics
            .into_iter()
            .map(|(code, _, message, slice)| (code, message, slice))
            .collect::<Vec<_>>(),
        [
            (
                7006,
                "Parameter 'value' implicitly has an 'any' type.",
                "value",
            ),
            (
                7019,
                "Rest parameter 'items' implicitly has an 'any[]' type.",
                "...items",
            ),
            (
                7006,
                "Parameter 'other' implicitly has an 'any' type.",
                "other",
            ),
            (
                7019,
                "Rest parameter 'rest' implicitly has an 'any[]' type.",
                "...rest",
            ),
            (
                7006,
                "Parameter 'methodValue' implicitly has an 'any' type.",
                "methodValue",
            ),
            (
                7019,
                "Rest parameter 'methodItems' implicitly has an 'any[]' type.",
                "...methodItems",
            ),
            (
                7006,
                "Parameter 'ctorValue' implicitly has an 'any' type.",
                "ctorValue",
            ),
            (
                7019,
                "Rest parameter 'ctorItems' implicitly has an 'any[]' type.",
                "...ctorItems",
            ),
            (
                7019,
                "Rest parameter '(Missing)' implicitly has an 'any[]' type.",
                "...",
            ),
            (
                7019,
                "Rest parameter '(Missing)' implicitly has an 'any[]' type.",
                "...",
            ),
            (
                7019,
                "Rest parameter '(Missing)' implicitly has an 'any[]' type.",
                "...",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nameless_type_signature_parameters_are_not_implicit_any_bindings() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
import type { Imported } from "./types";
import type DefaultType from "./missing-default";
import type * as Types from "./missing-namespace";
import { ValueImported } from "./missing-value";
import DefaultValue from "./missing-default-value";
import * as ValueTypes from "./missing-value-namespace";
class C {}
declare var a: { m(...string): void };
declare var b: (string, C) => void;
declare var c: { (C, number): void };
declare var d: { m(boolean, C, object, undefined): void };
type Builtins = (Date, Array, Promise, RegExp, Error) => void;
type Generic<T> = (T, ...T) => void;
type NestedGeneric = <U>(U) => void;
type ConstructorGeneric<T> = new (T) => object;
type RestArray<T> = (...T[]) => void;
type RestArrayNoArrow<T> = (...T[]);
interface Accessors { set value(string); }
type AccessorLiteral = { set value(number); };
type LeakingAlias<T> = T;
type OutsideAlias = (T) => void;
class LeakingClass<V> {}
type OutsideClass = (V) => void;
interface Signatures {
    generic<W>(W): void;
    unrelated(W): void;
}
type OutsideInterface = (W) => void;
const asserted = null as (<A>(A) => void);
class Holder { field = null as (<B>(B) => void); }
type Inferred<X> = X extends infer V ? (V) => void : never;
type InferredReference<X> = X extends Promise<infer R> ? (R) => void : never;
type InferredObject<X> = X extends { value: infer O } ? (O) => void : never;
type InferredTuple<X> = X extends [infer P] ? (P) => void : never;
type InferredIndexed<X> = X extends Record<string, infer Q> ? (Q) => void : never;
type Constraint<X extends <Y>(Y) => void> = X;
interface ConstraintInterface<X extends <Z>(Z) => void> {}
namespace N {
    export class ScopedClass {}
    type Inside = (ScopedClass) => void;
}
type OutsideNamespaceClass = (ScopedClass) => void;
namespace M {
    export interface ScopedInterface {}
}
type OutsideNamespaceInterface = (ScopedInterface) => void;
class NestedBody {
    method() {
        type Local<T> = (T) => void;
    }
}
function nestedControlFlow() {
    if (true) {
        type Local<U> = (U) => void;
    }
}
type TemplateInterpolation = `${(<K>(K, K) => void)}`;
type StringComma = (first: "a,b", string) => void;
type CommentComma = (first: string /* a,b */, number) => void;
type RegexComma = (first = /a,b/, string) => void;
type RegexAfterTypeof = (first = typeof /a,b/, string) => void;
type RegexAfterPlus = (first = +/a,b/, string) => void;
type RegexAfterMultiply = (first = 1 * /a,b/, string) => void;
type ImportedSignature = (Imported) => void;
type DefaultSignature = (DefaultType) => void;
type NamespaceSignature = (Types) => void;
type ValueImportedSignature = (ValueImported) => void;
type DefaultValueSignature = (DefaultValue) => void;
type ValueNamespaceSignature = (ValueTypes) => void;
"#;
    let diagnostics = check("nameless.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(code, _, message, slice)| (code, message, slice))
            .collect::<Vec<_>>(),
        [
            (7006, "Parameter 'T' implicitly has an 'any' type.", "T",),
            (
                7006,
                "Parameter 'string' implicitly has an 'any' type.",
                "string",
            ),
            (
                7006,
                "Parameter 'number' implicitly has an 'any' type.",
                "number",
            ),
            (7006, "Parameter 'T' implicitly has an 'any' type.", "T",),
            (7006, "Parameter 'V' implicitly has an 'any' type.", "V",),
            (7006, "Parameter 'W' implicitly has an 'any' type.", "W",),
            (7006, "Parameter 'W' implicitly has an 'any' type.", "W",),
            (
                7006,
                "Parameter 'ScopedClass' implicitly has an 'any' type.",
                "ScopedClass",
            ),
            (
                7006,
                "Parameter 'ScopedInterface' implicitly has an 'any' type.",
                "ScopedInterface",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                (
                    diagnostic.message.as_str(),
                    &source[span.start as usize..span.end as usize],
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                "Parameter has a name but no type. Did you mean 'arg0: string[]'?",
                "...string",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: string'?",
                "string",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: C'?",
                "C",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: C'?",
                "C",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: number'?",
                "number",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: boolean'?",
                "boolean",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: C'?",
                "C",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg2: object'?",
                "object",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg3: undefined'?",
                "undefined",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: Date'?",
                "Date",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: Array'?",
                "Array",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg2: Promise'?",
                "Promise",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg3: RegExp'?",
                "RegExp",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg4: Error'?",
                "Error",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: T'?",
                "T",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: T[]'?",
                "...T",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: U'?",
                "U",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: T[]'?",
                "...T",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: T[]'?",
                "...T",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: W'?",
                "W",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: A'?",
                "A",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: B'?",
                "B",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: V'?",
                "V",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: R'?",
                "R",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: O'?",
                "O",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: P'?",
                "P",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: Q'?",
                "Q",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: Y'?",
                "Y",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: Z'?",
                "Z",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: ScopedClass'?",
                "ScopedClass",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: T'?",
                "T",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: U'?",
                "U",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: K'?",
                "K",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: K'?",
                "K",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: string'?",
                "string",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: number'?",
                "number",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: string'?",
                "string",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: string'?",
                "string",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: string'?",
                "string",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg1: string'?",
                "string",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: Imported'?",
                "Imported",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: DefaultType'?",
                "DefaultType",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: Types'?",
                "Types",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: ValueImported'?",
                "ValueImported",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: DefaultValue'?",
                "DefaultValue",
            ),
            (
                "Parameter has a name but no type. Did you mean 'arg0: ValueTypes'?",
                "ValueTypes",
            ),
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nameless_parameter_index_counts_after_relational_less_than() {
    let source = r#"
class C {}
type F = (x = 0 < 1, C) => void;
type K = (x = 0 < 1, y = 2 > 1, C) => void;
type L = (x = 0 < ">", C) => void;
type N = (x = 0 < /a,b/.source.length, C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("relational-less-than-parameter.ts", source, options);

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "Parameter has a name but no type. Did you mean 'arg1: C'?",
            "Parameter has a name but no type. Did you mean 'arg2: C'?",
            "Parameter has a name but no type. Did you mean 'arg1: C'?",
            "Parameter has a name but no type. Did you mean 'arg1: C'?",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn injected_global_types_do_not_include_namespace_members() {
    let declarations = tsc_rs_parser::parse(
        "globals.d.ts",
        r#"
declare class GlobalClass {}
interface GlobalInterface {}
type GlobalAlias = {};
declare enum GlobalEnum {}
declare namespace N { interface NamespaceOnly {} }
"#,
    );
    let source = r#"
type A = (GlobalClass) => void;
type B = (GlobalInterface) => void;
type C = (GlobalAlias) => void;
type D = (GlobalEnum) => void;
type E = (NamespaceOnly) => void;
type F = (FakeNamespaceOnly) => void;
"#;
    let file = tsc_rs_parser::parse("use.ts", source);
    let fake_global = tsc_rs_parser::parse(
        "fake-global.d.ts",
        r#"
export {};
declare namespace global { interface FakeNamespaceOnly {} }
"#,
    );
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.inject_external_types(&[&declarations, &fake_global]);
    checker.take_diagnostics();
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = checker
        .check_with_options(&file, &symbols, &options)
        .diagnostics;

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        [
            "GlobalClass",
            "GlobalInterface",
            "GlobalAlias",
            "GlobalEnum",
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["NamespaceOnly", "FakeNamespaceOnly"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nested_conditional_infer_does_not_escape_its_true_branch() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let source = r#"
type Nested<X> = X extends (X extends infer U ? U : never)
    ? (U) => void
    : never;
"#;
    let diagnostics = check("nested-infer.ts", source, options);
    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(code, _, message, slice)| (code, message, slice))
            .collect::<Vec<_>>(),
        [(7006, "Parameter 'U' implicitly has an 'any' type.", "U",)],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 7051),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn resolved_imports_use_the_export_type_namespace() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after Unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("tsc-rs-implicit-any-imports-{unique}"));
    std::fs::create_dir_all(&directory).expect("create import fixture directory");
    let value_path = directory.join("value.ts");
    let types_path = directory.join("types.ts");
    let type_barrel_path = directory.join("type-barrel.ts");
    let shadow_barrel_path = directory.join("shadow-barrel.ts");
    let ambiguous_value_path = directory.join("ambiguous-value.ts");
    let ambiguous_type_path = directory.join("ambiguous-type.ts");
    let ambiguous_barrel_path = directory.join("ambiguous-barrel.ts");
    let ambiguous_type_first_barrel_path = directory.join("ambiguous-type-first-barrel.ts");
    let unrelated_path = directory.join("unrelated.ts");
    let anonymous_path = directory.join("anonymous.ts");
    let source_path = directory.join("source.ts");
    let barrel_path = directory.join("barrel.ts");
    let default_class_path = directory.join("default-class.ts");
    let default_expression_barrel_path = directory.join("default-expression-barrel.ts");
    let commonjs_path = directory.join("commonjs.ts");
    let commonjs_barrel_path = directory.join("commonjs-barrel.ts");
    let expression_commonjs_path = directory.join("expression-commonjs.ts");
    let namespace_source_path = directory.join("namespace-source.ts");
    let namespace_barrel_path = directory.join("namespace-barrel.ts");
    let qualified_path = directory.join("qualified.ts");
    let use_path = directory.join("use.ts");
    let value_source = "export let Foo: string;\nexport function activate() {}\n";
    let types_source = r#"
export type Alias = string;
export type X = {};
export enum E { A }
export interface Original {}
export { Original as Renamed };
const V = 1;
export type { V };
"#;
    let type_barrel_source = "export type * from \"./value\";\n";
    let shadow_barrel_source = "export const X = 1;\nexport * from \"./types\";\n";
    let ambiguous_value_source = "export const Ambiguous = 1;\n";
    let ambiguous_type_source = "export type Ambiguous = {};\n";
    let ambiguous_barrel_source =
        "export * from \"./ambiguous-value\";\nexport * from \"./ambiguous-type\";\n";
    let ambiguous_type_first_barrel_source =
        "export * from \"./ambiguous-type\";\nexport * from \"./ambiguous-value\";\n";
    let unrelated_source = "export type Foo = string;\n";
    let anonymous_source = "export default class {}\n";
    let source_source = "export default interface DefaultType {}\n";
    let barrel_source = "import DefaultType from \"./source\";\nexport { DefaultType };\n";
    let default_class_source = "export default class C {}\n";
    let default_expression_barrel_source =
        "import C from \"./default-class\";\nexport default C;\n";
    let commonjs_source = "class ExportedClass {}\nexport = ExportedClass;\n";
    let commonjs_barrel_source =
        "import ExportedClass = require(\"./commonjs\");\nexport = ExportedClass;\n";
    let expression_commonjs_source = "export = function () {};\n";
    let namespace_source = "export const x = 1;\n";
    let namespace_barrel_source =
        "import * as NS from \"./namespace-source\";\nexport { NS };\nexport * as DirectNS from \"./namespace-source\";\nexport type * as TypeNS from \"./namespace-source\";\n";
    let qualified_source = r#"
namespace Q {
    export const Other = 0;
    function Hidden() {}
}
namespace Q {
    export const Value = 1;
    export class Class {}
}
import QualifiedValue = Q.Value;
import QualifiedClass = Q.Class;
import HiddenValue = Q.Hidden;
import QAlias = Q;
import ChainedValue = QAlias.Value;
import ChainedClass = QAlias.Class;
export {
    QualifiedValue,
    QualifiedClass,
    HiddenValue,
    ChainedValue,
    ChainedClass,
};
"#;
    let use_source = r#"
import type { Foo } from "./value";
import { activate } from "./value";
import { Alias, E, Renamed, V } from "./types";
import { activate as TypeOnlyActivate } from "./type-barrel";
import { X as ShadowX } from "./shadow-barrel";
import { Ambiguous } from "./ambiguous-barrel";
import { Ambiguous as TypeFirstAmbiguous } from "./ambiguous-type-first-barrel";
import Anonymous from "./anonymous";
import { DefaultType } from "./barrel";
import DefaultBarrel from "./default-expression-barrel";
import { Missing } from "./value";
import MissingDefault from "./value";
import ImportedClass = require("./commonjs-barrel");
import ExpressionValue = require("./expression-commonjs");
import { NS, DirectNS, TypeNS } from "./namespace-barrel";
import {
    QualifiedValue,
    QualifiedClass,
    HiddenValue,
    ChainedValue,
    ChainedClass,
} from "./qualified";
type ValueOnly = (Foo) => void;
type ValueFunction = (activate) => void;
type TypeOnlyValue = (V) => void;
type TypeOnlyBarrelValue = (TypeOnlyActivate) => void;
type ShadowedStarType = (ShadowX) => void;
type AmbiguousStarType = (Ambiguous) => void;
type TypeFirstAmbiguousStarType = (TypeFirstAmbiguous) => void;
type AliasType = (Alias) => void;
type EnumType = (E) => void;
type RenamedType = (Renamed) => void;
type AnonymousType = (Anonymous) => void;
type BarrelType = (DefaultType) => void;
type DefaultBarrelType = (DefaultBarrel) => void;
type MissingType = (Missing) => void;
type MissingDefaultType = (MissingDefault) => void;
type ImportEqualsType = (ImportedClass) => void;
type ExpressionValueType = (ExpressionValue) => void;
type NamespaceValueType = (NS) => void;
type DirectNamespaceValueType = (DirectNS) => void;
type TypeNamespaceValueType = (TypeNS) => void;
type QualifiedValueType = (QualifiedValue) => void;
type QualifiedClassType = (QualifiedClass) => void;
type HiddenValueType = (HiddenValue) => void;
type ChainedValueType = (ChainedValue) => void;
type ChainedClassType = (ChainedClass) => void;
"#;
    for (path, source) in [
        (&value_path, value_source),
        (&types_path, types_source),
        (&type_barrel_path, type_barrel_source),
        (&shadow_barrel_path, shadow_barrel_source),
        (&ambiguous_value_path, ambiguous_value_source),
        (&ambiguous_type_path, ambiguous_type_source),
        (&ambiguous_barrel_path, ambiguous_barrel_source),
        (
            &ambiguous_type_first_barrel_path,
            ambiguous_type_first_barrel_source,
        ),
        (&unrelated_path, unrelated_source),
        (&anonymous_path, anonymous_source),
        (&source_path, source_source),
        (&barrel_path, barrel_source),
        (&default_class_path, default_class_source),
        (
            &default_expression_barrel_path,
            default_expression_barrel_source,
        ),
        (&commonjs_path, commonjs_source),
        (&commonjs_barrel_path, commonjs_barrel_source),
        (&expression_commonjs_path, expression_commonjs_source),
        (&namespace_source_path, namespace_source),
        (&namespace_barrel_path, namespace_barrel_source),
        (&qualified_path, qualified_source),
        (&use_path, use_source),
    ] {
        std::fs::write(path, source).expect("write import fixture");
    }

    let value = tsc_rs_parser::parse(value_path.to_str().unwrap(), value_source);
    let types = tsc_rs_parser::parse(types_path.to_str().unwrap(), types_source);
    let type_barrel = tsc_rs_parser::parse(type_barrel_path.to_str().unwrap(), type_barrel_source);
    let shadow_barrel =
        tsc_rs_parser::parse(shadow_barrel_path.to_str().unwrap(), shadow_barrel_source);
    let ambiguous_value = tsc_rs_parser::parse(
        ambiguous_value_path.to_str().unwrap(),
        ambiguous_value_source,
    );
    let ambiguous_type =
        tsc_rs_parser::parse(ambiguous_type_path.to_str().unwrap(), ambiguous_type_source);
    let ambiguous_barrel = tsc_rs_parser::parse(
        ambiguous_barrel_path.to_str().unwrap(),
        ambiguous_barrel_source,
    );
    let ambiguous_type_first_barrel = tsc_rs_parser::parse(
        ambiguous_type_first_barrel_path.to_str().unwrap(),
        ambiguous_type_first_barrel_source,
    );
    let unrelated = tsc_rs_parser::parse(unrelated_path.to_str().unwrap(), unrelated_source);
    let anonymous = tsc_rs_parser::parse(anonymous_path.to_str().unwrap(), anonymous_source);
    let source = tsc_rs_parser::parse(source_path.to_str().unwrap(), source_source);
    let barrel = tsc_rs_parser::parse(barrel_path.to_str().unwrap(), barrel_source);
    let default_class =
        tsc_rs_parser::parse(default_class_path.to_str().unwrap(), default_class_source);
    let default_expression_barrel = tsc_rs_parser::parse(
        default_expression_barrel_path.to_str().unwrap(),
        default_expression_barrel_source,
    );
    let commonjs = tsc_rs_parser::parse(commonjs_path.to_str().unwrap(), commonjs_source);
    let commonjs_barrel = tsc_rs_parser::parse(
        commonjs_barrel_path.to_str().unwrap(),
        commonjs_barrel_source,
    );
    let expression_commonjs = tsc_rs_parser::parse(
        expression_commonjs_path.to_str().unwrap(),
        expression_commonjs_source,
    );
    let namespace_source_file =
        tsc_rs_parser::parse(namespace_source_path.to_str().unwrap(), namespace_source);
    let namespace_barrel = tsc_rs_parser::parse(
        namespace_barrel_path.to_str().unwrap(),
        namespace_barrel_source,
    );
    let qualified = tsc_rs_parser::parse(qualified_path.to_str().unwrap(), qualified_source);
    let use_file = tsc_rs_parser::parse(use_path.to_str().unwrap(), use_source);
    let symbols = tsc_rs_symbols::bind(&use_file);
    let mut checker = TypeChecker::new();
    checker.inject_external_types(&[
        &value,
        &types,
        &type_barrel,
        &shadow_barrel,
        &ambiguous_value,
        &ambiguous_type,
        &ambiguous_barrel,
        &ambiguous_type_first_barrel,
        &unrelated,
        &anonymous,
        &source,
        &barrel,
        &default_class,
        &default_expression_barrel,
        &commonjs,
        &commonjs_barrel,
        &expression_commonjs,
        &namespace_source_file,
        &namespace_barrel,
        &qualified,
        &use_file,
    ]);
    checker.take_diagnostics();
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = checker
        .check_with_options(&use_file, &symbols, &options)
        .diagnostics;
    std::fs::remove_dir_all(&directory).expect("remove import fixture directory");

    assert_eq!(
        parameter_implicit_any_slices(use_source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        [
            "Foo",
            "activate",
            "V",
            "TypeOnlyActivate",
            "ShadowX",
            "Ambiguous",
            "ExpressionValue",
            "NS",
            "DirectNS",
            "TypeNS",
            "QualifiedValue",
            "ChainedValue",
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &use_source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        [
            "TypeFirstAmbiguous",
            "Alias",
            "E",
            "Renamed",
            "Anonymous",
            "DefaultType",
            "DefaultBarrel",
            "Missing",
            "MissingDefault",
            "ImportedClass",
            "QualifiedClass",
            "HiddenValue",
            "ChainedClass",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn resolved_imports_probe_sources_missing_from_the_donor() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after Unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("tsc-rs-implicit-any-cold-imports-{unique}"));
    std::fs::create_dir_all(&directory).expect("create cold import fixture directory");
    let value_path = directory.join("extension.ts");
    let types_path = directory.join("cold-types.ts");
    let ambiguous_value_path = directory.join("cold-ambiguous-value.ts");
    let ambiguous_type_path = directory.join("cold-ambiguous-type.ts");
    let ambiguous_type_first_barrel_path = directory.join("cold-type-first-barrel.ts");
    let use_path = directory.join("use.ts");
    let value_source = r#"
export function activate() {}
export function deactivate() {}
export { activate as fV2 };
export type { deactivate as fV3 };
export class ColdClass {}
export interface ColdInterface {}
export enum ColdEnum { A }
export * from "./cold-types";
export const Shadowed = 1;
export * from "./cold-ambiguous-value";
export * from "./cold-ambiguous-type";
namespace Q {
    export const Other = 0;
    function Hidden() {}
}
namespace Q {
    export const Value = 1;
    export class Class {}
}
import QualifiedValue = Q.Value;
import QualifiedClass = Q.Class;
import HiddenValue = Q.Hidden;
import QAlias = Q;
import ChainedValue = QAlias.Value;
import ChainedClass = QAlias.Class;
export {
    QualifiedValue,
    QualifiedClass,
    HiddenValue,
    ChainedValue,
    ChainedClass,
};
"#;
    let types_source = "export type Shadowed = {};\n";
    let ambiguous_value_source = "export const Ambiguous = 1;\n";
    let ambiguous_type_source = "export type Ambiguous = {};\n";
    let ambiguous_type_first_barrel_source =
        "export * from \"./cold-ambiguous-type\";\nexport * from \"./cold-ambiguous-value\";\n";
    let use_source = r#"
import {
    activate,
    Missing,
    ColdClass,
    ColdInterface,
    ColdEnum,
    Shadowed,
    Ambiguous,
    fV2,
    fV3,
    QualifiedValue,
    QualifiedClass,
    HiddenValue,
    ChainedValue,
    ChainedClass,
} from "./extension";
import { Ambiguous as TypeFirstAmbiguous } from "./cold-type-first-barrel";
import MissingDefault from "./extension";
type ValueFunction = (activate) => void;
type MissingNamed = (Missing) => void;
type ClassType = (ColdClass) => void;
type InterfaceType = (ColdInterface) => void;
type EnumType = (ColdEnum) => void;
type ShadowedStarType = (Shadowed) => void;
type AmbiguousStarType = (Ambiguous) => void;
type TypeFirstAmbiguousStarType = (TypeFirstAmbiguous) => void;
type ExportAliasValue = (fV2) => void;
type TypeExportAliasValue = (fV3) => void;
type QualifiedValueType = (QualifiedValue) => void;
type QualifiedClassType = (QualifiedClass) => void;
type HiddenValueType = (HiddenValue) => void;
type ChainedValueType = (ChainedValue) => void;
type ChainedClassType = (ChainedClass) => void;
type MissingDefaultType = (MissingDefault) => void;
"#;
    std::fs::write(&value_path, value_source).expect("write cold import source");
    std::fs::write(&types_path, types_source).expect("write cold type source");
    std::fs::write(&ambiguous_value_path, ambiguous_value_source)
        .expect("write cold ambiguous value source");
    std::fs::write(&ambiguous_type_path, ambiguous_type_source)
        .expect("write cold ambiguous type source");
    std::fs::write(
        &ambiguous_type_first_barrel_path,
        ambiguous_type_first_barrel_source,
    )
    .expect("write cold type-first barrel");
    std::fs::write(&use_path, use_source).expect("write cold import consumer");

    let use_file = tsc_rs_parser::parse(use_path.to_str().unwrap(), use_source);
    let symbols = tsc_rs_symbols::bind(&use_file);
    let mut checker = TypeChecker::new();
    // Deliberately omit extension.ts: this matches a check-pipe donor whose
    // export maps do not own every otherwise-resolvable project source.
    checker.inject_external_types(&[&use_file]);
    checker.take_diagnostics();
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = checker
        .check_with_options(&use_file, &symbols, &options)
        .diagnostics;
    std::fs::remove_dir_all(&directory).expect("remove cold import fixture directory");

    assert_eq!(
        parameter_implicit_any_slices(use_source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        [
            "activate",
            "Shadowed",
            "Ambiguous",
            "fV2",
            "fV3",
            "QualifiedValue",
            "ChainedValue",
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &use_source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        [
            "Missing",
            "ColdClass",
            "ColdInterface",
            "ColdEnum",
            "TypeFirstAmbiguous",
            "QualifiedClass",
            "HiddenValue",
            "ChainedClass",
            "MissingDefault",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_aliases_preserve_type_and_visibility_evidence() {
    let source = r#"
namespace N {
    function hidden() {}
    export function value() {}
    export class Class {}
    export interface Interface {}
    export namespace R {
        export function deepValue() {}
        export class DeepClass {}
    }
    export import RAlias = R;
}
import Missing = N.hidden;
import Value = N.value;
import ClassAlias = N.Class;
import InterfaceAlias = N.Interface;
import NamespaceAlias = N;
import ChainedValue = NamespaceAlias.value;
import ChainedClass = NamespaceAlias.Class;
import Outer = N.RAlias;
import DeepValue = Outer.deepValue;
import DeepClass = Outer.DeepClass;
namespace Nested {
    export import Alias = N.Class;
    export import ValueAlias = N.value;
    export type A = (Alias) => void;
    export type B = (ValueAlias) => void;
}
type A = (Missing) => void;
type B = (Value) => void;
type C = (ClassAlias) => void;
type D = (InterfaceAlias) => void;
type E = (NamespaceAlias) => void;
type F = (ChainedValue) => void;
type G = (ChainedClass) => void;
type H = (Outer) => void;
type I = (DeepValue) => void;
type J = (DeepClass) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("qualified-import-equals.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        [
            "ValueAlias",
            "Value",
            "NamespaceAlias",
            "ChainedValue",
            "Outer",
            "DeepValue",
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        [
            "Alias",
            "Missing",
            "ClassAlias",
            "InterfaceAlias",
            "ChainedClass",
            "DeepClass",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_cycle_detection_is_declaration_scoped() {
    let source = r#"
namespace Q {
    export namespace R {
        export function Value() {}
        export class Class {}
    }
    export import A = R;
}
import A = Q.A;
import V = A.Value;
import C = A.Class;
type T0 = (A) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("same-name-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["A", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_resolves_enclosing_namespace_scope() {
    let source = r#"
namespace R {
    export function Value() {}
    export class Class {}
}
namespace Q {
    export import A = R;
}
import Outer = Q.A;
import V = Outer.Value;
import C = Outer.Class;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("enclosing-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_resolves_merged_namespace_siblings() {
    let source = r#"
namespace Q {
    export namespace R {
        export function Value() {}
        export class Class {}
    }
}
namespace Q {
    export import A = R;
}
import Outer = Q.A;
import V = Outer.Value;
import C = Outer.Class;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_resolves_dotted_merged_namespace_siblings() {
    let source = r#"
namespace Q.R {
    export function Value() {}
    export class Class {}
}
namespace Q {
    export import A = R;
}
import Outer = Q.A;
import V = Outer.Value;
import C = Outer.Class;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("dotted-merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_aggregates_dotted_namespace_fragments() {
    let source = r#"
namespace Q.R {
    export function Other() {}
}
namespace Q.R {
    export function Value() {}
    export class Class {}
}
namespace Q {
    export import A = R;
}
import Outer = Q.A;
import V = Outer.Value;
import C = Outer.Class;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("multi-dotted-merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_aggregates_local_and_peer_fragments() {
    let source = r#"
namespace Q {
    export namespace R {}
    export import A = R;
}
namespace Q {
    export namespace R {
        export function V() {}
        export class C {}
    }
}
import Outer = Q.A;
import V = Outer.V;
import C = Outer.C;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("local-peer-merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_keeps_private_local_fragments_separate() {
    let source = r#"
namespace Q {
    namespace R {}
    export import A = R;
}
namespace Q {
    export namespace R {
        export function V() {}
        export class C {}
    }
}
import Outer = Q.A;
import V = Outer.V;
import C = Outer.C;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("private-local-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["V", "C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_propagates_recursive_merged_context() {
    let source = r#"
namespace P {
    export namespace Q {
        export namespace R {}
        export import A = R;
    }
}
namespace P {
    export namespace Q {
        export namespace R {
            export function V() {}
            export class C {}
        }
    }
}
import Outer = P.Q.A;
import V = Outer.V;
import C = Outer.C;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("recursive-merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_excludes_private_recursive_peers() {
    let source = r#"
namespace P {
    export namespace Q {
        export namespace R {}
        export import A = R;
    }
}
namespace P {
    namespace Q {
        export namespace R {
            export function V() {}
            export class C {}
        }
    }
}
import Outer = P.Q.A;
import V = Outer.V;
import C = Outer.C;
type T0 = (Outer) => void;
type T1 = (V) => void;
type T2 = (C) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check(
        "private-recursive-merged-qualified-alias.ts",
        source,
        options,
    );

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["Outer"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["V", "C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_drops_private_non_namespace_type_evidence() {
    let source = r#"
namespace Q {
    class R {}
    export import A = R;
}
namespace Q {
    export namespace R {
        export function V() {}
    }
}
import O = Q.A;
import V = O.V;
type T0 = (O) => void;
type T1 = (V) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("private-class-merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["O", "V"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 7051),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_preserves_exported_class_merge_evidence() {
    let source = r#"
namespace Q {
    export class R {}
    export import A = R;
}
namespace Q {
    export namespace R {
        export function V() {}
    }
}
import O = Q.A;
import V = O.V;
type T0 = (O) => void;
type T1 = (V) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("exported-class-merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["V"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["O"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_preserves_exported_type_only_evidence() {
    let source = r#"
namespace Q {
    export interface R {}
    export type S = {};
    export import A = R;
    export import B = S;
}
namespace Q {
    export namespace R {
        export function RV() {}
    }
    export namespace S {
        export function SV() {}
    }
}
import O = Q.A;
import P = Q.B;
type T0 = (O) => void;
type T1 = (P) => void;
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("exported-type-merged-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics),
        [],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["O", "P"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_prefers_local_namespace_targets() {
    let source = r#"
namespace Target {
    export function M() {}
}
namespace Outer {
    namespace Target {
        export class M {}
    }
    import A = Target.M;
    type X = (A) => void;
}
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("shadowed-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics),
        [],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["A"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_uses_immediate_enclosing_namespace_scope() {
    let source = r#"
namespace Target {
    export function M() {}
}
namespace Outer {
    namespace Target {
        export class M {}
    }
    namespace Inner {
        import B = Target.M;
        type X = (B) => void;
    }
}
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("enclosing-shadowed-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics),
        [],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["B"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_does_not_bypass_nearer_missing_members() {
    let source = r#"
namespace Target {
    export function M() {}
}
namespace Outer {
    namespace Target {
        export const Other = 0;
    }
    import A = Target.M;
    type Probe = (A) => void;
}
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("nearer-missing-qualified-member.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics),
        [],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["A"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_does_not_bypass_nearer_enum_heads() {
    let source = r#"
namespace Target {
    export function M() {}
}
namespace Outer {
    enum Target { X }
    import A = Target.M;
    type Probe = (A) => void;
}
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("nearer-enum-qualified-head.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics),
        [],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["A"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_skips_nearer_non_namespace_heads() {
    let source = r#"
namespace Target {
    export function M() {}
}
namespace Outer {
    function Target() {}
    import A = Target.M;
    type Probe = (A) => void;
}
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("nearer-function-qualified-head.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["A"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 7051),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_descends_dotted_namespace_scopes() {
    let source = r#"
namespace Root {
    export class C {}
    export function f() {}
}
namespace One.Two.Three {
    import C = Root.C;
    import F = Root.f;
    type X = (C) => void;
    type Y = (F) => void;
}
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("dotted-namespace-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["F"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_import_equals_resolves_intermediate_dotted_namespace_scopes() {
    let source = r#"
namespace One.Two {
    export namespace Target {
        export function f() {}
        export class C {}
    }
}
namespace One.Two.Three {
    import F = Target.f;
    import C = Target.C;
    type X = (F) => void;
    type Y = (C) => void;
}
"#;
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);
    let diagnostics = check("intermediate-dotted-qualified-alias.ts", source, options);

    assert_eq!(
        parameter_implicit_any_slices(source, &diagnostics)
            .into_iter()
            .map(|(_, _, _, slice)| slice)
            .collect::<Vec<_>>(),
        ["F"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7051)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7051 should carry a span");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["C"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_implicit_any_in_nested_function_and_constructor_types() {
    let mut options = CompilerOptions::default();
    options.no_implicit_any = Some(true);

    let parameter_list_5 = "function A(): (public B) => C {\n}\n";
    let diagnostics = check("ParameterList5.ts", parameter_list_5, options.clone());
    assert_eq!(
        implicit_any_slices(parameter_list_5, &diagnostics),
        [(
            15,
            "Parameter 'B' implicitly has an 'any' type.",
            "public B",
        )],
        "diagnostics: {diagnostics:?}"
    );

    let parameter_list_6 = "class C {\n  constructor(C: (public A) => any) {\n  }\n}\n";
    let diagnostics = check("ParameterList6.ts", parameter_list_6, options.clone());
    assert_eq!(
        implicit_any_slices(parameter_list_6, &diagnostics),
        [(
            28,
            "Parameter 'A' implicitly has an 'any' type.",
            "public A",
        )],
        "diagnostics: {diagnostics:?}"
    );

    let constructor_types =
        "type Abstract = abstract new (value) => object;\ntype Concrete = new (item) => object;";
    let diagnostics = check("constructors.ts", constructor_types, options);
    assert_eq!(
        implicit_any_slices(constructor_types, &diagnostics)
            .into_iter()
            .map(|(_, message, slice)| (message, slice))
            .collect::<Vec<_>>(),
        [
            ("Parameter 'value' implicitly has an 'any' type.", "value"),
            ("Parameter 'item' implicitly has an 'any' type.", "item"),
        ],
        "diagnostics: {diagnostics:?}"
    );

    let nested_containers = r#"
type Tuple = [(tupleValue) => void];
type Reference = Box<(referenceValue) => void>;
type Conditional<T> = T extends (checkValue) => void
    ? (trueValue) => void
    : (falseValue) => void;
type Indexed = { value: (objectValue) => void }["value"];
type Member = { method(): (returnValue) => void };
type Generic<X extends (constraintValue) => void = (defaultValue) => void> = X;
type Rest = (...restValues) => void;
type Destructured = ({ destructuredValue }) => void;
type NestedDestructured = ({ a: { nestedValue } }) => void;
type ArrayRest = ([...arrayRest]) => void;
type ObjectRest = ({ ...objectRest }) => void;
type OuterRest = (...[outerRestValue]) => void;
namespace A.B { type Dotted = (namespaceValue) => void; }
const outer = (cb: (expressionValue) => void) => {};
const asserted = null as ((assertionValue) => void);
class Container {
    field = null as unknown as ((classValue) => void);
}
const { a = null as unknown as ((patternValue) => void) } = {};
declare function dec(x: any): any;
class Decorated {
    method(@dec(null as ((decoratorValue) => void)) p: any) {}
}
"#;
    let diagnostics = check("nested_containers.ts", nested_containers, {
        let mut options = CompilerOptions::default();
        options.no_implicit_any = Some(true);
        options.experimental_decorators = Some(true);
        options
    });
    assert_eq!(
        implicit_any_slices(nested_containers, &diagnostics)
            .into_iter()
            .map(|(_, _, slice)| slice)
            .collect::<Vec<_>>(),
        [
            "tupleValue",
            "referenceValue",
            "checkValue",
            "trueValue",
            "falseValue",
            "objectValue",
            "returnValue",
            "constraintValue",
            "defaultValue",
            "namespaceValue",
            "expressionValue",
            "assertionValue",
            "classValue",
            "patternValue",
            "decoratorValue",
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7019)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7019 should carry a span");
                &nested_containers[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["...restValues"],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 7031)
            .map(|diagnostic| {
                let span = diagnostic.span.expect("TS7031 should carry a span");
                &nested_containers[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["destructuredValue", "nestedValue", "outerRestValue"],
        "diagnostics: {diagnostics:?}"
    );
}
