//! LSP coverage for zod fixtures.
//!
//! For each fixture under `tests/cases/zod/`, walk for inline assertion
//! comments and verify the LSP query results.
//!
//! Assertion grammar (one per line, embedded in the fixture or in a
//! sibling `<name>.lsp.txt` file):
//!
//!   `// @hover <ident>: <substring>`     hover-at-decl(ident) contains substring
//!   `// @def   <ident> -> <file>:<l>:<c>`  go-to-def(ident) lands at file/line/col
//!   `// @xhover <ident>: <substring>`     same as @hover but expected-fail
//!
//! Lookup rules:
//!   - <ident> is the name (e.g. `s`, `Schema`, `User`). We find its
//!     FIRST declaration position by simple textual scan over the
//!     fixture source for `const <ident>`, `let <ident>`, `var <ident>`,
//!     `function <ident>`, or `type <ident>`. That position is the
//!     offset passed to hover/definition.
//!
//! Skipping: if the zod symlink is missing, the test is reported as
//! SKIP and the assertion is `assert!(true)` so CI stays green.
//!
//! Run with:
//!   cargo test --release -p tsc_rs_harness --test zod_lsp -- --nocapture

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tsc_rs_ast::CompilerOptions;
use tsc_rs_harness::lsp_executor;
use tsc_rs_harness::lsp_parser::{LspTest, LspTestFile, Marker};

const ZOD_DIR_REL: &str = "tests/cases/zod";

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to find workspace root")
        .to_path_buf()
}

fn zod_dir() -> PathBuf {
    workspace_root().join(ZOD_DIR_REL)
}

fn zod_present() -> bool {
    zod_dir().join("node_modules/zod/package.json").exists()
}

// ---------------------------------------------------------------------------
// Assertion parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Assertion {
    Hover {
        ident: String,
        substring: String,
        xfail: bool,
    },
    Def {
        ident: String,
        file: String,
        line: u32,
        col: u32,
        xfail: bool,
    },
}

fn parse_assertions(source: &str) -> Vec<Assertion> {
    let mut out = Vec::new();
    for raw in source.lines() {
        let line = raw.trim_start();
        let (rest, xfail) = if let Some(r) = line
            .strip_prefix("// @xhover ")
            .or_else(|| line.strip_prefix("//@xhover "))
        {
            (r, true)
        } else if let Some(r) = line
            .strip_prefix("// @hover ")
            .or_else(|| line.strip_prefix("//@hover "))
        {
            (r, false)
        } else if let Some(r) = line
            .strip_prefix("// @xdef ")
            .or_else(|| line.strip_prefix("//@xdef "))
        {
            if let Some(a) = parse_def(r.trim(), true) {
                out.push(a);
            }
            continue;
        } else if let Some(r) = line
            .strip_prefix("// @def ")
            .or_else(|| line.strip_prefix("//@def "))
        {
            if let Some(a) = parse_def(r.trim(), false) {
                out.push(a);
            }
            continue;
        } else {
            continue;
        };
        if let Some((ident, substring)) = rest.split_once(':') {
            out.push(Assertion::Hover {
                ident: ident.trim().to_string(),
                substring: substring.trim().to_string(),
                xfail,
            });
        }
    }
    out
}

fn parse_def(text: &str, xfail: bool) -> Option<Assertion> {
    let (ident, target) = text.split_once("->")?;
    let target = target.trim();
    let parts: Vec<&str> = target.rsplitn(3, ':').collect();
    // parts is reverse: [col, line, file]
    if parts.len() != 3 {
        return None;
    }
    let col: u32 = parts[0].trim().parse().ok()?;
    let line: u32 = parts[1].trim().parse().ok()?;
    let file: String = parts[2].trim().to_string();
    Some(Assertion::Def {
        ident: ident.trim().to_string(),
        file,
        line,
        col,
        xfail,
    })
}

// ---------------------------------------------------------------------------
// Identifier locator
// ---------------------------------------------------------------------------

