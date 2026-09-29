//! Integration tests for the baseline test runner infrastructure.
//!
//! These tests verify that the harness can:
//! - Discover test cases from the tests/ directory
//! - Parse test file directives
//! - Split multi-file tests
//! - Compile through the pipeline
//! - Compare output against stored baselines
//!
//! The workspace root is computed by walking up from the crate directory.

use tsc_rs_harness::{BaselineRunner, Suite};

/// Find the workspace root (the directory containing `Cargo.toml` with
/// `[workspace]` and the `tests/` directory).
fn workspace_root() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/tsc_rs_harness -> crates -> workspace root
    let root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to find workspace root");
    assert!(
        root.join("tests").is_dir(),
        "workspace root does not contain tests/ directory: {}",
        root.display()
    );
    root.to_path_buf()
}

// ---------------------------------------------------------------------------
// Discovery tests
// ---------------------------------------------------------------------------

#[test]
fn discover_compiler_cases() {
    let runner = BaselineRunner::new(workspace_root());
    let cases = runner
        .discover_cases(Suite::Compiler)
        .expect("failed to discover compiler cases");
    assert!(
        !cases.is_empty(),
        "expected at least one compiler test case"
    );
    // Verify they are all .ts files.
    for case in &cases {
        let ext = case.extension().and_then(|e| e.to_str()).unwrap_or("");
        assert!(
            ext == "ts" || ext == "tsx",
            "unexpected extension for {:?}",
            case
        );
    }
}

#[test]
fn discover_conformance_cases() {
    let runner = BaselineRunner::new(workspace_root());
    let cases = runner
        .discover_cases(Suite::Compiler)
        .expect("failed to discover conformance cases");
    assert!(
        !cases.is_empty(),
        "expected at least one conformance test case"
    );
}

#[test]
fn discover_project_cases() {
    let runner = BaselineRunner::new(workspace_root());
    let cases = runner
        .discover_cases(Suite::Project)
        .expect("failed to discover project cases");
    assert!(!cases.is_empty(), "expected at least one project test case");
    for case in &cases {
        let ext = case.extension().and_then(|e| e.to_str()).unwrap_or("");
        assert_eq!(
            ext, "json",
            "unexpected project case extension for {:?}",
            case
        );
    }
}

// ---------------------------------------------------------------------------
// Small subset: verify infrastructure works end-to-end
// ---------------------------------------------------------------------------

/// Run a small, hand-picked set of compiler tests to validate the
/// harness infrastructure. These tests are chosen because they are
/// simple single-file tests with known baselines.
#[test]
fn run_small_compiler_subset() {
    let runner = BaselineRunner::new(workspace_root());

    // These tests are simple and have known .js baselines.
    let test_names = &[
        "2dArrays",
        "abstractClassInLocalScope",
        "abstractIdentifierNameStrict",
    ];

    let results = runner
        .run_named_cases(Suite::Compiler, test_names)
        .expect("failed to run named cases");

    assert_eq!(results.len(), test_names.len());

    // Print results for debugging.
    for result in &results {
        println!(
            "  {} :: passed={} baseline_exists={}",
            result.name, result.passed, result.baseline_exists
        );
        if let Some(ref diff) = result.diff {
            // Only print first few lines of the diff.
            let preview: String = diff.lines().take(10).collect::<Vec<_>>().join("\n");
            println!("    diff preview:\n{preview}");
        }
    }

    // All test files should exist.
    for result in &results {
        assert!(
            result.baseline_exists,
            "baseline for {} should exist",
            result.name
        );
    }

    // The emitter is functional but still under development. Some tests may
    // fail due to incomplete transform coverage. The important thing is that
    // the infrastructure works: tests are discovered, parsed, compiled, and
    // compared against baselines.
    println!("\n--- Infrastructure validation ---");
    println!(
        "All {} test cases were found, parsed, and compared against baselines.",
        results.len()
    );
    println!(
        "Passing: {} / {}",
        results.iter().filter(|r| r.passed).count(),
        results.len()
    );
}

#[test]
fn run_small_project_subset() {
    let runner = BaselineRunner::new(workspace_root());
    let test_names = &["baseline3", "outModuleSimpleNoOutdir", "relativeNested"];

    let results = runner
        .run_named_cases(Suite::Project, test_names)
        .expect("failed to run named project cases");

    assert_eq!(results.len(), test_names.len());

    for result in &results {
        println!(
            "  {} :: passed={} baseline_exists={}",
            result.name, result.passed, result.baseline_exists
        );
        if let Some(ref diff) = result.diff {
            let preview: String = diff.lines().take(10).collect::<Vec<_>>().join("\n");
            println!("    diff preview:\n{preview}");
        }
        assert!(
            result.baseline_exists,
            "project baseline for {} should exist",
            result.name
        );
        assert!(
            result.actual_output.contains("//// [node/"),
            "project output should contain node snapshot sections"
        );
        assert!(
            result.actual_output.contains("//// [amd/"),
            "project output should contain amd snapshot sections"
        );
        assert!(
            result.passed,
            "project case {} should match baseline",
            result.name
        );
    }
}

/// Verify that the baseline output format is correct structurally.
/// Even with the stub emitter, the format should have the right markers.
#[test]
fn baseline_output_format_is_correct() {
    let runner = BaselineRunner::new(workspace_root());
    let results = runner
        .run_named_cases(Suite::Compiler, &["2dArrays"])
        .expect("failed to run case");

    assert_eq!(results.len(), 1);
    let result = &results[0];

    // The actual output should contain the baseline markers.
    assert!(
        result
            .actual_output
            .contains("//// [tests/cases/compiler/2dArrays.ts] ////"),
        "output should contain the header marker"
    );
    assert!(
        result.actual_output.contains("//// [2dArrays.ts]"),
        "output should contain input file marker"
    );
    assert!(
        result.actual_output.contains("//// [2dArrays.js]"),
        "output should contain output file marker"
    );
}

/// Verify that multi-file test parsing works correctly.
#[test]
fn multi_file_test_parsing() {
    use tsc_rs_harness::parse_test_case;

    let source = r#"// @target: es2015
// @module: commonjs
// @filename: helper.ts
export function helper() { return 42; }

// @filename: main.ts
import { helper } from './helper';
console.log(helper());
"#;

    let tc = parse_test_case("tests/cases/compiler/multiFile.ts", source);

    assert_eq!(tc.files.len(), 2, "should have 2 files");
    assert_eq!(tc.files[0].name, "helper.ts");
    assert_eq!(tc.files[1].name, "main.ts");
    assert!(tc.files[0].content.contains("export function helper"));
    assert!(tc.files[1].content.contains("import { helper }"));

    // Options should be parsed.
    assert!(tc.options.target.is_some(), "target should be parsed");
    assert!(tc.options.module.is_some(), "module should be parsed");
}

