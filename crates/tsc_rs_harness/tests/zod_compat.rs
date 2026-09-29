//! Zod compatibility test runner.
//!
//! Discovers `.ts` fixtures under `tests/cases/zod/`, runs each through
//! `TsProject::compile()` (the full multi-file pipeline), and asserts the
//! collected diagnostics against header annotations.
//!
//! Annotation grammar:
//!   `// @expect: no-errors`
//!     The fixture must produce zero diagnostics.
//!   `// @expect-error: <CODE>`
//!     The fixture must produce at least one diagnostic with that TS code.
//!   `// @expect-error: <CODE> at line <N>`
//!     The fixture must produce a diagnostic with that TS code at line N.
//!   `// @xfail: <reason>`
//!     This fixture is *expected to fail* against the checker today
//!     (typically because it exercises a checker feature that isn't
//!     implemented yet). The test runner inverts the pass/fail signal:
//!     a fixture marked `@xfail` that actually passes against current
//!     tsc-rs is reported as XPASS (a successful regression toward
//!     correctness — usually means someone closed a checker gap and the
//!     marker can be removed).
//!
//! Skipping:
//!   - If `tests/cases/zod/node_modules/zod` is missing the run is skipped
//!     (treated as pass + warning) so CI without a zod install stays green.
//!
//! Run with:
//!   cargo test --release -p tsc_rs_harness --test zod_compat -- --nocapture

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tsc_rs_ast::{Diagnostic, DiagnosticCategory};
use tsc_rs_project::TsProject;

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
    let path = zod_dir().join("node_modules/zod/package.json");
    path.exists()
}

#[derive(Debug, Clone)]
enum Expectation {
    NoErrors,
    /// Specific error code; optional line for position pinning.
    Error {
        code: u32,
        line: Option<u32>,
    },
}

fn parse_expectations(source: &str) -> (Vec<Expectation>, Option<String>) {
    let mut out = Vec::new();
    let mut xfail: Option<String> = None;
    for line in source.lines() {
        let trim = line.trim_start();
        if let Some(rest) = trim
            .strip_prefix("// @xfail:")
            .or_else(|| trim.strip_prefix("//@xfail:"))
        {
            xfail = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = trim
            .strip_prefix("// @expect:")
            .or_else(|| trim.strip_prefix("//@expect:"))
        {
            if rest.trim() == "no-errors" {
                out.push(Expectation::NoErrors);
            }
            continue;
        }
        if let Some(rest) = trim
            .strip_prefix("// @expect-error:")
            .or_else(|| trim.strip_prefix("//@expect-error:"))
        {
            if let Some(exp) = parse_expect_error(rest.trim()) {
                out.push(exp);
            }
        }
    }
    (out, xfail)
}

fn parse_expect_error(text: &str) -> Option<Expectation> {
    // Accept "2322" or "2322 at line 12 col 7" (col is ignored for now).
    let mut parts = text.split_whitespace();
    let code: u32 = parts.next()?.parse().ok()?;
    let mut line = None;
    let rest: Vec<&str> = parts.collect();
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "at" && i + 2 < rest.len() && rest[i + 1] == "line" {
            line = rest[i + 2].trim_end_matches(',').parse().ok();
            i += 3;
        } else {
            i += 1;
        }
    }
    Some(Expectation::Error { code, line })
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

#[derive(Debug)]
struct RunResult {
    diagnostics: Vec<Diagnostic>,
}

fn run_fixture(fixture: &Path) -> RunResult {
    // Build a one-off tsconfig that includes only this fixture + zod's d.ts files.
    // Reusing the shared tsconfig (which globs `*.ts`) would also pull in the
    // OTHER fixtures, so each test would type-check N files instead of 1 — slow
    // and noisy. Synthesize a tsconfig dynamically.
    let zod_dir = zod_dir();
    let base_cfg = zod_dir.join("tsconfig.json");
    let base_text = std::fs::read_to_string(&base_cfg).expect("read base tsconfig");
    // Replace include with this one fixture, keep zod globs.
    let fixture_name = fixture
        .file_name()
        .expect("fixture file name")
        .to_string_lossy()
        .to_string();
    let edited = base_text.replace("\"*.ts\"", &format!("\"{}\"", fixture_name));
    let tmp_cfg = zod_dir.join(format!(".__zod_test_tsconfig_{}.json", fixture_name));
    std::fs::write(&tmp_cfg, edited).expect("write temp tsconfig");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let project = TsProject::from_config(tmp_cfg.to_str().expect("utf8 tsconfig path"))
            .expect("load test tsconfig");
        project.compile()
    }));
    let _ = std::fs::remove_file(&tmp_cfg);
    let compilation = match result {
        Ok(c) => c,
        Err(_) => {
            return RunResult {
                diagnostics: vec![Diagnostic {
                    code: 9999,
                    message: "tsc-rs panicked while checking this fixture".into(),
                    category: DiagnosticCategory::Error,
                    file_name: Some(fixture.to_string_lossy().to_string()),
                    span: None,
                    related: None,
                }],
            };
        }
    };

    // Collect diagnostics that belong to the fixture itself — drop noise from
    // zod's d.ts files (skipLibCheck isn't honored yet by tsc-rs).
    let fixture_path_str = fixture.to_string_lossy().to_string();
    let mut diagnostics = compilation.diagnostics.clone();
    for file in &compilation.files {
        if file.file_name == fixture_path_str {
            diagnostics.extend(file.diagnostics.clone());
        }
    }
    RunResult { diagnostics }
}