/// Find the byte offset where `ident` is declared (after `const`, `let`,
/// `var`, `function`, `type`, or `interface`). Returns the offset of the
/// identifier's first character — what hover/def expect.
fn find_decl_offset(source: &str, ident: &str) -> Option<u32> {
    let keywords = ["const ", "let ", "var ", "function ", "type ", "interface "];
    for kw in keywords {
        let needle = format!("{kw}{ident}");
        // Scan every occurrence; the first match may be a prefix of a
        // longer identifier (e.g. `const uniqueArr` for `unique`), in
        // which case we have to keep looking.
        let mut start = 0;
        while let Some(rel) = source[start..].find(&needle) {
            let pos = start + rel;
            let end = pos + needle.len();
            let next_ok = end >= source.len()
                || !(source.as_bytes()[end].is_ascii_alphanumeric()
                    || source.as_bytes()[end] == b'_');
            if next_ok {
                let off = (pos + kw.len()) as u32;
                if std::env::var("ZOD_LSP_DEBUG_OFFSET").is_ok() {
                    eprintln!("[find_decl_offset] ident={ident:?} kw={kw:?} pos={pos} → off={off}");
                }
                return Some(off);
            }
            start = pos + needle.len();
        }
    }
    // Fallback: any destructured / parameter / method binding for `ident`.
    // Look for `[ident`, `,ident`, `{ident`, `...ident`, `(ident`, `<ident`,
    // or whitespace before `ident`. We restrict to ASCII identifier
    // boundaries on both sides to avoid matching substrings inside larger
    // names. Skip occurrences inside line comments and string literals.
    let bytes = source.as_bytes();
    let ident_bytes = ident.as_bytes();
    let mut i = 0usize;
    let mut in_line_comment = false;
    while i + ident_bytes.len() <= bytes.len() {
        let b = bytes[i];
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if &bytes[i..i + ident_bytes.len()] == ident_bytes {
            let prev_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let post = i + ident_bytes.len();
            let next_ok = post >= bytes.len()
                || !(bytes[post].is_ascii_alphanumeric() || bytes[post] == b'_');
            if prev_ok && next_ok {
                // Only accept positions that look like a DECLARATION site
                // (preceded by `[`, `,`, `{`, `...`, `(`, `<`, or
                // whitespace inside a destructuring / parameter context).
                // Avoid identifiers used in expressions on the right of `=`.
                let prev_nws = (0..i).rev().find(|j| !bytes[*j].is_ascii_whitespace());
                let prev_byte = prev_nws.map(|j| bytes[j]).unwrap_or(0);
                if matches!(prev_byte, b'[' | b'{' | b',' | b'(' | b'<' | 0) {
                    return Some(i as u32);
                }
                // `...rest` rest-element
                if i >= 3 && &bytes[i - 3..i] == b"..." {
                    return Some(i as u32);
                }
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Project + LSP test construction
// ---------------------------------------------------------------------------

fn collect_zod_files() -> Vec<PathBuf> {
    let root = zod_dir().join("node_modules/zod");
    let mut out = Vec::new();
    let mut stack = vec![root];
    let excluded_prefixes = [
        zod_dir().join("node_modules/zod/v3"),
        zod_dir().join("node_modules/zod/mini"),
        zod_dir().join("node_modules/zod/locales"),
        zod_dir().join("node_modules/zod/v4/mini"),
        zod_dir().join("node_modules/zod/v4/locales"),
    ];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if excluded_prefixes.iter().any(|p| path.starts_with(p)) {
                    continue;
                }
                stack.push(path);
            } else if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.ends_with(".d.ts") {
                    out.push(path);
                }
            }
        }
    }
    out
}

fn build_lsp_test(fixture: &Path) -> LspTest {
    let mut files = Vec::new();

    let fixture_source = std::fs::read_to_string(fixture).expect("read fixture for LSP test");
    let fixture_name = fixture.to_string_lossy().to_string();
    files.push(LspTestFile {
        name: fixture_name.clone(),
        content: fixture_source,
    });

    for path in collect_zod_files() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            files.push(LspTestFile {
                name: path.to_string_lossy().to_string(),
                content,
            });
        }
    }

    let mut options = CompilerOptions::default();
    options.module_resolution = Some("bundler".into());
    options.skip_lib_check = Some(true);
    options.es_module_interop = Some(true);
    options.strict = Some(true);
    options.target = Some(tsc_rs_ast::ScriptTarget::ES2022);

    LspTest {
        name: fixture
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        files,
        markers: Vec::new(),
        verify_commands: Vec::new(),
        verify_text: String::new(),
        options,
        has_edits: false,
        include_module_exports: false,
    }
}

// ---------------------------------------------------------------------------
// Test driver
// ---------------------------------------------------------------------------

enum AssertOutcome {
    Pass(String),
    Fail(String),
    Xfail(String),
    Xpass(String),
}