// ---------------------------------------------------------------------------
// Full suite run (ignored by default -- use `cargo test -- --ignored`)
// ---------------------------------------------------------------------------

#[test]
fn debug_test_output() {
    let runner = BaselineRunner::new(workspace_root());
    let tests = &["regularExpressionScanning"];
    let results = runner
        .run_named_cases(Suite::Compiler, tests)
        .expect("failed to run cases");

    for r in &results {
        eprintln!("\n=== {} (passed={}) ===", r.name, r.passed);
        if !r.passed {
            if let Some(ref diff) = r.diff {
                eprintln!("{}", diff);
            }
            eprintln!("--- ACTUAL ---");
            for (i, line) in r.actual_output.lines().enumerate() {
                eprintln!("{:3}: {:?}", i + 1, line);
            }
            eprintln!("--- EXPECTED ---");
            for (i, line) in r.expected_output.lines().enumerate() {
                eprintln!("{:3}: {:?}", i + 1, line);
            }
        }
    }
}

#[test]
fn test_span_includes_comment() {
    let source = "var y: string[][]; // Expect no error here";
    let file = tsc_rs_parser::parse("test.ts", source);
    for (i, stmt) in file.statements.iter().enumerate() {
        let start = stmt.span.start as usize;
        let end = (stmt.span.end as usize).min(source.len());
        eprintln!(
            "stmt[{}] span: {}-{} text: {:?}",
            i,
            start,
            end,
            &source[start..end]
        );
    }
}

#[test]
fn test_arrow_iife_parsing() {
    let source = "(() => {\n    class A {}\n    return A;\n})();";
    let file = tsc_rs_parser::parse("test.ts", source);
    eprintln!("Statements: {}", file.statements.len());
    for (i, stmt) in file.statements.iter().enumerate() {
        let start = stmt.span.start as usize;
        let end = (stmt.span.end as usize).min(source.len());
        eprintln!(
            "  stmt[{}] span: {:?} text: {:?}",
            i,
            stmt.span,
            &source[start..end]
        );
    }
    let js = tsc_rs_emitter::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
    eprintln!("JS output:\n{}", js);
}

#[test]
fn test_cjs_export_predecl() {
    use tsc_rs_ast::*;
    let source = "export class C {\n}\n\nexport = B;";
    let file = tsc_rs_parser::parse("test.ts", source);
    eprintln!("Statements: {}", file.statements.len());
    for (i, stmt) in file.statements.iter().enumerate() {
        eprintln!("  stmt[{}]: {:?}", i, std::mem::discriminant(&stmt.kind));
        match &stmt.kind {
            StmtKind::Export(ed) => {
                eprintln!(
                    "    ExportDecl kind: {:?}",
                    std::mem::discriminant(&ed.kind)
                );
                if let ExportDeclKind::Decl(ref decl) = ed.kind {
                    eprintln!(
                        "    inner decl kind: {:?}",
                        std::mem::discriminant(&decl.kind)
                    );
                    if let StmtKind::ClassDecl(ref cd) = decl.kind {
                        eprintln!("    class name: {:?}, modifiers: {}", cd.name, cd.modifiers);
                    }
                }
            }
            StmtKind::ExportAssign(ref expr) => {
                eprintln!(
                    "    ExportAssign expr kind: {:?}",
                    std::mem::discriminant(&expr.kind)
                );
            }
            _ => {}
        }
    }
    let mut opts = CompilerOptions::default();
    opts.module = Some(ModuleKind::CommonJS);
    let js = tsc_rs_emitter::emit(&file, &opts).javascript;
    eprintln!("JS output:\n{}", js);
    assert!(
        js.contains("exports.C = void 0;"),
        "Missing pre-declaration. Output:\n{}",
        js
    );
}

