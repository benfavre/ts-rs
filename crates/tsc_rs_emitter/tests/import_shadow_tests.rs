//! A local binding that reuses an imported name must not be rewritten to the
//! CommonJS import access (`gate` -> `guard_1.gate`). Covers parameters,
//! block-scoped and hoisted declarations, catch bindings, destructuring,
//! shorthand properties, JSX tags and tagged templates, in both emit modes.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

fn emit_cjs(source: &str, fast_emit: bool) -> String {
    let file = tsc_rs_parser::parse("test.tsx", source);
    let opts = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        target: Some(ScriptTarget::ES2022),
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
fn local_const_shadowing_import_keeps_local() {
    let src = r#"import { gate } from "./guard";
export function loader(r: Request) {
  const gate = check(r);
  if (gate instanceof Response) return gate;
  return "ok";
}
export const real = () => gate(1);
"#;
    for js in both(src) {
        assert!(js.contains("if (gate instanceof Response)"), "{js}");
        assert!(js.contains("return gate;"), "{js}");
        assert!(js.contains("(0, guard_1.gate)(1)"), "{js}");
    }
}

#[test]
fn params_catch_and_hoisted_vars_shadow_import() {
    let src = r#"import gate from "./guard";
export const a = (gate: number) => gate * 2;
export const b = () => { try { x(); } catch (gate) { return gate; } };
export const c = { m() { if (y) { var gate = 2; } return gate; } };
export const d = ({ gate }: any) => ({ gate });
export class K { run(gate: number) { return gate; } }
export const e = () => gate();
"#;
    for js in both(src) {
        assert!(!js.contains("guard_1.default * 2"), "{js}");
        assert!(js.contains("(gate) => gate * 2"), "{js}");
        assert!(js.contains("return gate;"), "{js}");
        assert!(js.contains("({ gate })"), "{js}");
        assert!(js.contains("(0, guard_1.default)()"), "{js}");
        assert_eq!(js.matches("guard_1.default").count(), 1, "{js}");
    }
}

#[test]
fn block_scope_ends_at_block() {
    let src = r#"import { gate, css, Card } from "./guard";
export function s1() { { const gate = 1; void gate; } return gate(); }
export function s2(css: any) { return css`a`; }
export function s3() { const Card = () => null; return <Card />; }
export function s4() { return <Card />; }
"#;
    for js in both(src) {
        assert!(js.contains("void gate;"), "{js}");
        assert!(js.contains("return (0, guard_1.gate)();"), "{js}");
        assert!(
            js.contains("return css") && !js.contains("guard_1.css"),
            "{js}"
        );
        assert!(js.contains(".jsx)(Card, {})"), "{js}");
        assert!(js.contains(".jsx)(guard_1.Card, {})"), "{js}");
    }
}

#[test]
fn exported_let_qualified_in_one_line_bodies() {
    let src = r#"export let count = 0;
export function a() { count = 5; return count; }
export function b() { count++; }
export const c = () => { count = 1; };
export const d = { m() { return count; } };
"#;
    for js in both(src) {
        assert!(
            js.contains("function a() { exports.count = 5; return exports.count; }"),
            "{js}"
        );
        assert!(js.contains("exports.count++;"), "{js}");
        assert!(js.contains("=> { exports.count = 1; }"), "{js}");
        assert!(js.contains("m() { return exports.count; }"), "{js}");
    }
}

#[test]
fn locals_shadowing_exported_binding_stay_local() {
    let src = r#"export let count = 0;
export const a = () => { try { x(); } catch (count) { return count; } };
export function b() { for (const count of [1]) { use(count); } return count; }
export function c() { const count = 1; return { count }; }
"#;
    for js in both(src) {
        assert!(js.contains("catch (count) {\n    return count;"), "{js}");
        assert!(js.contains("use(count);"), "{js}");
        assert!(js.contains("return exports.count;"), "{js}");
        assert!(js.contains("return { count };"), "{js}");
    }
}

#[test]
fn decorators_resolve_outside_the_decorated_member() {
    let src = r#"import { dec, gate } from "./guard";
export class C {
  @dec(gate) m(gate: number) { return gate; }
  n(@dec(gate) gate: number) { return gate; }
}
export function f() { const gate = 1; class D { @dec(gate) m() {} } return D; }
"#;
    for fast in [false, true] {
        let file = tsc_rs_parser::parse("test.ts", src);
        let opts = CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            target: Some(ScriptTarget::ES2022),
            experimental_decorators: Some(true),
            fast_emit: Some(fast),
            ..Default::default()
        };
        let js = emit(&file, &opts).javascript;
        assert!(
            js.contains("(0, guard_1.dec)(guard_1.gate)\n], C.prototype, \"m\""),
            "{js}"
        );
        assert!(
            js.contains("__param(0, (0, guard_1.dec)(guard_1.gate))"),
            "{js}"
        );
        assert!(
            js.contains("(0, guard_1.dec)(gate)\n], D.prototype"),
            "{js}"
        );
        assert!(js.contains("m(gate) { return gate; }"), "{js}");
    }
}
