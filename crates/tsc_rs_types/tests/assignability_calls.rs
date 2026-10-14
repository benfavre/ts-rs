// Diagnostic expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{
    CompilerOptions, Diagnostic, DiagnosticCategory, ModuleKind, RelatedDiagnostic, ScriptTarget,
    Span,
};
use tsc_rs_types::TypeChecker;

fn check(source: &str, expected: &[(u32, &str, u32, u32)]) {
    let file = tsc_rs_parser::parse("conditional.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let output = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = output
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.code,
                diagnostic.message.as_str(),
                span.start,
                span.end - span.start,
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn object_constraint_preserves_both_array_outcomes() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends object>(x: IsArray<T>) {
    let onlyTrue: true = x;
    let onlyFalse: false = x;
    let either: boolean = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                108,
                8,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                136,
                9,
            ),
        ],
    );
}

#[test]
fn array_constraint_selects_true_branch() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends unknown[]>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}
function g<T extends string[]>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}"###,
        &[
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'false'.
  Type 'true' is not assignable to type 'false'."###,
                134,
                2,
            ),
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'false'.
  Type 'true' is not assignable to type 'false'."###,
                230,
                2,
            ),
        ],
    );
}

#[test]
fn disjoint_constraint_selects_false_branch() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends string>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}
function g<T extends Function>(x: IsArray<T>) {
    let yes: true = x;
    let no: false = x;
}"###,
        &[
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'true'.
  Type 'false' is not assignable to type 'true'."###,
                108,
                3,
            ),
            (
                2322,
                r###"Type 'IsArray<T>' is not assignable to type 'true'.
  Type 'false' is not assignable to type 'true'."###,
                204,
                3,
            ),
        ],
    );
}

#[test]
fn partial_union_overlap_keeps_true_branch() {
    check(
        r###"type Matches<T> = T extends "abc" | 42 ? true : false;
function f<T extends string>(x: Matches<T>) {
    let no: false = x;
    let yes: true = x;
    let either: boolean = x;
}
function g<T extends number | string>(x: Matches<T>) {
    let no: false = x;
    let either: boolean = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                109,
                2,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                132,
                3,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                241,
                2,
            ),
        ],
    );
}

#[test]
fn nested_conditions_respect_non_nullable_constraint() {
    check(
        r###"type Zero<T> = T extends null ? null : T extends undefined ? undefined : T extends string ? "" : T extends number ? 0 : T extends boolean ? false : never;
function f<T extends {}>(x: Zero<T>) {
    let zero: "" | 0 | false = x;
}"###,
        &[],
    );
}

#[test]
fn deferred_branches_and_targets_remain_supported() {
    check(
        r###"type Unwrap<T> = T extends (infer U)[] ? U : T;
type Deferred<T, U> = T extends U ? true : false;
function f<T, U>(x: Deferred<T, U>, y: Unwrap<T>, z: T extends string ? T : never) {
    let same: Deferred<T, U> = x;
    let value: Unwrap<T> = y;
    let original: T = z;
}
function g<T>(x: T extends string ? string : number) {
    let primitive: string | number = x;
}"###,
        &[],
    );
}

#[test]
fn unconstrained_and_never_constraints_keep_default_union() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<T>(x: IsArray<T>) {
    let no: false = x;
    let either: boolean = x;
}
function g<T extends never>(x: IsArray<T>) {
    let no: false = x;
    let either: boolean = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                93,
                2,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'false'."###,
                192,
                2,
            ),
        ],
    );
}

#[test]
fn primitive_branches_can_flow_to_union_destinations() {
    check(
        r###"type Select<T> = T extends string ? "text" : 42;
function f<T extends object>(x: Select<T>) {
    let yes: 42 = x;
    let either: "text" | 42 = x;
}
function g<T extends string | number>(x: Select<T>) {
    let either: "text" | 42 = x;
}"###,
        &[],
    );
}

#[test]
fn any_and_unknown_extends_respect_explicit_constraint() {
    check(
        r###"type Always<T> = T extends unknown ? true : false;
type AnyTest<T> = T extends any ? true : false;
function f<T>(x: Always<T>, y: AnyTest<T>) {
    let yes: true = x;
    let alsoYes: true = y;
}
function g<T extends unknown>(x: Always<T>) {
    let yes: true = x;
}"###,
        &[
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                152,
                3,
            ),
            (
                2322,
                r###"Type 'boolean' is not assignable to type 'true'."###,
                175,
                7,
            ),
        ],
    );
}

#[test]
fn unnamed_conditional_displays_boolean() {
    check(
        r###"function f<T>(x: T extends string ? true : false) { let y: true = x; }"###,
        &[(
            2322,
            r###"Type 'boolean' is not assignable to type 'true'."###,
            56,
            1,
        )],
    );
}

#[test]
fn primitive_union_constraint_diagnostics() {
    check(
        r###"type Select<T> = T extends string ? "text" : 42;
function f<T>(x: Select<T>) { let y: string = x; }
function g<T extends object>(x: Select<T>) { let y: string = x; }"###,
        &[
            (
                2322,
                r###"Type 'string | number' is not assignable to type 'string'.
  Type 'number' is not assignable to type 'string'."###,
                83,
                1,
            ),
            (
                2322,
                r###"Type 'Select<T>' is not assignable to type 'string'.
  Type 'number' is not assignable to type 'string'."###,
                149,
                1,
            ),
        ],
    );
}

