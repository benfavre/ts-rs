// Diagnostic expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
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
