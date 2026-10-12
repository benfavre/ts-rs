use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn strict_errors(source: &str, always_strict: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("strict_uses.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
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
        .filter(|diagnostic| matches!(diagnostic.code, 1100 | 1210 | 1215))
        .collect()
}

fn assert_strict_names(source: &str, always_strict: bool, positions: &[(usize, &str)]) {
    let errors = strict_errors(source, always_strict);
    assert_eq!(errors.len(), positions.len(), "{source}: {errors:?}");
    for (error, (start, name)) in errors.iter().zip(positions) {
        assert_eq!(error.code, 1100, "{source}: {error:?}");
        assert_eq!(
            error.message,
            format!("Invalid use of '{name}' in strict mode.")
        );
        let span = error.span.unwrap();
        assert_eq!(span.start as usize, *start, "{source}: {error:?}");
        assert_eq!(&source[*start..span.end as usize], *name);
    }
}

#[test]
fn assignments_and_updates_follow_the_enclosing_function_directive() {
    for name in ["eval", "arguments"] {
        for operation in [
            format!("{name} = 1"),
            format!("{name} += 1"),
            format!("{name}++"),
            format!("{name}--"),
            format!("++{name}"),
            format!("--{name}"),
        ] {
            let source = format!("function f({name}: any) {{ 'use strict'; {operation}; }}");
            assert_strict_names(&source, false, &[(source.rfind(name).unwrap(), name)]);
            assert_strict_names(&source.replace("'use strict';", ""), false, &[]);
        }
    }
}

#[test]
fn function_declarations_and_expressions_use_the_enclosing_strict_context() {
    for name in ["eval", "arguments"] {
        for declaration in [
            format!("function {name}() {{}}"),
            format!("var f = function {name}() {{}};"),
        ] {
            assert_strict_names(
                &declaration,
                true,
                &[(declaration.find(name).unwrap(), name)],
            );
            let source = format!("'use strict'; {declaration}");
            assert_strict_names(&source, false, &[(source.find(name).unwrap(), name)]);
            assert_strict_names(&declaration, false, &[]);
        }
    }
}

#[test]
fn catch_bindings_include_nested_destructuring() {
    for pattern in ["eval", "{value: eval}", "{eval = 1}", "[eval]"] {
        let source = format!("'use strict'; try {{}} catch ({pattern}) {{}}");
        assert_strict_names(&source, false, &[(source.find("eval").unwrap(), "eval")]);
        assert_strict_names(&source.replace("'use strict';", ""), false, &[]);
    }
}

#[test]
fn nested_strict_bindings_do_not_make_sibling_functions_strict() {
    let source = "function f() { 'use strict'; let eval = 0; eval++; } function g() { let eval = 0; eval++; }";
    let names: Vec<_> = source.match_indices("eval").take(2).collect();
    assert_strict_names(source, false, &names);
    assert_strict_names(
        "'use strict'; let eval = 0; eval = 1;",
        false,
        &[(18, "eval"), (28, "eval")],
    );
}

#[test]
fn body_directives_do_not_retroactively_change_function_names_or_parameters() {
    assert_strict_names(
        "function eval() { 'use strict'; } var f = function arguments() { 'use strict'; }; function g(eval: any) { 'use strict'; return eval; }",
        false,
        &[],
    );
    assert_strict_names(
        "function f() { 'use\\x20strict'; eval++; } { 'use strict'; eval++; }",
        false,
        &[],
    );
}

#[test]
fn wrapped_targets_reads_and_property_names_do_not_trigger_binding_errors() {
    assert_strict_names(
        "function f(eval: any) { 'use strict'; (eval)++; (eval) = 1; (eval!)++; (eval as any)++; eval(''); const obj = {eval: 0}; obj.eval++; }",
        false,
        &[],
    );
}

#[test]
fn ambient_function_names_are_exempt_but_variable_bindings_are_checked() {
    assert_strict_names(
        "'use strict'; declare function eval(): void; declare namespace N { function arguments(): void; }",
        false,
        &[],
    );
    let source = "'use strict'; declare var eval: any;";
    assert_strict_names(source, false, &[(source.find("eval").unwrap(), "eval")]);
}
