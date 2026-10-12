use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str, mut options: CompilerOptions) -> Vec<Diagnostic> {
    options.target = Some(ScriptTarget::ES2015);
    let file = tsc_rs_parser::parse("unknown.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

fn strict() -> CompilerOptions {
    CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    }
}

#[test]
fn catch_variables_follow_the_unknown_option_and_strict_default() {
    let source = "try {} catch (error) { error.message; error(); error++; }";
    for options in [
        strict(),
        CompilerOptions {
            use_unknown_in_catch_variables: Some(true),
            ..CompilerOptions::default()
        },
    ] {
        let errors = diagnostics(source, options);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [18046, 18046, 18046],
            "{errors:?}"
        );
        for error in errors {
            assert_eq!(error.message, "'error' is of type 'unknown'.");
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], "error");
        }
    }
    for options in [
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            use_unknown_in_catch_variables: Some(false),
            ..strict()
        },
    ] {
        let errors = diagnostics(source, options);
        assert!(errors.is_empty(), "{errors:?}");
    }
    let errors = diagnostics(
        "try {} catch (error: any) { error.message; error(); error++; }",
        strict(),
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn type_guards_allow_operations_on_narrowed_catch_variables() {
    let source = "try {} catch (error) { if (typeof error === 'string') { error.toUpperCase(); } if (typeof error === 'number') { error.toFixed(); error++; } if (error instanceof Error) { error.message; } }";
    let errors = diagnostics(source, strict());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn properties_calls_and_element_access_report_unknown_receivers() {
    let source = "declare const value: unknown; value.property; value(); value[0]; new value();";
    let errors = diagnostics(source, strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046, 18046, 18046, 18046],
        "{errors:?}"
    );
    for error in errors {
        assert_eq!(error.message, "'value' is of type 'unknown'.");
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "value");
    }
}

#[test]
fn diagnostic_names_follow_entity_expression_syntax() {
    let errors = diagnostics(
        "declare const object: { value: unknown }; object.value.property; (object.value).property;",
        strict(),
    );
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046, 2571],
        "{errors:?}"
    );
    assert_eq!(errors[0].message, "'object.value' is of type 'unknown'.");
    assert_eq!(errors[1].message, "Object is of type 'unknown'.");
}

#[test]
fn aliases_and_function_results_preserve_unknown() {
    let source = "type Value = unknown; declare const value: Value; declare function getValue(): unknown; value.property; getValue().property; getValue()(); new (getValue())();";
    let errors = diagnostics(source, strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046, 2571, 2571, 2571],
        "{errors:?}"
    );
}

#[test]
fn optional_access_checks_the_non_nullish_unknown_type() {
    let errors = diagnostics("declare const value: unknown; value?.toString(); value?.property; value?.(); value?.['key'];", strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2339, 2349, 7053],
        "{errors:?}"
    );
    assert_eq!(
        errors[0].message,
        "Property 'property' does not exist on type '{}'."
    );
    assert_eq!(
        errors[1].message,
        "This expression is not callable.\n  Type '{}' has no call signatures."
    );
    assert_eq!(errors[2].message, "Element implicitly has an 'any' type because expression of type '\"key\"' can't be used to index type '{}'.\n  Property 'key' does not exist on type '{}'.");
}

#[test]
fn optional_unknown_members_keep_short_circuit_undefined() {
    let errors = diagnostics("declare const value: unknown; value?.toString(); value?.['toString'](); const method = value?.toString; method(); const indexed = value?.['toString']; indexed();", strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2722, 2722],
        "{errors:?}"
    );
}

#[test]
fn optional_calls_respect_property_assignment_and_union_guards() {
    for source in [
        "const object: { method?: () => void } = {}; object.method = () => {}; object.method(); object!.method(); (object).method();",
        "interface A {} interface B {} declare const options: { value?: undefined; get: () => any } | { value: A | B; get?: undefined }; options.value || options.get();",
    ] {
        let errors = diagnostics(source, strict());
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn disabling_strict_null_checks_keeps_member_and_callability_diagnostics() {
    let options = CompilerOptions {
        strict: Some(false),
        no_implicit_any: Some(true),
        ..CompilerOptions::default()
    };
    let errors = diagnostics("declare const value: unknown; value.toString(); value.property; value(); value[0]; declare const key: string; value[key];", options);
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2339, 2349, 7053, 7053],
        "{errors:?}"
    );
    assert_eq!(
        errors[0].message,
        "Property 'property' does not exist on type 'unknown'."
    );
    assert!(errors[2]
        .message
        .ends_with("Property '0' does not exist on type '{}'."));
    assert!(errors[3]
        .message
        .ends_with("No index signature with a parameter of type 'string' was found on type '{}'."));
}