fn run_assertion(
    fixture: &Path,
    source: &str,
    qe: &tsc_rs_query::QueryEngine,
    analyses: &HashMap<String, lsp_executor::FileAnalysis>,
    assertion: &Assertion,
) -> AssertOutcome {
    let fixture_name = fixture.to_string_lossy().to_string();
    let label_for = |ident: &str, kind: &str| {
        format!(
            "{} [{}{}]",
            fixture.file_name().unwrap().to_string_lossy(),
            kind,
            format!(" {ident}")
        )
    };
    match assertion {
        Assertion::Hover {
            ident,
            substring,
            xfail,
        } => {
            let Some(offset) = find_decl_offset(source, ident) else {
                return AssertOutcome::Fail(format!(
                    "{}: identifier `{}` not found",
                    fixture_name, ident
                ));
            };
            let marker = Marker {
                name: ident.clone(),
                file_name: fixture_name.clone(),
                position: offset,
            };
            let hover = lsp_executor::hover_at_marker(qe, analyses, &marker);
            let label = label_for(ident, "hover");
            let actual = hover.unwrap_or_else(|| "<no hover>".to_string());
            let pass = actual.contains(substring);
            match (pass, *xfail) {
                (true, false) => AssertOutcome::Pass(format!(
                    "{label} → contains {substring:?} ({})",
                    truncate(&actual)
                )),
                (false, false) => AssertOutcome::Fail(format!(
                    "{label}: expected substring {substring:?}, got {:?}",
                    actual
                )),
                (true, true) => AssertOutcome::Xpass(format!(
                    "{label}: marker says xfail but hover already contains {substring:?}"
                )),
                (false, true) => AssertOutcome::Xfail(format!(
                    "{label}: expected {substring:?}, got {} (xfail)",
                    truncate(&actual)
                )),
            }
        }
        Assertion::Def {
            ident,
            file,
            line,
            col,
            xfail,
        } => {
            let Some(offset) = find_decl_offset(source, ident) else {
                return AssertOutcome::Fail(format!(
                    "{}: identifier `{}` not found",
                    fixture_name, ident
                ));
            };
            let marker = Marker {
                name: ident.clone(),
                file_name: fixture_name.clone(),
                position: offset,
            };
            let def = lsp_executor::definition_at_marker(qe, analyses, &marker);
            let label = label_for(ident, "def");
            let actual = match def {
                Some((target_file, span)) => {
                    // Convert span.start (byte offset) to line+col in the target file
                    let analysis = analyses.get(&target_file);
                    let (line_n, col_n) = analysis
                        .map(|a| offset_to_line_col(&a.source_text, span.start))
                        .unwrap_or((0, 0));
                    format!("{target_file}:{line_n}:{col_n}")
                }
                None => "<no def>".to_string(),
            };
            let expected = format!("{file}:{line}:{col}");
            // Match by suffix on file path (we use absolute paths).
            let pass = actual.ends_with(&expected);
            match (pass, *xfail) {
                (true, false) => AssertOutcome::Pass(format!("{label} → {actual}")),
                (false, false) => AssertOutcome::Fail(format!(
                    "{label}: expected ends-with {expected}, got {actual}"
                )),
                (true, true) => {
                    AssertOutcome::Xpass(format!("{label}: marker xfail but def landed: {actual}"))
                }
                (false, true) => AssertOutcome::Xfail(format!(
                    "{label}: expected {expected}, got {actual} (xfail)"
                )),
            }
        }
    }
}

fn truncate(s: &str) -> String {
    let one_line: String = s.replace('\n', " ").chars().take(80).collect();
    one_line
}

fn offset_to_line_col(source: &str, offset: u32) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, c) in source.char_indices() {
        if (i as u32) >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn collect_fixtures() -> Vec<PathBuf> {
    let dir = zod_dir();
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext == "ts" && !path.to_string_lossy().contains("node_modules") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn zod_lsp_assertions() {
    if !zod_present() {
        eprintln!("zod_lsp: SKIP — node_modules/zod missing");
        return;
    }

    let fixtures = collect_fixtures();
    if fixtures.is_empty() {
        panic!("no .ts fixtures under {}", zod_dir().display());
    }

    let mut summary: HashMap<&'static str, usize> = HashMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut xpasses: Vec<String> = Vec::new();
    let mut total_assertions = 0;

    for fixture in &fixtures {
        let source = std::fs::read_to_string(fixture).expect("read fixture");
        let assertions = parse_assertions(&source);
        if assertions.is_empty() {
            continue; // no LSP assertions in this fixture
        }
        total_assertions += assertions.len();

        let test = build_lsp_test(fixture);
        let (qe, analyses) = lsp_executor::build_analysis(&test);

        for assertion in &assertions {
            match run_assertion(fixture, &source, &qe, &analyses, assertion) {
                AssertOutcome::Pass(msg) => {
                    eprintln!("  PASS  {msg}");
                    *summary.entry("pass").or_default() += 1;
                }
                AssertOutcome::Fail(msg) => {
                    eprintln!("  FAIL  {msg}");
                    *summary.entry("fail").or_default() += 1;
                    failures.push(msg);
                }
                AssertOutcome::Xfail(msg) => {
                    eprintln!("  XFAIL {msg}");
                    *summary.entry("xfail").or_default() += 1;
                }
                AssertOutcome::Xpass(msg) => {
                    eprintln!("  XPASS {msg}");
                    *summary.entry("xpass").or_default() += 1;
                    xpasses.push(msg);
                }
            }
        }
    }

    eprintln!(
        "\nzod_lsp summary: {} pass, {} fail, {} xfail, {} xpass (of {} assertions across {} fixtures)",
        summary.get("pass").copied().unwrap_or(0),
        summary.get("fail").copied().unwrap_or(0),
        summary.get("xfail").copied().unwrap_or(0),
        summary.get("xpass").copied().unwrap_or(0),
        total_assertions,
        fixtures.len()
    );

    if !failures.is_empty() {
        panic!("{} LSP assertion(s) failed", failures.len());
    }
    if !xpasses.is_empty() {
        panic!(
            "{} xpass assertion(s) — remove the xfail marker:\n  {}",
            xpasses.len(),
            xpasses.join("\n  ")
        );
    }
}
