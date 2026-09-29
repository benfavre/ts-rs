use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn errors(source: &str, strict: bool) -> Vec<(String, String)> {
    let file = tsc_rs_parser::parse("overloads.ts", source);
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
        .filter(|d| d.code == 2394)
        .map(|d| {
            let span = d.span.unwrap();
            let related = &d.related.as_ref().unwrap()[0];
            assert_eq!(related.code, 2750);
            let implementation = related.span.unwrap();
            (
                source[span.start as usize..span.end as usize].into(),
                source[implementation.start as usize..implementation.end as usize].into(),
            )
        })
        .collect()
}

#[test]
fn overloads_check_return_types_arity_and_first_incompatibility() {
    for source in [
        "function f(): number; function f(): string { return ''; }",
        "function f(x: number): number; function f(x: number) { return; }",
        "function f(): void; function f(x: number): void {}",
        "function f(x: {a:number}): any; function f(x: {a:string}) {}",
        "function f(x:string):void; function f(x:number):void; function f(x:boolean):void {}",
        "function f(...x: string[]): void; function f(x: number): void {}",
        "function f(...x: [string, number]): void; function f(x: string, y: boolean): void {}",
    ] {
        assert_eq!(
            errors(source, false),
            [("f".into(), "f".into())],
            "{source}"
        );
    }
    for source in [
        "function f(): void; function f(): number { return 1; }",
        "function f(): number; function f(): number | string { return 1; }",
        "function f<T>(x:T):T; function f<U>(x:U):U {return x;}",
        "function f(x:string):string; function f(x:string) {return x;}",
        "function f(x: number):number; function f(x = 1) {return x;}",
        "function f(x:string,y:number):void; function f(...args:[string,number]) {}",
        "declare function f(x:string):number; declare function f(x:number):string;",
    ] {
        assert!(errors(source, true).is_empty(), "{source}");
    }
}

#[test]
fn strict_function_variance_and_method_bivariance_are_distinct() {
    let source = "function f(x: string | number): void; function f(x: string): void {}";
    assert!(errors(source, false).is_empty());
    assert_eq!(errors(source, true).len(), 1);
    let source = "function f(x?:string):void; function f(x:string):void {}";
    assert!(errors(source, false).is_empty());
    assert_eq!(errors(source, true).len(), 1);
    assert!(errors(
        "class C { f(x: string | number): void; f(x: string): void {} }",
        true
    )
    .is_empty());
    assert!(errors(
        "class C { constructor(x: string | number); constructor(x: string) {} }",
        true
    )
    .is_empty());
}

#[test]
fn methods_constructors_and_nested_function_scopes_report_their_own_overloads() {
    for (source, name) in [
        (
            "namespace N { export function f():number; export function f():string {return '';} }",
            "f",
        ),
        ("class C { f(): number; f(): string { return ''; } }", "f"),
        (
            "const C = class { f(): number; f(): string { return ''; } };",
            "f",
        ),
        (
            "class C { static f(): number; static f(): string {return '';} }",
            "f",
        ),
        (
            "class C { protected constructor(x: number); protected constructor(x: string) {} }",
            "protected constructor",
        ),
        (
            "function outer() { function f(): number; function f(): string {return '';} }",
            "f",
        ),
        (
            "{ function f(): number; function f(): string { return ''; } }",
            "f",
        ),
    ] {
        assert_eq!(
            errors(source, false),
            [(name.into(), name.into())],
            "{source}"
        );
    }
    assert!(errors("class C { f(): string; f(): string {return '';} static f(): number; static f(): number {return 1;} }", true).is_empty());
    assert!(errors("function f():number; function f():number {return 1;} function outer() { function f():string; function f():string {return '';} }", true).is_empty());
}

#[test]
fn constructor_diagnostics_preserve_modifiers_and_trivia() {
    // Verified against the TypeScript API: constructor diagnostics include
    // the declaration prefix through the keyword, including intervening trivia.
    for modifier in ["", "public ", "protected ", "private "] {
        let overload = format!("{modifier}/* constructor 🧭 */ constructor");
        let implementation = format!("{modifier}/* implementation */ constructor");
        let source =
            format!("class C {{\n {overload}(x: number);\n {implementation}(x: string) {{}}\n}}");
        // Leading comments are trivia outside the declaration span when there
        // is no modifier before them.
        let expected_overload = if modifier.is_empty() {
            "constructor"
        } else {
            &overload
        };
        let expected_implementation = if modifier.is_empty() {
            "constructor"
        } else {
            &implementation
        };
        assert_eq!(
            errors(&source, false),
            [(expected_overload.into(), expected_implementation.into())],
            "{source}"
        );

        let source = format!("class C {{\n {overload}();\n}}");
        let file = tsc_rs_parser::parse("missing.ts", &source);
        assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
        let symbols = tsc_rs_symbols::bind(&file);
        let output = TypeChecker::new().check(&file, &symbols);
        let missing: Vec<_> = output
            .diagnostics
            .iter()
            .filter(|d| d.code == 2390)
            .collect();
        assert_eq!(missing.len(), 1, "{:?}", output.diagnostics);
        let span = missing[0].span.unwrap();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            expected_overload,
            "{source}"
        );
    }
}

#[test]
fn callback_parameters_use_callback_variance_even_without_strict_function_types() {
    for strict in [false, true] {
        for source in [
            "function f(cb: (x: 'a') => number): void; function f(cb: (x: string) => number): void {}",
            "class C { f(cb: (x: 'a') => number): void; f(cb: (x: string) => number): void {} }",
        ] { assert_eq!(errors(source, strict), [("f".into(), "f".into())], "{source}"); }
        assert!(errors("function f(cb: (x: string) => number): void; function f(cb: (x: 'a') => number): void {}", strict).is_empty());
    }
}