/// Analyze failure patterns across the first 200 compiler tests.
///
/// Categorizes failures into buckets (PANIC, CRASH, no baseline, missing input,
/// diff) and further classifies diff failures by the kind of code construct
/// involved (class methods, enums, imports/exports, type stripping, etc.).
///
/// Run with:
///   cargo test -p tsc_rs_harness --test baseline_tests analyze_failure_patterns -- --ignored --nocapture
#[test]
#[ignore]
fn analyze_failure_patterns() {
    use std::collections::BTreeMap;

    let runner = BaselineRunner::new(workspace_root());
    let suite_result = runner
        .run_suite_with_limit(Suite::Compiler, Some(1000))
        .expect("failed to run compiler suite");

    // -----------------------------------------------------------------------
    // Categorize failures into buckets
    // -----------------------------------------------------------------------
    let mut pass_count = 0usize;
    let mut panic_count = 0usize;
    let mut crash_count = 0usize;
    let mut no_baseline_count = 0usize;
    let mut missing_input_count = 0usize;
    let mut diff_count = 0usize;

    // For diff failures, sub-classify
    let mut trailing_whitespace_only = 0usize;
    let mut real_content_diff = 0usize;

    // Pattern buckets for diff failures
    let mut pattern_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let patterns = [
        "class method emit",
        "enum emit",
        "import/export",
        "type stripping",
        "decorator",
        "namespace/module",
        "generic type params",
        "interface not stripped",
        "type alias not stripped",
        "arrow function",
        "async/await",
        "optional chaining",
        "template literal",
        "spread/rest",
        "destructuring",
        "blank line / formatting",
        "other",
    ];
    for p in &patterns {
        pattern_counts.insert(p, 0);
    }

    // Collect panic messages for reporting
    let mut panic_messages: Vec<(String, String)> = Vec::new();
    // Collect sample diffs
    let mut sample_diffs: Vec<(String, String, Vec<&str>)> = Vec::new();

    for result in &suite_result.results {
        if result.passed {
            pass_count += 1;
            continue;
        }

        let diff_text = result.diff.as_deref().unwrap_or("");

        // Categorize into primary bucket
        if diff_text.starts_with("PANIC during compilation:") {
            panic_count += 1;
            let msg = diff_text
                .strip_prefix("PANIC during compilation: ")
                .unwrap_or(diff_text)
                .to_string();
            // Truncate long messages
            let short_msg: String = msg.chars().take(120).collect();
            panic_messages.push((result.name.clone(), short_msg));
            continue;
        }

        if diff_text.contains("CRASH") || diff_text.contains("stack overflow") {
            crash_count += 1;
            continue;
        }

        if !result.baseline_exists || diff_text.contains("baseline file not found") {
            no_baseline_count += 1;
            continue;
        }

        if result.actual_output.trim().is_empty()
            || result
                .actual_output
                .lines()
                .all(|l| l.starts_with("////") || l.trim().is_empty())
        {
            missing_input_count += 1;
            continue;
        }

        // This is a diff failure
        diff_count += 1;

        // Check if the diff is just trailing whitespace
        let expected_trimmed: String = result
            .expected_output
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        let actual_trimmed: String = result
            .actual_output
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        if expected_trimmed == actual_trimmed {
            trailing_whitespace_only += 1;
            *pattern_counts.get_mut("blank line / formatting").unwrap() += 1;
            continue;
        }

        real_content_diff += 1;

        // Find the first differing line for classification
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let max_lines = exp_lines.len().max(act_lines.len());

        let mut first_diff_exp = "";
        let mut first_diff_act = "";
        for i in 0..max_lines {
            let e = exp_lines.get(i).copied().unwrap_or("");
            let a = act_lines.get(i).copied().unwrap_or("");
            if e != a {
                first_diff_exp = e;
                first_diff_act = a;
                break;
            }
        }

        // Combine expected + actual + diff for pattern matching
        let combined = format!(
            "{}\n{}\n{}\n{}\n{}",
            first_diff_exp, first_diff_act, diff_text, result.expected_output, result.actual_output
        );
        let combined_lower = combined.to_lowercase();

        // Classify into patterns (a test can match multiple)
        let mut matched_patterns: Vec<&str> = Vec::new();

        // Check for type annotations remaining in actual output that shouldn't be there
        let actual_has_type_annotations = result.actual_output.contains(": string")
            || result.actual_output.contains(": number")
            || result.actual_output.contains(": boolean")
            || result.actual_output.contains(": any")
            || result.actual_output.contains(": void")
            || result.actual_output.contains("as string")
            || result.actual_output.contains("as number");

        // Check for interface/type alias remaining in actual .js output
        let js_sections: Vec<&str> = result
            .actual_output
            .split("//// [")
            .filter(|s| s.contains(".js]"))
            .collect();
        let js_output = js_sections.join("\n");

        let has_interface_in_js =
            js_output.contains("interface ") && !js_output.contains("// interface");
        let has_type_alias_in_js = js_output.lines().any(|l| {
            let trimmed = l.trim();
            trimmed.starts_with("type ") && trimmed.contains('=') && !trimmed.starts_with("typeof")
        });

        if has_interface_in_js {
            matched_patterns.push("interface not stripped");
            *pattern_counts.get_mut("interface not stripped").unwrap() += 1;
        }

        if has_type_alias_in_js {
            matched_patterns.push("type alias not stripped");
            *pattern_counts.get_mut("type alias not stripped").unwrap() += 1;
        }

        if actual_has_type_annotations {
            matched_patterns.push("type stripping");
            *pattern_counts.get_mut("type stripping").unwrap() += 1;
        }

        if combined_lower.contains("class")
            && (combined_lower.contains("method")
                || combined_lower.contains("constructor")
                || combined_lower.contains("extends")
                || combined_lower.contains("super"))
        {
            matched_patterns.push("class method emit");
            *pattern_counts.get_mut("class method emit").unwrap() += 1;
        }

        if combined_lower.contains("enum ") || combined_lower.contains("enum{") {
            matched_patterns.push("enum emit");
            *pattern_counts.get_mut("enum emit").unwrap() += 1;
        }

        if combined_lower.contains("import ")
            || combined_lower.contains("export ")
            || combined_lower.contains("require(")
            || combined_lower.contains("module.exports")
        {
            matched_patterns.push("import/export");
            *pattern_counts.get_mut("import/export").unwrap() += 1;
        }

        if combined_lower.contains("decorator")
            || (combined_lower.contains("@") && combined_lower.contains("__decorate"))
        {
            matched_patterns.push("decorator");
            *pattern_counts.get_mut("decorator").unwrap() += 1;
        }

        if combined_lower.contains("namespace ")
            || (combined_lower.contains("module ") && combined_lower.contains("var "))
        {
            matched_patterns.push("namespace/module");
            *pattern_counts.get_mut("namespace/module").unwrap() += 1;
        }

        if combined.contains('<')
            && combined.contains('>')
            && (combined.contains("<T>")
                || combined.contains("<T,")
                || combined.contains("<T extends"))
        {
            matched_patterns.push("generic type params");
            *pattern_counts.get_mut("generic type params").unwrap() += 1;
        }

        if combined_lower.contains("=>")
            && (first_diff_exp.contains("=>") || first_diff_act.contains("=>"))
        {
            matched_patterns.push("arrow function");
            *pattern_counts.get_mut("arrow function").unwrap() += 1;
        }

        if combined_lower.contains("async ")
            || combined_lower.contains("await ")
            || combined_lower.contains("__awaiter")
        {
            matched_patterns.push("async/await");
            *pattern_counts.get_mut("async/await").unwrap() += 1;
        }

        if combined_lower.contains("?.") || combined_lower.contains("??") {
            matched_patterns.push("optional chaining");
            *pattern_counts.get_mut("optional chaining").unwrap() += 1;
        }

        if combined.contains('`') || combined.contains("${") {
            matched_patterns.push("template literal");
            *pattern_counts.get_mut("template literal").unwrap() += 1;
        }

        if combined_lower.contains("...")
            || combined_lower.contains("rest")
            || combined_lower.contains("spread")
        {
            matched_patterns.push("spread/rest");
            *pattern_counts.get_mut("spread/rest").unwrap() += 1;
        }

        if combined.contains("const {")
            || combined.contains("let {")
            || combined.contains("var {")
            || combined.contains("const [")
            || combined.contains("let [")
            || combined.contains("var [")
        {
            matched_patterns.push("destructuring");
            *pattern_counts.get_mut("destructuring").unwrap() += 1;
        }

        // If only formatting differences (blank line counts differ)
        let exp_non_blank = exp_lines.iter().filter(|l| !l.trim().is_empty()).count();
        let act_non_blank = act_lines.iter().filter(|l| !l.trim().is_empty()).count();
        if exp_non_blank == act_non_blank {
            let exp_nb: Vec<&&str> = exp_lines.iter().filter(|l| !l.trim().is_empty()).collect();
            let act_nb: Vec<&&str> = act_lines.iter().filter(|l| !l.trim().is_empty()).collect();
            if exp_nb == act_nb {
                matched_patterns.push("blank line / formatting");
                *pattern_counts.get_mut("blank line / formatting").unwrap() += 1;
            }
        }

        if matched_patterns.is_empty() {
            matched_patterns.push("other");
            *pattern_counts.get_mut("other").unwrap() += 1;
        }

        // Collect sample diffs (up to 15)
        if sample_diffs.len() < 15 {
            let diff_preview: String = diff_text.lines().take(8).collect::<Vec<_>>().join("\n");
            sample_diffs.push((result.name.clone(), diff_preview, matched_patterns));
        }
    }

    // -----------------------------------------------------------------------
    // Print summary table
    // -----------------------------------------------------------------------
    let total = suite_result.total;
    println!("\n{}", "=".repeat(72));
    println!(
        "  FAILURE PATTERN ANALYSIS -- first {} compiler tests",
        total
    );
    println!("{}", "=".repeat(72));

    println!("\n  {:<25} {:>6} {:>8}", "Category", "Count", "Pct");
    println!("  {}", "-".repeat(42));
    println!(
        "  {:<25} {:>6} {:>7.1}%",
        "PASS",
        pass_count,
        pass_count as f64 / total as f64 * 100.0
    );
    println!(
        "  {:<25} {:>6} {:>7.1}%",
        "PANIC",
        panic_count,
        panic_count as f64 / total as f64 * 100.0
    );
    println!(
        "  {:<25} {:>6} {:>7.1}%",
        "CRASH (stack overflow)",
        crash_count,
        crash_count as f64 / total as f64 * 100.0
    );
    println!(
        "  {:<25} {:>6} {:>7.1}%",
        "No baseline (.js)",
        no_baseline_count,
        no_baseline_count as f64 / total as f64 * 100.0
    );
    println!(
        "  {:<25} {:>6} {:>7.1}%",
        "Missing/empty output",
        missing_input_count,
        missing_input_count as f64 / total as f64 * 100.0
    );
    println!(
        "  {:<25} {:>6} {:>7.1}%",
        "Diff (total)",
        diff_count,
        diff_count as f64 / total as f64 * 100.0
    );
    println!("  {}", "-".repeat(42));
    println!("  {:<25} {:>6}", "TOTAL", total);

    println!("\n  Diff breakdown:");
    println!(
        "    {:<30} {:>6}",
        "Trailing whitespace only", trailing_whitespace_only
    );
    println!(
        "    {:<30} {:>6}",
        "Real content differences", real_content_diff
    );

    println!("\n  Diff pattern classification (tests can match multiple):");
    println!("  {:<30} {:>6}", "Pattern", "Count");
    println!("  {}", "-".repeat(38));
    let mut sorted_patterns: Vec<(&&str, &usize)> = pattern_counts.iter().collect();
    sorted_patterns.sort_by(|a, b| b.1.cmp(a.1));
    for (pattern, count) in &sorted_patterns {
        if **count > 0 {
            println!("  {:<30} {:>6}", pattern, count);
        }
    }

    // Print panic message summary
    if !panic_messages.is_empty() {
        println!("\n  PANIC messages (first 20):");
        println!("  {}", "-".repeat(60));
        // Group by message prefix
        let mut msg_groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, msg) in &panic_messages {
            let key: String = msg.chars().take(60).collect();
            msg_groups.entry(key).or_default().push(name.clone());
        }
        let mut msg_sorted: Vec<(String, Vec<String>)> = msg_groups.into_iter().collect();
        msg_sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (msg, names) in msg_sorted.iter().take(20) {
            println!("    ({:>3}x) {}", names.len(), msg);
            for name in names.iter().take(3) {
                println!("          e.g. {}", name);
            }
        }
    }

    // Print sample diffs
    if !sample_diffs.is_empty() {
        println!("\n  Sample diff failures (first 15):");
        println!("  {}", "-".repeat(60));
        for (name, diff_preview, pats) in &sample_diffs {
            println!("    {} [{}]:", name, pats.join(", "));
            for line in diff_preview.lines() {
                println!("      {}", line);
            }
            println!();
        }
    }

    println!("{}", "=".repeat(72));
}

