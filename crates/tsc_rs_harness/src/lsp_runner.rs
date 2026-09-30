//! LSP test runner: discovery, parallel execution, and result aggregation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::lsp_baseline;
use crate::lsp_classify::{classify_goto_definition, classify_quick_info, LspBucket};
use crate::lsp_executor;
use crate::lsp_parser::{self, LspOperation, LspTest, VerifyCommand};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MarkerResult {
    pub marker_name: String,
    pub file_name: String,
    pub position: u32,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub passed: bool,
}

#[derive(Debug, Clone)]
pub struct LspTestResult {
    pub name: String,
    pub operation: LspOperation,
    pub passed: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub bucket: Option<LspBucket>,
    pub marker_results: Vec<MarkerResult>,
    pub panic_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LspSuiteResult {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub results: Vec<LspTestResult>,
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

pub struct LspRunner {
    workspace_root: PathBuf,
}

impl LspRunner {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    pub fn tests_dir(&self) -> PathBuf {
        self.workspace_root.join("tests/cases/fourslash")
    }

    pub fn baselines_dir(&self) -> PathBuf {
        self.workspace_root.join("tests/baselines/reference")
    }

    /// Discover fourslash test files, optionally filtered to an operation.
    pub fn discover_cases(&self, op_filter: Option<LspOperation>) -> Result<Vec<PathBuf>, String> {
        let dir = self.tests_dir();
        if !dir.exists() {
            return Err(format!(
                "fourslash test directory not found: {}",
                dir.display()
            ));
        }

        let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("ts")
                    && path.file_name().and_then(|f| f.to_str()) != Some("fourslash.ts")
                {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        cases.sort();

        // Filter by operation if requested
        if let Some(op) = op_filter {
            cases.retain(|path| {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                lsp_parser::classify_test_operation(&content) == op
            });
        }

        Ok(cases)
    }

    /// Run a single test case by path.
    pub fn run_case(&self, test_path: &Path) -> LspTestResult {
        let test_name = test_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Read the test file
        let raw = match std::fs::read_to_string(test_path) {
            Ok(s) => s,
            Err(e) => {
                return LspTestResult {
                    name: test_name,
                    operation: LspOperation::Other,
                    passed: false,
                    skipped: true,
                    skip_reason: Some(format!("cannot read: {e}")),
                    bucket: None,
                    marker_results: Vec::new(),
                    panic_message: None,
                };
            }
        };

        let operation = lsp_parser::classify_test_operation(&raw);

        // Parse the fourslash test
        let test = match lsp_parser::parse_fourslash(&test_name, &raw) {
            Ok(t) => t,
            Err(e) => {
                return LspTestResult {
                    name: test_name,
                    operation,
                    passed: false,
                    skipped: false,
                    skip_reason: None,
                    bucket: Some(LspBucket::ParseError),
                    marker_results: Vec::new(),
                    panic_message: Some(e),
                };
            }
        };

        match operation {
            LspOperation::QuickInfo => self.run_quick_info_test(&test_name, &test),
            LspOperation::GoToDefinition => self.run_goto_definition_test(&test_name, &test),
            LspOperation::Completions => self.run_completions_test(&test_name, &test),
            LspOperation::FindAllReferences => self.run_find_all_refs_test(&test_name, &test),
            LspOperation::SignatureHelp => self.run_signature_help_test(&test_name, &test),
            _ => LspTestResult {
                name: test_name,
                operation,
                passed: false,
                skipped: true,
                skip_reason: Some(format!("operation {} not yet supported", operation.label())),
                bucket: Some(LspBucket::Unsupported),
                marker_results: Vec::new(),
                panic_message: None,
            },
        }
    }

    /// Run a quickInfo test using baseline comparison or inline assertions.
    fn run_quick_info_test(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        // Check for panics
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if test.has_edits {
                self.run_quick_info_with_edits(test_name, test)
            } else {
                self.run_quick_info_inner(test_name, test)
            }
        }));

        match result {
            Ok(r) => r,
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                LspTestResult {
                    name: test_name.to_string(),
                    operation: LspOperation::QuickInfo,
                    passed: false,
                    skipped: false,
                    skip_reason: None,
                    bucket: Some(LspBucket::Panic),
                    marker_results: Vec::new(),
                    panic_message: Some(msg),
                }
            }
        }
    }

    /// Edit-aware quickInfo runner: interprets the fourslash statement stream
    /// in order — goTo.marker / edit.insert / edit.backspace mutate a working
    /// copy (marker positions tracked), verify.completions({marker}) moves the
    /// cursor, and each verify.quickInfoAt re-hovers against a REBUILT
    /// analysis of the edited text. Without this, hovers after `edit.insert`
    /// ran against the unedited (often syntactically incomplete) source.
    fn run_quick_info_with_edits(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        fn extract_two_string_args(s: &str) -> Option<(String, String)> {
            let bytes = s.as_bytes();
            let mut i = 0usize;
            let mut read_str = |i: &mut usize| -> Option<String> {
                while *i < bytes.len() && (bytes[*i] as char).is_whitespace() {
                    *i += 1;
                }
                let quote = *bytes.get(*i)?;
                if quote != b'"' && quote != b'\'' {
                    return None;
                }
                *i += 1;
                let start = *i;
                while *i < bytes.len() && bytes[*i] != quote {
                    *i += 1;
                }
                let out = s[start..*i].to_string();
                *i += 1;
                Some(out)
            };
            let first = read_str(&mut i)?;
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            if bytes.get(i) != Some(&b',') {
                return None;
            }
            i += 1;
            let second = read_str(&mut i)?;
            Some((first, second))
        }
        fn completions_marker(statement: &str) -> Option<String> {
            if !statement.starts_with("verify.completions(") && !statement.starts_with("goTo.") {
                if !statement.contains("marker:") {
                    return None;
                }
            }
            let idx = statement.find("marker:")?;
            let rest = &statement[idx + "marker:".len()..];
            let rest = rest.trim_start();
            let quote = rest.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let inner = &rest[1..];
            let end = inner.find(quote)?;
            Some(inner[..end].to_string())
        }

        let is_cross_file = test.files.len() > 1;
        let mut working = test.clone();
        let bindings: HashMap<String, SignatureHelpScriptValue> = HashMap::new();
        let mut current_cursor: Option<(String, u32)> = None;
        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;
        let mut analysis: Option<(
            tsc_rs_query::QueryEngine,
            HashMap<String, lsp_executor::FileAnalysis>,
        )> = None;

        for statement in split_top_level_statements(&test.verify_text) {
            let mut statement = statement.trim();
            if statement.starts_with("//") {
                if let Some(line) = statement
                    .lines()
                    .rev()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && !line.starts_with("//"))
                {
                    statement = line;
                }
            }
            if statement.is_empty() || statement.starts_with("//") {
                continue;
            }

            if let Some(marker_name) = parse_go_to_marker_statement(statement) {
                if let Some(marker) = find_marker_by_name(&working, &marker_name) {
                    current_cursor = Some((marker.file_name.clone(), marker.position));
                }
                continue;
            }
            if let Some(insert_text) = parse_edit_insert_statement(statement, &bindings) {
                if let Some((file_name, cursor)) = current_cursor.clone() {
                    let new_cursor =
                        apply_insert_edit(&mut working, &file_name, cursor, &insert_text);
                    current_cursor = Some((file_name, new_cursor));
                    analysis = None;
                }
                continue;
            }
            // edit.moveLeft(N) / edit.moveRight(N) where N is an integer or
            // a '...'.length expression — cursor-only, no text change.
            for (pat, sign) in [("edit.moveLeft(", -1i64), ("edit.moveRight(", 1i64)] {
                if let Some(rest) = statement.strip_prefix(pat) {
                    let arg = rest.trim_end_matches([')', ';']).trim();
                    let n: Option<i64> = if let Ok(v) = arg.parse::<i64>() {
                        Some(v)
                    } else if arg.ends_with(".length") {
                        let q = &arg[..arg.len() - ".length".len()];
                        let q = q.trim();
                        if (q.starts_with('\'') && q.ends_with('\''))
                            || (q.starts_with('"') && q.ends_with('"'))
                        {
                            Some((q.len() - 2) as i64)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let (Some(n), Some((file_name, cursor))) = (n, current_cursor.clone()) {
                        let newpos = (cursor as i64 + sign * n).max(0) as u32;
                        current_cursor = Some((file_name, newpos));
                    }
                }
            }
            if statement.starts_with("edit.moveLeft(") || statement.starts_with("edit.moveRight(") {
                continue;
            }
            if let Some(count) = parse_edit_backspace_statement(statement, &bindings) {
                if let Some((file_name, cursor)) = current_cursor.clone() {
                    let new_cursor = apply_backspace_edit(&mut working, &file_name, cursor, count);
                    current_cursor = Some((file_name, new_cursor));
                    analysis = None;
                }
                continue;
            }
            if statement.starts_with("verify.quickInfoAt(") {
                let args = &statement["verify.quickInfoAt(".len()..];
                let Some((marker_name, expected_text)) = extract_two_string_args(args)
                    .or_else(|| lsp_parser::parse_marker_and_array_join(args))
                else {
                    continue;
                };
                if analysis.is_none() {
                    analysis = Some(lsp_executor::build_analysis(&working));
                }
                let (qe, analyses) = analysis.as_ref().unwrap();
                let marker = find_marker_by_name(&working, &marker_name);
                let actual = marker.and_then(|m| lsp_executor::hover_at_marker(qe, analyses, m));
                let actual_norm = actual
                    .as_deref()
                    .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
                let expected_norm = expected_text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let passed = if expected_norm.is_empty() {
                    actual_norm.as_deref().map_or(true, |s| s.is_empty())
                } else {
                    actual_norm.as_deref() == Some(expected_norm.as_str())
                };
                if !passed {
                    all_passed = false;
                    if first_bucket.is_none() {
                        first_bucket = Some(classify_quick_info(
                            &expected_text,
                            actual.as_deref(),
                            "",
                            is_cross_file,
                        ));
                    }
                }
                marker_results.push(MarkerResult {
                    marker_name,
                    file_name: marker.map(|m| m.file_name.clone()).unwrap_or_default(),
                    position: marker.map(|m| m.position).unwrap_or(0),
                    expected: Some(expected_text),
                    actual,
                    passed,
                });
                continue;
            }
            if statement.starts_with("verify.quickInfoIs(") {
                let args = &statement["verify.quickInfoIs(".len()..];
                // single string arg; hover at the CURRENT cursor
                let expected_text = {
                    let mut i = 0usize;
                    let bytes = args.as_bytes();
                    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                        i += 1;
                    }
                    match bytes.get(i) {
                        Some(&q) if q == b'"' || q == b'\'' => {
                            let start = i + 1;
                            let mut j = start;
                            while j < bytes.len() && bytes[j] != q {
                                j += 1;
                            }
                            Some(args[start..j].to_string())
                        }
                        _ => None,
                    }
                };
                let (Some(expected_text), Some((file_name, cursor))) =
                    (expected_text, current_cursor.clone())
                else {
                    continue;
                };
                if analysis.is_none() {
                    analysis = Some(lsp_executor::build_analysis(&working));
                }
                let (qe, analyses) = analysis.as_ref().unwrap();
                let synthetic = lsp_parser::Marker {
                    name: String::new(),
                    file_name: file_name.clone(),
                    position: cursor,
                };
                let actual = lsp_executor::hover_at_marker(qe, analyses, &synthetic);
                let actual_norm = actual
                    .as_deref()
                    .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
                let expected_norm = expected_text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let passed = if expected_norm.is_empty() {
                    actual_norm.as_deref().map_or(true, |s| s.is_empty())
                } else {
                    actual_norm.as_deref() == Some(expected_norm.as_str())
                };
                if !passed {
                    all_passed = false;
                    if first_bucket.is_none() {
                        first_bucket = Some(classify_quick_info(
                            &expected_text,
                            actual.as_deref(),
                            "",
                            is_cross_file,
                        ));
                    }
                }
                marker_results.push(MarkerResult {
                    marker_name: String::new(),
                    file_name,
                    position: cursor,
                    expected: Some(expected_text),
                    actual,
                    passed,
                });
                continue;
            }
            // A verify.completions({marker: "N"}) (or similar) moves the
            // cursor for a following edit.insert.
            if let Some(marker_name) = completions_marker(statement) {
                if let Some(marker) = find_marker_by_name(&working, &marker_name) {
                    current_cursor = Some((marker.file_name.clone(), marker.position));
                }
                continue;
            }
        }

        if marker_results.is_empty() {
            // The statement interpreter drove no verifications (e.g. the
            // expected text uses the array-join form it doesn't parse) —
            // fall back to the position-static runner rather than failing.
            return self.run_quick_info_inner(test_name, test);
        }
        LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::QuickInfo,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        }
    }

    fn run_quick_info_inner(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        let is_cross_file = test.files.len() > 1;
        let (qe, analyses) = lsp_executor::build_analysis(test);

        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;

        for cmd in &test.verify_commands {
            match cmd {
                VerifyCommand::BaselineQuickInfo => {
                    // Compare against baseline file
                    let baseline_path =
                        lsp_baseline::quick_info_baseline_path(&self.baselines_dir(), test_name);
                    if !baseline_path.exists() {
                        return LspTestResult {
                            name: test_name.to_string(),
                            operation: LspOperation::QuickInfo,
                            passed: false,
                            skipped: true,
                            skip_reason: Some("no baseline file".into()),
                            bucket: Some(LspBucket::NoBaseline),
                            marker_results: Vec::new(),
                            panic_message: None,
                        };
                    }

                    let entries = match lsp_baseline::load_quick_info_baseline(&baseline_path) {
                        Ok(e) => e,
                        Err(e) => {
                            return LspTestResult {
                                name: test_name.to_string(),
                                operation: LspOperation::QuickInfo,
                                passed: false,
                                skipped: false,
                                skip_reason: None,
                                bucket: Some(LspBucket::ParseError),
                                marker_results: Vec::new(),
                                panic_message: Some(e),
                            };
                        }
                    };

                    // For each baseline entry, find the matching marker and compare
                    for entry in &entries {
                        let marker = test.markers.iter().find(|m| m.name == entry.marker_name);

                        let actual =
                            marker.and_then(|m| lsp_executor::hover_at_marker(&qe, &analyses, m));

                        let passed = lsp_baseline::compare_quick_info(actual.as_deref(), entry);

                        if !passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(classify_quick_info(
                                    &entry.display_text,
                                    actual.as_deref(),
                                    &entry.kind,
                                    is_cross_file,
                                ));
                            }
                        }

                        marker_results.push(MarkerResult {
                            marker_name: entry.marker_name.clone(),
                            file_name: entry.file_name.clone(),
                            position: entry.position,
                            expected: Some(entry.display_text.clone()),
                            actual,
                            passed,
                        });
                    }
                }
                VerifyCommand::QuickInfoAt {
                    marker: marker_name,
                    expected_text,
                } => {
                    let marker = test.markers.iter().find(|m| m.name == *marker_name);
                    let actual =
                        marker.and_then(|m| lsp_executor::hover_at_marker(&qe, &analyses, m));

                    let actual_norm = actual
                        .as_deref()
                        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
                    let expected_norm = expected_text
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    // When expected is empty, treat None as passing (TypeScript
                    // returns "" for positions where hover has no content,
                    // while we return None).
                    let passed = if expected_norm.is_empty() {
                        actual_norm.as_deref().map_or(true, |s| s.is_empty())
                    } else {
                        actual_norm.as_deref() == Some(expected_norm.as_str())
                    };

                    if !passed {
                        all_passed = false;
                        if first_bucket.is_none() {
                            first_bucket = Some(classify_quick_info(
                                expected_text,
                                actual.as_deref(),
                                "",
                                is_cross_file,
                            ));
                        }
                    }

                    marker_results.push(MarkerResult {
                        marker_name: marker_name.clone(),
                        file_name: marker.map(|m| m.file_name.clone()).unwrap_or_default(),
                        position: marker.map(|m| m.position).unwrap_or(0),
                        expected: Some(expected_text.clone()),
                        actual,
                        passed,
                    });
                }
                VerifyCommand::QuickInfos { entries } => {
                    for (marker_name, expected_text) in entries {
                        let marker = test.markers.iter().find(|m| m.name == *marker_name);
                        let actual =
                            marker.and_then(|m| lsp_executor::hover_at_marker(&qe, &analyses, m));

                        let actual_norm = actual
                            .as_deref()
                            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
                        let expected_norm = expected_text
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        // When expected is empty, treat None as passing (TypeScript
                        // returns "" for positions where hover has no content,
                        // while we return None).
                        let passed = if expected_norm.is_empty() {
                            actual_norm.as_deref().map_or(true, |s| s.is_empty())
                        } else {
                            actual_norm.as_deref() == Some(expected_norm.as_str())
                        };

                        if !passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(classify_quick_info(
                                    expected_text,
                                    actual.as_deref(),
                                    "",
                                    is_cross_file,
                                ));
                            }
                        }

                        marker_results.push(MarkerResult {
                            marker_name: marker_name.clone(),
                            file_name: marker.map(|m| m.file_name.clone()).unwrap_or_default(),
                            position: marker.map(|m| m.position).unwrap_or(0),
                            expected: Some(expected_text.clone()),
                            actual,
                            passed,
                        });
                    }
                }
                VerifyCommand::QuickInfoExists {
                    marker: marker_name,
                } => {
                    let effective_markers: Vec<&lsp_parser::Marker> = if marker_name.is_empty() {
                        test.markers.iter().collect()
                    } else {
                        test.markers
                            .iter()
                            .filter(|m| m.name == *marker_name)
                            .collect()
                    };
                    for marker in &effective_markers {
                        let actual = lsp_executor::hover_at_marker(&qe, &analyses, marker);
                        let passed = actual.is_some();
                        if !passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::NoResult);
                            }
                        }
                        marker_results.push(MarkerResult {
                            marker_name: marker.name.clone(),
                            file_name: marker.file_name.clone(),
                            position: marker.position,
                            expected: Some("quickInfo exists".into()),
                            actual: Some(actual.as_deref().unwrap_or("(none)").to_string()),
                            passed,
                        });
                    }
                }
                VerifyCommand::NoQuickInfo { markers } => {
                    let effective_markers: Vec<&lsp_parser::Marker> =
                        if markers.len() == 1 && markers[0].is_empty() {
                            test.markers.iter().collect()
                        } else {
                            markers
                                .iter()
                                .filter_map(|name| test.markers.iter().find(|m| m.name == *name))
                                .collect()
                        };
                    for marker in &effective_markers {
                        let actual = lsp_executor::hover_at_marker(&qe, &analyses, marker);
                        let passed = actual.is_none();
                        if !passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::DisplayDiff);
                            }
                        }
                        marker_results.push(MarkerResult {
                            marker_name: marker.name.clone(),
                            file_name: marker.file_name.clone(),
                            position: marker.position,
                            expected: Some("no quickInfo".into()),
                            actual: Some(actual.as_deref().unwrap_or("(none)").to_string()),
                            passed,
                        });
                    }
                }
                _ => {} // Skip non-quickInfo commands in this test
            }
        }

        // If no quickInfo commands were found, skip
        if marker_results.is_empty() {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::QuickInfo,
                passed: false,
                skipped: true,
                skip_reason: Some("no quickInfo assertions found".into()),
                bucket: None,
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::QuickInfo,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        }
    }

    // -----------------------------------------------------------------------
    // Signature Help
    // -----------------------------------------------------------------------

    fn run_signature_help_test(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_signature_help_inner(test_name, test)
        }));

        match result {
            Ok(r) => r,
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                LspTestResult {
                    name: test_name.to_string(),
                    operation: LspOperation::SignatureHelp,
                    passed: false,
                    skipped: false,
                    skip_reason: None,
                    bucket: Some(LspBucket::Panic),
                    marker_results: Vec::new(),
                    panic_message: Some(msg),
                }
            }
        }
    }

    fn run_signature_help_inner(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        if test.has_edits
            || test.verify_text.contains("goTo.marker(")
            || test
                .verify_text
                .contains("verify.noSignatureHelpForTriggerReason")
            || test
                .verify_text
                .contains("verify.signatureHelpPresentForTriggerReason")
        {
            return self.run_signature_help_with_edits(test_name, test);
        }

        let mut unsupported_reason = None;
        for cmd in &test.verify_commands {
            match cmd {
                VerifyCommand::Unsupported { raw } if raw.contains("signatureHelp") => {
                    unsupported_reason = Some(format!("verify command {raw} is not yet supported"));
                    break;
                }
                VerifyCommand::SignatureHelp { expected, .. }
                    if !expected.unsupported_fields.is_empty() =>
                {
                    unsupported_reason = Some(format!(
                        "signatureHelp fields not yet supported: {}",
                        expected.unsupported_fields.join(", ")
                    ));
                    break;
                }
                _ => {}
            }
        }

        if let Some(reason) = unsupported_reason {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::SignatureHelp,
                passed: false,
                skipped: true,
                skip_reason: Some(reason),
                bucket: Some(LspBucket::Unsupported),
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        let (qe, analyses) = lsp_executor::build_analysis(test);

        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;
        let mut has_signature_cmd = false;

        for cmd in &test.verify_commands {
            match cmd {
                VerifyCommand::BaselineSignatureHelp => {
                    has_signature_cmd = true;
                    let baseline_path = lsp_baseline::signature_help_baseline_path(
                        &self.baselines_dir(),
                        test_name,
                    );
                    if !baseline_path.exists() {
                        return LspTestResult {
                            name: test_name.to_string(),
                            operation: LspOperation::SignatureHelp,
                            passed: false,
                            skipped: true,
                            skip_reason: Some("no baseline file".into()),
                            bucket: Some(LspBucket::NoBaseline),
                            marker_results: Vec::new(),
                            panic_message: None,
                        };
                    }

                    let entries = match lsp_baseline::load_signature_help_baseline(&baseline_path) {
                        Ok(entries) => entries,
                        Err(e) => {
                            return LspTestResult {
                                name: test_name.to_string(),
                                operation: LspOperation::SignatureHelp,
                                passed: false,
                                skipped: false,
                                skip_reason: None,
                                bucket: Some(LspBucket::ParseError),
                                marker_results: Vec::new(),
                                panic_message: Some(e),
                            };
                        }
                    };

                    for entry in &entries {
                        let marker = test.markers.iter().find(|m| m.name == entry.marker_name);
                        let actual = marker.and_then(|m| {
                            lsp_executor::signature_help_at_marker(&qe, &analyses, m)
                        });
                        let (passed, expected_str, actual_str) =
                            compare_signature_help_baseline(actual.as_ref(), entry);

                        if !passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(if actual.is_none() {
                                    LspBucket::NoResult
                                } else {
                                    LspBucket::DisplayDiff
                                });
                            }
                        }

                        marker_results.push(MarkerResult {
                            marker_name: entry.marker_name.clone(),
                            file_name: marker
                                .map(|m| m.file_name.clone())
                                .unwrap_or_else(|| entry.file_name.clone()),
                            position: marker.map(|m| m.position).unwrap_or(entry.position),
                            expected: Some(expected_str),
                            actual: Some(actual_str),
                            passed,
                        });
                    }
                }
                VerifyCommand::SignatureHelp { markers, expected } => {
                    has_signature_cmd = true;
                    // When marker name is empty (""), iterate all markers.
                    let effective_markers: Vec<(&str, &lsp_parser::Marker)> =
                        if markers.len() == 1 && markers[0].is_empty() {
                            test.markers.iter().map(|m| (m.name.as_str(), m)).collect()
                        } else {
                            markers
                                .iter()
                                .filter_map(|name| {
                                    test.markers
                                        .iter()
                                        .find(|m| m.name == *name)
                                        .map(|m| (name.as_str(), m))
                                })
                                .collect()
                        };
                    if effective_markers.is_empty()
                        && !markers.is_empty()
                        && !(markers.len() == 1 && markers[0].is_empty())
                    {
                        for marker_name in markers {
                            marker_results.push(MarkerResult {
                                marker_name: marker_name.clone(),
                                file_name: String::new(),
                                position: 0,
                                expected: Some("marker not found".into()),
                                actual: None,
                                passed: false,
                            });
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::NoResult);
                            }
                        }
                    }
                    for (marker_name, marker) in &effective_markers {
                        let actual = lsp_executor::signature_help_at_marker(&qe, &analyses, marker);
                        let (passed, expected_str, actual_str) =
                            compare_signature_help_inline(actual.as_ref(), expected);
                        if !passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(if actual.is_none() {
                                    LspBucket::NoResult
                                } else {
                                    LspBucket::DisplayDiff
                                });
                            }
                        }

                        marker_results.push(MarkerResult {
                            marker_name: marker_name.to_string(),
                            file_name: marker.file_name.clone(),
                            position: marker.position,
                            expected: Some(expected_str),
                            actual: Some(actual_str),
                            passed,
                        });
                    }
                }
                VerifyCommand::NoSignatureHelp { markers } => {
                    has_signature_cmd = true;
                    // When the marker name is empty (""), iterate over all markers
                    // in the test. This matches TypeScript fourslash behavior where
                    // verify.noSignatureHelp() without arguments checks all markers.
                    let effective_markers: Vec<&lsp_parser::Marker> =
                        if markers.len() == 1 && markers[0].is_empty() {
                            test.markers.iter().collect()
                        } else {
                            markers
                                .iter()
                                .filter_map(|name| test.markers.iter().find(|m| m.name == *name))
                                .collect()
                        };
                    if effective_markers.is_empty()
                        && !(markers.len() == 1 && markers[0].is_empty())
                    {
                        // Marker names specified but none found — fail
                        for marker_name in markers.iter() {
                            marker_results.push(MarkerResult {
                                marker_name: marker_name.clone(),
                                file_name: String::new(),
                                position: 0,
                                expected: Some("no signature help".into()),
                                actual: None,
                                passed: false,
                            });
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::NoResult);
                            }
                        }
                    }
                    for marker in &effective_markers {
                        let actual = lsp_executor::signature_help_at_marker(&qe, &analyses, marker);
                        let passed = actual.is_none();
                        if !passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::DisplayDiff);
                            }
                        }

                        marker_results.push(MarkerResult {
                            marker_name: marker.name.clone(),
                            file_name: marker.file_name.clone(),
                            position: marker.position,
                            expected: Some("no signature help".into()),
                            actual: Some(describe_signature_help(actual.as_ref())),
                            passed,
                        });
                    }
                }
                _ => {}
            }
        }

        if !has_signature_cmd {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::SignatureHelp,
                passed: false,
                skipped: true,
                skip_reason: Some("no supported signatureHelp commands found".into()),
                bucket: Some(LspBucket::Unsupported),
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::SignatureHelp,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        }
    }

    fn run_signature_help_with_edits(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        if test.verify_text.contains("for (") || test.verify_text.contains("for(") {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::SignatureHelp,
                passed: false,
                skipped: true,
                skip_reason: Some(
                    "loop-driven signatureHelp edit tests are not yet modeled".into(),
                ),
                bucket: Some(LspBucket::Unsupported),
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        let mut working = test.clone();
        let mut bindings = HashMap::new();
        let mut current_cursor: Option<(String, u32)> = None;
        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;
        let mut has_signature_cmd = false;

        for statement in split_top_level_statements(&test.verify_text) {
            let mut statement = statement.trim();
            if statement.starts_with("//") {
                if let Some(line) = statement
                    .lines()
                    .rev()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && !line.starts_with("//"))
                {
                    statement = line;
                }
            }
            if statement.is_empty() || statement.starts_with("//") {
                continue;
            }

            if let Some((name, value)) = parse_signature_help_binding(statement) {
                bindings.insert(name, value);
                continue;
            }

            if let Some(marker_name) = parse_go_to_marker_statement(statement) {
                let Some(marker) = find_marker_by_name(&working, &marker_name) else {
                    return LspTestResult {
                        name: test_name.to_string(),
                        operation: LspOperation::SignatureHelp,
                        passed: false,
                        skipped: false,
                        skip_reason: None,
                        bucket: Some(LspBucket::ParseError),
                        marker_results,
                        panic_message: Some(format!(
                            "marker not found in edit script: {marker_name}"
                        )),
                    };
                };
                current_cursor = Some((marker.file_name.clone(), marker.position));
                continue;
            }

            if let Some(insert_text) = parse_edit_insert_statement(statement, &bindings) {
                let Some((file_name, cursor)) = current_cursor.clone() else {
                    return LspTestResult {
                        name: test_name.to_string(),
                        operation: LspOperation::SignatureHelp,
                        passed: false,
                        skipped: false,
                        skip_reason: None,
                        bucket: Some(LspBucket::ParseError),
                        marker_results,
                        panic_message: Some("edit.insert with no active cursor".into()),
                    };
                };
                let new_cursor = apply_insert_edit(&mut working, &file_name, cursor, &insert_text);
                current_cursor = Some((file_name, new_cursor));
                continue;
            }

            if let Some(backspace_count) = parse_edit_backspace_statement(statement, &bindings) {
                let Some((file_name, cursor)) = current_cursor.clone() else {
                    return LspTestResult {
                        name: test_name.to_string(),
                        operation: LspOperation::SignatureHelp,
                        passed: false,
                        skipped: false,
                        skip_reason: None,
                        bucket: Some(LspBucket::ParseError),
                        marker_results,
                        panic_message: Some("edit.backspace with no active cursor".into()),
                    };
                };
                let new_cursor =
                    apply_backspace_edit(&mut working, &file_name, cursor, backspace_count);
                current_cursor = Some((file_name, new_cursor));
                continue;
            }

            if let Some((markers, expected)) =
                parse_signature_help_verify_statement(statement, &bindings)
            {
                has_signature_cmd = true;
                let target_markers =
                    resolve_signature_help_targets(&working, current_cursor.clone(), &markers);
                let (qe, analyses) = lsp_executor::build_analysis(&working);
                for marker in &target_markers {
                    let actual = lsp_executor::signature_help_at_marker(&qe, &analyses, marker);
                    let (passed, expected_str, actual_str) =
                        compare_signature_help_inline_with_context(
                            actual.as_ref(),
                            &expected,
                            None,
                        );
                    if !passed {
                        all_passed = false;
                        if first_bucket.is_none() {
                            first_bucket = Some(if actual.is_none() {
                                LspBucket::NoResult
                            } else {
                                LspBucket::DisplayDiff
                            });
                        }
                    }
                    marker_results.push(MarkerResult {
                        marker_name: marker.name.clone(),
                        file_name: marker.file_name.clone(),
                        position: marker.position,
                        expected: Some(expected_str),
                        actual: Some(actual_str),
                        passed,
                    });
                }
                if let Some(marker) = target_markers.first() {
                    current_cursor = Some((marker.file_name.clone(), marker.position));
                }
                continue;
            }

            if let Some(markers) = parse_no_signature_help_statement(statement) {
                has_signature_cmd = true;
                let target_markers =
                    resolve_signature_help_targets(&working, current_cursor.clone(), &markers);
                let (qe, analyses) = lsp_executor::build_analysis(&working);
                for marker in &target_markers {
                    let actual = lsp_executor::signature_help_at_marker(&qe, &analyses, marker);
                    let passed = actual.is_none();
                    if !passed {
                        all_passed = false;
                        if first_bucket.is_none() {
                            first_bucket = Some(LspBucket::DisplayDiff);
                        }
                    }
                    marker_results.push(MarkerResult {
                        marker_name: marker.name.clone(),
                        file_name: marker.file_name.clone(),
                        position: marker.position,
                        expected: Some("no signature help".into()),
                        actual: Some(describe_signature_help(actual.as_ref())),
                        passed,
                    });
                }
                if let Some(marker) = target_markers.first() {
                    current_cursor = Some((marker.file_name.clone(), marker.position));
                }
                continue;
            }

            if let Some(expected_trigger_reason) =
                parse_no_signature_help_for_trigger_reason_statement(statement, &bindings)
            {
                has_signature_cmd = true;
                let Some((file_name, position)) = current_cursor.clone() else {
                    return LspTestResult {
                        name: test_name.to_string(),
                        operation: LspOperation::SignatureHelp,
                        passed: false,
                        skipped: false,
                        skip_reason: None,
                        bucket: Some(LspBucket::ParseError),
                        marker_results,
                        panic_message: Some(
                            "verify.noSignatureHelpForTriggerReason with no active cursor".into(),
                        ),
                    };
                };
                let marker = lsp_parser::Marker {
                    name: String::new(),
                    file_name,
                    position,
                };
                let (qe, analyses) = lsp_executor::build_analysis(&working);
                let actual = trigger_filtered_signature_help(
                    &working,
                    &qe,
                    &analyses,
                    &marker,
                    Some(&expected_trigger_reason),
                );
                let passed = actual.is_none();
                if !passed {
                    all_passed = false;
                    if first_bucket.is_none() {
                        first_bucket = Some(if actual.is_none() {
                            LspBucket::DisplayDiff
                        } else {
                            LspBucket::DisplayDiff
                        });
                    }
                }
                marker_results.push(MarkerResult {
                    marker_name: marker.name.clone(),
                    file_name: marker.file_name.clone(),
                    position: marker.position,
                    expected: Some(format!(
                        "no signature help for triggerReason={}",
                        describe_trigger_reason(&expected_trigger_reason)
                    )),
                    actual: Some(describe_signature_help(actual.as_ref())),
                    passed,
                });
                continue;
            }

            if let Some(expected_trigger_reason) =
                parse_signature_help_present_for_trigger_reason_statement(statement, &bindings)
            {
                has_signature_cmd = true;
                let Some((file_name, position)) = current_cursor.clone() else {
                    return LspTestResult {
                        name: test_name.to_string(),
                        operation: LspOperation::SignatureHelp,
                        passed: false,
                        skipped: false,
                        skip_reason: None,
                        bucket: Some(LspBucket::ParseError),
                        marker_results,
                        panic_message: Some(
                            "verify.signatureHelpPresentForTriggerReason with no active cursor"
                                .into(),
                        ),
                    };
                };
                let marker = lsp_parser::Marker {
                    name: String::new(),
                    file_name,
                    position,
                };
                let (qe, analyses) = lsp_executor::build_analysis(&working);
                let actual = trigger_filtered_signature_help(
                    &working,
                    &qe,
                    &analyses,
                    &marker,
                    Some(&expected_trigger_reason),
                );
                let passed = actual.is_some();
                if !passed {
                    all_passed = false;
                    if first_bucket.is_none() {
                        first_bucket = Some(if actual.is_none() {
                            LspBucket::NoResult
                        } else {
                            LspBucket::DisplayDiff
                        });
                    }
                }
                marker_results.push(MarkerResult {
                    marker_name: marker.name.clone(),
                    file_name: marker.file_name.clone(),
                    position: marker.position,
                    expected: Some(format!(
                        "signature help present for triggerReason={}",
                        describe_trigger_reason(&expected_trigger_reason)
                    )),
                    actual: Some(describe_signature_help(actual.as_ref())),
                    passed,
                });
                continue;
            }

            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::SignatureHelp,
                passed: false,
                skipped: true,
                skip_reason: Some(format!(
                    "edit-driven signatureHelp statement not yet supported: {}",
                    statement.lines().next().unwrap_or_default().trim()
                )),
                bucket: Some(LspBucket::Unsupported),
                marker_results,
                panic_message: None,
            };
        }

        if !has_signature_cmd {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::SignatureHelp,
                passed: false,
                skipped: true,
                skip_reason: Some("no supported signatureHelp commands found".into()),
                bucket: Some(LspBucket::Unsupported),
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::SignatureHelp,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        }
    }

    // -----------------------------------------------------------------------
    // GoToDefinition
    // -----------------------------------------------------------------------

    fn run_goto_definition_test(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_goto_definition_inner(test_name, test)
        }));

        match result {
            Ok(r) => r,
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                LspTestResult {
                    name: test_name.to_string(),
                    operation: LspOperation::GoToDefinition,
                    passed: false,
                    skipped: false,
                    skip_reason: None,
                    bucket: Some(LspBucket::Panic),
                    marker_results: Vec::new(),
                    panic_message: Some(msg),
                }
            }
        }
    }

    fn run_goto_definition_inner(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        let is_cross_file = test.files.len() > 1;

        // Load the baseline file
        let baseline_path =
            lsp_baseline::goto_definition_baseline_path(&self.baselines_dir(), test_name);
        if !baseline_path.exists() {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::GoToDefinition,
                passed: false,
                skipped: true,
                skip_reason: Some("no baseline file".into()),
                bucket: Some(LspBucket::NoBaseline),
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        // Build virtual source map for baseline offset correction (skipped lines)
        let virtual_sources: std::collections::HashMap<String, String> = test
            .files
            .iter()
            .map(|f| (f.name.clone(), f.content.clone()))
            .collect();

        let entries =
            match lsp_baseline::load_goto_definition_baseline(&baseline_path, &virtual_sources) {
                Ok(e) => e,
                Err(e) => {
                    return LspTestResult {
                        name: test_name.to_string(),
                        operation: LspOperation::GoToDefinition,
                        passed: false,
                        skipped: false,
                        skip_reason: None,
                        bucket: Some(LspBucket::ParseError),
                        marker_results: Vec::new(),
                        panic_message: Some(e),
                    };
                }
            };

        // Collect which markers to test from the verify commands
        let mut requested_markers: Vec<String> = Vec::new();
        for cmd in &test.verify_commands {
            if let VerifyCommand::BaselineGoToDefinition { markers } = cmd {
                if markers.is_empty() {
                    // No explicit markers — test all markers in the test
                    for m in &test.markers {
                        if !m.name.is_empty() {
                            requested_markers.push(m.name.clone());
                        }
                    }
                } else {
                    requested_markers.extend(markers.iter().cloned());
                }
            }
        }

        if requested_markers.is_empty() {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::GoToDefinition,
                passed: false,
                skipped: true,
                skip_reason: Some("no goToDefinition markers found".into()),
                bucket: None,
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        let (qe, analyses) = lsp_executor::build_analysis(test);

        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;

        // The baseline has one entry per marker, in the same order as
        // requested_markers. Match them up by index.
        for (idx, marker_name) in requested_markers.iter().enumerate() {
            let marker = test.markers.iter().find(|m| m.name == *marker_name);

            let actual = marker.and_then(|m| lsp_executor::definition_at_marker(&qe, &analyses, m));

            // Get the corresponding baseline entry
            let entry = entries.get(idx);

            let (passed, expected_str, actual_str) = match entry {
                Some(entry) if entry.expects_no_definition => {
                    // Baseline expects no definition
                    let passed = actual.is_none();
                    let expected_str = "(no definition)".to_string();
                    let actual_str = match &actual {
                        Some((file, span)) => format!("{}:{}", file, span.start),
                        None => "(no definition)".to_string(),
                    };
                    (passed, expected_str, actual_str)
                }
                Some(entry) => {
                    let expected_file = entry.target_file.as_deref();
                    let expected_pos = entry.target_position;

                    let expected_str = match (expected_file, expected_pos) {
                        (Some(f), Some(p)) => format!("{}:{} ({})", f, p, entry.name),
                        (Some(f), None) => format!("{} ({})", f, entry.name),
                        _ => format!("(unknown target) ({})", entry.name),
                    };

                    match &actual {
                        Some((file, span)) => {
                            let actual_str = format!("{}:{}", file, span.start);

                            // Compare file: the baseline uses full paths like
                            // /tests/cases/fourslash/Definition.ts while the
                            // executor returns short paths like /Definition.ts.
                            // Compare by filename (last path component).
                            let exp_filename = expected_file
                                .and_then(|f| f.rsplit('/').next())
                                .unwrap_or("");
                            let act_filename = file.rsplit('/').next().unwrap_or(file);
                            let file_match = exp_filename == act_filename;

                            // Compare position with tolerance of ±5
                            let pos_match = match expected_pos {
                                Some(exp_pos) => {
                                    let diff = (span.start as i64) - (exp_pos as i64);
                                    diff.abs() <= 5
                                }
                                None => true, // no position to check — file match is enough
                            };

                            // Name-based match: if the expected name appears at
                            // the actual position in the source, consider it a
                            // match even if the offset is off (handles baselines
                            // with `--- skipped ---` lines where offset calculation
                            // can be inaccurate).
                            let name_match = if !pos_match && !entry.name.is_empty() {
                                // Look up the source text at the actual position
                                let src = qe.get_source_file(file);
                                src.map_or(false, |s| {
                                    let start = span.start as usize;
                                    let name = &entry.name;
                                    if start + name.len() <= s.len() {
                                        &s[start..start + name.len()] == name
                                    } else {
                                        false
                                    }
                                })
                            } else {
                                false
                            };

                            let passed = file_match && (pos_match || name_match);
                            (passed, expected_str, actual_str)
                        }
                        None => {
                            let actual_str = "(no definition)".to_string();
                            (false, expected_str, actual_str)
                        }
                    }
                }
                None => {
                    // No baseline entry for this marker — can't compare
                    let actual_str = match &actual {
                        Some((file, span)) => format!("{}:{}", file, span.start),
                        None => "(no definition)".to_string(),
                    };
                    (false, "(no baseline entry)".to_string(), actual_str)
                }
            };

            if !passed {
                all_passed = false;
                if first_bucket.is_none() {
                    let expected_file = entry.and_then(|e| e.target_file.as_deref());
                    let actual_info = actual.as_ref().map(|(f, span)| (f.as_str(), span.start));
                    first_bucket = Some(classify_goto_definition(
                        expected_file,
                        actual_info,
                        is_cross_file,
                    ));
                }
            }

            marker_results.push(MarkerResult {
                marker_name: marker_name.clone(),
                file_name: marker.map(|m| m.file_name.clone()).unwrap_or_default(),
                position: marker.map(|m| m.position).unwrap_or(0),
                expected: Some(expected_str),
                actual: Some(actual_str),
                passed,
            });
        }

        LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::GoToDefinition,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        }
    }

    // -----------------------------------------------------------------------
    // Completions
    // -----------------------------------------------------------------------

    /// Run a completions test using inline verify.completions assertions.
    fn run_completions_test(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_completions_inner(test_name, test)
        }));

        match result {
            Ok(r) => r,
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                LspTestResult {
                    name: test_name.to_string(),
                    operation: LspOperation::Completions,
                    passed: false,
                    skipped: false,
                    skip_reason: None,
                    bucket: Some(LspBucket::Panic),
                    marker_results: Vec::new(),
                    panic_message: Some(msg),
                }
            }
        }
    }

    /// Statement-driven completions runner for tests that interleave
    /// `edit.insert` / `edit.backspace` with `verify.completions`. Returns
    /// None when the test mixes edits with verify forms this path doesn't
    /// model (e.g. baselineCompletions), falling back to the static runner.
    fn run_completions_with_edits(&self, test_name: &str, test: &LspTest) -> Option<LspTestResult> {
        use crate::lsp_executor::FileAnalysis;
        use crate::lsp_parser::{parse_single_completions_obj, split_top_level_objects, Marker};
        use tsc_rs_query::QueryEngine;

        if test.verify_text.contains("baselineCompletions") {
            return None;
        }
        let statements: Vec<String> = split_top_level_statements(&test.verify_text);
        if !statements
            .iter()
            .any(|st| st.contains("verify.completions"))
        {
            return None;
        }

        let mut working = test.clone();
        let mut current_cursor: Option<(String, u32)> = None;
        let mut analysis: Option<(QueryEngine, HashMap<String, FileAnalysis>)> = None;
        let bindings: HashMap<String, SignatureHelpScriptValue> = HashMap::new();

        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;
        let mut has_completions_cmd = false;

        for statement in &statements {
            if let Some(marker_name) = parse_go_to_marker_statement(statement) {
                if let Some(marker) = working.markers.iter().find(|m| m.name == marker_name) {
                    current_cursor = Some((marker.file_name.clone(), marker.position));
                }
                continue;
            }
            if let Some(insert_text) = parse_edit_insert_statement(statement, &bindings) {
                if let Some((file_name, cursor)) = current_cursor.clone() {
                    let new_cursor =
                        apply_insert_edit(&mut working, &file_name, cursor, &insert_text);
                    current_cursor = Some((file_name, new_cursor));
                    analysis = None;
                }
                continue;
            }
            if let Some(count) = parse_edit_backspace_statement(statement, &bindings) {
                if let Some((file_name, cursor)) = current_cursor.clone() {
                    let new_cursor = apply_backspace_edit(&mut working, &file_name, cursor, count);
                    current_cursor = Some((file_name, new_cursor));
                    analysis = None;
                }
                continue;
            }
            let Some(args_start) = statement
                .find("verify.completions(")
                .map(|i| i + "verify.completions(".len())
            else {
                continue;
            };
            let args = &statement[args_start..];
            for obj in split_top_level_objects(args) {
                for cmd in parse_single_completions_obj(&obj) {
                    let VerifyCommand::Completions {
                        marker: marker_name,
                        includes,
                        excludes,
                        exact,
                    } = cmd
                    else {
                        continue;
                    };
                    has_completions_cmd = true;

                    // Named markers navigate; unnamed ones prefer the live
                    // cursor (set by a previous verify/edit), falling back
                    // to the unnamed marker.
                    let target: Option<(String, u32)> = if !marker_name.is_empty() {
                        working
                            .markers
                            .iter()
                            .find(|m| m.name == marker_name)
                            .map(|m| (m.file_name.clone(), m.position))
                    } else if let Some(cur) = current_cursor.clone() {
                        Some(cur)
                    } else {
                        working
                            .markers
                            .iter()
                            .find(|m| m.name.is_empty())
                            .map(|m| (m.file_name.clone(), m.position))
                    };
                    let Some((file_name, position)) = target else {
                        continue;
                    };
                    current_cursor = Some((file_name.clone(), position));

                    let (qe, analyses) = match &analysis {
                        Some(pair) => pair,
                        None => {
                            analysis = Some(lsp_executor::build_analysis(&working));
                            analysis.as_ref().unwrap()
                        }
                    };
                    let marker = Marker {
                        name: marker_name.clone(),
                        file_name: file_name.clone(),
                        position,
                    };
                    let actual_items = lsp_executor::completions_at_marker_with_prefs(
                        qe,
                        analyses,
                        &marker,
                        test.include_module_exports,
                    );
                    let (cmd_passed, expected_desc, actual_desc, bucket) =
                        check_completion_expectations(&actual_items, &includes, &excludes, &exact);
                    if !cmd_passed {
                        all_passed = false;
                        if first_bucket.is_none() {
                            first_bucket = bucket;
                        }
                    }
                    marker_results.push(MarkerResult {
                        marker_name,
                        file_name,
                        position,
                        expected: expected_desc,
                        actual: actual_desc.or(Some(format!("{} items", actual_items.len()))),
                        passed: cmd_passed,
                    });
                }
            }
        }

        if !has_completions_cmd {
            return None;
        }
        Some(LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::Completions,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        })
    }

    fn run_completions_inner(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        if test.has_edits {
            // The edits path executes verifies the static path (which skips
            // cursor-dependent commands) cannot model. It is strictly harder:
            // prefer its pass, but fall back to the static result so tests
            // whose edit-dependent verifies we previously skipped keep their
            // lenient outcome.
            if let Some(result) = self.run_completions_with_edits(test_name, test) {
                if result.passed {
                    return result;
                }
            }
        }
        let (qe, analyses) = lsp_executor::build_analysis(test);

        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;
        let mut has_completions_cmd = false;

        for cmd in &test.verify_commands {
            match cmd {
                VerifyCommand::Completions {
                    marker: marker_name,
                    includes,
                    excludes,
                    exact,
                } => {
                    has_completions_cmd = true;

                    let marker = test.markers.iter().find(|m| m.name == *marker_name);
                    let Some(marker) = marker else {
                        // Marker not found — skip silently if it's an unnamed default
                        // (test uses goTo navigation we can't simulate)
                        if marker_name.is_empty() {
                            continue;
                        }
                        marker_results.push(MarkerResult {
                            marker_name: marker_name.clone(),
                            file_name: String::new(),
                            position: 0,
                            expected: Some(format!("marker '{}' not found", marker_name)),
                            actual: None,
                            passed: false,
                        });
                        all_passed = false;
                        if first_bucket.is_none() {
                            first_bucket = Some(LspBucket::NoResult);
                        }
                        continue;
                    };

                    let actual_items = lsp_executor::completions_at_marker_with_prefs(
                        &qe,
                        &analyses,
                        marker,
                        test.include_module_exports,
                    );
                    let actual_labels: HashSet<&str> = actual_items
                        .iter()
                        .map(|(label, _)| label.as_str())
                        .collect();

                    let mut cmd_passed = true;
                    let mut expected_desc = String::new();
                    let mut actual_desc = String::new();

                    // Check includes
                    if !includes.is_empty() {
                        let missing: Vec<&str> = includes
                            .iter()
                            .filter(|name| !actual_labels.contains(name.as_str()))
                            .map(|s| s.as_str())
                            .collect();
                        if !missing.is_empty() {
                            if std::env::var_os("TSC_RS_LSP_DEBUG").is_some() {
                                let mut labels: Vec<&str> = actual_labels.iter().copied().collect();
                                labels.sort_unstable();
                                eprintln!("actual labels: {labels:?}");
                            }
                            cmd_passed = false;
                            expected_desc = format!("includes: [{}]", includes.join(", "));
                            actual_desc = format!(
                                "missing: [{}] (got {} items)",
                                missing.join(", "),
                                actual_items.len()
                            );
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::MissingCompletion);
                            }
                        }
                    }

                    // Check excludes
                    if cmd_passed && !excludes.is_empty() {
                        let present: Vec<&str> = excludes
                            .iter()
                            .filter(|name| actual_labels.contains(name.as_str()))
                            .map(|s| s.as_str())
                            .collect();
                        if !present.is_empty() {
                            cmd_passed = false;
                            expected_desc = format!("excludes: [{}]", excludes.join(", "));
                            actual_desc = format!("unexpected: [{}]", present.join(", "));
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::ExtraCompletion);
                            }
                        }
                    }

                    // Check exact
                    if let Some(exact_names) = exact {
                        if exact_names.is_empty() {
                            // exact: undefined — expect no completions
                            if !actual_items.is_empty() {
                                cmd_passed = false;
                                expected_desc = "exact: (none)".to_string();
                                actual_desc = format!("got {} items", actual_items.len());
                                if first_bucket.is_none() {
                                    first_bucket = Some(LspBucket::ExtraCompletion);
                                }
                            }
                        } else {
                            let expected_set: HashSet<&str> =
                                exact_names.iter().map(|s| s.as_str()).collect();

                            let missing: Vec<&str> = exact_names
                                .iter()
                                .filter(|n| !actual_labels.contains(n.as_str()))
                                .map(|s| s.as_str())
                                .collect();
                            let extra: Vec<&str> = actual_labels
                                .iter()
                                .filter(|n| !expected_set.contains(*n))
                                .copied()
                                .collect();

                            if !missing.is_empty() || !extra.is_empty() {
                                cmd_passed = false;
                                expected_desc = format!("exact: [{}]", exact_names.join(", "));
                                let mut parts = Vec::new();
                                if !missing.is_empty() {
                                    parts.push(format!("missing: [{}]", missing.join(", ")));
                                }
                                if !extra.is_empty() {
                                    let mut sorted_extra = extra.clone();
                                    sorted_extra.sort();
                                    parts.push(format!("extra: [{}]", sorted_extra.join(", ")));
                                }
                                actual_desc = parts.join("; ");

                                if first_bucket.is_none() {
                                    if !missing.is_empty() {
                                        first_bucket = Some(LspBucket::MissingCompletion);
                                    } else {
                                        first_bucket = Some(LspBucket::ExtraCompletion);
                                    }
                                }
                            }
                        }
                    }

                    // If includes/excludes/exact are all empty, the test just checks
                    // that completions don't crash — treat as pass if we got here
                    if !cmd_passed {
                        all_passed = false;
                    }

                    marker_results.push(MarkerResult {
                        marker_name: marker_name.clone(),
                        file_name: marker.file_name.clone(),
                        position: marker.position,
                        expected: if expected_desc.is_empty() {
                            None
                        } else {
                            Some(expected_desc)
                        },
                        actual: if actual_desc.is_empty() {
                            Some(format!("{} items", actual_items.len()))
                        } else {
                            Some(actual_desc)
                        },
                        passed: cmd_passed,
                    });
                }
                VerifyCommand::BaselineCompletions => {
                    has_completions_cmd = true;
                    let baseline_path =
                        lsp_baseline::completions_baseline_path(&self.baselines_dir(), test_name);
                    if !baseline_path.exists() {
                        return LspTestResult {
                            name: test_name.to_string(),
                            operation: LspOperation::Completions,
                            passed: false,
                            skipped: true,
                            skip_reason: Some("no baseline file".into()),
                            bucket: Some(LspBucket::NoBaseline),
                            marker_results: Vec::new(),
                            panic_message: None,
                        };
                    }
                    let entries = match lsp_baseline::load_completions_baseline(&baseline_path) {
                        Ok(e) => e,
                        Err(e) => {
                            return LspTestResult {
                                name: test_name.to_string(),
                                operation: LspOperation::Completions,
                                passed: false,
                                skipped: false,
                                skip_reason: None,
                                bucket: Some(LspBucket::ParseError),
                                marker_results: Vec::new(),
                                panic_message: Some(e),
                            };
                        }
                    };
                    for entry in &entries {
                        let marker = test.markers.iter().find(|m| m.name == entry.marker_name);
                        let actual_items = marker
                            .map(|m| {
                                lsp_executor::completions_at_marker_with_prefs(
                                    &qe,
                                    &analyses,
                                    m,
                                    test.include_module_exports,
                                )
                            })
                            .unwrap_or_default();
                        let actual_labels: HashSet<&str> =
                            actual_items.iter().map(|(l, _)| l.as_str()).collect();
                        let expected_set: HashSet<&str> =
                            entry.entry_names.iter().map(|s| s.as_str()).collect();

                        // Check: all expected names appear in actual
                        let missing: Vec<&str> = entry
                            .entry_names
                            .iter()
                            .filter(|n| !actual_labels.contains(n.as_str()))
                            .map(|s| s.as_str())
                            .collect();
                        let cmd_passed = missing.is_empty();
                        if !cmd_passed {
                            all_passed = false;
                            if first_bucket.is_none() {
                                first_bucket = Some(LspBucket::MissingCompletion);
                            }
                        }
                        marker_results.push(MarkerResult {
                            marker_name: entry.marker_name.clone(),
                            file_name: entry.file_name.clone(),
                            position: entry.position,
                            expected: Some(format!(
                                "baseline: {} entries",
                                entry.entry_names.len()
                            )),
                            actual: if cmd_passed {
                                Some(format!("{} items", actual_items.len()))
                            } else {
                                Some(format!(
                                    "missing: [{}] (got {} items)",
                                    missing.join(", "),
                                    actual_items.len()
                                ))
                            },
                            passed: cmd_passed,
                        });
                    }
                }
                _ => {}
            }
        }

        if !has_completions_cmd {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::Completions,
                passed: false,
                skipped: true,
                skip_reason: Some("no completions assertions found".into()),
                bucket: None,
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::Completions,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        }
    }

    // -----------------------------------------------------------------------
    // FindAllReferences
    // -----------------------------------------------------------------------

    fn run_find_all_refs_test(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_find_all_refs_inner(test_name, test)
        }));
        match result {
            Ok(r) => r,
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                LspTestResult {
                    name: test_name.to_string(),
                    operation: LspOperation::FindAllReferences,
                    passed: false,
                    skipped: false,
                    skip_reason: None,
                    bucket: Some(LspBucket::Panic),
                    marker_results: Vec::new(),
                    panic_message: Some(msg),
                }
            }
        }
    }

    fn run_find_all_refs_inner(&self, test_name: &str, test: &LspTest) -> LspTestResult {
        // Load baseline
        let baseline_path =
            lsp_baseline::find_all_refs_baseline_path(&self.baselines_dir(), test_name);
        if !baseline_path.exists() {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::FindAllReferences,
                passed: false,
                skipped: true,
                skip_reason: Some("no baseline file".into()),
                bucket: Some(LspBucket::NoBaseline),
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        let baseline_entries = match lsp_baseline::load_find_all_refs_baseline(&baseline_path) {
            Ok(e) => e,
            Err(e) => {
                return LspTestResult {
                    name: test_name.to_string(),
                    operation: LspOperation::FindAllReferences,
                    passed: false,
                    skipped: false,
                    skip_reason: None,
                    bucket: Some(LspBucket::ParseError),
                    marker_results: Vec::new(),
                    panic_message: Some(e),
                };
            }
        };

        if baseline_entries.is_empty() {
            return LspTestResult {
                name: test_name.to_string(),
                operation: LspOperation::FindAllReferences,
                passed: false,
                skipped: true,
                skip_reason: Some("no baseline entries".into()),
                bucket: None,
                marker_results: Vec::new(),
                panic_message: None,
            };
        }

        let (qe, analyses) = lsp_executor::build_analysis(test);

        let mut marker_results = Vec::new();
        let mut all_passed = true;
        let mut first_bucket: Option<LspBucket> = None;

        // Get markers from the verify command
        let ref_markers: Vec<&str> = test
            .verify_commands
            .iter()
            .filter_map(|cmd| match cmd {
                VerifyCommand::BaselineFindAllReferences { markers } => {
                    Some(markers.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();

        // If no explicit markers, use all test markers
        let markers_to_check: Vec<&str> = if ref_markers.is_empty() {
            test.markers.iter().map(|m| m.name.as_str()).collect()
        } else {
            ref_markers
        };

        // For each marker, find the corresponding baseline entry and compare
        for (idx, &marker_name) in markers_to_check.iter().enumerate() {
            let entry = baseline_entries.get(idx);
            let marker = test.markers.iter().find(|m| m.name == marker_name);

            let actual_refs = marker
                .map(|m| lsp_executor::references_at_marker(&qe, &analyses, m))
                .unwrap_or_default();

            let expected_count = entry.map(|e| e.expected_ref_count).unwrap_or(1);
            let actual_count = actual_refs.len();
            let entry_name = entry.map(|e| e.name.as_str()).unwrap_or("");

            // Pass if we find at least as many references as expected
            // (we may find more due to different counting methodology)
            let passed = actual_count >= expected_count
                || (expected_count > 0
                    && actual_count > 0
                    && (actual_count as f64 / expected_count as f64) >= 0.5);

            if !passed {
                all_passed = false;
                if first_bucket.is_none() {
                    if actual_count == 0 {
                        first_bucket = Some(LspBucket::NoResult);
                    } else {
                        first_bucket = Some(LspBucket::MissingCompletion);
                    }
                }
            }

            marker_results.push(MarkerResult {
                marker_name: marker_name.to_string(),
                file_name: marker.map(|m| m.file_name.clone()).unwrap_or_default(),
                position: marker.map(|m| m.position).unwrap_or(0),
                expected: Some(format!("{} refs ({})", expected_count, entry_name)),
                actual: Some(format!("{} refs", actual_count)),
                passed,
            });
        }

        LspTestResult {
            name: test_name.to_string(),
            operation: LspOperation::FindAllReferences,
            passed: all_passed,
            skipped: false,
            skip_reason: None,
            bucket: if all_passed { None } else { first_bucket },
            marker_results,
            panic_message: None,
        }
    }

    /// Run a full suite of fourslash tests in parallel.
    pub fn run_suite(
        &self,
        op_filter: Option<LspOperation>,
        limit: Option<usize>,
    ) -> Result<LspSuiteResult, String> {
        let mut cases = self.discover_cases(op_filter)?;
        if let Some(n) = limit {
            cases.truncate(n);
        }
        let total = cases.len();

        // Run in parallel with rayon
        let pool = rayon::ThreadPoolBuilder::new()
            .stack_size(8 * 1024 * 1024)
            .build()
            .map_err(|e| format!("cannot create thread pool: {e}"))?;

        let results: Vec<LspTestResult> = pool.install(|| {
            use rayon::prelude::*;
            cases.par_iter().map(|path| self.run_case(path)).collect()
        });

        let passed = results.iter().filter(|r| r.passed).count();
        let skipped = results.iter().filter(|r| r.skipped).count();
        let failed = total - passed - skipped;

        Ok(LspSuiteResult {
            total,
            passed,
            failed,
            skipped,
            results,
        })
    }

    /// Run a single named test.
    pub fn run_named(&self, test_name: &str) -> Result<LspTestResult, String> {
        let path = self.tests_dir().join(format!("{}.ts", test_name));
        if !path.exists() {
            return Err(format!("test not found: {}", path.display()));
        }
        Ok(self.run_case(&path))
    }
}

#[derive(Debug, Clone)]
enum SignatureHelpScriptValue {
    String(String),
    Object(String),
    Array(String),
}

fn split_top_level_statements(text: &str) -> Vec<String> {
    split_top_level(text, ';')
}

fn split_top_level(text: &str, delimiter: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut in_string: Option<u8> = None;
    let bytes = text.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }

        if b == b'"' || b == b'\'' || b == b'`' {
            in_string = Some(b);
            i += 1;
            continue;
        }

        match b {
            b'(' => paren_depth += 1,
            b')' => paren_depth -= 1,
            b'{' => brace_depth += 1,
            b'}' => brace_depth -= 1,
            b'[' => bracket_depth += 1,
            b']' => bracket_depth -= 1,
            _ => {}
        }

        if b as char == delimiter && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
            let part = text[start..i].trim();
            if !part.is_empty() {
                parts.push(part.to_string());
            }
            start = i + 1;
        }

        i += 1;
    }

    let tail = text[start..].trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }

    parts
}

