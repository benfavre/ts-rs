use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn ts1212(source: &str, options: CompilerOptions) -> Vec<(u32, String)> {
    let file = tsc_rs_parser::parse("strict_reserved_words.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code == 1212)
        .map(|diagnostic| {
            (
                diagnostic.span.expect("TS1212 must have a span").start,
                diagnostic.message,
            )
        })
        .collect()
}

#[test]
fn always_strict_checks_bindings_references_and_type_positions() {
    let source = r#"
var public = 1;
function f<implements>(private: package) {
    let { value: protected } = source;
    return public + protected;
}
const object = { public: 1 };
type T = public.member;
"#;
    let diagnostics = ts1212(
        source,
        CompilerOptions {
            always_strict: Some(true),
            ..CompilerOptions::default()
        },
    );

    assert_eq!(diagnostics.len(), 8, "{diagnostics:#?}");
    assert!(diagnostics
        .iter()
        .any(|(_, message)| message.contains("'public'")));
    assert!(diagnostics
        .iter()
        .any(|(_, message)| message.contains("'implements'")));
    assert!(diagnostics
        .iter()
        .any(|(_, message)| message.contains("'protected'")));
}

#[test]
fn nested_use_strict_applies_to_the_function_signature_and_body_only() {
    let source = r#"
var public = 0;
function f(private) {
    "use strict";
    return private;
}
var protected = 0;
"#;
    let diagnostics = ts1212(
        source,
        CompilerOptions {
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
    );

    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics
        .iter()
        .all(|(_, message)| message.contains("'private'")));
}

#[test]
fn es2015_reserves_let_without_enabling_all_strict_reserved_words() {
    let source = "var let = 1; let = 2; var public = 3;";
    let es2015 = ts1212(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    let es5 = ts1212(
        source,
        CompilerOptions {
            target: Some(ScriptTarget::ES5),
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );

    assert_eq!(es2015.len(), 2, "{es2015:#?}");
    assert!(es5.is_empty(), "{es5:#?}");
}

#[test]
fn property_names_classes_and_external_modules_do_not_get_generic_ts1212() {
    let property_names = ts1212(
        r#"var object = { public: 1, ["private"]: 2 }; object.protected;"#,
        CompilerOptions {
            always_strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    let class = ts1212(
        "class C { method(public: private) { return public; } }",
        CompilerOptions::default(),
    );
    let module = ts1212(
        "export {}; var public = 1;",
        CompilerOptions {
            always_strict: Some(true),
            ..CompilerOptions::default()
        },
    );

    assert!(property_names.is_empty(), "{property_names:#?}");
    assert!(class.is_empty(), "{class:#?}");
    assert!(module.is_empty(), "{module:#?}");
}

#[test]
fn typescript_declaration_names_reject_strict_reserved_words() {
    let diagnostics = ts1212(
        "interface public {}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );

    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0].1.contains("'public'"));
}

#[test]
fn escaped_directives_and_explicit_always_strict_false_remain_sloppy() {
    let escaped = ts1212(
        r#""use\x20strict"; var public = 1;"#,
        CompilerOptions {
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    let overridden = ts1212(
        "var public = 1; interface public {}",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(true),
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
    );

    assert!(escaped.is_empty(), "{escaped:#?}");
    assert!(overridden.is_empty(), "{overridden:#?}");
}

#[test]
fn parser_recovery_suppresses_strict_name_cascades() {
    let diagnostics = ts1212(
        r#""use strict"; var public = 1; const value = ;"#,
        CompilerOptions::default(),
    );

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn generator_context_does_not_leak_into_nested_functions_or_arrows() {
    let source = r#"
function* outer() {
    yield;
    function inner() { yield; }
    const arrow = () => yield;
}
"#;
    let parsed = tsc_rs_parser::parse("strict_reserved_words.ts", source);
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let diagnostics = ts1212(
        source,
        CompilerOptions {
            always_strict: Some(true),
            ..CompilerOptions::default()
        },
    );

    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    assert!(diagnostics
        .iter()
        .all(|(_, message)| message.contains("'yield'")));
}

#[test]
fn namespace_directive_enables_strict_mode_for_its_body() {
    let diagnostics = ts1212(
        r#"namespace N { "use strict"; var public = 1; }"#,
        CompilerOptions {
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
    );

    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0].1.contains("'public'"));
}

#[test]
fn non_generator_yield_expression_keeps_ts1163_authoritative() {
    let diagnostics = ts1212(
        "function f() { yield 'literal'; }",
        CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            ..CompilerOptions::default()
        },
    );

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}