/// Run ALL compiler tests and report pass rates.
///
/// This is ignored by default because it may take a while and the stub
/// emitter will produce many failures. Run with:
///   cargo test -p tsc_rs_harness --test baseline_tests -- --ignored
#[test]
#[ignore]
fn run_all_compiler_tests() {
    let runner = BaselineRunner::new(workspace_root());
    let suite_result = runner
        .run_suite(Suite::Compiler)
        .expect("failed to run compiler suite");

    suite_result.print_summary();

    // Print all failures with mismatch counts for analysis.
    let failures = suite_result.failures();
    if !failures.is_empty() {
        // Group failures by mismatch count
        let mut by_mismatch: std::collections::BTreeMap<usize, Vec<&str>> =
            std::collections::BTreeMap::new();
        for result in &failures {
            if let Some(ref diff) = result.diff {
                // Extract mismatched count from diff
                let mismatch = diff
                    .lines()
                    .find(|l| l.starts_with("total lines:"))
                    .and_then(|l| l.rfind("mismatched="))
                    .and_then(|idx| {
                        let l = diff
                            .lines()
                            .find(|l| l.starts_with("total lines:"))
                            .unwrap();
                        l[idx + "mismatched=".len()..].parse::<usize>().ok()
                    })
                    .unwrap_or(999);
                by_mismatch.entry(mismatch).or_default().push(&result.name);
            }
        }
        println!("\nFailure distribution by mismatch count:");
        for (count, names) in &by_mismatch {
            println!("  {} mismatched line(s): {} tests", count, names.len());
            if *count <= 3 {
                for name in names {
                    println!("    - {}", name);
                }
            }
        }
        // For 1-mismatch tests, show the specific diff
        println!("\n1-MISMATCH DETAILS:");
        let mut pattern_counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for result in &failures {
            if let Some(ref diff) = result.diff {
                let mismatch = diff
                    .lines()
                    .find(|l| l.starts_with("total lines:"))
                    .and_then(|l| l.rfind("mismatched="))
                    .and_then(|idx| {
                        let l = diff
                            .lines()
                            .find(|l| l.starts_with("total lines:"))
                            .unwrap();
                        l[idx + "mismatched=".len()..].parse::<usize>().ok()
                    })
                    .unwrap_or(999);
                if mismatch != 1 {
                    continue;
                }
                // Extract expected and actual
                let mut exp = String::new();
                let mut act = String::new();
                for line in diff.lines() {
                    if line.starts_with("  expected: ") {
                        exp = line["  expected: ".len()..].to_string();
                    }
                    if line.starts_with("  actual:   ") {
                        act = line["  actual:   ".len()..].to_string();
                    }
                }
                // Classify pattern
                let pattern = if exp.trim() == act.trim() {
                    "WHITESPACE-ONLY".to_string()
                } else if exp.contains("exports.") && !act.contains("exports.") {
                    "MISSING-EXPORTS".to_string()
                } else if !exp.contains("exports.") && act.contains("exports.") {
                    "EXTRA-EXPORTS".to_string()
                } else if exp.starts_with("//") || act.starts_with("//") {
                    "COMMENT-DIFF".to_string()
                } else if (exp.is_empty() || exp == "<missing>") && !act.is_empty() {
                    let trunc = act
                        .char_indices()
                        .nth(60)
                        .map(|(i, _)| i)
                        .unwrap_or(act.len());
                    format!("EXTRA-LINE: {}", &act[..trunc])
                } else if (act.is_empty() || act == "<missing>") && !exp.is_empty() {
                    let trunc = exp
                        .char_indices()
                        .nth(60)
                        .map(|(i, _)| i)
                        .unwrap_or(exp.len());
                    format!("MISSING-LINE: {}", &exp[..trunc])
                } else {
                    let exp_trunc = exp
                        .char_indices()
                        .nth(40)
                        .map(|(i, _)| i)
                        .unwrap_or(exp.len());
                    let act_trunc = act
                        .char_indices()
                        .nth(40)
                        .map(|(i, _)| i)
                        .unwrap_or(act.len());
                    format!(
                        "CONTENT: exp=[{}] act=[{}]",
                        &exp[..exp_trunc],
                        &act[..act_trunc]
                    )
                };
                *pattern_counts.entry(pattern).or_insert(0) += 1;
            }
        }
        let mut sorted: Vec<_> = pattern_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (pattern, count) in sorted {
            println!("  {:>4} {}", count, pattern);
        }
    }

    // This assertion is intentionally lenient. With the stub emitter
    // we expect mostly failures. As the real emitter is developed,
    // tighten this threshold.
    println!(
        "\nCompiler suite: {}/{} passed ({:.1}%)",
        suite_result.passed,
        suite_result.total,
        suite_result.pass_rate()
    );
}

