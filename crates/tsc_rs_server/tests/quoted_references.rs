//! Reference ranges captured independently with TypeScript 6.0.3.
use tsc_rs_ast::Span;
use tsc_rs_query::QueryEngine;
use tsc_rs_server::references_at;

fn check(source: &str, positions: &[u32], expected: &[(u32, u32)]) {
    let mut engine = QueryEngine::new();
    engine.add_source("probe.ts".into(), source.into()).unwrap();
    let _ = engine.check_all();
    let symbols = engine.get_file_symbols("probe.ts").unwrap();
    let expected: Vec<_> = expected
        .iter()
        .map(|(start, end)| ("probe.ts".into(), Span::new(*start, *end)))
        .collect();
    for &position in positions {
        assert_eq!(
            references_at(&engine, symbols, "probe.ts", position, None),
            expected,
            "position {position} in {source}"
        );
    }
}

#[test]
fn quoted_member_reference_ranges() {
    check(
        r###"class C { "foo":number=1; } const c=new C(); c["foo"];"###,
        &[10, 11, 47, 48],
        &[(11, 14), (48, 51)],
    );
    check(
        r###"class C { "foo"(){return 1;} } const c=new C(); c["foo"];"###,
        &[10, 11, 50, 51],
        &[(11, 14), (51, 54)],
    );
    check(
        r###"class C { static "foo":number=1; } C["foo"];"###,
        &[17, 18, 37, 38],
        &[(18, 21), (38, 41)],
    );
    check(
        r###"class C { 'foo'=1; } const c=new C();c.foo;c['foo'];"###,
        &[10, 11, 39, 45, 46],
        &[(11, 14), (39, 42), (46, 49)],
    );
    check(
        r###"class C { get 'foo'(){return 1;} } const c=new C();c.foo;"###,
        &[14, 15, 53],
        &[(15, 18), (53, 56)],
    );
    check(
        r###"class C { set 'foo'(v:number){} } const c=new C();c.foo=1;"###,
        &[14, 15, 52],
        &[(15, 18), (52, 55)],
    );
}

#[test]
fn quoted_member_references_preserve_whole_names() {
    check(
        r###"class C { "x-y":number=1; } const c=new C(); c["x-y"];"###,
        &[10, 11, 47, 48],
        &[(11, 14), (48, 51)],
    );
    check(
        r###"class C { "a b"(){return 1;} } const c=new C(); c["a b"];"###,
        &[10, 11, 50, 51],
        &[(11, 14), (51, 54)],
    );
    check(
        r###"interface C { "a'b":number; } declare const c:C; c["a'b"];"###,
        &[14, 15, 51, 52],
        &[(15, 18), (52, 55)],
    );
    check(
        r###"class C { "é":number=1; } const c=new C(); c["é"];"###,
        &[10, 11, 46, 47],
        &[(11, 13), (47, 49)],
    );
    check(
        r###"class C { "𐐀":number=1; } const c=new C(); c["𐐀"];"###,
        &[10, 11, 48, 49],
        &[(11, 15), (49, 53)],
    );
}

#[test]
fn empty_member_names_have_no_reference_span() {
    check(
        r###"class C { "":number=1; } const c=new C(); c[""];"###,
        &[10, 11, 44, 45],
        &[],
    );
    check(
        r###"class C { ""(){return 1;} } const c=new C(); c[""];"###,
        &[10, 11, 47, 48],
        &[],
    );
    check(
        r###"class C { static "":number=1; } C[""];"###,
        &[17, 18, 34, 35],
        &[],
    );
    check(
        r###"interface C { "":number; } declare const c:C; c[""];"###,
        &[14, 15, 48, 49],
        &[],
    );
}

