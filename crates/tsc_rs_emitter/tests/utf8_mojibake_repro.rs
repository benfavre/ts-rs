//! Repro for UTF-8 mojibake in JSX attribute emission.
//! See bext-prism issue: `"Créer un site"` -> `"CrÃ©er un site"`.

use tsc_rs_ast::JsxEmit;
use tsc_rs_emitter::emit_strip_types_jsx;
use tsc_rs_parser::parse;

fn compile(src: &str) -> String {
    let sf = parse("/t.tsx", src);
    let out = emit_strip_types_jsx(&sf, JsxEmit::ReactJSX, None);
    out.javascript
}

/// Same bug, slightly bigger sample showing all the symptoms together.
/// Mirrors the production repro: a JSX file that contains `"super-app"`
/// somewhere (triggering the bare-`super` recovery fixup) and a JSX
/// string attribute with non-ASCII chars (`é`, `…`, `—`). Before the
/// fix, every non-ASCII codepoint became a 2-byte Latin-1 sequence in
/// the bundle (`Démo` → `DÃ©mo`).
#[test]
fn bare_super_fixup_preserves_utf8_in_jsx_attrs() {
    let src = r#"
// designer rows are in-app links, super-app rows
function P() {
  return (
    <div>
      <Switcher
        title="Sites PRISM gérés par le designer"
        placeholder="Rechercher un site, un tag…"
        createLabel="Créer un site"
      />
    </div>
  );
}
"#;
    let out = compile(src);
    eprintln!("--- output ---\n{out}\n--- end ---");
    assert!(!out.contains("CrÃ©er"), "createLabel mojibake");
    assert!(!out.contains("gÃ©r"), "title mojibake");
    assert!(out.contains("Créer un site"), "createLabel preserved");
    assert!(out.contains("gérés"), "title preserved");
    assert!(out.contains('…'), "ellipsis preserved");
}

#[test]
fn jsx_attr_value_preserves_utf8_in_recovery_path() {
    // Minimal trigger reduced via delta debugging from real-world Header.tsx.
    // The IconGlo(item.id) is intentionally malformed JSX -- it puts the
    // parser into a recovery path. The CORRECT output has "Créer un site";
    // the mojibake output has "CrÃ©er un site" (UTF-8 0xC3 0xA9 bytes
    // mis-decoded as Latin-1 0xC3, 0xA9 then re-encoded as UTF-8).
    let src = " picker but in
  // separate sections; designer rows are in-app links, super-app rows
  // open in a new>
            <Switcher
              icon={<IconGlo(item.id)}
              createLabel=\"Créer un site\"";
    let out = compile(src);
    eprintln!("--- output ---");
    eprintln!("{out}");
    eprintln!("--- end output ---");
    assert!(
        !out.contains("CrÃ©er"),
        "mojibake found: 'CrÃ©er' in output"
    );
    assert!(
        out.contains("Créer un site"),
        "expected 'Créer un site' preserved in output"
    );
}
