//! A statement whose nested string literals hold a control character
//! (NUL) or U+FFFD is ordinary code on a clean parse. The garbage-recovery
//! check dropped the whole statement: a bext test file lost its
//! `describe(...)` block (found by `scripts/emit-diff.py`).

use tsc_rs_ast::{CompilerOptions, ModuleKind};

fn compile(src: &str, fast_emit: bool) -> String {
    let sf = tsc_rs_parser::parse("/t.ts", src);
    let options = CompilerOptions {
        module: ModuleKind::parse("commonjs"),
        fast_emit: Some(fast_emit),
        ..Default::default()
    };
    tsc_rs_emitter::emit(&sf, &options).javascript
}

#[test]
fn nul_in_nested_string_keeps_the_statement() {
    for fast in [true, false] {
        let out = compile("d(\"a\", () => {\n  const i = [\"/\0e\"];\n});\n", fast);
        assert!(out.contains("d(\"a\""), "fast={fast}\n{out}");
        assert!(out.contains('\0'), "fast={fast}\n{out}");
    }
}

#[test]
fn replacement_char_in_nested_string_keeps_the_statement() {
    for fast in [true, false] {
        let out = compile("log([\"bad \u{fffd} byte\"]);\n", fast);
        assert!(out.contains("log(["), "fast={fast}\n{out}");
    }
}
