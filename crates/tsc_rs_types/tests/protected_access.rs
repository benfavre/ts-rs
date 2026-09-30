// Diagnostic messages and byte spans verified with TypeScript 6.0.3.
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
            no_lib: Some(true),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, 2445 | 2446))
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
fn protected_outside() {
    check(
        r###"class B { protected p=1; protected m() {} protected static s=1; constructor(protected q=1) {} } const b=new B(); b.p; b.m(); b.q; B.s;"###,
        &[
            (
                2445,
                115,
                1,
                r###"Property 'p' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2445,
                120,
                1,
                r###"Property 'm' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2445,
                127,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2445,
                132,
                1,
                r###"Property 's' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn protected_receivers() {
    check(
        r###"class B { protected p=1; protected m() {} protected static s=1; } class D extends B { f(b:B,d:D,s:S) { this.p; d.p; b.p; s.p; super.m(); B.s; D.s; } } class S extends B {} class Public extends B { p=2; } new Public().p;"###,
        &[
            (
                2446,
                118,
                1,
                r###"Property 'p' is protected and only accessible through an instance of class 'D'. This is an instance of class 'B'."###,
            ),
            (
                2446,
                123,
                1,
                r###"Property 'p' is protected and only accessible through an instance of class 'D'. This is an instance of class 'S'."###,
            ),
        ],
    );
}

#[test]
fn protected_redeclared() {
    check(
        r###"class B { protected p=1; f(d:D) { d.p; } } class D extends B { protected p=2; f(b:B) { b.p; } }"###,
        &[
            (
                2445,
                36,
                1,
                r###"Property 'p' is protected and only accessible within class 'D' and its subclasses."###,
            ),
            (
                2446,
                89,
                1,
                r###"Property 'p' is protected and only accessible through an instance of class 'D'. This is an instance of class 'B'."###,
            ),
        ],
    );
}

#[test]
fn protected_contextual() {
    check(
        r###"class B { protected p=1; protected static s=1; } class D extends B {} function f(this:D,b:B,d:D) { this.p; d.p; b.p; B.s; const arrow=()=>d.p; function nested() { d.p; } } const g: (this:B)=>void = function() { this.p; };"###,
        &[
            (
                2446,
                114,
                1,
                r###"Property 'p' is protected and only accessible through an instance of class 'D'. This is an instance of class 'B'."###,
            ),
            (
                2445,
                119,
                1,
                r###"Property 's' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2445,
                165,
                1,
                r###"Property 'p' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn protected_nested() {
    check(
        r###"class B { protected p=1; } class D extends B { f(b:B,d:D) { class Inner { f() { b.p; d.p; } } } }"###,
        &[(
            2446,
            82,
            1,
            r###"Property 'p' is protected and only accessible through an instance of class 'D'. This is an instance of class 'B'."###,
        )],
    );
}

#[test]
fn protected_generic() {
    check(
        r###"class B<T> { protected p!: T; } class D<T> extends B<T> { f<U extends D<T>>(b:B<string>,d:U) { b.p; d.p; } } declare const b:B<number>; b.p;"###,
        &[
            (
                2446,
                97,
                1,
                r###"Property 'p' is protected and only accessible through an instance of class 'D<T>'. This is an instance of class 'B<string>'."###,
            ),
            (
                2445,
                138,
                1,
                r###"Property 'p' is protected and only accessible within class 'B<T>' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn protected_statics() {
    check(
        r###"class B { protected static s=1; } class D extends B { static f() { B.s; D.s; S.s; super.s; } } class S extends B {} B.s; D.s; S.s;"###,
        &[
            (
                2445,
                118,
                1,
                r###"Property 's' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2445,
                123,
                1,
                r###"Property 's' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2445,
                128,
                1,
                r###"Property 's' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn protected_accessors() {
    check(
        r###"class B { protected get p() {return 1;} protected set q(v:number) {} } let b=new B(); b.p; b.q=2;"###,
        &[
            (
                2445,
                88,
                1,
                r###"Property 'p' is protected and only accessible within class 'B' and its subclasses."###,
            ),
            (
                2445,
                93,
                1,
                r###"Property 'q' is protected and only accessible within class 'B' and its subclasses."###,
            ),
        ],
    );
}

#[test]
fn protected_bracket() {
    check(
        r###"class B { protected p=1; } let b=new B(); b['p'];"###,
        &[],
    );
}

#[test]
fn protected_deep_hierarchy_has_no_artificial_access_limit() {
    let mut source = String::from("class B { protected p = 1; }");
    let mut base = String::from("B");
    for i in 0..24 {
        let name = format!("C{i}");
        source.push_str(&format!("class {name} extends {base} {{}}"));
        base = name;
    }
    source.push_str(&format!(
        "class Last extends {base} {{ f(other: Last) {{ other.p; this.p; }} }}"
    ));
    check(&source, &[]);
}

#[test]
fn protected_setters_are_checked_on_writes_not_public_getter_reads() {
    check(
        "class B { get p(){return 1} protected set p(v:number){} } const b=new B(); b.p; b.p=2; b.p++; ++b.p; b.p += 1;",
        &[
            (2445, 82, 1, "Property 'p' is protected and only accessible within class 'B' and its subclasses."),
            (2445, 89, 1, "Property 'p' is protected and only accessible within class 'B' and its subclasses."),
            (2445, 98, 1, "Property 'p' is protected and only accessible within class 'B' and its subclasses."),
            (2445, 103, 1, "Property 'p' is protected and only accessible within class 'B' and its subclasses."),
        ],
    );
}

#[test]
fn first_declaration_controls_duplicate_property_visibility() {
    check(
        "class A { p=1; constructor(protected p:number) {} } new A(1).p;",
        &[],
    );
    check(
        "class A { protected p=1; constructor(public p:number) {} } new A(1).p;",
        &[(
            2445,
            68,
            1,
            "Property 'p' is protected and only accessible within class 'A' and its subclasses.",
        )],
    );
}

#[test]
fn object_rest_copies_public_data_properties() {
    let source = "class A { visible=1; protected hidden=2; method() {} get value() { return 3; } callback=()=>4; } const {...rest}=new A(); rest.visible; rest.callback(); rest.hidden; rest.method; rest.value; function copy<T extends A>(x:T) { const {...generic}=x; generic.visible; generic.hidden; generic.method; generic.value; }";
    check(source, &[]);
    let file = tsc_rs_parser::parse("case.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            no_lib: Some(false),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.code,
                d.span.map(|s| s.start).unwrap_or(u32::MAX),
                d.message.as_str(),
            )
        })
        .collect();
    assert_eq!(actual, vec![
        (2339, 158, "Property 'hidden' does not exist on type '{ visible: number; callback: () => number; }'."),
        (2339, 171, "Property 'method' does not exist on type '{ visible: number; callback: () => number; }'."),
        (2339, 184, "Property 'value' does not exist on type '{ visible: number; callback: () => number; }'."),
        (2339, 272, "Property 'hidden' does not exist on type 'Omit<T, \"value\" | \"method\">'."),
        (2339, 288, "Property 'method' does not exist on type 'Omit<T, \"value\" | \"method\">'."),
        (2339, 304, "Property 'value' does not exist on type 'Omit<T, \"value\" | \"method\">'."),
    ]);
}

#[test]
fn protected_interface_inherited() {
    check("class B { protected p=1; } interface I extends B {} interface J extends I {} declare const j:J; j.p; interface Public extends I { p:number; } declare const pub:Public; pub.p;", &[
        (2445, 98, 1, "Property 'p' is protected and only accessible within class 'B' and its subclasses."),
    ]);
}

#[test]
fn protected_interface_multiple() {
    check("class A { protected a=1; protected same=1; } class B { protected b=1; protected same=1; } interface I extends A,B {} declare const i:I; i.a; i.b; i.same; interface Public { same:number; } interface First extends Public,A {} declare const f:First; f.same;", &[
        (2445, 138, 1, "Property 'a' is protected and only accessible within class 'A' and its subclasses."),
        (2445, 143, 1, "Property 'b' is protected and only accessible within class 'B' and its subclasses."),
        (2445, 148, 4, "Property 'same' is protected and only accessible within class 'A' and its subclasses."),
    ]);
}

#[test]
fn protected_interface_receiver() {
    check("class B { protected p=1; f(i:I) { i.p; } } interface I extends B {} class D extends B { g(i:J,b:I) { i.p; b.p; } } interface J extends D {}", &[
        (2446, 108, 1, "Property 'p' is protected and only accessible through an instance of class 'D'. This is an instance of class 'I'."),
    ]);
}

#[test]
fn protected_interface_cycle() {
    check(
        "interface A extends B {} interface B extends A {} declare const a:A; a.absent;",
        &[],
    );
}

#[test]
fn protected_interface_this_parameter() {
    check("class B { protected p=1; } interface I extends B {} function f(this:I,b:B,i:I) { this.p; b.p; i.p; }", &[
        (2446, 91, 1, "Property 'p' is protected and only accessible through an instance of class 'I'. This is an instance of class 'B'."),
    ]);
}
