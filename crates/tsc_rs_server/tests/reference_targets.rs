//! Exact constructor reference spans captured independently with TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, Span};
use tsc_rs_query::QueryEngine;
use tsc_rs_server::harness_api::{references_at, ProjectState};

fn check(files: &[(&str, &str)], marker: &str, position: u32, expected: &[(&str, u32, u32)]) {
    let mut options = CompilerOptions::default();
    options.no_lib = Some(false);
    let mut engine = QueryEngine::with_options(options);
    for (file, source) in files {
        engine
            .add_source(file.to_string(), source.to_string())
            .unwrap();
    }
    let _ = engine.check_all();
    let symbols = engine.get_file_symbols(marker).unwrap();
    let project = ProjectState::from_file_map(
        files
            .iter()
            .filter(|(f, _)| *f != marker)
            .map(|(file, source)| {
                (
                    file.to_string(),
                    (
                        source.to_string(),
                        engine.get_file_symbols(file).unwrap().clone(),
                    ),
                )
            })
            .collect(),
    );
    let mut actual = references_at(&engine, symbols, marker, position, Some(&project));
    actual.sort_by_key(|(f, s)| (f.clone(), s.start, s.end));
    let mut expected: Vec<_> = expected
        .iter()
        .map(|(f, s, e)| (f.to_string(), Span::new(*s, *e)))
        .collect();
    expected.sort_by_key(|(f, s)| (f.clone(), s.start, s.end));
    assert_eq!(actual, expected);
}

#[test]
fn constructor_overloads() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor(n: number); constructor(); constructor(n?: number) {} } new A(); new A(1); let a: A; A;"####,
        )],
        r####"/a.ts"####,
        10,
        &[
            (r####"/a.ts"####, 10, 21),
            (r####"/a.ts"####, 34, 45),
            (r####"/a.ts"####, 49, 60),
            (r####"/a.ts"####, 82, 83),
            (r####"/a.ts"####, 91, 92),
        ],
    );
}

#[test]
fn constructor_modifier() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { private constructor() {} } new A(); let a: A;"####,
        )],
        r####"/a.ts"####,
        10,
        &[(r####"/a.ts"####, 18, 29), (r####"/a.ts"####, 41, 42)],
    );
}

#[test]
fn constructor_shadowed_class() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor() {} } new A(); function f() { class A { constructor() {} } new A(); }"####,
        )],
        r####"/a.ts"####,
        10,
        &[(r####"/a.ts"####, 10, 21), (r####"/a.ts"####, 33, 34)],
    );
}

#[test]
fn constructor_inherited() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor(s: string) {} } class B extends A {} class C extends B { constructor() { super(""); } } new A(""); new B(""); new C();"####,
        )],
        r####"/a.ts"####,
        10,
        &[
            (r####"/a.ts"####, 10, 21),
            (r####"/a.ts"####, 114, 115),
            (r####"/a.ts"####, 95, 100),
            (r####"/a.ts"####, 125, 126),
        ],
    );
}

#[test]
fn constructor_derived() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor() {} } class B extends A { constructor() { super(); } } class C extends B {} new A(); new B(); new C();"####,
        )],
        r####"/a.ts"####,
        49,
        &[
            (r####"/a.ts"####, 49, 60),
            (r####"/a.ts"####, 112, 113),
            (r####"/a.ts"####, 121, 122),
        ],
    );
}

#[test]
fn constructor_alias() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor() {} } const Alias = A; new A(); new Alias();"####,
        )],
        r####"/a.ts"####,
        10,
        &[(r####"/a.ts"####, 10, 21), (r####"/a.ts"####, 50, 51)],
    );
}

#[test]
fn constructor_import() {
    check(
        &[
            (
                r####"/a.ts"####,
                r####"export class A { constructor() {} }"####,
            ),
            (
                r####"/b.ts"####,
                r####"import { A as B } from "./a"; new B(); let b: B;"####,
            ),
        ],
        r####"/a.ts"####,
        17,
        &[
            (r####"/a.ts"####, 17, 28),
            (r####"/b.ts"####, 9, 10),
            (r####"/b.ts"####, 34, 35),
        ],
    );
}

#[test]
fn constructor_namespace() {
    check(
        &[
            (
                r####"/a.ts"####,
                r####"export class A { constructor() {} }"####,
            ),
            (
                r####"/b.ts"####,
                r####"import * as ns from "./a"; new ns.A(); let b: ns.A;"####,
            ),
        ],
        r####"/a.ts"####,
        17,
        &[(r####"/a.ts"####, 17, 28), (r####"/b.ts"####, 34, 35)],
    );
}

#[test]
fn constructor_super() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor() {} } class B extends A { constructor() { super(); } } class C implements A { constructor() { super(); } }"####,
        )],
        r####"/a.ts"####,
        10,
        &[(r####"/a.ts"####, 10, 21), (r####"/a.ts"####, 65, 70)],
    );
}

#[test]
fn constructor_named_method() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor() {} method() {} } const o = { constructor() {} }; o.constructor(); new A();"####,
        )],
        r####"/a.ts"####,
        10,
        &[(r####"/a.ts"####, 10, 21), (r####"/a.ts"####, 94, 95)],
    );
}

#[test]
fn constructor_static_this() {
    check(
        &[(
            r####"/a.ts"####,
            r####"class A { constructor() {} static f() { this.f(); new this(); } } new A();"####,
        )],
        r####"/a.ts"####,
        10,
        &[
            (r####"/a.ts"####, 10, 21),
            (r####"/a.ts"####, 54, 58),
            (r####"/a.ts"####, 70, 71),
        ],
    );
}