#[test]
fn arguments_are_still_checked_after_an_unknown_callee_error() {
    let errors = diagnostics(
        "declare const value: unknown; value(missingName);",
        strict(),
    );
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046, 2304],
        "{errors:?}"
    );
}

#[test]
fn private_names_on_any_and_unknown_require_lexical_declarations() {
    let errors = diagnostics("class C { #field = 1; m(value: any, other: unknown) { value.#field; value.#missing; other.#field; other.#missing; } }", strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2339, 18046, 18046, 2339],
        "{errors:?}"
    );
    assert_eq!(
        errors[0].message,
        "Property '#missing' does not exist on type 'any'."
    );
    assert_eq!(
        errors[3].message,
        "Property '#missing' does not exist on type 'any'."
    );
}

#[test]
fn unknown_indexing_validates_keys_before_implicit_any() {
    let source = "declare const value: unknown; value?.[true]; value?.[{}]; value?.[undefined]; value?.[null];";
    for strict_null_checks in [true, false] {
        let errors = diagnostics(
            source,
            CompilerOptions {
                strict_null_checks: Some(strict_null_checks),
                no_implicit_any: Some(false),
                ..strict()
            },
        );
        assert_eq!(
            errors.iter().map(|e| e.code).collect::<Vec<_>>(),
            [2538, 2538, 2538, 2538],
            "{errors:?}"
        );
        for (error, expected) in errors.iter().zip(["true", "{}", "undefined", "null"]) {
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], expected);
        }
    }
    let errors = diagnostics("declare const value: unknown; declare const key: 'toString' | 'missing'; value?.[key]; declare const neverKey: never; value?.[neverKey];", strict());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 7053);
    assert!(errors[0]
        .message
        .ends_with("Property 'missing' does not exist on type '{}'."));
}