#[test]
fn augmented_function_and_array_relations_remain_supported() {
    check(
        r###"interface Function extends Array<unknown> {}
type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends Function>(x: IsArray<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn shadowed_function_alias_uses_declared_shape() {
    check(
        r###"type Function = string[];
type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends Function>(x: IsArray<T>) { let yes: true = x; }
export {};"###,
        &[],
    );
}

#[test]
fn augmented_array_relations_remain_supported() {
    check(
        r###"interface Array<T> extends Function {}
type IsFunction<T> = T extends Function ? true : false;
function f<T extends unknown[]>(x: IsFunction<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn never_constraint_falls_back_even_against_unknown() {
    check(
        r###"type Always<T> = T extends unknown ? true : false;
function f<T extends never>(x: Always<T>) { let yes: true = x; let no: false = x; }
function g<T extends any>(x: Always<T>) { let yes: true = x; let no: false = x; }
function h<T extends unknown>(x: Always<T>) { let yes: true = x; let no: false = x; }"###,
        &[
            (2322, "Type 'boolean' is not assignable to type 'true'.", 99, 3),
            (2322, "Type 'boolean' is not assignable to type 'false'.", 118, 2),
            (2322, "Type 'Always<T>' is not assignable to type 'false'.\n  Type 'true' is not assignable to type 'false'.", 200, 2),
            (2322, "Type 'Always<T>' is not assignable to type 'false'.\n  Type 'true' is not assignable to type 'false'.", 286, 2),
        ],
    );
}

#[test]
fn inherited_builtin_constraints_are_not_treated_as_empty_objects() {
    check(
        r###"class MyArray extends Array<unknown> {}
interface ArrayInterface extends Array<unknown> {}
type IsArray<T> = T extends unknown[] ? true : false;
function f<T extends MyArray>(x: IsArray<T>) { let yes: true = x; }
function g<T extends ArrayInterface>(x: IsArray<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn any_and_unknown_constraints_do_not_overlap_never() {
    check(
        r###"type IsNever<T> = T extends never ? true : false;
function f<T extends any>(x: IsNever<T>) { let no: false = x; }
function g<T extends unknown>(x: IsNever<T>) { let no: false = x; }"###,
        &[],
    );
}

#[test]
fn nested_conditionals_preserve_outer_branch_correlation() {
    check(
        r###"type Choice<T> = T extends string ? (T extends string ? true : false) : true;
type Other<T> = T extends string ? (T extends number ? false : true) : true;
function f<T extends string | number>(x: Choice<T>) { let yes: true = x; }
function g<T extends string | number>(x: Other<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn type_parameter_named_function_is_not_the_global_function() {
    check(
        r###"type IsArray<T> = T extends unknown[] ? true : false;
function f<Function extends unknown[], T extends Function>(x: IsArray<T>) { let yes: true = x; }"###,
        &[],
    );
}

#[test]
fn array_alias_is_not_the_global_array() {
    check(
        r###"type Array<T> = () => void;
type IsFunction<T> = T extends Function ? true : false;
function f<T extends Array<number>>(x: IsFunction<T>) { let yes: true = x; }
export {};"###,
        &[],
    );
}

#[test]
fn array_class_is_not_the_global_array() {
    check(
        r###"class Array<T> extends Function {}
type IsFunction<T> = T extends Function ? true : false;
function f<T extends Array<number>>(x: IsFunction<T>) { let yes: true = x; }
export {};"###,
        &[],
    );
}

fn index_assignment_diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("probe.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let output = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    );
    output.diagnostics
}

fn check_index_assignment(source: &str, expected: &[(u32, u32, u32, &str)]) {
    let diagnostics = index_assignment_diagnostics(source);
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|d: &Diagnostic| {
            let span = d.span.unwrap();
            (d.code, span.start, span.end, d.message.as_str())
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

// Expected diagnostics were checked against TypeScript 6.0.3 with strict ES2015.

#[test]
fn index_assignment_shadowed_class_name() {
    let source = r###"declare class Shadow { [key:string]:number; }
function f<Shadow extends {[key:string]:string}>(source:Shadow) {
  let values:{[key:string]:string}=source;
}"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_noncanonical_numeric_properties() {
    let source = r###"declare let source:{'01':string; '-0':boolean; 0:number; '1':number};
let target:{[key:number]:number}=source;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_finite_mapped_index_control() {
    let source = r###"interface Named { value:number; }
declare let named:Named;
let target:Record<'value', number>=named;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_disjoint_index_domains() {
    let source = r###"declare let strings:{[key:string]:string};
declare let symbols:{[key:symbol]:number};
strings=symbols;
symbols=strings;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_optional_explicit_undefined() {
    let source = r###"declare let source:{value?:number|undefined};
let target:{[key:string]:number}=source;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_class_numeric() {
    let source = r###"declare class Base { foo: string; }
declare class Derived extends Base { bar: string; }
declare class Numeric { [index: number]: Base; }
declare let base: Numeric;
declare let derived: { [index: number]: Derived };
base = derived;
derived = base;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            231,
            238,
            r###"Type 'Numeric' is not assignable to type '{ [index: number]: Derived; }'.
  'number' index signatures are incompatible.
    Property 'bar' is missing in type 'Base' but required in type 'Derived'."###,
        )],
    );
}

#[test]
fn index_assignment_interface_string() {
    let source = r###"interface Base { foo: string; }
interface Derived extends Base { bar: string; }
interface Values { [index: string]: Base; }
declare let base: Values;
declare let derived: { [index: string]: Derived };
base = derived;
derived = base;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            217,
            224,
            r###"Type 'Values' is not assignable to type '{ [index: string]: Derived; }'.
  'string' index signatures are incompatible.
    Property 'bar' is missing in type 'Base' but required in type 'Derived'."###,
        )],
    );
}

#[test]
fn index_assignment_inherited_generic_class() {
    let source = r###"interface Base { foo: string; }
interface Derived extends Base { bar: string; }
declare class Values<T> { [index: string]: T; }
declare class Child<T> extends Values<T> {}
declare let base: Child<Base>;
declare let derived: Child<Derived>;
base = derived;
derived = base;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            256,
            263,
            r###"Type 'Child<Base>' is not assignable to type 'Child<Derived>'.
  Property 'bar' is missing in type 'Base' but required in type 'Derived'."###,
        )],
    );
}

#[test]
fn index_assignment_inherited_generic_interface() {
    let source = r###"interface Base { foo: string; }
interface Derived extends Base { bar: string; }
interface Values<T> { [index: number]: T; }
interface Child<T> extends Values<T> {}
declare let base: Child<Base>;
declare let derived: Child<Derived>;
base = derived;
derived = base;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            248,
            255,
            r###"Type 'Child<Base>' is not assignable to type 'Child<Derived>'.
  Property 'bar' is missing in type 'Base' but required in type 'Derived'."###,
        )],
    );
}

#[test]
fn index_assignment_generic_index_values() {
    let source = r###"interface Base { foo: string; }
interface Derived extends Base { bar: string; }
interface Values<T> { [index: number]: T; }
function f<T extends Base>(source: Values<T>, base: Values<Base>, derived: Values<Derived>) {
  base = source;
  source = base;
  source = derived;
  derived = source;
}"###;
    check_index_assignment(
        source,
        &[
            (
                2322,
                237,
                243,
                r###"Type 'Values<Base>' is not assignable to type 'Values<T>'.
  Type 'Base' is not assignable to type 'T'.
    'Base' is assignable to the constraint of type 'T', but 'T' could be instantiated with a different subtype of constraint 'Base'."###,
            ),
            (
                2322,
                254,
                260,
                r###"Type 'Values<Derived>' is not assignable to type 'Values<T>'.
  Type 'Derived' is not assignable to type 'T'.
    'Derived' is assignable to the constraint of type 'T', but 'T' could be instantiated with a different subtype of constraint 'Base'."###,
            ),
            (
                2322,
                274,
                281,
                r###"Type 'Values<T>' is not assignable to type 'Values<Derived>'.
  Type 'T' is not assignable to type 'Derived'.
    Property 'bar' is missing in type 'Base' but required in type 'Derived'."###,
            ),
        ],
    );
}

#[test]
fn index_assignment_generic_chained_constraints() {
    let source = r###"interface Base { foo: string; }
interface Derived extends Base { bar: string; }
function f<T extends Base, U extends T>(source: { [key: string]: U }, base: { [key: string]: Base }, derived: { [key: string]: Derived }) {
  base = source;
  derived = source;
}"###;
    check_index_assignment(
        source,
        &[(
            2322,
            239,
            246,
            r###"Type '{ [key: string]: U; }' is not assignable to type '{ [key: string]: Derived; }'.
  'string' index signatures are incompatible.
    Type 'U' is not assignable to type 'Derived'.
      Type 'T' is not assignable to type 'Derived'.
        Property 'bar' is missing in type 'Base' but required in type 'Derived'."###,
        )],
    );
}

#[test]
fn index_assignment_key_domains() {
    let source = r###"declare let strings: { [key: string]: number };
declare let numbers: { [key: number]: number };
declare let wrong: { [key: number]: string };
strings = numbers;
strings = wrong;
numbers = strings;
numbers = wrong;
wrong = strings;"###;
    check_index_assignment(
        source,
        &[
            (
                2322,
                161,
                168,
                r###"Type '{ [key: number]: string; }' is not assignable to type '{ [key: string]: number; }'.
  'number' and 'string' index signatures are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            ),
            (
                2322,
                197,
                204,
                r###"Type '{ [key: number]: string; }' is not assignable to type '{ [key: number]: number; }'.
  'number' index signatures are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            ),
            (
                2322,
                214,
                219,
                r###"Type '{ [key: string]: number; }' is not assignable to type '{ [key: number]: string; }'.
  'string' and 'number' index signatures are incompatible.
    Type 'number' is not assignable to type 'string'."###,
            ),
        ],
    );
}

#[test]
fn index_assignment_class_key_domains() {
    let source = r###"declare class Numeric { [key: number]: number; name: string; }
declare let source: Numeric;
let ok: { [key: number]: number } = source;
let bad: { [key: string]: number } = source;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            140,
            143,
            r###"Type 'Numeric' is not assignable to type '{ [key: string]: number; }'.
  Index signature for type 'string' is missing in type 'Numeric'."###,
        )],
    );
}

#[test]
fn index_assignment_readonly_index() {
    let source = r###"declare class Values { readonly [key: string]: number; }
declare let readonlyValues: Values;
declare let mutableValues: { [key: string]: number };
mutableValues = readonlyValues;
readonlyValues = mutableValues;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_optional_properties() {
    let source = r###"declare let optional: { value?: number; 0?: number };
declare let explicit: { value: number | undefined };
let stringIndex: { [key: string]: number } = optional;
let numberIndex: { [key: number]: number } = optional;
stringIndex = explicit;"###;
    check_index_assignment(
        source,
        &[
            (
                2322,
                166,
                177,
                r###"Type '{ value?: number | undefined; 0?: number | undefined; }' is not assignable to type '{ [key: number]: number; }'.
  Property '0' is incompatible with index signature.
    Type 'number | undefined' is not assignable to type 'number'.
      Type 'undefined' is not assignable to type 'number'."###,
            ),
            (
                2322,
                217,
                228,
                r###"Type '{ value: number | undefined; }' is not assignable to type '{ [key: string]: number; }'.
  Property 'value' is incompatible with index signature.
    Type 'number | undefined' is not assignable to type 'number'.
      Type 'undefined' is not assignable to type 'number'."###,
            ),
        ],
    );
}

#[test]
fn index_assignment_numeric_ignores_non_numeric_properties() {
    let source = r###"declare let source: { name: string; readonly 0: number; 1?: number };
let exact: { [key: number]: number | undefined } = source;
let error: { [key: number]: string } = source;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            133,
            138,
            r###"Type '{ name: string; readonly 0: number; 1?: number | undefined; }' is not assignable to type '{ [key: number]: string; }'.
  Property '0' is incompatible with index signature.
    Type 'number' is not assignable to type 'string'."###,
        )],
    );
}

#[test]
fn index_assignment_missing_index() {
    let source = r###"interface Named { value: number; }
declare class ClassNamed { value: number; }
type Alias = { value: number };
declare let named: Named;
declare let instance: ClassNamed;
declare let alias: Alias;
let target: { [key: string]: number } = alias;
target = named;
target = instance;
let arbitrary: { [key: string]: any } = named;
let unknownValues: { [key: string]: unknown } = named;"###;
    check_index_assignment(
        source,
        &[
            (
                2322,
                244,
                250,
                r###"Type 'Named' is not assignable to type '{ [key: string]: number; }'.
  Index signature for type 'string' is missing in type 'Named'."###,
            ),
            (
                2322,
                260,
                266,
                r###"Type 'ClassNamed' is not assignable to type '{ [key: string]: number; }'.
  Index signature for type 'string' is missing in type 'ClassNamed'."###,
            ),
            (
                2322,
                330,
                343,
                r###"Type 'Named' is not assignable to type '{ [key: string]: unknown; }'.
  Index signature for type 'string' is missing in type 'Named'."###,
            ),
        ],
    );
}

#[test]
fn index_assignment_required_named_property() {
    let source = r###"declare class Values { [key: string]: number; }
declare let source: Values;
interface RequiredValues { required: number; [key: string]: number; }
let target: RequiredValues = source;"###;
    check_index_assignment(
        source,
        &[(
            2741,
            150,
            156,
            r###"Property 'required' is missing in type 'Values' but required in type 'RequiredValues'."###,
        )],
    );
}

#[test]
fn index_assignment_multiple_index_signatures() {
    let source = r###"declare class Values { [key: string]: number | string; [key: number]: number; }
declare let source: Values;
let n: { [key: number]: number } = source;
let s: { [key: string]: number | string } = source;
let bad: { [key: string]: number } = source;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            207,
            210,
            r###"Type 'Values' is not assignable to type '{ [key: string]: number; }'.
  'string' index signatures are incompatible.
    Type 'string | number' is not assignable to type 'number'.
      Type 'string' is not assignable to type 'number'."###,
        )],
    );
}

