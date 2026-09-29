//! Tests for CJS module interop helpers (__importDefault, __importStar, __exportStar).
//!
//! When `esModuleInterop` is true and module is CommonJS, the emitter should
//! generate proper interop helpers and use them for import/export transforms.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

/// Helper: parse and emit with CommonJS + esModuleInterop enabled.
fn emit_cjs_interop(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        es_module_interop: Some(true),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse and emit with plain CommonJS (no esModuleInterop).
fn emit_cjs(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

// ---------------------------------------------------------------
// __esModule marker
// ---------------------------------------------------------------

#[test]
fn test_esmodule_marker_emitted() {
    let js = emit_cjs_interop("export const x = 1;");
    assert!(
        js.contains("Object.defineProperty(exports, \"__esModule\", { value: true });"),
        "__esModule marker should be emitted: {js}"
    );
}

#[test]
fn test_esmodule_marker_not_emitted_with_export_assign() {
    let js = emit_cjs_interop("export = 42;");
    assert!(
        !js.contains("__esModule"),
        "__esModule should NOT be emitted when export = is present: {js}"
    );
}

// ---------------------------------------------------------------
// __importDefault helper
// ---------------------------------------------------------------

#[test]
fn test_import_default_helper_emitted() {
    let js = emit_cjs_interop("import foo from './module';\nconsole.log(foo);");
    assert!(
        js.contains("var __importDefault"),
        "__importDefault helper should be emitted: {js}"
    );
}

#[test]
fn test_import_default_uses_helper() {
    let js = emit_cjs_interop("import foo from './module';\nconsole.log(foo);");
    assert!(
        js.contains("__importDefault(require(\"./module\"))"),
        "default import should use __importDefault: {js}"
    );
}

#[test]
fn test_import_default_emitted_without_explicit_interop() {
    // TypeScript 5.x always wraps default imports with __importDefault in CJS
    let js = emit_cjs("import foo from './module';\nconsole.log(foo);");
    assert!(
        js.contains("__importDefault"),
        "__importDefault should be used even without explicit esModuleInterop: {js}"
    );
    assert!(
        js.contains("require(\"./module\")"),
        "should still use require: {js}"
    );
}

#[test]
fn test_import_default_helper_content() {
    let js = emit_cjs_interop("import foo from './module';\nconsole.log(foo);");
    assert!(
        js.contains("(mod && mod.__esModule) ? mod : { \"default\": mod }"),
        "__importDefault helper should have correct body: {js}"
    );
}

// ---------------------------------------------------------------
// __importStar helper
// ---------------------------------------------------------------

#[test]
fn test_import_star_helper_emitted() {
    let js = emit_cjs_interop("import * as ns from './module';\nconsole.log(ns);");
    assert!(
        js.contains("var __importStar"),
        "__importStar helper should be emitted: {js}"
    );
}

#[test]
fn test_import_star_uses_helper() {
    let js = emit_cjs_interop("import * as ns from './module';\nconsole.log(ns);");
    assert!(
        js.contains("__importStar(require(\"./module\"))"),
        "namespace import should use __importStar: {js}"
    );
}

#[test]
fn test_import_star_emitted_without_explicit_interop() {
    // TypeScript 5.x always wraps namespace imports with __importStar in CJS
    let js = emit_cjs("import * as ns from './module';\nconsole.log(ns);");
    assert!(
        js.contains("__importStar"),
        "__importStar should be used even without explicit esModuleInterop: {js}"
    );
    assert!(
        js.contains("__importStar(require(\"./module\"))"),
        "should wrap require with __importStar: {js}"
    );
}

// ---------------------------------------------------------------
// __exportStar helper
// ---------------------------------------------------------------

#[test]
fn test_export_star_helper_emitted() {
    let js = emit_cjs_interop("export * from './module';");
    assert!(
        js.contains("var __exportStar"),
        "__exportStar helper should be emitted: {js}"
    );
}

#[test]
fn test_export_star_uses_helper() {
    let js = emit_cjs_interop("export * from './module';");
    assert!(
        js.contains("__exportStar(require(\"./module\"), exports)"),
        "export * should use __exportStar helper: {js}"
    );
}

#[test]
fn test_export_star_emitted_without_explicit_interop() {
    // TypeScript 5.x always uses __createBinding + __exportStar for export * in CJS
    let js = emit_cjs("export * from './module';");
    assert!(
        js.contains("__exportStar"),
        "__exportStar should be used even without explicit esModuleInterop: {js}"
    );
    assert!(
        js.contains("__createBinding"),
        "__createBinding should be emitted for __exportStar: {js}"
    );
}

// ---------------------------------------------------------------
// export { name } from "mod" with Object.defineProperty
// ---------------------------------------------------------------

#[test]
fn test_named_reexport_with_interop() {
    let js = emit_cjs_interop("export { foo } from './module';");
    assert!(
        js.contains("Object.defineProperty(exports, \"foo\""),
        "named re-export should use Object.defineProperty with interop: {js}"
    );
    assert!(
        js.contains("enumerable: true"),
        "should have enumerable: true: {js}"
    );
    assert!(
        js.contains("get: function"),
        "should have getter function: {js}"
    );
}

#[test]
fn test_named_reexport_with_rename_interop() {
    let js = emit_cjs_interop("export { foo as bar } from './module';");
    assert!(
        js.contains("Object.defineProperty(exports, \"bar\""),
        "renamed re-export should use the exported name: {js}"
    );
    assert!(
        js.contains(".foo"),
        "getter should reference the local name: {js}"
    );
}

#[test]
fn test_named_reexport_without_interop() {
    // TypeScript 5.x always uses Object.defineProperty for named re-exports in CJS
    let js = emit_cjs("export { foo } from './module';");
    assert!(
        js.contains("Object.defineProperty(exports, \"foo\""),
        "named re-export should use Object.defineProperty: {js}"
    );
}

// ---------------------------------------------------------------
// Helpers only emitted when needed
// ---------------------------------------------------------------

#[test]
fn test_no_helpers_when_not_needed() {
    let js = emit_cjs_interop("export const x = 1;");
    assert!(
        !js.contains("__importDefault"),
        "__importDefault should not be emitted when not needed: {js}"
    );
    assert!(
        !js.contains("__importStar"),
        "__importStar should not be emitted when not needed: {js}"
    );
    assert!(
        !js.contains("__exportStar"),
        "__exportStar should not be emitted when not needed: {js}"
    );
}

#[test]
fn test_only_needed_helpers_emitted() {
    // Only default import - should only emit __importDefault, not __importStar/__exportStar
    let js = emit_cjs_interop("import foo from './module';\nconsole.log(foo);");
    assert!(
        js.contains("__importDefault"),
        "__importDefault should be emitted: {js}"
    );
    assert!(
        !js.contains("__importStar"),
        "__importStar should NOT be emitted when not needed: {js}"
    );
    assert!(
        !js.contains("__exportStar"),
        "__exportStar should NOT be emitted when not needed: {js}"
    );
}

#[test]
fn test_multiple_helpers_emitted() {
    let source = r#"
import foo from './a';
import * as ns from './b';
export * from './c';
console.log(foo, ns);
"#;
    let js = emit_cjs_interop(source);
    assert!(
        js.contains("__importDefault"),
        "__importDefault should be emitted: {js}"
    );
    assert!(
        js.contains("__importStar"),
        "__importStar should be emitted: {js}"
    );
    assert!(
        js.contains("__exportStar"),
        "__exportStar should be emitted: {js}"
    );
}

// ---------------------------------------------------------------
// Side-effect imports (no bindings)
// ---------------------------------------------------------------

#[test]
fn test_side_effect_import_no_helper() {
    let js = emit_cjs_interop("import './polyfill';");
    assert!(
        js.contains("require(\"./polyfill\")"),
        "side-effect import should still use require: {js}"
    );
    assert!(
        !js.contains("__importDefault"),
        "side-effect import should not trigger __importDefault: {js}"
    );
}

// ---------------------------------------------------------------
// Named imports (no default) - should NOT use __importDefault
// ---------------------------------------------------------------

#[test]
fn test_named_import_no_import_default() {
    let js = emit_cjs_interop("import { foo, bar } from './module';\nconsole.log(foo, bar);");
    assert!(
        !js.contains("__importDefault"),
        "named-only import should NOT use __importDefault: {js}"
    );
    assert!(
        js.contains("require(\"./module\")"),
        "should use plain require for named imports: {js}"
    );
}

// ---------------------------------------------------------------
// Type-only imports should be skipped
// ---------------------------------------------------------------

#[test]
fn test_type_only_import_skipped() {
    let js = emit_cjs_interop("import type { Foo } from './module';");
    assert!(
        !js.contains("require"),
        "type-only import should be completely skipped: {js}"
    );
    assert!(
        !js.contains("__importDefault"),
        "type-only import should not trigger helpers: {js}"
    );
}

// ---------------------------------------------------------------
// Named-only import with interop should NOT use __importDefault
// ---------------------------------------------------------------

#[test]
fn test_named_only_import_with_interop_no_default_helper() {
    let js = emit_cjs_interop("import { readFile } from 'fs';\nconsole.log(readFile);");
    assert!(
        js.contains("require(\"fs\")"),
        "named import should use require: {js}"
    );
    assert!(
        !js.contains("__importDefault"),
        "named-only import should NOT use __importDefault even with interop: {js}"
    );
}

// ---------------------------------------------------------------
// export * as ns from './module' (aliased export star)
// ---------------------------------------------------------------

#[test]
fn test_export_star_as_namespace() {
    let js = emit_cjs_interop("export * as ns from './module';");
    // Aliased export star uses direct assignment, not __exportStar
    assert!(
        !js.contains("__exportStar"),
        "aliased export star should NOT use __exportStar: {js}"
    );
    assert!(
        js.contains("require(\"./module\")"),
        "should require the module: {js}"
    );
}

// ---------------------------------------------------------------
// Verbatim-copied heads must still rewrite imported bindings
// ---------------------------------------------------------------

#[test]
fn test_if_head_with_leading_line_comment_rewrites_imports() {
    // A multi-line `if (` whose condition starts with a `//` comment takes the
    // layout-preserving path. It used to copy the condition as source text,
    // leaving `f`/`g` bare (ReferenceError at runtime in a PRISM site).
    let js = emit_cjs(
        "import { f, g } from \"./m\";\nexport function run(x: boolean) {\n  if (\n    // leading comment\n    x && f() && g()\n  ) return 1;\n  return 0;\n}\n",
    );
    assert!(js.contains("(0, m_1.f)()"), "f not rewritten:\n{js}");
    assert!(js.contains("(0, m_1.g)()"), "g not rewritten:\n{js}");
    assert!(!js.contains("&& f()"), "bare f() left in:\n{js}");
}

#[test]
fn test_if_head_with_leading_line_comment_without_imports_keeps_layout() {
    // No module rewrite involved: the layout-preserving path still applies.
    let js = emit_cjs("export function run(x: boolean, y: boolean) {\n  if (\n    // why\n    x && y\n  ) return 1;\n  return 0;\n}\n");
    assert!(js.contains("// why"), "comment lost:\n{js}");
    assert!(js.contains("x && y"), "condition lost:\n{js}");
}