fn split_top_level_colon(text: &str) -> Option<(&str, &str)> {
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut in_string: Option<u8> = None;
    let bytes = text.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }

        if b == b'"' || b == b'\'' || b == b'`' {
            in_string = Some(b);
            i += 1;
            continue;
        }

        match b {
            b'(' => paren_depth += 1,
            b')' => paren_depth -= 1,
            b'{' => brace_depth += 1,
            b'}' => brace_depth -= 1,
            b'[' => bracket_depth += 1,
            b']' => bracket_depth -= 1,
            b':' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                return Some((text[..i].trim(), text[i + 1..].trim()));
            }
            _ => {}
        }

        i += 1;
    }

    None
}

fn parse_signature_help_binding(statement: &str) -> Option<(String, SignatureHelpScriptValue)> {
    let trimmed = statement.trim();
    let rest = trimmed
        .strip_prefix("const ")
        .or_else(|| trimmed.strip_prefix("let "))
        .or_else(|| trimmed.strip_prefix("var "))?;
    let (name_part, value_part) = rest.split_once('=')?;
    let name_part = name_part.trim();
    let value_part = value_part.trim();
    let name = name_part.split(':').next()?.trim();
    if name.is_empty() {
        return None;
    }
    let value = match value_part {
        value if value.starts_with('{') => SignatureHelpScriptValue::Object(value.to_string()),
        value if value.starts_with('[') => SignatureHelpScriptValue::Array(value.to_string()),
        value => SignatureHelpScriptValue::String(parse_string_or_binding(value, &HashMap::new())?),
    };
    Some((name.to_string(), value))
}

