use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("references.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn assert_codes(source: &str, expected: &[u32]) -> Vec<Diagnostic> {
    let errors = diagnostics(source);
    let mut codes: Vec<_> = errors.iter().map(|error| error.code).collect();
    codes.sort_unstable();
    assert_eq!(codes, expected, "{source}: {errors:?}");
    errors
}

#[test]
fn assignment_references_preserve_operand_errors_and_target_spans() {
    for target in ["1", "(value())", "(1 + 2)", "new C()"] {
        let source =
            format!("declare function value(): number; declare class C {{}} {target} = 'text';");
        let errors = assert_codes(&source, &[2364]);
        let span = errors[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], target);
    }
    assert_codes("missing() = unknownValue;", &[2304, 2304, 2364]);
    assert_codes(
        "declare function value(): number; const result: string = (value() = 'text');",
        &[2364],
    );
}

#[test]
fn compound_operand_errors_precede_reference_checks() {
    for source in ["1 >>= 2;", "1 >>>= 2;", "1 += 'text';"] {
        assert_codes(source, &[2364]);
    }
    assert_codes("1 -= 'text';", &[2363]);
    assert_codes("'text' -= 1;", &[2362]);
}

#[test]
fn updates_check_references_after_operand_types() {
    for target in ["1", "value()", "(1 + 2)"] {
        for expression in [format!("++{target}"), format!("{target}--")] {
            let source = format!("declare function value(): number; {expression};");
            let errors = assert_codes(&source, &[2357]);
            let span = errors[0].span.unwrap();
            assert_eq!(&source[span.start as usize..span.end as usize], target);
        }
    }
    assert_codes("++missing();", &[2304, 2357]);
    assert_codes("++{};", &[2356]);
    assert_codes("declare function text(): string; text()++;", &[2356]);
    assert_codes("++null;", &[2357, 18050]);
}

#[test]
fn optional_accesses_cannot_be_assignment_or_update_targets() {
    for target in [
        "obj?.value",
        "obj?.['value']",
        "(obj?.value)!",
        "(obj?.value as number)",
    ] {
        let source = format!("declare const obj: {{ value: number }}; {target} = 1;");
        assert_codes(&source, &[2779]);
        let source = format!("declare const obj: {{ value: number }}; {target}++;");
        assert_codes(&source, &[2777]);
    }
    assert_codes(
        "declare const obj: { value: string }; obj?.value++;",
        &[2356],
    );
    assert_codes(
        "declare const obj: { readonly value: number }; obj?.value = 1;",
        &[2540, 2779],
    );
}

#[test]
fn valid_references_and_destructuring_are_preserved() {
    for source in [
        "let n: number; (n) = 1; (n as number) = 2; ++n; n--;",
        "let n: number; [n] = [1]; ({ value: n } = { value: 2 });",
        "declare const obj: { inner: { value: number } }; (obj?.inner)!.value = 1; (obj?.inner)!.value++;",
    ] {
        assert_codes(source, &[]);
    }
    assert_codes("let n: number; ([n]) = [1];", &[2364]);
    assert_codes("let n: number; ({ value: n }) = { value: 1 };", &[2364]);
}

#[test]
fn intrinsic_undefined_is_not_a_variable_but_local_bindings_are() {
    for source in ["undefined = 1;", "undefined++;", "--(undefined);"] {
        assert_codes(source, &[2539]);
    }
    assert_codes(
        "function f(undefined: number) { undefined = 1; undefined++; }",
        &[],
    );
    assert_codes("function f() { let undefined = 0; undefined++; }", &[]);
    assert_codes("NaN++; Infinity--;", &[]);
}

#[test]
fn parenthesized_writes_preserve_definite_assignment_order() {
    assert_codes("function f() { let n: number; (n) = 1; return n; }", &[]);
    assert_codes("function f() { let n: number; (n!) = 1; return n; }", &[]);
    assert_codes(
        "function f() { let n: number; (n) = n; return n; }",
        &[2454],
    );
    assert_codes(
        "function f() { let n: number; (n as number) = 1; return n; }",
        &[2454, 2454],
    );
}
