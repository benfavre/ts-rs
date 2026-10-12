//! The whole-output layout passes (`normalize_multiline_if_condition_recovery_output`,
//! `normalize_multiline_return_binary_indent`) re-indent lines; a line inside
//! a template literal's text is part of the string and must keep its
//! whitespace. Found by `scripts/emit-diff.py` on the bext sites: a SQL
//! template after a multi-line `if (` condition had its continuation lines
//! moved to the statement's indentation.

use tsc_rs_ast::{CompilerOptions, JsxEmit, ModuleKind, ScriptTarget};

fn compile(src: &str, fast_emit: bool) -> String {
    let sf = tsc_rs_parser::parse("/t.ts", src);
    let options = CompilerOptions {
        target: ScriptTarget::parse("es2022"),
        module: ModuleKind::parse("commonjs"),
        jsx: Some(JsxEmit::ReactJSX),
        fast_emit: Some(fast_emit),
        ..Default::default()
    };
    tsc_rs_emitter::emit(&sf, &options).javascript
}

const AFTER_MULTILINE_IF: &str = r#"
export async function f(a: number) {
  if (
    a > 1 &&
    a < 3
  )
    throw new Error("x");
  const rows = await db<Row>(
    `SELECT a
     FROM t
       WHERE b`,
    [a],
  );
  return rows;
}
"#;

const RETURN_BINARY: &str = r#"
export function g(a: boolean) {
  return a &&
    `line one
        line two`;
}
"#;

#[test]
fn template_after_multiline_if_keeps_its_text() {
    for fast in [true, false] {
        let out = compile(AFTER_MULTILINE_IF, fast);
        assert!(
            out.contains("`SELECT a\n     FROM t\n       WHERE b`"),
            "fast={fast}\n{out}"
        );
    }
}

#[test]
fn template_in_multiline_return_binary_keeps_its_text() {
    for fast in [true, false] {
        let out = compile(RETURN_BINARY, fast);
        assert!(
            out.contains("`line one\n        line two`"),
            "fast={fast}\n{out}"
        );
    }
}

// emit_compact_multiline_operator_expr trimmed and normalized each line of
// a multi-line `+` chain, template text included (`<div>` -> `<div >`,
// `a="0"` -> `a = "0"`). Found on ui-prism/src/lib/utilities.tsx.
const OPERATOR_CHAIN: &str = r#"
export const doc: Doc = {
  sections: [
    {
      body: wrap(
        P(`intro`) +
        `<div class="grid">
          <div>${P(`<code>a</code>`)}<div tabindex="0" class="x"></div></div>
        </div>` +
        Code({ style: { size: "80px" } as any }),
      ),
    },
  ],
};
"#;

#[test]
fn template_in_multiline_operator_chain_keeps_its_text() {
    for fast in [true, false] {
        let out = compile(OPERATOR_CHAIN, fast);
        assert!(
            out.contains("`<div class=\"grid\">\n          <div>${P(`<code>a</code>`)}<div tabindex=\"0\" class=\"x\"></div></div>\n        </div>`"),
            "fast={fast}\n{out}"
        );
    }
}
