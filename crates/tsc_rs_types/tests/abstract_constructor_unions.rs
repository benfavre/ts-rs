use std::sync::Arc;
use tsc_rs_ast::CompilerOptions;

use tsc_rs_types::{ConstructorType, FunctionType, Type, TypeChecker};

fn diagnostics(source: &str) -> Vec<tsc_rs_ast::Diagnostic> {
    let file = tsc_rs_parser::parse("abstract_constructor_unions.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn diagnostic_codes(source: &str) -> Vec<u32> {
    let mut codes = diagnostics(source)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();
    codes.sort_unstable();
    codes
}

#[test]
fn unions_containing_abstract_constructors_cannot_be_instantiated() {
    let declarations = r#"
class ConcreteA {}
class ConcreteB {}
abstract class AbstractA { a!: string; }
abstract class AbstractB { b!: string; }

type Abstracts = typeof AbstractA | typeof AbstractB;
type Concretes = typeof ConcreteA | typeof ConcreteB;
type ConcretesOrAbstracts = Concretes | Abstracts;

declare const mixed: ConcretesOrAbstracts;
declare const abstractOnly: Abstracts;
declare const concreteOnly: Concretes;
"#;

    for expression in [
        "new mixed();",
        "new abstractOnly();",
        "[ConcreteA, AbstractA, AbstractB].map(cls => new cls());",
        "[AbstractA, AbstractB, ConcreteA].map(cls => new cls());",
        "[AbstractA, AbstractB].map(cls => new cls());",
    ] {
        let source = format!("{declarations}\n{expression}");
        assert_eq!(
            diagnostic_codes(&source),
            vec![2511],
            "expression: {expression}"
        );
    }
    for expression in [
        "new concreteOnly();",
        "[ConcreteA, ConcreteB].map(cls => new cls());",
    ] {
        let source = format!("{declarations}\n{expression}");
        assert_eq!(
            diagnostic_codes(&source),
            Vec::<u32>::new(),
            "expression: {expression}"
        );
    }
}

#[test]
fn direct_concrete_and_abstract_classes_keep_the_existing_behavior() {
    assert_eq!(
        diagnostic_codes("class Concrete {}\nnew Concrete();"),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes("abstract class Abstract {}\nnew Abstract();"),
        vec![2511]
    );
}

#[test]
fn non_constructable_union_members_do_not_turn_into_abstract_class_errors() {
    for (source, expected) in [
        (
            "abstract class Abstract {}\ndeclare const value: typeof Abstract | (() => void);\nnew value();",
            2351,
        ),
        (
            "abstract class Abstract {}\ndeclare const value: typeof Abstract | unknown;\nnew value();",
            18046,
        ),
    ] {
        assert_eq!(
            diagnostic_codes(source),
            vec![expected],
            "source:\n{source}"
        );
    }
}

#[test]
fn abstract_construct_signatures_and_intersections_preserve_abstractness() {
    for source in [
        "type AC = abstract new () => object;\ndeclare const value: AC;\nnew value();",
        "abstract class Abstract {}\nclass Concrete {}\ndeclare const value: typeof Abstract | (new () => Concrete);\nnew value();",
        "abstract class Abstract {}\nclass Concrete {}\ndeclare const value: typeof Abstract | { new(): Concrete };\nnew value();",
        "abstract class Abstract {}\nclass Concrete {}\ndeclare const value: typeof Abstract & typeof Concrete;\nnew value();",
        "abstract class Abstract {}\nfunction create<T extends typeof Abstract>(Ctor: T) { new Ctor(); }",
    ] {
        assert_eq!(diagnostic_codes(source), vec![2511], "source:\n{source}");
    }
}

#[test]
fn typeof_constructor_resolution_respects_lexical_shadowing() {
    let source = r#"
abstract class A {}
class C {}
function f(A: typeof C) {
    const K: typeof A = A;
    new K();
}
"#;

    assert_eq!(diagnostic_codes(source), Vec::<u32>::new());

    assert_eq!(
        diagnostic_codes(
            "abstract class A {}\nfunction f(A: {}) { const K: typeof A = A; new K(); }"
        ),
        vec![2351],
        "a nonconstructable lexical shadow must not fall back to the class"
    );
}

#[test]
fn concrete_construct_signatures_remain_constructable_and_check_arguments() {
    assert_eq!(
        diagnostic_codes(
            "type Ctor = new (value: string) => { value: string };\ndeclare const C: Ctor;\nnew C(1);"
        ),
        vec![2345]
    );
}

#[test]
fn concrete_constructor_unions_and_constraints_remain_constructable() {
    for source in [
        "class A {}\nclass B {}\ndeclare const C: typeof A | typeof B;\nnew C();",
        "type A = new () => { a: string };\ntype B = new () => { b: string };\ndeclare const C: A | B;\nnew C();",
        "class A {}\nfunction make<T extends new () => A>(C: T) { return new C(); }",
    ] {
        assert_eq!(
            diagnostic_codes(source),
            Vec::<u32>::new(),
            "source:\n{source}"
        );
    }
}

#[test]
fn constructor_unions_preserve_each_members_overload_group() {
    assert_eq!(
        diagnostic_codes(
            r#"
interface A { new(): { a: 1 }; new(value: string): { a: 1 } }
interface B { new(): { b: 1 } }
declare const C: A | B;
new C();
"#,
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn any_absorbs_abstract_constructor_intersections() {
    assert_eq!(
        diagnostic_codes("abstract class A {}\ndeclare const C: typeof A & any;\nnew C();"),
        Vec::<u32>::new()
    );
}

#[test]
fn constructor_signature_assignability_respects_abstractness_arity_and_returns() {
    let source = r#"
class A {}
class B extends A {}
abstract class AbstractA extends A {}

declare let source: new () => B;
let compatible: new () => A = source;
let concreteFromAbstract: new () => A = AbstractA;
let abstractFromConcrete: abstract new () => A = A;

declare let needsArg: new (value: string) => A;
let rejectsRequiredSource: new () => A = needsArg;

declare let restSource: new (...values: string[]) => A;
let rejectsBadRest: new (a: string, b: number) => A = restSource;
"#;
    assert_eq!(diagnostic_codes(source), vec![2322, 2322, 2322]);
}

#[test]
fn constructor_assignability_preserves_optional_rest_and_static_surfaces() {
    let source = r#"
class A { static x = 1; }
declare let bare: new () => A;
let missingStatic: typeof A = bare;

interface WithStatic { new(): A; x: number; }
declare let withStatic: WithStatic;
let dropsExtraStatic: new () => A = withStatic;

declare let required: new (value: string) => A;
let optionalTarget: new (value?: string) => A = required;

class RestSource { constructor(...values: string[]) {} }
class FixedTarget { constructor(a: string, b: number) {} }
let badRest: typeof FixedTarget = RestSource;

class PublicTarget { static value = 1; }
class PrivateSource { private static value = 1; }
let privateAsPublic: typeof PublicTarget = PrivateSource;
"#;
    let diagnostics = diagnostics(source);
    let mut codes = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();
    codes.sort_unstable();
    assert_eq!(
        codes,
        vec![2322, 2322, 2322, 2322],
        "diagnostics: {diagnostics:#?}"
    );
}

#[test]
fn object_constructor_constraints_and_assignment_directions_are_preserved() {
    for source in [
        "type C = { new<T extends string>(x: T): T };\ndeclare const C: C;\nnew C(1);",
        "interface C { new<T extends string>(x: T): T }\ndeclare const C: C;\nnew C(1);",
    ] {
        assert_eq!(diagnostic_codes(source), vec![2345], "source:\n{source}");
    }

    assert_eq!(
        diagnostic_codes(
            "declare let constrained: new<T extends string>(x: T) => T;\ndeclare let broad: new<T>(x: T) => T;\nbroad = constrained;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "declare let constrained: new<T extends string>(x: T) => T;\ndeclare let broad: new<T>(x: T) => T;\nconstrained = broad;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "declare let generic: new<T>(x: T) => T;\ndeclare let concrete: new(x: string) => string;\ngeneric = concrete;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "declare let generic: new<T>(x: T) => T;\ndeclare let concrete: new(x: string) => string;\nconcrete = generic;"
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn tuple_rest_constructors_preserve_arity_and_variance() {
    assert_eq!(
        diagnostic_codes("declare const C: new (...args: [string, number]) => {};\nnew C();"),
        vec![2554]
    );
    assert_eq!(
        diagnostic_codes(
            "declare let source: new (...args: [string, number]) => {};\nlet target: new(a: string, b: number) => {} = source;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes("class C { constructor(...args: [string, number]) {} }\nnew C();"),
        vec![2554]
    );
    assert_eq!(
        diagnostic_codes("class C { constructor(...args: [string, number]) {} }\nnew C('x', 1);"),
        Vec::<u32>::new()
    );
}

#[test]
fn construct_signature_relations_include_signatures_and_static_surfaces() {
    assert_eq!(
        diagnostic_codes(
            "interface S { new(x: string): {} }\ninterface T { new(x: number): {} }\ndeclare let source: S;\nlet target: T = source;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "class A {}\ninterface Need { new(): A; x: number }\nlet target: Need = A;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "class Source { private static x = 1 }\ninterface Target { new(): Source; x: number }\nlet target: Target = Source;"
        ),
        vec![2322]
    );
}

#[test]
fn explicit_constructor_type_arguments_validate_arity_and_constraints() {
    assert_eq!(
        diagnostic_codes("declare const C: new<T>(x: T) => T;\nnew C<string, number>('x');"),
        vec![2558]
    );
    assert_eq!(
        diagnostic_codes("declare const C: new<T extends string>(x: T) => T;\nnew C<number>(1);"),
        vec![2344]
    );
    assert_eq!(
        diagnostic_codes(
            "type A = new<T extends string>(x: T) => { a: T };\ntype B = new<T extends number>(x: T) => { b: T };\ndeclare const C: A | B;\nnew C<string>('x');"
        ),
        vec![2351]
    );
}

#[test]
fn class_defaults_nested_binders_and_class_expressions_remain_precise() {
    assert_eq!(
        diagnostic_codes(
            "class Box<T = string> { value!: T }\nconst value: string = new Box().value;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "type Factory<X> = X extends any ? { new<X>(x: X): X } : never;\ndeclare const C: Factory<string>;\nconst value: number = new C(1);"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes("const C = class { value = 1 };\nconst value: string = new C().value;"),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes("class C<T extends string> { constructor(value: T) {} }\nnew C(1);"),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "class Pair<T, U = string> { value!: U; constructor(first: T) {} }\nconst value: string = new Pair(1).value;"
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn class_expressions_preserve_private_nominal_identity() {
    assert_eq!(
        diagnostic_codes(
            "class A { private x = 1 }\nconst C = class { private x = 1 };\nconst value: A = new C();"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "class A { private static x = 1 }\nconst C = class { private static x = 1 };\nlet value: typeof A = C;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "class C { a = 1 }\nconst X = class C { b = 1 };\nconst value: C = new X();"
        ),
        vec![2741]
    );
    let self_reference_diagnostics = diagnostics(
        "class C { a = 1 }\nconst X = class C {\n  b = 1;\n  make(): C { return new C(); }\n};\nconst value: { b: number } = new X().make();",
    );
    assert!(
        self_reference_diagnostics.is_empty(),
        "{self_reference_diagnostics:#?}"
    );
    let static_self_reference_diagnostics = diagnostics(
        "class C { a = 1 }\nconst X = class C {\n  static self: typeof C = C;\n};\nconst value: typeof X = X.self;",
    );
    assert!(
        static_self_reference_diagnostics.is_empty(),
        "{static_self_reference_diagnostics:#?}"
    );
}

#[test]
fn generic_inference_traverses_construct_signatures_without_capturing_binders() {
    assert_eq!(
        diagnostic_codes(
            "declare function infer<T>(ctor: { new(x: T): any }): T;\ndeclare const S: { new(x: string): any };\nconst got = infer(S);\nconst bad: number = got;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "declare function outer<T>(ctor: new<T>(x: T) => T): T;\ndeclare const C: new<U>(x: U) => U;\nconst got = outer(C);\nconst bad: number = got;"
        ),
        vec![2322]
    );
}

#[test]
fn constructor_assignability_respects_lexical_shadowing_and_private_targets() {
    assert_eq!(
        diagnostic_codes(
            "class A {}\nfunction f(A: {}) { const source: typeof A = A; let target: new () => object = source; }"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "class C { private static x = 1 }\ninterface F { new(): C; prototype: C; x: number }\ndeclare const source: F;\nlet target: typeof C = source;"
        ),
        vec![2322]
    );
}

#[test]
fn qualified_namespace_classes_remain_constructable() {
    assert_eq!(
        diagnostic_codes(
            "declare namespace Foo { export class C {} }\nnew Foo.C();\nnamespace Bar { export class C {} }\nnew Bar.C();"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes("class C {}\nconst object = { C: {} };\nnew object.C();"),
        vec![2351]
    );
    assert_eq!(
        diagnostic_codes(
            "namespace N { export class C {} }\nfunction f(N: { C: {} }) { new N.C(); }"
        ),
        vec![2351]
    );
}

#[test]
fn nonconstructable_objects_report_ts2351() {
    assert_eq!(
        diagnostic_codes("const value = {};\nnew value();\nnew ({})();"),
        vec![2351, 2351]
    );
}

#[test]
fn standard_intl_constructor_recovery_does_not_apply_to_local_shadows() {
    assert_eq!(
        diagnostic_codes("const Intl = { NumberFormat: () => 0 };\nnew Intl.NumberFormat();"),
        vec![2351]
    );
    assert_eq!(
        diagnostic_codes(
            "export {};\nnamespace Intl { export const NumberFormat = () => 0; }\nnew Intl.NumberFormat();"
        ),
        vec![2351]
    );
}

#[test]
fn conditional_constructor_assignability_observes_optional_parameters() {
    assert_eq!(
        diagnostic_codes(
            "type X = (new(x: string) => object) extends (new(x?: string) => object) ? true : false;\nconst value: true = null as any as X;"
        ),
        vec![2322]
    );
}

#[test]
fn generic_constructor_signatures_infer_and_substitute_return_types() {
    let source = r#"
declare const Box: new <T>(value: T) => { value: T };
const boxed = new Box("value");
const good: string = boxed.value;
const bad: number = boxed.value;
declare const Defaulted: new <T = string>() => { value: T };
const defaultGood: string = new Defaulted().value;
const defaultBad: number = new Defaulted().value;
"#;
    assert_eq!(diagnostic_codes(source), vec![2322, 2322]);
}

#[test]
fn generic_constructor_constraints_and_nested_binders_are_preserved() {
    let source = r#"
declare const Constrained: new <T extends string>(value: T) => T;
const constrainedNumber: number = new Constrained(1);

type Outer<T> = new <T>(value: T) => T;
declare const Nested: Outer<string>;
const nested = new Nested(1);
const nestedGood: number = nested;
const nestedBad: string = nested;
"#;
    assert_eq!(diagnostic_codes(source), vec![2322, 2322, 2345]);
}

#[test]
fn indirect_generic_class_construction_infers_and_defaults_type_arguments() {
    let source = r#"
class Box<T = string> {
    value!: T;
    constructor(value?: T) {}
}
declare const BoxCtor: typeof Box;
const inferred = new BoxCtor(1);
const inferredGood: number = inferred.value;
const inferredBad: string = inferred.value;
const defaulted = new BoxCtor();
const defaultGood: string = defaulted.value;
const defaultBad: number = defaulted.value;
"#;
    assert_eq!(diagnostic_codes(source), vec![2322, 2322]);
}

#[test]
fn generic_constructor_inference_contextually_types_later_callbacks() {
    assert_eq!(
        diagnostic_codes(
            r#"
declare const C: new <T>(seed: T, callback: (value: T) => void) => object;
new C("", value => { value = 1; });
"#,
        ),
        vec![2322]
    );
}

#[test]
fn constructor_types_display_with_valid_precedence_and_parameter_syntax() {
    let constructor = Type::Constructor(ConstructorType {
        is_abstract: true,
        params: vec![
            ("?value".into(), Type::TypeParameter("T".into())),
            (
                "...rest".into(),
                Type::Array(Arc::new(Type::TypeParameter("T".into()))),
            ),
        ],
        return_type: Arc::new(Type::TypeParameter("T".into())),
        type_params: vec!["T".into()],
        type_param_constraints: vec![Some(Type::String)],
        type_param_defaults: vec![Some(Type::String)],
    });
    assert_eq!(
        constructor.display_string(),
        "abstract new <T extends string = string>(value?: T, ...rest: T[]) => T"
    );
    assert_eq!(
        Type::Array(Arc::new(constructor.clone())).display_string(),
        "(abstract new <T extends string = string>(value?: T, ...rest: T[]) => T)[]"
    );
    assert_eq!(
        Type::Union(vec![constructor, Type::Undefined].into()).display_string(),
        "(abstract new <T extends string = string>(value?: T, ...rest: T[]) => T) | undefined"
    );

    let object = Type::ObjectType(tsc_rs_types::ObjectTypeInfo {
        properties: Vec::new(),
        call_signatures: Vec::new(),
        construct_signatures: vec![ConstructorType {
            is_abstract: false,
            params: vec![("?value".into(), Type::TypeParameter("T".into()))],
            return_type: Arc::new(Type::TypeParameter("T".into())),
            type_params: vec!["T".into()],
            type_param_constraints: vec![Some(Type::String)],
            type_param_defaults: vec![Some(Type::String)],
        }],
        index_signature: None,
        index_signature_name: None,
        method_names: Vec::new(),
    });
    assert_eq!(
        object.display_string(),
        "new <T extends string = string>(value?: T) => T"
    );

    let returns_function = Type::ObjectType(tsc_rs_types::ObjectTypeInfo {
        properties: Vec::new(),
        call_signatures: Vec::new(),
        construct_signatures: vec![ConstructorType {
            is_abstract: false,
            params: Vec::new(),
            return_type: Arc::new(Type::Function(FunctionType {
                params: Vec::new(),
                return_type: Arc::new(Type::String),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_param_constraints: Vec::new(),
                type_predicate: None,
            })),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            type_param_defaults: Vec::new(),
        }],
        index_signature: None,
        index_signature_name: None,
        method_names: Vec::new(),
    });
    assert_eq!(returns_function.display_string(), "new () => () => string");
}

#[test]
fn constructor_conditional_types_infer_class_instances() {
    let source = r#"
type Instance<T> = T extends abstract new (...args: any[]) => infer R ? R : never;
abstract class A { value!: string; }
declare const instance: Instance<typeof A>;
const good: string = instance.value;
const bad: number = instance.value;
"#;
    assert_eq!(diagnostic_codes(source), vec![2322]);
}

#[test]
fn abstract_base_remains_the_best_common_constructor_type() {
    let source = r#"
abstract class Base { value!: string; }
class Derived extends Base {}
const constructors = [Base, Derived];
"#;
    let file = tsc_rs_parser::parse("abstract_constructor_bct.ts", source);
    let array_start = source.find("[Base, Derived]").unwrap() as u32;
    let symbols = tsc_rs_symbols::bind(&file);
    let output =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());

    assert!(output.diagnostics.is_empty(), "{:#?}", output.diagnostics);
    assert_eq!(
        output
            .expression_types
            .get(&array_start)
            .map(String::as_str),
        Some("typeof Base[]"),
        "expression types: {:?}",
        output.expression_types
    );
}

#[test]
fn incompatible_constructor_parameters_and_private_origins_prevent_bct_collapse() {
    for (source, expected) in [
        (
            "class S { constructor(value: string) {} }\nclass N { constructor(value: number) {} }\nconst constructors = [S, N];",
            "(typeof S | typeof N)[]",
        ),
        (
            "class A { private value = 1; }\nclass B { private value = 1; }\nconst constructors = [A, B];",
            "(typeof A | typeof B)[]",
        ),
        (
            "class A { private static value = 1; }\nclass B { private static value = 1; }\nconst constructors = [A, B];",
            "(typeof A | typeof B)[]",
        ),
    ] {
        let file = tsc_rs_parser::parse("constructor_bct.ts", source);
        let array_start = source.find('[').unwrap() as u32;
        let symbols = tsc_rs_symbols::bind(&file);
        let output =
            TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());

        assert_eq!(
            output
                .expression_types
                .get(&array_start)
                .map(String::as_str),
            Some(expected),
            "expression types: {:?}",
            output.expression_types
        );
    }
}

#[test]
fn callable_constructor_objects_remain_assignable_as_functions() {
    assert_eq!(
        diagnostic_codes(
            "interface Dual { (value?: any): boolean; new(value?: any): object }\ndeclare const Dual: Dual;\ndeclare function accepts(callback: (value: string) => unknown): void;\naccepts(Dual);"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Dual { (x: number): number; new(): object }\ninterface CallOnly { (x: string): string }\ndeclare const dual: Dual;\nlet callOnly: CallOnly = dual;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Dual { (x: number): number; (x: string): string; new(): object }\ndeclare const dual: Dual;\nlet callback: (x: string) => string = dual;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface DualTarget { (x: string): string; new(): object }\ndeclare const fn: (x: string) => string;\nlet target: DualTarget = fn;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: number): number; (x: string): string }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; required: number }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2741]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; optional?: number }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; length: number }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "declare const fn: (x: string) => string;\nlet target: { length: number } = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "declare const fn: (x: string) => string;\nlet target: { optional?: number } = fn;"
        ),
        vec![2559]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Weak { optional?: number }\ndeclare const fn: (x: string) => string;\nlet target: Weak = fn;"
        ),
        vec![2559]
    );
    assert_eq!(
        diagnostic_codes("declare const fn: (x: string) => string;\nlet target: any[] = fn;"),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "declare const fn: (x: string) => string;\nlet target: { length?: number } = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; length: string }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; apply: string }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; apply: () => string }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: string }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: Function }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: object }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: Object }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "type Caller = Function;\ninterface Target { (x: string): string; caller: Caller }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: {} }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: Function | null }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: Function & { tag?: string } }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: Function & { length: number } }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; caller: Function & { length: string } }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2322]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Target { (x: string): string; length?: string }\ndeclare const fn: (x: string) => string;\nlet target: Target = fn;"
        ),
        vec![2322]
    );
}

