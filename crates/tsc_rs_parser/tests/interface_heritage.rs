use tsc_rs_ast::{PropName, StmtKind, TypeMemberKind};

#[test]
fn rejected_implements_clause_does_not_become_interface_members() {
    let source = "interface Base<T> {} interface Derived<T> extends Base<T> implements Base<T>, Missing { value: T; implements: number; }";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert_eq!(file.diagnostics.len(), 1, "{:?}", file.diagnostics);
    let diagnostic = &file.diagnostics[0];
    assert_eq!(diagnostic.code, 1176);
    assert_eq!(
        diagnostic.message,
        "Interface declaration cannot have 'implements' clause."
    );
    let span = diagnostic.span.unwrap();
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        "implements"
    );
    let StmtKind::InterfaceDecl(interface) = &file.statements[1].kind else {
        panic!("missing interface");
    };
    assert_eq!(interface.extends.len(), 1);
    assert_eq!(interface.members.len(), 2);
    for (member, name) in interface.members.iter().zip(["value", "implements"]) {
        let TypeMemberKind::PropertySig(property) = &member.kind else {
            panic!("missing property");
        };
        assert!(matches!(&property.name, PropName::Ident(actual, _) if actual == name));
    }
}

#[test]
fn implements_is_still_a_valid_interface_property_name() {
    let file = tsc_rs_parser::parse("case.ts", "interface I { implements: number; }");
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let StmtKind::InterfaceDecl(interface) = &file.statements[0].kind else {
        panic!("missing interface");
    };
    assert_eq!(interface.members.len(), 1);
}
