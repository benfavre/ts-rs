use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

/// (code, text at span) of every diagnostic with one of `codes`.
fn errors(source: &str, codes: &[u32]) -> Vec<(u32, String)> {
    let file = tsc_rs_parser::parse("rules.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let result =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
    result
        .diagnostics
        .into_iter()
        .filter(|d| codes.contains(&d.code))
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                source[span.start as usize..span.end as usize].into(),
            )
        })
        .collect()
}

#[test]
fn strict_parameters_may_not_be_named_eval_or_arguments() {
    assert_eq!(
        errors(
            "function f(arguments: number) {}\nvar g = (eval: number) => 0;\nvar t: (arguments: number) => void;",
            &[1100]
        ),
        [
            (1100, "arguments".into()),
            (1100, "eval".into()),
            (1100, "arguments".into())
        ]
    );
    assert!(errors("declare function f(arguments: number): void;", &[1100]).is_empty());
}

#[test]
fn derived_constructors_must_call_super_directly() {
    let source = "class A {}\nclass B extends A {\n    public constructor() { const f = () => super(); }\n}\nclass C extends A { constructor() { super(); } }\nclass D extends null { constructor() {} }";
    assert_eq!(
        errors(source, &[2377]),
        [(2377, "public constructor".into())]
    );
}

#[test]
fn switch_cases_must_be_comparable_to_the_discriminant() {
    let source = "class Foo {}\ndeclare var q: string;\ndeclare var r: number | \"hello\";\nswitch (0) { case Foo: break; case \"sss\": break; case 0: break; }\nswitch (r) { case q: break; case 42: break; case true: break; }\nvar s: any = 0;\nswitch (s) { case Foo: break; }";
    assert_eq!(
        errors(source, &[2678]),
        [
            (2678, "Foo".into()),
            (2678, "\"sss\"".into()),
            (2678, "true".into())
        ]
    );
}

#[test]
fn super_property_needs_a_method_container() {
    let source = "var obj = {\n    method() { super.a(); },\n    p1: function () { super.b(); },\n    p2: () => { super.c(); }\n};\nclass A { m() {} }\nclass B extends A {\n    x = super.m;\n    f() { const g = () => super.m(); function h() { super.m(); } }\n}";
    assert_eq!(
        errors(source, &[2660]),
        [
            (2660, "super".into()),
            (2660, "super".into()),
            (2660, "super".into())
        ]
    );
}

#[test]
fn object_destructuring_assignment_checks_each_property() {
    let source = "var C: any;\n({ C = 1 } = {});\n({ C: C = 1 } = {});\n({ C } = {});";
    assert_eq!(errors(source, &[2339, 2741]), [(2339, "C".into())]);
}

#[test]
fn namespaces_without_a_type_meaning_are_not_types() {
    let source = "namespace A { }\nvar a: A;\nnamespace B { var b = 1; }\nvar b: B;\nnamespace C {}\ninterface C {}\nvar c: C;";
    assert_eq!(
        errors(source, &[2709]),
        [(2709, "A".into()), (2709, "B".into())]
    );
}

#[test]
fn renamings_in_bodyless_signatures_are_reported() {
    let source = "type F = ({ a: string }) => void;\ntype G = ({ a: string }) => typeof string;\ndeclare function f({ a: number }): void;\nfunction g({ a: b }) { return b; }";
    assert_eq!(
        errors(source, &[2842]),
        [(2842, "string".into()), (2842, "number".into())]
    );
}

#[test]
fn import_alias_conflicts_with_a_var_when_its_target_is_a_value() {
    let source = "namespace m { export var m = ''; }\nimport x = m.m;\nvar x = '';\nnamespace T { interface I {} }\nimport t = T;\nvar t;";
    assert_eq!(errors(source, &[2440]), [(2440, "import x = m.m;".into())]);
}

#[test]
fn shorthand_property_without_a_value_in_scope() {
    let source = "var x = 1;\nvar a = { x, b };";
    assert_eq!(errors(source, &[18004, 2304]), [(18004, "b".into())]);
}

