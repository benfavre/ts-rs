// Expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(
    source: &str,
    strict_null_checks: bool,
    no_implicit_any: bool,
    expected_type: &str,
    expected: &[(u32, &str, &str)],
    arrays: &[(u32, u32, &str)],
) {
    let file = tsc_rs_parser::parse("subject.js", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let output = checker.check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2020),
            allow_js: Some(true),
            check_js: Some(true),
            strict: Some(true),
            strict_null_checks: Some(strict_null_checks),
            no_implicit_any: Some(no_implicit_any),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        output.diagnostics.len(),
        expected.len(),
        "{source}: {:?}",
        output.diagnostics
    );
    for (actual, (code, message, text)) in output.diagnostics.iter().zip(expected) {
        assert_eq!(actual.code, *code);
        assert_eq!(actual.message, *message);
        let span = actual.span.unwrap();
        assert_eq!(&source[span.start as usize..span.end as usize], *text);
    }
    for (position, end, expected_type) in arrays {
        assert_eq!(
            output
                .expression_type_spans
                .get(&(*position, *end))
                .map(|entry| entry.type_string.as_str()),
            Some(*expected_type),
            "array at {position}: {source}"
        );
    }
    let position = source.rfind("value;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&position).map(String::as_str),
        Some(expected_type),
        "{source}"
    );
}

#[test]
fn constructor_numeric_array() {
    verify("class Box{constructor(){\n/** @type {number[]} */\nthis.items=[];}}const box=new Box();box.items.push(1);const value=box.items;value;", true, true, "number[]", &[], &[(60, 62, "never[]")]);
}

#[test]
fn constructor_wrong_push() {
    verify("class Box{constructor(){\n/** @type {number[]} */\nthis.items=[];}}const box=new Box();box.items.push(\"x\");const value=box.items;value;", true, true, "number[]", &[(2345, "Argument of type 'string' is not assignable to parameter of type 'number'.", "\"x\"")], &[(60, 62, "never[]")]);
}

#[test]
fn constructor_wrong_initializer() {
    verify("class Box{constructor(){\n/** @type {number[]} */\nthis.items=[\"x\"];}}const value=new Box().items;value;", true, true, "number[]", &[(2322, "Type 'string' is not assignable to type 'number'.", "\"x\"")], &[(60, 65, "string[]")]);
}

#[test]
fn constructor_bracket_property() {
    verify("class Box{constructor(){\n/** @type {number[]} */\nthis[\"items\"]=[];}}const box=new Box();box.items.push(\"x\");const value=box.items;value;", true, true, "number[]", &[(2345, "Argument of type 'string' is not assignable to parameter of type 'number'.", "\"x\"")], &[(63, 65, "never[]")]);
}

#[test]
fn constructor_nested_block() {
    verify("class Box{constructor(){{\n/** @type {number[]} */\nthis.items=[];}}}const box=new Box();box.items.push(\"x\");const value=box.items;value;", true, true, "number[]", &[(2345, "Argument of type 'string' is not assignable to parameter of type 'number'.", "\"x\"")], &[(61, 63, "never[]")]);
}

#[test]
fn constructor_annotated_conditional() {
    verify("class Box{constructor(){if(Math.random()){\n/** @type {number[]} */\nthis.items=[];}}}const box=new Box();box.items.push(\"x\");const value=box.items;value;", true, true, "number[]", &[(2345, "Argument of type 'string' is not assignable to parameter of type 'number'.", "\"x\"")], &[(78, 80, "never[]")]);
}

#[test]
fn constructor_annotated_try() {
    verify("class Box{constructor(){try{\n/** @type {number[]} */\nthis.items=[];}finally{}}}const value=new Box().items;value;", true, true, "number[]", &[], &[(64, 66, "never[]")]);
}

#[test]
fn inline_trailing_comment_does_not_annotate_assignment() {
    verify("class Box{constructor(){/** @type {number[]} */ this.items=[\"x\"];}}const value=new Box().items;value;", true, true, "string[]", &[], &[(59, 64, "string[]")]);
}