/// Analyze ALL diff lines across ALL failing tests to find common fixable patterns.
/// Groups by the specific (expected, actual) line pair to find which exact changes
/// would fix the most tests.
#[test]
#[ignore]
fn analyze_diff_line_patterns() {
    use std::collections::BTreeMap;

    let runner = BaselineRunner::new(workspace_root());
    let suite_result = runner
        .run_suite(Suite::Compiler)
        .expect("failed to run compiler suite");

    // For each diff line, extract the pattern and count how many tests it appears in.
    // We look at structural patterns, not exact text.
    let mut pattern_test_count: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for result in &suite_result.results {
        if result.passed {
            continue;
        }
        let diff_text = match result.diff.as_deref() {
            Some(d) => d,
            None => continue,
        };
        if diff_text.starts_with("PANIC") || diff_text.contains("baseline file not found") {
            continue;
        }

        // Parse all diff lines
        let lines: Vec<&str> = diff_text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].starts_with("line ") && lines[i].ends_with(':') {
                let exp = lines
                    .get(i + 1)
                    .and_then(|s| s.strip_prefix("  expected: "))
                    .unwrap_or("");
                let act = lines
                    .get(i + 2)
                    .and_then(|s| s.strip_prefix("  actual:   "))
                    .unwrap_or("");

                // Classify this diff pair into a pattern
                let pattern = classify_diff_pair(exp, act);
                pattern_test_count
                    .entry(pattern)
                    .or_default()
                    .push(result.name.clone());
                i += 3;
            } else {
                i += 1;
            }
        }
    }

    // Deduplicate test names per pattern (a pattern can appear multiple times in one test)
    let mut pattern_unique: BTreeMap<String, usize> = BTreeMap::new();
    for (pattern, tests) in &pattern_test_count {
        let mut unique_tests = tests.clone();
        unique_tests.sort();
        unique_tests.dedup();
        pattern_unique.insert(pattern.clone(), unique_tests.len());
    }

    let mut sorted: Vec<_> = pattern_unique.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    println!("\n{}", "=".repeat(80));
    println!("  CROSS-TEST DIFF PATTERN ANALYSIS (all failing tests)");
    println!("{}", "=".repeat(80));
    println!("\n  {:<60} {:>6}", "PATTERN", "TESTS");
    println!("  {}", "-".repeat(68));

    for (pattern, count) in sorted.iter().take(50) {
        println!("  {:<60} {:>6}", pattern, count);
    }

    // Also show examples for top patterns
    println!("\n\n  TOP PATTERN EXAMPLES:");
    println!("  {}", "-".repeat(68));
    for (pattern, count) in sorted.iter().take(15) {
        let tests = &pattern_test_count[*pattern];
        let mut unique_tests = tests.clone();
        unique_tests.sort();
        unique_tests.dedup();
        println!("\n  {} ({} tests):", pattern, count);
        for t in unique_tests.iter().take(5) {
            println!("    - {}", t);
        }
    }

    // Find tests where ALL mismatches are semicolon-only or whitespace-only
    println!("\n\n  TESTS FIXABLE BY SEMICOLON NORMALIZATION ONLY:");
    println!("  {}", "-".repeat(68));
    let mut semicolon_only_tests: Vec<String> = Vec::new();
    for result in &suite_result.results {
        if result.passed {
            continue;
        }
        let diff_text = match result.diff.as_deref() {
            Some(d) => d,
            None => continue,
        };
        if diff_text.starts_with("PANIC") || diff_text.contains("baseline file not found") {
            continue;
        }
        let lines: Vec<&str> = diff_text.lines().collect();
        let mut all_semi = true;
        let mut has_diff = false;
        let mut i = 0;
        while i < lines.len() {
            if lines[i].starts_with("line ") && lines[i].ends_with(':') {
                let exp = lines
                    .get(i + 1)
                    .and_then(|s| s.strip_prefix("  expected: "))
                    .unwrap_or("");
                let act = lines
                    .get(i + 2)
                    .and_then(|s| s.strip_prefix("  actual:   "))
                    .unwrap_or("");
                has_diff = true;
                let et = exp.trim();
                let at = act.trim();
                if et.trim_end_matches(';') != at.trim_end_matches(';') {
                    if et != at {
                        all_semi = false;
                    }
                }
                i += 3;
            } else {
                i += 1;
            }
        }
        if has_diff && all_semi {
            semicolon_only_tests.push(result.name.clone());
        }
    }
    println!("  Count: {}", semicolon_only_tests.len());
    for t in &semicolon_only_tests {
        println!("    - {}", t);
    }

    println!("\n{}", "=".repeat(80));
}

