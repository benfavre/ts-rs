//! Cross-file alias following for find-all-references.
//!
//! `references_at` finds same-named identifiers across the project. That
//! misses every place where the entity is re-bound under another name:
//! `export { x as y }`, `import { x as z }`, `export default x` followed by
//! `import d from "./a"`, `export = N` followed by `import N2 = require()`,
//! and `const { a: b } = require()`. tsc's findAllReferences follows these
//! aliases forward (from the original to the new names).
//!
//! This module scans import/export clauses textually, builds the closure of
//! `(file, name)` pairs reachable from the target name, and returns the
//! identifier occurrences of every newly introduced local name.

use std::collections::HashSet;

use tsc_rs_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok<'a> {
    Ident(&'a str),
    Str(&'a str),
    Punct(u8),
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$' || b >= 0x80
}

fn is_ident_part(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}

/// Tokenize `src[start..]` until `max` tokens, a `;`, or a newline that
/// follows a complete clause. Comments are skipped.
fn tokens_from(src: &str, start: usize, max: usize) -> Vec<Tok<'_>> {
    let b = src.as_bytes();
    let mut i = start;
    let mut out = Vec::new();
    while i < b.len() && out.len() < max {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            let q = c;
            let s = i + 1;
            i += 1;
            while i < b.len() && b[i] != q && b[i] != b'\n' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            let e = i.min(b.len());
            out.push(Tok::Str(&src[s..e]));
            i += 1;
            continue;
        }
        if is_ident_start(c) {
            let s = i;
            while i < b.len() && is_ident_part(b[i]) {
                i += 1;
            }
            out.push(Tok::Ident(&src[s..i]));
            continue;
        }
        out.push(Tok::Punct(c));
        i += 1;
        if c == b';' {
            break;
        }
    }
    out
}

/// One alias edge: `(source module or None for local, source name)` →
/// `local name` in the declaring file.
#[derive(Debug, Clone)]
struct Edge {
    /// Module specifier the source name lives in; `None` = same file.
    from_spec: Option<String>,
    src_name: String,
    dst_name: String,
}

