//! A generic type annotation closed by a `>` fused with the next token
//! (`Record<K,V>={}` scans `>=`, `Map<K,Array<V>>=` scans `>>=`): the parser
//! splits the `>` off, and the annotation's span must still include it.
//! Fast-emit erases annotations by span, so a span that stopped one byte short
//! left `const headers>={}` in the output (V8: "Missing initializer in const
//! declaration"), which took down a whole PRISM API bundle.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

fn fast(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    let opts = CompilerOptions {
        fast_emit: Some(true),
        ..Default::default()
    };
    emit(&file, &opts).javascript
}

fn structured(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.ts", source);
    emit(&file, &CompilerOptions::default()).javascript
}

#[test]
fn fast_emit_erases_the_split_closing_angle() {
    for (source, expected) in [
        (
            "const headers:Record<string,string>={};",
            "const headers={};",
        ),
        (
            "let m:Map<string,Array<number>>=new Map();",
            "let m=new Map();",
        ),
        ("let n:Array<Array<Array<number>>>=[];", "let n=[];"),
        (
            "function f(x:Array<number>=[1], y:Set<string>=new Set()){return x}",
            "function f(x=[1], y=new Set()){return x}",
        ),
        (
            "const g=(a:Promise<void>=Promise.resolve())=>a;",
            "const g=(a=Promise.resolve())=>a;",
        ),
        (
            "for(let k:Array<number>=[];;){break}",
            "for(let k=[];;){break}",
        ),
    ] {
        let out = fast(source);
        assert!(out.contains(expected), "{source}\n=> {out}");
        assert!(!out.contains(">="), "{source}\n=> {out}");
    }
}

#[test]
fn fast_and_structured_agree_on_split_angles() {
    // Structured emit never depended on the span; it is the reference.
    for source in [
        "const headers:Record<string,string>={};",
        "let m:Map<string,Array<number>>=new Map();",
        "function f(x:Array<number>=[1]){return x}",
    ] {
        // Fast-emit copies statements verbatim, so only whitespace and a
        // block's last `;` may differ.
        let squash = |s: String| s.split_whitespace().collect::<String>().replace(";}", "}");
        assert_eq!(squash(fast(source)), squash(structured(source)), "{source}");
    }
}

#[test]
fn comparisons_and_shifts_are_untouched() {
    // `>=` / `>>=` that are real operators must survive.
    let out = fast("let a=1;let b:number=2;a>=b;a>>=1;const ok:Record<string,string> = {};");
    assert!(out.contains("a>=b"), "{out}");
    assert!(out.contains("a>>=1"), "{out}");
    assert!(out.contains("const ok = {}"), "{out}");
}

#[test]
fn annotation_span_includes_the_split_angle() {
    let source = "const headers:Record<string,string>={};";
    let file = tsc_rs_parser::parse("test.ts", source);
    let StmtKind::Var(vs) = &file.statements[0].kind else {
        panic!("expected a variable statement");
    };
    let ty = vs.declarations[0].type_ann.as_ref().expect("annotation");
    assert_eq!(
        &source[ty.span.start as usize..ty.span.end as usize],
        "Record<string,string>"
    );
}

#[test]
fn return_type_closed_right_before_the_arrow_parses() {
    // `>=>` used to scan as `>=` + `>` and fail with TS1109.
    for (source, expected) in [
        (
            "const h=():Promise<void>=>Promise.resolve();",
            "const h=()=>Promise.resolve();",
        ),
        (
            "const k=(a:number):Map<string,Array<number>>=>new Map();",
            "const k=(a)=>new Map();",
        ),
        ("let x=1;x>>=1;const ok=x>=1;", "x>>=1"),
    ] {
        let file = tsc_rs_parser::parse("test.ts", source);
        assert!(
            file.diagnostics.is_empty(),
            "{source}: {:?}",
            file.diagnostics
        );
        let out = fast(source);
        assert!(out.contains(expected), "{source}\n=> {out}");
        let squash = |s: String| s.split_whitespace().collect::<String>().replace(";}", "}");
        assert_eq!(squash(fast(source)), squash(structured(source)), "{source}");
    }
}