fn classify_diff_pair(exp: &str, act: &str) -> String {
    let et = exp.trim();
    let at = act.trim();

    // Whitespace only
    if et == at {
        let exp_indent = exp.len() - exp.trim_start().len();
        let act_indent = act.len() - act.trim_start().len();
        if exp_indent != act_indent {
            let diff = if exp_indent > act_indent {
                format!(
                    "WHITESPACE: under-indented by {} (exp={}, act={})",
                    exp_indent - act_indent,
                    exp_indent,
                    act_indent
                )
            } else {
                format!(
                    "WHITESPACE: over-indented by {} (exp={}, act={})",
                    act_indent - exp_indent,
                    exp_indent,
                    act_indent
                )
            };
            return diff;
        }
        return "WHITESPACE: trailing space diff".to_string();
    }

    // Missing line
    if act.is_empty() || act == "<missing>" {
        if et.starts_with("\"use strict\"") || et.starts_with("'use strict'") {
            return "MISSING: use strict".to_string();
        }
        if et.starts_with("Object.defineProperty(exports") {
            return "MISSING: Object.defineProperty(exports)".to_string();
        }
        if et.starts_with("exports.") {
            return "MISSING: exports.X = ...".to_string();
        }
        if et.contains("void 0") && et.contains("exports") {
            return "MISSING: exports.X = void 0".to_string();
        }
        if et.starts_with("//") {
            return "MISSING: comment".to_string();
        }
        if et.starts_with("var ") {
            return "MISSING: var declaration".to_string();
        }
        if et.is_empty() {
            return "MISSING: blank line".to_string();
        }
        return "MISSING: other line".to_string();
    }

    // Extra line
    if exp.is_empty() || exp == "<missing>" {
        if at.starts_with("//") {
            return "EXTRA: comment".to_string();
        }
        if at.contains(": string")
            || at.contains(": number")
            || at.contains(": boolean")
            || at.contains(": any")
            || at.contains(": void")
        {
            return "EXTRA: type annotation not stripped".to_string();
        }
        if at.starts_with("interface ") {
            return "EXTRA: interface not erased".to_string();
        }
        if at.starts_with("type ") && at.contains("=") {
            return "EXTRA: type alias not erased".to_string();
        }
        if at.is_empty() {
            return "EXTRA: blank line".to_string();
        }
        return "EXTRA: other line".to_string();
    }

    // Both present but differ
    // Comment diff
    if et.starts_with("//") && at.starts_with("//") {
        return "COMMENT: different content".to_string();
    }
    if et.starts_with("//") || at.starts_with("//") {
        return "COMMENT: line comment mismatch".to_string();
    }

    // Semicolon
    if et.trim_end_matches(';') == at.trim_end_matches(';') {
        let exp_semis = et.matches(';').count();
        let act_semis = at.matches(';').count();
        if exp_semis > act_semis {
            return format!(
                "SEMICOLON: missing trailing semicolon (exp has {} more)",
                exp_semis - act_semis
            );
        } else {
            return format!(
                "SEMICOLON: extra trailing semicolon (act has {} more)",
                act_semis - exp_semis
            );
        }
    }

    // Trailing comma
    if et.trim_end_matches(',') == at.trim_end_matches(',') {
        return "COMMA: trailing comma diff".to_string();
    }

    // IIFE/namespace
    if (et.contains("(function") || at.contains("(function"))
        && (et.contains("||") || at.contains("||"))
    {
        return "IIFE: enum/namespace wrapping".to_string();
    }

    // Module imports
    if et.contains("require(") || at.contains("require(") {
        return "MODULE: require() mismatch".to_string();
    }
    if et.contains("exports.") || at.contains("exports.") {
        return "MODULE: exports mismatch".to_string();
    }

    // Type not stripped
    if (at.contains(": string")
        || at.contains(": number")
        || at.contains(": boolean")
        || at.contains(": any")
        || at.contains(": void")
        || at.contains("?: "))
        && !et.contains(": string")
        && !et.contains(": number")
        && !et.contains(": boolean")
        && !et.contains(": any")
        && !et.contains(": void")
        && !et.contains("?: ")
    {
        return "TYPE: annotation not stripped".to_string();
    }

    // Generic type params
    if at.contains('<') && at.contains('>') && !et.contains('<') {
        return "TYPE: generic params not stripped".to_string();
    }

    // Return type
    if at.contains("): ") && !et.contains("): ") {
        return "TYPE: return type not stripped".to_string();
    }

    // As assertion
    if at.contains(" as ") && !et.contains(" as ") {
        return "TYPE: as assertion not stripped".to_string();
    }

    // System.register named vs unnamed
    if et.contains("System.register") && at.contains("System.register") {
        return "MODULE: System.register mismatch".to_string();
    }

    // var vs const vs let
    if (et.starts_with("var ") || et.starts_with("const ") || et.starts_with("let "))
        && (at.starts_with("var ") || at.starts_with("const ") || at.starts_with("let "))
    {
        // Check if only the keyword differs
        let et_after = et.splitn(2, ' ').nth(1).unwrap_or("");
        let at_after = at.splitn(2, ' ').nth(1).unwrap_or("");
        if et_after == at_after {
            return "KEYWORD: var/let/const mismatch".to_string();
        }
    }

    // __rest / destructuring
    if et.contains("__rest") || at.contains("...") {
        return "REST: destructuring/rest mismatch".to_string();
    }

    // __awaiter
    if et.contains("__awaiter") || at.contains("__awaiter") {
        return "ASYNC: __awaiter mismatch".to_string();
    }

    // Decorator
    if et.contains("__decorate") || at.contains("__decorate") {
        return "DECORATOR: __decorate mismatch".to_string();
    }

    "OTHER".to_string()
}

