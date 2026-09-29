use tsc_rs_ast::Diagnostic;
use tsc_rs_types::TypeChecker;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("readonly_member_diagnostic_spans.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new().check(&file, &symbols).diagnostics
}

#[test]
fn normal_readonly_member_diagnostics_point_to_property_tokens() {
    let source = r#"
interface X {
    readonly a: number;
    readonly b: number;
}
let x: X = { a: 0, b: 0 };
x.a = 1;
x.b = 1;

class C {
    readonly a = 0;
    readonly b = 0;
    get c() { return 0; }

    method() {
        this.a = 1;
        this.b = 1;
        this.c = 1;
    }
}

enum E { A }
E.A = 1;

const mutable = { a: 0 };
mutable.a = 1;
"#;

    let all_diagnostics = diagnostics(source);
    let readonly: Vec<_> = all_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2540)
        .collect();

    assert_eq!(readonly.len(), 6, "diagnostics: {all_diagnostics:?}");
    assert_eq!(
        readonly
            .iter()
            .map(|diagnostic| {
                let span = diagnostic
                    .span
                    .expect("TS2540 should point at the readonly property");
                &source[span.start as usize..span.end as usize]
            })
            .collect::<Vec<_>>(),
        ["a", "b", "a", "b", "c", "A"],
        "diagnostics: {all_diagnostics:?}"
    );
}
