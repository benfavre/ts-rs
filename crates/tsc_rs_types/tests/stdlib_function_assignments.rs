use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str, options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("functions.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
}

#[test]
fn standard_function_declarations_reject_assignment_and_updates() {
    for name in [
        "eval",
        "parseInt",
        "parseFloat",
        "isNaN",
        "decodeURI",
        "setTimeout",
    ] {
        for operation in [
            format!("{name} = 1"),
            format!("{name} += 1"),
            format!("++({name})"),
        ] {
            let errors = diagnostics(
                &operation,
                CompilerOptions {
                    always_strict: Some(false),
                    ..CompilerOptions::default()
                },
            );
            assert_eq!(errors.len(), 1, "{operation}: {errors:?}");
            assert_eq!(errors[0].code, 2630, "{operation}: {errors:?}");
            assert_eq!(
                errors[0].message,
                format!("Cannot assign to '{name}' because it is a function.")
            );
            let span = errors[0].span.unwrap();
            assert_eq!(&operation[span.start as usize..span.end as usize], name);
        }
    }
}

#[test]
fn local_variables_parameters_and_function_values_stay_assignable() {
    for source in [
        "function f(parseInt: number) { parseInt = 1; parseInt++; }",
        "function f() { let parseFloat = 0; parseFloat++; }",
        "function f() { let eval = 0; eval++; }",
        "export {}; let parseInt = (text: string) => 0; parseInt = text => 1;",
        "NaN++; Infinity--; Number = Number;",
        "(eval as any) = 1; (parseInt as any)++;",
    ] {
        let errors = diagnostics(
            source,
            CompilerOptions {
                always_strict: Some(false),
                ..CompilerOptions::default()
            },
        );
        assert!(errors.is_empty(), "{source}: {errors:?}");
    }
}

#[test]
fn function_classification_follows_the_selected_libraries() {
    let source = "setTimeout = 1; parseInt = 1;";
    let errors = diagnostics(
        source,
        CompilerOptions {
            lib: vec!["es5".to_string()],
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    let function_errors: Vec<_> = errors
        .iter()
        .filter(|diagnostic| diagnostic.code == 2630)
        .collect();
    assert_eq!(function_errors.len(), 1, "{errors:?}");
    let span = function_errors[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "parseInt");
}

#[test]
fn conflicting_script_globals_do_not_shadow_library_function_symbols() {
    for source in [
        "var parseInt: any; parseInt = 1;",
        "let parseInt = 0; parseInt++;",
    ] {
        let errors = diagnostics(source, CompilerOptions::default());
        let writes: Vec<_> = errors
            .iter()
            .filter(|diagnostic| diagnostic.code == 2630)
            .collect();
        assert_eq!(writes.len(), 1, "{source}: {errors:?}");
        let span = writes[0].span.unwrap();
        assert_eq!(span.start as usize, source.rfind("parseInt").unwrap());
    }
}
