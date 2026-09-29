// Expected diagnostics and spans checked against TypeScript 6.0.3.
use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn check(source: &str, name: &str, expected: &[(u32, &str, &str)]) {
    let file = tsc_rs_parser::parse(name, source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(false),
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, 1104 | 1105 | 1107 | 1114 | 1115 | 1116))
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                d.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn top_level_break() {
    check(
        r###"break;"###,
        r###"jump.ts"###,
        &[(
            1105,
            r###"A 'break' statement can only be used within an enclosing iteration or switch statement."###,
            r###"break;"###,
        )],
    );
}

#[test]
fn top_level_continue() {
    check(
        r###"continue;"###,
        r###"jump.ts"###,
        &[(
            1104,
            r###"A 'continue' statement can only be used within an enclosing iteration statement."###,
            r###"continue;"###,
        )],
    );
}

#[test]
fn missing_break_label() {
    check(
        r###"while (true) { break missing; }"###,
        r###"jump.ts"###,
        &[(
            1116,
            r###"A 'break' statement can only jump to a label of an enclosing statement."###,
            r###"break missing;"###,
        )],
    );
}

#[test]
fn missing_continue_label() {
    check(
        r###"while (true) { continue missing; }"###,
        r###"jump.ts"###,
        &[(
            1115,
            r###"A 'continue' statement can only jump to a label of an enclosing iteration statement."###,
            r###"continue missing;"###,
        )],
    );
}

#[test]
fn function_break() {
    check(
        r###"function f() { break; }"###,
        r###"jump.ts"###,
        &[(
            1107,
            r###"Jump target cannot cross function boundary."###,
            r###"break;"###,
        )],
    );
}

#[test]
fn function_continue() {
    check(
        r###"function f() { continue; }"###,
        r###"jump.ts"###,
        &[(
            1107,
            r###"Jump target cannot cross function boundary."###,
            r###"continue;"###,
        )],
    );
}

#[test]
fn function_missing_label() {
    check(
        r###"function f() { break missing; continue missing; }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break missing;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue missing;"###,
            ),
        ],
    );
}

#[test]
fn nested_function_boundary() {
    check(
        r###"outer: while (true) { function f() { while (true) { break outer; continue outer; } } }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break outer;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue outer;"###,
            ),
        ],
    );
}

#[test]
fn arrow_boundary() {
    check(
        r###"outer: while (true) { const f = () => { break outer; continue; }; }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break outer;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue;"###,
            ),
        ],
    );
}

#[test]
fn expression_boundary() {
    check(
        r###"while (true) { const f = function() { break; continue; }; }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue;"###,
            ),
        ],
    );
}

#[test]
fn object_method_boundary() {
    check(
        r###"outer: while (true) { const o = { f() { break outer; } }; }"###,
        r###"jump.ts"###,
        &[(
            1107,
            r###"Jump target cannot cross function boundary."###,
            r###"break outer;"###,
        )],
    );
}

#[test]
fn object_accessor_boundary() {
    check(
        r###"outer: while (true) { const o = { get x() { break outer; return 1; }, set x(v) { continue; } }; }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break outer;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue;"###,
            ),
        ],
    );
}

#[test]
fn class_method_boundary() {
    check(
        r###"outer: while (true) { class C { f() { break outer; } } }"###,
        r###"jump.ts"###,
        &[(
            1107,
            r###"Jump target cannot cross function boundary."###,
            r###"break outer;"###,
        )],
    );
}

#[test]
fn constructor_boundary() {
    check(
        r###"outer: while (true) { class C { constructor() { break outer; } } }"###,
        r###"jump.ts"###,
        &[(
            1107,
            r###"Jump target cannot cross function boundary."###,
            r###"break outer;"###,
        )],
    );
}

#[test]
fn class_accessor_boundary() {
    check(
        r###"outer: while (true) { class C { get x() { break outer; return 1; } set x(v) { continue; } } }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break outer;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue;"###,
            ),
        ],
    );
}

#[test]
fn static_block_boundary() {
    check(
        r###"outer: while (true) { class C { static { break outer; continue; } } }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break outer;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue;"###,
            ),
        ],
    );
}

#[test]
fn static_block_top_level() {
    check(
        r###"class C { static { break; continue; } }"###,
        r###"jump.ts"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue;"###,
            ),
        ],
    );
}

#[test]
fn non_iteration_continue() {
    check(
        r###"label: { continue label; }"###,
        r###"jump.ts"###,
        &[(
            1115,
            r###"A 'continue' statement can only jump to a label of an enclosing iteration statement."###,
            r###"continue label;"###,
        )],
    );
}

#[test]
fn switch_continue() {
    check(
        r###"label: switch (1) { case 1: continue label; }"###,
        r###"jump.ts"###,
        &[(
            1115,
            r###"A 'continue' statement can only jump to a label of an enclosing iteration statement."###,
            r###"continue label;"###,
        )],
    );
}

#[test]
fn switch_unlabeled_continue() {
    check(
        r###"switch (1) { default: continue; }"###,
        r###"jump.ts"###,
        &[(
            1104,
            r###"A 'continue' statement can only be used within an enclosing iteration statement."###,
            r###"continue;"###,
        )],
    );
}