/// Parse `{ a, b as c, default as d, type e as f }` starting at `toks[i]`
/// (which must be `{`). Returns the (imported, local) pairs and the index
/// after the closing brace.
fn parse_specs(toks: &[Tok<'_>], mut i: usize) -> (Vec<(String, String)>, usize) {
    let mut out = Vec::new();
    i += 1;
    while i < toks.len() {
        match &toks[i] {
            Tok::Punct(b'}') => return (out, i + 1),
            Tok::Punct(b',') => i += 1,
            Tok::Ident(_) | Tok::Str(_) => {
                let mut name = match &toks[i] {
                    Tok::Ident(n) | Tok::Str(n) => n.to_string(),
                    _ => unreachable!(),
                };
                i += 1;
                // `type A` / `type A as B`
                if name == "type" {
                    if let Some(Tok::Ident(n)) = toks.get(i) {
                        if *n != "as" {
                            name = n.to_string();
                            i += 1;
                        }
                    }
                }
                let mut local = name.clone();
                if let Some(Tok::Ident("as")) = toks.get(i) {
                    if let Some(Tok::Ident(n) | Tok::Str(n)) = toks.get(i + 1) {
                        local = n.to_string();
                        i += 2;
                    }
                }
                out.push((name, local));
            }
            _ => i += 1,
        }
    }
    (out, i)
}

fn from_spec_after(toks: &[Tok<'_>], i: usize) -> Option<String> {
    match (toks.get(i), toks.get(i + 1)) {
        (Some(Tok::Ident("from")), Some(Tok::Str(s))) => Some(s.to_string()),
        _ => None,
    }
}

/// Collect alias edges declared by one file.
fn file_edges(src: &str) -> Vec<Edge> {
    let mut edges = Vec::new();
    let bytes = src.as_bytes();
    for kw in ["import", "export", "const", "let", "var"] {
        let mut from = 0;
        while let Some(rel) = src[from..].find(kw) {
            let pos = from + rel;
            from = pos + kw.len();
            let before_ok = pos == 0 || !is_ident_part(bytes[pos - 1]) && bytes[pos - 1] != b'.';
            let after_ok = from >= bytes.len() || !is_ident_part(bytes[from]);
            if !before_ok || !after_ok {
                continue;
            }
            let toks = tokens_from(src, from, 64);
            match kw {
                "export" => export_edges(&toks, &mut edges),
                "import" => import_edges(&toks, &mut edges),
                _ => require_edges(&toks, &mut edges),
            }
        }
    }
    edges
}

/// A module's export bindings: `(from module or None for local, name in that
/// module, exported name)`, plus the specifiers of its `export * from`.
pub(crate) fn module_exports(src: &str) -> (Vec<(Option<String>, String, String)>, Vec<String>) {
    let bytes = src.as_bytes();
    let mut edges = Vec::new();
    let mut stars = Vec::new();
    let mut from = 0;
    while let Some(rel) = src[from..].find("export") {
        let pos = from + rel;
        from = pos + "export".len();
        let before_ok = pos == 0 || !is_ident_part(bytes[pos - 1]) && bytes[pos - 1] != b'.';
        let after_ok = from >= bytes.len() || !is_ident_part(bytes[from]);
        if !before_ok || !after_ok {
            continue;
        }
        let toks = tokens_from(src, from, 64);
        if let (Some(Tok::Punct(b'*')), Some(Tok::Ident("from")), Some(Tok::Str(s))) =
            (toks.first(), toks.get(1), toks.get(2))
        {
            stars.push(s.to_string());
            continue;
        }
        if matches!(toks.first(), Some(Tok::Ident("import"))) {
            continue;
        }
        let mut local = Vec::new();
        export_edges(&toks, &mut local);
        edges.extend(
            local
                .into_iter()
                .map(|e| (e.from_spec, e.src_name, e.dst_name)),
        );
    }
    (edges, stars)
}

fn export_edges(toks: &[Tok<'_>], edges: &mut Vec<Edge>) {
    let mut i = 0;
    if let Some(Tok::Ident("type")) = toks.first() {
        i = 1;
    }
    match toks.get(i) {
        Some(Tok::Punct(b'{')) => {
            let (specs, after) = parse_specs(toks, i);
            let spec = from_spec_after(toks, after);
            for (name, local) in specs {
                edges.push(Edge {
                    from_spec: spec.clone(),
                    src_name: name,
                    dst_name: local,
                });
            }
        }
        Some(Tok::Ident("default")) => {
            let mut j = i + 1;
            while let Some(Tok::Ident("async" | "function" | "class" | "abstract")) = toks.get(j) {
                j += 1;
            }
            if let Some(Tok::Punct(b'*')) = toks.get(j) {
                j += 1;
            }
            if let Some(Tok::Ident(n)) = toks.get(j) {
                if *n != "extends" && *n != "implements" {
                    edges.push(Edge {
                        from_spec: None,
                        src_name: n.to_string(),
                        dst_name: "default".to_string(),
                    });
                }
            }
        }
        Some(Tok::Punct(b'=')) => {
            if let Some(Tok::Ident(n)) = toks.get(i + 1) {
                edges.push(Edge {
                    from_spec: None,
                    src_name: n.to_string(),
                    dst_name: "export=".to_string(),
                });
            }
        }
        // `export import X = N.Y` / `export import X = require("m")`
        Some(Tok::Ident("import")) => import_edges(&toks[i + 1..], edges),
        _ => {}
    }
}

fn import_edges(toks: &[Tok<'_>], edges: &mut Vec<Edge>) {
    let mut i = 0;
    if let Some(Tok::Ident("type")) = toks.first() {
        if !matches!(
            toks.get(1),
            Some(Tok::Ident("from")) | Some(Tok::Punct(b'='))
        ) {
            i = 1;
        }
    }
    // Default binding: `import X from`, `import X, {...} from`, `import X = require(...)`
    let mut default_local: Option<String> = None;
    let mut ns_local: Option<String> = None;
    let mut specs: Vec<(String, String)> = Vec::new();
    if let Some(Tok::Ident(n)) = toks.get(i) {
        if *n != "from" {
            let n = n.to_string();
            i += 1;
            if let Some(Tok::Punct(b'=')) = toks.get(i) {
                // import X = require("m")  |  import X = A.B
                match (toks.get(i + 1), toks.get(i + 2), toks.get(i + 3)) {
                    (Some(Tok::Ident("require")), Some(Tok::Punct(b'(')), Some(Tok::Str(s))) => {
                        edges.push(Edge {
                            from_spec: Some(s.to_string()),
                            src_name: "export=".to_string(),
                            dst_name: n,
                        });
                    }
                    (Some(Tok::Ident(first)), _, _) => {
                        // `import X = N` / `import X = N.M` — alias of the last name
                        let mut last = first.to_string();
                        let mut j = i + 2;
                        while let (Some(Tok::Punct(b'.')), Some(Tok::Ident(m))) =
                            (toks.get(j), toks.get(j + 1))
                        {
                            last = m.to_string();
                            j += 2;
                        }
                        edges.push(Edge {
                            from_spec: None,
                            src_name: last,
                            dst_name: n,
                        });
                    }
                    _ => {}
                }
                return;
            }
            default_local = Some(n);
            if let Some(Tok::Punct(b',')) = toks.get(i) {
                i += 1;
            }
        }
    }
    match toks.get(i) {
        Some(Tok::Punct(b'{')) => {
            let (s, after) = parse_specs(toks, i);
            specs = s;
            i = after;
        }
        Some(Tok::Punct(b'*')) => {
            if let (Some(Tok::Ident("as")), Some(Tok::Ident(n))) =
                (toks.get(i + 1), toks.get(i + 2))
            {
                ns_local = Some(n.to_string());
                i += 3;
            }
        }
        _ => {}
    }
    let Some(spec) = from_spec_after(toks, i) else {
        return;
    };
    if let Some(d) = default_local {
        edges.push(Edge {
            from_spec: Some(spec.clone()),
            src_name: "default".to_string(),
            dst_name: d.clone(),
        });
        edges.push(Edge {
            from_spec: Some(spec.clone()),
            src_name: "export=".to_string(),
            dst_name: d,
        });
    }
    if let Some(ns) = ns_local {
        edges.push(Edge {
            from_spec: Some(spec.clone()),
            src_name: "export=".to_string(),
            dst_name: ns,
        });
    }
    for (name, local) in specs {
        edges.push(Edge {
            from_spec: Some(spec.clone()),
            src_name: name,
            dst_name: local,
        });
    }
}

/// `const X = require("m")`, `const { a, b: c } = require("m")`.
fn require_edges(toks: &[Tok<'_>], edges: &mut Vec<Edge>) {
    let req_spec = |j: usize| -> Option<String> {
        match (
            toks.get(j),
            toks.get(j + 1),
            toks.get(j + 2),
            toks.get(j + 3),
        ) {
            (
                Some(Tok::Punct(b'=')),
                Some(Tok::Ident("require")),
                Some(Tok::Punct(b'(')),
                Some(Tok::Str(s)),
            ) => Some(s.to_string()),
            _ => None,
        }
    };
    match toks.first() {
        Some(Tok::Ident(n)) => {
            if let Some(spec) = req_spec(1) {
                edges.push(Edge {
                    from_spec: Some(spec),
                    src_name: "export=".to_string(),
                    dst_name: n.to_string(),
                });
            }
        }
        Some(Tok::Punct(b'{')) => {
            // `{ a, b: c }` — reuse the spec parser with `:` as `as`.
            let mut pairs = Vec::new();
            let mut i = 1;
            while i < toks.len() {
                match &toks[i] {
                    Tok::Punct(b'}') => {
                        i += 1;
                        break;
                    }
                    Tok::Ident(n) => {
                        let mut local = n.to_string();
                        if let (Some(Tok::Punct(b':')), Some(Tok::Ident(l))) =
                            (toks.get(i + 1), toks.get(i + 2))
                        {
                            local = l.to_string();
                            i += 2;
                        }
                        pairs.push((n.to_string(), local));
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            if let Some(spec) = req_spec(i) {
                for (name, local) in pairs {
                    edges.push(Edge {
                        from_spec: Some(spec.clone()),
                        src_name: name,
                        dst_name: local,
                    });
                }
            }
        }
        _ => {}
    }
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Resolve a module specifier relative to `from_file` against `files`.
pub(crate) fn resolve_spec(from_file: &str, spec: &str, files: &[(&str, &str)]) -> Option<String> {
    let known = |p: &str| files.iter().any(|(f, _)| *f == p);
    if spec.starts_with('.') || spec.starts_with('/') {
        let dir = match from_file.rfind('/') {
            Some(i) => &from_file[..i],
            None => "",
        };
        let joined = if spec.starts_with('/') {
            spec.to_string()
        } else {
            format!("{dir}/{spec}")
        };
        let base = normalize_path(&joined);
        let stem = base
            .strip_suffix(".js")
            .or_else(|| base.strip_suffix(".jsx"))
            .or_else(|| base.strip_suffix(".mjs"))
            .or_else(|| base.strip_suffix(".cjs"))
            .unwrap_or(&base)
            .to_string();
        let candidates = [
            base.clone(),
            format!("{stem}.ts"),
            format!("{stem}.tsx"),
            format!("{stem}.d.ts"),
            format!("{stem}.js"),
            format!("{stem}.jsx"),
            format!("{base}/index.ts"),
            format!("{base}/index.tsx"),
            format!("{base}/index.d.ts"),
            format!("{base}/index.js"),
        ];
        return candidates.into_iter().find(|c| known(c));
    }
    // Bare specifier: a file that declares `declare module "spec"`.
    let needle_dq = format!("declare module \"{spec}\"");
    let needle_sq = format!("declare module '{spec}'");
    files
        .iter()
        .find(|(_, src)| src.contains(&needle_dq) || src.contains(&needle_sq))
        .map(|(f, _)| f.to_string())
}

/// When `name` in `cursor_file` is a default import (`import name from "m"`,
/// `import { default as name } from "m"`), the local name that module `m`
/// (following re-exports) exports as default. tsc treats the default import
/// as the same entity, so its references include the original's.
pub(crate) fn default_import_origin(
    files: &[(&str, &str)],
    cursor_file: &str,
    name: &str,
) -> Option<String> {
    reexport_origin(files, cursor_file, name, true)
}

/// The original local name behind `name` in `cursor_file` when it is bound
/// by an import or re-export from another module (`export { default } from
/// "./c"`, `export { foo as default } from "./a"`), following re-exports.
/// With `default_only`, only bindings of that module's default export count.
pub(crate) fn reexport_origin(
    files: &[(&str, &str)],
    cursor_file: &str,
    name: &str,
    default_only: bool,
) -> Option<String> {
    let src = files.iter().find(|(f, _)| *f == cursor_file)?.1;
    let edge = file_edges(src).into_iter().find(|e| {
        e.dst_name == name
            && e.from_spec.is_some()
            && e.src_name != "export="
            && (!default_only || e.src_name == "default")
    })?;
    let mut module = resolve_spec(cursor_file, edge.from_spec.as_deref()?, files)?;
    let mut wanted = edge.src_name.clone();
    for _ in 0..8 {
        let msrc = files.iter().find(|(f, _)| *f == module)?.1;
        let (edges, _) = module_exports(msrc);
        let Some((from_spec, src_name, _)) = edges.into_iter().find(|(_, _, dst)| *dst == wanted)
        else {
            // Reached the declaring module (`export function foo`).
            return (wanted != "default").then_some(wanted);
        };
        match from_spec {
            None => return (src_name != "default").then_some(src_name),
            Some(spec) => {
                module = resolve_spec(&module, &spec, files)?;
                wanted = src_name;
            }
        }
    }
    None
}

/// Occurrences of every name that aliases `target_name` (forward direction),
/// across `files` (`(path, source)`), excluding the target name itself.
pub(crate) fn alias_reference_spans(
    files: &[(&str, &str)],
    target_name: &str,
) -> Vec<(String, Span)> {
    if target_name.is_empty() {
        return Vec::new();
    }
    // Only bother when some file re-binds names.
    let edges: Vec<(usize, Vec<Edge>)> = files
        .iter()
        .enumerate()
        .map(|(i, (_, src))| (i, file_edges(src)))
        .filter(|(_, e)| !e.is_empty())
        .collect();
    if edges.is_empty() {
        return Vec::new();
    }
    let resolved: Vec<Vec<Option<Option<usize>>>> = edges
        .iter()
        .map(|(fi, es)| {
            es.iter()
                .map(|e| {
                    e.from_spec.as_ref().map(|s| {
                        resolve_spec(files[*fi].0, s, files)
                            .and_then(|p| files.iter().position(|(f, _)| *f == p))
                    })
                })
                .collect()
        })
        .collect();

    let mut pairs: HashSet<(usize, String)> = (0..files.len())
        .map(|i| (i, target_name.to_string()))
        .collect();
    let mut names: HashSet<String> = HashSet::from([target_name.to_string()]);
    loop {
        let mut added = false;
        for (k, (fi, es)) in edges.iter().enumerate() {
            for (m, e) in es.iter().enumerate() {
                let src_ok = match resolved[k][m] {
                    None => pairs.contains(&(*fi, e.src_name.clone())),
                    Some(Some(target_file)) => pairs.contains(&(target_file, e.src_name.clone())),
                    // Unresolved module: fall back to the name alone.
                    Some(None) => names.contains(&e.src_name),
                };
                if src_ok && pairs.insert((*fi, e.dst_name.clone())) {
                    names.insert(e.dst_name.clone());
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }

    let mut out = Vec::new();
    for (fi, name) in pairs {
        if name == target_name || name == "default" || name == "export=" {
            continue;
        }
        for span in crate::find_identifier_occurrences(files[fi].1, &name) {
            out.push((files[fi].0.to_string(), span));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(files: &[(&str, &str)], target: &str) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = alias_reference_spans(files, target)
            .into_iter()
            .map(|(f, sp)| {
                let src = files.iter().find(|(p, _)| *p == f).unwrap().1;
                (f, src[sp.start as usize..sp.end as usize].to_string())
            })
            .collect();
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn renamed_exports_and_imports_are_followed() {
        let files = [
            ("/a.ts", "var x;\nexport { x };\nexport { x as y };\n"),
            ("/b.ts", "import { x, y } from \"./a\";\nx; y;\n"),
            ("/c.ts", "import { y as z } from \"./a\";\nz;\n"),
        ];
        assert_eq!(
            names(&files, "x"),
            vec![
                ("/a.ts".into(), "y".into()),
                ("/b.ts".into(), "y".into()),
                ("/c.ts".into(), "z".into())
            ]
        );
        // Aliases are followed forward only.
        assert_eq!(names(&files, "z"), vec![]);
    }

    #[test]
    fn default_and_export_equals_are_followed() {
        let files = [
            ("/export.ts", "const foo = 1;\nexport default foo;\n"),
            ("/re.ts", "export { default } from \"./export\";\nexport { default as fooDefault } from \"./export\";\n"),
            ("/use.ts", "import fooDefault from \"./re\";\nimport { fooDefault as f2 } from \"./re\";\nfooDefault; f2;\n"),
            ("/n.ts", "namespace N { export var x = 0; }\nexport = N;\n"),
            ("/m.ts", "import M = require(\"./n\");\nimport * as K from \"./n\";\nM.x; K.x;\n"),
        ];
        let foo = names(&files, "foo");
        assert!(foo.contains(&("/re.ts".into(), "fooDefault".into())));
        assert!(foo.contains(&("/use.ts".into(), "fooDefault".into())));
        assert!(foo.contains(&("/use.ts".into(), "f2".into())));
        let n = names(&files, "N");
        assert!(n.contains(&("/m.ts".into(), "M".into())));
        assert!(n.contains(&("/m.ts".into(), "K".into())));
    }

    #[test]
    fn re_export_chains_reach_default_imports() {
        let files = [
            ("/a.ts", "export function foo(): void {}\n"),
            ("/b.ts", "export { foo as bar } from \"./a\";\n"),
            ("/c.ts", "export { foo as default } from \"./a\";\n"),
            ("/d.ts", "export { default } from \"./c\";\n"),
            (
                "/e.ts",
                "import { bar } from \"./b\";\nimport baz from \"./c\";\nimport { default as bang } from \"./c\";\nimport boom from \"./d\";\nbar(); baz(); bang(); boom();\n",
            ),
        ];
        let foo = names(&files, "foo");
        for want in ["bar", "baz", "bang", "boom"] {
            assert!(
                foo.iter().any(|(_, n)| n == want),
                "{want} missing from {foo:?}"
            );
        }
        assert_eq!(
            default_import_origin(&files, "/e.ts", "boom").as_deref(),
            Some("foo")
        );
        assert_eq!(
            default_import_origin(&files, "/e.ts", "baz").as_deref(),
            Some("foo")
        );
    }

    #[test]
    fn require_destructuring_is_followed() {
        let files = [
            ("/X.js", "module.exports = { x: 1 };\n"),
            (
                "/Y.js",
                "const { x: renamed } = require(\"./X\");\nrenamed;\n",
            ),
        ];
        assert_eq!(names(&files, "x"), vec![("/Y.js".into(), "renamed".into())]);
    }
}