#[test]
fn index_assignment_static_index_excluded() {
    let source = r###"declare class Static { static [key: string]: number; }
declare let instance: Static;
let target: { [key: string]: number } = instance;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            89,
            95,
            r###"Type 'Static' is not assignable to type '{ [key: string]: number; }'.
  Index signature for type 'string' is missing in type 'Static'."###,
        )],
    );
}

// Compound-target elaboration and quoted symbol-like member spelling
// still have pre-existing formatting gaps. These controls preserve the
// TypeScript diagnostic codes and source spans independently of those gaps.
fn check_index_assignment_spans(source: &str, expected: &[(u32, u32, u32)]) {
    let actual: Vec<_> = index_assignment_diagnostics(source)
        .into_iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (diagnostic.code, span.start, span.end)
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn index_assignment_union_named_index() {
    let source = r###"interface Source {[key:string]:number}
declare let source:Source;
let target:{[key:string]:string}|{[key:symbol]:string}=source;"###;
    check_index_assignment_spans(source, &[(2322, 70, 76)]);
}

#[test]
fn index_assignment_union_alias_target() {
    let source = r###"interface Source {[key:string]:number}
type Target = {[key:string]:string}|{[key:symbol]:string};
declare let source:Source;
let target:Target=source;"###;
    check_index_assignment_spans(source, &[(2322, 129, 135)]);
}

#[test]
fn index_assignment_union_source_and_target() {
    let source = r###"interface Source {[key:string]:number}
interface Other {[key:symbol]:string}
declare let source:Source|Other;
let target:{[key:string]:string}|{[key:symbol]:string}=source;"###;
    check_index_assignment_spans(source, &[(2322, 114, 120)]);
}

#[test]
fn index_assignment_intersection_named_index() {
    let source = r###"interface Source {[key:string]:number}
declare let source:Source;
let target:{[key:string]:number}&{[key:symbol]:string}=source;"###;
    check_index_assignment_spans(source, &[(2322, 70, 76)]);
}

#[test]
fn index_assignment_union_named_index_valid() {
    let source = r###"interface Source {[key:string]:number}
declare let source:Source;
let target:{[key:string]:number}|{[key:symbol]:string}=source;
let loose:{[key:symbol]:string}|{}=source;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_unique_symbol_properties() {
    let source = r###"declare const sym:unique symbol;
type SymbolBag = {readonly [sym]:string};
declare let source:SymbolBag;
let target:{[key:symbol]:number}=source;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            109,
            115,
            r###"Type 'SymbolBag' is not assignable to type '{ [key: symbol]: number; }'.
  Property '[sym]' is incompatible with index signature.
    Type 'string' is not assignable to type 'number'."###,
        )],
    );
}

