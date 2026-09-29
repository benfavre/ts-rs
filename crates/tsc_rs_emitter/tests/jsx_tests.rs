//! JSX parsing and emit tests.
//!
//! Verifies that JSX syntax is correctly parsed and transformed
//! to React.createElement calls.

use tsc_rs_ast::*;
use tsc_rs_emitter::emit;

/// Helper: parse TSX source and emit JavaScript with JSX=react options.
fn emit_jsx(source: &str) -> String {
    let file = tsc_rs_parser::parse("test.tsx", source);
    let opts = CompilerOptions {
        jsx: Some(JsxEmit::React),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

/// Helper: parse TSX source and emit with custom factory.
fn emit_jsx_with_factory(source: &str, factory: &str, fragment_factory: &str) -> String {
    let file = tsc_rs_parser::parse("test.tsx", source);
    let opts = CompilerOptions {
        jsx: Some(JsxEmit::React),
        jsx_factory: Some(factory.to_string()),
        jsx_fragment_factory: Some(fragment_factory.to_string()),
        ..Default::default()
    };
    let out = emit(&file, &opts);
    out.javascript
}

fn emit_jsx_with_options(source: &str, jsx: JsxEmit, target: ScriptTarget) -> String {
    let file = tsc_rs_parser::parse("test.tsx", source);
    let opts = CompilerOptions {
        jsx: Some(jsx),
        target: Some(target),
        module: Some(ModuleKind::CommonJS),
        ..Default::default()
    };
    emit(&file, &opts).javascript
}

// ---------------------------------------------------------------
// 1. Simple self-closing element
// ---------------------------------------------------------------

#[test]
fn test_jsx_self_closing_div() {
    let js = emit_jsx("<div />;");
    assert!(
        js.contains("React.createElement(\"div\", null)"),
        "self-closing <div /> should emit React.createElement(\"div\", null): got: {js}"
    );
}

// ---------------------------------------------------------------
// 2. Element with string attribute
// ---------------------------------------------------------------

#[test]
fn test_jsx_element_with_string_attr() {
    let js = emit_jsx("<div className=\"x\" />;");
    assert!(
        js.contains("React.createElement(\"div\", { className: \"x\" })"),
        "should emit attributes as object: got: {js}"
    );
}

// ---------------------------------------------------------------
// 3. Element with children text
// ---------------------------------------------------------------

#[test]
fn test_jsx_element_with_text_child() {
    let js = emit_jsx("<div>hello</div>;");
    assert!(
        js.contains("React.createElement(\"div\", null, \"hello\")"),
        "should emit text child as string arg: got: {js}"
    );
}

// ---------------------------------------------------------------
// 4. Nested elements
// ---------------------------------------------------------------

#[test]
fn test_jsx_nested_elements() {
    let js = emit_jsx("<div><span>text</span></div>;");
    assert!(
        js.contains("React.createElement(\"div\", null,")
            && js.contains("React.createElement(\"span\", null, \"text\")"),
        "nested elements should be nested createElement calls: got: {js}"
    );
}

// ---------------------------------------------------------------
// 5. Fragment
// ---------------------------------------------------------------

#[test]
fn test_jsx_fragment() {
    let js = emit_jsx("<>hello</>;");
    assert!(
        js.contains("React.createElement(React.Fragment, null, \"hello\")"),
        "fragment should use React.Fragment: got: {js}"
    );
}

// ---------------------------------------------------------------
// 6. Expression children
// ---------------------------------------------------------------

#[test]
fn test_jsx_expression_child() {
    let js = emit_jsx("<div>{x + 1}</div>;");
    assert!(
        js.contains("React.createElement(\"div\", null, x + 1)"),
        "expression child should be emitted inline: got: {js}"
    );
}

// ---------------------------------------------------------------
// 7. Spread attributes
// ---------------------------------------------------------------

#[test]
fn test_jsx_spread_attributes() {
    let js = emit_jsx("<div {...props} />;");
    assert!(
        js.contains("React.createElement(\"div\", { ...props })"),
        "ESNext spread attributes should use native object spread: got: {js}"
    );
    assert!(
        js.contains("props"),
        "spread should contain props: got: {js}"
    );
}

// ---------------------------------------------------------------
// 8. Component (uppercase tag)
// ---------------------------------------------------------------

#[test]
fn test_jsx_component_tag() {
    let js = emit_jsx("<MyComponent />;");
    assert!(
        js.contains("React.createElement(MyComponent, null)"),
        "uppercase tag should be a reference, not string: got: {js}"
    );
}

// ---------------------------------------------------------------
// 9. Member expression tag
// ---------------------------------------------------------------

#[test]
fn test_jsx_member_tag() {
    let js = emit_jsx("<Foo.Bar />;");
    assert!(
        js.contains("React.createElement(Foo.Bar, null)"),
        "member expression tag should be emitted as member access: got: {js}"
    );
}

// ---------------------------------------------------------------
// 10. Multiple attributes
// ---------------------------------------------------------------

#[test]
fn test_jsx_multiple_attributes() {
    let js = emit_jsx("<div id=\"main\" className=\"container\" />;");
    assert!(
        js.contains("id: \"main\""),
        "should contain id attribute: got: {js}"
    );
    assert!(
        js.contains("className: \"container\""),
        "should contain className attribute: got: {js}"
    );
}

// ---------------------------------------------------------------
// 11. Attribute with expression value
// ---------------------------------------------------------------

#[test]
fn test_jsx_attr_expression_value() {
    let js = emit_jsx("<div onClick={handler} />;");
    assert!(
        js.contains("onClick: handler"),
        "expression attribute value should be emitted: got: {js}"
    );
}

// ---------------------------------------------------------------
// 12. Boolean attribute (no value)
// ---------------------------------------------------------------

#[test]
fn test_jsx_boolean_attribute() {
    let js = emit_jsx("<input disabled />;");
    assert!(
        js.contains("disabled: true"),
        "boolean attribute should emit true: got: {js}"
    );
}

// ---------------------------------------------------------------
// 13. Empty fragment
// ---------------------------------------------------------------

#[test]
fn test_jsx_empty_fragment() {
    let js = emit_jsx("<></>;");
    assert!(
        js.contains("React.createElement(React.Fragment, null)"),
        "empty fragment should emit Fragment with null: got: {js}"
    );
}

// ---------------------------------------------------------------
// 14. Multiple children
// ---------------------------------------------------------------

#[test]
fn test_jsx_multiple_children() {
    let js = emit_jsx("<div><span>a</span><span>b</span></div>;");
    assert!(
        js.contains("React.createElement(\"span\", null, \"a\")"),
        "first child should be present: got: {js}"
    );
    assert!(
        js.contains("React.createElement(\"span\", null, \"b\")"),
        "second child should be present: got: {js}"
    );
}

// ---------------------------------------------------------------
// 15. Custom jsx factory
// ---------------------------------------------------------------

#[test]
fn test_jsx_custom_factory() {
    let js = emit_jsx_with_factory("<div />;", "h", "Fragment");
    assert!(
        js.contains("h(\"div\", null)"),
        "should use custom factory 'h': got: {js}"
    );
}

// ---------------------------------------------------------------
// 16. Custom fragment factory
// ---------------------------------------------------------------

#[test]
fn test_jsx_custom_fragment_factory() {
    let js = emit_jsx_with_factory("<>hello</>;", "h", "Fragment");
    assert!(
        js.contains("h(Fragment, null, \"hello\")"),
        "should use custom fragment factory: got: {js}"
    );
}

// ---------------------------------------------------------------
// 17. Self-closing vs opening/closing
// ---------------------------------------------------------------

#[test]
fn test_jsx_self_closing_vs_open_close() {
    let self_closing = emit_jsx("<br />;");
    let open_close = emit_jsx("<br></br>;");
    assert!(
        self_closing.contains("React.createElement(\"br\", null)"),
        "self-closing should work: got: {self_closing}"
    );
    assert!(
        open_close.contains("React.createElement(\"br\", null)"),
        "open/close without children should work: got: {open_close}"
    );
}

// ---------------------------------------------------------------
// 18. Mixed attribute types with spread
// ---------------------------------------------------------------

#[test]
fn test_jsx_mixed_attrs_with_spread() {
    let js = emit_jsx("<div className=\"x\" {...props} id=\"y\" />;");
    assert!(
        js.contains("{ className: \"x\", ...props, id: \"y\" }"),
        "ESNext mixed attrs should preserve native object spread order: got: {js}"
    );
    assert!(
        js.contains("className: \"x\""),
        "should contain pre-spread attr: got: {js}"
    );
    assert!(
        js.contains("id: \"y\""),
        "should contain post-spread attr: got: {js}"
    );
}

// ---------------------------------------------------------------
// 19. Intrinsic element name is a string
// ---------------------------------------------------------------

#[test]
fn test_jsx_intrinsic_is_string() {
    let js = emit_jsx("<span />;");
    assert!(
        js.contains("\"span\""),
        "intrinsic element name should be a string: got: {js}"
    );
}

// ---------------------------------------------------------------
// 20. Component name is NOT a string
// ---------------------------------------------------------------

#[test]
fn test_jsx_component_is_not_string() {
    let js = emit_jsx("<App />;");
    // Should have App without quotes
    assert!(
        js.contains("React.createElement(App, null)"),
        "component name should not be quoted: got: {js}"
    );
    assert!(
        !js.contains("\"App\""),
        "component name should NOT be a string: got: {js}"
    );
}

// ---------------------------------------------------------------
// 21. JSX with expression and text children
// ---------------------------------------------------------------

#[test]
fn test_jsx_mixed_children() {
    let js = emit_jsx("<div>hello {name}</div>;");
    assert!(
        js.contains("\"hello \""),
        "should contain text child with trailing space: got: {js}"
    );
    assert!(
        js.contains("name"),
        "should contain expression child: got: {js}"
    );
}

// ---------------------------------------------------------------
// 22. Deeply nested JSX
// ---------------------------------------------------------------

#[test]
fn test_jsx_deeply_nested() {
    let js = emit_jsx("<div><ul><li>item</li></ul></div>;");
    assert!(
        js.contains("React.createElement(\"li\", null, \"item\")"),
        "deeply nested should work: got: {js}"
    );
    assert!(
        js.contains("React.createElement(\"ul\", null,"),
        "middle layer should work: got: {js}"
    );
}

#[test]
fn test_classic_jsx_flattens_comment_free_object_spreads_at_native_targets() {
    let source = r#"
        <Comp before={first()} {...{
            data: <span />,
            method() { return <b />; },
            get value() { return <i />; },
            set value(next) { use(next); }
        }} after={last()} />;
    "#;

    for target in [ScriptTarget::ES2015, ScriptTarget::ESNext] {
        let js = emit_jsx_with_options(source, JsxEmit::React, target);
        assert!(
            js.contains("before: first(), data: React.createElement(\"span\", null), method()"),
            "comment-free object-literal spreads should merge into the props object at {target:?}: {js}"
        );
        assert!(js.contains("get value()"), "{target:?}: {js}");
        assert!(js.contains("set value(next)"), "{target:?}: {js}");
        assert!(js.contains("after: last()"), "{target:?}: {js}");
        assert!(!js.contains("...{"), "{target:?}: {js}");
        assert!(!js.contains("Object.assign"), "{target:?}: {js}");
    }
}

#[test]
fn test_classic_jsx_native_spread_boundaries_remain_semantic() {
    let source = r#"
        <Comp {...props} />;
        <Comp {...{ __proto__: proto }} />;
        <Comp {...{ ...nested }} />;
        <Comp {...{ /* copy boundary */ get value() { return 1; } }} />;
    "#;
    let js = emit_jsx_with_options(source, JsxEmit::React, ScriptTarget::ESNext);

    assert!(
        js.contains("{ ...props }"),
        "non-object spread identity: {js}"
    );
    assert!(
        js.contains("{ ...{ __proto__: proto } }"),
        "__proto__ must retain a CopyDataProperties boundary: {js}"
    );
    assert!(
        js.contains("{ ...nested }"),
        "nested non-literal spread must retain its boundary: {js}"
    );
    assert!(
        js.contains("{ ...{ /* copy boundary */ get value()"),
        "commented spread objects must retain their comment and boundary: {js}"
    );
}

#[test]
fn test_automatic_jsx_single_spread_child_uses_jsxs_array() {
    let source = r#"
        const direct = <div>{...<span />}</div>;
        const asserted = <div>{...(<span /> as any)}</div>;
    "#;

    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        let js = emit_jsx_with_options(source, jsx, ScriptTarget::ES2015);
        if jsx == JsxEmit::ReactJSX {
            assert!(
                js.contains("jsxs"),
                "spread children require jsxs for {jsx:?}: {js}"
            );
        } else {
            assert!(js.contains("jsxDEV"), "{jsx:?}: {js}");
        }
        assert_eq!(
            js.matches("children: [...").count(),
            2,
            "both direct and asserted spread children require arrays for {jsx:?}: {js}"
        );
        assert!(
            !js.contains("children: ..."),
            "automatic JSX output must be syntactically valid for {jsx:?}: {js}"
        );
        if jsx == JsxEmit::ReactJSXDev {
            assert_eq!(
                js.matches(", true, { fileName:").count(),
                2,
                "jsxDEV must mark spread-child arrays as static children: {js}"
            );
        }
    }
}

#[test]
fn test_automatic_jsx_fragment_single_spread_child_uses_jsxs_array() {
    for jsx in [JsxEmit::ReactJSX, JsxEmit::ReactJSXDev] {
        let js = emit_jsx_with_options(
            "const value = <>{...<span />}</>;",
            jsx,
            ScriptTarget::ES2015,
        );
        if jsx == JsxEmit::ReactJSX {
            assert!(js.contains("jsxs"), "{jsx:?}: {js}");
        } else {
            assert!(js.contains("jsxDEV"), "{jsx:?}: {js}");
            assert!(js.contains(", true, { fileName:"), "{jsx:?}: {js}");
        }
        assert!(js.contains("Fragment"), "{jsx:?}: {js}");
        assert!(js.contains("children: [..."), "{jsx:?}: {js}");
        assert!(!js.contains("children: ..."), "{jsx:?}: {js}");
    }
}

#[test]
fn test_jsx_spread_child_does_not_change_nonspread_or_classic_shapes() {
    let one = emit_jsx_with_options(
        "const value = <div><span /></div>;",
        JsxEmit::ReactJSX,
        ScriptTarget::ES2015,
    );
    assert!(one.contains("jsx"), "{one}");
    assert!(
        !one.contains("jsxs"),
        "a regular single child still uses jsx: {one}"
    );
    assert!(!one.contains("children: ["), "{one}");

    let two = emit_jsx_with_options(
        "const value = <div><span /><b /></div>;",
        JsxEmit::ReactJSX,
        ScriptTarget::ES2015,
    );
    assert!(
        two.contains("jsxs"),
        "regular multiple children still use jsxs: {two}"
    );
    assert!(two.contains("children: ["), "{two}");

    let classic = emit_jsx_with_options(
        "const value = <div>{...<span />}</div>;",
        JsxEmit::React,
        ScriptTarget::ES2015,
    );
    assert!(
        classic
            .contains("React.createElement(\"div\", null, ...React.createElement(\"span\", null))"),
        "classic JSX keeps its spread argument shape: {classic}"
    );
}
