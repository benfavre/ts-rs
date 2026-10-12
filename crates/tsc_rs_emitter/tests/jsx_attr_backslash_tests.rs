//! A JSX string attribute has no escape sequences: a backslash in it is a
//! backslash (JSX spec; tsc and Babel keep it). Found by `scripts/emit-diff.py`
//! on the bext sites: `onclick="return confirm('l\'accès')"` lost its
//! backslash and the browser got a syntax error; `pattern="0(\.\d{1,2})?"`
//! became `0(.d{1,2})?`.

use tsc_rs_ast::JsxEmit;
use tsc_rs_emitter::emit_strip_types_jsx;
use tsc_rs_parser::parse;

fn compile(src: &str) -> String {
    let sf = parse("/t.tsx", src);
    emit_strip_types_jsx(&sf, JsxEmit::ReactJSX, None).javascript
}

#[test]
fn backslash_before_quote_is_kept() {
    let out =
        compile(r#"export const A = () => <button onclick="return confirm('l\'accès ?');" />;"#);
    // The JS string holds `l\'acc`, so the emitted literal escapes the backslash.
    assert!(
        out.contains(r#"onclick: "return confirm('l\\'acc"#),
        "{out}"
    );
}

#[test]
fn regex_pattern_backslashes_are_kept() {
    let out = compile(r#"export const A = () => <input pattern="0(\.\d{1,2})?" />;"#);
    assert!(out.contains(r#"pattern: "0(\\.\\d{1,2})?""#), "{out}");
}

#[test]
fn unicode_escape_text_is_literal() {
    let out = compile(r#"export const A = () => <p title="a\u00a0b" />;"#);
    assert!(out.contains(r#"title: "a\\u00a0b""#), "{out}");
}

#[test]
fn single_quoted_attribute_keeps_backslash() {
    let out = compile(r#"export const A = () => <p title='a\b' />;"#);
    assert!(out.contains(r#"title: 'a\\b'"#), "{out}");
}

#[test]
fn expression_container_string_still_escapes() {
    // `{"..."}` is a JS string: its escapes are escapes.
    let out = compile(r#"export const A = () => <p title={"a\u00a0b"} />;"#);
    assert!(out.contains(r#"title: "a\u00a0b""#), "{out}");
}

#[test]
fn attribute_without_backslash_is_unchanged() {
    let out = compile(r#"export const A = () => <p title="Créer un site" />;"#);
    assert!(out.contains(r#"title: "Créer un site""#), "{out}");
}