fn line_of(diag: &Diagnostic, source: &str) -> Option<u32> {
    let span = diag.span?;
    let mut line = 1u32;
    for (i, c) in source.char_indices() {
        if (i as u32) >= span.start {
            return Some(line);
        }
        if c == '\n' {
            line += 1;
        }
    }
    Some(line)
}

enum Outcome {
    Pass(String),
    Fail(String),
    /// Expected-to-fail marker hit (test failed as documented). String holds the marker reason.
    Xfail(String),
    /// Expected-to-fail marker was set, but the fixture passes today — flag for cleanup.
    Xpass(String),
}

fn check_fixture(fixture: &Path) -> Outcome {
    let source = match std::fs::read_to_string(fixture) {
        Ok(s) => s,
        Err(e) => return Outcome::Fail(format!("read {}: {}", fixture.display(), e)),
    };
    let (expects, xfail) = parse_expectations(&source);
    if expects.is_empty() {
        return Outcome::Fail(format!(
            "no @expect / @expect-error annotations in {}",
            fixture.display()
        ));
    }
    let inner = check_fixture_inner(fixture, &source, &expects);
    match (inner, xfail) {
        (Ok(s), None) => Outcome::Pass(s),
        (Err(e), None) => Outcome::Fail(e),
        (Ok(s), Some(reason)) => Outcome::Xpass(format!(
            "{}: marker says xfail({reason}) but it passed: {s}",
            fixture.file_name().unwrap().to_string_lossy()
        )),
        (Err(e), Some(reason)) => Outcome::Xfail(format!(
            "{}: xfail({reason}) — {}",
            fixture.file_name().unwrap().to_string_lossy(),
            e.lines().next().unwrap_or(""),
        )),
    }
}