#[test]
fn index_assignment_optional_unique_symbol_properties() {
    let source = r###"declare const sym:unique symbol;
type SymbolBag = {[sym]?:string};
declare let source:SymbolBag;
let target:{[key:symbol]:number}=source;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            101,
            107,
            r###"Type 'SymbolBag' is not assignable to type '{ [key: symbol]: number; }'.
  Property '[sym]' is incompatible with index signature.
    Type 'string' is not assignable to type 'number'."###,
        )],
    );
}

#[test]
fn index_assignment_optional_symbol_values() {
    let source = r###"declare const sym:unique symbol;
declare let source:{[sym]?:number|undefined};
let target:{[key:symbol]:number}=source;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_well_known_symbol_properties() {
    let source = r###"type SymbolBag = {readonly [Symbol.iterator]:string};
declare let source:SymbolBag;
let target:{[key:symbol]:number}=source;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            88,
            94,
            r###"Type 'SymbolBag' is not assignable to type '{ [key: symbol]: number; }'.
  Property '[Symbol.iterator]' is incompatible with index signature.
    Type 'string' is not assignable to type 'number'."###,
        )],
    );
}

#[test]
fn index_assignment_optional_well_known_symbol_properties() {
    let source = r###"type SymbolBag = {[Symbol.iterator]?:string};
declare let source:SymbolBag;
let target:{[key:symbol]:number}=source;"###;
    check_index_assignment(
        source,
        &[(
            2322,
            80,
            86,
            r###"Type 'SymbolBag' is not assignable to type '{ [key: symbol]: number; }'.
  Property '[Symbol.iterator]' is incompatible with index signature.
    Type 'string' is not assignable to type 'number'."###,
        )],
    );
}

#[test]
fn index_assignment_quoted_unique_looking_property() {
    let source = r###"declare let source:{'[unique:sym]':string};
let target:{[key:symbol]:number}=source;"###;
    check_index_assignment(source, &[]);
}

#[test]
fn index_assignment_quoted_well_known_string_index() {
    let source = r###"type QuotedBag = {"[Symbol.iterator]":string};
declare let source:QuotedBag;
let target:{[key:string]:number}=source;"###;
    check_index_assignment_spans(source, &[(2322, 81, 87)]);
}

#[test]
fn index_assignment_quoted_unique_identity() {
    let source = r###"declare let s:{"[unique:sym\u0000probe.ts\u000014]":string}; let t:{[key:string]:number}=s;"###;
    check_index_assignment_spans(source, &[(2322, 65, 66)]);
}

