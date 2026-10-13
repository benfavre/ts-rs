//! Member access through program type aliases, `Record<string, V>` and
//! `noUncheckedIndexedAccess` element reads (TS2339), `keyof` of a known
//! object type (TS2322 / TS2820), and value exports of an interface merged
//! with a same-named variable (no TS2359 on `instanceof`).

use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn strict() -> CompilerOptions {
    CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    }
}

fn check(file_name: &str, source: &str, options: &CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse(file_name, source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, options)
        .diagnostics
}

/// Checks `use.ts` in a program that also holds `files` (name, source).
fn check_program(files: &[(&str, &str)], use_source: &str) -> Vec<Diagnostic> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after Unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "tsc-rs-alias-member-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create fixture directory");
    let mut parsed = Vec::new();
    for (name, source) in files {
        let path = directory.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture subdirectory");
        }
        std::fs::write(&path, source).expect("write fixture");
        parsed.push(tsc_rs_parser::parse(
            path.to_str().expect("utf-8 path"),
            source,
        ));
    }
    let use_path = directory.join("use.ts");
    std::fs::write(&use_path, use_source).expect("write fixture");
    let use_file = tsc_rs_parser::parse(use_path.to_str().expect("utf-8 path"), use_source);
    let symbols = tsc_rs_symbols::bind(&use_file);
    let mut program: Vec<&tsc_rs_ast::SourceFile> = parsed.iter().collect();
    program.push(&use_file);
    let mut checker = TypeChecker::new();
    checker.inject_external_types(&program);
    checker.take_diagnostics();
    let diagnostics = checker
        .check_with_options(&use_file, &symbols, &strict())
        .diagnostics;
    std::fs::remove_dir_all(&directory).expect("remove fixture directory");
    diagnostics
}

