use tsc_rs_symbols::{bind, generate_symbols_baseline, render_annotated_source};

#[test]
fn declarations_keep_full_trivia_starts_and_precise_navigation_spans() {
    let source = "// lead\nexport class Box {\n /** member */\n readonly value: number;\n get item(): number { return 1; }\n}\n// next\nexport function f( /* p */ ...args: number[]) {}";
    let file = tsc_rs_parser::parse("positions.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = bind(&file);
    // Positions independently checked with TypeScript 6.0.3's Node.pos.
    for (name, full_start, name_start) in [
        ("Box", 0, 21),
        ("value", 26, 52),
        ("item", 66, 72),
        ("f", 102, 127),
        ("args", 129, 141),
    ] {
        let symbol = symbols
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap();
        let declaration = &symbol.declarations[0];
        assert_eq!(declaration.full_start, full_start, "{name}");
        assert_eq!(declaration.span.start, name_start, "{name}");
        assert_eq!(
            &source[declaration.span.start as usize..declaration.span.end as usize],
            name
        );
    }
}

#[test]
fn annotation_groups_preserve_source_and_separate_nonbrace_lines() {
    let source = "class C {\n  value: number;\n}\n\nconst x = 1;\n";
    let actual = render_annotated_source(
        source,
        [
            (0, ">C : C\n".into()),
            (1, ">value : number\n".into()),
            (4, ">x : 1\n".into()),
        ],
    );
    assert_eq!(
        actual,
        "class C {\n>C : C\n\n  value: number;\n>value : number\n}\n\nconst x = 1;\n>x : 1\n\n"
    );
    assert_eq!(
        render_annotated_source("// no names\n", []),
        "\n// no names\n\n"
    );
    assert_eq!(
        render_annotated_source("x\ny", [(0, ">x\n".into())]),
        "x\n>x\n\ny\n"
    );
}

#[test]
fn symbol_locations_count_utf16_and_all_ecmascript_line_breaks() {
    let source = "/*😀*/ class A {}\r\nclass B {}\u{2028}class C {}\rclass D {}\u{2029}class E {}";
    let file = tsc_rs_parser::parse("unicode.ts", source);
    let baseline = generate_symbols_baseline(&file, &bind(&file));
    for expected in [
        "Decl(unicode.ts, 0, 0)",
        "Decl(unicode.ts, 0, 17)",
        "Decl(unicode.ts, 1, 10)",
        "Decl(unicode.ts, 2, 10)",
        "Decl(unicode.ts, 3, 10)",
    ] {
        assert!(baseline.contains(expected), "{expected}: {baseline}");
    }
    assert!(!baseline.contains('\r'), "{baseline}");
}
