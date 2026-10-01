// Messages and byte spans verified against TypeScript 6.0.3.
use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn check(source: &str, expected: &[(u32, u32, u32, &str)]) {
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(false),
            no_lib: Some(true),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, 2341 | 2445 | 2446))
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                span.start,
                span.end - span.start,
                d.message.as_str(),
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn destructuring_bindings() {
    check(
        r###"class B { private p=1; protected q=2; private m() {} } const b=new B(); const {p, q, m}=b; const {p:a, q:c}=b; const {p:d=0, q:e=0}=b;"###,
        &[
            (
                2341,
                79,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                82,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2341,
                85,
                1,
                r###"Property 'm' is private and only accessible within class 'B'."###,
            ),
            (
                2341,
                98,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                103,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2341,
                118,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                125,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn destructuring_legal_contexts() {
    check(
        r###"class B {private p=1; protected q=2; f(b:B){const {p,q}=b; const {p:a,q:c}=this;} } class D extends B {f(b:B){const {q}=this; const {p}=this; const {q:other}=b;}}"###,
        &[
            (
                2341,
                133,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2446,
                149,
                1,
                r###"Property 'q' is protected and only accessible through an instance of class 'D'. This is an instance of class 'B'."###,
            ),
        ],
    );
}

#[test]
fn destructuring_nested() {
    check(
        r###"class B {private p=1; protected q=2;} const box={inner:new B()}; const {inner:{p,q}}=box; const [{p:a,q:b}]:[B]=[new B()];"###,
        &[
            (
                2341,
                79,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                81,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2341,
                98,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                102,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn destructuring_parameters() {
    check(
        r###"class B {private p=1; protected q=2;} function f({p,q}:B){} const arrow=({p,q}:B)=>{}; const expr=function({p,q}:B){}; const obj={m({p,q}:B){}}; class Other {m({p,q}:B){}}"###,
        &[
            (
                2341,
                50,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                52,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2341,
                74,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                76,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2341,
                108,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                110,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2341,
                133,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                135,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2341,
                161,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                163,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn destructuring_keys() {
    check(
        r###"class B {private p=1; protected q=2;} const b=new B(); const {"p":a, ["q"]:c}=b;"###,
        &[
            (
                2341,
                62,
                3,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                69,
                5,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn destructuring_generic_and_rest() {
    check(
        r###"class B {private p=1; protected q=2; public r=3;} function f<T extends B>(value:T){const {p,q}=value;} const {...rest}=new B(); const {r}=rest; class Public extends B {q=4;} const {q}=new Public();"###,
        &[
            (
                2341,
                90,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                92,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn destructuring_loops() {
    check(
        r###"class B {private p=1; protected q=2;} for(const {p,q} of [new B()]) {}"###,
        &[
            (
                2341,
                49,
                1,
                r###"Property 'p' is private and only accessible within class 'B'."###,
            ),
            (
                2445,
                51,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}
