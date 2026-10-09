//! A line break inside a template literal is part of the string. It must not
//! make the emitter treat the surrounding binary expression as "laid out over
//! several lines": that path re-indents and re-spaces source text, and it
//! rewrote the string itself (`min-height` became `min - height`, every line
//! gained indentation). Found in production as CSS built by
//! `` const CSS = `...` + more() `` reaching the browser unusable.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

fn emit_cjs(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };
    emit(&file, &opts).javascript
}

const CSS: &str =
    "`\n.o-wrap { min-height: 100vh; max-width: calc(100% - 4px); }\n  .a-b{c-d:1}\n`";

fn assert_template_untouched(statement: &str) {
    // Top level: the template comes out byte for byte.
    let js = emit_cjs(statement);
    assert!(
        js.contains(CSS),
        "template text changed for:\n{statement}\n--- emitted:\n{js}"
    );

    // Inside a function body the emitter still re-indents continuation lines
    // (a separate, older limitation that also applies to a lone template),
    // so compare line by line without leading whitespace. What must hold is
    // that no token of the string is rewritten.
    let nested = format!("var m = (function () {{\n{statement}\nreturn C;\n}})();\n");
    let js = emit_cjs(&nested);
    let trimmed: Vec<&str> = js.lines().map(str::trim_start).collect();
    for line in CSS
        .lines()
        .filter(|l| !l.trim().is_empty() && l.trim() != "`")
    {
        assert!(
            trimmed
                .iter()
                .any(|emitted| emitted.contains(line.trim_start())),
            "template line {line:?} rewritten for:\n{nested}\n--- emitted:\n{js}"
        );
    }
    assert!(!js.contains("min - height"), "{js}");
}

#[test]
fn multiline_template_plus_call_keeps_its_text() {
    assert_template_untouched(&format!("const C = {CSS} + f(x);"));
}

#[test]
fn multiline_template_plus_string_keeps_its_text() {
    assert_template_untouched(&format!("const C = {CSS} + 'x';"));
    assert_template_untouched(&format!("const C = 'x' + {CSS};"));
}

#[test]
fn several_multiline_templates_keep_their_text() {
    assert_template_untouched(&format!("const C = {CSS} + f(x) + {CSS};"));
}

#[test]
fn a_really_multiline_binary_is_still_laid_out() {
    let js = emit_cjs("const C = aaa +\n    bbb -\n    ccc;\n");
    assert!(js.contains("aaa +\n"), "{js}");
}
