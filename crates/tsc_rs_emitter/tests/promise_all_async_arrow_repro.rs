// Repro: bext PRISM compile reported "Uncaught SyntaxError: missing ) after
// argument list" for an inline `async (e) => { ... }` arrow passed to
// `arr.map(...)` inside `Promise.all(...)`. bext emits via emit_strip_types.
// Bisect which syntactic ingredient triggers the broken emit.

use tsc_rs_emitter::emit_strip_types;

fn strip(src: &str) -> String {
    let file = tsc_rs_parser::parse("test.tsx", src);
    emit_strip_types(&file).javascript
}

fn balanced(js: &str) -> bool {
    let (mut p, mut b, mut c) = (0i32, 0i32, 0i32);
    let mut in_s: Option<char> = None;
    let mut prev = '\0';
    for ch in js.chars() {
        if let Some(q) = in_s {
            if ch == q && prev != '\\' {
                in_s = None;
            }
            prev = ch;
            continue;
        }
        match ch {
            '"' | '\'' | '`' => in_s = Some(ch),
            '(' => p += 1,
            ')' => p -= 1,
            '{' => b += 1,
            '}' => b -= 1,
            '[' => c += 1,
            ']' => c -= 1,
            _ => {}
        }
        prev = ch;
    }
    p == 0 && b == 0 && c == 0
}

fn check(name: &str, src: &str) {
    let js = strip(src);
    let ok = balanced(&js);
    eprintln!(
        "\n===== {} (balanced={}) =====\n{}\n===== END {} =====",
        name, ok, js, name
    );
    assert!(
        ok,
        "{}: emitted JS has unbalanced delimiters (the bug)",
        name
    );
}

#[test]
fn v1_async_arrow_block_in_map_in_promise_all() {
    check(
        "v1_basic",
        r#"
async function loader(host: string, entries: { path: string }[]) {
  const flags = await Promise.all(entries.map(async (e) => {
    try { return !!(await seoForPath(host, e.path)).noindex; }
    catch { return false; }
  }));
  return flags;
}
"#,
    );
}

#[test]
fn v2_with_trailing_comma() {
    check(
        "v2_trailing_comma",
        r#"
async function loader(host: string, entries: { path: string }[]) {
  const flags = await Promise.all(
    entries.map(async (e) => {
      try { return !!(await seoForPath(host, e.path)).noindex; }
      catch { return false; }
    }),
  );
  return flags;
}
"#,
    );
}

#[test]
fn v3_with_block_comment_in_catch() {
    check(
        "v3_block_comment",
        r#"
async function loader(host: string, entries: { path: string }[]) {
  const flags = await Promise.all(
    entries.map(async (e) => {
      try { return !!(await seoForPath(host, e.path)).noindex; }
      catch { return false; /* default: index */ }
    }),
  );
  return flags;
}
"#,
    );
}

#[test]
fn v4_simple_async_arrow_no_trycatch() {
    check(
        "v4_simple",
        r#"
async function loader(host: string, entries: { path: string }[]) {
  const flags = await Promise.all(entries.map(async (e) => await seoForPath(host, e.path)));
  return flags;
}
"#,
    );
}

#[test]
fn v5_typed_arrow_param() {
    check(
        "v5_typed_param",
        r#"
async function loader(host: string, entries: { path: string }[]) {
  const flags = await Promise.all(entries.map(async (e: { path: string }) => {
    return !!(await seoForPath(host, e.path)).noindex;
  }));
  return flags;
}
"#,
    );
}