#[test]
fn merge_conflict_markers_are_reported_by_the_parser() {
    let source = "class C {\n<<<<<<< HEAD\n    v = 1;\n=======\n    v = 2;\n>>>>>>> Branch-a\n}";
    let file = tsc_rs_parser::parse("markers.ts", source);
    let markers: Vec<_> = file
        .diagnostics
        .iter()
        .filter(|d| d.code == 1185)
        .map(|d| {
            let span = d.span.unwrap();
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(markers, ["<<<<<<<", "=======", ">>>>>>>"]);
}

#[test]
fn builtin_global_redeclarations() {
    let source = "var undefined = void 0;\nnamespace globalThis { export var x = 1; }\ninterface undefined {}";
    assert_eq!(
        errors(source, &[2397]),
        [(2397, "undefined".into()), (2397, "globalThis".into())]
    );
}

#[test]
fn loop_and_pattern_bindings_are_uninitialized_in_their_own_initializers() {
    let source = "for (let v of v) { }\nlet [x2 = x2] = [];\nlet [p, q = p] = [1];\no;\ndeclare const o: number;";
    assert_eq!(
        errors(source, &[2448]),
        [(2448, "v".into()), (2448, "x2".into())]
    );
}

#[test]
fn recursive_interface_bases() {
    let source = "interface I5 extends I5 {}\ninterface A<T> extends B<T> {}\ninterface B<T> extends A<T> {}\ninterface C extends B<string> {}";
    assert_eq!(
        errors(source, &[2310]),
        [(2310, "I5".into()), (2310, "A".into()), (2310, "B".into())]
    );
}

#[test]
fn type_only_namespaces_are_not_values() {
    let source = "namespace M {}\nnamespace N { interface I {} }\nnamespace E { export const enum X { A } }\nnamespace V { export var v = 1; }\nclass C extends M {}\nN;\nV;\nE.X.A;";
    assert_eq!(
        errors(source, &[2708]),
        [(2708, "M".into()), (2708, "N".into())]
    );
}

#[test]
fn local_declaration_hides_the_aliased_namespace() {
    let source = "namespace Foo { export var x = 'hello'; }\nnamespace Bar {\n    var Foo = 1;\n    import F = Foo;\n}";
    assert_eq!(errors(source, &[2437]), [(2437, "Foo".into())]);
}

#[test]
fn value_used_as_a_type() {
    let source = "var v = 1;\nvar a: v;\nfunction f() {}\nvar b: f;\nvar I = 1;\ninterface I {}\nvar c: I;\nfunction g<v>(x: v) {}";
    assert_eq!(
        errors(source, &[2749]),
        [(2749, "v".into()), (2749, "f".into())]
    );
}

#[test]
fn bare_return_is_checked_as_undefined() {
    let file = tsc_rs_parser::parse(
        "r.ts",
        "class P {}\nfunction a(): P { return; }\nfunction b(): P | undefined { return; }\nfunction c(): void { return; }",
    );
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let found: Vec<_> = TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == 2322)
        .map(|d| d.message)
        .collect();
    assert_eq!(found, ["Type 'undefined' is not assignable to type 'P'."]);
}

#[test]
fn computed_property_names_must_be_string_number_or_symbol_like() {
    let source = "declare var p1: number | string;\ndeclare var p2: number | number[];\nvar v = { [p1]: 0, [p2]: 1 };\nfunction f<T, U extends string>(t: T, u: U) { return { [t]: 0, [u]: 1 }; }";
    assert_eq!(
        errors(source, &[2464]),
        [(2464, "[p2]".into()), (2464, "[t]".into())]
    );
}

#[test]
fn private_names_are_lexically_scoped() {
    let source = "class A {\n    #p = 1;\n    get #q() { return 1; }\n    m(a: A) { return a.#p + a.#q; }\n}\nclass B extends A {\n    n(b: B) { return b.#p; }\n}\nnew A().#q;";
    let found: Vec<_> = errors(source, &[18013])
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    assert_eq!(found, ["#p", "#q"]);
}

#[test]
fn await_is_reserved_in_module_top_level_and_async_functions() {
    let module = "export {};\nvar [await] = [1];\nfunction f(await: number) {}";
    assert_eq!(errors(module, &[1262, 1359]), [(1262, "await".into())]);
    let script = "async function g(await: number) {}\nvar await = 1;";
    assert_eq!(errors(script, &[1262, 1359]), [(1359, "await".into())]);
}

#[test]
fn multiple_default_exports_follow_binder_exclusions() {
    let classes = "export default class AA1 {}\nexport default class BB1 {}";
    assert_eq!(
        errors(classes, &[2528]),
        [(2528, "AA1".into()), (2528, "BB1".into())]
    );
    // Functions merge; an alias does not conflict with a class.
    assert!(errors(
        "export default function f(): void;\nexport default function f() {}",
        &[2528]
    )
    .is_empty());
    assert!(errors(
        "const foo = 1;\nexport default foo;\nexport default class Foo {}",
        &[2528]
    )
    .is_empty());
}

#[test]
fn property_and_accessor_overrides() {
    let source = "class A { p = 'yep'; get q() { return 1; } }\nclass B extends A { get p() { return 'x'; } q = 2; }\nclass C { get r() { return 1; } }\nclass D extends C { get r() { return 2; } }";
    assert_eq!(
        errors(source, &[2610, 2611]),
        [(2611, "p".into()), (2610, "q".into())]
    );
}

#[test]
fn super_in_non_derived_classes_and_computed_names() {
    let source = "class C {\n    constructor() { super(); }\n    m() { return super.x; }\n}\nclass Base { bar() { return 0; } }\nclass D extends Base {\n    [super.bar()]() {}\n    ok() { return class { [super.bar()]() {} }; }\n}";
    assert_eq!(
        errors(source, &[2335, 2466]),
        [
            (2335, "super".into()),
            (2335, "super".into()),
            (2466, "super".into())
        ]
    );
}

#[test]
fn comma_left_side_follows_tsc_side_effect_rules() {
    let source = "declare var a: any, b: any;\ntypeof a, b;\nvoid a, b;\na = (() => {}, b);\n(0, a.fn)();\nb.x, a;";
    assert_eq!(
        errors(source, &[2695]),
        [(2695, "typeof a".into()), (2695, "() => {}".into())]
    );
}
