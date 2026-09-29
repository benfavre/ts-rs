use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::{TypeCheckOutput, TypeChecker};

fn check(source: &str) -> TypeCheckOutput {
    let file = tsc_rs_parser::parse("longObjectInstantiationChain1.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(true),
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn fifty_link_object_instantiation_chain_keeps_lazy_alias_identity() {
    let source = include_str!("../../../tests/cases/compiler/longObjectInstantiationChain1.ts");
    let output = check(source);

    assert_eq!(
        output
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        vec![2339; 4]
    );
    let missing_properties = output
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS2339 must have a property span");
            &source[span.start as usize..span.end as usize]
        })
        .collect::<Vec<_>>();
    assert_eq!(missing_properties, ["p51", "p4", "p38", "p51"]);
    assert!(output.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .message
            .contains("does not exist on type 'merge<")
            && !diagnostic.message.contains('\n')
    }));

    let o50_position = source.find("o50 =").expect("o50 declaration") as u32;
    let o50_display = output
        .expression_types
        .get(&o50_position)
        .expect("default query metadata must include o50");
    assert!(o50_display.starts_with("merge<merge<"));
    assert!(o50_display.contains("p51: number"));
    assert!(
        o50_display.len() < 4_096,
        "lazy display unexpectedly expanded: {} bytes",
        o50_display.len()
    );
    assert!(
        !output.stable_types.is_empty(),
        "default query mode must retain stable type metadata"
    );
}

#[test]
fn merge_chain_uses_right_hand_override_and_preserves_other_members() {
    let source = r#"
type Merge<Base, Props> = Omit<Base, keyof Props & keyof Base> & Props;
declare const merge: <L, R>(left: L, right: R) => Merge<L, R>;
const o1 = merge({ value: "old", keep: true }, { value: 1 });
const o2 = merge(o1, { other: "yes" });
const o3 = merge(o2, { value: 2 });
const value: number = o3.value;
const keep: boolean = o3.keep;
const other: string = o3.other;
const wrong: string = o3.value;
"#;
    let output = check(source);
    assert_eq!(output.diagnostics.len(), 1, "{:?}", output.diagnostics);
    assert_eq!(output.diagnostics[0].code, 2322);
}
