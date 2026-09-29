use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn errors(source: &str, strict: bool) -> Vec<(String, String)> {
    diagnostics(source, strict, 2411)
}

fn diagnostics(source: &str, strict: bool, code: u32) -> Vec<(String, String)> {
    let file = tsc_rs_parser::parse("indexes.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(strict),
            ..CompilerOptions::default()
        },
    );
    result
        .diagnostics
        .into_iter()
        .filter(|d| d.code == code)
        .map(|d| {
            let span = d.span.unwrap();
            (
                source[span.start as usize..span.end as usize].into(),
                d.message,
            )
        })
        .collect()
}

#[test]
fn class_index_constraints_cover_fields_methods_accessors_and_static_members() {
    for (source, name, ty) in [
        (
            "class C { [k:string]:number; field = ''; }",
            "field",
            "string",
        ),
        (
            "class C { [k:string]:number; private field = ''; }",
            "field",
            "string",
        ),
        (
            "class C { [k:string]:number; method() {} }",
            "method",
            "() => void",
        ),
        (
            "class C { [k:string]:number; get field() { return ''; } }",
            "field",
            "string",
        ),
        (
            "class C { [k:string]:number; set field(value:string) {} }",
            "field",
            "string",
        ),
        (
            "class C { static [k:string]:number; static field = ''; }",
            "field",
            "string",
        ),
        (
            "class C { static [k:string]:number; static method() {} }",
            "method",
            "() => void",
        ),
        (
            "const C = class { [k:string]:number; field = ''; };",
            "field",
            "string",
        ),
    ] {
        assert_eq!(errors(source, false), [(name.into(), format!("Property '{name}' of type '{ty}' is not assignable to 'string' index type 'number'."))], "{source}");
    }
    for source in [
        "class C { [k:string]:number; static field = ''; #secret = ''; field = 1; }",
        "class C { static [k:string]:number; field = ''; static field = 1; }",
        "class C { [k:string]:number; get field():number {return 1;} set field(value:number|string) {} }",
        "class C { [k:string]:number; constructor() {} }",
    ] { assert!(errors(source, false).is_empty(), "{source}"); }
}

#[test]
fn numeric_and_computed_keys_select_their_applicable_index_signatures() {
    for name in [
        "0",
        "-1",
        "1.2e-20",
        "1e+21",
        "NaN",
        "Infinity",
        "-Infinity",
    ] {
        let source = format!("class C {{ [k:number]:number; '{name}' = ''; }}");
        assert_eq!(errors(&source, false).len(), 1, "{source}");
    }
    for name in [
        "-0", "+1", "1e0", "0x10", "0123", " 1", "+NaN", "-NaN", "1E+21", "",
    ] {
        let source = format!("class C {{ [k:number]:number; '{name}' = ''; }}");
        assert!(errors(&source, false).is_empty(), "{source}");
    }
    for source in [
        "class C { [k:string]:number; ['field'] = ''; }",
        "class C { [k:number]:number; [1] = ''; }",
        "declare const symbol: unique symbol; class C { [k:symbol]:number; [symbol] = ''; }",
        "class C { [k:`data-${string}`]:number; 'data-field' = ''; }",
    ] {
        assert_eq!(errors(source, false).len(), 1, "{source}");
    }
    for source in [
        "declare const symbol: unique symbol; class C { [k:string]:number; [symbol] = ''; }",
        "class C { [k:`data-${string}`]:number; field = ''; }",
        "class C { [k:symbol]:number; field = ''; }",
    ] {
        assert!(errors(source, false).is_empty(), "{source}");
    }
}

#[test]
fn inherited_generic_indexers_and_optional_fields_are_checked() {
    for source in [
        "class B<T> { [k:string]:T; } class C extends B<number> { field = ''; }",
        "class B<T> { [k:string]:T; } class M<U> extends B<U> {} class C extends M<number> { field = ''; }",
        "class B { field = ''; } class C extends B { [k:string]:number; }",
    ] { assert_eq!(errors(source, false).len(), 1, "{source}"); }
    assert!(errors(
        "class B<T> { [k:string]:T; } class C extends B<string> { field = ''; }",
        false
    )
    .is_empty());
    assert!(errors(
        "class B { static [k:string]:number; } class C extends B { static field = ''; }",
        false
    )
    .is_empty());
    let source = "class C { [k:string]:number; field?:number; }";
    assert!(errors(source, false).is_empty());
    assert_eq!(errors(source, true).len(), 1);
}