#[test]
fn comparisons_and_assignments_preserve_guard_facts() {
    let source = "class C { field = 1; } function f(value: unknown) { let flag = false; if (flag = value instanceof C) { value.field; } if (value instanceof C === false || value.field !== 1) {} if (false !== (value instanceof C)) { value.field; } }";
    let errors = diagnostics(source, strict());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn constrained_and_named_generic_inference_preserves_known_receivers() {
    let source = "interface Shape { field: number } declare const f: { <T extends Shape>(): T }; f().field; type Wrapped<T> = { [K in keyof T]: T[K] }; declare function unwrap<T>(value: Wrapped<T>): T; declare const wrapped: Wrapped<Shape>; unwrap(wrapped).field;";
    let errors = diagnostics(source, strict());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn structural_inference_finds_callback_constructor_and_mapped_results() {
    let mut failures = Vec::new();
    for source in [
        "interface Box<T> { value: T; consume: (value: T) => void } declare function infer<T>(factory: () => Box<T>): T; const result = infer(() => ({ value: { field: 1 }, consume: value => {} })); result.field;",
        "class C<T extends number> { field!: T; } declare function create<T>(constructor: new () => T): T; const result = create(C); result.field;",
        "function infer<T>(constructor = class { static field: T }): T { return undefined!; } infer(class { static field = 'value' }).length;",
        "class C { field!: { value: number }; } declare function get<T>(object: { field: T }): T; get(new C()).value;",
        "type Box<T> = { value: T }; type Boxes<T> = { [K in keyof T]: Box<T[K]> }; declare function unbox<T>(boxes: Boxes<T>): T; const result = unbox({ field: { value: 1 } }); result.field;",
        "declare function read<T>(value: Readonly<T>): T; const result = read({ field: 1 }); result.field;",
        "declare const values: { [K in 'field']: null | { value: number } }; declare function identity<T>(value: T): T | null; function f<K extends 'field'>(key: K) { const result = identity(values[key]); if (result == null) return; result.value; }",
    ] {
        let errors = diagnostics(source, strict());
        if !errors.is_empty() { failures.push(format!("{source}: {errors:?}")); }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn inferred_results_keep_their_concrete_types() {
    for (source, expected) in [
        ("interface Box<T> { value: T; consume: (value: T) => void } declare function infer<T>(factory: () => Box<T>): T; const result = infer(() => ({ value: { field: 1 }, consume: value => {} })); result;", "{ field: number; }"),
        ("class C<T extends number> { field!: T; } declare function create<T>(constructor: new () => T): T; const result = create(C); result;", "C<number>"),
        ("type Box<T> = { value: T }; type Boxes<T> = { [K in keyof T]: Box<T[K]> }; declare function unbox<T>(boxes: Boxes<T>): T; const result = unbox({ field: { value: 1 } }); result;", "{ field: number; }"),
        ("declare function read<T>(value: Readonly<T>): T; const result = read({ field: 1 }); result;", "{ field: number; }"),
    ] {
        let file = tsc_rs_parser::parse("inference.ts", source);
        let symbols = tsc_rs_symbols::bind(&file);
        let result = TypeChecker::new().check_with_options(&file, &symbols, &strict());
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.expression_types.get(&(source.rfind("result;").unwrap() as u32)).map(|shown| shown.split_whitespace().collect::<Vec<_>>().join(" ")), Some(expected.to_string()), "{source}");
    }
}

#[test]
fn structural_reads_preserve_guards_numeric_keys_and_static_indices() {
    for source in [
        "const object: { key?: { x?: number } } = {}; const key = 'key'; if (object[key] && object[key].x) { object[key].x; }",
        "declare const object: { '1': number; '1.': string; '1.0': Date }; var n = object[1]; var n = object[1.]; var n = object[1.0];",
        "function create() { return class { static [name: string]: number; }; } const C = create(); C.value; C.value = 1;",
        "class C { static #first = class { m() { new C.#second(); } }; static #second = class {}; }",
        "type Fn<T> = (value: T) => void; declare const broad: Fn<string>; declare const narrow: Fn<'a'>; declare function accepts<T>(a: Fn<T>, b: Fn<T>): void; accepts(broad, narrow);",
    ] {
        let errors = diagnostics(source, strict());
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn unknown_arithmetic_operands_report_their_expression() {
    let source = "declare let value: unknown; value + 1; value * 2; value < 0; +value; -value; ~value; value++; value += 1; value *= 2; 'text' + value; value == 0; (value)++;";
    let errors = diagnostics(source, strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046, 18046, 18046, 18046, 18046, 18046, 18046, 18046, 18046, 2571],
        "{errors:?}"
    );
    for error in &errors[..9] {
        assert_eq!(error.message, "'value' is of type 'unknown'.");
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "value");
    }
}

#[test]
fn unknown_updates_without_strict_null_checks_are_arithmetic_errors() {
    let errors = diagnostics(
        "declare let value: unknown; value++; +value; -value; ~value; value * 2; value *= 2;",
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2356, 2362, 2362],
        "{errors:?}"
    );
}

#[test]
fn unknown_spread_and_rest_report_the_operand_and_recover() {
    let source = "declare const value: unknown; const first = { ...value }; const second: { n: number } = { ...value, n: 'bad' }; const { ...rest } = value; first.missing; second.n;";
    for options in [
        strict(),
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    ] {
        let errors = diagnostics(source, options);
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [2698, 2698, 2700],
            "{errors:?}"
        );
        for (error, expected) in errors.iter().zip(["...value", "...value", "rest"]) {
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], expected);
        }
    }
}

#[test]
fn unknown_and_generic_assignments_respect_non_nullish_object_types() {
    let source = "declare let value: unknown; let empty: {} = value; let complete: {} | null | undefined = value; let incomplete: {} | null = value; function f<T, U extends unknown, V extends object>(t: T, u: U, v: V) { let a: {} = t; let b: object = t; let c: {} = u; let d: {} = v; let e: object = v; }";
    for (options, expected) in [
        (strict(), vec!["empty", "incomplete", "a", "b", "c"]),
        (
            CompilerOptions {
                strict: Some(false),
                ..CompilerOptions::default()
            },
            vec!["b"],
        ),
    ] {
        let errors = diagnostics(source, options);
        assert_eq!(errors.len(), expected.len(), "{errors:?}");
        for (error, expected) in errors.iter().zip(expected) {
            assert_eq!(error.code, 2322, "{error:?}");
            let span = error.span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], expected);
        }
    }
}

#[test]
fn arrays_report_missing_string_index_signatures() {
    let errors = diagnostics(
        "declare const values: number[]; let record: { [name: string]: unknown } = values;",
        strict(),
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2322);
    assert_eq!(errors[0].message, "Type 'number[]' is not assignable to type '{ [name: string]: unknown; }'.\n  Index signature for type 'string' is missing in type 'number[]'.");
}

