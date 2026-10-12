use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

const CORE_GLOBALS: &str = "interface Array<T> {} interface Boolean {} interface Function {} interface IArguments {} interface Number {} interface Object {} interface RegExp {} interface String {}";
const CORE_NAMES: &[&str] = &[
    "Array",
    "Boolean",
    "Function",
    "IArguments",
    "Number",
    "Object",
    "RegExp",
    "String",
];

fn check(source: &str, providers: &[&str], options: CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("subject.ts", source);
    let providers: Vec<_> = providers
        .iter()
        .enumerate()
        .map(|(i, source)| tsc_rs_parser::parse(&format!("provider{i}.ts"), source))
        .collect();
    let mut checker = TypeChecker::new();
    if !providers.is_empty() {
        let mut files: Vec<_> = providers.iter().collect();
        files.push(&file);
        checker.inject_external_types(&files);
        checker.take_diagnostics();
    }
    checker
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                no_lib: Some(true),
                ..options
            },
        )
        .diagnostics
}

fn assert_missing(diagnostics: &[Diagnostic], names: &[&str]) {
    assert_eq!(diagnostics.len(), names.len(), "{diagnostics:?}");
    for (diagnostic, name) in diagnostics.iter().zip(names) {
        assert_eq!(diagnostic.code, 2318, "{diagnostic:?}");
        assert_eq!(
            diagnostic.message,
            format!("Cannot find global type '{name}'.")
        );
        assert!(diagnostic.span.is_none(), "{diagnostic:?}");
        assert!(diagnostic.file_name.is_none(), "{diagnostic:?}");
    }
}

#[test]
fn explicit_lib_with_no_lib_still_reports_the_required_globals() {
    let file = tsc_rs_parser::parse("nolib_globals.ts", "");
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                no_lib: Some(true),
                lib: vec!["es5".to_string()],
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    let missing_globals: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2318)
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();

    assert_eq!(
        missing_globals,
        [
            "Cannot find global type 'Array'.",
            "Cannot find global type 'Boolean'.",
            "Cannot find global type 'CallableFunction'.",
            "Cannot find global type 'Function'.",
            "Cannot find global type 'IArguments'.",
            "Cannot find global type 'NewableFunction'.",
            "Cannot find global type 'Number'.",
            "Cannot find global type 'Object'.",
            "Cannot find global type 'RegExp'.",
            "Cannot find global type 'String'.",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn strict_bind_call_apply_controls_function_global_requirements() {
    for (strict, child, extra) in [
        (Some(false), None, false),
        (Some(true), None, true),
        (None, None, true),
        (Some(true), Some(false), false),
        (Some(false), Some(true), true),
    ] {
        let errors = check(
            CORE_GLOBALS,
            &[],
            CompilerOptions {
                strict,
                strict_bind_call_apply: child,
                ..CompilerOptions::default()
            },
        );
        assert_missing(
            &errors,
            if extra {
                &["CallableFunction", "NewableFunction"]
            } else {
                &[]
            },
        );
    }
}

#[test]
fn globals_supplied_by_another_file_satisfy_the_program_requirements() {
    let errors = check("export {};", &[CORE_GLOBALS], CompilerOptions::default());
    assert_missing(&errors, &["CallableFunction", "NewableFunction"]);
    let errors = check(
        "export {};",
        &[
            CORE_GLOBALS,
            "interface CallableFunction {} interface NewableFunction {}",
        ],
        CompilerOptions::default(),
    );
    assert_missing(&errors, &[]);
}

#[test]
fn module_and_namespace_types_do_not_supply_global_types() {
    for source in [
        format!("export {{}}; {CORE_GLOBALS}"),
        format!("export {{}}; {CORE_GLOBALS} declare global {{}}"),
        format!("namespace Local {{ {CORE_GLOBALS} }}"),
        format!("function local() {{ {CORE_GLOBALS} }}"),
    ] {
        for inject in [false, true] {
            let errors = check(
                &source,
                if inject { &[""] } else { &[] },
                CompilerOptions {
                    strict: Some(false),
                    ..CompilerOptions::default()
                },
            );
            assert_missing(&errors, CORE_NAMES);
        }
    }
}

#[test]
fn invalid_script_global_augmentation_does_not_supply_types() {
    let errors = check(
        &format!("declare global {{ {CORE_GLOBALS} }}"),
        &[],
        CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(errors.len(), CORE_NAMES.len() + 1, "{errors:?}");
    assert_missing(&errors[..CORE_NAMES.len()], CORE_NAMES);
    assert_eq!(errors.last().unwrap().code, 2669);
}

#[test]
fn global_augmentations_supply_required_types() {
    let source = format!("export {{}}; declare global {{ {CORE_GLOBALS} }}");
    for providers in [&[][..], &[""][..]] {
        assert_missing(
            &check(
                &source,
                providers,
                CompilerOptions {
                    strict: Some(false),
                    ..CompilerOptions::default()
                },
            ),
            &[],
        );
    }
    assert_missing(
        &check("export {};", &[&source], CompilerOptions::default()),
        &["CallableFunction", "NewableFunction"],
    );
}