#[test]
fn interface_members_obey_own_inherited_and_merged_index_domains() {
    for source in [
        "interface I { [k:string]:number; field:string; }",
        "interface I { [k:string]:number; method():void; }",
        "interface B<T> { [k:string]:T; } interface I extends B<number> { field:string; }",
        "interface B<T> { [k:string]:T; } interface M<U> extends B<U> {} interface I extends M<number> { field:string; }",
        "class B<T> { [k:string]:T; } interface I extends B<number> { field:string; }",
        "interface I { [k:string]:number; } interface I { field:string; }",
        "interface I { [k:string]:number; field:string; } interface I { field:string; }",
        "interface I { field:string; } interface I { [k:string]:number; }",
        "interface I { [k:number]:number; '1':string; }",
        "declare const symbol:unique symbol; interface I { [k:symbol]:number; [symbol]:string; }",
    ] { assert_eq!(errors(source, false).len(), 1, "{source}"); }
    let source = "interface B { [k:string]:number|string; [k:number]:number; } interface I extends B { 0:string; field:string; }";
    assert_eq!(errors(source, false).len(), 1);
    for source in [
        "interface I { [k:string]:number; ():string; new():string; }",
        "interface B<T> { [k:string]:T; } interface I extends B<string> { field:string; }",
        "interface I { [k:number]:number; field:string; }",
        "interface I { [k:symbol]:number; field:string; }",
    ] {
        assert!(errors(source, false).is_empty(), "{source}");
    }
    let source = "interface I { [k:string]:number; field?:number; }";
    assert!(errors(source, false).is_empty());
    assert_eq!(errors(source, true).len(), 1);
}

#[test]
fn index_constraint_diagnostics_render_nested_types_on_one_line() {
    let source =
        "interface B { [k:string]: { x:number }; } interface I extends B { field:{ y:number }; }";
    assert_eq!(errors(source, false), [("field".into(), "Property 'field' of type '{ y: number; }' is not assignable to 'string' index type '{ x: number; }'.".into())]);
}

#[test]
fn index_value_types_respect_key_containment_and_static_sides() {
    for (source, expected_span) in [
        (
            "interface I { [s:string]:number; [n:number]:string; }",
            "[n:number]:string;",
        ),
        (
            "class C { [s:string]:number; [n:number]:string; }",
            "[n:number]:string;",
        ),
        (
            "class C { static [s:string]:number; static [n:number]:string; }",
            "static [n:number]:string;",
        ),
        (
            "type S=string; type N=number; interface I { [s:S]:number; [n:N]:string; }",
            "[n:N]:string;",
        ),
        (
            "interface B<T> { [s:string]:T; } interface I extends B<number> { [n:number]:string; }",
            "[n:number]:string;",
        ),
        (
            "class B<T> { [s:string]:T; } class C extends B<number> { [n:number]:string; }",
            "[n:number]:string;",
        ),
        (
            "interface B { [n:number]:string; } interface I extends B { [s:string]:number; }",
            "[s:string]:number;",
        ),
        (
            "interface I { [s:string]:number; } interface I { [n:number]:string; }",
            "[n:number]:string;",
        ),
        (
            "interface I { [n:number]:string; } interface I { [s:string]:number; }",
            "[n:number]:string;",
        ),
    ] {
        assert_eq!(
            diagnostics(source, false, 2413),
            [(
                expected_span.into(),
                "'number' index type 'string' is not assignable to 'string' index type 'number'."
                    .into()
            )],
            "{source}"
        );
    }
    for source in [
        "interface I { [s:string]:number|string; [n:number]:number; }",
        "interface I { [n:number]:string; } interface I { [s:string]:{length:number}; }",
        "class C { [s:string]:string; static [n:number]:number; }",
        "class B { static [n:number]:string; } class C extends B { static [s:string]:number; }",
        "interface I { [s:symbol]:number; [n:number]:string; }",
        "interface I { [s:string]:number; [n:number]:never; }",
    ] {
        assert!(diagnostics(source, false, 2413).is_empty(), "{source}");
    }
}