#[test]
fn builtin_collection_constructors_widen_mutable_literal_elements() {
    assert_eq!(
        diagnostic_codes(
            "interface Set<T> { has(value: T): boolean }\ninterface SetConstructor { new<T = any>(values?: T[]): Set<T> }\ndeclare const Set: SetConstructor;\nconst roles = new Set(['superadmin', 'super_admin', 'super-admin']);\ndeclare const role: string;\nroles.has(role);"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Set<T> { has(value: T): boolean }\ninterface SetConstructor { new<T = any>(values?: T[]): Set<T> }\ndeclare const Set: SetConstructor;\ndeclare const tuple: ['a', 'b'];\nconst values = new Set(tuple);\nvalues.has('c');"
        ),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Set<T> { has(value: T): boolean }\ninterface SetConstructor { new<T = any>(values?: T[]): Set<T> }\ndeclare const Set: SetConstructor;\ndeclare const values: ('a' | 'b')[];\nconst set = new Set(values);\nset.has('c');"
        ),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Map<K, V> { get(key: K): V }\ninterface MapConstructor { new<K = any, V = any>(values?: [K, V][]): Map<K, V> }\ndeclare const Map: MapConstructor;\ndeclare const entries: ['a' | 'b', 1 | 2][];\nconst map = new Map(entries);\nmap.get('c');"
        ),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Set<T> { has(value: T): boolean }\ninterface SetConstructor { new<T = any>(values?: T[]): Set<T> }\ndeclare const Set: SetConstructor;\nconst set = new Set(['a' as const]);\nset.has('b');"
        ),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Map<K, V> { get(key: K): V }\ninterface MapConstructor { new<K = any, V = any>(values?: [K, V][]): Map<K, V> }\ndeclare const Map: MapConstructor;\nconst map = new Map([[(('a' as const)), 1]]);\nmap.get('b');"
        ),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Map<K, V> { get(key: K): V }\ninterface MapConstructor { new<K = any, V = any>(values?: [K, V][]): Map<K, V> }\ndeclare const Map: MapConstructor;\nconst key = 'a' as const;\nconst map = new Map([[key, 1]]);\nmap.get('b');"
        ),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Set<T> { has(value: T): boolean }\ninterface SetConstructor { new<T = any>(values?: T[]): Set<T> }\ndeclare const Set: SetConstructor;\nconst key = 'a' as const;\nconst set = new Set([key]);\nset.has('b');"
        ),
        vec![2345]
    );
    assert_eq!(
        diagnostic_codes(
            "interface Set<T> { has(value: T): boolean }\ninterface SetConstructor { new<T = any>(values?: T[]): Set<T> }\ndeclare const Set: SetConstructor;\nconst set = new Set([`a`]);\ndeclare const value: string;\nset.has(value);"
        ),
        Vec::<u32>::new()
    );
    assert_eq!(
        diagnostic_codes(
            "interface Set<T> { has(value: T): boolean }\ninterface SetConstructor { new<T = any>(values?: T[]): Set<T> }\ndeclare const Set: SetConstructor;\ndeclare const values: string[];\nconst deduplicated = new Set([...values, 'c']);\ndeduplicated.has('d');"
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn inherited_static_members_are_required_on_constructor_assignments() {
    let diagnostics = diagnostics(
        "class Required { static inherited = 1; }\nclass Target extends Required {}\nclass Source {}\nlet constructor: typeof Target = Source;",
    );
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        vec![2322],
        "diagnostics: {diagnostics:#?}"
    );
}