#[test]
fn unknown_numeric_aliases_and_nullish_operands_keep_both_errors() {
    let source = "type U = unknown; declare let value: U; +value; value * 2; value + null; null + value; value < null; null < value; value += null; value * null; value *= null;";
    let errors = diagnostics(source, strict());
    assert_eq!(
        errors.iter().filter(|error| error.code == 18046).count(),
        9,
        "{errors:?}"
    );
    assert_eq!(
        errors.iter().filter(|error| error.code == 18050).count(),
        7,
        "{errors:?}"
    );
    assert_eq!(errors.len(), 16, "{errors:?}");
}

#[test]
fn constructor_inference_uses_intersection_and_mapped_callback_sites() {
    let source = "type Computed<T> = { get?(): T; set?(value: T): void }; type Accessors<T> = { [K in keyof T]: (() => T[K]) | Computed<T[K]> }; type Options<D, M, P> = ThisType<D & M & P> & { data?: D | (() => D); methods?: M; computed?: Accessors<P> }; declare const Build: new <D, M, P>(options: Options<D, M, P>) => D & M & P; const result = new Build({ data: () => ({ x: 1 }), methods: { run() { return 1; } }, computed: { count(): number { return 1; }, greeting: { get() { return 'hi'; }, set(value: string) {} } } }); result.x; result.run(); result.count; result.greeting;";
    let file = tsc_rs_parser::parse("inference.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let checked = TypeChecker::new().check_with_options(&file, &symbols, &strict());
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    for (expression, expected) in [
        ("result.x", "number"),
        ("result.run()", "number"),
        ("result.count", "number"),
        ("result.greeting", "string"),
    ] {
        let actual = checked
            .expression_types
            .get(&(source.rfind(expression).unwrap() as u32));
        assert_eq!(
            actual.map(String::as_str),
            Some(expected),
            "{expression}: {actual:?}"
        );
    }
}

#[test]
fn unknown_guards_preserve_non_nullish_origin_for_in_checks() {
    let source = "function f(value: unknown, ordinary: {}) { if (value) { 'a' in value; const copy = value; 'a' in copy; } 'a' in ordinary; if (value == null) return; 'a' in value; }";
    let errors = diagnostics(source, strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2638, 2638, 2638],
        "{errors:?}"
    );
    for error in errors {
        assert_eq!(error.message, "Type '{}' may represent a primitive value, which is not permitted as the right operand of the 'in' operator.");
    }
    let errors = diagnostics(
        source,
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2322, 2322, 2322],
        "{errors:?}"
    );
}

#[test]
fn in_operands_require_property_keys_and_objects() {
    let errors = diagnostics("declare const value: unknown; true in 1; value in {}; 'a' in value; 'a' in {}; 1 in []; Symbol() in {};", strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2322, 2322, 18046, 18046],
        "{errors:?}"
    );
    assert_eq!(
        errors[0].message,
        "Type 'boolean' is not assignable to type 'string | number | symbol'."
    );
    assert_eq!(
        errors[1].message,
        "Type 'number' is not assignable to type 'object'."
    );
}

#[test]
fn unknown_nullish_guards_keep_the_correct_remaining_values() {
    let source = "function f(value: unknown) { if (value !== null) { let a: {} | undefined = value; let b: {} = value; } if (value !== undefined) { let c: {} | null = value; let d: {} = value; } if (value == null) { let e: null | undefined = value; } else { let f: {} = value; } }";
    let errors = diagnostics(source, strict());
    assert_eq!(errors.len(), 2, "{errors:?}");
    for (error, name) in errors.iter().zip(["b", "d"]) {
        assert_eq!(error.code, 2322);
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], name);
    }
}