#[test]
fn inherited_index_conflicts_report_only_where_the_domains_are_combined() {
    let source = "interface A { [s:string]:number; } interface B { [n:number]:string; } interface C extends A,B {} interface D extends C {}";
    assert_eq!(
        diagnostics(source, false, 2413),
        [(
            "C".into(),
            "'number' index type 'string' is not assignable to 'string' index type 'number'."
                .into()
        )]
    );
    let source = "interface A { [s:string]:number; [n:number]:string; } interface B extends A {} interface C extends A,B {}";
    assert_eq!(diagnostics(source, false, 2413).len(), 1);
    let source = "interface A { [s:string]:number; } interface B { [n:number]:string; } interface C extends A,B { [n:number]:number; }";
    assert!(diagnostics(source, false, 2413).is_empty());
}

#[test]
fn index_values_include_boxed_primitive_properties() {
    assert!(diagnostics(
        "interface I { [s:string]:{length:number}; [n:number]:string; }",
        false,
        2413
    )
    .is_empty());
    assert_eq!(
        diagnostics(
            "interface I { [s:string]:{length:number}; [n:number]:number; }",
            false,
            2413
        )
        .len(),
        1
    );
    assert_eq!(
        diagnostics(
            "interface I { [s:string]:{length:string}; [n:number]:string; }",
            false,
            2413
        )
        .len(),
        1
    );
    assert_eq!(
        diagnostics(
            "interface I { [s:string]:{unknown?:number}; [n:number]:string; }",
            false,
            2413
        )
        .len(),
        1
    );
}

#[test]
fn empty_index_values_do_not_satisfy_required_any_properties() {
    let source = "interface A { [s:string]:{a;}; } interface B { [n:number]:{}; } interface C extends A,B {}";
    assert_eq!(diagnostics(source, false, 2413).len(), 1);
}

#[test]
fn inherited_properties_are_checked_where_interface_bases_are_combined() {
    for (source, span) in [
        ("interface A { [k:string]:number; } interface B { field:string; } interface C extends A,B {} interface D extends C {}", "C"),
        ("interface A { field:string; } interface B extends A { [k:string]:number; }", "[k:string]:number;"),
        ("interface A<T> { field:T; } interface B extends A<string> { [k:string]:number; }", "[k:string]:number;"),
        ("class A { field:string; } interface B extends A { [k:string]:number; }", "[k:string]:number;"),
    ] {
        assert_eq!(errors(source, false), [(span.into(), "Property 'field' of type 'string' is not assignable to 'string' index type 'number'.".into())], "{source}");
    }
    let source = "interface A { [k:string]:{a:any}; } interface B { [k:number]:{a:any,b:any}; } interface P { 0:{}; } interface C extends A,B,P {} interface D extends A,C {}";
    assert_eq!(errors(source, false).len(), 2);
    for (source, expected) in [
        ("interface A { [k:string]:number; field:string; } interface B extends A {} interface C extends A,B {}", 1),
        ("interface A { [k:string]:number; } interface B { field:string; } interface C extends A,B { field:number; }", 0),
    ] {
        assert_eq!(errors(source, false).len(), expected, "{source}");
    }
}

#[test]
fn inherited_symbol_properties_keep_their_key_domain() {
    for declaration in [
        "class B { [key] = ''; } class D extends B",
        "interface B { [key]:string; } interface D extends B",
    ] {
        let source =
            format!("declare const key:unique symbol; {declaration} {{ [s:string]:number; }}");
        assert!(errors(&source, false).is_empty(), "{source}");
        let source =
            format!("declare const key:unique symbol; {declaration} {{ [s:symbol]:number; }}");
        assert_eq!(errors(&source, false).len(), 1, "{source}");
    }
}

#[test]
fn inherited_index_diagnostics_at_one_location_follow_message_order() {
    let source = "interface A { [s:string]:{a:any}; } interface B { [n:number]:{a:any,b:any}; } interface P { m:{}; 0:{}; } interface C extends A,B,P {}";
    let messages: Vec<_> = errors(source, false)
        .into_iter()
        .map(|(_, message)| message)
        .collect();
    assert_eq!(messages.len(), 3);
    assert!(messages[0].starts_with("Property '0'") && messages[0].contains("'number' index"));
    assert!(messages[1].starts_with("Property '0'") && messages[1].contains("'string' index"));
    assert!(messages[2].starts_with("Property 'm'"));
}