fn constructor_return_diagnostics(
    source: &str,
    filename: &str,
    mut options: CompilerOptions,
) -> Vec<Diagnostic> {
    options.target = Some(ScriptTarget::ES2015);
    let file = tsc_rs_parser::parse(filename, source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

fn constructor_return_assert_pair(
    source: &str,
    options: CompilerOptions,
    code: u32,
    message: &str,
) {
    let mut actual = constructor_return_diagnostics(source, "probe.ts", options)
        .into_iter()
        .map(|d| {
            (
                d.code,
                d.span.unwrap().start,
                d.span.unwrap().end,
                d.message,
            )
        })
        .collect::<Vec<_>>();
    actual.sort();
    let start = source.find("return").unwrap() as u32;
    let mut expected = vec![
        (code, start, start + 6, message.to_string()),
        (2409, start, start + 6, "Return type of constructor signature must be assignable to the instance type of the class.".to_string()),
    ];
    expected.sort();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn constructor_return_reports_primitive_and_generic_instance_mismatches() {
    for (source, message) in [
        (
            "class X{foo(){}constructor(){return 1;}}",
            "Type 'number' is not assignable to type 'X'.",
        ),
        (
            "class X<T>{constructor(x:T){return x;}}",
            "Type 'T' is not assignable to type 'X<T>'.",
        ),
        (
            "class X{constructor(x:unknown){return x;}}",
            "Type 'unknown' is not assignable to type 'X'.",
        ),
        (
            "declare function f():void;class X{constructor(){return f();}}",
            "Type 'void' is not assignable to type 'X'.",
        ),
    ] {
        constructor_return_assert_pair(
            source,
            CompilerOptions {
                strict: Some(true),
                ..Default::default()
            },
            2322,
            message,
        );
    }
}

#[test]
fn constructor_return_preserves_weak_target_literal_diagnostics() {
    for (value, printed) in [("0", "0"), ("\"\"", "\"\""), ("true", "true")] {
        let source = format!("class X<T>{{foo?:number;constructor(){{return {value};}}}}");
        constructor_return_assert_pair(
            &source,
            CompilerOptions {
                strict: Some(true),
                ..Default::default()
            },
            2559,
            &format!("Type '{printed}' has no properties in common with type 'X<T>'."),
        );
    }
    constructor_return_assert_pair(
        "class B{foo?:number;}class C extends B{constructor(){super();return 0;}}",
        CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
        2559,
        "Type '0' has no properties in common with type 'C'.",
    );
}

#[test]
fn constructor_return_nullish_checks_follow_effective_strict_options() {
    for value in ["null", "undefined"] {
        let source = format!("class X{{foo=1;constructor(){{return {value};}}}}");
        for strict in [Some(true), None] {
            constructor_return_assert_pair(
                &source,
                CompilerOptions {
                    strict,
                    ..Default::default()
                },
                2322,
                &format!("Type '{value}' is not assignable to type 'X'."),
            );
        }
        for options in [
            CompilerOptions {
                strict: Some(false),
                ..Default::default()
            },
            CompilerOptions {
                strict: Some(true),
                strict_null_checks: Some(false),
                ..Default::default()
            },
        ] {
            assert!(constructor_return_diagnostics(&source, "probe.ts", options).is_empty());
        }
    }
}

#[test]
fn constructor_return_accepts_unknown_weak_targets_without_strict_null_checks() {
    let source = "class C{foo?:number;constructor(){return 1 as unknown;}}";
    for options in [
        CompilerOptions {
            strict: Some(false),
            ..Default::default()
        },
        CompilerOptions {
            strict: Some(true),
            strict_null_checks: Some(false),
            ..Default::default()
        },
    ] {
        assert!(constructor_return_diagnostics(source, "probe.ts", options).is_empty());
    }
}

#[test]
fn constructor_return_accepts_compatible_values_and_nested_function_returns() {
    for source in [
        "class X{constructor(){return 1;}}",
        "class X<X>{constructor(){return 1;}}",
        "class X<T extends number>{constructor(x:T){return x;}}",
        "class X{foo=1;constructor(x:any){return x;}}",
        "class X{foo=1;constructor(x:never){return x;}}",
        "class X{foo=1;constructor(){return;}}",
        "class X{foo=1;constructor(){return this;}}",
        "class X{foo=1;constructor(){return {foo:1};}}",
        "class X{foo=1;constructor(){function f(){return \"\";}let g=()=>{return 1;};return this;}}",
        "class X{toFixed?:(n?:number)=>string;constructor(){return 1;}}",
        "class X{toFixed(n?:number){return \"\";}constructor(){return 1;}}",
        "class X{foo=1;constructor(){const o={get x(){return 1;}};}}",
        "class X{foo=1;constructor(){const o:{x:number}={get x(){return 1;}};}}",
    ] {
        assert!(constructor_return_diagnostics(source, "probe.ts", CompilerOptions { strict: Some(true), ..Default::default() }).is_empty(), "{source}");
    }
}

#[test]
fn constructor_return_does_not_confuse_sibling_function_class_names() {
    let source = "function f(){class X{constructor(){return 1;}}return X;}function g(){class X{foo=1;}return X;}";
    assert!(constructor_return_diagnostics(
        source,
        "probe.ts",
        CompilerOptions {
            strict: Some(true),
            ..Default::default()
        }
    )
    .is_empty());
}

#[test]
fn constructor_return_checks_javascript_independently_of_no_implicit_any() {
    let source = "class X { foo = 1; constructor() { return 0; } }";
    // The project/harness chooses whether to check a JS file. This direct
    // checker test exercises the checked-file path for each inference option.
    for no_implicit_any in [Some(true), Some(false), None] {
        let actual = constructor_return_diagnostics(
            source,
            "probe.js",
            CompilerOptions {
                allow_js: Some(true),
                check_js: Some(true),
                strict: Some(true),
                no_implicit_any,
                ..Default::default()
            },
        );
        let mut codes = actual.into_iter().map(|d| d.code).collect::<Vec<_>>();
        codes.sort();
        assert_eq!(codes, vec![2322, 2409]);
    }
}

fn object_assignment_diagnostics(source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    object_assignment_file_diagnostics(source, "probe.ts", options)
}

fn object_assignment_file_diagnostics(
    source: &str,
    filename: &str,
    options: CompilerOptions,
) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(filename, source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

fn object_assignment_options() -> CompilerOptions {
    CompilerOptions {
        strict: Some(true),
        target: Some(ScriptTarget::ES2015),
        module: Some(ModuleKind::ES2015),
        ..Default::default()
    }
}

// Exact diagnostic messages and ranges checked against TypeScript 6.0.3.
#[test]
fn global_object_explicit_overrides_report_library_member_mismatches() {
    for (source, start, end, message) in [
        (
            r#"declare let source:{toString:number}; let target:Object=source;"#,
            42,
            48,
            r#"Type '{ toString: number; }' is not assignable to type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"declare let source:{constructor:number}; let target:Object=source;"#,
            45,
            51,
            r#"Type '{ constructor: number; }' is not assignable to type 'Object'.
  Types of property 'constructor' are incompatible.
    Type 'number' is not assignable to type 'Function'."#,
        ),
        (
            r#"declare let source:{hasOwnProperty:number}; let target:Object=source;"#,
            48,
            54,
            r#"Type '{ hasOwnProperty: number; }' is not assignable to type 'Object'.
  Types of property 'hasOwnProperty' are incompatible.
    Type 'number' is not assignable to type '(v: PropertyKey) => boolean'."#,
        ),
        (
            r#"declare let source:{propertyIsEnumerable:()=>string}; let target:Object=source;"#,
            58,
            64,
            r#"Type '{ propertyIsEnumerable: () => string; }' is not assignable to type 'Object'.
  The types returned by 'propertyIsEnumerable(...)' are incompatible between these types.
    Type 'string' is not assignable to type 'boolean'."#,
        ),
        (
            r#"declare let source:{readonly toString:number};let target:Object=source;"#,
            50,
            56,
            r#"Type '{ readonly toString: number; }' is not assignable to type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"declare let source:{toString():number};let target:Object=source;"#,
            43,
            49,
            r#"Type '{ toString(): number; }' is not assignable to type 'Object'.
  The types returned by 'toString()' are incompatible between these types.
    Type 'number' is not assignable to type 'string'."#,
        ),
        (
            r#"interface Source{toString:number}declare let source:Source;let target:Object=source;"#,
            63,
            69,
            r#"Type 'Source' is not assignable to type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"declare class Source{toString:number}declare let source:Source;let target:Object=source;"#,
            67,
            73,
            r#"Type 'Source' is not assignable to type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"interface Base{toString:number}interface Source extends Base{}declare let source:Source;let target:Object=source;"#,
            92,
            98,
            r#"Type 'Source' is not assignable to type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"interface Source<T>{toString:T}declare let source:Source<number>;let target:Object=source;"#,
            69,
            75,
            r#"Type 'Source<number>' is not assignable to type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"let target:Object={toString:0};"#,
            19,
            27,
            r#"Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"let target:Object={toString(){return 0;}};"#,
            19,
            27,
            r#"Type '() => number' is not assignable to type '() => string'.
  Type 'number' is not assignable to type 'string'."#,
        ),
        (
            r#"declare function f(source:Object):void;f({toString:0});"#,
            42,
            50,
            r#"Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"declare function f(source:Object):void;let source={toString:0};f(source);"#,
            65,
            71,
            r#"Argument of type '{ toString: number; }' is not assignable to parameter of type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"function f():Object{return {toString:0};}"#,
            28,
            36,
            r#"Type 'number' is not assignable to type '() => string'."#,
        ),
        (
            r#"declare let source:{toString?:number}; let target:Object=source;"#,
            43,
            49,
            r#"Type '{ toString?: number | undefined; }' is not assignable to type 'Object'.
  Types of property 'toString' are incompatible.
    Type 'number | undefined' is not assignable to type '() => string'.
      Type 'undefined' is not assignable to type '() => string'."#,
        ),
    ] {
        let diagnostics = object_assignment_diagnostics(source, object_assignment_options());
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(
            diagnostics[0].code,
            if source.contains("f(source)") {
                2345
            } else {
                2322
            }
        );
        assert_eq!(diagnostics[0].span, Some(Span::new(start, end)), "{source}");
        assert_eq!(diagnostics[0].message, message, "{source}");
    }
}

#[test]
fn global_object_preserves_implicit_members_valid_overrides_and_shadowing() {
    for source in [
        r#"declare let source:{toString:()=>string}; let target:Object=source;"#,
        r#"declare let source:{toString:any}; let target:Object=source;"#,
        r#"declare let source:{toString:never}; let target:Object=source;"#,
        r#"declare let source:{valueOf:()=>number}; let target:Object=source;"#,
        r#"declare let source:{constructor:()=>string}; let target:Object=source;"#,
        r#"declare let source:{hasOwnProperty:(v:string)=>boolean}; let target:Object=source;"#,
        r#"let target:Object={toString(x?:number){return "";}};"#,
        r#"let target:Object={extra:0};"#,
        r#"let target:Object={};"#,
        r#"let target:Object=[];"#,
        r#"let target:Object=()=>0;"#,
        r#"let a:Object="";let b:Object=0;let c:Object=true;let d:Object=Symbol();"#,
        r#"declare let source:{toString:()=>string}|number;let target:Object=source;"#,
        r#"declare let source:{toString:number}&{toString:()=>string};let target:Object=source;"#,
        r#"interface Source{toString():string}declare let source:Source;let target:Object=source;"#,
        r#"declare class Source{toString():string}declare let source:Source;let target:Object=source;"#,
        r#"type Object={toString:number};let target:Object={toString:0};export {};"#,
        r#"class Object{toString=0;}let target:Object={toString:0};export {};"#,
        r#"interface Object{toString:number}let target:Object={toString:0};export {};"#,
        r#"function f<Object extends {toString:number}>(source:Object){let target:Object=source;}"#,
        r#"interface Object{custom:number}declare let source:{custom:number};let target:Object=source;"#,
        r#"declare let source:{toString:number};let target:object=source;let empty:{}=source;"#,
        r#"declare let other:any;let target:Object={toString:0,...other};"#,
        r#"declare let other:any;let target:Object={...other,toString:0};"#,
        r#"let target:Object={toString(x=0){return "";}};"#,
        r#"let target:Object={toString(...args:number[]){return "";}};"#,
        r#"let target:Object={toString(this:{x:number}){return "";}};"#,
        r#"let target:Object={toString:function(x?:number){return "";}};"#,
        r#"let target:Object={toString:(x?:number)=>""};"#,
        r#"declare let source:{toString:()=>number}&{toString:()=>string};let target:Object=source;"#,
        r#"declare let source:{toString<T>():T};let target:Object=source;"#,
        r#"declare let source:{toString<T extends string>():T};let target:Object=source;"#,
        r#"declare let source:{toString():number;toString():string};let target:Object=source;"#,
        r#"interface R{valueOf():R}declare let source:R;let target:Object=source;"#,
        r#"declare let source:{toString:(radix?:number)=>string};let target:Object=source;"#,
        r#"export {};declare let source:(...xs:[...number[],number])=>string;let target:()=>string=source;"#,
        r#"export {};declare let source:(...xs:[void])=>string;let target:()=>string=source;"#,
        r#"export {};declare let source:(...xs:[number,void])=>string;let target:(x:number)=>string=source;"#,
        r#"export {};declare let source:(...xs:[number|void])=>string;let target:()=>string=source;"#,
        r#"export {};declare let source:(...xs:[void,void])=>string;let target:()=>string=source;"#,
        r#"export {};type Function=number;let target:Object={constructor:()=>0};"#,
        r#"export {};type PropertyKey=number;declare let source:{hasOwnProperty(v:string):boolean};let target:Object=source;"#,
    ] {
        let diagnostics = object_assignment_diagnostics(source, object_assignment_options());
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    }
}

#[test]
fn contextual_method_arity_retains_required_positions_and_tuple_rest_elements() {
    for (source, start, end) in [
        (r#"let x:{f(x:number):void}={f(a=0,b:number){}};"#, 26, 27),
        (r#"let x:{f():void}={f(...a:[number]){}};"#, 18, 19),
    ] {
        let diagnostics = object_assignment_diagnostics(source, object_assignment_options());
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, 2322, "{source}");
        assert_eq!(diagnostics[0].span, Some(Span::new(start, end)), "{source}");
        assert!(diagnostics[0]
            .message
            .contains("Target signature provides too few arguments."));
    }
}

#[test]
fn contextual_variadic_method_compares_its_required_tuple_suffix() {
    let source =
        "export {};let target:{f(x:number):void}={f(...xs:[number,...number[],number]){}};";
    let diagnostics = object_assignment_diagnostics(source, object_assignment_options());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2322);
    assert_eq!(diagnostics[0].span, Some(Span::new(41, 42)));
    assert!(diagnostics[0].message.contains("Types of parameters"));
}

#[test]
fn unsupported_object_method_parameters_do_not_hide_other_member_errors() {
    let source = "let target:Object={toString(x?:number){return \"\";},valueOf:0};";
    let diagnostics = object_assignment_diagnostics(source, object_assignment_options());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2322);
    let span = diagnostics[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "valueOf");
    assert_eq!(
        diagnostics[0].message,
        "Type 'number' is not assignable to type '() => Object'."
    );
}

#[test]
fn object_member_checks_follow_library_selection_and_nullability() {
    let source = "declare let source:{toString:number}; let target:Object=source;";
    let mut options = object_assignment_options();
    options.lib = vec!["es5".into()];
    let diagnostics = object_assignment_diagnostics(source, options);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2322);
    let mut options = object_assignment_options();
    options.no_lib = Some(true);
    let diagnostics = object_assignment_diagnostics(
        "interface Array<T> {} interface Boolean {} interface CallableFunction {} \
         interface Function {} interface IArguments {} interface NewableFunction {} \
         interface Number {} interface RegExp {} interface String {} \
         interface Object { toString:number } declare let source:Object; let target:Object=source;",
        options,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let mut options = object_assignment_options();
    options.strict = Some(false);
    let diagnostics = object_assignment_diagnostics(
        "declare let source:{toString(x?:number):string};let target:Object=source;",
        options,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn global_object_jsdoc_follows_effective_no_implicit_any() {
    for source in [
        "/** @type {Object} */ var x={valueOf:1};",
        "/** @param {Object} x */ function f(x){} f({valueOf:1});",
        "/** @returns {Object} */ function f(){return {valueOf:1};}",
        "/** @typedef {Object} Alias */ /** @type {Alias} */ var x={valueOf:1};",
    ] {
        for strict in [Some(true), Some(false), None] {
            for no_implicit_any in [Some(true), Some(false), None] {
                let options = CompilerOptions {
                    strict,
                    no_implicit_any,
                    allow_js: Some(true),
                    check_js: Some(true),
                    ..object_assignment_options()
                };
                let diagnostics = object_assignment_file_diagnostics(source, "probe.js", options);
                let codes = diagnostics.iter().map(|d| d.code).collect::<Vec<_>>();
                if no_implicit_any.unwrap_or(strict.unwrap_or(true)) {
                    assert_eq!(
                        codes,
                        vec![2322],
                        "{source}: {strict:?}, {no_implicit_any:?}"
                    );
                    let start = source.find("valueOf").unwrap() as u32;
                    assert_eq!(diagnostics[0].span, Some(Span::new(start, start + 7)));
                } else {
                    assert!(
                        diagnostics.is_empty(),
                        "{source}: {strict:?}, {no_implicit_any:?}: {diagnostics:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn global_object_typescript_override_check_is_independent_of_no_implicit_any() {
    let source = "let x:Object={valueOf:1};";
    for strict in [Some(true), Some(false), None] {
        let options = CompilerOptions {
            strict,
            no_implicit_any: Some(false),
            ..object_assignment_options()
        };
        let diagnostics = object_assignment_diagnostics(source, options);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, 2322);
        assert_eq!(diagnostics[0].span, Some(Span::new(14, 21)));
    }
}

#[test]
fn global_object_jsdoc_preserves_valid_dictionary_and_shadowed_types() {
    for source in [
        "/** @type {object} */ var x={valueOf:1};",
        "/** @type {Object<string,number>} */ var x={valueOf:1};",
        "export {};class Object{valueOf=1;} /** @type {Object} */ var x={valueOf:1};",
        "/** @type {Object} */ var x={valueOf(){return {};}};",
    ] {
        for no_implicit_any in [Some(true), Some(false), None] {
            let options = CompilerOptions {
                no_implicit_any,
                allow_js: Some(true),
                check_js: Some(true),
                ..object_assignment_options()
            };
            let diagnostics = object_assignment_file_diagnostics(source, "probe.js", options);
            assert!(
                diagnostics.is_empty(),
                "{source}: {no_implicit_any:?}: {diagnostics:?}"
            );
        }
    }
}

#[test]
fn global_object_array_errors_point_to_incompatible_property() {
    for (filename, source) in [
        (
            "probe.js",
            "/** @type {Object[]} */ const x=[{toString:1}];",
        ),
        ("probe.ts", "let x:Object[]=[{toString:1}];"),
        ("probe.ts", "const x:Object[][]=[[{toString:1}]];"),
        ("probe.ts", "const x:ReadonlyArray<Object>=[{toString:1}];"),
        ("probe.ts", "const x:readonly Object[]=[{toString:1}];"),
        ("probe.ts", "const x:[Object[]]=[[{toString:1}]];"),
        (
            "probe.ts",
            "const x:{field:Object}[]=[{field:{toString:1}}];",
        ),
        (
            "probe.ts",
            "const x:{field:Object[]}={field:[{toString:1}]};",
        ),
        ("probe.ts", "const x:(Object|string)[]=[{toString:1}];"),
        (
            "probe.ts",
            "declare function f(x:Object[]):void;f([{toString:1}]);",
        ),
        ("probe.ts", "function f():Object[]{return [{toString:1}];}"),
    ] {
        let diagnostics = object_assignment_file_diagnostics(
            source,
            filename,
            CompilerOptions {
                allow_js: Some(true),
                check_js: Some(true),
                ..object_assignment_options()
            },
        );
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, 2322);
        let start = source.find("toString").unwrap() as u32;
        assert_eq!(
            diagnostics[0].span,
            Some(Span::new(start, start + 8)),
            "{source}"
        );
    }
}

#[test]
fn global_object_js_uncertainty_follows_injected_program_and_clones() {
    let js = tsc_rs_parser::parse("library.js", "/** @param {Object} x */ function f(x){}");
    let js = tsc_rs_types::desugar_jsdoc(&js).unwrap();
    let consumer = tsc_rs_parser::parse("consumer.ts", "f({toString:1});");
    let symbols = tsc_rs_symbols::bind(&consumer);
    for consumer_first in [true, false] {
        let files = if consumer_first {
            vec![&consumer, &js]
        } else {
            vec![&js, &consumer]
        };
        let mut donor = TypeChecker::new();
        donor.inject_external_types(&files);
        // Checking a clone must retain the donor's injected JS membership,
        // even without the separate module-resolution registration API.
        let diagnostics = donor
            .clone()
            .check_with_options(
                &consumer,
                &symbols,
                &CompilerOptions {
                    no_implicit_any: Some(false),
                    ..object_assignment_options()
                },
            )
            .diagnostics;
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}

#[test]
fn global_object_checking_js_clone_does_not_change_unrelated_ts_clone() {
    let donor = TypeChecker::new();
    let js = tsc_rs_parser::parse("probe.js", "/** @type {Object} */ var x={valueOf:1};");
    let js_symbols = tsc_rs_symbols::bind(&js);
    let options = CompilerOptions {
        no_implicit_any: Some(false),
        ..object_assignment_options()
    };
    assert!(donor
        .clone()
        .check_with_options(&js, &js_symbols, &options)
        .diagnostics
        .is_empty());
    let ts = tsc_rs_parser::parse("probe.ts", "let x:Object={valueOf:1};");
    let ts_symbols = tsc_rs_symbols::bind(&ts);
    let diagnostics = donor
        .check_with_options(&ts, &ts_symbols, &options)
        .diagnostics;
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2322);
}

#[test]
fn global_object_preserves_shadowed_array_aliases_before_normalization() {
    for name in ["Array", "ReadonlyArray"] {
        let alias = format!("type {name}<T>={{toString:number}}[];");
        let assignment = format!("let x:{name}<Object>=[{{toString:1}}];");
        for source in [
            format!("export {{}};{alias}{assignment}"),
            format!("export {alias}{assignment}"),
            format!("function f(){{{alias}{assignment}}}"),
            format!("{{{alias}{assignment}}}"),
            format!("const f=()=>{{{assignment}{alias}}};"),
            format!("const f=function(){{{alias}{assignment}}};"),
            format!("const o={{f(){{{alias}{assignment}}}}};"),
            format!("namespace N{{export {alias}export {assignment}}}"),
        ] {
            let diagnostics = object_assignment_diagnostics(&source, object_assignment_options());
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        }
    }
}

#[test]
fn global_object_shadowed_array_uncertainty_survives_injection_and_clone() {
    for name in ["Array", "ReadonlyArray"] {
        // The namespace and lexical aliases can have different registry keys;
        // the binder still records their original declaration names.
        for source in [
            format!("namespace N{{export type {name}<T>={{toString:number}}[];}}"),
            format!("import type {{Other as {name}}} from './definition';"),
            format!("const f=()=>{{type {name}<T>={{toString:number}}[];}};"),
        ] {
            let donor_file = tsc_rs_parser::parse("donor.ts", &source);
            let consumer = tsc_rs_parser::parse("consumer.ts", "let x:Object={toString:1};");
            let symbols = tsc_rs_symbols::bind(&consumer);
            let mut donor = TypeChecker::new();
            donor.inject_external_types(&[&consumer, &donor_file]);
            let diagnostics = donor
                .clone()
                .check_with_options(&consumer, &symbols, &object_assignment_options())
                .diagnostics;
            // Until the erased container identity is retained, direct Object
            // assignments in these ambiguous programs remain deferred too.
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        }
    }
}

#[test]
fn global_object_array_shadow_does_not_disable_other_assignment_errors() {
    let source = "export {};type Array<T>={toString:number}[];let x:number='bad';";
    let diagnostics = object_assignment_diagnostics(source, object_assignment_options());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, 2322);
    let start = source.find("x:number").unwrap() as u32;
    assert_eq!(diagnostics[0].span, Some(Span::new(start, start + 1)));
}

#[test]
fn global_object_jsdoc_array_typedefs_use_desugared_binding_metadata() {
    for name in ["Array", "ReadonlyArray"] {
        let source = format!(
            "export {{}};/** @template T @typedef {{{{toString:number}}[]}} {name} */ \
             /** @type {{{name}<Object>}} */ const x=[{{toString:1}}];"
        );
        let diagnostics = object_assignment_file_diagnostics(
            &source,
            "probe.js",
            CompilerOptions {
                allow_js: Some(true),
                check_js: Some(true),
                ..object_assignment_options()
            },
        );
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    }
}

fn overload_arity_diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("probe.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict: Some(true),
                target: Some(ScriptTarget::ES2015),
                ..Default::default()
            },
        )
        .diagnostics
}

#[test]
fn overload_arity_missing_argument_reports_exact_declaration_note() {
    // TypeScript 6.0.3: the first signature with the smallest minimum owns
    // the missing-parameter note, independently of the implementation.
    for source in [
        "declare function f(x:number):void;declare function f(x:string):void;f();",
        "function f(x:string):void;function f(x:number):void;function f(x:any){}f();",
        "declare function f(x:number,y:number,z:number):void;declare function f(x:string):void;f();",
    ] {
        let diagnostics = overload_arity_diagnostics(source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        let call_start = source.rfind("f()").unwrap() as u32;
        let note_text = if source.contains("z:number") { "x:string" } else if source.starts_with("function") { "x:string" } else { "x:number" };
        let parameter_start = source.find(note_text).unwrap() as u32;
        assert_eq!(diagnostics[0], Diagnostic {
            code: 2554,
            message: if source.contains("z:number") { "Expected 1-3 arguments, but got 0." } else { "Expected 1 arguments, but got 0." }.into(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(Span::new(call_start, call_start + 1)),
            related: Some(vec![RelatedDiagnostic {
                code: 6210, message: "An argument for 'x' was not provided.".into(),
                file_name: Some("probe.ts".into()),
                span: Some(Span::new(parameter_start, parameter_start + note_text.len() as u32)),
            }]),
        });
    }
}

#[test]
fn overload_arity_excess_arguments_span_only_the_extra_expressions() {
    let source = "declare function f(x:number):void;declare function f(x:string):void;f(1, /*extra*/ 2,\n 3);";
    let diagnostics = overload_arity_diagnostics(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let first = source.find("2,").unwrap() as u32;
    let last = source.rfind('3').unwrap() as u32 + 1;
    assert_eq!(diagnostics[0].code, 2554);
    assert_eq!(diagnostics[0].message, "Expected 1 arguments, but got 3.");
    assert_eq!(diagnostics[0].span, Some(Span::new(first, last)));
    assert_eq!(diagnostics[0].related, None);
}

#[test]
fn overload_arity_gap_uses_declared_endpoints() {
    for (source, expected) in [
        ("declare function f(x:number):void;declare function f(x:number,y:number,z:number):void;f(1,2);",
         "No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments."),
        ("declare function f(x:number,y?:number):void;declare function f(x:string,y:string,z:string,w:string,q?:string,r?:string):void;f(1,2,3);",
         "No overload expects 3 arguments, but overloads do exist that expect either 1 or 6 arguments."),
    ] {
        let diagnostics = overload_arity_diagnostics(source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, 2575);
        assert_eq!(diagnostics[0].message, expected);
        let start = source.rfind("f(1").unwrap() as u32;
        assert_eq!(diagnostics[0].span, Some(Span::new(start, start + 1)));
        assert_eq!(diagnostics[0].related, None);
    }
}

#[test]
fn overload_arity_excludes_this_and_keeps_binding_pattern_note() {
    for (source, code, message, parameter) in [
        ("declare function f(this:any,x:number):void;declare function f(this:any,x:string):void;f();", 6210, "An argument for 'x' was not provided.", "x:number"),
        ("declare function f({x}:{x:number}):void;declare function f([x]:[number]):void;f();", 6211, "An argument matching this binding pattern was not provided.", "{x}:{x:number}"),
    ] {
        let diagnostics = overload_arity_diagnostics(source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, 2554);
        assert_eq!(diagnostics[0].message, "Expected 1 arguments, but got 0.");
        let start = source.find(parameter).unwrap() as u32;
        assert_eq!(diagnostics[0].related, Some(vec![RelatedDiagnostic {
            code, message: message.into(), file_name: Some("probe.ts".into()),
            span: Some(Span::new(start, start + parameter.len() as u32)),
        }]));
    }
}

#[test]
fn overload_arity_preserves_accepted_calls_and_shadowed_bindings() {
    for source in [
        "declare function f(x:number):number;declare function f(x:string):string;f(1);f('ok');",
        "declare function f(x?:number):void;declare function f(x?:string):void;f();",
        "declare function f(x:number):number;declare function f(x:string):string;{const f=()=>{};f();}",
        "declare function f(x:number):number;declare function f(x:string):string;function outer(f:()=>void){f();}",
        "declare function f<T>(x:T):T;declare function f<T>(x:T,y:T):T;f<number>(1);",
        "String.fromCharCode();",
    ] {
        assert!(overload_arity_diagnostics(source).is_empty(), "{source}");
    }
}

#[test]
fn overload_arity_retains_uniform_exports_and_module_local_library_names() {
    for (source, name) in [
        ("export function f(a:number):void;export function f(a:number,b:number,c:number):void;export function f(...args:any[]){};f(1,2);", "f"),
        ("export {};declare function Date(a:number):number;declare function Date(a:number,b:number,c:number):number;Date(1,2);", "Date"),
    ] {
        let diagnostics = overload_arity_diagnostics(source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        let start = source.rfind(&format!("{name}(1,2)")).unwrap() as u32;
        assert_eq!(diagnostics[0].code, 2575);
        assert_eq!(diagnostics[0].span, Some(Span::new(start, start + name.len() as u32)));
        assert_eq!(diagnostics[0].message, "No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments.");
        assert_eq!(diagnostics[0].related, None);
    }
}
