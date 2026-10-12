use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn strict_errors(source: &str, always_strict: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("contexts.ts", source);
    assert!(
        file.diagnostics.is_empty(),
        "{source}: {:?}",
        file.diagnostics
    );
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                strict: Some(false),
                always_strict: Some(always_strict),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.code, 1100 | 1210 | 1212..=1215))
        .collect()
}

fn assert_names(source: &str, expected: &[(u32, &str)], always_strict: bool) {
    let errors = strict_errors(source, always_strict);
    assert_eq!(errors.len(), expected.len(), "{source}: {errors:?}");
    for (error, (code, name)) in errors.iter().zip(expected) {
        assert_eq!(error.code, *code, "{source}: {error:?}");
        let span = error.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], *name);
        let message = match code {
            1213 => format!("Identifier expected. '{name}' is a reserved word in strict mode. Class definitions are automatically in strict mode."),
            1214 => format!("Identifier expected. '{name}' is a reserved word in strict mode. Modules are automatically in strict mode."),
            1215 => format!("Invalid use of '{name}'. Modules are automatically in strict mode."),
            1210 => format!("Code contained in a class is evaluated in JavaScript's strict mode which does not allow this use of '{name}'. For more information, see https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Strict_mode."),
            _ => panic!("unexpected strict diagnostic: {error:?}"),
        };
        assert_eq!(error.message, message);
    }
}

#[test]
fn class_names_heritage_computed_keys_and_signatures_are_strict() {
    assert_names(
        "class C extends public { [private]: protected; m(package: static) { return package; } }",
        &[
            (1213, "public"),
            (1213, "private"),
            (1213, "protected"),
            (1213, "package"),
            (1213, "static"),
            (1213, "package"),
        ],
        false,
    );
    assert_names(
        "class public {} const C = class static {};",
        &[(1213, "public"), (1213, "static")],
        false,
    );
}

#[test]
fn modules_are_strict_even_when_always_strict_is_disabled() {
    assert_names(
        "export {}; var public = 1; function f(private: number) { return private; }",
        &[(1214, "public"), (1214, "private"), (1214, "private")],
        false,
    );
    assert_names(
        "export {}; function eval() {} eval++; let arguments = 0; arguments = 1;",
        &[
            (1215, "eval"),
            (1215, "eval"),
            (1215, "arguments"),
            (1215, "arguments"),
        ],
        false,
    );
}

#[test]
fn class_context_takes_priority_over_module_and_file_strictness() {
    let class = "class C { m(eval: any) { eval++; } n() { try {} catch (arguments) {} function eval() {} } }";
    let expected = [
        (1210, "eval"),
        (1210, "eval"),
        (1210, "arguments"),
        (1210, "eval"),
    ];
    assert_names(class, &expected, false);
    assert_names(class, &expected, true);
    assert_names(&format!("export {{}}; {class}"), &expected, false);
    assert_names(
        "export {}; class C { m(public: any) { return public; } }",
        &[(1213, "public"), (1213, "public")],
        false,
    );
}

#[test]
fn class_strictness_propagates_to_nested_functions_and_static_blocks() {
    assert_names(
        "class C { static { let eval = 0; eval++; } m() { function f() { let arguments = 0; arguments++; } } }",
        &[(1210, "eval"), (1210, "eval"), (1210, "arguments"), (1210, "arguments")],
        false,
    );
}

#[test]
fn strictness_does_not_leak_out_of_a_class_and_property_names_stay_legal() {
    assert_names(
        "var public = 0; class C { public = 1; m() { return this.public; } } var private = 0; const obj = {public: 1};",
        &[],
        false,
    );
    assert_names(
        "export {}; const obj = {public: 1}; obj.public;",
        &[],
        false,
    );
}

#[test]
fn ambient_classes_and_ambient_function_parameters_are_exempt() {
    assert_names(
        "declare class C { m(public: any, eval: any): void; [private]: any; }",
        &[],
        false,
    );
    assert_names(
        "export {}; declare function eval(arguments: any): void;",
        &[],
        false,
    );
}

#[test]
fn constructor_accessor_and_destructured_bindings_use_class_diagnostics() {
    assert_names(
        "class C { constructor({eval}: any) {} set value(arguments: any) {} m() { const {value: eval} = {value: 0}; } }",
        &[(1210, "eval"), (1210, "arguments"), (1210, "eval")],
        false,
    );
}