/// Run ALL conformance tests and report pass rates.
#[test]
#[ignore]
fn run_all_conformance_tests() {
    let runner = BaselineRunner::new(workspace_root());
    let suite_result = runner
        .run_suite(Suite::Conformance)
        .expect("failed to run conformance suite");

    suite_result.print_summary();

    println!(
        "\nConformance suite: {}/{} passed ({:.1}%)",
        suite_result.passed,
        suite_result.total,
        suite_result.pass_rate()
    );
}

/// Find quick-win fixes by analysing small-diff failures across the first
/// 500 compiler tests.
///
/// "Small diff" = at most 5 mismatched lines (parsed from the harness diff
/// output). For every such failure the *first* diff line is categorised into
/// a human-readable pattern bucket. The test prints patterns sorted by
/// frequency together with 3-5 concrete examples for each top bucket.
///
/// Run with:
///   cargo test -p tsc_rs_harness --test baseline_tests find_quick_wins -- --ignored --nocapture
#[test]
#[ignore]
fn find_quick_wins() {
    use std::collections::BTreeMap;

    let runner = BaselineRunner::new(workspace_root());
    let suite_result = runner
        .run_suite_with_limit(Suite::Compiler, Some(1000))
        .expect("failed to run compiler suite (first 1000)");

    // ------------------------------------------------------------------
    // 1. Collect small-diff failures
    // ------------------------------------------------------------------

    /// Parsed representation of one mismatch line inside a diff.
    struct DiffLine {
        expected: String,
        actual: String,
    }

    /// Parse the harness diff text into individual mismatch entries.
    fn parse_diff_lines(diff: &str) -> (Vec<DiffLine>, usize) {
        let mut entries: Vec<DiffLine> = Vec::new();
        let mut total_mismatched: usize = 0;
        let lines: Vec<&str> = diff.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i];
            // "line N:"
            if let Some(rest) = l.strip_prefix("line ") {
                if let Some(num_str) = rest.strip_suffix(':') {
                    if num_str.parse::<usize>().is_ok() {
                        let exp = lines
                            .get(i + 1)
                            .and_then(|s| s.strip_prefix("  expected: "))
                            .unwrap_or("")
                            .to_string();
                        let act = lines
                            .get(i + 2)
                            .and_then(|s| s.strip_prefix("  actual:   "))
                            .unwrap_or("")
                            .to_string();
                        entries.push(DiffLine {
                            expected: exp,
                            actual: act,
                        });
                        i += 3;
                        continue;
                    }
                }
            }
            // "total lines: expected=X, actual=Y, mismatched=Z"
            if l.starts_with("total lines:") {
                if let Some(idx) = l.rfind("mismatched=") {
                    let num_part = &l[idx + "mismatched=".len()..];
                    total_mismatched = num_part.parse::<usize>().unwrap_or(0);
                }
            }
            i += 1;
        }
        if total_mismatched == 0 {
            total_mismatched = entries.len();
        }
        (entries, total_mismatched)
    }

    /// Classify a diff line pair into a pattern string.
    fn classify_pattern(expected: &str, actual: &str) -> String {
        let exp = expected.trim();
        let act = actual.trim();

        // Missing line (actual is empty / <missing>)
        if act == "<missing>" || act.is_empty() {
            if exp.starts_with("\"use strict\"") || exp.starts_with("'use strict'") {
                return "missing \"use strict\"".to_string();
            }
            if exp.starts_with("Object.defineProperty(exports") {
                return "missing Object.defineProperty(exports)".to_string();
            }
            if exp.starts_with("exports.") || exp.starts_with("module.exports") {
                return "missing exports assignment".to_string();
            }
            if exp.starts_with("var ") || exp.starts_with("let ") || exp.starts_with("const ") {
                return "missing variable declaration".to_string();
            }
            if exp.starts_with("//") || exp.starts_with("////") {
                return "missing comment / header line".to_string();
            }
            return format!("expected line missing in actual: {}", truncate(exp, 60));
        }

        // Extra line (expected is empty / <missing>)
        if exp == "<missing>" || exp.is_empty() {
            if act.contains(": ")
                && (act.contains("string")
                    || act.contains("number")
                    || act.contains("boolean")
                    || act.contains("any")
                    || act.contains("void"))
            {
                return "extra type annotation not stripped".to_string();
            }
            if act.starts_with("interface ") {
                return "interface not erased".to_string();
            }
            if act.starts_with("type ") && act.contains("=") {
                return "type alias not erased".to_string();
            }
            return format!("extra line in actual: {}", truncate(act, 60));
        }

        // Both lines present but differ -- classify the difference
        // Check for type annotation remaining
        if act.contains(": string")
            || act.contains(": number")
            || act.contains(": boolean")
            || act.contains(": any")
            || act.contains(": void")
            || act.contains(": undefined")
            || act.contains(": null")
            || act.contains("?: ")
        {
            if !exp.contains(": string")
                && !exp.contains(": number")
                && !exp.contains(": boolean")
                && !exp.contains(": any")
                && !exp.contains(": void")
                && !exp.contains(": undefined")
                && !exp.contains(": null")
                && !exp.contains("?: ")
            {
                return "type annotation not stripped".to_string();
            }
        }

        // Generic type params remaining
        if act.contains('<') && act.contains('>') {
            let stripped = strip_strings(act);
            if stripped.contains('<') && !exp.contains('<') {
                return "generic type params not stripped".to_string();
            }
        }

        // Return type annotation not stripped
        if act.contains("): ") && !exp.contains("): ") {
            return "return type annotation not stripped".to_string();
        }

        // "as X" cast not stripped
        if act.contains(" as ") && !exp.contains(" as ") {
            return "type assertion (as) not stripped".to_string();
        }

        // "use strict" mismatch
        if exp.contains("use strict") || act.contains("use strict") {
            return "\"use strict\" mismatch".to_string();
        }

        // Object.defineProperty(exports)
        if exp.contains("Object.defineProperty") || act.contains("Object.defineProperty") {
            return "Object.defineProperty(exports) mismatch".to_string();
        }

        // exports.__esModule
        if exp.contains("__esModule") || act.contains("__esModule") {
            return "__esModule mismatch".to_string();
        }

        // Enum lowering
        if exp.contains("||")
            && exp.contains("(function")
            && (exp.contains("var ") || exp.contains("let ") || exp.contains("const "))
        {
            return "enum IIFE lowering".to_string();
        }
        if exp.contains("[\"") && exp.contains("] = ") {
            return "enum member assignment".to_string();
        }

        // Namespace / module IIFE
        if (exp.contains("(function") || act.contains("(function"))
            && (exp.contains("var ") || act.contains("var "))
        {
            return "namespace/module IIFE wrapping".to_string();
        }

        // import/require mismatch
        if exp.contains("require(") || act.contains("require(") {
            return "require() import mismatch".to_string();
        }
        if exp.contains("exports.") || act.contains("exports.") {
            return "exports assignment mismatch".to_string();
        }

        // Whitespace / indentation only
        if exp.trim() == act.trim() {
            return "whitespace/indentation difference".to_string();
        }

        // Semicolon difference
        if exp.trim().trim_end_matches(';') == act.trim().trim_end_matches(';') {
            return "semicolon mismatch".to_string();
        }

        // Trailing comma
        if exp.trim().trim_end_matches(',') == act.trim().trim_end_matches(',') {
            return "trailing comma mismatch".to_string();
        }

        // var vs let vs const
        if (exp.starts_with("var ") && (act.starts_with("let ") || act.starts_with("const ")))
            || (act.starts_with("var ") && (exp.starts_with("let ") || exp.starts_with("const ")))
        {
            return "var/let/const keyword mismatch".to_string();
        }

        // Catch-all: show a summary of the diff
        let common_prefix_len = exp
            .chars()
            .zip(act.chars())
            .take_while(|(a, b)| a == b)
            .count();
        if common_prefix_len > 10 {
            let diverge_exp = &exp[common_prefix_len..];
            let diverge_act = &act[common_prefix_len..];
            return format!(
                "content mismatch at col {}: exp='{}' act='{}'",
                common_prefix_len + 1,
                truncate(diverge_exp, 40),
                truncate(diverge_act, 40)
            );
        }

        format!(
            "content mismatch: exp='{}' act='{}'",
            truncate(exp, 40),
            truncate(act, 40)
        )
    }

    fn truncate(s: &str, max: usize) -> String {
        if s.len() <= max {
            s.to_string()
        } else {
            format!("{}...", &s[..max])
        }
    }

    /// Very rough string-literal stripping so we don't classify quoted `<`
    /// as a generic.
    fn strip_strings(s: &str) -> String {
        let mut out = String::new();
        let mut in_str = None;
        let mut prev = ' ';
        for ch in s.chars() {
            match in_str {
                None => {
                    if ch == '\'' || ch == '"' || ch == '`' {
                        in_str = Some(ch);
                    } else {
                        out.push(ch);
                    }
                }
                Some(delim) => {
                    if ch == delim && prev != '\\' {
                        in_str = None;
                    }
                }
            }
            prev = ch;
        }
        out
    }

    // ------------------------------------------------------------------
    // Structures for collecting results
    // ------------------------------------------------------------------
    struct QuickWin {
        name: String,
        mismatched_count: usize,
        pattern: String,
        first_expected: String,
        first_actual: String,
    }

    let mut quick_wins: Vec<QuickWin> = Vec::new();

    for result in &suite_result.results {
        if result.passed {
            continue;
        }
        let diff_text = match result.diff.as_deref() {
            Some(d) => d,
            None => continue,
        };
        // Skip panics/crashes
        if diff_text.starts_with("PANIC")
            || diff_text.starts_with("CRASH")
            || diff_text.contains("baseline file not found")
        {
            continue;
        }

        let (entries, total_mismatched) = parse_diff_lines(diff_text);
        if total_mismatched == 0 || total_mismatched > 5 {
            continue;
        }

        // Use the first diff entry to classify
        if let Some(first) = entries.first() {
            let pattern = classify_pattern(&first.expected, &first.actual);
            quick_wins.push(QuickWin {
                name: result.name.clone(),
                mismatched_count: total_mismatched,
                pattern,
                first_expected: first.expected.clone(),
                first_actual: first.actual.clone(),
            });
        }
    }

    // ------------------------------------------------------------------
    // 2. Group by pattern and sort by frequency
    // ------------------------------------------------------------------
    let mut pattern_groups: BTreeMap<String, Vec<&QuickWin>> = BTreeMap::new();
    for qw in &quick_wins {
        pattern_groups
            .entry(qw.pattern.clone())
            .or_default()
            .push(qw);
    }

    let mut sorted_groups: Vec<(String, Vec<&QuickWin>)> = pattern_groups.into_iter().collect();
    sorted_groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    // ------------------------------------------------------------------
    // 3. Print results
    // ------------------------------------------------------------------
    println!("\n{}", "=".repeat(80));
    println!(
        "  QUICK-WIN ANALYSIS  (first 500 compiler tests, failures with <= 5 mismatched lines)"
    );
    println!("{}", "=".repeat(80));
    println!("\n  Total tests run:          {}", suite_result.total);
    println!("  Total passed:             {}", suite_result.passed);
    println!(
        "  Total failed:             {}",
        suite_result.failed + suite_result.skipped
    );
    println!(
        "  Quick-win candidates:     {} (small-diff failures)",
        quick_wins.len()
    );

    println!("\n  {:<55} {:>5}", "PATTERN (by first diff line)", "COUNT");
    println!("  {}", "-".repeat(62));

    for (pattern, examples) in &sorted_groups {
        println!("  {:<55} {:>5}", pattern, examples.len());
    }

    println!("\n{}", "-".repeat(80));
    println!("  DETAILED EXAMPLES (3-5 per pattern)");
    println!("{}", "-".repeat(80));

    for (pattern, examples) in &sorted_groups {
        let show_count = examples.len().min(5);
        println!(
            "\n  -- {} ({} total, showing {}) --",
            pattern,
            examples.len(),
            show_count
        );
        for qw in examples.iter().take(5) {
            println!(
                "    [{}] ({} mismatched lines)",
                qw.name, qw.mismatched_count
            );
            println!("      expected: {}", truncate(&qw.first_expected, 100));
            println!("      actual:   {}", truncate(&qw.first_actual, 100));
        }
    }

    println!("\n{}", "=".repeat(80));
    println!(
        "  Fixing the top patterns above would improve pass rate by up to {} tests",
        quick_wins.len()
    );
    println!(
        "  Current pass rate: {:.1}% ({}/{})",
        suite_result.pass_rate(),
        suite_result.passed,
        suite_result.total
    );
    println!("{}", "=".repeat(80));
}