#[test]
fn switch_in_function_continue() {
    check(
        r###"function f() { switch (1) { default: continue; } }"###,
        r###"jump.ts"###,
        &[(
            1107,
            r###"Jump target cannot cross function boundary."###,
            r###"continue;"###,
        )],
    );
}

#[test]
fn valid_loop_jumps() {
    check(
        r###"while (true) { break; continue; } do { break; continue; } while (true); for (;;) { break; continue; }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn valid_for_in_of() {
    check(
        r###"for (const x in {}) { break; continue; } for (const x of []) { break; continue; }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn valid_chained_labels() {
    check(
        r###"a: b: while (true) { continue a; continue b; break a; break b; }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn valid_block_break() {
    check(r###"label: { break label; }"###, r###"jump.ts"###, &[]);
}

#[test]
fn valid_switch_break() {
    check(
        r###"switch (1) { default: break; }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn valid_switch_continue_to_loop() {
    check(
        r###"while (true) { switch (1) { default: continue; } }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn valid_function_inner_loop() {
    check(
        r###"outer: while (true) { function f() { inner: while (true) { break inner; continue inner; } } }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn valid_static_block_inner_loop() {
    check(
        r###"outer: while (true) { class C { static { inner: while (true) { break inner; continue inner; } } } }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn duplicate_label() {
    check(
        r###"a: { a: {} }"###,
        r###"jump.ts"###,
        &[(1114, r###"Duplicate label 'a'."###, r###"a"###)],
    );
}

#[test]
fn duplicate_chained_label() {
    check(
        r###"a: a: while (true) {}"###,
        r###"jump.ts"###,
        &[(1114, r###"Duplicate label 'a'."###, r###"a"###)],
    );
}

#[test]
fn duplicate_static_block_label() {
    check(
        r###"a: { class C { static { a: {} } } }"###,
        r###"jump.ts"###,
        &[(1114, r###"Duplicate label 'a'."###, r###"a"###)],
    );
}

#[test]
fn valid_reused_sibling_labels() {
    check(r###"a: {} a: {}"###, r###"jump.ts"###, &[]);
}

#[test]
fn valid_function_label_reuse() {
    check(
        r###"a: { function f() { a: {} } }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn escaped_duplicate_label() {
    check(
        r###"a: { \u0061: {} }"###,
        r###"jump.ts"###,
        &[(1114, r###"Duplicate label '\u0061'."###, r###"\u0061"###)],
    );
}

#[test]
fn unicode_duplicate_label() {
    check(
        r###"é: { é: {} }"###,
        r###"jump.ts"###,
        &[(1114, r###"Duplicate label 'é'."###, r###"é"###)],
    );
}

#[test]
fn break_line_terminator() {
    check(
        r###"while (true) { break
missing; }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn commented_jump() {
    check(
        r###"break /* a comment */ missing;"###,
        r###"jump.ts"###,
        &[(
            1116,
            r###"A 'break' statement can only jump to a label of an enclosing statement."###,
            r###"break /* a comment */ missing;"###,
        )],
    );
}

#[test]
fn nested_try() {
    check(
        r###"outer: while (true) { try { continue outer; } catch { break outer; } finally { break outer; } }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn syntax_error_suppression() {
    check(
        r###"let x = ; break; continue; a: a: {}"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn ambient_first_statement() {
    check(
        r###"declare namespace N { break; continue; }"###,
        r###"jump.ts"###,
        &[(
            1104,
            r###"A 'continue' statement can only be used within an enclosing iteration statement."###,
            r###"continue;"###,
        )],
    );
}

#[test]
fn ambient_nested_statement() {
    check(
        r###"declare namespace N { label: { break; continue; } }"###,
        r###"jump.ts"###,
        &[(
            1104,
            r###"A 'continue' statement can only be used within an enclosing iteration statement."###,
            r###"continue;"###,
        )],
    );
}

#[test]
fn declaration_file() {
    check(
        r###"break; continue;"###,
        r###"jump.d.ts"###,
        &[(
            1104,
            r###"A 'continue' statement can only be used within an enclosing iteration statement."###,
            r###"continue;"###,
        )],
    );
}

#[test]
fn namespace_jump() {
    check(
        r###"outer: while (true) { namespace N { break outer; continue outer; } }"###,
        r###"jump.ts"###,
        &[],
    );
}

#[test]
fn javascript_jumps() {
    check(
        r###"function f() { break; continue; }"###,
        r###"jump.js"###,
        &[
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"break;"###,
            ),
            (
                1107,
                r###"Jump target cannot cross function boundary."###,
                r###"continue;"###,
            ),
        ],
    );
}

#[test]
fn ambient_block_error_precedes_jump_error() {
    let source = "declare namespace N { label: { let x: number; break; continue; } }";
    let file = tsc_rs_parser::parse("jump.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = file
        .diagnostics
        .iter()
        .chain(result.diagnostics.iter())
        .filter(|d| matches!(d.code, 1036 | 1104 | 1105 | 1107))
        .map(|d| {
            let span = d.span.unwrap();
            (d.code, &source[span.start as usize..span.end as usize])
        })
        .collect();
    assert_eq!(
        actual,
        vec![(1036, "label"), (1036, "break"), (1104, "continue;")]
    );
}

#[test]
fn escaped_jump_target_matches_unescaped_label() {
    check(
        r"a: while (true) { break \u0061; continue \u0061; }",
        "jump.ts",
        &[],
    );
}