#[test]
fn unknown_truthiness_and_object_guards_combine() {
    let source = "function f(value: unknown) { return value && value instanceof Object && 'key' in value; } function g<T>(value: T) { return value && value instanceof Object && 'key' in value; } function h(value: unknown) { return !!value && typeof value === 'object' && 'key' in value; }";
    let errors = diagnostics(source, strict());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn in_rejects_unconstrained_members_of_generic_unions() {
    let errors = diagnostics(
        "function f<T>(value: T | object) { return 'key' in value; }",
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2322);
    assert_eq!(errors[0].message, "Type 'object | T' is not assignable to type 'object'.\n  Type 'T' is not assignable to type 'object'.");
}

#[test]
fn successful_in_guard_establishes_an_object_shape() {
    let errors = diagnostics("function f(value: unknown) { if (value && 'first' in value) { 'second' in value; value.first; } }", strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [2638],
        "{errors:?}"
    );
}

#[test]
fn unknown_absorbs_nested_union_members_but_any_takes_priority() {
    use tsc_rs_types::{type_ops, Type};
    let nested = Type::Union(
        vec![
            Type::Number,
            Type::Union(vec![Type::Unknown, Type::String].into()),
        ]
        .into(),
    );
    for simplify in [Type::flatten_union, type_ops::simplify_union] {
        assert_eq!(simplify(vec![nested.clone(), Type::Null]), Type::Unknown);
        assert_eq!(simplify(vec![nested.clone(), Type::Any]), Type::Any);
    }
}

#[test]
fn strict_unknown_equality_uses_primitive_and_object_types() {
    let source = "declare let value: unknown; declare const text: string; declare const symbol: unique symbol; declare const object: { x: number }; declare const fn: () => void; if (value === text) { const textResult: string = value; } if (value !== symbol) {} else { const symbolResult: typeof symbol = value; } if (value === object) { const objectResult: object = value; } if (value === fn) { const fnResult: object = value; } value.missing;";
    let errors = diagnostics(source, strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046],
        "{errors:?}"
    );
}

#[test]
fn loose_equality_does_not_narrow_unknown_to_literals() {
    let errors = diagnostics("function f(value: unknown) { if (value == 1) { value.toFixed(); } if (value == true) { value.valueOf(); } if (value === 1) { value.toFixed(); } }", strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046, 18046],
        "{errors:?}"
    );
}

#[test]
fn unknown_switch_includes_fallthrough_and_typed_cases() {
    let source = "declare const symbol: unique symbol; declare const fn: () => void; function f(value: unknown): 'one' | 'two' { switch(value) { case 'one': case 'two': case 'three': return value; default: throw 1; } } function g(value: unknown) { switch(value) { case symbol: const same: typeof symbol = value; break; case fn: const object: object = value; break; } }";
    let errors = diagnostics(source, strict());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2322);
    assert_eq!(errors[0].message, "Type '\"one\" | \"two\" | \"three\"' is not assignable to type '\"one\" | \"two\"'.\n  Type '\"three\"' is not assignable to type '\"one\" | \"two\"'.");
}

#[test]
fn unknown_enum_equality_preserves_member_identity() {
    let errors = diagnostics("enum E { A, B } function f(value: unknown) { if(value === E.A) { const member: E.A = value; } if(value !== E) {} else { const object: object = value; } }", strict());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn sequential_unknown_nullish_comparisons_restore_the_original_type() {
    let errors = diagnostics("function f(value: unknown, text: string) { if (value === undefined) { const u: undefined = value; } if (value === null) { const n: null = value; } if (value === text) { const s: string = value; } value.missing; }", strict());
    assert_eq!(
        errors.iter().map(|error| error.code).collect::<Vec<_>>(),
        [18046],
        "{errors:?}"
    );
}

#[test]
fn unknown_equality_respects_shadowed_undefined() {
    let errors = diagnostics("function f(value: unknown, undefined: number) { if (value === undefined) { const n: number = value; const u: undefined = value; } }", strict());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2322);
    assert_eq!(
        errors[0].message,
        "Type 'number' is not assignable to type 'undefined'."
    );
}

#[test]
fn unknown_like_unions_expand_anonymous_empty_object_aliases() {
    let source = "type A = {}; type B = {}; type Nodes = A | B; type Nullable = Nodes | null; declare const value: unknown; const a: Nodes | null | undefined = value; const b: Nullable | undefined = value; interface Empty {} const badInterface: Empty | null | undefined = value; const badVoid: {} | null | void = value;";
    let errors = diagnostics(source, strict());
    assert_eq!(errors.len(), 2, "{errors:?}");
    for (error, name) in errors.iter().zip(["badInterface", "badVoid"]) {
        assert_eq!(error.code, 2322);
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], name);
    }
}

#[test]
fn nullish_intersections_reduce_after_generic_instantiation() {
    let source = "type N = null & {}; type U = undefined & string; type V = undefined & void; declare const n: N; declare const u: U; declare const v: V; const a: never = n; const b: never = u; const c: undefined = v; declare function present<T>(value: T & {}): void; present(null);";
    for options in [
        strict(),
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    ] {
        let strict = options.strict;
        let errors = diagnostics(source, options);
        assert_eq!(errors.len(), 1, "strict={strict:?}: {errors:?}");
        assert_eq!(errors[0].code, 2345);
        assert_eq!(
            errors[0].message,
            "Argument of type 'null' is not assignable to parameter of type 'never'."
        );
    }
}

