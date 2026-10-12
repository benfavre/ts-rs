//! Syntax diagnostics and recovery regressions from upstream baselines and
//! `scripts/parse-parity.py`. Bext serves a module only when this parser reports
//! no syntax error. Broken code must be rejected, while valid counterparts
//! must stay error-free.

fn syntax_codes(file: &str, src: &str) -> Vec<u32> {
    let sf = tsc_rs_parser::parse(file, src);
    sf.diagnostics
        .iter()
        .filter(|d| tsc_rs_parser::has_syntax_errors(std::slice::from_ref(*d)) || d.code == 1128)
        .map(|d| d.code)
        .collect()
}

fn rejected(src: &str, code: u32) {
    let codes = syntax_codes("/t.ts", src);
    assert!(
        codes.contains(&code),
        "{src:?}: expected TS{code}, got {codes:?}"
    );
}

fn accepted(file: &str, src: &str) {
    let codes = syntax_codes(file, src);
    assert!(
        codes.is_empty(),
        "{src:?}: expected no syntax error, got {codes:?}"
    );
}

#[test]
fn unterminated_block_comment() {
    rejected("const a = 1;\n/* never closed\nconst b = 2;\n", 1010);
    accepted("/t.ts", "const a = 1; /* closed */\n");
    // `/*` in JSX text is text.
    accepted("/t.tsx", "const x = <div>/* text</div>;\n");
}

#[test]
fn missing_comma_in_arguments_and_arrays() {
    rejected("f(0 1);\n", 1005);
    rejected("fail(404 \"msg\");\n", 1005);
    rejected("const a = [\"x\" \"y\"];\n", 1005);
    accepted("/t.ts", "f(0, 1);\nconst a = [\"x\", \"y\"];\n");
}

#[test]
fn try_without_catch_or_finally() {
    rejected("try {\n  f();\n}\nconst x = 1;\n", 1472);
    accepted("/t.ts", "try { f(); } catch {}\ntry { f(); } finally {}\n");
}

#[test]
fn cut_off_imports() {
    rejected("import ;\n", 1128);
    rejected("import Commerc\n", 1005);
    rejected("import { a }\n", 1005);
    rejected("import { a } from \"x\" y\n", 1005);
    accepted(
        "/t.ts",
        "import A from \"x\";\nimport \"y\";\nimport type { T } from \"z\";\n",
    );
}

#[test]
fn cut_off_exports() {
    rejected("export { a } fr\n", 1005);
    rejected("export * from \"x\" fr\n", 1005);
    rejected("export cons\n", 1128);
    rejected("export async funct\n", 1128);
    rejected("export as\n", 1128);
    accepted(
        "/t.ts",
        "export { a } from \"x\";\nexport * from \"y\";\nexport async function f() {}\nexport type T = 1;\nexport declare const c: 1;\n",
    );
}

#[test]
fn cut_off_type_arguments() {
    rejected("function f(): Promise<A", 1005);
    accepted("/t.ts", "let x: Array<number> = [];\nconst a = f < A;\n");
}

#[test]
fn same_line_junk_after_return_and_variable_statements() {
    // `return X attr="…"/>` (a JSX element missing its `<`) and
    // `…?.[1]return …` both emitted JS that does not parse.
    rejected("function f() { return SitePage path=\"/x\"/>; }\n", 1005);
    rejected("function f(r) { const id = g(r)?.[1]return id; }\n", 1005);
    accepted(
        "/t.ts",
        "function f() { const a = 1; return a }\nfunction g() {\n  return 1\n}\n",
    );
}

#[test]
fn missing_type_member_separator() {
    rejected(
        "let b: { name?: string; town?: string service?: string };\n",
        1005,
    );
    rejected("interface I { a: number b: string }\n", 1005);
    accepted(
        "/t.ts",
        "var x: { [index: string]: string; one: string; };\ninterface J {\n  a: number\n  b: string\n}\nlet c: { a: 1, b: 2 };\n",
    );
}

#[test]
fn statement_and_type_alias_separators() {
    for source in [
        "while (true) { break label debugger; }",
        "while (true) { continue label debugger; }",
        "debugger debugger;",
        "throw value debugger;",
        "type T = string type U = number;",
    ] {
        rejected(source, 1005);
    }
    for source in [
        "label: while (true) { break label; }",
        "label: while (true) { continue label\n}",
        "while (true) { break\nlabel; }",
        "while (true) { continue\nlabel; }",
        "debugger\ndebugger;",
        "throw value\ndebugger;",
        "type T = string\ntype U = number;",
        "do {} while (false) debugger;",
    ] {
        accepted("/t.ts", source);
    }
}

#[test]
fn missing_throw_operand_preserves_the_closing_brace() {
    let source = "function f() { throw } const after = 1;";
    let file = tsc_rs_parser::parse("/t.ts", source);
    assert_eq!(file.diagnostics.len(), 1, "{:?}", file.diagnostics);
    assert_eq!(file.diagnostics[0].code, 1109);
    let span = file.diagnostics[0].span.unwrap();
    assert_eq!(&source[span.start as usize..span.end as usize], "}");
    assert_eq!(file.statements.len(), 2);
    assert!(matches!(
        file.statements[1].kind,
        tsc_rs_ast::StmtKind::Var(_)
    ));
}

#[test]
fn declared_type_alias_cannot_break_before_its_name() {
    for source in [
        "declare type\nAlias = null;",
        "declare type /*\ncomment */ Alias = null;",
    ] {
        let file = tsc_rs_parser::parse("/t.ts", source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        assert_eq!(file.diagnostics[0].code, 1142);
        assert!(tsc_rs_parser::has_syntax_errors(&file.diagnostics));
        let span = file.diagnostics[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "Alias");
        assert!(matches!(
            file.statements[0].kind,
            tsc_rs_ast::StmtKind::TypeAlias(_)
        ));
    }
    for source in [
        "declare type /* comment */ Alias = null;",
        "declare\ntype Alias = null;",
        "declare\ntype\nAlias = null;",
        "type\nAlias = null;",
    ] {
        accepted("/t.ts", source);
    }
}

#[test]
fn type_alias_line_break_takes_precedence_over_a_missing_name() {
    for (source, code) in [
        ("declare type\n= number;", 1142),
        ("declare type = number;", 1003),
    ] {
        let file = tsc_rs_parser::parse("/t.ts", source);
        assert_eq!(
            file.diagnostics.len(),
            1,
            "{source}: {:?}",
            file.diagnostics
        );
        assert_eq!(file.diagnostics[0].code, code);
        let span = file.diagnostics[0].span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], "=");
    }
}

#[test]
fn export_cannot_prefix_an_ordinary_statement() {
    for source in [
        "export throw null;",
        "export return;",
        "export break;",
        "export continue;",
        "export debugger;",
        "export if (true) {}",
        "export while (true) {}",
        "export do {} while (false);",
        "export for (;;) {}",
        "export switch (0) {}",
        "export try {} finally {}",
        "export with ({}) {}",
        "export foo",
        "export ;",
    ] {
        let file = tsc_rs_parser::parse("/t.ts", source);
        let errors: Vec<_> = file.diagnostics.iter().filter(|d| d.code == 1128).collect();
        assert_eq!(errors.len(), 1, "{source}: {:?}", file.diagnostics);
        assert_eq!(
            errors[0].span,
            Some(tsc_rs_ast::Span::new(0, 6)),
            "{source}"
        );
        assert_eq!(syntax_codes("/t.ts", source), vec![1128], "{source}");
    }
}
