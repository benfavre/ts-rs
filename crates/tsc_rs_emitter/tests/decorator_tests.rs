//! Tests for decorator runtime emit transformations.
//!
//! When `experimentalDecorators: true` is set, the emitter should produce
//! `__decorate` and `__param` helper calls for decorated classes, methods,
//! properties, and parameters.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

/// Helper: parse TypeScript source and emit JavaScript with experimentalDecorators enabled.
fn emit_decorators(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        experimental_decorators: Some(true),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

// ---------------------------------------------------------------
// __decorate helper emission
// ---------------------------------------------------------------

#[test]
fn test_decorate_helper_emitted_when_decorators_present() {
    let js = emit_decorators(
        r#"
function sealed(target: any) {}

@sealed
class Greeter {}
"#,
    );
    assert!(
        js.contains("var __decorate"),
        "should emit __decorate helper: {js}"
    );
    assert!(
        js.contains("(this && this.__decorate)"),
        "should have __decorate guard: {js}"
    );
}

#[test]
fn test_no_decorate_helper_without_decorators() {
    let file = tsc_rs_parser::parse("test.ts", "class Greeter {}");
    let opts = CompilerOptions {
        experimental_decorators: Some(true),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    assert!(
        !out.javascript.contains("__decorate"),
        "should not emit __decorate helper when no decorators: {}",
        out.javascript
    );
}

// ---------------------------------------------------------------
// Class decorators
// ---------------------------------------------------------------

#[test]
fn test_class_decorator() {
    let js = emit_decorators(
        r#"
function sealed(target: any) {}

@sealed
class Greeter {
    greeting: string;
}
"#,
    );
    assert!(
        js.contains("Greeter = __decorate(["),
        "should emit class __decorate call: {js}"
    );
    assert!(
        js.contains("sealed"),
        "should reference the sealed decorator: {js}"
    );
    assert!(
        js.contains("], Greeter);"),
        "should pass class name to __decorate: {js}"
    );
}

#[test]
fn test_multiple_class_decorators() {
    let js = emit_decorators(
        r#"
function first(target: any) {}
function second(target: any) {}

@first
@second
class Example {}
"#,
    );
    assert!(
        js.contains("Example = __decorate(["),
        "should emit class __decorate call: {js}"
    );
    // Both decorators should be in the list
    assert!(js.contains("first"), "should contain first decorator: {js}");
    assert!(
        js.contains("second"),
        "should contain second decorator: {js}"
    );
}

// ---------------------------------------------------------------
// Method decorators
// ---------------------------------------------------------------

#[test]
fn test_method_decorator() {
    let js = emit_decorators(
        r#"
function log(target: any, key: string, descriptor: PropertyDescriptor) {}

class MyClass {
    @log
    greet() {}
}
"#,
    );
    assert!(
        js.contains("__decorate(["),
        "should emit __decorate for method: {js}"
    );
    assert!(
        js.contains("MyClass.prototype"),
        "should target prototype for instance method: {js}"
    );
    assert!(
        js.contains("\"greet\""),
        "should reference method name: {js}"
    );
    assert!(
        js.contains(", null);"),
        "method descriptor arg should be null: {js}"
    );
}

#[test]
fn test_static_method_decorator() {
    let js = emit_decorators(
        r#"
function log(target: any, key: string) {}

class MyClass {
    @log
    static create() {}
}
"#,
    );
    // Static methods should target the class itself, not prototype
    assert!(
        js.contains("], MyClass, \"create\""),
        "static method should target class directly, not prototype: {js}"
    );
}

// ---------------------------------------------------------------
// Property decorators
// ---------------------------------------------------------------

#[test]
fn test_property_decorator() {
    let js = emit_decorators(
        r#"
function observable(target: any, key: string) {}

class MyClass {
    @observable
    name: string = "test";
}
"#,
    );
    assert!(
        js.contains("__decorate(["),
        "should emit __decorate for property: {js}"
    );
    assert!(
        js.contains("MyClass.prototype"),
        "should target prototype for instance property: {js}"
    );
    assert!(
        js.contains("\"name\""),
        "should reference property name: {js}"
    );
    assert!(
        js.contains("void 0);"),
        "property descriptor arg should be void 0: {js}"
    );
}

#[test]
fn test_static_property_decorator() {
    let js = emit_decorators(
        r#"
function observable(target: any, key: string) {}

class MyClass {
    @observable
    static count: number = 0;
}
"#,
    );
    assert!(
        js.contains("], MyClass, \"count\", void 0);"),
        "static property should target class directly: {js}"
    );
}

// ---------------------------------------------------------------
// Parameter decorators
// ---------------------------------------------------------------

#[test]
fn test_param_helper_emitted_for_parameter_decorators() {
    let js = emit_decorators(
        r#"
function inject(target: any, key: string, index: number) {}

class MyClass {
    greet(@inject name: string) {}
}
"#,
    );
    assert!(
        js.contains("var __param"),
        "should emit __param helper: {js}"
    );
    assert!(
        js.contains("__param(0, inject)"),
        "should emit __param call with index: {js}"
    );
}

#[test]
fn test_constructor_parameter_decorator() {
    let js = emit_decorators(
        r#"
function inject(target: any, key: string, index: number) {}

class MyService {
    constructor(@inject dep: any) {}
}
"#,
    );
    assert!(
        js.contains("var __param"),
        "should emit __param helper for constructor params: {js}"
    );
    // Constructor parameter decorators show up in the class-level __decorate call
    assert!(
        js.contains("__param(0, inject)"),
        "should emit __param for constructor param: {js}"
    );
    assert!(
        js.contains("MyService = __decorate(["),
        "constructor param decorators should appear in class __decorate: {js}"
    );
    assert!(
        !js.contains("@inject"),
        "legacy parameter decorators must be lowered, not preserved: {js}"
    );
}

#[test]
fn test_standard_parameter_decorators_are_preserved_for_esnext() {
    let source = r#"
declare let dec: any;
class C {
    constructor(@dec x: any) {}
    method(@dec x: any) {}
    set x(@dec x: any) {}
    static method(@dec x: any) {}
}
(class { constructor(@dec x: any) {} });
"#;
    let file = tsc_rs_parser::parse("test.ts", source);
    let js = emit(
        &file,
        &CompilerOptions {
            target: Some(ScriptTarget::ESNext),
            ..Default::default()
        },
    )
    .javascript;

    assert!(
        js.contains("constructor(\n    @dec\n    x) { }"),
        "expected constructor parameter decorator syntax to be preserved: {js}"
    );
    assert!(
        js.contains("method(\n    @dec\n    x) { }"),
        "expected method parameter decorator syntax to be preserved: {js}"
    );
    assert!(
        js.contains("set x(\n    @dec\n    x) { }"),
        "expected accessor parameter decorator syntax to be preserved: {js}"
    );
    assert!(
        !js.contains("__param"),
        "standard preserve mode must not use the legacy helper: {js}"
    );
}

#[test]
fn test_metadata_unions_keep_nullish_constituents() {
    let source = r#"
declare const dec: any;
declare class A {}
class C {
    @dec stringOrNull: "value" | null;
    @dec booleanOrNever: true | never;
    @dec classOrUndefined: A | undefined;
    @dec onlyNullish: null | undefined;
}
"#;
    let file = tsc_rs_parser::parse("test.ts", source);
    let js = emit(
        &file,
        &CompilerOptions {
            experimental_decorators: Some(true),
            emit_decorator_metadata: Some(true),
            strict_null_checks: Some(true),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    )
    .javascript;

    assert_eq!(
        js.matches("__metadata(\"design:type\", Object)").count(),
        2,
        "mixed nullish unions should fall back to Object: {js}"
    );
    assert!(
        js.contains("__metadata(\"design:type\", Boolean)"),
        "never should not change a union's runtime metadata type: {js}"
    );
    assert!(
        js.contains("__metadata(\"design:type\", void 0)"),
        "an entirely nullish union should remain void 0: {js}"
    );

    let no_strict_file = tsc_rs_parser::parse(
        "test.ts",
        "declare const dec: any; class C { @dec value: string | null; }",
    );
    let no_strict_js = emit(
        &no_strict_file,
        &CompilerOptions {
            experimental_decorators: Some(true),
            emit_decorator_metadata: Some(true),
            strict_null_checks: Some(false),
            target: Some(ScriptTarget::ES2015),
            ..Default::default()
        },
    )
    .javascript;
    assert!(
        no_strict_js.contains("__metadata(\"design:type\", String)"),
        "non-strict metadata should erase nullish union constituents: {no_strict_js}"
    );
}

#[test]
fn test_metadata_union_import_retention_matches_serialization() {
    let source = r#"
import { Runtime } from "runtime";
import { Mixed } from "mixed";
declare const dec: any;
class C {
    @dec sameRuntime: Runtime<string> | Runtime<number>;
    @dec heterogeneous: Mixed | void;
}
"#;
    let file = tsc_rs_parser::parse("test.ts", source);
    let emit_with_module = |module| {
        emit(
            &file,
            &CompilerOptions {
                experimental_decorators: Some(true),
                emit_decorator_metadata: Some(true),
                strict_null_checks: Some(false),
                target: Some(ScriptTarget::ES2015),
                module: Some(module),
                ..Default::default()
            },
        )
        .javascript
    };

    let js = emit_with_module(ModuleKind::ES2015);

    assert!(
        js.contains("from \"runtime\""),
        "a same-runtime union must retain its runtime import: {js}"
    );
    assert!(
        !js.contains("from \"mixed\""),
        "a union serialized as Object must not retain an unused import: {js}"
    );
    assert!(
        js.contains("__metadata(\"design:type\", Runtime)"),
        "same-runtime union metadata should use the imported value: {js}"
    );
    assert!(
        js.contains("__metadata(\"design:type\", Object)"),
        "heterogeneous union metadata should use Object: {js}"
    );

    let cjs = emit_with_module(ModuleKind::CommonJS);
    assert!(
        cjs.contains("require(\"runtime\")")
            && cjs.contains("__metadata(\"design:type\", runtime_1.Runtime)"),
        "a same-runtime union must retain and qualify its CommonJS import: {cjs}"
    );
    assert!(
        !cjs.contains("require(\"mixed\")"),
        "an Object union must not retain an unused CommonJS import: {cjs}"
    );
}

#[test]
fn test_metadata_union_import_retention_respects_strict_null_checks() {
    let source = r#"
import { Runtime } from "runtime";
declare const dec: any;
class C {
    @dec value: Runtime | undefined;
}
"#;
    let file = tsc_rs_parser::parse("test.ts", source);
    let emit_with_strict_null_checks = |strict_null_checks| {
        emit(
            &file,
            &CompilerOptions {
                experimental_decorators: Some(true),
                emit_decorator_metadata: Some(true),
                strict_null_checks: Some(strict_null_checks),
                target: Some(ScriptTarget::ES2015),
                module: Some(ModuleKind::ES2015),
                ..Default::default()
            },
        )
        .javascript
    };

    let loose = emit_with_strict_null_checks(false);
    assert!(
        loose.contains("from \"runtime\"")
            && loose.contains("__metadata(\"design:type\", Runtime)"),
        "non-strict nullish filtering must retain the runtime import: {loose}"
    );

    let strict = emit_with_strict_null_checks(true);
    assert!(
        !strict.contains("from \"runtime\"")
            && strict.contains("__metadata(\"design:type\", Object)"),
        "strict nullish unions must serialize to Object without retaining the import: {strict}"
    );
}

#[test]
fn test_es5_metadata_import_collision_downlevels_runtime_and_decorated_classes() {
    let options = CompilerOptions {
        experimental_decorators: Some(true),
        emit_decorator_metadata: Some(true),
        no_emit_helpers: Some(true),
        target: Some(ScriptTarget::ES5),
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };

    let db = tsc_rs_parser::parse(
        "db.ts",
        r#"
export class db {
    typeOnly: string;
    constructor(value: string) { this.value = value; }
    public doSomething(prefix: string) { return prefix + this.value; }
}
"#,
    );
    let db_js = emit(&db, &options).javascript;
    assert!(
        db_js.contains("var db = /** @class */ (function () {")
            && db_js.contains("function db(value)")
            && db_js.contains("this.value = value;")
            && db_js.contains("db.prototype.doSomething = function (prefix)")
            && db_js.contains("return prefix + this.value;")
            && !db_js.contains("prototype.typeOnly")
            && !db_js.contains("class db"),
        "metadata-enabled ES5 emit must preserve method runtime semantics and erase type-only fields: {db_js}"
    );

    let service = tsc_rs_parser::parse(
        "service.ts",
        r#"
import { db } from "./db";
declare function someDecorator(target: unknown): unknown;
@someDecorator
class MyClass {
    db: db;
    constructor(db: db) { this.db = db; }
}
export { MyClass };
"#,
    );
    let service_js = emit(&service, &options).javascript;
    assert!(
        service_js.contains("var MyClass = /** @class */ (function () {")
            && service_js.contains("var db_1 = require(\"./db\");")
            && service_js.contains("this.db = db;")
            && !service_js.contains("this.db = db_1.db;")
            && service_js.contains("__metadata(\"design:paramtypes\", [db_1.db])")
            && !service_js.contains("class MyClass"),
        "decorated ES5 classes must lower without changing imported metadata names: {service_js}"
    );
    let decorator_pos = service_js.find("someDecorator,").unwrap();
    let metadata_pos = service_js
        .find("__metadata(\"design:paramtypes\", [db_1.db])")
        .unwrap();
    let return_pos = service_js.find("return MyClass;").unwrap();
    assert!(
        decorator_pos < metadata_pos && metadata_pos < return_pos,
        "class decorators and metadata must stay ordered inside the legacy IIFE: {service_js}"
    );

    let default_service = tsc_rs_parser::parse(
        "default-service.ts",
        r#"
import db from "./db";
declare function someDecorator(target: unknown): unknown;
@someDecorator
class DefaultConsumer {
    value: db;
    constructor(db: db) { this.value = db; }
}
"#,
    );
    let default_js = emit(&default_service, &options).javascript;
    assert!(
        default_js.contains("this.value = db;")
            && default_js.contains("__metadata(\"design:paramtypes\", [db_1.default])")
            && !default_js.contains("this.value = db_1.default;"),
        "default-import metadata must stay qualified while the shadowing parameter stays local: {default_js}"
    );

    let unsupported = tsc_rs_parser::parse(
        "unsupported.ts",
        "declare const dec: any; class Unsupported { @dec method() {} }",
    );
    let unsupported_js = emit(&unsupported, &options).javascript;
    assert!(
        unsupported_js.contains("class Unsupported")
            && unsupported_js.contains("__decorate(["),
        "decorated member shapes outside the narrow gate must keep the existing safe path: {unsupported_js}"
    );
}

#[test]
fn test_es5_non_metadata_import_collision_preserves_existing_emit() {
    let file = tsc_rs_parser::parse(
        "plain.ts",
        r#"
import { db } from "./db";
export class Plain {
    constructor(db: db) { this.db = db; }
}
"#,
    );
    let javascript = emit(
        &file,
        &CompilerOptions {
            target: Some(ScriptTarget::ES5),
            module: Some(ModuleKind::CommonJS),
            ..Default::default()
        },
    )
    .javascript;

    assert_eq!(
        javascript,
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.Plain = void 0;\n",
            "var db_1 = require(\"./db\");\n",
            "var Plain = /** @class */ (function () {\n",
            "    function Plain(db) { this.db = db_1.db; }\n",
            "    return Plain;\n",
            "}());\n",
            "exports.Plain = Plain;\n",
        ),
        "non-metadata ES5 CommonJS class lowering must remain byte-identical"
    );
}

// ---------------------------------------------------------------
// Decorator factories (decorator returning decorator)
// ---------------------------------------------------------------

#[test]
fn test_decorator_factory() {
    let js = emit_decorators(
        r#"
function color(value: string) {
    return function (target: any) {};
}

@color("red")
class Car {}
"#,
    );
    assert!(
        js.contains("Car = __decorate(["),
        "should emit class __decorate: {js}"
    );
    // The factory call should appear as the decorator expression
    assert!(
        js.contains("color(\"red\")"),
        "should emit decorator factory call: {js}"
    );
}

#[test]
fn test_method_decorator_factory() {
    let js = emit_decorators(
        r#"
function enumerable(value: boolean) {
    return function (target: any, key: string, descriptor: PropertyDescriptor) {};
}

class Greeter {
    @enumerable(false)
    greet() { return "Hello"; }
}
"#,
    );
    assert!(
        js.contains("enumerable(false)"),
        "should emit decorator factory call for method: {js}"
    );
    assert!(
        js.contains("Greeter.prototype, \"greet\""),
        "should target method on prototype: {js}"
    );
}

// ---------------------------------------------------------------
// Combined decorators
// ---------------------------------------------------------------

#[test]
fn test_class_with_multiple_decorator_types() {
    let js = emit_decorators(
        r#"
function classDecorator(target: any) {}
function methodDecorator(target: any, key: string) {}
function propDecorator(target: any, key: string) {}

@classDecorator
class MyClass {
    @propDecorator
    name: string = "";

    @methodDecorator
    greet() {}
}
"#,
    );
    // Should have the __decorate helper
    assert!(
        js.contains("var __decorate"),
        "should emit __decorate helper: {js}"
    );
    // Should have method decorator application
    assert!(
        js.contains("\"greet\", null);"),
        "should emit method decorator: {js}"
    );
    // Should have property decorator application
    assert!(
        js.contains("\"name\", void 0);"),
        "should emit property decorator: {js}"
    );
    // Should have class decorator application
    assert!(
        js.contains("MyClass = __decorate(["),
        "should emit class decorator: {js}"
    );
}

#[test]
fn test_no_decorate_helper_without_experimental_decorators_option() {
    // Without experimentalDecorators, decorators should not produce __decorate calls
    let file = tsc_rs_parser::parse(
        "test.ts",
        r#"
function sealed(target: any) {}
@sealed
class Greeter {}
"#,
    );
    let opts = CompilerOptions::default(); // No experimentalDecorators
    let out = emit(&file, &opts);
    assert!(
        !out.javascript.contains("__decorate"),
        "should not emit __decorate without experimentalDecorators: {}",
        out.javascript
    );
}