#[test]
fn constrained_nullish_intersections_prove_impossible_values() {
    let source = "type Keys = 'A' | 'B'; function impossible<T extends Keys>(value: T & undefined) { const result: never = value; } function possible<T>(value: T & undefined) { const result: never = value; }";
    let errors = diagnostics(source, strict());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2322);
    assert_eq!(
        errors[0].message,
        "Type 'T & undefined' is not assignable to type 'never'."
    );
}

#[test]
fn generic_values_keep_their_identity_in_nullish_equality_branches() {
    let source = "function f<T extends unknown>(value: T): T { if (value === undefined) { return value; } if (value === null) { return value; } return value; }";
    let errors = diagnostics(source, strict());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn constrained_generic_nullish_guards_still_narrow_possible_values() {
    let source = "function a<T extends string | undefined>(value: T) { if(value === undefined) { const u: undefined = value; } } function b<T extends {}>(value: T) { if(value === undefined) { const impossible: never = value; } } function c<T>(value: T) { if(value === undefined) { const u: undefined = value; } }";
    let errors = diagnostics(source, strict());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, 2322);
    assert_eq!(
        errors[0].message,
        "Type 'T' is not assignable to type 'undefined'."
    );
}

#[test]
fn nullish_object_intersections_respect_strict_null_checks() {
    let source = "interface Empty {} class C {} declare const n: null & { tag: string }; declare const u: undefined & (() => void); declare const named: null & Empty; declare const instance: undefined & C; const a: never = n; const b: never = u; const c: never = named; const d: never = instance;";
    assert!(diagnostics(source, strict()).is_empty());
    let errors = diagnostics(
        source,
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(errors.len(), 4, "{errors:?}");
    assert_eq!(
        errors[0].message,
        "Type 'null' is not assignable to type 'never'."
    );
    assert_eq!(
        errors[1].message,
        "Type 'undefined' is not assignable to type 'never'."
    );
    assert_eq!(errors[2].message, errors[0].message);
    assert_eq!(errors[3].message, errors[1].message);
}

#[test]
fn generic_keyof_requires_keys_valid_for_the_original_receiver() {
    let source = "function invalid<T>(value: T, key: keyof (T & {})) { value[key]; } function valid<T>(value: T, key: keyof T) { value[key]; } function guarded<T>(value: T & {}, key: keyof T) { value[key]; } function constrained<T extends {}>(value: T, key: keyof (T & {})) { value[key]; } declare function read<T>(value: T, key: keyof T): void; read(null, 'key');";
    let errors = diagnostics(source, strict());
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0].code, 2536);
    assert_eq!(
        errors[0].message,
        "Type 'keyof (T & {})' cannot be used to index type 'T'."
    );
    assert_eq!(errors[1].code, 2345);
    assert_eq!(
        errors[1].message,
        "Argument of type '\"key\"' is not assignable to parameter of type 'never'."
    );
}

#[test]
fn generic_nullish_intersections_preserve_only_nonredundant_guards() {
    let source = "function a<T extends {}>(value: T & ({} | null)) { if (value === 42) {} } function b<T extends {} | null>(value: T & ({} | null)) { if (value === 42) {} } function c<T extends {} | undefined>(value: T & ({} | null)) { if (value === 42) {} } function d<T>(value: T & ({} | null)) { if (value === 42) {} }";
    let errors = diagnostics(source, strict());
    assert_eq!(errors.len(), 2, "{errors:?}");
    for error in errors {
        assert_eq!(error.code, 2367);
        assert_eq!(error.message, "This comparison appears to be unintentional because the types 'T' and 'number' have no overlap.");
    }
}

#[test]
fn branded_literal_aliases_are_excluded_by_equality_guards() {
    let source = "type Left = 'left'; type Right = 'right' & { brand: true }; type Either = Left | Right; function f(value: Either) { if(value === 'left') {} else if (value === 'right') {} else { const done: never = value; } }";
    let errors = diagnostics(source, strict());
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn keyof_bottom_and_nullish_types_have_their_typescript_key_sets() {
    let source = "declare const key: keyof never; const valid: string | number | symbol = key; declare const n: keyof null; declare const u: keyof undefined; const noNullKeys: never = n; const noUndefinedKeys: never = u;";
    let errors = diagnostics(source, strict());
    assert!(errors.is_empty(), "{errors:?}");
}