#[test]
fn well_known_symbol_members_are_checked_against_symbol_indices() {
    for source in [
        "interface I { [Symbol.iterator]:number; [s:symbol]:string; }",
        "class C { [Symbol.iterator] = 1; [s:symbol]:string; }",
        "class Base { [s:symbol]:string; } class C extends Base { [Symbol.iterator] = 1; }",
    ] {
        let result = errors(source, false);
        assert_eq!(result.len(), 1, "{source}: {result:?}");
        assert!(
            result[0]
                .1
                .contains("of type 'number' is not assignable to 'symbol' index type 'string'"),
            "{result:?}"
        );
    }
    for source in [
        "interface I { [Symbol.iterator]:number; [s:string]:string; }",
        "class C { [Symbol.iterator] = 1; [s:string]:string; }",
        "interface I { [Symbol.iterator]:number; [s:symbol]:number; }",
        "class C { [Symbol.iterator] = 1; [s:symbol]:number; }",
        "interface I { ['[Symbol.iterator]']:number; [s:symbol]:string; }",
        "class C { ['[Symbol.iterator]'] = 1; [s:symbol]:string; }",
        "function f(Symbol: { iterator: 'key' }) { interface I { [Symbol.iterator]:number; [s:symbol]:string; } }",
        "function f(Symbol: { iterator: 'key' }) { class C { [Symbol.iterator] = 1; [s:symbol]:string; } }",
        "export {}; const Symbol = { iterator: 'key' as const }; interface I { [Symbol.iterator]:number; [s:symbol]:string; }",
        "export {}; const Symbol = { iterator: 'key' as const }; class C { [Symbol.iterator] = 1; [s:symbol]:string; }",
    ] {
        assert!(errors(source, false).is_empty(), "{source}");
    }
}

#[test]
fn inherited_computed_member_diagnostics_retain_the_declaration_origin() {
    for (base, derived, name) in [
        (
            "class Base { get ['value']():string { return ''; } }",
            "class Derived extends Base { [s:string]:number; }",
            "['value']",
        ),
        (
            "class Base { static get ['value']():string { return ''; } }",
            "class Derived extends Base { static [s:string]:number; }",
            "['value']",
        ),
        (
            "class Base { [Symbol.iterator]():string { return ''; } }",
            "class Derived extends Base { [s:symbol]:number; }",
            "[Symbol.iterator]",
        ),
        (
            "declare const key:unique symbol; class Base { [key] = ''; }",
            "class Derived extends Base { [s:symbol]:number; }",
            "[key]",
        ),
    ] {
        for separate_files in [false, true] {
            let source = if separate_files {
                derived.to_string()
            } else {
                format!("{base}\n{derived}")
            };
            let base_file = tsc_rs_parser::parse("base.ts", base);
            let file = tsc_rs_parser::parse("derived.ts", &source);
            assert!(base_file.diagnostics.is_empty());
            assert!(file.diagnostics.is_empty());
            let mut checker = TypeChecker::new();
            if separate_files {
                checker.inject_external_types(&[&base_file]);
            }
            let result = checker.check_with_options(
                &file,
                &tsc_rs_symbols::bind(&file),
                &CompilerOptions {
                    strict: Some(false),
                    ..CompilerOptions::default()
                },
            );
            let errors: Vec<_> = result
                .diagnostics
                .iter()
                .filter(|d| d.code == 2411)
                .collect();
            assert_eq!(errors.len(), 1, "{base}: {separate_files}: {errors:?}");
            let error = errors[0];
            assert!(
                error.message.starts_with(&format!("Property '{name}' ")),
                "{error:?}"
            );
            let related = error.related.as_ref().unwrap();
            assert_eq!(related.len(), 1);
            assert_eq!(related[0].code, 2728);
            assert_eq!(related[0].message, format!("'{name}' is declared here."));
            assert_eq!(
                related[0].file_name.as_deref(),
                Some(if separate_files {
                    "base.ts"
                } else {
                    "derived.ts"
                })
            );
            let span = related[0].span.unwrap();
            assert_eq!(&base[span.start as usize..span.end as usize], name);
        }
    }
}

#[test]
fn ordinary_inherited_members_do_not_add_computed_declaration_notes() {
    let source = "class Base { value = ''; } class Derived extends Base { [s:string]:number; }";
    let file = tsc_rs_parser::parse("ordinary.ts", source);
    let result = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == 2411)
        .collect();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].related.is_none());
}