fn messages(diagnostics: &[Diagnostic], code: u32) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<u32> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn missing_member_of_an_object_type_alias_is_reported() {
    let diagnostics = check(
        "alias_receiver.ts",
        r#"
type T1 = { a: number };
declare const d1: T1;
d1.nope1;
const c1: T1 = { a: 1 };
c1.nope2;
type N = { v: { a: number } };
declare const n: N;
n.v.nope3;
const s: string = n.v;
type G<T> = { v: T };
declare const g: G<number>;
g.nope4;
d1.a;
n.v.a;
g.v;
"#,
        &strict(),
    );
    assert_eq!(
        messages(&diagnostics, 2339),
        vec![
            "Property 'nope1' does not exist on type 'T1'.",
            "Property 'nope2' does not exist on type 'T1'.",
            "Property 'nope3' does not exist on type '{ a: number; }'.",
            "Property 'nope4' does not exist on type 'G<number>'.",
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        messages(&diagnostics, 2322),
        vec!["Type '{ a: number; }' is not assignable to type 'string'."],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(diagnostics.len(), 5, "diagnostics: {diagnostics:?}");
}

#[test]
fn alias_receivers_whose_members_are_open_stay_silent() {
    let diagnostics = check(
        "alias_receiver_negatives.ts",
        r#"
type Idx = { [k: string]: number };
declare const i: Idx; i.anything;
type Al = Idx; declare const al: Al; al.foo;
type Opt = { a?: number }; declare const o: Opt; o.a;
type U = { a: number } | { a: string; b: number }; declare const u: U; u.a;
type X = { a: number } & { b: string }; declare const x: X; x.a; x.b; x.c;
type AnyProp = { a: any }; declare const ap: AnyProp; ap.zz;
type Keys = "p" | "q"; type M = { [K in Keys]: number }; declare const m: M; m.p;
type M2<K extends string> = { [P in K]: number }; declare const m2: M2<string>; m2.whatever;
type Om = Omit<{ a: number; b: number }, "a">; declare const om: Om; om.b;
type Meth = { f(): void; get g(): number; set h(v: string) };
declare const me: Meth; me.f(); me.g; me.h; me.toString(); me.hasOwnProperty("x");
type Fn = { (): void; extra: number }; declare const fn: Fn; fn.call; fn.bind; fn.extra;
interface IM { a: number } interface IM { [k: string]: number }
type AI = IM; declare const ai: AI; ai.zzz;
type Unknowns = Record<string, unknown>; declare const ru: Unknowns; ru.k;
declare const rec: Record<string, unknown>; rec.anything;
function generic<T>(value: T) { return value; }
"#,
        &strict(),
    );
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 2339),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn javascript_alias_receivers_keep_expando_writes() {
    let diagnostics = check(
        "alias_receiver.js",
        r#"
/** @typedef {{ a: number }} T1 */
/** @type {T1} */
const t = { a: 1 };
t.extra = 2;
"#,
        &strict(),
    );
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code != 2339),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn accessor_signatures_of_a_type_literal_are_properties() {
    let diagnostics = check(
        "accessor_signatures.ts",
        r#"
declare const both: { get g(): number; set h(v: string); f(): void };
const n: number = both.g;
const s: string = both.h;
both.nope;
const literal: { get g(): number } = { g: 1 };
"#,
        &strict(),
    );
    assert_eq!(
        messages(&diagnostics, 2339),
        // TypeScript 6.0.3 prints getter-only signatures as readonly properties.
        vec!["Property 'nope' does not exist on type '{ readonly g: number; h: string; f(): void; }'."],
        "diagnostics: {diagnostics:?}"
    );
    assert_eq!(diagnostics.len(), 1, "diagnostics: {diagnostics:?}");
}

#[test]
fn unchecked_index_read_reports_the_missing_member_after_possibly_undefined() {
    let options = CompilerOptions {
        strict: Some(true),
        no_unchecked_indexed_access: Some(true),
        ..CompilerOptions::default()
    };
    let diagnostics = check(
        "unchecked_index.ts",
        r#"
interface E { quantity: number }
const list: E[] = [];
list[0].quantityBOGUS;
list[0].quantity;
declare const ix: { [k: string]: E };
ix["k"].quantityBOGUS;
"#,
        &options,
    );
    assert_eq!(
        codes(&diagnostics),
        vec![2532, 2339, 2532, 2532, 2339],
        "{diagnostics:?}"
    );
    assert_eq!(
        messages(&diagnostics, 2339),
        vec![
            "Property 'quantityBOGUS' does not exist on type 'E'.",
            "Property 'quantityBOGUS' does not exist on type 'E'.",
        ]
    );
}

#[test]
fn record_of_string_keys_reads_report_missing_members() {
    for unchecked in [false, true] {
        let options = CompilerOptions {
            strict: Some(true),
            no_unchecked_indexed_access: Some(unchecked),
            ..CompilerOptions::default()
        };
        let diagnostics = check(
            "record_reads.ts",
            r#"
interface E { quantity: number }
const rec: Record<string, E> = {};
rec["k"].quantityBOGUS;
rec.k.quantityBOGUS;
rec["k"].quantity;
rec.k.quantity;
type RL = Record<"a" | "b", E>; declare const rl: RL; rl.a.quantity;
"#,
            &options,
        );
        assert_eq!(
            messages(&diagnostics, 2339),
            vec![
                "Property 'quantityBOGUS' does not exist on type 'E'.",
                "Property 'quantityBOGUS' does not exist on type 'E'.",
            ],
            "unchecked={unchecked}: {diagnostics:?}"
        );
        assert_eq!(
            diagnostics.len(),
            2,
            "unchecked={unchecked}: {diagnostics:?}"
        );
    }
}

#[test]
fn keyof_a_known_object_type_is_its_member_names() {
    let diagnostics = check(
        "keyof_object.ts",
        r#"
type K1 = keyof { A: 1; CONFLICT: 2 };
const k1: K1 = "Z";
declare const CODES: { readonly A: -1; readonly CONFLICT: -2 };
type K2 = keyof typeof CODES;
const k2: K2 = "CONFLICTZ";
const k3: keyof typeof CODES = "A";
declare function f(opts: { code: K2 }): void;
f({ code: "CONFLICT" });
const numeric: keyof { 0: string } = 0;
declare const open: { [k: string]: number };
const anyKey: keyof typeof open = "x";
"#,
        &strict(),
    );
    assert_eq!(codes(&diagnostics), vec![2322, 2820], "{diagnostics:?}");
    assert_eq!(
        diagnostics[1].message,
        "Type '\"CONFLICTZ\"' is not assignable to type 'K2'. Did you mean '\"CONFLICT\"'?"
    );
}

#[test]
fn misspelled_string_literal_suggests_the_union_member() {
    let diagnostics = check(
        "did_you_mean.ts",
        r#"
type K = "A" | "CONFLICT" | "NOT_FOUND";
const k1: K = "CONFLICTZ";
declare function g(code: K): void;
g("CONFLICTZ");
declare function f(o: { code: K }): void;
f({ code: "CONFLICTZ" });
const o: { code: K } = { code: "NOTFOUND" };
let k2: K = "A"; k2 = "conflict";
const k3: K = "B";
"#,
        &strict(),
    );
    assert_eq!(
        codes(&diagnostics),
        vec![2820, 2345, 2820, 2820, 2820, 2322],
        "{diagnostics:?}"
    );
    assert_eq!(
        diagnostics[0].message,
        "Type '\"CONFLICTZ\"' is not assignable to type 'K'. Did you mean '\"CONFLICT\"'?"
    );
    assert_eq!(
        diagnostics[3].message,
        "Type '\"NOTFOUND\"' is not assignable to type 'K'. Did you mean '\"NOT_FOUND\"'?"
    );
}

#[test]
fn literal_union_alias_from_another_module_checks_literal_sources() {
    let diagnostics = check_program(
        &[(
            "codes.d.ts",
            r#"
declare const CODES: { readonly A: -1; readonly CONFLICT: -2 };
type KEY = keyof typeof CODES;
declare class Err extends Error {
    constructor(opts: { message?: string; code: KEY });
}
type Mode = "light" | "dark";
export { Err, KEY, Mode };
"#,
        )],
        r#"
import { Err } from "./codes";
import type { Mode } from "./codes";
export const bad = new Err({ code: "CONFLICTZ", message: "x" });
export const good = new Err({ code: "CONFLICT" });
const mode: Mode = "dim";
const fine: Mode = "dark";
declare const wide: string;
const lenient: Mode = wide as Mode;
"#,
    );
    assert_eq!(codes(&diagnostics), vec![2820, 2322], "{diagnostics:?}");
}

#[test]
fn interface_merged_with_a_variable_exports_the_variable_value() {
    let diagnostics = check_program(
        &[(
            "errors.d.ts",
            r#"
export interface $constructor<T> {
    new (def: unknown): T;
    init(inst: T, def: unknown): asserts inst is T;
}
export interface ZodError { format(): string }
export declare const ZodError: $constructor<ZodError>;
"#,
        )],
        r#"
import { ZodError } from "./errors";
declare const e: unknown;
export const r = e instanceof ZodError;
declare const z: ZodError;
const n: number = z.format();
"#,
    );
    assert_eq!(codes(&diagnostics), vec![2322], "{diagnostics:?}");
}

/// A library class reached through a bundler-style re-export chain, while
/// other program files declare a class and an alias of the same names: both
/// the renamed direct import and a barrel re-export construct the library's
/// class (whatever order the files are registered in).
#[test]
fn library_class_through_reexports_is_not_a_same_named_program_class() {
    let chunk = r#"
declare const CODES: { readonly CONFLICT: -1; readonly NOT_FOUND: -2 };
type KEY = keyof typeof CODES;
declare class TRPCError extends Error {
    readonly code: KEY;
    constructor(opts: { message?: string; code: KEY });
}
export { TRPCError as zr, KEY as Wr };
"#;
    let index = r#"
import { zr as TRPCError, Wr as TRPC_ERROR_CODE_KEY } from "./chunk";
export { TRPCError, type TRPC_ERROR_CODE_KEY };
"#;
    let barrel = r#"export { TRPCError } from "./server/index";
"#;
    let decoy = r#"
class TRPCError extends Error {
    code: string;
    constructor(opts: { code: string; message?: string }) { super(opts.message); this.code = opts.code; }
}
type KEY = string;
export const decoy = new TRPCError({ code: "anything" });
"#;
    let use_source = r#"
import { TRPCError as Raw } from "./server/index";
import { TRPCError } from "./barrel";
export const direct = new Raw({ code: "CONFLICTZ", message: "x" });
export const reexported = new TRPCError({ code: "CONFLICTZ", message: "x" });
export const fine = new TRPCError({ code: "NOT_FOUND" });
"#;
    for files in [
        vec![
            ("decoy.test.ts", decoy),
            ("server/chunk.d.ts", chunk),
            ("server/index.d.ts", index),
            ("barrel.ts", barrel),
        ],
        vec![
            ("server/chunk.d.ts", chunk),
            ("server/index.d.ts", index),
            ("barrel.ts", barrel),
            ("decoy.test.ts", decoy),
        ],
    ] {
        let diagnostics = check_program(&files, use_source);
        assert_eq!(codes(&diagnostics), vec![2820, 2820], "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.message.ends_with("Did you mean '\"CONFLICT\"'?")),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn const_asserted_object_literal_property_keeps_its_literal_type() {
    let diagnostics = check(
        "const_asserted_property.ts",
        r#"
declare function f(o: { mode: "manual" | "auto"; n: number }): void;
const opts = { mode: "manual" as const, n: 1 };
f(opts);
let opts2 = { mode: <const>"auto", n: 1 };
f(opts2);
const widened = { mode: "manual", n: 1 };
f(widened);
"#,
        &strict(),
    );
    assert_eq!(codes(&diagnostics), vec![2345], "{diagnostics:?}");
}
