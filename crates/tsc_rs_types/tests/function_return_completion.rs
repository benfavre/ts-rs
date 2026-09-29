use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn completion(source: &str, strict: bool, implicit: bool) -> Vec<(u32, String)> {
    let file = tsc_rs_parser::parse("returns.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict_null_checks: Some(strict),
                no_implicit_returns: Some(implicit),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    diagnostics
        .into_iter()
        .filter(|d| matches!(d.code, 2355 | 2366 | 2534 | 7030))
        .map(|Diagnostic { code, span, .. }| {
            let span = span.expect("diagnostic span");
            (
                code,
                source[span.start as usize..span.end as usize].to_owned(),
            )
        })
        .collect()
}

#[test]
fn annotated_function_forms_require_a_return() {
    for source in [
        "function f(): number {}",
        "const f = function(): number {};",
        "const f = (): number => {};",
        "const f: () => number = (): number => {};",
        "const f: () => number = function(): number {};",
        "class C { f(): number {} }",
        "const c = { f(): number {} };",
        "const c: { f(): number } = { f(): number {} };",
        "class C { get f(): number {} }",
        "const c = { get f(): number {} };",
        "declare class C { f(): number {} }",
    ] {
        assert_eq!(
            completion(source, true, false),
            vec![(2355, "number".into())],
            "{source}"
        );
    }
}

#[test]
fn explicit_return_types_and_strictness_choose_the_diagnostic() {
    for (ty, code) in [
        ("any", None),
        ("void", None),
        ("undefined", None),
        ("number | void", None),
        ("number | undefined", Some(2355)),
        ("unknown", Some(2355)),
        ("never", Some(2534)),
    ] {
        let source = format!("function f(): {ty} {{}}");
        assert_eq!(
            completion(&source, true, false),
            code.map(|c| (c, ty.into())).into_iter().collect::<Vec<_>>(),
            "{source}"
        );
    }
    let source = "function f(b: boolean): number { if (b) return 1; }";
    assert_eq!(
        completion(source, true, false),
        vec![(2366, "number".into())]
    );
    assert!(completion(source, false, false).is_empty());
    assert_eq!(
        completion(source, false, true),
        vec![(7030, "number".into())]
    );
}

#[test]
fn flow_handles_loops_labels_switches_and_finally() {
    for body in [
        "throw 1;",
        "while (true) {}",
        "for (;;) {}",
        "do { return 1; } while (false);",
        "if (b) return 1; else throw 1;",
        "switch (b) { case true: return 1; case false: throw 1; }",
        "switch (b) { case true: case false: return 1; }",
        "try { if (b) return 1; } finally { throw 1; }",
        "outer: while (true) { while (b) { continue outer; } }",
    ] {
        let source = format!("function f(b: boolean): number {{ {body} }}");
        assert!(completion(&source, true, false).is_empty(), "{source}");
    }
    for (body, code) in [
        ("if (false) return 1;", 2355),
        ("function nested() { return 1; }", 2355),
        ("while (true) { if (b) break; }", 2355),
        ("outer: while (true) { break outer; }", 2355),
        ("switch (b) { case true: return 1; }", 2366),
        ("try { return 1; } catch {}", 2366),
    ] {
        let source = format!("function f(b: boolean): number {{ {body} }}");
        assert_eq!(
            completion(&source, true, false),
            vec![(code, "number".into())],
            "{source}"
        );
    }
}

#[test]
fn async_and_generator_completion_uses_the_unwrapped_return_type() {
    for (source, expected) in [
        (
            "async function f(): Promise<number> {}",
            vec![(2355, "Promise<number>".into())],
        ),
        ("async function f(): Promise<void> {}", vec![]),
        (
            "function* f(): Generator<number, number, unknown> { yield 1; }",
            vec![(2355, "Generator<number, number, unknown>".into())],
        ),
        (
            "function* f(): Generator<number, void, unknown> { yield 1; }",
            vec![],
        ),
        (
            "function* f(): IterableIterator<number> { yield 1; }",
            vec![],
        ),
    ] {
        assert_eq!(completion(source, true, false), expected, "{source}");
    }
}

#[test]
fn implicit_returns_ignore_void_bodies_and_nested_returns() {
    let source = "function f(b: boolean) { if (b) return 1; }";
    assert_eq!(completion(source, true, true), vec![(7030, "f".into())]);
    assert!(completion(source, true, false).is_empty());
    for source in [
        "function f(b: boolean) { if (b) return; }",
        "function f() { function nested() { return 1; } }",
        "function f(b: boolean) { if (b) return undefined; }",
    ] {
        assert!(completion(source, true, true).is_empty(), "{source}");
    }
}

#[test]
fn nonreturning_calls_require_explicit_callee_bindings() {
    for (declaration, expected) in [
        ("declare function stop(): never;", vec![]),
        (
            "const stop = () => { throw 1; };",
            vec![(2355, "number".into())],
        ),
        (
            "const stop = (): never => { throw 1; };",
            vec![(2355, "number".into())],
        ),
        ("const stop: () => never = () => { throw 1; };", vec![]),
    ] {
        let source = format!("{declaration} function f(): number {{ stop(); }}");
        assert_eq!(completion(&source, true, false), expected, "{source}");
    }
    let source = "declare function stop(): never; function f(): number { const x = stop(); }";
    assert_eq!(
        completion(source, true, false),
        vec![(2355, "number".into())]
    );
}

#[test]
fn implicit_return_diagnostics_use_function_name_or_expression_spans() {
    for (source, span) in [
        (
            "const f = function(b: boolean) { if (b) return 1; };",
            "function",
        ),
        (
            "const f = function named(b: boolean) { if (b) return 1; };",
            "named",
        ),
        (
            "const f = (b: boolean) => { if (b) return 1; };",
            "(b: boolean) => { if (b) return 1; }",
        ),
        (
            "const f = { method(b: boolean) { if (b) return 1; } };",
            "method",
        ),
    ] {
        assert_eq!(
            completion(source, true, true),
            vec![(7030, span.into())],
            "{source}"
        );
    }
}

#[test]
fn explicit_never_calls_preserve_nested_scopes_and_shadowing() {
    for source in [
        "function f(stop: () => never): number { stop(); }",
        "function f(): number { { const stop: () => never = () => { throw 1; }; stop(); } }",
        "const obj: { stop(): never } = { stop(): never { throw 1; } }; function f(): number { obj.stop(); }",
        "function f(): Missing {}",
        "async function f(): Missing<void> {}",
    ] {
        assert!(completion(source, true, false).is_empty(), "{source}");
    }
    for source in [
        "declare function stop(): never; function f(): number { const stop = (): never => { throw 1; }; stop(); }",
        "const obj = { stop(): never { throw 1; } }; function f(): number { obj.stop(); }",
    ] {
        assert_eq!(completion(source, true, false), vec![(2355, "number".into())], "{source}");
    }
}

#[test]
fn readonly_class_discriminants_make_switches_exhaustive() {
    let source = r#"
class A { readonly kind = "A"; }
class B { readonly kind = "B"; }
function f(value: A | B): number {
    switch (value.kind) { case "A": return 1; case "B": return 2; }
}
"#;
    assert!(completion(source, true, false).is_empty());
}

#[test]
fn enum_switches_require_every_member() {
    for declaration in ["enum E { A, B }", "enum E { A = 'a', B = 'b' }"] {
        let source = format!("{declaration} function f(value: E): number {{ switch (value) {{ case E.A: return 1; case E.B: return 2; }} }}");
        assert!(completion(&source, true, false).is_empty(), "{source}");
        let source = format!("{declaration} function f(value: E): number {{ switch (value) {{ case E.A: return 1; }} }}");
        assert_eq!(
            completion(&source, true, false),
            vec![(2366, "number".into())],
            "{source}"
        );
    }
}