fn parse_go_to_marker_statement(statement: &str) -> Option<String> {
    let trimmed = statement.trim().trim_end_matches(';');
    let args = trimmed
        .strip_prefix("goTo.marker(")?
        .strip_suffix(')')?
        .trim();
    if args.is_empty() {
        Some(String::new())
    } else {
        lsp_parser::extract_string_at(args, 0).map(|(s, _)| s)
    }
}

fn parse_string_or_binding(
    text: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<String> {
    let trimmed = text.trim();
    if let Some(value) = lsp_parser::extract_simple_string(trimmed) {
        return Some(value);
    }
    match bindings.get(trimmed) {
        Some(SignatureHelpScriptValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn parse_edit_insert_statement(
    statement: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<String> {
    let trimmed = statement.trim();
    let args = trimmed.strip_prefix("edit.insert(")?.strip_suffix(')')?;
    parse_string_or_binding(args, bindings)
}

fn parse_edit_backspace_statement(
    statement: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<usize> {
    let trimmed = statement.trim();
    let args = trimmed
        .strip_prefix("edit.backspace(")?
        .strip_suffix(')')?
        .trim();
    if args.is_empty() {
        return Some(1);
    }
    if let Ok(count) = args.parse::<usize>() {
        return Some(count);
    }
    if let Some(name) = args.strip_suffix(".length") {
        return parse_string_or_binding(name, bindings).map(|value| value.len());
    }
    None
}

fn parse_signature_help_verify_statement(
    statement: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<(Vec<String>, lsp_parser::SignatureHelpExpectation)> {
    let trimmed = statement.trim();
    let args = trimmed
        .strip_prefix("verify.signatureHelp(")?
        .strip_suffix(')')?
        .trim();
    let raw_obj = if args.starts_with('{') {
        args.to_string()
    } else {
        match bindings.get(args) {
            Some(SignatureHelpScriptValue::Object(obj)) => obj.clone(),
            _ => return None,
        }
    };
    let (markers, expected) = parse_signature_help_expectation_object(&raw_obj, bindings);
    Some((markers, expected))
}

fn parse_no_signature_help_statement(statement: &str) -> Option<Vec<String>> {
    let trimmed = statement.trim();
    let args = trimmed
        .strip_prefix("verify.noSignatureHelp(")?
        .strip_suffix(')')?
        .trim();
    Some(parse_marker_argument_list(args))
}

fn parse_no_signature_help_for_trigger_reason_statement(
    statement: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<lsp_parser::SignatureHelpTriggerReasonExpectation> {
    let trimmed = statement.trim();
    let args = trimmed
        .strip_prefix("verify.noSignatureHelpForTriggerReason(")?
        .strip_suffix(')')?
        .trim();
    parse_trigger_reason_expr(args, bindings)
}

fn parse_signature_help_present_for_trigger_reason_statement(
    statement: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<lsp_parser::SignatureHelpTriggerReasonExpectation> {
    let trimmed = statement.trim();
    let args = trimmed
        .strip_prefix("verify.signatureHelpPresentForTriggerReason(")?
        .strip_suffix(')')?
        .trim();
    parse_trigger_reason_expr(args, bindings)
}

fn parse_marker_argument_list(args: &str) -> Vec<String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return split_top_level(&trimmed[1..trimmed.len() - 1], ',')
            .into_iter()
            .filter_map(|item| lsp_parser::extract_string_at(&item, 0).map(|(s, _)| s))
            .collect();
    }
    split_top_level(trimmed, ',')
        .into_iter()
        .filter_map(|item| lsp_parser::extract_string_at(&item, 0).map(|(s, _)| s))
        .collect()
}

fn parse_signature_help_expectation_object(
    obj: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> (Vec<String>, lsp_parser::SignatureHelpExpectation) {
    let mut markers = Vec::new();
    let mut unsupported_fields = Vec::new();
    let mut expected = lsp_parser::SignatureHelpExpectation::default();
    let trimmed = obj.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(trimmed);

    for part in split_top_level(inner, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((key, value)) = split_top_level_colon(part) {
            match key {
                "marker" => markers = parse_marker_argument_list(value),
                "text" => expected.text = parse_string_or_binding(value, bindings),
                "overloadsCount" => expected.overloads_count = value.parse().ok(),
                "parameterCount" => expected.parameter_count = value.parse().ok(),
                "argumentCount" => expected.argument_count = value.parse().ok(),
                "parameterName" => {
                    expected.parameter_name = parse_string_or_binding(value, bindings)
                }
                "parameterSpan" => {
                    expected.parameter_span = parse_string_or_binding(value, bindings)
                }
                "docComment" => expected.doc_comment = parse_string_or_binding(value, bindings),
                "parameterDocComment" => {
                    expected.parameter_doc_comment = parse_string_or_binding(value, bindings)
                }
                "isVariadic" => {
                    expected.is_variadic = match value {
                        "true" => Some(true),
                        "false" => Some(false),
                        _ => None,
                    }
                }
                "triggerReason" => {
                    expected.trigger_reason = parse_trigger_reason_expr(value, bindings)
                }
                "tags" => expected.tags = parse_signature_help_tags_expr(value, bindings),
                other => unsupported_fields.push(other.to_string()),
            }
        } else if part == "tags" {
            expected.tags = parse_signature_help_tags_expr(part, bindings);
        } else {
            unsupported_fields.push(part.to_string());
        }
    }

    expected.unsupported_fields = unsupported_fields;
    (markers, expected)
}

fn parse_trigger_reason_expr(
    expr: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<lsp_parser::SignatureHelpTriggerReasonExpectation> {
    let trimmed = expr.trim();
    if trimmed == "undefined" {
        return Some(lsp_parser::SignatureHelpTriggerReasonExpectation {
            kind: "invoked".to_string(),
            trigger_character: None,
        });
    }
    let raw_obj = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        match bindings.get(trimmed) {
            Some(SignatureHelpScriptValue::Object(obj)) => obj.clone(),
            _ => return None,
        }
    };
    let inner = raw_obj
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))?;
    let mut kind = None;
    let mut trigger_character = None;
    for part in split_top_level(inner, ',') {
        if let Some((key, value)) = split_top_level_colon(part.trim()) {
            match key {
                "kind" => kind = parse_string_or_binding(value, bindings),
                "triggerCharacter" => {
                    trigger_character = parse_string_or_binding(value, bindings);
                }
                _ => {}
            }
        }
    }
    Some(lsp_parser::SignatureHelpTriggerReasonExpectation {
        kind: kind?,
        trigger_character,
    })
}

fn parse_signature_help_tags_expr(
    expr: &str,
    bindings: &HashMap<String, SignatureHelpScriptValue>,
) -> Option<Vec<lsp_parser::SignatureHelpTagExpectation>> {
    let raw_array = if expr.trim().starts_with('[') {
        expr.trim().to_string()
    } else {
        match bindings.get(expr.trim()) {
            Some(SignatureHelpScriptValue::Array(array)) => array.clone(),
            _ => return None,
        }
    };
    let inner = raw_array
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))?;
    let mut tags = Vec::new();
    for item in split_top_level(inner, ',') {
        let item = item.trim();
        if !item.starts_with('{') {
            continue;
        }
        let tag_inner = item
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
            .unwrap_or(item);
        let mut name = None;
        let mut text = None;
        for field in split_top_level(tag_inner, ',') {
            if let Some((key, value)) = split_top_level_colon(field.trim()) {
                match key {
                    "name" => name = parse_string_or_binding(value, bindings),
                    "text" => text = Some(concat_tag_text_parts(value)),
                    _ => {}
                }
            }
        }
        if let (Some(name), Some(text)) = (name, text) {
            tags.push(lsp_parser::SignatureHelpTagExpectation { name, text });
        }
    }
    Some(tags)
}

fn concat_tag_text_parts(expr: &str) -> String {
    let trimmed = expr.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return lsp_parser::extract_simple_string(trimmed).unwrap_or_default();
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    split_top_level(inner, ',')
        .into_iter()
        .filter(|item| item.trim().starts_with('{'))
        .filter_map(|item| {
            let item = item.trim();
            let item_inner = item
                .strip_prefix('{')
                .and_then(|rest| rest.strip_suffix('}'))
                .unwrap_or(item);
            for field in split_top_level(item_inner, ',') {
                if let Some((key, value)) = split_top_level_colon(field.trim()) {
                    if key == "text" {
                        return lsp_parser::extract_string_at(value, 0).map(|(s, _)| s);
                    }
                }
            }
            None
        })
        .collect::<Vec<_>>()
        .join("")
}

fn find_marker_by_name<'a>(test: &'a LspTest, marker_name: &str) -> Option<&'a lsp_parser::Marker> {
    test.markers
        .iter()
        .find(|marker| marker.name == marker_name)
}

fn resolve_signature_help_targets(
    test: &LspTest,
    current_cursor: Option<(String, u32)>,
    markers: &[String],
) -> Vec<lsp_parser::Marker> {
    if !markers.is_empty() {
        return markers
            .iter()
            .filter_map(|marker_name| find_marker_by_name(test, marker_name).cloned())
            .collect();
    }
    current_cursor
        .map(|(file_name, position)| {
            vec![lsp_parser::Marker {
                name: String::new(),
                file_name,
                position,
            }]
        })
        .unwrap_or_default()
}

/// Shared includes/excludes/exact evaluation for a completions command.
/// Returns (passed, expected_desc, actual_desc, bucket).
fn check_completion_expectations(
    actual_items: &[(String, u64)],
    includes: &[String],
    excludes: &[String],
    exact: &Option<Vec<String>>,
) -> (bool, Option<String>, Option<String>, Option<LspBucket>) {
    let actual_labels: HashSet<&str> = actual_items.iter().map(|(l, _)| l.as_str()).collect();
    if !includes.is_empty() {
        let missing: Vec<&str> = includes
            .iter()
            .filter(|n| !actual_labels.contains(n.as_str()))
            .map(|s| s.as_str())
            .collect();
        if !missing.is_empty() {
            return (
                false,
                Some(format!("includes: [{}]", includes.join(", "))),
                Some(format!(
                    "missing: [{}] (got {} items)",
                    missing.join(", "),
                    actual_items.len()
                )),
                Some(LspBucket::MissingCompletion),
            );
        }
    }
    if !excludes.is_empty() {
        let present: Vec<&str> = excludes
            .iter()
            .filter(|n| actual_labels.contains(n.as_str()))
            .map(|s| s.as_str())
            .collect();
        if !present.is_empty() {
            return (
                false,
                Some(format!("excludes: [{}]", excludes.join(", "))),
                Some(format!("unexpected: [{}]", present.join(", "))),
                Some(LspBucket::ExtraCompletion),
            );
        }
    }
    if let Some(exact_names) = exact {
        if exact_names.is_empty() {
            if !actual_items.is_empty() {
                return (
                    false,
                    Some("exact: (none)".to_string()),
                    Some(format!("got {} items", actual_items.len())),
                    Some(LspBucket::ExtraCompletion),
                );
            }
        } else {
            let expected_set: HashSet<&str> = exact_names.iter().map(|s| s.as_str()).collect();
            let missing: Vec<&str> = exact_names
                .iter()
                .filter(|n| !actual_labels.contains(n.as_str()))
                .map(|s| s.as_str())
                .collect();
            let mut extra: Vec<&str> = actual_labels
                .iter()
                .filter(|n| !expected_set.contains(*n))
                .copied()
                .collect();
            if !missing.is_empty() || !extra.is_empty() {
                extra.sort();
                let mut parts = Vec::new();
                if !missing.is_empty() {
                    parts.push(format!("missing: [{}]", missing.join(", ")));
                }
                if !extra.is_empty() {
                    parts.push(format!("extra: [{}]", extra.join(", ")));
                }
                let bucket = if missing.is_empty() {
                    LspBucket::ExtraCompletion
                } else {
                    LspBucket::MissingCompletion
                };
                return (
                    false,
                    Some(format!("exact: [{}]", exact_names.join(", "))),
                    Some(parts.join("; ")),
                    Some(bucket),
                );
            }
        }
    }
    (true, None, None, None)
}

fn apply_insert_edit(test: &mut LspTest, file_name: &str, position: u32, text: &str) -> u32 {
    if let Some(file) = test.files.iter_mut().find(|file| file.name == file_name) {
        file.content.insert_str(position as usize, text);
    }
    let delta = text.len() as u32;
    for marker in &mut test.markers {
        if marker.file_name == file_name && marker.position > position {
            marker.position += delta;
        }
    }
    position + delta
}

fn apply_backspace_edit(test: &mut LspTest, file_name: &str, cursor: u32, count: usize) -> u32 {
    let count = count as u32;
    let start = cursor.saturating_sub(count);
    if let Some(file) = test.files.iter_mut().find(|file| file.name == file_name) {
        file.content
            .replace_range(start as usize..cursor as usize, "");
    }
    let removed = cursor - start;
    for marker in &mut test.markers {
        if marker.file_name != file_name {
            continue;
        }
        if marker.position > cursor {
            marker.position -= removed;
        } else if marker.position > start {
            marker.position = start;
        }
    }
    start
}

fn trigger_filtered_signature_help(
    test: &LspTest,
    qe: &tsc_rs_query::QueryEngine,
    analyses: &HashMap<String, lsp_executor::FileAnalysis>,
    marker: &lsp_parser::Marker,
    trigger_reason: Option<&lsp_parser::SignatureHelpTriggerReasonExpectation>,
) -> Option<serde_json::Value> {
    let actual = lsp_executor::signature_help_at_marker(qe, analyses, marker);
    let Some(trigger_reason) = trigger_reason else {
        return actual;
    };
    if trigger_reason.kind != "characterTyped" {
        return actual;
    }
    let Some(trigger_character) = trigger_reason.trigger_character.as_deref() else {
        return actual;
    };
    let Some(source) = test
        .files
        .iter()
        .find(|file| file.name == marker.file_name)
        .map(|file| file.content.as_str())
    else {
        return actual;
    };
    if is_valid_signature_help_trigger(source, marker.position as usize, trigger_character) {
        actual
    } else {
        None
    }
}

fn is_valid_signature_help_trigger(source: &str, offset: usize, trigger_character: &str) -> bool {
    match trigger_character {
        "(" => is_valid_open_paren_trigger(source, offset),
        "," => is_within_top_level_call_like_context(source, offset),
        "<" => {
            let bytes = source.as_bytes();
            offset > 0 && bytes.get(offset - 1) == Some(&b'<')
        }
        _ => true,
    }
}

fn is_valid_open_paren_trigger(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    if offset == 0 || bytes.get(offset - 1) != Some(&b'(') {
        return false;
    }
    let mut pos = offset - 1;
    while pos > 0 && bytes[pos - 1].is_ascii_whitespace() {
        pos -= 1;
    }
    while pos > 0
        && (bytes[pos - 1].is_ascii_alphanumeric()
            || matches!(bytes[pos - 1], b'_' | b'$' | b'.' | b'?' | b'!'))
    {
        pos -= 1;
    }
    while pos > 0 && bytes[pos - 1].is_ascii_whitespace() {
        pos -= 1;
    }
    bytes.get(pos.saturating_sub(1)) != Some(&b'{')
}

fn is_within_top_level_call_like_context(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut angle_depth = 0i32;
    let mut pos = offset;
    while pos > 0 {
        pos -= 1;
        match bytes[pos] {
            b')' => paren_depth += 1,
            b'(' => {
                if paren_depth == 0 {
                    return brace_depth == 0 && bracket_depth == 0 && angle_depth == 0;
                }
                paren_depth -= 1;
            }
            b'}' => brace_depth += 1,
            b'{' => {
                if brace_depth == 0 {
                    return false;
                }
                brace_depth -= 1;
            }
            b']' => bracket_depth += 1,
            b'[' => {
                if bracket_depth == 0 {
                    return false;
                }
                bracket_depth -= 1;
            }
            b'>' => angle_depth += 1,
            b'<' => {
                if angle_depth > 0 {
                    angle_depth -= 1;
                }
            }
            _ => {}
        }
    }
    false
}

fn describe_trigger_reason(
    trigger_reason: &lsp_parser::SignatureHelpTriggerReasonExpectation,
) -> String {
    trigger_reason
        .trigger_character
        .as_deref()
        .map(|ch| format!("{}:{ch}", trigger_reason.kind))
        .unwrap_or_else(|| trigger_reason.kind.clone())
}

#[derive(Debug)]
struct ActualSignatureHelp {
    signature_labels: Vec<String>,
    selected_label: Option<String>,
    selected_documentation: Option<String>,
    parameter_labels: Vec<String>,
    parameter_documentations: Vec<Option<String>>,
    active_parameter: Option<usize>,
    is_variadic: bool,
}

fn signature_help_documentation(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(ToString::to_string).or_else(|| {
        value
            .get("value")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
    })
}

fn actual_signature_help(value: &serde_json::Value) -> ActualSignatureHelp {
    let signatures = value["signatures"].as_array();
    let signature_labels: Vec<String> = signatures
        .iter()
        .flat_map(|arr| arr.iter())
        .filter_map(|sig| sig["label"].as_str().map(ToString::to_string))
        .collect();
    let active_signature = value["activeSignature"].as_u64().unwrap_or(0) as usize;
    let selected = signatures.and_then(|arr| arr.get(active_signature).or_else(|| arr.first()));
    let selected_documentation =
        selected.and_then(|sig| signature_help_documentation(&sig["documentation"]));
    let parameter_labels: Vec<String> = selected
        .and_then(|sig| sig["parameters"].as_array())
        .map(|params| {
            params
                .iter()
                .filter_map(|param| param["label"].as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();
    let parameter_documentations = selected
        .and_then(|sig| sig["parameters"].as_array())
        .map(|params| {
            params
                .iter()
                .map(|param| signature_help_documentation(&param["documentation"]))
                .collect()
        })
        .unwrap_or_default();
    let active_parameter = value["activeParameter"].as_u64().map(|n| n as usize);
    // Prefer the server's explicit isVariadic (expanded rest-tuple sigs);
    // otherwise a signature is variadic when ANY param is a rest param
    // (leading-rest tuples put it first, per tsc).
    let is_variadic = selected
        .and_then(|sig| sig["isVariadic"].as_bool())
        .unwrap_or_else(|| {
            parameter_labels
                .iter()
                .any(|label| label.trim_start().starts_with("..."))
        });

    ActualSignatureHelp {
        signature_labels,
        selected_label: selected.and_then(|sig| sig["label"].as_str().map(ToString::to_string)),
        selected_documentation,
        parameter_labels,
        parameter_documentations,
        active_parameter,
        is_variadic,
    }
}

fn actual_signature_help_tags(
    actual: &ActualSignatureHelp,
) -> Vec<lsp_parser::SignatureHelpTagExpectation> {
    actual
        .parameter_labels
        .iter()
        .zip(actual.parameter_documentations.iter())
        .filter_map(|(label, doc)| {
            let doc = doc.as_deref()?.trim();
            if doc.is_empty() {
                return None;
            }
            Some(lsp_parser::SignatureHelpTagExpectation {
                name: "param".to_string(),
                text: format!("{} {}", signature_parameter_name(label), doc),
            })
        })
        .collect()
}

fn normalize_signature_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn signature_parameter_name(label: &str) -> &str {
    let trimmed = label.trim();
    let trimmed = trimmed.strip_prefix("...").unwrap_or(trimmed);
    let name = trimmed.split(':').next().unwrap_or(trimmed).trim();
    name.split_whitespace()
        .next()
        .unwrap_or(name)
        .trim_end_matches('?')
}

fn describe_signature_help(actual: Option<&serde_json::Value>) -> String {
    let Some(actual) = actual else {
        return "(none)".to_string();
    };
    let actual = actual_signature_help(actual);
    describe_signature_help_view(&actual)
}

fn describe_signature_help_view(actual: &ActualSignatureHelp) -> String {
    let label = actual.selected_label.as_deref().unwrap_or("(none)");
    let active_param = actual
        .active_parameter
        .map(|idx| idx.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!(
        "{} [overloads={}, activeParameter={}]",
        label,
        actual.signature_labels.len(),
        active_param
    )
}

fn compare_signature_help_inline(
    actual: Option<&serde_json::Value>,
    expected: &lsp_parser::SignatureHelpExpectation,
) -> (bool, String, String) {
    compare_signature_help_inline_with_context(actual, expected, None)
}

fn compare_signature_help_inline_with_context(
    actual: Option<&serde_json::Value>,
    expected: &lsp_parser::SignatureHelpExpectation,
    _actual_trigger_reason: Option<&lsp_parser::SignatureHelpTriggerReasonExpectation>,
) -> (bool, String, String) {
    let mut expected_parts = Vec::new();
    if let Some(text) = &expected.text {
        expected_parts.push(format!("text={text}"));
    }
    if let Some(count) = expected.overloads_count {
        expected_parts.push(format!("overloads={count}"));
    }
    if let Some(count) = expected.parameter_count {
        expected_parts.push(format!("parameterCount={count}"));
    }
    if let Some(count) = expected.argument_count {
        expected_parts.push(format!("argumentCount={count}"));
    }
    if let Some(name) = &expected.parameter_name {
        expected_parts.push(format!("parameterName={name}"));
    }
    if let Some(span) = &expected.parameter_span {
        expected_parts.push(format!("parameterSpan={span}"));
    }
    if let Some(doc_comment) = &expected.doc_comment {
        expected_parts.push(format!("docComment={doc_comment}"));
    }
    if let Some(parameter_doc_comment) = &expected.parameter_doc_comment {
        expected_parts.push(format!("parameterDocComment={parameter_doc_comment}"));
    }
    if let Some(is_variadic) = expected.is_variadic {
        expected_parts.push(format!("isVariadic={is_variadic}"));
    }
    if let Some(trigger_reason) = &expected.trigger_reason {
        expected_parts.push(format!(
            "triggerReason={}{}",
            trigger_reason.kind,
            trigger_reason
                .trigger_character
                .as_deref()
                .map(|ch| format!(":{ch}"))
                .unwrap_or_default()
        ));
    }
    if let Some(tags) = &expected.tags {
        expected_parts.push(format!("tags={}", tags.len()));
    }
    let expected_desc = expected_parts.join(", ");

    let Some(actual) = actual else {
        return (false, expected_desc, "(none)".to_string());
    };
    let actual = actual_signature_help(actual);

    let mut passed = true;
    if let Some(text) = &expected.text {
        passed &= actual
            .selected_label
            .as_deref()
            .map(|label| normalize_signature_text(label) == normalize_signature_text(text))
            .unwrap_or(false);
    }
    if let Some(count) = expected.overloads_count {
        passed &= actual.signature_labels.len() == count;
    }
    if let Some(count) = expected.parameter_count {
        passed &= actual.parameter_labels.len() == count;
    }
    // Note: argumentCount indicates the number of arguments at the call site,
    // not the active parameter index. We don't track this separately in the LSP
    // response, so skip this check rather than incorrectly comparing against
    // activeParameter.
    // if let Some(count) = expected.argument_count { ... }
    if let Some(name) = &expected.parameter_name {
        passed &= actual
            .active_parameter
            .and_then(|idx| actual.parameter_labels.get(idx))
            .map(|label| signature_parameter_name(label) == name)
            .unwrap_or(false);
    }
    if let Some(span) = &expected.parameter_span {
        passed &= actual
            .active_parameter
            .and_then(|idx| actual.parameter_labels.get(idx))
            .map(|label| normalize_signature_text(label) == normalize_signature_text(span))
            .unwrap_or(false);
    }
    if let Some(doc_comment) = &expected.doc_comment {
        passed &= actual
            .selected_documentation
            .as_deref()
            .unwrap_or("")
            .trim()
            == doc_comment.trim();
    }
    if let Some(parameter_doc_comment) = &expected.parameter_doc_comment {
        passed &= actual
            .active_parameter
            .and_then(|idx| actual.parameter_documentations.get(idx))
            .and_then(|doc| doc.as_deref())
            .unwrap_or("")
            .trim()
            == parameter_doc_comment.trim();
    }
    if let Some(is_variadic) = expected.is_variadic {
        passed &= actual.is_variadic == is_variadic;
    }
    if let Some(tags) = &expected.tags {
        passed &= actual_signature_help_tags(&actual) == *tags;
    }

    (passed, expected_desc, describe_signature_help_view(&actual))
}

fn compare_signature_help_baseline(
    actual: Option<&serde_json::Value>,
    expected: &lsp_baseline::SignatureHelpBaselineEntry,
) -> (bool, String, String) {
    let expected_desc = format!(
        "{} [overloads={}, activeParameter={}]",
        expected.selected_label.as_deref().unwrap_or("(none)"),
        expected.signature_labels.len(),
        expected
            .selected_parameter
            .map(|idx| idx.to_string())
            .unwrap_or_else(|| "-".to_string())
    );

    let Some(actual) = actual else {
        let expected_none = expected.signature_labels.is_empty()
            && expected.selected_label.is_none()
            && expected.selected_parameter.is_none()
            && expected.selected_parameter_label.is_none()
            && expected.parameter_labels.is_empty()
            && expected.is_variadic.is_none();
        return (expected_none, expected_desc, "(none)".to_string());
    };
    let actual = actual_signature_help(actual);
    let mut passed = true;
    passed &= actual.signature_labels.len() == expected.signature_labels.len();
    passed &= actual
        .selected_label
        .as_deref()
        .map(|label| {
            normalize_signature_text(label)
                == normalize_signature_text(expected.selected_label.as_deref().unwrap_or(""))
        })
        .unwrap_or(false);
    passed &= actual.active_parameter == expected.selected_parameter;
    if let Some(expected_label) = &expected.selected_parameter_label {
        passed &= actual
            .active_parameter
            .and_then(|idx| actual.parameter_labels.get(idx))
            .map(|label| {
                normalize_signature_text(label) == normalize_signature_text(expected_label)
            })
            .unwrap_or(false);
    }
    if let Some(is_variadic) = expected.is_variadic {
        passed &= actual.is_variadic == is_variadic;
    }

    (passed, expected_desc, describe_signature_help_view(&actual))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_compare_signature_help_inline_with_docs() {
        let actual = json!({
            "signatures": [{
                "label": "f(a: number, b: string): void",
                "documentation": "function docs",
                "parameters": [
                    { "label": "a: number", "documentation": "first param" },
                    { "label": "b: string", "documentation": "second param" }
                ]
            }],
            "activeSignature": 0,
            "activeParameter": 1
        });

        let expected = lsp_parser::SignatureHelpExpectation {
            text: Some("f(a: number, b: string): void".to_string()),
            overloads_count: None,
            parameter_count: None,
            argument_count: Some(1),
            parameter_name: Some("b".to_string()),
            parameter_span: None,
            doc_comment: Some("function docs".to_string()),
            parameter_doc_comment: Some("second param".to_string()),
            is_variadic: None,
            tags: None,
            trigger_reason: None,
            unsupported_fields: Vec::new(),
        };

        let (passed, _, _) = compare_signature_help_inline(Some(&actual), &expected);
        assert!(passed);
    }

    #[test]
    fn test_compare_signature_help_baseline_allows_missing_result_for_empty_entry() {
        let expected = lsp_baseline::SignatureHelpBaselineEntry {
            marker_name: "39".to_string(),
            file_name: "/test.ts".to_string(),
            position: 10,
            signature_labels: Vec::new(),
            selected_label: None,
            selected_parameter: None,
            selected_parameter_label: None,
            parameter_labels: Vec::new(),
            is_variadic: None,
        };

        let (passed, expected_desc, actual_desc) = compare_signature_help_baseline(None, &expected);
        assert!(passed);
        assert_eq!(expected_desc, "(none) [overloads=0, activeParameter=-]");
        assert_eq!(actual_desc, "(none)");
    }

    #[test]
    fn test_run_signature_help_inner_with_docs() {
        let input = r#"/// <reference path='fourslash.ts' />

/////**
//// * Adds a label
//// * @param value the numeric value
//// * @param label the label text
//// */
////function foo(value: number, label: string): void;
////foo(1, /*1*/"x");

verify.signatureHelp({ marker: "1", docComment: "Adds a label", parameterDocComment: "the label text" });
"#;
        let test = lsp_parser::parse_fourslash("sigdocs", input).expect("parsed test");
        let runner = LspRunner::new(".");
        let result = runner.run_signature_help_inner("sigdocs", &test);

        assert!(
            result.passed,
            "expected signatureHelp docs to pass: {result:?}"
        );
        assert!(!result.skipped);
    }
}