#[test]
fn independent_class_members_keep_their_declaration_identity() {
    check(
        r###"class C { "foo"=1; } function f(){ class C { "foo"=2; } }"###,
        &[11],
        &[(11, 14)],
    );
    check(
        r###"class C { "foo"=1; } function f(){ class C { "foo"=2; } }"###,
        &[46],
        &[(46, 49)],
    );
    check(
        r###"class A { foo=1; } class B { foo=2; } "###,
        &[10],
        &[(10, 13)],
    );
    check(
        r###"class A { foo=1; } class B { foo=2; } "###,
        &[29],
        &[(29, 32)],
    );

    let source = r#"class C { "foo"=1; } class C { "foo"=1; }"#;
    check(source, &[10, 11, 14], &[(11, 14)]);
    check(source, &[31, 32, 35], &[(32, 35)]);
    check(
        r#"class B { "foo"=0; } class C extends B { "foo"=1; } class C extends B { "foo"=2; }"#,
        &[10, 11, 14, 41, 42, 45, 72, 73, 76],
        &[(11, 14), (42, 45), (73, 76)],
    );
    check(
        r#"class B { "foo"=0; } class C implements B { "foo"=1; } class C implements B { "foo"=2; }"#,
        &[10, 11, 14, 44, 45, 48, 78, 79, 82],
        &[(11, 14), (45, 48), (79, 82)],
    );
}

#[test]
fn project_references_do_not_mix_file_local_symbol_ids() {
    use tsc_rs_server::harness_api::ProjectState;
    let source = r#"export class C { "foo"=1; } const c=new C(); c["foo"];"#;
    for (other, extra) in [
        (
            "export class D { unrelated=1; } const d=new D(); d.unrelated;",
            None,
        ),
        (
            r#"import {C} from './a'; const c=new C(); c["foo"];"#,
            Some((43, 46)),
        ),
    ] {
        let mut engine = QueryEngine::new();
        engine.add_source("/a.ts".into(), source.into()).unwrap();
        engine.add_source("/b.ts".into(), other.into()).unwrap();
        let _ = engine.check_all();
        let project = ProjectState::from_file_map(
            [
                (
                    "/a.ts".into(),
                    (
                        source.into(),
                        engine.get_file_symbols("/a.ts").unwrap().clone(),
                    ),
                ),
                (
                    "/b.ts".into(),
                    (
                        other.into(),
                        engine.get_file_symbols("/b.ts").unwrap().clone(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let mut expected = vec![
            ("/a.ts".into(), Span::new(18, 21)),
            ("/a.ts".into(), Span::new(48, 51)),
        ];
        if let Some((start, end)) = extra {
            expected.push(("/b.ts".into(), Span::new(start, end)));
        }
        for position in [17, 18, 47, 48] {
            assert_eq!(
                references_at(
                    &engine,
                    engine.get_file_symbols("/a.ts").unwrap(),
                    "/a.ts",
                    position,
                    Some(&project)
                ),
                expected
            );
        }
    }
}

#[test]
fn local_class_receivers_keep_reference_identity() {
    check(
        r###"class A { "foo"=1; } class B { "foo"=2; } const a:A=new A(); const b:B=new B(); a["foo"];b["foo"];"###,
        &[10, 11, 14, 82, 83, 86],
        &[(11, 14), (83, 86)],
    );
    check(
        r###"class A { "foo"=1; } class B { "foo"=2; } const a:A=new A(); const b:B=new B(); a["foo"];b["foo"];"###,
        &[31, 32, 35, 91, 92, 95],
        &[(32, 35), (92, 95)],
    );
    check(
        r###"class A { "x-y"=1; } class B { "x-y"=2; } const a:A=new A(); const b:B=new B(); a["x-y"];b["x-y"];"###,
        &[10, 11, 14, 82, 83, 86],
        &[(11, 14), (83, 86)],
    );
    check(
        r###"class A { "x-y"=1; } class B { "x-y"=2; } const a:A=new A(); const b:B=new B(); a["x-y"];b["x-y"];"###,
        &[31, 32, 35, 91, 92, 95],
        &[(32, 35), (92, 95)],
    );
}
