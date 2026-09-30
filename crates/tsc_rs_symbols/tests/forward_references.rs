use tsc_rs_symbols::{bind, ReferenceKind};

#[test]
fn forward_references_resolve_to_later_value_and_type_declarations() {
    let source = "run();\nlet item: Later;\nfunction run() { return item; }\ninterface Later {}";
    let file = tsc_rs_parser::parse("forward.ts", source);
    let table = bind(&file);
    for (usage, declaration, kind) in [
        (0, source.find("run() {").unwrap(), ReferenceKind::Value),
        (
            source.find("Later;").unwrap(),
            source.find("Later {}").unwrap(),
            ReferenceKind::Type,
        ),
    ] {
        let reference = table.scope_graph.reference_at(usage as u32).unwrap();
        assert_eq!(reference.kind, kind);
        assert_eq!(
            reference.symbol_id,
            table.position_to_symbol[&(declaration as u32)]
        );
        assert_eq!(
            table.position_to_symbol[&(usage as u32)],
            reference.symbol_id
        );
    }
}

#[test]
fn later_local_declarations_shadow_outer_names_for_earlier_references() {
    let source =
        "const value = 1;\nfunction f() { value; let value = 2; return () => value; }\nvalue;";
    let file = tsc_rs_parser::parse("shadow.ts", source);
    let table = bind(&file);
    let outer = table.position_to_symbol[&6];
    let inner = table.position_to_symbol[&(source.find("value = 2").unwrap() as u32)];
    assert_ne!(outer, inner);
    for offset in [
        source.find("value;").unwrap(),
        source.find("value; }").unwrap(),
    ] {
        assert_eq!(
            table
                .scope_graph
                .reference_at(offset as u32)
                .unwrap()
                .symbol_id,
            inner
        );
    }
    assert_eq!(
        table
            .scope_graph
            .reference_at(source.rfind("value;").unwrap() as u32)
            .unwrap()
            .symbol_id,
        outer
    );
}

#[test]
fn forward_bindings_do_not_leak_from_sibling_or_nested_scopes() {
    let source = "hidden; { let hidden = 1; } { hidden; }\nfunction f() { return secret; }\nfunction g() { let secret = 1; }";
    let file = tsc_rs_parser::parse("isolated.ts", source);
    let table = bind(&file);
    for offset in [
        0,
        source.find("hidden; }").unwrap(),
        source.find("secret;").unwrap(),
    ] {
        assert!(table.scope_graph.reference_at(offset as u32).is_none());
        assert!(!table.position_to_symbol.contains_key(&(offset as u32)));
    }
}

#[test]
fn class_members_do_not_shadow_lexical_names() {
    let source = "const value = 1; class C { before() { return value; } value = 2; after() { return value; } }";
    let file = tsc_rs_parser::parse("members.ts", source);
    let table = bind(&file);
    let outer = table.position_to_symbol[&6];
    let member = table.position_to_symbol[&(source.find("value = 2").unwrap() as u32)];
    assert_ne!(outer, member);
    for (offset, _) in source.match_indices("value;") {
        assert_eq!(
            table
                .scope_graph
                .reference_at(offset as u32)
                .unwrap()
                .symbol_id,
            outer
        );
    }
}
