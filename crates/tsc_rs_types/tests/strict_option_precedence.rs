use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn codes(source: &str, options: CompilerOptions) -> Vec<u32> {
    let file = tsc_rs_parser::parse("strict_options.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn implicit_any_explicit_setting_overrides_strict() {
    for strict in [None, Some(false), Some(true)] {
        for no_implicit_any in [None, Some(false), Some(true)] {
            let expected = if no_implicit_any.unwrap_or(strict != Some(false)) {
                vec![7006]
            } else {
                vec![]
            };
            assert_eq!(
                codes(
                    "function f(value) {}",
                    CompilerOptions {
                        strict,
                        no_implicit_any,
                        ..CompilerOptions::default()
                    }
                ),
                expected,
                "strict={strict:?}, noImplicitAny={no_implicit_any:?}"
            );
        }
    }
}

#[test]
fn null_checks_explicit_setting_controls_nullability_and_definite_assignment() {
    for strict in [None, Some(false), Some(true)] {
        for strict_null_checks in [None, Some(false), Some(true)] {
            let enabled = strict_null_checks.unwrap_or(strict != Some(false));
            let options = CompilerOptions {
                strict,
                strict_null_checks,
                ..CompilerOptions::default()
            };
            assert_eq!(
                codes("let n: number = null;", options.clone()),
                if enabled { vec![2322] } else { vec![] },
                "{options:?}"
            );
            assert_eq!(
                codes("function f() { let n: number; return n; }", options.clone()),
                if enabled { vec![2454] } else { vec![] },
                "{options:?}"
            );
        }
    }
}

#[test]
fn function_variance_uses_explicit_setting() {
    let source = "declare let acceptsString: (value: string) => void; declare let acceptsLiteral: (value: 'a') => void; acceptsString = acceptsLiteral;";
    for strict in [None, Some(false), Some(true)] {
        for strict_function_types in [None, Some(false), Some(true)] {
            let expected = if strict_function_types.unwrap_or(strict != Some(false)) {
                vec![2322]
            } else {
                vec![]
            };
            let options = CompilerOptions {
                strict,
                strict_function_types,
                ..CompilerOptions::default()
            };
            assert_eq!(codes(source, options.clone()), expected, "{options:?}");
        }
    }
}

#[test]
fn property_initialization_requires_both_effective_settings() {
    for strict in [None, Some(false), Some(true)] {
        for strict_null_checks in [None, Some(false), Some(true)] {
            for strict_property_initialization in [None, Some(false), Some(true)] {
                let enabled = strict_null_checks.unwrap_or(strict != Some(false))
                    && strict_property_initialization.unwrap_or(strict != Some(false));
                let options = CompilerOptions {
                    strict,
                    strict_null_checks,
                    strict_property_initialization,
                    ..CompilerOptions::default()
                };
                assert_eq!(
                    codes("class C { value: number; }", options.clone()),
                    if enabled { vec![2564] } else { vec![] },
                    "{options:?}"
                );
            }
        }
    }
}

#[test]
fn always_strict_setting_overrides_umbrella_but_not_source_directives() {
    for strict in [None, Some(false), Some(true)] {
        for always_strict in [None, Some(false), Some(true)] {
            let enabled = always_strict.unwrap_or(strict != Some(false));
            let options = CompilerOptions {
                strict,
                always_strict,
                ..CompilerOptions::default()
            };
            assert_eq!(
                codes("var eval = 1;", options.clone()),
                if enabled { vec![1100] } else { vec![] },
                "{options:?}"
            );
            assert_eq!(
                codes("'use strict'; var eval = 1;", options.clone()),
                vec![1100],
                "{options:?}"
            );
        }
    }
}

#[test]
fn strict_reserved_words_follow_options_and_ambient_context() {
    use tsc_rs_ast::ScriptTarget;
    for always_strict in [None, Some(false), Some(true)] {
        let options = CompilerOptions {
            strict: Some(true),
            always_strict,
            target: Some(ScriptTarget::ES2015),
            ..CompilerOptions::default()
        };
        assert_eq!(
            codes("var implements = 1;", options.clone()),
            if always_strict == Some(false) {
                vec![]
            } else {
                vec![1212]
            },
            "{options:?}"
        );
        assert!(codes(
            "declare var implements: number; declare namespace N { var private: number; }",
            options.clone()
        )
        .is_empty());
        assert!(codes(
            "declare interface implements {} declare type private = number;",
            options.clone()
        )
        .is_empty());
        let source = tsc_rs_parser::parse(
            "ambient.d.ts",
            "declare var eval: number; declare var implements: number;",
        );
        let symbols = tsc_rs_symbols::bind(&source);
        let diagnostics = TypeChecker::new()
            .check_with_options(&source, &symbols, &options)
            .diagnostics;
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
