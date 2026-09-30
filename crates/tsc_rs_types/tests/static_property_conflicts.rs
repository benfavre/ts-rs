use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(file_name: &str, source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code == 2699)
        .collect()
}

#[test]
fn define_semantics_control_non_prototype_conflicts() {
    let source = "class C { static name: string; static length: number; static caller: any; static arguments: any; static prototype: any; }";
    for target in [
        None,
        Some(ScriptTarget::ES5),
        Some(ScriptTarget::ES2015),
        Some(ScriptTarget::ES2021),
        Some(ScriptTarget::ES2022),
        Some(ScriptTarget::ESNext),
    ] {
        for define in [None, Some(false), Some(true)] {
            let diagnostics = check(
                "static.ts",
                source,
                CompilerOptions {
                    target,
                    use_define_for_class_fields: define,
                    ..CompilerOptions::default()
                },
            );
            let define =
                define.unwrap_or(target.unwrap_or(ScriptTarget::ES2025) >= ScriptTarget::ES2022);
            let names: Vec<_> = diagnostics
                .iter()
                .map(|diagnostic| {
                    let span = diagnostic.span.unwrap();
                    &source[span.start as usize..span.end as usize]
                })
                .collect();
            assert_eq!(
                names,
                if define {
                    vec!["prototype"]
                } else {
                    vec!["name", "length", "caller", "arguments", "prototype"]
                },
                "{target:?}, define={define}"
            );
        }
    }
}

#[test]
fn computed_string_names_preserve_diagnostic_spans() {
    let source = r#"const keys = { name: 'name', prototype: 'prototype' } as const;
class Named {
    static "length"() {}
    static ['caller']: any;
    static [keys.name]: string;
    static [keys.prototype]: any;
}"#;
    let diagnostics = check(
        "static.ts",
        source,
        CompilerOptions {
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.unwrap();
            (
                diagnostic.message.clone(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    let expected: Vec<_> = [("length", "\"length\""), ("caller", "['caller']"), ("name", "[keys.name]"), ("prototype", "[keys.prototype]")].into_iter().map(|(name, span)| (format!("Static property '{name}' conflicts with built-in property 'Function.{name}' of constructor function 'Named'."), span)).collect();
    assert_eq!(actual, expected);
}

#[test]
fn methods_accessors_and_declared_fields_are_checked() {
    let source = "class C { static name() {} static get length() { return 1; } static set length(value: number) {} declare static caller: any; static accessor arguments: any; }";
    let diagnostics = check(
        "static.ts",
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2022),
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(diagnostics.len(), 5, "{diagnostics:#?}");
}

#[test]
fn ambient_classes_and_instance_or_private_members_are_exempt() {
    let source = "declare class Ambient { static prototype: any; static name: string; } declare namespace N { class C { static prototype: any; } } class Actual { prototype: any; name: any; static #name: any; }";
    assert!(check(
        "static.ts",
        source,
        CompilerOptions {
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        }
    )
    .is_empty());
    assert!(check(
        "static.d.ts",
        "declare class C { static prototype: any; static name: string; }",
        CompilerOptions::default()
    )
    .is_empty());
}

#[test]
fn class_expression_names_and_default_exports_are_preserved() {
    let source = "const Bound = class { static prototype: any; }; const Outer = class Inner { static prototype: any; }; (class { static prototype: any; }); export default class { static prototype: any; }";
    let diagnostics = check("static.ts", source, CompilerOptions::default());
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    let expected: Vec<_> = ["Bound", "Inner", "(Anonymous class)", "default"].into_iter().map(|name| format!("Static property 'prototype' conflicts with built-in property 'Function.prototype' of constructor function '{name}'.")).collect();
    assert_eq!(actual, expected);
}

#[test]
fn unknown_computed_names_do_not_become_conflicts() {
    let source = "declare const key: string; class C { static [key]: any; static safe: any; }";
    assert!(check(
        "static.ts",
        source,
        CompilerOptions {
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        }
    )
    .is_empty());
}

#[test]
fn no_check_disables_static_property_diagnostics() {
    assert!(check(
        "static.ts",
        "class C { static prototype: any; }",
        CompilerOptions {
            no_check: Some(true),
            ..CompilerOptions::default()
        }
    )
    .is_empty());
}

#[test]
fn prototype_methods_and_accessors_also_conflict_with_the_implicit_symbol() {
    let source = "const key = 'prototype'; class C { static [key]() {} } class D { static get prototype() { return {}; } static set prototype(value: any) {} } declare class E { static prototype(): void; } class F { static accessor prototype: any; } class G { static prototype: any; }";
    let file = tsc_rs_parser::parse("static.ts", source);
    assert!(file.diagnostics.is_empty(), "{:#?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    let duplicates: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2300)
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert_eq!(
        duplicates,
        [
            "Duplicate identifier '[key]'.",
            "Duplicate identifier 'prototype'.",
            "Duplicate identifier 'prototype'.",
            "Duplicate identifier 'prototype'.",
            "Duplicate identifier 'prototype'."
        ]
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2699)
            .count(),
        5
    );
}
