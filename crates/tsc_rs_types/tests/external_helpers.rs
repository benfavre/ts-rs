use tsc_rs_ast::{CompilerOptions, ModuleKind, ScriptTarget};
use tsc_rs_types::TypeChecker;

/// (code, text at span) of TS2343/TS2354 for `source` checked as `main.ts`
/// with `importHelpers`, where 'tslib' exports `helpers` (or is missing).
fn helper_errors(
    source: &str,
    helpers: Option<&[&str]>,
    target: ScriptTarget,
) -> Vec<(u32, String)> {
    let file = tsc_rs_parser::parse("main.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    match helpers {
        Some(names) => {
            let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            checker.register_helpers_module_exports("ambient:tslib", &names);
            checker.register_helpers_module("main.ts", Some("ambient:tslib".into()));
        }
        None => checker.register_helpers_module("main.ts", None),
    }
    checker.set_current_file_name("main.ts");
    let options = CompilerOptions {
        import_helpers: Some(true),
        target: Some(target),
        module: Some(ModuleKind::CommonJS),
        ..CompilerOptions::default()
    };
    checker
        .check_with_options(&file, &symbols, &options)
        .diagnostics
        .into_iter()
        .filter(|d| matches!(d.code, 2343 | 2354))
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
fn missing_tslib_is_reported_once_at_the_first_request() {
    let source = "export {};\nasync function foo() {}\nasync function bar() {}";
    assert_eq!(
        helper_errors(source, None, ScriptTarget::ES2015),
        [(2354, "foo".into())]
    );
}

#[test]
fn each_missing_helper_is_reported_once() {
    let source =
        "export * from './other';\nconst o = { a: 1 };\nconst { ...x } = o;\nconst { ...y } = o;";
    assert_eq!(
        helper_errors(source, Some(&["__exportStar"]), ScriptTarget::ES2017),
        [(2343, "x".into())]
    );
    // Nothing is requested at a target that needs no helper.
    assert!(helper_errors(source, Some(&[]), ScriptTarget::ES2018)
        .iter()
        .all(|(_, text)| text.starts_with("export")));
}

#[test]
fn scripts_request_no_helpers() {
    assert!(helper_errors("async function foo() {}", None, ScriptTarget::ES2015).is_empty());
}

#[test]
fn jsx_factory_namespace_must_be_in_scope() {
    let source = "declare namespace JSX { interface IntrinsicElements { [s: string]: any } }\nconst a = <foo data />;";
    let file = tsc_rs_parser::parse_with_jsx("main.tsx", source, true);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        jsx: Some(tsc_rs_ast::JsxEmit::React),
        jsx_factory: Some("myReactLib.createElement".into()),
        ..CompilerOptions::default()
    };
    let found: Vec<_> = TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == 2874)
        .map(|d| d.message)
        .collect();
    assert_eq!(
        found,
        ["This JSX tag requires 'myReactLib' to be in scope, but it could not be found."]
    );
}