fn check_fixture_inner(
    fixture: &Path,
    source: &str,
    expects: &[Expectation],
) -> Result<String, String> {
    let source = source.to_string();
    let RunResult { diagnostics } = run_fixture(fixture);
    let errors: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| d.category == DiagnosticCategory::Error)
        .collect();

    // --- expectations -> matched diagnostics ---
    let mut matched = vec![false; errors.len()];
    let mut missing: Vec<String> = Vec::new();
    let mut unexpected_no_errors: Option<()> = None;

    for exp in expects {
        match exp {
            Expectation::NoErrors => {
                if !errors.is_empty() {
                    unexpected_no_errors = Some(());
                }
            }
            Expectation::Error { code, line } => {
                let mut found_idx = None;
                for (i, d) in errors.iter().enumerate() {
                    if matched[i] {
                        continue;
                    }
                    if d.code != *code {
                        continue;
                    }
                    if let Some(want_line) = line {
                        if line_of(d, &source) != Some(*want_line) {
                            continue;
                        }
                    }
                    found_idx = Some(i);
                    break;
                }
                if let Some(i) = found_idx {
                    matched[i] = true;
                } else {
                    let pos_str = line.map(|l| format!(" at line {l}")).unwrap_or_default();
                    missing.push(format!("TS{code}{pos_str}"));
                }
            }
        }
    }

    let extras: Vec<String> = errors
        .iter()
        .zip(&matched)
        .filter_map(|(d, m)| {
            if *m {
                None
            } else {
                let l = line_of(d, &source)
                    .map(|n| format!(" L{n}"))
                    .unwrap_or_default();
                let msg_short: String = d
                    .message
                    .chars()
                    .take(120)
                    .collect::<String>()
                    .replace('\n', " ");
                Some(format!("TS{}{l}: {msg_short}", d.code))
            }
        })
        .collect();

    let want_no_errors = matches!(expects.first(), Some(Expectation::NoErrors));
    if want_no_errors && unexpected_no_errors.is_some() {
        return Err(format!(
            "{}: expected no errors but got {}:\n  - {}",
            fixture.file_name().unwrap().to_string_lossy(),
            errors.len(),
            extras.join("\n  - ")
        ));
    }
    if !missing.is_empty() {
        return Err(format!(
            "{}: missing expected diagnostics: {}{}",
            fixture.file_name().unwrap().to_string_lossy(),
            missing.join(", "),
            if extras.is_empty() {
                String::new()
            } else {
                format!("\n  unexpected extras:\n    - {}", extras.join("\n    - "))
            }
        ));
    }
    if !want_no_errors && !extras.is_empty() {
        return Err(format!(
            "{}: unexpected extra diagnostics:\n  - {}",
            fixture.file_name().unwrap().to_string_lossy(),
            extras.join("\n  - ")
        ));
    }
    let count = expects
        .iter()
        .filter(|e| matches!(e, Expectation::Error { .. }))
        .count();
    Ok(format!(
        "{}: ok ({} expected error{}, {} actual diag{})",
        fixture.file_name().unwrap().to_string_lossy(),
        count,
        if count == 1 { "" } else { "s" },
        errors.len(),
        if errors.len() == 1 { "" } else { "s" },
    ))
}

#[test]
fn zod_fixtures_pass_annotations() {
    if !zod_present() {
        eprintln!(
            "zod_compat: SKIP — {} missing.\n  Symlink the zod install to enable these tests:\n  ln -s /path/to/app/node_modules/zod {}/node_modules/zod",
            zod_dir().join("node_modules/zod").display(),
            zod_dir().display(),
        );
        return;
    }

    let fixtures = collect_fixtures();
    if fixtures.is_empty() {
        panic!("no .ts fixtures under {}", zod_dir().display());
    }

    let mut summary: HashMap<&'static str, usize> = HashMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut xpasses: Vec<String> = Vec::new();
    for fixture in &fixtures {
        match check_fixture(fixture) {
            Outcome::Pass(line) => {
                eprintln!("  PASS  {line}");
                *summary.entry("pass").or_default() += 1;
            }
            Outcome::Fail(err) => {
                eprintln!("  FAIL  {err}");
                *summary.entry("fail").or_default() += 1;
                failures.push(err);
            }
            Outcome::Xfail(line) => {
                eprintln!("  XFAIL {line}");
                *summary.entry("xfail").or_default() += 1;
            }
            Outcome::Xpass(line) => {
                eprintln!("  XPASS {line}");
                *summary.entry("xpass").or_default() += 1;
                xpasses.push(line);
            }
        }
    }
    eprintln!(
        "\nzod_compat summary: {} pass, {} fail, {} xfail, {} xpass (of {} fixtures)",
        summary.get("pass").copied().unwrap_or(0),
        summary.get("fail").copied().unwrap_or(0),
        summary.get("xfail").copied().unwrap_or(0),
        summary.get("xpass").copied().unwrap_or(0),
        fixtures.len()
    );
    if !failures.is_empty() {
        panic!("{} unexpected failure(s)", failures.len());
    }
    if !xpasses.is_empty() {
        panic!(
            "{} xpass case(s) — remove the @xfail marker from:\n  {}",
            xpasses.len(),
            xpasses.join("\n  ")
        );
    }
}
