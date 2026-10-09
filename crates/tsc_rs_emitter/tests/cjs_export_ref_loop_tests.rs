//! A statement that reads one of its module's own `export const` bindings must
//! not be copied verbatim under CommonJS: the bare name has no local binding
//! (`exports.A` is the only one). `for...of` bodies were the common miss
//! (`A is not defined` at render on PRISM sites); `for...in`, `for` heads,
//! `try`, labels, tagged templates and JSX are covered by the same check.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

fn emit_cjs(source: &str, fast_emit: bool) -> String {
    let file = tsc_rs_parser::parse("test.tsx", source);
    let opts = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        target: Some(ScriptTarget::ES2020),
        jsx: Some(JsxEmit::ReactJSX),
        fast_emit: Some(fast_emit),
        ..Default::default()
    };
    emit(&file, &opts).javascript
}

fn both(source: &str) -> [String; 2] {
    [emit_cjs(source, false), emit_cjs(source, true)]
}

#[test]
fn for_of_reads_own_export() {
    let src = r#"export const A = [1, 2];
export function f1() { let s = 0; for (const v of A) { s += v; } return s; }
export function f2() { let s = 0; for (const v of A) s += v; return s; }
export function f3() { let s = 0; for (const [i, v] of A.entries()) s += i + v + A[0]; return s; }
export async function f4() { let s = 0; for await (const v of A) s += v; return s; }
export function f5() { let s = 0; for (const v of A) for (const w of A) s += v * w; return s; }
"#;
    for js in both(src) {
        assert!(!js.contains(" of A"), "{js}");
        assert!(!js.contains("+ A[0]"), "{js}");
        assert!(js.contains("for (const v of exports.A)"), "{js}");
        assert!(js.contains("for await (const v of exports.A)"), "{js}");
    }
}

#[test]
fn top_level_for_of_with_nested_loop_reads_own_export() {
    let src = r#"export const FAQ_SLUGS = [{ canonical: "a", aliases: ["b"] }];
const byAlias = new Map();
for (const e of FAQ_SLUGS) {
  for (const a of e.aliases) byAlias.set(a, e);
}
"#;
    for js in both(src) {
        assert!(js.contains("for (const e of exports.FAQ_SLUGS)"), "{js}");
    }
}

#[test]
fn for_in_for_head_try_label_read_own_export() {
    let src = r#"export const O = { k: 1 };
export const N = 2;
export function g1() { let s = 0; for (const k in O) { s += 1; } return s; }
export function g2() { let s = 0; for (let i = N; i > 0; i--) { s += i; } return s; }
export function g3() { try { return 1; } catch { return N + 1; } }
export function g4() { let s = 0; top: for (const v of [1]) { s += v + N; break top; } return s; }
"#;
    for js in both(src) {
        assert!(js.contains("in exports.O"), "{js}");
        assert!(js.contains("let i = exports.N"), "{js}");
        assert!(js.contains("return exports.N + 1"), "{js}");
        assert!(js.contains("s += v + exports.N"), "{js}");
    }
}

#[test]
fn loop_and_catch_bindings_shadowing_an_export_stay_local() {
    let src = r#"export const A = [1, 2];
export const B = 100;
export function s1() { let s = 0; for (const B of A) s += B; return s; }
export function s2() { try { throw 7; } catch (B) { return (B as number) + A[0]; } }
"#;
    for js in both(src) {
        assert!(js.contains("for (const B of exports.A)"), "{js}");
        assert!(js.contains("s += B;"), "{js}");
        assert!(js.contains("return B + exports.A[0]"), "{js}");
    }
}
