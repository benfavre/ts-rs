use std::collections::HashMap;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to find workspace root")
        .to_path_buf()
}

fn safe_trunc(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn categorize_line(line: &str) -> &'static str {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        "BLANK"
    } else if trimmed.contains("use strict") {
        "USE-STRICT"
    } else if trimmed.contains("//") || trimmed.contains("/*") {
        "COMMENT"
    } else if trimmed.contains("exports.") || trimmed.contains("module.exports") {
        "EXPORT-LINE"
    } else if trimmed.contains("Object.defineProperty") {
        "OBJECT-DEFINE"
    } else if trimmed.contains("require(") {
        "IMPORT-LINE"
    } else if trimmed.contains("void 0") {
        "VOID-0"
    } else {
        "OTHER"
    }
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diff_by_1_analysis() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Category -> Vec<(test_name, direction, line_content, line_number)>
    let mut categories: HashMap<&str, Vec<(String, String, String, usize)>> = HashMap::new();

    let mut total_diff_by_1 = 0;
    let mut extra_in_actual = 0;
    let mut missing_in_actual = 0;

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        let len_diff = act_lines.len() as i64 - exp_lines.len() as i64;
        if len_diff.unsigned_abs() != 1 {
            continue;
        }

        total_diff_by_1 += 1;

        if len_diff == 1 {
            // actual has one extra line -- find it
            extra_in_actual += 1;
            let mut found = false;
            let mut ai = 0;
            let mut ei = 0;
            while ai < act_lines.len() && ei < exp_lines.len() {
                if act_lines[ai] == exp_lines[ei] {
                    ai += 1;
                    ei += 1;
                } else {
                    // This line in actual is the extra one
                    let line = act_lines[ai];
                    let cat = categorize_line(line);
                    let samples = categories.entry(cat).or_insert_with(Vec::new);
                    samples.push((
                        name.clone(),
                        "EXTRA-IN-ACTUAL".to_string(),
                        line.to_string(),
                        ai + 1,
                    ));
                    found = true;
                    break;
                }
            }
            if !found {
                // Extra line is the last line in actual
                let line = act_lines[act_lines.len() - 1];
                let cat = categorize_line(line);
                let samples = categories.entry(cat).or_insert_with(Vec::new);
                samples.push((
                    name.clone(),
                    "EXTRA-IN-ACTUAL".to_string(),
                    line.to_string(),
                    act_lines.len(),
                ));
            }
        } else {
            // expected has one extra line (actual is missing one) -- find it
            missing_in_actual += 1;
            let mut found = false;
            let mut ai = 0;
            let mut ei = 0;
            while ai < act_lines.len() && ei < exp_lines.len() {
                if act_lines[ai] == exp_lines[ei] {
                    ai += 1;
                    ei += 1;
                } else {
                    // This line in expected is missing from actual
                    let line = exp_lines[ei];
                    let cat = categorize_line(line);
                    let samples = categories.entry(cat).or_insert_with(Vec::new);
                    samples.push((
                        name.clone(),
                        "MISSING-IN-ACTUAL".to_string(),
                        line.to_string(),
                        ei + 1,
                    ));
                    found = true;
                    break;
                }
            }
            if !found {
                // Missing line is the last line in expected
                let line = exp_lines[exp_lines.len() - 1];
                let cat = categorize_line(line);
                let samples = categories.entry(cat).or_insert_with(Vec::new);
                samples.push((
                    name.clone(),
                    "MISSING-IN-ACTUAL".to_string(),
                    line.to_string(),
                    exp_lines.len(),
                ));
            }
        }
    }

    eprintln!("\n============================================================");
    eprintln!("DIFF-BY-1 ANALYSIS");
    eprintln!("============================================================");
    eprintln!("Total diff-by-1 failures: {}", total_diff_by_1);
    eprintln!("  Extra line in actual:   {}", extra_in_actual);
    eprintln!("  Missing line in actual: {}", missing_in_actual);
    eprintln!("============================================================\n");

    // Sort categories by count descending
    let mut sorted_cats: Vec<_> = categories.iter().collect();
    sorted_cats.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (cat, samples) in &sorted_cats {
        let extra_count = samples.iter().filter(|s| s.1 == "EXTRA-IN-ACTUAL").count();
        let missing_count = samples
            .iter()
            .filter(|s| s.1 == "MISSING-IN-ACTUAL")
            .count();
        eprintln!(
            "=== {} (total: {}, extra: {}, missing: {}) ===",
            cat,
            samples.len(),
            extra_count,
            missing_count
        );

        // Show up to 5 samples (30 for OTHER category)
        let sample_limit = if **cat == "OTHER" { 30 } else { 5 };
        for (i, (test_name, direction, line_content, line_num)) in samples.iter().enumerate() {
            if i >= sample_limit {
                break;
            }
            eprintln!(
                "  [{}] {} L{}: {:?}",
                direction,
                test_name,
                line_num,
                safe_trunc(line_content, 120)
            );
        }
        eprintln!();
    }

    eprintln!("============================================================");
    eprintln!("SUMMARY TABLE");
    eprintln!("============================================================");
    eprintln!(
        "{:<20} {:>8} {:>8} {:>8}",
        "Category", "Total", "Extra", "Missing"
    );
    eprintln!("{:-<20} {:-^8} {:-^8} {:-^8}", "", "", "", "");
    for (cat, samples) in &sorted_cats {
        let extra_count = samples.iter().filter(|s| s.1 == "EXTRA-IN-ACTUAL").count();
        let missing_count = samples
            .iter()
            .filter(|s| s.1 == "MISSING-IN-ACTUAL")
            .count();
        eprintln!(
            "{:<20} {:>8} {:>8} {:>8}",
            cat,
            samples.len(),
            extra_count,
            missing_count
        );
    }
    eprintln!("{:-<20} {:-^8} {:-^8} {:-^8}", "", "", "", "");
    eprintln!(
        "{:<20} {:>8} {:>8} {:>8}",
        "TOTAL", total_diff_by_1, extra_in_actual, missing_in_actual
    );
}

#[test]
#[ignore = "manual debug emit investigation"]
fn count_passing() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    let mut passed = 0;
    let mut failed = 0;
    let mut no_baseline = 0;
    let mut panics = 0;
    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed {
            passed += 1;
        } else if !result.baseline_exists {
            no_baseline += 1;
        } else if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            panics += 1;
        } else {
            failed += 1;
        }
    }
    let total = passed + failed + panics + no_baseline;
    eprintln!(
        "\nPassed: {}/{} ({:.1}%)",
        passed,
        total,
        passed as f64 / total as f64 * 100.0
    );
    eprintln!(
        "Failed: {}, Panics: {}, No baseline: {}",
        failed, panics, no_baseline
    );
}

#[test]
#[ignore = "manual debug emit investigation"]
fn check_specific() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    // Quick: find the most common first-mismatch patterns across ALL failures
    let root2 = root.clone();
    let runner2 = tsc_rs_harness::BaselineRunner::new(root2);
    let mut first_mismatch_patterns: HashMap<String, usize> = HashMap::new();
    let entries2: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                // Categorize the first mismatch
                let exp_t = exp_lines[i].trim();
                let act_t = act_lines[i].trim();
                let pattern = if exp_t == act_t {
                    "WHITESPACE-ONLY".to_string()
                } else if act_t.is_empty() {
                    format!("ACT-BLANK (exp: {})", categorize_line(exp_lines[i]))
                } else if exp_t.is_empty() {
                    format!("EXP-BLANK (act: {})", categorize_line(act_lines[i]))
                } else if act_t == "=;" || act_t == "= ;" {
                    "BOGUS-EQUALS-SEMI".to_string()
                } else if act_t.starts_with("export {}") && !exp_t.starts_with("export {}") {
                    "SPURIOUS-EXPORT-EMPTY".to_string()
                } else if exp_t.starts_with("Object.defineProperty")
                    && !act_t.starts_with("Object.defineProperty")
                {
                    "MISSING-OBJECT-DEFINE".to_string()
                } else if exp_t.starts_with("//") && !act_t.starts_with("//") {
                    "MISSING-COMMENT".to_string()
                } else if act_t.starts_with("//") && !exp_t.starts_with("//") {
                    "EXTRA-COMMENT".to_string()
                } else if exp_t.contains("exports.") && !act_t.contains("exports.") {
                    "MISSING-EXPORTS-DOT".to_string()
                } else if act_t.contains("exports.") && !exp_t.contains("exports.") {
                    "EXTRA-EXPORTS-DOT".to_string()
                } else if exp_t.starts_with("\"use strict\"")
                    && !act_t.starts_with("\"use strict\"")
                {
                    "MISSING-USE-STRICT".to_string()
                } else if act_t.starts_with("\"use strict\"")
                    && !exp_t.starts_with("\"use strict\"")
                {
                    "EXTRA-USE-STRICT".to_string()
                } else if exp_t.starts_with("//// [") && act_t.starts_with("//// [") {
                    "WRONG-FILE-HEADER".to_string()
                } else if exp_t.starts_with("//// [") || act_t.starts_with("//// [") {
                    "FILE-HEADER-MISMATCH".to_string()
                } else {
                    "CODE-DIFF".to_string()
                };
                *first_mismatch_patterns.entry(pattern).or_insert(0) += 1;
                break;
            }
        }
        if exp_lines.len() != act_lines.len()
            && exp_lines.iter().zip(act_lines.iter()).all(|(e, a)| e == a)
        {
            // Lines match but different count - extra/missing at end
            if act_lines.len() > exp_lines.len() {
                *first_mismatch_patterns
                    .entry("EXTRA-LINES-AT-END".to_string())
                    .or_insert(0) += 1;
            } else {
                *first_mismatch_patterns
                    .entry("MISSING-LINES-AT-END".to_string())
                    .or_insert(0) += 1;
            }
        }
    }
    let mut sorted_patterns: Vec<_> = first_mismatch_patterns.iter().collect();
    sorted_patterns.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!("\n============================================================");
    eprintln!(
        "FIRST-MISMATCH PATTERN ANALYSIS (all {} failures)",
        sorted_patterns.iter().map(|(_, c)| *c).sum::<usize>()
    );
    eprintln!("============================================================");
    for (pattern, count) in &sorted_patterns {
        eprintln!("  {:>5} {}", count, pattern);
    }
    eprintln!("============================================================\n");

    // Investigate MISSING-LINES-AT-END pattern
    let mut missing_end_samples: Vec<(String, usize, Vec<String>)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        if act_lines.len() >= exp_lines.len() {
            continue;
        }
        // Check if all actual lines match expected (truncated at end)
        let all_match = act_lines.iter().zip(exp_lines.iter()).all(|(a, e)| a == e);
        if !all_match {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let missing_count = exp_lines.len() - act_lines.len();
        let missing_lines: Vec<String> = exp_lines[act_lines.len()..]
            .iter()
            .map(|l| l.to_string())
            .collect();
        missing_end_samples.push((name, missing_count, missing_lines));
    }
    missing_end_samples.sort_by_key(|(_, c, _)| *c);
    eprintln!("\n============================================================");
    eprintln!("MISSING-LINES-AT-END: {} tests", missing_end_samples.len());
    eprintln!("============================================================");
    // Group by what the first missing line looks like
    let mut first_missing_cats: HashMap<String, usize> = HashMap::new();
    for (_, _, lines) in &missing_end_samples {
        if let Some(first) = lines.first() {
            let cat = if first.trim().is_empty() {
                "BLANK".to_string()
            } else if first.trim().starts_with("//// [") {
                "FILE-SECTION-HEADER".to_string()
            } else if first.trim().starts_with("//") {
                "COMMENT".to_string()
            } else if first.trim().contains("use strict") {
                "USE-STRICT".to_string()
            } else if first.trim().starts_with("Object.defineProperty") {
                "OBJECT-DEFINE".to_string()
            } else if first.trim().starts_with("exports.")
                || first.trim().starts_with("module.exports")
            {
                "EXPORTS".to_string()
            } else {
                format!("CODE: {}", &first.trim()[..first.trim().len().min(60)])
            };
            *first_missing_cats.entry(cat).or_insert(0) += 1;
        }
    }
    let mut sorted_cats: Vec<_> = first_missing_cats.iter().collect();
    sorted_cats.sort_by(|a, b| b.1.cmp(a.1));
    for (cat, count) in &sorted_cats {
        eprintln!("  {:>5} {}", count, cat);
    }
    // Show samples with ALL missing lines
    eprintln!("\nSamples (showing ALL missing lines):");
    for (name, count, lines) in missing_end_samples.iter().take(15) {
        eprintln!("  {} (missing {} lines):", name, count);
        for (j, l) in lines.iter().enumerate().take(5) {
            eprintln!("    L{}: {:?}", j + 1, &l[..l.len().min(100)]);
        }
    }
    // Show BLANK specifically - what content follows the blank?
    eprintln!("\nBLANK-first samples (what follows the blank?):");
    let blank_samples: Vec<_> = missing_end_samples
        .iter()
        .filter(|(_, _, lines)| lines.first().map(|l| l.trim().is_empty()).unwrap_or(false))
        .collect();
    for (name, count, lines) in blank_samples.iter().take(15) {
        let second = lines.get(1).map(|l| l.trim()).unwrap_or("<none>");
        eprintln!(
            "  {} (missing {} lines): blank + {:?}",
            name,
            count,
            &second[..second.len().min(80)]
        );
    }

    // Investigate WRONG-FILE-HEADER pattern
    let mut header_mismatch_samples: Vec<(String, String, String)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let et = exp_lines[i].trim();
                let at = act_lines[i].trim();
                if et.starts_with("//// [") && at.starts_with("//// [") {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    header_mismatch_samples.push((name, et.to_string(), at.to_string()));
                }
                break;
            }
        }
    }
    eprintln!("\n============================================================");
    eprintln!("WRONG-FILE-HEADER: {} tests", header_mismatch_samples.len());
    eprintln!("============================================================");
    // Categorize types of header mismatches
    let mut mismatch_types: HashMap<String, usize> = HashMap::new();
    for (_, exp, act) in &header_mismatch_samples {
        let pattern = if exp.contains(".d.ts") && !act.contains(".d.ts") {
            "exp=.d.ts, act=.js".to_string()
        } else if !exp.contains(".d.ts") && act.contains(".d.ts") {
            "exp=.js, act=.d.ts".to_string()
        } else if exp.contains(".js") && act.contains(".js") {
            let exp_name: &str = exp.trim_start_matches("//// [").trim_end_matches(']');
            let act_name: &str = act.trim_start_matches("//// [").trim_end_matches(']');
            format!("both .js: {} vs {}", exp_name, act_name)
        } else {
            format!("{} vs {}", exp, act)
        };
        *mismatch_types.entry(pattern).or_insert(0) += 1;
    }
    let mut sorted_types: Vec<_> = mismatch_types.iter().collect();
    sorted_types.sort_by(|a, b| b.1.cmp(a.1));
    for (t, c) in sorted_types.iter().take(20) {
        eprintln!("  {:>5} {}", c, t);
    }
    // Show specific samples
    eprintln!("\nSamples:");
    for (name, exp, act) in header_mismatch_samples.iter().take(10) {
        eprintln!("  {}: exp={:?} act={:?}", name, exp, act);
    }

    // Investigate WHITESPACE-ONLY failures
    let mut ws_only_samples: Vec<(String, usize, String, String)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let et = exp_lines[i].trim();
                let at = act_lines[i].trim();
                if et == at {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    ws_only_samples.push((
                        name,
                        i + 1,
                        exp_lines[i].to_string(),
                        act_lines[i].to_string(),
                    ));
                }
                break;
            }
        }
    }
    eprintln!("\n============================================================");
    eprintln!("WHITESPACE-ONLY: {} tests", ws_only_samples.len());
    eprintln!("============================================================");
    // Show 15 samples
    for (name, line, exp, act) in ws_only_samples.iter().take(15) {
        let exp_ws = exp.len() - exp.trim_start().len();
        let act_ws = act.len() - act.trim_start().len();
        eprintln!(
            "  {} L{}: exp_indent={} act_indent={} content={:?}",
            name,
            line,
            exp_ws,
            act_ws,
            &exp.trim()[..exp.trim().len().min(60)]
        );
    }

    // Investigate BOGUS-EQUALS-SEMI failures
    let mut bogus_eq_samples: Vec<(String, usize, String, String)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let at = act_lines[i].trim();
                if at == "=;" || at == "= ;" {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    bogus_eq_samples.push((
                        name,
                        i + 1,
                        exp_lines[i].to_string(),
                        act_lines[i].to_string(),
                    ));
                }
                break;
            }
        }
    }
    eprintln!("\n============================================================");
    eprintln!("BOGUS-EQUALS-SEMI: {} tests", bogus_eq_samples.len());
    eprintln!("============================================================");
    // Group by what the expected line looks like
    let mut exp_patterns: HashMap<String, usize> = HashMap::new();
    for (_, _, exp, _) in &bogus_eq_samples {
        let et = exp.trim();
        let pattern = if et.starts_with("Object.defineProperty") {
            "Object.defineProperty(exports, \"__esModule\", ...)".to_string()
        } else if et.starts_with("define(") || et.starts_with("define([") {
            "define(...) AMD wrapper".to_string()
        } else if et.contains("exports") {
            format!("exports-related: {}", &et[..et.len().min(60)])
        } else {
            format!("other: {}", &et[..et.len().min(60)])
        };
        *exp_patterns.entry(pattern).or_insert(0) += 1;
    }
    let mut sorted_ep: Vec<_> = exp_patterns.iter().collect();
    sorted_ep.sort_by(|a, b| b.1.cmp(a.1));
    for (p, c) in &sorted_ep {
        eprintln!("  {:>5} {}", c, p);
    }
    eprintln!("\nSamples:");
    for (name, line, exp, _) in bogus_eq_samples.iter().take(10) {
        eprintln!(
            "  {} L{}: exp={:?}",
            name,
            line,
            &exp.trim()[..exp.trim().len().min(80)]
        );
    }

    // Investigate MISSING-OBJECT-DEFINE and SPURIOUS-EXPORT-EMPTY
    let mut missing_od: Vec<(String, String)> = Vec::new();
    let mut spurious_export: Vec<(String, String, String)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let et = exp_lines[i].trim();
                let at = act_lines[i].trim();
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                if et.starts_with("Object.defineProperty")
                    && !at.starts_with("Object.defineProperty")
                {
                    missing_od.push((name.clone(), at.to_string()));
                }
                if at.starts_with("export {}") && !et.starts_with("export {}") {
                    spurious_export.push((name, et.to_string(), at.to_string()));
                }
                break;
            }
        }
    }
    eprintln!("\n== MISSING-OBJECT-DEFINE: {} tests ==", missing_od.len());
    // Group by what actual has instead
    let mut od_replacements: HashMap<String, usize> = HashMap::new();
    for (_, actual) in &missing_od {
        let at = actual.trim();
        let cat = if at.is_empty() {
            "BLANK"
        } else if at == "=;" {
            "BOGUS-=;"
        } else if at.starts_with("\"use strict\"") {
            "USE-STRICT"
        } else if at.starts_with("const ") {
            "CONST"
        } else if at.starts_with("exports.") {
            "EXPORTS-DOT"
        } else {
            "OTHER"
        };
        *od_replacements.entry(cat.to_string()).or_insert(0) += 1;
    }
    for (k, v) in &od_replacements {
        eprintln!("  {:>5} {}", v, k);
    }
    eprintln!(
        "\n== SPURIOUS-EXPORT-EMPTY: {} tests ==",
        spurious_export.len()
    );
    // Group by what expected has instead
    let mut se_expected: HashMap<String, usize> = HashMap::new();
    for (_, exp, _) in &spurious_export {
        let et = exp.trim();
        let cat = if et.is_empty() {
            "BLANK/EOF"
        } else if et.starts_with("module.exports") {
            "module.exports"
        } else if et.starts_with("var ") {
            "VAR"
        } else if et.starts_with("const ") {
            "CONST"
        } else if et.starts_with("//") {
            "COMMENT"
        } else {
            &et[..et.len().min(40)]
        };
        *se_expected.entry(cat.to_string()).or_insert(0) += 1;
    }
    for (k, v) in &se_expected {
        eprintln!("  {:>5} {}", v, k);
    }

    // Investigate remaining SPURIOUS-EXPORT-EMPTY tests
    let mut se_tests: Vec<(String, String, String)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let at = act_lines[i].trim();
                if at.starts_with("export {}") && !exp_lines[i].trim().starts_with("export {}") {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    // Get the module option from the test
                    let source = std::fs::read_to_string(&path).unwrap_or_default();
                    let module_opt = source
                        .lines()
                        .find(|l| l.to_lowercase().contains("@module"))
                        .unwrap_or("(none)")
                        .to_string();
                    se_tests.push((name, module_opt, exp_lines[i].trim().to_string()));
                }
                break;
            }
        }
    }
    eprintln!("\n== SPURIOUS-EXPORT-EMPTY by module type ==");
    let mut mod_types: HashMap<String, usize> = HashMap::new();
    for (_, opt, _) in &se_tests {
        *mod_types.entry(opt.clone()).or_insert(0) += 1;
    }
    let mut sorted_mods: Vec<_> = mod_types.iter().collect();
    sorted_mods.sort_by(|a, b| b.1.cmp(a.1));
    for (k, v) in &sorted_mods {
        eprintln!("  {:>5} {}", v, k);
    }
    eprintln!("\nSamples:");
    for (name, opt, exp) in se_tests.iter().take(10) {
        eprintln!(
            "  {}: module={}, exp={:?}",
            name,
            opt,
            &exp[..exp.len().min(60)]
        );
    }

    // Find tests with 0 line diff (same line count) that still fail
    let mut zero_diff_fails: Vec<(String, usize, String, String)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        if exp_lines.len() != act_lines.len() {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        for i in 0..exp_lines.len() {
            if exp_lines[i] != act_lines[i] {
                zero_diff_fails.push((
                    name,
                    i + 1,
                    exp_lines[i].to_string(),
                    act_lines[i].to_string(),
                ));
                break;
            }
        }
    }
    eprintln!(
        "\n== ZERO-DIFF FAILURES: {} tests ==",
        zero_diff_fails.len()
    );
    // Categorize the first mismatch
    let mut zd_cats: HashMap<String, usize> = HashMap::new();
    for (_, _, exp, act) in &zero_diff_fails {
        let et = exp.trim();
        let at = act.trim();
        let cat = if et == at {
            "WHITESPACE-ONLY".to_string()
        } else if et.is_empty() || at.is_empty() {
            "BLANK-MISMATCH".to_string()
        } else if et.contains("//") || at.contains("//") {
            "COMMENT-DIFF".to_string()
        } else {
            "CODE-DIFF".to_string()
        };
        *zd_cats.entry(cat).or_insert(0) += 1;
    }
    let mut sorted_zd: Vec<_> = zd_cats.iter().collect();
    sorted_zd.sort_by(|a, b| b.1.cmp(a.1));
    for (k, v) in &sorted_zd {
        eprintln!("  {:>5} {}", v, k);
    }
    // Show 10 WHITESPACE-ONLY samples
    eprintln!("\nWHITESPACE-ONLY samples:");
    let mut ws_count = 0;
    for (name, line, exp, act) in &zero_diff_fails {
        let et = exp.trim();
        let at = act.trim();
        if et == at {
            if ws_count < 10 {
                let exp_ws = exp.len() - exp.trim_start().len();
                let act_ws = act.len() - act.trim_start().len();
                eprintln!(
                    "  {} L{}: exp_indent={} act_indent={}",
                    name, line, exp_ws, act_ws
                );
            }
            ws_count += 1;
        }
    }

    // Find tests where exactly 1 line differs (same line count, only first mismatch differs)
    let mut single_line_diffs: Vec<(String, usize, String, String)> = Vec::new();
    for entry in &entries2 {
        let path = entry.path();
        let result = runner2.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        if exp_lines.len() != act_lines.len() {
            continue;
        }
        // Count differing lines
        let mut diff_count = 0;
        let mut first_diff = None;
        for i in 0..exp_lines.len() {
            if exp_lines[i] != act_lines[i] {
                diff_count += 1;
                if first_diff.is_none() {
                    first_diff = Some(i);
                }
            }
        }
        if diff_count == 1 {
            if let Some(idx) = first_diff {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                single_line_diffs.push((
                    name,
                    idx + 1,
                    exp_lines[idx].to_string(),
                    act_lines[idx].to_string(),
                ));
            }
        }
    }
    eprintln!(
        "\n== SINGLE-LINE-DIFF tests ({}) ==",
        single_line_diffs.len()
    );
    // Categorize them
    let mut sld_cats: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
    for (name, _line, exp, act) in &single_line_diffs {
        let et = exp.trim();
        let at = act.trim();
        let cat = if et == at {
            "INDENT-ONLY"
        } else if et.replace(' ', "") == at.replace(' ', "") {
            "WHITESPACE"
        } else if et.contains("//") || at.contains("//") || et.contains("/*") || at.contains("/*") {
            "COMMENT"
        } else if et.contains("exports.") && !at.contains("exports.") {
            "MISSING-EXPORTS-DOT"
        } else if at.contains("exports.") && !et.contains("exports.") {
            "EXTRA-EXPORTS-DOT"
        } else if et.contains("export ") != at.contains("export ") {
            "EXPORT-KEYWORD"
        } else if et.contains("require(") || at.contains("require(") {
            "REQUIRE"
        } else if (et.starts_with("var ") && at.starts_with("let "))
            || (et.starts_with("let ") && at.starts_with("var "))
        {
            "VAR-LET"
        } else if et.contains("function ") && at.contains("=>") {
            "FN-VS-ARROW"
        } else {
            "OTHER"
        };
        sld_cats.entry(cat.to_string()).or_default().push((
            name.clone(),
            et.to_string(),
            at.to_string(),
        ));
    }
    let mut sorted_sld: Vec<_> = sld_cats.iter().collect();
    sorted_sld.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (cat, samples) in &sorted_sld {
        eprintln!("  {:>3} {}", samples.len(), cat);
        for (name, exp, act) in samples.iter().take(3) {
            eprintln!("      {} EXP: {:?}", name, safe_trunc(exp, 80));
            eprintln!("               ACT: {:?}", safe_trunc(act, 80));
        }
    }
    // Show ALL indent-only tests with their indent values
    eprintln!("\n== INDENT-ONLY details ==");
    for (name, _line, exp, act) in &single_line_diffs {
        let et = exp.trim();
        let at = act.trim();
        if et == at {
            let exp_ws: String = exp.chars().take_while(|c| c.is_whitespace()).collect();
            let act_ws: String = act.chars().take_while(|c| c.is_whitespace()).collect();
            eprintln!(
                "  {} exp_ws={:?} act_ws={:?} content={:?}",
                name,
                exp_ws,
                act_ws,
                safe_trunc(et, 60)
            );
        }
    }
    // Show ALL OTHER single-line-diff tests
    eprintln!("\n== OTHER single-line-diff tests ==");
    if let Some(other_samples) = sld_cats.get("OTHER") {
        for (name, exp, act) in other_samples {
            eprintln!("  {} EXP: {:?}", name, safe_trunc(exp, 80));
            eprintln!("      ACT: {:?}", safe_trunc(act, 80));
        }
    }

    let test_names: Vec<&str> = vec![
        "typeCheckTypeArgument",
        "castFunctionExpressionShouldBeParenthesized",
        "modularizeLibrary_Dom.asynciterable",
        "throwWithoutNewLine2",
        "declarationEmitDestructuring5",
        "noImplicitAnyDestructuringInPrivateMethod",
        "declarationQuotedMembers",
        "qualify",
        "taggedTemplateWithoutDeclaredHelper",
        "regularExpressionScanning",
        "commaOperatorLeftSideUnused",
        "objectLiteralMemberWithoutBlock1",
    ];
    for tname in &test_names {
        let path = root.join(format!("tests/cases/compiler/{}.ts", tname));
        if !path.exists() {
            eprintln!("SKIP: {} not found", tname);
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let diff = act_lines.len() as i64 - exp_lines.len() as i64;
        eprintln!(
            "\n====== {} (exp:{} act:{} diff:{}) pass={} ======",
            tname,
            exp_lines.len(),
            act_lines.len(),
            diff,
            result.passed
        );
        // Show first mismatch with context
        for i in 0..std::cmp::max(exp_lines.len(), act_lines.len()) {
            let e = exp_lines.get(i).unwrap_or(&"<missing>");
            let a = act_lines.get(i).unwrap_or(&"<missing>");
            if e != a {
                let start = if i > 3 { i - 3 } else { 0 };
                let end = std::cmp::min(i + 5, std::cmp::max(exp_lines.len(), act_lines.len()));
                for j in start..end {
                    let e2 = exp_lines.get(j).unwrap_or(&"<missing>");
                    let a2 = act_lines.get(j).unwrap_or(&"<missing>");
                    let marker = if j == i { ">>>" } else { "   " };
                    if e2 == a2 {
                        eprintln!("  {} {:3}: {}", marker, j + 1, safe_trunc(e2, 120));
                    } else {
                        eprintln!("  {} {:3}: EXP: {}", marker, j + 1, safe_trunc(e2, 100));
                        eprintln!("  {} {:3}: ACT: {}", marker, j + 1, safe_trunc(a2, 100));
                    }
                }
                break;
            }
        }
    }
}

#[test]
#[ignore = "manual debug emit investigation"]
fn debug_analysis() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Collect detailed samples per category
    let mut cat_samples: HashMap<String, Vec<String>> = HashMap::new();
    let mut cat_counts: HashMap<String, usize> = HashMap::new();

    let mut same_line_total = 0;
    let mut diff_by_1 = 0;
    let mut diff_by_2 = 0;
    let mut diff_by_3_plus = 0;

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        if exp_lines.len() != act_lines.len() {
            let diff = (act_lines.len() as i64 - exp_lines.len() as i64).unsigned_abs() as usize;
            if diff == 1 {
                diff_by_1 += 1;
            } else if diff == 2 {
                diff_by_2 += 1;
            } else {
                diff_by_3_plus += 1;
            }
            continue;
        }

        same_line_total += 1;
        for (idx, (e, a)) in exp_lines.iter().zip(act_lines.iter()).enumerate() {
            if e == a {
                continue;
            }
            let et = e.trim();
            let at = a.trim();

            let cat;
            if et == at {
                cat = "INDENT-ONLY".to_string();
            } else if et.ends_with(';') && !at.ends_with(';') && format!("{};", at) == *et {
                cat = "SEMI-MISSING".to_string();
            } else if et.contains("//")
                || at.contains("//")
                || et.contains("/*")
                || at.contains("/*")
            {
                cat = "COMMENT-DIFF".to_string();
            } else if et.contains("use strict") || at.contains("use strict") {
                cat = "USE-STRICT".to_string();
            } else if et.contains("__esModule") || at.contains("__esModule") {
                cat = "ESMODULE".to_string();
            } else if et.starts_with("exports.") || at.starts_with("exports.") {
                cat = "CJS-EXPORTS".to_string();
            } else if et.contains("require(") || at.contains("require(") {
                cat = "CJS-REQUIRE".to_string();
            } else if et.contains("(function")
                || at.contains("(function")
                || et.contains("})(")
                || at.contains("})(")
            {
                cat = "NS-IIFE".to_string();
            } else if et.replace(' ', "") == at.replace(' ', "") {
                cat = "WHITESPACE".to_string();
            } else {
                cat = "OTHER".to_string();
            }
            let samples = cat_samples.entry(cat.clone()).or_insert_with(Vec::new);
            if samples.len() < 5 {
                samples.push(format!(
                    "{} L{}: E={:?} A={:?}",
                    name,
                    idx + 1,
                    safe_trunc(et, 80),
                    safe_trunc(at, 80)
                ));
            }
            *cat_counts.entry(cat).or_insert(0) += 1;
            break;
        }
    }

    eprintln!("\n=== SAME-LINE-COUNT FAILURES: {} ===", same_line_total);
    eprintln!(
        "=== DIFF-BY-1: {}, DIFF-BY-2: {}, DIFF-BY-3+: {} ===",
        diff_by_1, diff_by_2, diff_by_3_plus
    );

    let mut cats: Vec<_> = cat_counts.iter().collect();
    cats.sort_by(|a, b| b.1.cmp(a.1));
    for (cat, count) in &cats {
        eprintln!("\n=== {} ({}) ===", cat, count);
        if let Some(samples) = cat_samples.get(*cat) {
            for s in samples {
                eprintln!("  {}", s);
            }
        }
    }

    // Collect ALL test names per category
    let mut cat_all_names: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        if exp_lines.len() != act_lines.len() {
            continue;
        }

        for (e, a) in exp_lines.iter().zip(act_lines.iter()) {
            if e == a {
                continue;
            }
            let et = e.trim();
            let at = a.trim();

            let cat;
            if et == at {
                cat = "INDENT-ONLY".to_string();
            } else if et.ends_with(';') && !at.ends_with(';') && format!("{};", at) == *et {
                cat = "SEMI-MISSING".to_string();
            } else if et.contains("//")
                || at.contains("//")
                || et.contains("/*")
                || at.contains("/*")
            {
                cat = "COMMENT-DIFF".to_string();
            } else if et.contains("use strict") || at.contains("use strict") {
                cat = "USE-STRICT".to_string();
            } else if et.contains("__esModule") || at.contains("__esModule") {
                cat = "ESMODULE".to_string();
            } else if et.starts_with("exports.") || at.starts_with("exports.") {
                cat = "CJS-EXPORTS".to_string();
            } else if et.contains("require(") || at.contains("require(") {
                cat = "CJS-REQUIRE".to_string();
            } else if et.contains("(function")
                || at.contains("(function")
                || et.contains("})(")
                || at.contains("})(")
            {
                cat = "NS-IIFE".to_string();
            } else if et.replace(' ', "") == at.replace(' ', "") {
                cat = "WHITESPACE".to_string();
            } else {
                cat = "OTHER".to_string();
            }
            cat_all_names
                .entry(cat)
                .or_insert_with(Vec::new)
                .push(name.clone());
            break;
        }
    }

    eprintln!(
        "\n=== ALL OTHER TEST NAMES ({}) ===",
        cat_all_names.get("OTHER").map(|v| v.len()).unwrap_or(0)
    );
    if let Some(names) = cat_all_names.get("OTHER") {
        let mut sorted_names = names.clone();
        sorted_names.sort();
        for n in &sorted_names {
            eprintln!("  {}", n);
        }
    }
}

#[test]
#[ignore = "manual debug emit investigation"]
fn comment_diff_analysis() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Category key: (direction, has_remove_comments, comment_type)
    #[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
    struct CatKey {
        direction: String,         // "EXTRA" or "MISSING"
        has_remove_comments: bool, // test source contains "removeComments"
        comment_type: String,      // "REFERENCE-DIRECTIVE", "SINGLE-LINE", "BLOCK"
    }

    impl std::fmt::Display for CatKey {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{} | removeComments={} | {}",
                self.direction,
                if self.has_remove_comments {
                    "YES"
                } else {
                    "NO"
                },
                self.comment_type
            )
        }
    }

    // Sample: (test_name, line_content, line_number)
    let mut categories: HashMap<CatKey, Vec<(String, String, usize)>> = HashMap::new();

    let mut total_comment_diff_by_1 = 0;
    let mut total_diff_by_1 = 0;

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        let len_diff = act_lines.len() as i64 - exp_lines.len() as i64;
        if len_diff.unsigned_abs() != 1 {
            continue;
        }

        total_diff_by_1 += 1;

        // Find the differing line
        let (direction, diff_line, diff_line_num) = if len_diff == 1 {
            // actual has one extra line -- find it
            let mut found_line: Option<(String, usize)> = None;
            let mut ai = 0;
            let mut ei = 0;
            while ai < act_lines.len() && ei < exp_lines.len() {
                if act_lines[ai] == exp_lines[ei] {
                    ai += 1;
                    ei += 1;
                } else {
                    found_line = Some((act_lines[ai].to_string(), ai + 1));
                    break;
                }
            }
            let (line, num) = found_line
                .unwrap_or_else(|| (act_lines[act_lines.len() - 1].to_string(), act_lines.len()));
            ("EXTRA".to_string(), line, num)
        } else {
            // expected has one extra line (actual is missing one)
            let mut found_line: Option<(String, usize)> = None;
            let mut ai = 0;
            let mut ei = 0;
            while ai < act_lines.len() && ei < exp_lines.len() {
                if act_lines[ai] == exp_lines[ei] {
                    ai += 1;
                    ei += 1;
                } else {
                    found_line = Some((exp_lines[ei].to_string(), ei + 1));
                    break;
                }
            }
            let (line, num) = found_line
                .unwrap_or_else(|| (exp_lines[exp_lines.len() - 1].to_string(), exp_lines.len()));
            ("MISSING".to_string(), line, num)
        };

        // Filter: only keep lines that contain "//" or "/*"
        let trimmed = diff_line.trim();
        if !trimmed.contains("//") && !trimmed.contains("/*") {
            continue;
        }

        total_comment_diff_by_1 += 1;

        // Check if the test file source contains "removeComments"
        let test_source = std::fs::read_to_string(&path).unwrap_or_default();
        let has_remove_comments = test_source.contains("removeComments");

        // Determine comment type
        let comment_type = if trimmed.starts_with("///") {
            "REFERENCE-DIRECTIVE".to_string()
        } else if trimmed.contains("/*") {
            "BLOCK".to_string()
        } else {
            "SINGLE-LINE".to_string()
        };

        let key = CatKey {
            direction,
            has_remove_comments,
            comment_type,
        };

        categories
            .entry(key)
            .or_insert_with(Vec::new)
            .push((name, diff_line, diff_line_num));
    }

    eprintln!("\n============================================================");
    eprintln!("COMMENT DIFF-BY-1 ANALYSIS");
    eprintln!("============================================================");
    eprintln!("Total diff-by-1 failures:         {}", total_diff_by_1);
    eprintln!(
        "Comment-related diff-by-1:        {}",
        total_comment_diff_by_1
    );
    eprintln!("============================================================\n");

    // High-level direction breakdown
    let total_extra: usize = categories
        .iter()
        .filter(|(k, _)| k.direction == "EXTRA")
        .map(|(_, v)| v.len())
        .sum();
    let total_missing: usize = categories
        .iter()
        .filter(|(k, _)| k.direction == "MISSING")
        .map(|(_, v)| v.len())
        .sum();
    eprintln!(
        "  EXTRA in actual (emitter produces comment that shouldn't be there):  {}",
        total_extra
    );
    eprintln!(
        "  MISSING in actual (emitter omits comment that should be there):      {}",
        total_missing
    );
    eprintln!();

    // removeComments breakdown
    let with_remove: usize = categories
        .iter()
        .filter(|(k, _)| k.has_remove_comments)
        .map(|(_, v)| v.len())
        .sum();
    let without_remove: usize = categories
        .iter()
        .filter(|(k, _)| !k.has_remove_comments)
        .map(|(_, v)| v.len())
        .sum();
    eprintln!("  With @removeComments:    {}", with_remove);
    eprintln!("  Without @removeComments: {}", without_remove);
    eprintln!();

    // Comment type breakdown
    let ref_dir: usize = categories
        .iter()
        .filter(|(k, _)| k.comment_type == "REFERENCE-DIRECTIVE")
        .map(|(_, v)| v.len())
        .sum();
    let single_line: usize = categories
        .iter()
        .filter(|(k, _)| k.comment_type == "SINGLE-LINE")
        .map(|(_, v)| v.len())
        .sum();
    let block: usize = categories
        .iter()
        .filter(|(k, _)| k.comment_type == "BLOCK")
        .map(|(_, v)| v.len())
        .sum();
    eprintln!("  Reference directives (///):  {}", ref_dir);
    eprintln!("  Single-line (//) comments:   {}", single_line);
    eprintln!("  Block (/* */) comments:      {}", block);

    eprintln!("\n============================================================");
    eprintln!("DETAILED CATEGORIES (sorted by count desc)");
    eprintln!("============================================================\n");

    let mut sorted_cats: Vec<_> = categories.into_iter().collect();
    sorted_cats.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));

    for (key, samples) in &sorted_cats {
        eprintln!("=== {} (count: {}) ===", key, samples.len());
        for (i, (test_name, line_content, line_num)) in samples.iter().enumerate() {
            if i >= 5 {
                break;
            }
            eprintln!(
                "  [{}] {} L{}: {:?}",
                i + 1,
                test_name,
                line_num,
                safe_trunc(line_content, 120)
            );
        }
        if samples.len() > 5 {
            eprintln!("  ... and {} more", samples.len() - 5);
        }
        eprintln!();
    }

    eprintln!("============================================================");
    eprintln!("SUMMARY TABLE");
    eprintln!("============================================================");
    eprintln!("{:<45} {:>6}", "Category", "Count");
    eprintln!("{:-<45} {:-^6}", "", "");
    for (key, samples) in &sorted_cats {
        eprintln!("{:<45} {:>6}", format!("{}", key), samples.len());
    }
    eprintln!("{:-<45} {:-^6}", "", "");
    eprintln!(
        "{:<45} {:>6}",
        "TOTAL COMMENT DIFF-BY-1", total_comment_diff_by_1
    );
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diff_by_2_analysis() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // -----------------------------------------------------------------------
    // Simple LCS-based diff to find exactly which lines differ
    // -----------------------------------------------------------------------
    #[derive(Debug, Clone)]
    enum DiffLine<'a> {
        Same,
        Extra(&'a str),
        Missing(&'a str),
    }

    fn lcs_diff<'a>(expected: &[&'a str], actual: &[&'a str]) -> Vec<DiffLine<'a>> {
        let n = expected.len();
        let m = actual.len();
        let mut dp = vec![vec![0u32; m + 1]; n + 1];
        for i in 1..=n {
            for j in 1..=m {
                if expected[i - 1] == actual[j - 1] {
                    dp[i][j] = dp[i - 1][j - 1] + 1;
                } else {
                    dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
                }
            }
        }
        let mut result = Vec::new();
        let mut i = n;
        let mut j = m;
        while i > 0 || j > 0 {
            if i > 0 && j > 0 && expected[i - 1] == actual[j - 1] {
                result.push(DiffLine::Same);
                i -= 1;
                j -= 1;
            } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
                result.push(DiffLine::Extra(actual[j - 1]));
                j -= 1;
            } else {
                result.push(DiffLine::Missing(expected[i - 1]));
                i -= 1;
            }
        }
        result.reverse();
        result
    }

    // -----------------------------------------------------------------------
    // Content classification
    // -----------------------------------------------------------------------
    fn classify_content(line: &str) -> &'static str {
        let t = line.trim();
        if t.is_empty() {
            "BLANK"
        } else if t.contains("use strict") {
            "USE-STRICT"
        } else if t.starts_with("//")
            || t.starts_with("/*")
            || t.starts_with("*")
            || t.ends_with("*/")
        {
            "COMMENT"
        } else if t.contains("exports.")
            || t.contains("module.exports")
            || t.contains("__exportStar")
            || t.starts_with("export ")
            || t.starts_with("export{")
        {
            "EXPORT"
        } else if t.contains("require(") || t.starts_with("import ") {
            "IMPORT"
        } else if t.contains("Object.defineProperty") {
            "OBJECT-DEFINE-PROP"
        } else if t.contains("(function") || t.contains("})(") || t.contains("|| (") {
            "NAMESPACE-IIFE"
        } else if t == ";" {
            "SEMICOLON-ONLY"
        } else if t == "}" || t == "});" || t == "})();" {
            "CLOSING-BRACE"
        } else if t.starts_with("var ") || t.starts_with("let ") || t.starts_with("const ") {
            "VAR-DECL"
        } else if t.starts_with("\"use strict\"") || t == "\"use strict\";" {
            "USE-STRICT"
        } else {
            "OTHER"
        }
    }

    // -----------------------------------------------------------------------
    // Pattern key: structural shape + content categories
    // -----------------------------------------------------------------------
    fn make_pattern_label(extras: &[&str], missings: &[&str]) -> String {
        let mut parts = Vec::new();
        for m in missings {
            parts.push(format!("-{}", classify_content(m)));
        }
        for e in extras {
            parts.push(format!("+{}", classify_content(e)));
        }
        parts.sort();

        let shape = if extras.len() == 2 && missings.is_empty() {
            "BOTH-EXTRA"
        } else if missings.len() == 2 && extras.is_empty() {
            "BOTH-MISSING"
        } else if extras.len() >= 1 && missings.len() >= 1 {
            "EXTRA+MISSING"
        } else {
            "OTHER"
        };

        format!("{} [{}]", shape, parts.join(", "))
    }

    // Sample data
    struct Sample {
        test_name: String,
        extra_lines: Vec<String>,
        missing_lines: Vec<String>,
    }

    // -----------------------------------------------------------------------
    // Main loop
    // -----------------------------------------------------------------------
    let mut pattern_map: HashMap<String, Vec<Sample>> = HashMap::new();
    let mut total_diff_by_2 = 0;
    let mut skipped_large = 0;
    let mut skipped_complex_diff = 0;

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        let len_diff = (act_lines.len() as i64 - exp_lines.len() as i64).unsigned_abs() as usize;
        if len_diff != 2 {
            continue;
        }

        total_diff_by_2 += 1;

        // Skip very large files to keep LCS tractable
        if exp_lines.len() > 4000 || act_lines.len() > 4000 {
            skipped_large += 1;
            continue;
        }

        let diff = lcs_diff(&exp_lines, &act_lines);

        let extras: Vec<&str> = diff
            .iter()
            .filter_map(|d| match d {
                DiffLine::Extra(line) => Some(*line),
                _ => None,
            })
            .collect();
        let missings: Vec<&str> = diff
            .iter()
            .filter_map(|d| match d {
                DiffLine::Missing(line) => Some(*line),
                _ => None,
            })
            .collect();

        // For a pure diff-by-2 we expect total diffs <= 6
        // (LCS can find "replacement" pairs beyond the 2 shifted lines)
        if extras.len() + missings.len() > 8 {
            skipped_complex_diff += 1;
            continue;
        }

        let label = make_pattern_label(&extras, &missings);
        pattern_map
            .entry(label)
            .or_insert_with(Vec::new)
            .push(Sample {
                test_name: name,
                extra_lines: extras.iter().map(|s| s.to_string()).collect(),
                missing_lines: missings.iter().map(|s| s.to_string()).collect(),
            });
    }

    // -----------------------------------------------------------------------
    // Output
    // -----------------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!("DIFF-BY-2 FAILURE PATTERN ANALYSIS");
    eprintln!("================================================================");
    eprintln!("Total diff-by-2 failures:         {}", total_diff_by_2);
    eprintln!("Skipped (too large for LCS):      {}", skipped_large);
    eprintln!("Skipped (complex diff >8 hunks):  {}", skipped_complex_diff);
    let analyzed: usize = pattern_map.values().map(|v| v.len()).sum();
    eprintln!("Successfully analyzed:            {}", analyzed);
    eprintln!("Distinct pattern types:           {}", pattern_map.len());
    eprintln!("================================================================\n");

    // Sort by frequency
    let mut sorted: Vec<_> = pattern_map.iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    // High-level structural breakdown
    let both_extra: usize = sorted
        .iter()
        .filter(|(k, _)| k.starts_with("BOTH-EXTRA"))
        .map(|(_, v)| v.len())
        .sum();
    let both_missing: usize = sorted
        .iter()
        .filter(|(k, _)| k.starts_with("BOTH-MISSING"))
        .map(|(_, v)| v.len())
        .sum();
    let extra_missing: usize = sorted
        .iter()
        .filter(|(k, _)| k.starts_with("EXTRA+MISSING"))
        .map(|(_, v)| v.len())
        .sum();

    eprintln!("--- STRUCTURAL BREAKDOWN ---");
    eprintln!(
        "  BOTH-EXTRA (actual has 2 extra lines):     {:>4}",
        both_extra
    );
    eprintln!(
        "  BOTH-MISSING (actual missing 2 lines):     {:>4}",
        both_missing
    );
    eprintln!(
        "  EXTRA+MISSING (line transformations):       {:>4}",
        extra_missing
    );
    eprintln!();

    eprintln!("================================================================");
    eprintln!("DETAILED PATTERNS (sorted by frequency, with samples)");
    eprintln!("================================================================\n");

    let mut total_samples_shown = 0;
    for (rank, (label, samples)) in sorted.iter().enumerate() {
        eprintln!("--- Pattern #{} | count={} ---", rank + 1, samples.len());
        eprintln!("  {}", label);
        eprintln!();

        let max_show = if rank < 3 {
            5
        } else if rank < 10 {
            3
        } else {
            1
        };
        for (j, s) in samples.iter().enumerate() {
            if j >= max_show {
                break;
            }
            total_samples_shown += 1;
            eprintln!("  [{}] Test: {}", j + 1, s.test_name);
            for line in &s.missing_lines {
                let display = if line.len() > 130 {
                    &line[..130]
                } else {
                    line.as_str()
                };
                eprintln!("       - {:?}", display);
            }
            for line in &s.extra_lines {
                let display = if line.len() > 130 {
                    &line[..130]
                } else {
                    line.as_str()
                };
                eprintln!("       + {:?}", display);
            }
            eprintln!();
        }
        if samples.len() > max_show {
            eprintln!(
                "  ... and {} more tests with this pattern\n",
                samples.len() - max_show
            );
        }
    }

    eprintln!("================================================================");
    eprintln!("SUMMARY TABLE");
    eprintln!("================================================================");
    eprintln!("{:<65} {:>6}", "Pattern", "Count");
    eprintln!("{:-<65} {:-^6}", "", "");
    for (label, samples) in &sorted {
        eprintln!("{:<65} {:>6}", label, samples.len());
    }
    eprintln!("{:-<65} {:-^6}", "", "");
    eprintln!("{:<65} {:>6}", "TOTAL ANALYZED", analyzed);
    eprintln!();
    eprintln!("Total samples shown in detail: {}", total_samples_shown);
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn refined_diff_by_1_patterns() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Detailed pattern categories
    let mut patterns: HashMap<String, Vec<String>> = HashMap::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        let len_diff = act_lines.len() as i64 - exp_lines.len() as i64;
        if len_diff.unsigned_abs() != 1 {
            continue;
        }

        // Find the divergence point (first mismatch)
        let mut first_mismatch = None;
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                first_mismatch = Some(i);
                break;
            }
        }
        // If no mismatch in overlapping lines, divergence is at the end
        let div = first_mismatch.unwrap_or(std::cmp::min(exp_lines.len(), act_lines.len()));

        let exp_line = exp_lines.get(div).unwrap_or(&"<EOF>");
        let act_line = act_lines.get(div).unwrap_or(&"<EOF>");

        let exp_trimmed = exp_line.trim();
        let act_trimmed = act_line.trim();

        let pattern;

        if len_diff == 1 {
            // Actual has 1 extra line
            if act_trimmed.is_empty()
                && (div == 0 || exp_trimmed == act_lines.get(div + 1).unwrap_or(&"").trim())
            {
                // Extra blank line inserted before next content
                pattern = "EXTRA-BLANK-LINE (actual has extra empty line)".to_string();
            } else if exp_trimmed.contains(act_trimmed)
                && !act_trimmed.is_empty()
                && act_lines
                    .get(div + 1)
                    .map(|l| exp_trimmed.contains(l.trim()))
                    .unwrap_or(false)
            {
                // Expression was split across 2 lines (line-break inserted mid-expression)
                pattern = "LINE-BREAK-SPLIT (expression split across 2 lines)".to_string();
            } else if act_trimmed == "=;" || act_trimmed.starts_with("= ") {
                // Broken destructuring/assignment: "= ;" emitted as separate line
                pattern = "BROKEN-EQUALS (stray '=;' or '=' on its own line)".to_string();
            } else if div == act_lines.len() - 1
                || (div + 1 < act_lines.len() && *exp_line == act_lines[div + 1])
            {
                // Extra line inserted, everything else shifts down by 1
                let et = act_trimmed;
                if et.is_empty() {
                    pattern = "EXTRA-BLANK-LINE (actual has extra empty line)".to_string();
                } else if et.contains("//") || et.contains("/*") {
                    pattern =
                        "EXTRA-COMMENT-LINE (emitter adds comment not in expected)".to_string();
                } else if et.contains("export") {
                    pattern = "EXTRA-EXPORT-STMT (emitter adds extra export)".to_string();
                } else if et.contains("Object.defineProperty") {
                    pattern = "EXTRA-OBJECT-DEFINE (emitter adds extra defineProperty)".to_string();
                } else if et.contains("require(") {
                    pattern = "EXTRA-REQUIRE (emitter adds extra require)".to_string();
                } else if et.contains("\"use strict\"") || et.contains("'use strict'") {
                    pattern = "EXTRA-USE-STRICT (emitter adds extra use strict)".to_string();
                } else {
                    // Check if the extra line is a piece of the expected combined line
                    let next = act_lines.get(div + 1).unwrap_or(&"");
                    let combined = format!("{}{}", act_trimmed, next.trim());
                    let combined_sp = format!("{} {}", act_trimmed, next.trim());
                    if combined == exp_trimmed.replace(' ', "")
                        || exp_trimmed == combined_sp
                        || exp_trimmed.starts_with(&combined)
                    {
                        pattern = "LINE-BREAK-SPLIT (expression split across 2 lines)".to_string();
                    } else {
                        pattern = format!("OTHER-EXTRA (line: {:?})", safe_trunc(act_trimmed, 60));
                    }
                }
            } else {
                // Complex: content differs and shift doesn't align cleanly
                // Check for line-break splitting heuristic
                let next = act_lines.get(div + 1).unwrap_or(&"");
                let combined = format!("{} {}", act_trimmed, next.trim());
                if combined.trim() == exp_trimmed || exp_trimmed.starts_with(act_trimmed) {
                    pattern = "LINE-BREAK-SPLIT (expression split across 2 lines)".to_string();
                } else {
                    pattern = format!("OTHER-EXTRA-COMPLEX");
                }
            }
        } else {
            // Actual is missing 1 line (len_diff == -1)
            if exp_trimmed.is_empty() {
                pattern = "MISSING-BLANK-LINE (expected has blank line actual lacks)".to_string();
            } else if exp_trimmed.contains("//")
                || exp_trimmed.contains("/*")
                || exp_trimmed.starts_with("///")
            {
                // Missing comment/directive
                if exp_trimmed.starts_with("///") {
                    pattern = "MISSING-REFERENCE-DIRECTIVE (/// directive stripped)".to_string();
                } else if exp_trimmed.starts_with("//") || exp_trimmed.starts_with("/*") {
                    pattern = "MISSING-STANDALONE-COMMENT (whole comment line dropped)".to_string();
                } else {
                    // inline comment: same code, but expected has trailing comment
                    let exp_no_comment = exp_trimmed.split("//").next().unwrap_or("").trim();
                    if act_trimmed.starts_with(exp_no_comment) || act_trimmed == exp_no_comment {
                        pattern =
                            "INLINE-COMMENT-STRIPPED (trailing // comment removed, merging lines)"
                                .to_string();
                    } else {
                        pattern = "MISSING-COMMENT-OTHER".to_string();
                    }
                }
            } else if exp_trimmed.contains("exports.") || exp_trimmed.contains("module.exports") {
                pattern = "MISSING-EXPORT-STMT (expected export line not emitted)".to_string();
            } else if exp_trimmed.contains("Object.defineProperty") {
                pattern = "MISSING-OBJECT-DEFINE (expected defineProperty not emitted)".to_string();
            } else if exp_trimmed.contains("require(") {
                pattern = "MISSING-REQUIRE".to_string();
            } else if exp_trimmed.contains("\"use strict\"") || exp_trimmed.contains("'use strict'")
            {
                pattern = "MISSING-USE-STRICT".to_string();
            } else if exp_trimmed.starts_with("var ")
                || exp_trimmed.starts_with("let ")
                || exp_trimmed.starts_with("const ")
            {
                // Check if it's a hoisted var declaration
                pattern = "MISSING-VAR-DECL (var/let/const declaration not emitted)".to_string();
            } else {
                // Check if actual line is a merged version of expected + next
                let exp_next = exp_lines.get(div + 1).unwrap_or(&"");
                let merged = format!("{} {}", exp_trimmed, exp_next.trim());
                if act_trimmed == merged.trim() || act_trimmed.starts_with(exp_trimmed) {
                    pattern =
                        "LINES-MERGED (two expected lines merged into one actual)".to_string();
                } else {
                    pattern = format!("OTHER-MISSING");
                }
            }
        }

        patterns.entry(pattern).or_insert_with(Vec::new).push(name);
    }

    eprintln!("\n================================================================");
    eprintln!("REFINED DIFF-BY-1 PATTERN ANALYSIS");
    eprintln!("================================================================\n");

    let mut sorted: Vec<_> = patterns.iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let total: usize = sorted.iter().map(|(_, v)| v.len()).sum();

    for (pattern, tests) in &sorted {
        eprintln!(
            "{:>4}  ({:4.1}%)  {}",
            tests.len(),
            tests.len() as f64 / total as f64 * 100.0,
            pattern
        );
        // Show first 3 test names
        for (i, t) in tests.iter().enumerate() {
            if i >= 3 {
                break;
            }
            eprintln!("             - {}", t);
        }
        if tests.len() > 3 {
            eprintln!("             ... and {} more", tests.len() - 3);
        }
    }
    eprintln!("\n{:-<60}", "");
    eprintln!("TOTAL: {}", total);
    eprintln!("================================================================");

    // Group the OTHER-EXTRA-COMPLEX ones for deeper inspection
    eprintln!("\n================================================================");
    eprintln!("DEEP DIVE: OTHER-EXTRA-COMPLEX + OTHER-MISSING");
    eprintln!("================================================================\n");

    // Re-iterate to show full diffs for complex cases
    let complex_extra_tests: Vec<String> = patterns
        .get("OTHER-EXTRA-COMPLEX")
        .map(|v| v.clone())
        .unwrap_or_default();
    let other_missing_tests: Vec<String> = patterns
        .get("OTHER-MISSING")
        .map(|v| v.clone())
        .unwrap_or_default();

    eprintln!(
        "--- OTHER-EXTRA-COMPLEX ({} tests) ---",
        complex_extra_tests.len()
    );
    for (i, tname) in complex_extra_tests.iter().enumerate() {
        if i >= 20 {
            break;
        }
        let path = root.join(format!("tests/cases/compiler/{}.ts", tname));
        if !path.exists() {
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();

        // Find mismatch
        for j in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[j] != act_lines[j] {
                let el = exp_lines[j].trim();
                let al = act_lines[j].trim();
                // Determine sub-pattern
                let sub = if al.len() < el.len() && el.starts_with(al) {
                    "EXPR-SPLIT-START"
                } else if al.len() < el.len() && el.ends_with(al) {
                    "EXPR-SPLIT-END"
                } else if el.contains(al) && !al.is_empty() {
                    "EXPR-SPLIT-PARTIAL"
                } else if al.contains("//") || al.contains("/*") {
                    "COMMENT-MISPLACED"
                } else if el.contains(&format!("{} ", al)) || el.contains(&format!(" {}", al)) {
                    "MISSING-CONCATENATION"
                } else {
                    "TRUE-CONTENT-DIFF"
                };
                eprintln!("  [{}] {} (sub: {})", i + 1, tname, sub);
                eprintln!("      EXP: {:?}", safe_trunc(el, 100));
                eprintln!("      ACT: {:?}", safe_trunc(al, 100));
                if j + 1 < act_lines.len() {
                    eprintln!(
                        "      ACT+1: {:?}",
                        safe_trunc(act_lines[j + 1].trim(), 100)
                    );
                }
                break;
            }
        }
    }

    // Count sub-patterns for OTHER-EXTRA-COMPLEX
    let mut complex_sub_patterns: HashMap<String, usize> = HashMap::new();
    for tname in &complex_extra_tests {
        let path = root.join(format!("tests/cases/compiler/{}.ts", tname));
        if !path.exists() {
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for j in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[j] != act_lines[j] {
                let el = exp_lines[j].trim();
                let al = act_lines[j].trim();
                let sub = if el.starts_with("////") && al.starts_with("////") {
                    "FILE-HEADER-MISMATCH (wrong output file name/order)"
                } else if al.len() < el.len() && el.starts_with(al) {
                    "EXPR-SPLIT (expression wrapped to next line)"
                } else if al.len() < el.len() && el.ends_with(al) {
                    "EXPR-SPLIT (expression wrapped to next line)"
                } else if el.contains(al) && !al.is_empty() && al.len() > 2 {
                    "EXPR-SPLIT (expression wrapped to next line)"
                } else if al.contains("<") && el.contains("(") && !el.contains("<") {
                    "TYPE-ARGS-NOT-ERASED (type parameters emitted in JS output)"
                } else if el.contains("Object.assign(Object.assign")
                    && al.contains("Object.assign(")
                {
                    "OBJECT-ASSIGN-NESTING (nested Object.assign not emitted correctly)"
                } else if el.starts_with("catch") && al.starts_with("catch") {
                    "CATCH-BINDING-DIFF"
                } else if el.contains("define(") && !al.contains("define(") {
                    "WRONG-MODULE-FORMAT (AMD/UMD not generated)"
                } else if al.contains("@") || al.contains("decorate") {
                    "DECORATOR-EMIT"
                } else {
                    "EMIT-CONTENT-DIFF (expression emitted differently)"
                };
                *complex_sub_patterns.entry(sub.to_string()).or_insert(0) += 1;
                break;
            }
        }
    }

    eprintln!("\n--- OTHER-EXTRA-COMPLEX sub-pattern summary ---");
    let mut sub_sorted: Vec<_> = complex_sub_patterns.iter().collect();
    sub_sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (sub, count) in &sub_sorted {
        eprintln!("  {:>4}  {}", count, sub);
    }

    eprintln!(
        "\n--- OTHER-MISSING ({} tests) ---",
        other_missing_tests.len()
    );
    for (i, tname) in other_missing_tests.iter().enumerate() {
        if i >= 15 {
            break;
        }
        let path = root.join(format!("tests/cases/compiler/{}.ts", tname));
        if !path.exists() {
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();

        for j in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[j] != act_lines[j] {
                let el = exp_lines[j].trim();
                let al = act_lines[j].trim();
                // Determine sub-pattern
                let sub = if al.contains(el) && !el.is_empty() {
                    "LINES-MERGED"
                } else if el.starts_with("this[") || el.starts_with("this.") {
                    "MISSING-THIS-PROP"
                } else if el.contains("NS.") || el.contains("module.") || el.contains("exports.") {
                    "MISSING-NAMESPACE-PROP"
                } else if el.starts_with("define(") || el.starts_with("System.") {
                    "WRONG-MODULE-WRAPPER"
                } else {
                    "TRUE-CONTENT-DIFF"
                };
                eprintln!("  [{}] {} (sub: {})", i + 1, tname, sub);
                eprintln!("      EXP: {:?}", safe_trunc(el, 100));
                eprintln!("      ACT: {:?}", safe_trunc(al, 100));
                break;
            }
        }
    }
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn other_pattern_analysis() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    let mut patterns: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        if exp_lines.len() != act_lines.len() {
            continue;
        }

        for (_idx, (e, a)) in exp_lines.iter().zip(act_lines.iter()).enumerate() {
            if e == a {
                continue;
            }
            let et = e.trim();
            let at = a.trim();

            // Replicate the exact debug_analysis filter to only get OTHER
            if et == at {
                break;
            }
            if et.ends_with(';') && !at.ends_with(';') && format!("{};", at) == *et {
                break;
            }
            if et.contains("//") || at.contains("//") || et.contains("/*") || at.contains("/*") {
                break;
            }
            if et.contains("use strict") || at.contains("use strict") {
                break;
            }
            if et.contains("__esModule") || at.contains("__esModule") {
                break;
            }
            if et.starts_with("exports.") || at.starts_with("exports.") {
                break;
            }
            if et.contains("require(") || at.contains("require(") {
                break;
            }
            if et.contains("(function")
                || at.contains("(function")
                || et.contains("})(")
                || at.contains("})(")
            {
                break;
            }
            if et.replace(' ', "") == at.replace(' ', "") {
                break;
            }

            let pattern = categorize_other_diff(et, at);
            patterns.entry(pattern).or_insert_with(Vec::new).push((
                name.clone(),
                et.to_string(),
                at.to_string(),
            ));
            break;
        }
    }

    eprintln!("\n================================================================");
    eprintln!("SAME-LINE OTHER: DETAILED PATTERN ANALYSIS");
    eprintln!("================================================================\n");

    let total: usize = patterns.values().map(|v| v.len()).sum();
    eprintln!("Total OTHER tests categorized: {}\n", total);

    let mut sorted: Vec<_> = patterns.iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (pattern, tests) in &sorted {
        let limit = if *pattern == "UNCATEGORIZED" { 60 } else { 3 };
        eprintln!(
            "{:>4}  ({:4.1}%)  {}",
            tests.len(),
            tests.len() as f64 / total as f64 * 100.0,
            pattern
        );
        for (i, (tname, exp, act)) in tests.iter().enumerate() {
            if i >= limit {
                break;
            }
            eprintln!("        [{}]  EXP: {:?}", tname, safe_trunc(exp, 90));
            eprintln!("              ACT: {:?}", safe_trunc(act, 90));
        }
        if tests.len() > limit {
            eprintln!("        ... and {} more", tests.len() - limit);
        }
        eprintln!();
    }

    eprintln!("================================================================");
    eprintln!("SUMMARY");
    eprintln!("================================================================");
    eprintln!("{:<60} {:>5} {:>6}", "Pattern", "Count", "%");
    eprintln!("{:-<60} {:-^5} {:-^6}", "", "", "");
    for (pattern, tests) in &sorted {
        eprintln!(
            "{:<60} {:>5} {:>5.1}%",
            pattern,
            tests.len(),
            tests.len() as f64 / total as f64 * 100.0
        );
    }
    eprintln!("{:-<60} {:-^5} {:-^6}", "", "", "");
    eprintln!("{:<60} {:>5}", "TOTAL", total);
}

fn categorize_other_diff(exp: &str, act: &str) -> String {
    // 1. <error> TOKEN
    if act.contains("<error>") {
        return "ERROR-TOKEN (this-param/type not erased => <error>)".to_string();
    }

    // 2. ENUM NOT FOLDED
    if exp.contains("] = ") && act.contains("] = ") && exp.contains("[") && act.contains("[") {
        fn extract_enum_val(s: &str) -> Option<&str> {
            let start = s.find("] = ")?;
            let rest = &s[start + 4..];
            let end = rest.find(']')?;
            Some(rest[..end].trim())
        }
        if let (Some(ev), Some(av)) = (extract_enum_val(exp), extract_enum_val(act)) {
            if ev != av {
                let exp_is_num = ev
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '-' || c == '.' || c == 'n');
                if exp_is_num {
                    return "ENUM-NOT-FOLDED (enum init not const-evaluated)".to_string();
                }
                // String enum value differs
                if (ev.starts_with('"') || ev.starts_with('\''))
                    && !av.starts_with('"')
                    && !av.starts_with('\'')
                {
                    return "ENUM-STRING-NOT-RESOLVED (string enum not resolved)".to_string();
                }
                return "ENUM-VALUE-DIFF (enum initializer value differs)".to_string();
            }
        }
    }

    // 3. QUOTE STYLE
    if exp.replace('\'', "\"") == act.replace('\'', "\"") {
        return "QUOTE-STYLE (single vs double quotes)".to_string();
    }

    // 4. CONSTRUCTOR PROPERTY ORDER
    if exp.starts_with("this.") && act.starts_with("this.") {
        return "CONSTRUCTOR-PROP-ORDER (param property vs field init order)".to_string();
    }

    // 5. NS QUALIFICATION MISSING: e.g., M_1.x -> x
    {
        let mut stripped = exp.to_string();
        for prefix in &[
            "M_1.", "M_2.", "M_3.", "m1_1.", "m1_2.", "m2_1.", "m2_2.", "m1.", "M.", "exports.",
        ] {
            stripped = stripped.replace(prefix, "");
        }
        if stripped.trim() == act {
            return "NS-QUALIFICATION-MISSING (module alias prefix not emitted)".to_string();
        }
    }

    // 6. TYPE NOT STRIPPED
    if act.contains("<") && !exp.contains("<") && act.contains(">") && !act.contains("<<") {
        return "TYPE-NOT-STRIPPED (type annotation/generic in JS output)".to_string();
    }
    if act.contains("implements ") && !exp.contains("implements ") {
        return "TYPE-NOT-STRIPPED (implements clause not removed)".to_string();
    }

    // 7. PARAM DEFAULT NOT STRIPPED
    if (exp.contains("function ")
        || act.contains("function ")
        || exp.contains("constructor")
        || act.contains("constructor"))
        && act.contains("= ")
        && !exp.contains("= ")
    {
        return "PARAM-DEFAULT-NOT-STRIPPED (param default in output)".to_string();
    }

    // 8. DECORATOR DOWNLEVEL
    if exp.starts_with("let ") && exp.contains("= class ") && act.starts_with("class ") {
        return "DECORATOR-DOWNLEVEL (class not downleveled for decorator)".to_string();
    }

    // 9. CONST ENUM NOT INLINED
    if exp
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
        && (act.contains("Foo.") || act.contains("E.") || act.contains("MyEnum."))
    {
        return "CONST-ENUM-NOT-INLINED (const enum ref not replaced)".to_string();
    }

    // 10. OBJECT.ASSIGN NESTING
    if exp.contains("Object.assign(Object.assign") && !act.contains("Object.assign(Object.assign") {
        return "OBJECT-ASSIGN-NESTING (nested Object.assign)".to_string();
    }

    // 11. EXPORT MISPLACED
    if (act.trim() == "export {};" || act.contains("export ")) && !exp.contains("export") {
        return "EXPORT-MISPLACED (export at wrong position)".to_string();
    }

    // 12. ACCESSOR keyword
    if exp.contains("accessor ") && !act.contains("accessor ") {
        return "ACCESSOR-NOT-EMITTED (accessor keyword missing)".to_string();
    }

    // 13. NUMERIC LITERAL
    {
        let exp_val = exp
            .split("= ")
            .nth(1)
            .unwrap_or("")
            .trim()
            .trim_end_matches(';');
        let act_val = act
            .split("= ")
            .nth(1)
            .unwrap_or("")
            .trim()
            .trim_end_matches(';');
        if !exp_val.is_empty()
            && !act_val.is_empty()
            && exp_val.chars().all(|c| c.is_ascii_digit() || c == '.')
            && act_val.chars().all(|c| c.is_ascii_digit() || c == '.')
            && exp_val != act_val
        {
            return "NUMERIC-LITERAL-NOT-FOLDED (octal/literal not converted)".to_string();
        }
    }

    // 14. JSX TRANSFORM
    if (act.contains("createElement") && !exp.contains("createElement"))
        || (!act.contains("createElement") && exp.contains("createElement"))
    {
        return "JSX-TRANSFORM-DIFF (JSX vs createElement mismatch)".to_string();
    }

    // 15. CONFLICT MARKER
    if act.contains("<<<")
        || act.contains("< HEAD")
        || act.contains("<< HEAD")
        || act.contains("<<< HEAD")
    {
        return "CONFLICT-MARKER-NOT-HANDLED".to_string();
    }

    // 16. NULLISH/OPTIONAL CHAIN HELPER
    if (act.contains("((_a") || act.contains("_a =")) && !exp.contains("_a") {
        return "NULLISH-HELPER-DIFF (helper variable differs)".to_string();
    }

    // 17. PRIVATE FIELD
    if exp.contains("#") && !act.contains("#") {
        return "PRIVATE-FIELD-NOT-EMITTED".to_string();
    }

    // 18. MISSING VAR keyword
    if exp.starts_with("var ") && !act.starts_with("var ") {
        return "MISSING-VAR-KEYWORD".to_string();
    }

    // 19. PROLOGUE ordering
    if (exp.starts_with("\"") || exp.starts_with("'")) && act.starts_with("this.") {
        return "PROLOGUE-ORDER (prologue vs property order)".to_string();
    }
    if exp.starts_with("this.") && (act.starts_with("\"") || act.starts_with("'")) {
        return "PROLOGUE-ORDER (prologue vs property order)".to_string();
    }

    // 20. STRING ESCAPE / UNICODE content difference
    if (exp.starts_with("var ") && act.starts_with("var "))
        || (exp.starts_with("const ") && act.starts_with("const "))
    {
        return "LITERAL-VALUE-DIFF (string/unicode/escape literal differs)".to_string();
    }

    // 21. TRAILING COMMA
    if exp.ends_with(',') && !act.ends_with(',') && exp.trim_end_matches(',') == act {
        return "TRAILING-COMMA-MISSING".to_string();
    }

    // 22. OPTIONAL CHAINING NOT DOWNLEVELED: obj?.kind vs (obj === null ... ? void 0 : obj.kind)
    if (act.contains("?.") && !exp.contains("?."))
        || (act.contains("??") && exp.contains("!== void 0"))
    {
        return "OPTIONAL-CHAIN-NOT-DOWNLEVELED".to_string();
    }
    // Reverse: expected has ?. but actual has ternary
    if exp.contains("?.") && !act.contains("?.") && act.contains("void 0") {
        return "OPTIONAL-CHAIN-NOT-DOWNLEVELED".to_string();
    }
    // typeof (expr === null ...) vs typeof expr?.
    if (exp.contains("typeof (") && act.contains("typeof ") && act.contains("?."))
        || (act.contains("typeof (") && exp.contains("typeof ") && exp.contains("?."))
    {
        return "OPTIONAL-CHAIN-NOT-DOWNLEVELED".to_string();
    }

    // 23. EXTRA PARENTHESES: (expr) vs expr or vice versa
    if exp.starts_with("(") && !act.starts_with("(") && exp.ends_with(");") && act.ends_with(";") {
        let inner = exp.trim_start_matches('(').trim_end_matches(");");
        if format!("{};", inner) == act || inner == act.trim_end_matches(';') {
            return "EXTRA-PARENS-DIFF (parenthesization differs)".to_string();
        }
    }
    if !exp.starts_with("(") && act.starts_with("(") {
        return "EXTRA-PARENS-DIFF (parenthesization differs)".to_string();
    }
    // ({}.toString()) vs ({}).toString()
    if (exp.contains("({") && act.contains("({")) && (exp.contains("})") || act.contains("})")) {
        if exp.replace("({}", "PLACEHOLDER") != act.replace("({}", "PLACEHOLDER") {
            return "EXTRA-PARENS-DIFF (parenthesization differs)".to_string();
        }
    }

    // 24. AWAIT NOT DOWNLEVELED: yield vs await
    if exp.contains("yield") && act.contains("await") && exp.replace("yield", "await") == act {
        return "AWAIT-NOT-DOWNLEVELED (await not converted to yield)".to_string();
    }
    if act.contains("await") && exp.contains("yield") {
        return "AWAIT-NOT-DOWNLEVELED (await not converted to yield)".to_string();
    }

    // 25. MULTIPLE EXTENDS DROPPED: class C extends A, B -> class C extends A
    if exp.contains("extends")
        && act.contains("extends")
        && exp.contains(",")
        && !act.contains(",")
        && exp.starts_with("class ")
        && act.starts_with("class ")
    {
        return "MULTI-EXTENDS-DROPPED (second extends clause dropped)".to_string();
    }
    // implements clause preceding extends
    if exp.contains("extends") && !act.contains("extends") && act.starts_with("class ") {
        return "EXTENDS-CLAUSE-DROPPED".to_string();
    }

    // 26. COMMA-VS-SEMICOLON in object literal: a, vs a;
    if exp.ends_with(',')
        && act.ends_with(';')
        && exp.trim_end_matches(',') == act.trim_end_matches(';')
    {
        return "COMMA-VS-SEMICOLON (comma replaced by semicolon in object)".to_string();
    }
    if exp.ends_with(';')
        && act.ends_with(',')
        && exp.trim_end_matches(';') == act.trim_end_matches(',')
    {
        return "COMMA-VS-SEMICOLON (comma replaced by semicolon in object)".to_string();
    }

    // 27. CATCH BINDING: catch (_a) vs catch
    if exp.starts_with("catch") && act.starts_with("catch") {
        return "CATCH-BINDING-DIFF (catch parameter differs)".to_string();
    }

    // 28. Arrow paren style: async (x) => vs async x =>
    if exp.contains("(") && !act.contains("(") && exp.replace("(", "").replace(")", "") == act {
        return "ARROW-PARENS-DIFF (arrow function paren style)".to_string();
    }
    if !exp.contains("(") && act.contains("(") && act.replace("(", "").replace(")", "") == exp {
        return "ARROW-PARENS-DIFF (arrow function paren style)".to_string();
    }

    // 29. NS PREFIX MISSING (broader): Foo.C() -> C(), C.prototype -> .prototype
    if exp.len() > act.len() && act.len() > 0 {
        // Check if removing a dotted prefix from exp yields act
        if let Some(dot_pos) = exp.find('.') {
            let after_dot = &exp[dot_pos + 1..];
            if after_dot == act || format!(".{}", after_dot) == act {
                return "NS-QUALIFICATION-MISSING (module alias prefix not emitted)".to_string();
            }
        }
    }

    // 30. MODULE FORMAT MISMATCH: e.g., define(["require",...]) vs function foo()
    if exp.contains("define(") && !act.contains("define(") {
        return "MODULE-FORMAT-MISMATCH (AMD/UMD wrapper missing)".to_string();
    }

    // 31. SATISFIES NOT STRIPPED
    if act.contains("satisfies") && !exp.contains("satisfies") {
        return "TYPE-NOT-STRIPPED (satisfies expression not removed)".to_string();
    }

    // 32. STATIC BLOCK vs static method/property
    if exp.contains("static {")
        || act.contains("static {")
        || exp.contains("static test")
        || act.contains("static test")
    {
        return "STATIC-MEMBER-DIFF (static block/member emit differs)".to_string();
    }

    // 33. NAMESPACE KEYWORD in output (should be removed for JS emit)
    if act.contains("namespace ") && !exp.contains("namespace ") {
        return "NAMESPACE-NOT-STRIPPED (namespace keyword in JS output)".to_string();
    }

    // 34. METHOD NAME WRONG: foo(x) vs C(x) (method name resolution)
    if exp.contains("(")
        && act.contains("(")
        && exp.split('(').next() != act.split('(').next()
        && exp.split('(').nth(1) == act.split('(').nth(1)
    {
        return "EMIT-CONTENT-DIFF (function/method name or call target differs)".to_string();
    }

    // 35. ENUM NAME QUOTING: E[E[0n] = 0] = 0n vs E[E["0n"] = 0] = "0n"
    if exp.contains("[") && act.contains("[") && exp.contains("] =") && act.contains("] =") {
        return "ENUM-VALUE-DIFF (enum name/value quoting differs)".to_string();
    }

    // 36. BROKEN PARSE: garbled output
    if act.contains("@<") || act.contains("obju") || act.len() < 3 {
        return "BROKEN-PARSE-EMIT (garbled/truncated output from error recovery)".to_string();
    }

    // 37. CASE BLOCK: case 'abc': { vs case 'abc':
    if exp.contains("case ") && act.contains("case ") {
        return "CASE-BLOCK-DIFF (case statement formatting)".to_string();
    }

    // 38. Object.assign extra arg: Object.assign({ a: "a" }) vs Object.assign({}, { a: "a" })
    if exp.contains("Object.assign") && act.contains("Object.assign") {
        return "OBJECT-ASSIGN-DIFF (Object.assign args differ)".to_string();
    }

    // 39. MISSING EXPRESSION: expected has content, actual is empty or minimal
    if act.is_empty() || act == "}" || act == ";" || act == "};" {
        return "MISSING-CONTENT (expected content not emitted)".to_string();
    }

    // 40. FUNC CALL/EXPRESSION difference (broad catch)
    if exp.contains("(") && act.contains("(") {
        return "EMIT-CONTENT-DIFF (expression/call emitted differently)".to_string();
    }

    format!("UNCATEGORIZED")
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diff_by_2_deep_analysis() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Collect all diff-by-2 test names and directions
    let mut test_data: Vec<(String, i64, Vec<String>, Vec<String>)> = Vec::new(); // (name, direction, extra_lines, missing_lines)

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        let len_diff = act_lines.len() as i64 - exp_lines.len() as i64;
        if len_diff.unsigned_abs() != 2 {
            continue;
        }

        // Use a simple greedy alignment to find extra/missing lines
        // Walk both sequences, on mismatch try skipping one or the other
        let mut extras: Vec<String> = Vec::new();
        let mut missings: Vec<String> = Vec::new();

        let mut ai = 0usize;
        let mut ei = 0usize;
        while ai < act_lines.len() && ei < exp_lines.len() {
            if act_lines[ai] == exp_lines[ei] {
                ai += 1;
                ei += 1;
            } else if len_diff > 0 && ai + 1 < act_lines.len() && act_lines[ai + 1] == exp_lines[ei]
            {
                // actual has an extra line
                extras.push(act_lines[ai].to_string());
                ai += 1;
            } else if len_diff < 0 && ei + 1 < exp_lines.len() && act_lines[ai] == exp_lines[ei + 1]
            {
                // expected has an extra line (missing from actual)
                missings.push(exp_lines[ei].to_string());
                ei += 1;
            } else {
                // Both differ - record both and advance both
                extras.push(act_lines[ai].to_string());
                missings.push(exp_lines[ei].to_string());
                ai += 1;
                ei += 1;
            }
        }
        while ai < act_lines.len() {
            extras.push(act_lines[ai].to_string());
            ai += 1;
        }
        while ei < exp_lines.len() {
            missings.push(exp_lines[ei].to_string());
            ei += 1;
        }

        test_data.push((name, len_diff, extras, missings));
    }

    // Print all test names and directions
    eprintln!("\n================================================================");
    eprintln!("ALL DIFF-BY-2 TESTS ({} total)", test_data.len());
    eprintln!("================================================================");

    let actual_extra: Vec<_> = test_data.iter().filter(|t| t.1 == 2).collect();
    let actual_missing: Vec<_> = test_data.iter().filter(|t| t.1 == -2).collect();
    eprintln!("Actual has 2 EXTRA lines:   {}", actual_extra.len());
    eprintln!("Actual has 2 MISSING lines: {}", actual_missing.len());

    // Classify patterns for the +2 (actual has extra) cases
    eprintln!("\n================================================================");
    eprintln!("ACTUAL HAS 2 EXTRA LINES - PATTERN CLASSIFICATION");
    eprintln!("================================================================\n");

    let mut extra_patterns: HashMap<String, Vec<String>> = HashMap::new();
    for (name, dir, extras, missings) in &test_data {
        if *dir != 2 {
            continue;
        }
        // Classify the 2 extra lines
        let extra_cats: Vec<&str> = extras.iter().map(|l| classify_line_type(l)).collect();
        let missing_cats: Vec<&str> = missings.iter().map(|l| classify_line_type(l)).collect();

        let pattern = if missings.is_empty() {
            // Pure 2 extra lines
            format!("PURE-2-EXTRA: [{}]", extra_cats.join(", "))
        } else {
            format!(
                "EXTRA+REPLACED: extras=[{}] missings=[{}]",
                extra_cats.join(", "),
                missing_cats.join(", ")
            )
        };

        extra_patterns
            .entry(pattern)
            .or_insert_with(Vec::new)
            .push(name.clone());
    }

    let mut sorted_extra: Vec<_> = extra_patterns.iter().collect();
    sorted_extra.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (pattern, tests) in &sorted_extra {
        eprintln!("{:>4}  {}", tests.len(), pattern);
        for (i, t) in tests.iter().enumerate() {
            if i >= 3 {
                break;
            }
            eprintln!("        - {}", t);
        }
        if tests.len() > 3 {
            eprintln!("        ... +{} more", tests.len() - 3);
        }
    }

    // Classify patterns for the -2 (actual missing) cases
    eprintln!("\n================================================================");
    eprintln!("ACTUAL HAS 2 MISSING LINES - PATTERN CLASSIFICATION");
    eprintln!("================================================================\n");

    let mut missing_patterns: HashMap<String, Vec<String>> = HashMap::new();
    for (name, dir, extras, missings) in &test_data {
        if *dir != -2 {
            continue;
        }
        let extra_cats: Vec<&str> = extras.iter().map(|l| classify_line_type(l)).collect();
        let missing_cats: Vec<&str> = missings.iter().map(|l| classify_line_type(l)).collect();

        let pattern = if extras.is_empty() {
            format!("PURE-2-MISSING: [{}]", missing_cats.join(", "))
        } else {
            format!(
                "MISSING+REPLACED: extras=[{}] missings=[{}]",
                extra_cats.join(", "),
                missing_cats.join(", ")
            )
        };

        missing_patterns
            .entry(pattern)
            .or_insert_with(Vec::new)
            .push(name.clone());
    }

    let mut sorted_missing: Vec<_> = missing_patterns.iter().collect();
    sorted_missing.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (pattern, tests) in &sorted_missing {
        eprintln!("{:>4}  {}", tests.len(), pattern);
        for (i, t) in tests.iter().enumerate() {
            if i >= 3 {
                break;
            }
            eprintln!("        - {}", t);
        }
        if tests.len() > 3 {
            eprintln!("        ... +{} more", tests.len() - 3);
        }
    }

    // Now show actual line content for the top patterns
    eprintln!("\n================================================================");
    eprintln!("DETAILED SAMPLES: actual extra/missing line content");
    eprintln!("================================================================\n");

    // For dir=+2 tests, show the actual extra lines
    eprintln!("--- ACTUAL HAS 2 EXTRA: line content samples ---\n");
    let mut shown = 0;
    for (name, dir, extras, missings) in &test_data {
        if *dir != 2 {
            continue;
        }
        if shown >= 30 {
            break;
        }
        shown += 1;
        eprintln!("[{}]", name);
        for (i, e) in extras.iter().enumerate() {
            let display = if e.len() > 120 { &e[..120] } else { e.as_str() };
            eprintln!("  EXTRA[{}]: {:?}", i, display);
        }
        for (i, m) in missings.iter().enumerate() {
            let display = if m.len() > 120 { &m[..120] } else { m.as_str() };
            eprintln!("  MISS[{}]:  {:?}", i, display);
        }
        eprintln!();
    }

    // For dir=-2 tests, show the missing lines
    eprintln!("--- ACTUAL HAS 2 MISSING: line content samples ---\n");
    shown = 0;
    for (name, dir, extras, missings) in &test_data {
        if *dir != -2 {
            continue;
        }
        if shown >= 30 {
            break;
        }
        shown += 1;
        eprintln!("[{}]", name);
        for (i, m) in missings.iter().enumerate() {
            let display = if m.len() > 120 { &m[..120] } else { m.as_str() };
            eprintln!("  MISS[{}]:  {:?}", i, display);
        }
        for (i, e) in extras.iter().enumerate() {
            let display = if e.len() > 120 { &e[..120] } else { e.as_str() };
            eprintln!("  EXTRA[{}]: {:?}", i, display);
        }
        eprintln!();
    }
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diff_by_2_aggregate_patterns() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    let mut patterns: HashMap<String, Vec<String>> = HashMap::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        let len_diff = act_lines.len() as i64 - exp_lines.len() as i64;
        if len_diff.unsigned_abs() != 2 {
            continue;
        }

        // Find first mismatch
        let mut div = std::cmp::min(exp_lines.len(), act_lines.len());
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                div = i;
                break;
            }
        }

        let exp_at_div = exp_lines.get(div).unwrap_or(&"<EOF>");
        let act_at_div = act_lines.get(div).unwrap_or(&"<EOF>");
        let exp_trimmed = exp_at_div.trim();
        let act_trimmed = act_at_div.trim();

        // Also look at actual lines div+1 (if actual is longer)
        let act_next = act_lines.get(div + 1).map(|l| l.trim()).unwrap_or("<EOF>");
        let exp_next = exp_lines.get(div + 1).map(|l| l.trim()).unwrap_or("<EOF>");

        let pattern;

        if len_diff == 2 {
            // ACTUAL HAS 2 EXTRA LINES
            // Check if actual[div+2..] aligns with expected[div..]
            let shifts_clean = div + 2 < act_lines.len()
                && div < exp_lines.len()
                && act_lines[div + 2] == exp_lines[div];

            // Check if it's an outFile vs per-file difference
            if act_trimmed.starts_with("//// [")
                && exp_trimmed.starts_with("//// [")
                && act_trimmed != exp_trimmed
            {
                pattern = "P1-OUTFILE-VS-PERFILE (outFile header mismatch: separate files emitted instead of bundle)".to_string();
            }
            // Check for CJS module emission: Object.defineProperty replaced by require()
            else if exp_trimmed.contains("Object.defineProperty")
                && exp_trimmed.contains("__esModule")
                && (act_trimmed.contains("require(") || act_trimmed.starts_with("const _module"))
            {
                pattern = "P2-CJS-IMPORT-REWRITE (Object.defineProperty(__esModule) replaced by require(\"\") pattern)".to_string();
            }
            // Extra blank lines
            else if act_trimmed.is_empty() && act_next.is_empty() {
                pattern = "P3-EXTRA-BLANK-LINES (2 extra empty lines)".to_string();
            } else if act_trimmed.is_empty() && shifts_clean {
                pattern = "P4-EXTRA-BLANK-LINE-PLUS (1 blank + shifted content)".to_string();
            }
            // Comment in actual not in expected
            else if (act_trimmed.starts_with("//") || act_trimmed.starts_with("/*"))
                && (act_next.starts_with("//") || act_next.starts_with("/*") || act_next.is_empty())
                && !exp_trimmed.starts_with("//")
                && !exp_trimmed.starts_with("/*")
            {
                pattern = "P5-EXTRA-COMMENTS (2 extra comment/blank lines in actual)".to_string();
            }
            // Line break in expression: actual splits one expected line into 2+
            else if shifts_clean {
                // actual[div] and actual[div+1] together should approximate expected[div]
                let combined = format!("{}{}", act_trimmed, act_next);
                let combined_sp = format!("{} {}", act_trimmed, act_next);
                if combined == exp_trimmed.replace(' ', "") || combined_sp.trim() == exp_trimmed {
                    pattern = "P6-EXPRESSION-SPLIT (single line split into 2 lines)".to_string();
                } else if act_trimmed.contains(":") && act_next == exp_trimmed {
                    // Extra ":;" or ": any;" type annotation not erased
                    pattern = "P7-TYPE-ANNOTATION-NOT-ERASED (extra colon/type on separate line)"
                        .to_string();
                } else if act_trimmed.contains("=;") || act_trimmed == "=;" {
                    pattern = "P8-BROKEN-EQUALS (stray '=;' emitted as separate line)".to_string();
                } else if act_trimmed.contains("\\") || act_trimmed.ends_with("/") {
                    pattern =
                        "P9-REGEX-OR-ESCAPE-SPLIT (regex/escape split across lines)".to_string();
                } else {
                    // check for namespace/IIFE or multiline body expansion
                    let et = exp_trimmed;
                    let at = act_trimmed;
                    if et.contains("constructor")
                        && at.contains("constructor")
                        && !et.contains('}')
                        && at.contains('{')
                    {
                        pattern = "P10-BLOCK-EXPANSION (single-line block expanded to multi-line)"
                            .to_string();
                    } else if et.contains("exports.") || at.contains("m1.") || at.contains("m2.") {
                        pattern = "P11-EXTRA-NAMESPACE-EXPORT (extra namespace export statement)"
                            .to_string();
                    } else {
                        pattern = format!(
                            "P99-OTHER-EXTRA (exp: {:?} act: {:?})",
                            safe_trunc(exp_trimmed, 50),
                            safe_trunc(act_trimmed, 50)
                        );
                    }
                }
            } else {
                // Not clean shift - more complex diffs
                if act_trimmed.contains("require(\"\")") || act_trimmed.starts_with("const _module")
                {
                    pattern = "P2-CJS-IMPORT-REWRITE (Object.defineProperty(__esModule) replaced by require(\"\") pattern)".to_string();
                } else if act_trimmed.starts_with("//// [") && exp_trimmed.starts_with("//// [") {
                    pattern = "P1-OUTFILE-VS-PERFILE (outFile header mismatch)".to_string();
                } else if (act_trimmed.starts_with("//")
                    || act_trimmed.starts_with("/*")
                    || act_trimmed.starts_with("#"))
                    && !exp_trimmed.starts_with("//")
                    && !exp_trimmed.starts_with("/*")
                    && !exp_trimmed.starts_with("#")
                {
                    pattern = "P5-EXTRA-COMMENTS (extra comment/directive in actual)".to_string();
                } else if exp_trimmed.contains("Object.defineProperty")
                    && act_trimmed.contains("require(")
                {
                    pattern = "P2-CJS-IMPORT-REWRITE (Object.defineProperty(__esModule) replaced by require(\"\") pattern)".to_string();
                } else {
                    pattern = format!(
                        "P99-OTHER-EXTRA-COMPLEX (exp: {:?} act: {:?})",
                        safe_trunc(exp_trimmed, 50),
                        safe_trunc(act_trimmed, 50)
                    );
                }
            }
        } else {
            // ACTUAL HAS 2 MISSING LINES (len_diff == -2)
            // Check if expected[div+2..] aligns with actual[div..]
            let shifts_clean = div + 2 < exp_lines.len()
                && div < act_lines.len()
                && exp_lines[div + 2] == act_lines[div];

            // Missing comments
            if (exp_trimmed.starts_with("//")
                || exp_trimmed.starts_with("/*")
                || exp_trimmed.starts_with("*"))
                && (exp_next.starts_with("//")
                    || exp_next.starts_with("/*")
                    || exp_next.starts_with("*")
                    || exp_next.is_empty())
            {
                pattern = "P12-MISSING-COMMENTS (2 comment lines dropped from output)".to_string();
            }
            // Missing var declarations (namespace hoisted vars)
            else if exp_trimmed.starts_with("let ") || exp_trimmed.starts_with("var ") {
                if exp_next.starts_with("(function")
                    || exp_next.starts_with("let ")
                    || exp_next.starts_with("var ")
                {
                    pattern =
                        "P13-MISSING-NAMESPACE-VAR-DECL (hoisted let/var before IIFE not emitted)"
                            .to_string();
                } else {
                    pattern = "P14-MISSING-VAR-DECL (var/let declaration not emitted)".to_string();
                }
            }
            // Missing exports
            else if exp_trimmed.starts_with("exports.") || exp_trimmed.starts_with("export ") {
                pattern = "P15-MISSING-EXPORT (export statement not emitted)".to_string();
            }
            // Missing reference directive (///  <reference>)
            else if exp_trimmed.starts_with("///") {
                pattern = "P16-MISSING-REFERENCE-DIRECTIVE (/// directive dropped)".to_string();
            }
            // Lines merged (2 expected lines combined into 1 actual line)
            else if shifts_clean {
                let combined = format!("{} {}", exp_trimmed, exp_next);
                if act_trimmed == combined.trim() || act_trimmed.starts_with(exp_trimmed) {
                    pattern = "P17-LINES-MERGED (2 expected lines merged into 1)".to_string();
                } else {
                    pattern = format!(
                        "P99-OTHER-MISSING-SHIFTED (exp: {:?} act: {:?})",
                        safe_trunc(exp_trimmed, 50),
                        safe_trunc(act_trimmed, 50)
                    );
                }
            }
            // Missing __importDefault helper
            else if exp_trimmed.contains("__importDefault")
                || exp_trimmed.contains("__importStar")
            {
                pattern = "P18-MISSING-IMPORT-HELPER (__importDefault/__importStar not emitted)"
                    .to_string();
            }
            // Missing downlevel helper variable
            else if exp_trimmed.starts_with("_a") || exp_trimmed.starts_with("var _a") {
                pattern = "P19-MISSING-DOWNLEVEL-VAR (helper variable not emitted)".to_string();
            } else if exp_trimmed.starts_with("();") || exp_trimmed == "();" {
                pattern = "P20-MISSING-CALL-EXPR (call expression not emitted)".to_string();
            } else {
                pattern = format!(
                    "P99-OTHER-MISSING (exp: {:?} act: {:?})",
                    safe_trunc(exp_trimmed, 50),
                    safe_trunc(act_trimmed, 50)
                );
            }
        }

        patterns.entry(pattern).or_insert_with(Vec::new).push(name);
    }

    eprintln!("\n================================================================");
    eprintln!("DIFF-BY-2: AGGREGATE ROOT-CAUSE CLASSIFICATION");
    eprintln!("================================================================\n");

    let total: usize = patterns.values().map(|v| v.len()).sum();
    let mut sorted: Vec<_> = patterns.iter().collect();
    sorted.sort_by(|a, b| {
        // Group by count descending, then keep deterministic ordering by pattern prefix.
        b.1.len().cmp(&a.1.len()).then_with(|| {
            let a_id = a.0.split('-').next().unwrap_or("Z");
            let b_id = b.0.split('-').next().unwrap_or("Z");
            a_id.cmp(b_id)
        })
    });

    // High level:
    let extra_total: usize = sorted
        .iter()
        .filter(|(k, _)| !k.contains("MISSING"))
        .map(|(_, v)| v.len())
        .sum();
    let missing_total: usize = sorted
        .iter()
        .filter(|(k, _)| {
            k.contains("MISSING")
                || k.contains("P12")
                || k.contains("P13")
                || k.contains("P14")
                || k.contains("P15")
                || k.contains("P16")
                || k.contains("P17")
                || k.contains("P18")
                || k.contains("P19")
                || k.contains("P20")
        })
        .map(|(_, v)| v.len())
        .sum();

    eprintln!("Total diff-by-2 failures: {}", total);
    eprintln!("  Actual has 2 EXTRA lines:   {}", extra_total);
    eprintln!(
        "  Actual has 2 MISSING lines: {} (note: some 'OTHER' may be either)",
        missing_total
    );
    eprintln!();

    eprintln!("{:>5} {:>5}  {}", "Count", "%", "Pattern");
    eprintln!("{:-^5} {:-^5}  {:-<80}", "", "", "");
    for (pattern, tests) in &sorted {
        let pct = tests.len() as f64 / total as f64 * 100.0;
        eprintln!("{:>5} {:>4.1}%  {}", tests.len(), pct, pattern);
        // Show 3 test names
        for (i, t) in tests.iter().enumerate() {
            if i >= 3 {
                break;
            }
            eprintln!("                - {}", t);
        }
        if tests.len() > 3 {
            eprintln!("                ... +{} more", tests.len() - 3);
        }
    }
    eprintln!("{:-^5} {:-^5}  {:-<80}", "", "", "");
    eprintln!("{:>5}        TOTAL", total);
}

fn classify_line_type(line: &str) -> &'static str {
    let t = line.trim();
    if t.is_empty() {
        "BLANK"
    } else if t == "\"use strict\";" || t == "'use strict';" || t.contains("use strict") {
        "USE-STRICT"
    } else if t.starts_with("///") {
        "REFERENCE-DIRECTIVE"
    } else if t.starts_with("////") {
        "FILE-HEADER"
    } else if t.starts_with("//") || t.starts_with("/*") || t.starts_with("*") || t.ends_with("*/")
    {
        "COMMENT"
    } else if t.contains("Object.defineProperty") && t.contains("__esModule") {
        "ESMODULE-DEFINE"
    } else if t.contains("Object.defineProperty") {
        "OBJECT-DEFINE-PROP"
    } else if t.contains("exports.")
        || t.contains("module.exports")
        || t.starts_with("export ")
        || t == "export {};"
        || t.starts_with("export{")
    {
        "EXPORT"
    } else if t.contains("require(") || t.starts_with("import ") {
        "IMPORT"
    } else if t.contains("__awaiter")
        || t.contains("__generator")
        || t.contains("__rest")
        || t.contains("__decorate")
        || t.contains("__metadata")
        || t.contains("__param")
    {
        "HELPER-FN"
    } else if t.starts_with("var ") || t.starts_with("let ") || t.starts_with("const ") {
        "VAR-DECL"
    } else if t.contains("(function") || t.contains("})(") {
        "NAMESPACE-IIFE"
    } else if t == "}" || t == "});" || t == "})();" || t == "})" {
        "CLOSING-BRACE"
    } else if t == ";" {
        "SEMICOLON"
    } else {
        "OTHER"
    }
}

#[test]
#[ignore] // Debug investigation test — fill in test_name and run manually
fn diag_use_strict_specific() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");

    // Investigate patterns: find tests that are close to passing
    let test_name = "placeholder_not_used";
    let path = cases_dir.join(format!("{}.ts", test_name));
    let content = std::fs::read_to_string(&path).unwrap();
    eprintln!("=== SOURCE ===");
    for (i, line) in content.lines().enumerate().take(10) {
        eprintln!("  {:3}: {}", i + 1, line);
    }

    // Parse options
    let test_case = tsc_rs_harness::parse_test_case(path.to_str().unwrap(), &content);
    eprintln!("\n=== OPTIONS ===");
    eprintln!("  target: {:?}", test_case.options.target);
    eprintln!("  module: {:?}", test_case.options.module);
    eprintln!("  strict: {:?}", test_case.options.strict);
    eprintln!("  always_strict: {:?}", test_case.options.always_strict);

    // Parse & emit first file
    let first_file = &test_case.files[0];
    let sf = tsc_rs_parser::parse(&first_file.name, &first_file.content);
    eprintln!("\n=== PARSED STMTS ({}) ===", sf.statements.len());
    for (i, stmt) in sf.statements.iter().enumerate().take(10) {
        let kind_str = match &stmt.kind {
            tsc_rs_ast::StmtKind::Import(imp) => format!("Import(source={:?})", imp.source),
            tsc_rs_ast::StmtKind::Export(e) => {
                format!("Export({:?})", std::mem::discriminant(&e.kind))
            }
            tsc_rs_ast::StmtKind::ExportAssign(_) => "ExportAssign".to_string(),
            tsc_rs_ast::StmtKind::ModuleDecl(m) => format!("ModuleDecl(name={:?})", m.name),
            tsc_rs_ast::StmtKind::FnDecl(f) => format!("FnDecl(name={:?})", f.name),
            tsc_rs_ast::StmtKind::ClassDecl(c) => format!("ClassDecl(name={:?})", c.name),
            tsc_rs_ast::StmtKind::Var(_) => "Var".to_string(),
            tsc_rs_ast::StmtKind::InterfaceDecl(i) => format!("InterfaceDecl(name={})", i.name),
            tsc_rs_ast::StmtKind::TypeAlias(t) => format!("TypeAlias(name={})", t.name),
            tsc_rs_ast::StmtKind::EnumDecl(e) => format!("EnumDecl(name={})", e.name),
            _ => format!("{:?}", std::mem::discriminant(&stmt.kind)),
        };
        eprintln!("  [{}]: {}", i, kind_str);
    }

    eprintln!(
        "  emit_declaration_only: {:?}",
        test_case.options.emit_declaration_only
    );
    eprintln!("  declaration: {:?}", test_case.options.declaration);
    eprintln!(
        "  files: {:?}",
        test_case.files.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // Run through the baseline runner to check
    let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
    eprintln!("  passed: {}", result.passed);
    let exp_lines: Vec<&str> = result.expected_output.lines().collect();
    let act_lines: Vec<&str> = result.actual_output.lines().collect();
    eprintln!(
        "  exp lines: {}, act lines: {}",
        exp_lines.len(),
        act_lines.len()
    );
    for i in 0..std::cmp::max(exp_lines.len(), act_lines.len()).min(30) {
        let e = exp_lines.get(i).unwrap_or(&"<EOF>");
        let a = act_lines.get(i).unwrap_or(&"<EOF>");
        let marker = if e != a { ">>>" } else { "   " };
        if e != a {
            eprintln!("  {} {:3}: EXP: {}", marker, i + 1, e);
            eprintln!("  {} {:3}: ACT: {}", marker, i + 1, a);
        } else {
            eprintln!("  {} {:3}: {}", marker, i + 1, e);
        }
    }

    let tests: Vec<&str> = vec![];
    for name in tests {
        let path = cases_dir.join(format!("{}.ts", name));
        if !path.exists() {
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        eprintln!("\n=== {} ===", name);
        eprintln!("Passed: {}", result.passed);
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let max = std::cmp::max(exp_lines.len(), act_lines.len());
        for i in 0..max {
            let e = exp_lines.get(i).unwrap_or(&"<EOF>");
            let a = act_lines.get(i).unwrap_or(&"<EOF>");
            if e != a {
                eprintln!("FIRST DIFF at L{}:", i + 1);
                let start = if i > 3 { i - 3 } else { 0 };
                for j in start..std::cmp::min(i + 8, max) {
                    let e2 = exp_lines.get(j).unwrap_or(&"<EOF>");
                    let a2 = act_lines.get(j).unwrap_or(&"<EOF>");
                    let marker = if j == i { ">>>" } else { "   " };
                    if e2 == a2 {
                        eprintln!("  {} {:3}: {}", marker, j + 1, e2);
                    } else {
                        eprintln!("  {} {:3}: EXP: {}", marker, j + 1, e2);
                        eprintln!("  {} {:3}: ACT: {}", marker, j + 1, a2);
                    }
                }
                break;
            }
        }
    }
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_use_strict() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    let mut missing_strict: Vec<String> = Vec::new();
    let mut extra_strict: Vec<String> = Vec::new();
    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let et = exp_lines[i].trim();
                let at = act_lines[i].trim();
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                if et.starts_with("\"use strict\"") && !at.starts_with("\"use strict\"") {
                    missing_strict.push(name);
                } else if at.starts_with("\"use strict\"") && !et.starts_with("\"use strict\"") {
                    extra_strict.push(name);
                }
                break;
            }
        }
    }

    eprintln!("\n== MISSING-USE-STRICT: {} tests ==", missing_strict.len());
    for name in &missing_strict {
        let source = std::fs::read_to_string(format!(
            "{}/tests/cases/compiler/{}.ts",
            root.display(),
            name
        ))
        .unwrap_or_default();
        let opts: Vec<&str> = source
            .lines()
            .filter(|l| l.trim().starts_with("// @"))
            .take(10)
            .collect();
        eprintln!("  {}: {}", name, opts.join(", "));
    }

    eprintln!("\n== EXTRA-USE-STRICT: {} tests ==", extra_strict.len());
    for name in &extra_strict {
        let source = std::fs::read_to_string(format!(
            "{}/tests/cases/compiler/{}.ts",
            root.display(),
            name
        ))
        .unwrap_or_default();
        let opts: Vec<&str> = source
            .lines()
            .filter(|l| l.trim().starts_with("// @"))
            .take(10)
            .collect();
        eprintln!("  {}: {}", name, opts.join(", "));
    }
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_whitespace() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Collect all WHITESPACE-ONLY failures: tests where the first differing
    // line has the same trimmed content but different whitespace.
    struct WsFailure {
        test_name: String,
        line_num: usize,       // 1-based line number of first mismatch
        expected_line: String, // raw expected line
        actual_line: String,   // raw actual line
        category: String,      // whitespace difference category
    }

    fn visualize_ws(line: &str) -> String {
        // Show leading whitespace character by character
        let mut vis = String::new();
        for ch in line.chars() {
            match ch {
                ' ' => vis.push_str("\u{00B7}"), // middle dot for space
                '\t' => vis.push_str("\\t"),
                '\r' => vis.push_str("\\r"),
                _ => break,
            }
        }
        vis
    }

    fn categorize_ws_diff(exp: &str, act: &str) -> String {
        let exp_leading: String = exp.chars().take_while(|c| c.is_whitespace()).collect();
        let act_leading: String = act.chars().take_while(|c| c.is_whitespace()).collect();
        let exp_trailing: String = exp
            .chars()
            .rev()
            .take_while(|c| c.is_whitespace())
            .collect();
        let act_trailing: String = act
            .chars()
            .rev()
            .take_while(|c| c.is_whitespace())
            .collect();
        let exp_has_tabs = exp_leading.contains('\t');
        let act_has_tabs = act_leading.contains('\t');
        let exp_indent = exp_leading.len();
        let act_indent = act_leading.len();
        let exp_trailing_len = exp_trailing.len();
        let act_trailing_len = act_trailing.len();

        // Determine the category
        if exp_has_tabs && !act_has_tabs {
            format!(
                "TABS-VS-SPACES (exp has tabs, act has spaces; exp_indent={}, act_indent={})",
                exp_indent, act_indent
            )
        } else if !exp_has_tabs && act_has_tabs {
            format!(
                "SPACES-VS-TABS (exp has spaces, act has tabs; exp_indent={}, act_indent={})",
                exp_indent, act_indent
            )
        } else if exp_has_tabs && act_has_tabs && exp_indent != act_indent {
            format!(
                "TAB-COUNT-DIFF (both tabs; exp_indent={}, act_indent={})",
                exp_indent, act_indent
            )
        } else if exp_indent != act_indent && exp_trailing_len == act_trailing_len {
            let diff = act_indent as i64 - exp_indent as i64;
            if diff > 0 {
                format!(
                    "OVER-INDENTED (spaces; exp={}, act={}, +{})",
                    exp_indent, act_indent, diff
                )
            } else {
                format!(
                    "UNDER-INDENTED (spaces; exp={}, act={}, {})",
                    exp_indent, act_indent, diff
                )
            }
        } else if exp_trailing_len != act_trailing_len && exp_indent == act_indent {
            format!(
                "TRAILING-WS-DIFF (exp_trailing={}, act_trailing={})",
                exp_trailing_len, act_trailing_len
            )
        } else if exp_indent != act_indent && exp_trailing_len != act_trailing_len {
            format!("LEADING-AND-TRAILING-DIFF (exp_indent={}, act_indent={}, exp_trail={}, act_trail={})",
                exp_indent, act_indent, exp_trailing_len, act_trailing_len)
        } else {
            // Same leading length but different characters (e.g., internal whitespace diff)
            // Check for internal whitespace differences
            let exp_inner = exp.trim();
            let act_inner = act.trim();
            if exp_inner == act_inner && exp_indent == act_indent {
                // Must be internal whitespace difference
                format!(
                    "INTERNAL-WS-DIFF (same indent={}, same trim, different internal spacing)",
                    exp_indent
                )
            } else {
                format!(
                    "OTHER-WS-DIFF (exp_indent={}, act_indent={})",
                    exp_indent, act_indent
                )
            }
        }
    }

    let mut failures: Vec<WsFailure> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Find first mismatch
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let et = exp_lines[i].trim();
                let at = act_lines[i].trim();
                if et == at {
                    let category = categorize_ws_diff(exp_lines[i], act_lines[i]);
                    failures.push(WsFailure {
                        test_name: name,
                        line_num: i + 1,
                        expected_line: exp_lines[i].to_string(),
                        actual_line: act_lines[i].to_string(),
                        category,
                    });
                }
                break;
            }
        }
    }

    // Sort by test name for stable output
    failures.sort_by(|a, b| a.test_name.cmp(&b.test_name));

    // =====================================================================
    // SECTION 1: Summary
    // =====================================================================
    eprintln!("\n================================================================");
    eprintln!("WHITESPACE-ONLY FIRST MISMATCH DIAGNOSTIC");
    eprintln!("================================================================");
    eprintln!("Total WHITESPACE-ONLY failures: {}\n", failures.len());

    // =====================================================================
    // SECTION 2: Per-test details
    // =====================================================================
    eprintln!("================================================================");
    eprintln!("PER-TEST DETAILS (test name, line #, expected/actual with visible WS)");
    eprintln!("================================================================\n");

    for f in &failures {
        let exp_vis = visualize_ws(&f.expected_line);
        let act_vis = visualize_ws(&f.actual_line);
        let exp_indent_len = f.expected_line.len() - f.expected_line.trim_start().len();
        let act_indent_len = f.actual_line.len() - f.actual_line.trim_start().len();
        let trimmed_content = f.expected_line.trim();
        let content_display = if trimmed_content.len() > 80 {
            &trimmed_content[..80]
        } else {
            trimmed_content
        };

        eprintln!("--- {} ---", f.test_name);
        eprintln!("  Line:     {}", f.line_num);
        eprintln!("  Content:  {:?}", content_display);
        eprintln!(
            "  Expected: [{}]{:?} (indent={})",
            exp_vis,
            &f.expected_line[..f.expected_line.len().min(100)],
            exp_indent_len
        );
        eprintln!(
            "  Actual:   [{}]{:?} (indent={})",
            act_vis,
            &f.actual_line[..f.actual_line.len().min(100)],
            act_indent_len
        );
        eprintln!("  Category: {}", f.category);
        eprintln!();
    }

    // =====================================================================
    // SECTION 3: Category summary
    // =====================================================================
    let mut cat_counts: HashMap<String, Vec<&WsFailure>> = HashMap::new();
    for f in &failures {
        // Extract the category prefix (before the first parenthesis)
        let cat_key = f
            .category
            .split('(')
            .next()
            .unwrap_or(&f.category)
            .trim()
            .to_string();
        cat_counts.entry(cat_key).or_insert_with(Vec::new).push(f);
    }

    let mut sorted_cats: Vec<_> = cat_counts.iter().collect();
    sorted_cats.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    eprintln!("================================================================");
    eprintln!("CATEGORY SUMMARY");
    eprintln!("================================================================\n");

    eprintln!("{:<35} {:>5} {:>6}", "Category", "Count", "%");
    eprintln!("{:-<35} {:-^5} {:-^6}", "", "", "");
    for (cat, tests) in &sorted_cats {
        let pct = if failures.is_empty() {
            0.0
        } else {
            tests.len() as f64 / failures.len() as f64 * 100.0
        };
        eprintln!("{:<35} {:>5} {:>5.1}%", cat, tests.len(), pct);
    }
    eprintln!("{:-<35} {:-^5} {:-^6}", "", "", "");
    eprintln!("{:<35} {:>5}", "TOTAL", failures.len());

    eprintln!("\n================================================================");
    eprintln!("DETAILED CATEGORY BREAKDOWN (with test names)");
    eprintln!("================================================================\n");

    for (cat, tests) in &sorted_cats {
        eprintln!("=== {} ({} tests) ===", cat, tests.len());
        // Sub-categorize by indent delta
        let mut indent_deltas: HashMap<i64, Vec<&WsFailure>> = HashMap::new();
        for f in *tests {
            let exp_indent =
                f.expected_line.len() as i64 - f.expected_line.trim_start().len() as i64;
            let act_indent = f.actual_line.len() as i64 - f.actual_line.trim_start().len() as i64;
            let delta = act_indent - exp_indent;
            indent_deltas.entry(delta).or_insert_with(Vec::new).push(f);
        }
        let mut sorted_deltas: Vec<_> = indent_deltas.iter().collect();
        sorted_deltas.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (delta, delta_tests) in &sorted_deltas {
            let sign = if **delta > 0 { "+" } else { "" };
            eprintln!(
                "  indent delta={}{}: {} tests",
                sign,
                delta,
                delta_tests.len()
            );
            for f in delta_tests.iter().take(5) {
                eprintln!(
                    "    - {} (L{}, exp_indent={}, act_indent={})",
                    f.test_name,
                    f.line_num,
                    f.expected_line.len() - f.expected_line.trim_start().len(),
                    f.actual_line.len() - f.actual_line.trim_start().len()
                );
            }
            if delta_tests.len() > 5 {
                eprintln!("    ... and {} more", delta_tests.len() - 5);
            }
        }
        eprintln!();
    }

    eprintln!("================================================================");
    eprintln!("END WHITESPACE-ONLY DIAGNOSTIC");
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_missing_comment() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Record: (test_name, line_num, expected_first80, actual_first80, expected_type, actual_type)
    struct MissingCommentRecord {
        test_name: String,
        line_num: usize,
        expected_first80: String,
        actual_first80: String,
        expected_type: &'static str,
        actual_type: &'static str,
    }

    let mut records: Vec<MissingCommentRecord> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Find first mismatch
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let exp_t = exp_lines[i].trim();
                let act_t = act_lines[i].trim();

                // Filter: expected starts with "//" and actual does NOT start with "//"
                if exp_t.starts_with("//") && !act_t.starts_with("//") {
                    // Categorize expected comment type
                    let expected_type: &'static str = if exp_t.starts_with("/// <reference") {
                        "FILE-REF"
                    } else if exp_t.starts_with("///") {
                        "TRIPLE-SLASH"
                    } else if exp_t.starts_with("/*") {
                        // This won't trigger since we check starts_with("//") first,
                        // but keep for completeness
                        "BLOCK-COMMENT"
                    } else {
                        "LINE-COMMENT"
                    };

                    // Categorize actual line type
                    let actual_type: &'static str = if act_t.is_empty() {
                        "BLANK"
                    } else if act_t.contains("use strict") {
                        "USE-STRICT"
                    } else if act_t.starts_with("exports.")
                        || act_t.contains("module.exports")
                        || act_t.contains("Object.defineProperty") && act_t.contains("exports")
                    {
                        "EXPORTS"
                    } else if act_t.starts_with("/*")
                        || act_t.starts_with("*")
                        || act_t.ends_with("*/")
                    {
                        "OTHER-COMMENT"
                    } else {
                        "CODE"
                    };

                    let exp_display = safe_trunc(exp_lines[i], 80).to_string();
                    let act_display = safe_trunc(act_lines[i], 80).to_string();

                    records.push(MissingCommentRecord {
                        test_name: name.clone(),
                        line_num: i + 1,
                        expected_first80: exp_display,
                        actual_first80: act_display,
                        expected_type,
                        actual_type,
                    });
                }
                break; // Only look at first mismatch per test
            }
        }
    }

    // -----------------------------------------------------------------------
    // Print individual records
    // -----------------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!(
        "MISSING-COMMENT DIAGNOSTIC: {} failing tests",
        records.len()
    );
    eprintln!("================================================================\n");

    for rec in &records {
        eprintln!(
            "[{}] L{} expected_type={} actual_type={}",
            rec.test_name, rec.line_num, rec.expected_type, rec.actual_type
        );
        eprintln!("  EXP: {:?}", rec.expected_first80);
        eprintln!("  ACT: {:?}", rec.actual_first80);
    }

    // -----------------------------------------------------------------------
    // Build cross-tabulation: expected_type x actual_type
    // -----------------------------------------------------------------------
    let mut combo_counts: HashMap<(&str, &str), Vec<&str>> = HashMap::new();
    for rec in &records {
        combo_counts
            .entry((rec.expected_type, rec.actual_type))
            .or_insert_with(Vec::new)
            .push(&rec.test_name);
    }

    // Sort combos by count descending
    let mut sorted_combos: Vec<_> = combo_counts.iter().collect();
    sorted_combos.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    eprintln!("\n================================================================");
    eprintln!("SUMMARY: expected_type x actual_type CROSS-TABULATION");
    eprintln!("================================================================\n");

    eprintln!(
        "{:<20} {:<20} {:>6}   Sample tests",
        "Expected-Type", "Actual-Type", "Count"
    );
    eprintln!("{:-<20} {:-<20} {:-^6}   {:-<50}", "", "", "", "");
    for ((exp_type, act_type), test_names) in &sorted_combos {
        // Collect up to 3 sample test names
        let samples: Vec<&str> = test_names.iter().take(3).copied().collect();
        let sample_str = samples.join(", ");
        eprintln!(
            "{:<20} {:<20} {:>6}   {}",
            exp_type,
            act_type,
            test_names.len(),
            sample_str
        );
    }

    let total: usize = sorted_combos.iter().map(|(_, v)| v.len()).sum();
    eprintln!("{:-<20} {:-<20} {:-^6}", "", "", "");
    eprintln!("{:<20} {:<20} {:>6}", "TOTAL", "", total);

    // -----------------------------------------------------------------------
    // Also print expected-type totals and actual-type totals
    // -----------------------------------------------------------------------
    eprintln!("\n--- Expected-Type totals ---");
    let mut exp_totals: HashMap<&str, usize> = HashMap::new();
    for rec in &records {
        *exp_totals.entry(rec.expected_type).or_insert(0) += 1;
    }
    let mut sorted_exp: Vec<_> = exp_totals.iter().collect();
    sorted_exp.sort_by(|a, b| b.1.cmp(a.1));
    for (typ, count) in &sorted_exp {
        eprintln!("  {:<20} {:>6}", typ, count);
    }

    eprintln!("\n--- Actual-Type totals ---");
    let mut act_totals: HashMap<&str, usize> = HashMap::new();
    for rec in &records {
        *act_totals.entry(rec.actual_type).or_insert(0) += 1;
    }
    let mut sorted_act: Vec<_> = act_totals.iter().collect();
    sorted_act.sort_by(|a, b| b.1.cmp(a.1));
    for (typ, count) in &sorted_act {
        eprintln!("  {:<20} {:>6}", typ, count);
    }

    // -----------------------------------------------------------------------
    // Detailed 3-sample listing per combination
    // -----------------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!("3 SAMPLE TESTS PER COMBINATION");
    eprintln!("================================================================\n");

    for ((exp_type, act_type), test_names) in &sorted_combos {
        eprintln!(
            "--- {} x {} ({} tests) ---",
            exp_type,
            act_type,
            test_names.len()
        );
        for (i, tname) in test_names.iter().enumerate() {
            if i >= 3 {
                break;
            }
            // Find the matching record to show context
            if let Some(rec) = records.iter().find(|r| r.test_name == **tname) {
                eprintln!("  [{}] {} L{}:", i + 1, tname, rec.line_num);
                eprintln!("       EXP: {:?}", rec.expected_first80);
                eprintln!("       ACT: {:?}", rec.actual_first80);
            }
        }
        if test_names.len() > 3 {
            eprintln!("  ... and {} more", test_names.len() - 3);
        }
        eprintln!();
    }

    eprintln!("================================================================");
    eprintln!("END MISSING-COMMENT DIAGNOSTIC");
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_extra_lines() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Categorize a single extra line
    fn cat_extra_line(line: &str) -> &'static str {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            "BLANK"
        } else if trimmed.contains("export {}") || trimmed == "export {};" {
            "EXPORT-EMPTY"
        } else if trimmed.starts_with("//// [") {
            "FILE-HEADER"
        } else if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            "COMMENT"
        } else {
            "CODE"
        }
    }

    // Record for each matching test
    struct ExtraLinesRecord {
        test_name: String,
        expected_count: usize,
        actual_count: usize,
        extra_lines: Vec<String>,
    }

    let mut records: Vec<ExtraLinesRecord> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);

        // Skip passing, missing-baseline, and crash/panic tests
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();

        // Condition 1: actual must have MORE lines than expected
        if act_lines.len() <= exp_lines.len() {
            continue;
        }

        // Condition 2: ALL overlapping lines must match exactly
        let all_overlap_match = exp_lines.iter().zip(act_lines.iter()).all(|(e, a)| e == a);
        if !all_overlap_match {
            continue;
        }

        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let extra: Vec<String> = act_lines[exp_lines.len()..]
            .iter()
            .map(|l| l.to_string())
            .collect();

        records.push(ExtraLinesRecord {
            test_name: name,
            expected_count: exp_lines.len(),
            actual_count: act_lines.len(),
            extra_lines: extra,
        });
    }

    // Sort records by test name for stable output
    records.sort_by(|a, b| a.test_name.cmp(&b.test_name));

    // ---------------------------------------------------------------
    // Categorize ALL extra lines across ALL matching tests
    // ---------------------------------------------------------------
    // category -> Vec<(test_name, Vec<extra_line_content>)>
    let mut category_samples: HashMap<&str, Vec<(String, Vec<String>)>> = HashMap::new();
    let mut category_line_count: HashMap<&str, usize> = HashMap::new();

    for rec in &records {
        // Categorize each individual extra line
        let mut per_line_cats: Vec<&str> = Vec::new();
        for line in &rec.extra_lines {
            let cat = cat_extra_line(line);
            per_line_cats.push(cat);
            *category_line_count.entry(cat).or_insert(0) += 1;
        }

        // For the per-test categorization, use the category of the
        // first non-blank extra line (or BLANK if all blank).
        let primary_cat = per_line_cats
            .iter()
            .find(|&&c| c != "BLANK")
            .copied()
            .unwrap_or("BLANK");

        category_samples
            .entry(primary_cat)
            .or_insert_with(Vec::new)
            .push((rec.test_name.clone(), rec.extra_lines.clone()));
    }

    // ---------------------------------------------------------------
    // Output
    // ---------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!("EXTRA-LINES-AT-END DIAGNOSTIC");
    eprintln!("================================================================");
    eprintln!(
        "Total tests with EXTRA-LINES-AT-END pattern: {}",
        records.len()
    );
    eprintln!("================================================================\n");

    // Summary of extra line types (line-level counts)
    eprintln!("--- EXTRA LINE TYPE SUMMARY (by individual line) ---\n");
    let total_extra_lines: usize = category_line_count.values().sum();
    let mut sorted_line_cats: Vec<_> = category_line_count.iter().collect();
    sorted_line_cats.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!("{:<20} {:>8} {:>7}", "Category", "Lines", "%");
    eprintln!("{:-<20} {:-^8} {:-^7}", "", "", "");
    for (cat, count) in &sorted_line_cats {
        let pct = **count as f64 / total_extra_lines as f64 * 100.0;
        eprintln!("{:<20} {:>8} {:>6.1}%", cat, count, pct);
    }
    eprintln!("{:-<20} {:-^8} {:-^7}", "", "", "");
    eprintln!("{:<20} {:>8}", "TOTAL", total_extra_lines);

    // Summary of extra line types (test-level counts, by primary category)
    eprintln!("\n--- EXTRA LINE TYPE SUMMARY (by test, primary category) ---\n");
    let mut sorted_test_cats: Vec<_> = category_samples.iter().collect();
    sorted_test_cats.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    eprintln!("{:<20} {:>8} {:>7}", "Category", "Tests", "%");
    eprintln!("{:-<20} {:-^8} {:-^7}", "", "", "");
    for (cat, tests) in &sorted_test_cats {
        let pct = tests.len() as f64 / records.len() as f64 * 100.0;
        eprintln!("{:<20} {:>8} {:>6.1}%", cat, tests.len(), pct);
    }
    eprintln!("{:-<20} {:-^8} {:-^7}", "", "", "");
    eprintln!("{:<20} {:>8}", "TOTAL", records.len());

    // Extra lines count distribution
    eprintln!("\n--- EXTRA LINE COUNT DISTRIBUTION ---\n");
    let mut count_dist: HashMap<usize, usize> = HashMap::new();
    for rec in &records {
        *count_dist.entry(rec.extra_lines.len()).or_insert(0) += 1;
    }
    let mut sorted_dist: Vec<_> = count_dist.iter().collect();
    sorted_dist.sort_by_key(|&(count, _)| *count);
    for (extra_count, num_tests) in &sorted_dist {
        eprintln!("  {} extra line(s): {} tests", extra_count, num_tests);
    }

    // ---------------------------------------------------------------
    // 5 samples per category
    // ---------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!("SAMPLES PER CATEGORY (up to 5 tests each)");
    eprintln!("================================================================\n");

    for (cat, tests) in &sorted_test_cats {
        eprintln!("=== {} ({} tests) ===\n", cat, tests.len());
        for (i, (test_name, extra_lines)) in tests.iter().enumerate() {
            if i >= 5 {
                break;
            }
            // Find the record to get line counts
            let rec = records.iter().find(|r| r.test_name == *test_name).unwrap();
            eprintln!(
                "  [{}] {} (expected: {} lines, actual: {} lines, extra: {})",
                i + 1,
                test_name,
                rec.expected_count,
                rec.actual_count,
                extra_lines.len()
            );
            for (j, line) in extra_lines.iter().enumerate() {
                let display = if line.len() > 120 {
                    &line[..120]
                } else {
                    line.as_str()
                };
                let line_cat = cat_extra_line(line);
                eprintln!("       extra[{}] [{}]: {:?}", j, line_cat, display);
            }
            eprintln!();
        }
        if tests.len() > 5 {
            eprintln!(
                "  ... and {} more tests in this category\n",
                tests.len() - 5
            );
        }
    }

    eprintln!("================================================================");
    eprintln!("END EXTRA-LINES-AT-END DIAGNOSTIC");
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_export_empty() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Collect tests where:
    //  - test fails
    //  - actual has more lines than expected
    //  - all overlapping lines match (i.e. extra lines are appended at the end)
    //  - the first extra line (at end) is `export {};`

    struct ExportEmptyCase {
        name: String,
        // Compiler options from the test
        target: String,
        module: String,
        module_detection: String,
        isolated_modules: String,
        verbatim_module_syntax: String,
        es_module_interop: String,
        // Source analysis
        source_has_import: bool,
        source_has_export: bool,
        source_has_module_detection_force: bool,
        // Raw @-directives from the source
        raw_directives: Vec<String>,
        // Baseline analysis
        expected_contains_export_empty: bool,
        expected_last_5_lines: Vec<String>,
        // Extra lines appended at end
        extra_lines: Vec<String>,
        // Line counts
        expected_line_count: usize,
        actual_line_count: usize,
    }

    let mut cases: Vec<ExportEmptyCase> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();

        // Must have actual > expected (extra lines at end)
        if act_lines.len() <= exp_lines.len() {
            continue;
        }

        // All overlapping lines must match
        let all_overlap_match = exp_lines.iter().zip(act_lines.iter()).all(|(e, a)| e == a);
        if !all_overlap_match {
            continue;
        }

        // The first extra line must be `export {};`
        let first_extra = act_lines[exp_lines.len()].trim();
        if first_extra != "export {};" {
            continue;
        }

        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(&path).unwrap_or_default();

        // Parse the test case to get structured compiler options
        let relative_test = format!(
            "tests/cases/compiler/{}",
            path.file_name().unwrap().to_string_lossy()
        );
        let test_case = tsc_rs_ast::parse_test_case(&relative_test, &source);
        let opts = &test_case.options;

        // Extract raw @-directives from source
        let raw_directives: Vec<String> = source
            .lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("// @")
            })
            .map(|l| l.trim().to_string())
            .collect();

        // Check if any source file contains actual import/export statements
        let source_has_import = test_case.files.iter().any(|f| {
            f.content.lines().any(|l| {
                let t = l.trim();
                t.starts_with("import ") || t.starts_with("import{") || t.starts_with("import(")
            })
        });
        let source_has_export = test_case.files.iter().any(|f| {
            f.content.lines().any(|l| {
                let t = l.trim();
                (t.starts_with("export ") || t.starts_with("export{") || t.starts_with("export*"))
                    && !t.starts_with("export {};")
            })
        });

        // Check for moduleDetection: force in the raw source (case-insensitive)
        let source_has_module_detection_force =
            source.to_lowercase().contains("@moduledetection: force")
                || source.to_lowercase().contains("@moduledetection:force");

        // Check if the expected baseline already contains `export {};`
        let expected_contains_export_empty = exp_lines.iter().any(|l| l.trim() == "export {};");

        // Last 5 lines of expected
        let start = if exp_lines.len() > 5 {
            exp_lines.len() - 5
        } else {
            0
        };
        let expected_last_5_lines: Vec<String> =
            exp_lines[start..].iter().map(|l| l.to_string()).collect();

        // Extra lines
        let extra_lines: Vec<String> = act_lines[exp_lines.len()..]
            .iter()
            .map(|l| l.to_string())
            .collect();

        cases.push(ExportEmptyCase {
            name,
            target: opts
                .target
                .map(|t| format!("{:?}", t))
                .unwrap_or_else(|| "(default)".to_string()),
            module: opts
                .module
                .map(|m| format!("{:?}", m))
                .unwrap_or_else(|| "(default)".to_string()),
            module_detection: opts
                .module_detection
                .clone()
                .unwrap_or_else(|| "(default)".to_string()),
            isolated_modules: opts
                .isolated_modules
                .map(|b| format!("{}", b))
                .unwrap_or_else(|| "(default)".to_string()),
            verbatim_module_syntax: opts
                .verbatim_module_syntax
                .map(|b| format!("{}", b))
                .unwrap_or_else(|| "(default)".to_string()),
            es_module_interop: opts
                .es_module_interop
                .map(|b| format!("{}", b))
                .unwrap_or_else(|| "(default)".to_string()),
            source_has_import,
            source_has_export,
            source_has_module_detection_force,
            raw_directives,
            expected_contains_export_empty,
            expected_last_5_lines,
            extra_lines,
            expected_line_count: exp_lines.len(),
            actual_line_count: act_lines.len(),
        });
    }

    cases.sort_by(|a, b| a.name.cmp(&b.name));

    eprintln!("\n================================================================");
    eprintln!("DIAG: EXTRA `export {{}}` AT END OF OUTPUT");
    eprintln!("================================================================");
    eprintln!(
        "Found {} tests with extra `export {{}};` appended at end\n",
        cases.len()
    );

    for (i, c) in cases.iter().enumerate() {
        eprintln!("--------------------------------------------------------------");
        eprintln!("[{}/{}] Test: {}", i + 1, cases.len(), c.name);
        eprintln!("--------------------------------------------------------------");

        // Compiler options
        eprintln!("  Compiler Options:");
        eprintln!("    @target:                {}", c.target);
        eprintln!("    @module:                {}", c.module);
        eprintln!("    @moduleDetection:       {}", c.module_detection);
        eprintln!("    @isolatedModules:       {}", c.isolated_modules);
        eprintln!("    @verbatimModuleSyntax:  {}", c.verbatim_module_syntax);
        eprintln!("    @esModuleInterop:       {}", c.es_module_interop);

        // Raw directives
        if !c.raw_directives.is_empty() {
            eprintln!("  Raw @-directives:");
            for d in &c.raw_directives {
                eprintln!("    {}", d);
            }
        }

        // Source analysis
        eprintln!("  Source Analysis:");
        eprintln!("    Has actual import statements:  {}", c.source_has_import);
        eprintln!("    Has actual export statements:  {}", c.source_has_export);
        eprintln!(
            "    Has @moduleDetection: force:   {}",
            c.source_has_module_detection_force
        );

        // Baseline analysis
        eprintln!("  Baseline Analysis:");
        eprintln!(
            "    Expected already contains `export {{}}`:  {}",
            c.expected_contains_export_empty
        );
        eprintln!(
            "    Expected line count: {}, Actual line count: {}",
            c.expected_line_count, c.actual_line_count
        );

        // Last 5 lines of expected
        eprintln!(
            "  Last {} lines of expected output:",
            c.expected_last_5_lines.len()
        );
        for (j, l) in c.expected_last_5_lines.iter().enumerate() {
            let line_num = c.expected_line_count - c.expected_last_5_lines.len() + j + 1;
            let display = if l.len() > 120 { &l[..120] } else { l.as_str() };
            eprintln!("    L{:>4}: {:?}", line_num, display);
        }

        // Extra lines emitted
        eprintln!("  Extra lines appended ({}):", c.extra_lines.len());
        for (j, l) in c.extra_lines.iter().enumerate() {
            let display = if l.len() > 120 { &l[..120] } else { l.as_str() };
            eprintln!("    extra[{}]: {:?}", j, display);
        }
        eprintln!();
    }

    // Summary table
    eprintln!("================================================================");
    eprintln!("SUMMARY");
    eprintln!("================================================================");
    eprintln!(
        "Total tests with extra `export {{}};` at end: {}",
        cases.len()
    );
    eprintln!();

    // Group by module detection
    let mut by_module_detection: HashMap<String, usize> = HashMap::new();
    for c in &cases {
        *by_module_detection
            .entry(c.module_detection.clone())
            .or_insert(0) += 1;
    }
    eprintln!("  By @moduleDetection:");
    let mut sorted: Vec<_> = by_module_detection.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (k, v) in &sorted {
        eprintln!("    {:>5} {}", v, k);
    }

    // Group by module kind
    let mut by_module: HashMap<String, usize> = HashMap::new();
    for c in &cases {
        *by_module.entry(c.module.clone()).or_insert(0) += 1;
    }
    eprintln!("  By @module:");
    let mut sorted: Vec<_> = by_module.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (k, v) in &sorted {
        eprintln!("    {:>5} {}", v, k);
    }

    // Group by target
    let mut by_target: HashMap<String, usize> = HashMap::new();
    for c in &cases {
        *by_target.entry(c.target.clone()).or_insert(0) += 1;
    }
    eprintln!("  By @target:");
    let mut sorted: Vec<_> = by_target.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (k, v) in &sorted {
        eprintln!("    {:>5} {}", v, k);
    }

    // Does expected already contain export {}?
    let already_has = cases
        .iter()
        .filter(|c| c.expected_contains_export_empty)
        .count();
    let does_not_have = cases
        .iter()
        .filter(|c| !c.expected_contains_export_empty)
        .count();
    eprintln!("  Expected baseline already contains `export {{}}`:");
    eprintln!("    Yes: {}", already_has);
    eprintln!("    No:  {}", does_not_have);

    // Source has real imports/exports?
    let has_import = cases.iter().filter(|c| c.source_has_import).count();
    let has_export = cases.iter().filter(|c| c.source_has_export).count();
    let has_neither = cases
        .iter()
        .filter(|c| !c.source_has_import && !c.source_has_export)
        .count();
    eprintln!("  Source has real import/export statements:");
    eprintln!("    Has imports:  {}", has_import);
    eprintln!("    Has exports:  {}", has_export);
    eprintln!("    Has neither:  {}", has_neither);

    // How many use moduleDetection: force?
    let force_count = cases
        .iter()
        .filter(|c| c.source_has_module_detection_force)
        .count();
    eprintln!("  Uses @moduleDetection: force: {}", force_count);

    eprintln!("\n================================================================");
    eprintln!("END DIAG: EXTRA `export {{}}` AT END");
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_extra_comment() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Record: (test_name, line_number, actual_line_trunc, expected_line_trunc, actual_type, expected_type)
    struct Hit {
        test_name: String,
        line_number: usize,
        actual_line: String,
        expected_line: String,
        actual_type: &'static str,
        expected_type: &'static str,
    }

    let mut hits: Vec<Hit> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Find the first line where actual != expected
        let min_len = exp_lines.len().min(act_lines.len());
        let mut mismatch_idx: Option<usize> = None;
        for i in 0..min_len {
            if act_lines[i] != exp_lines[i] {
                mismatch_idx = Some(i);
                break;
            }
        }
        // If lines matched up to min_len but lengths differ, the first mismatch is at min_len
        if mismatch_idx.is_none() && act_lines.len() != exp_lines.len() {
            mismatch_idx = Some(min_len);
        }

        let Some(idx) = mismatch_idx else {
            continue;
        };

        // Get actual and expected lines at the mismatch point (may be beyond one side's length)
        let actual_line = if idx < act_lines.len() {
            act_lines[idx]
        } else {
            "<EOF>"
        };
        let expected_line = if idx < exp_lines.len() {
            exp_lines[idx]
        } else {
            "<EOF>"
        };

        // Filter: actual starts with "//" but expected does not
        if !actual_line.trim_start().starts_with("//") {
            continue;
        }
        if expected_line.trim_start().starts_with("//") {
            continue;
        }

        // Categorize the actual comment type
        let actual_trimmed = actual_line.trim_start();
        let actual_type: &'static str = if actual_trimmed.starts_with("//// [") {
            "FILE-HEADER"
        } else if actual_trimmed.starts_with("/// <reference") {
            "FILE-REF"
        } else if actual_trimmed.starts_with("///") {
            "TRIPLE-SLASH-DIRECTIVE"
        } else {
            "LINE-COMMENT"
        };

        // Categorize what the expected line IS
        let exp_trimmed = expected_line.trim();
        let expected_type: &'static str = if exp_trimmed.is_empty() || expected_line == "<EOF>" {
            "BLANK"
        } else if exp_trimmed.contains("use strict") {
            "USE-STRICT"
        } else if exp_trimmed.starts_with("exports.")
            || exp_trimmed.starts_with("module.exports")
            || exp_trimmed == "export {};"
            || exp_trimmed.starts_with("export ")
        {
            "EXPORTS"
        } else if exp_trimmed.starts_with("//") || exp_trimmed.starts_with("/*") {
            "COMMENT"
        } else {
            "CODE"
        };

        hits.push(Hit {
            test_name: name,
            line_number: idx + 1, // 1-based
            actual_line: safe_trunc(actual_line, 100).to_string(),
            expected_line: safe_trunc(expected_line, 100).to_string(),
            actual_type,
            expected_type,
        });
    }

    eprintln!("\n================================================================");
    eprintln!("DIAG: EXTRA-COMMENT first mismatch patterns");
    eprintln!("================================================================");
    eprintln!(
        "Total hits (actual starts with '//', expected does not): {}",
        hits.len()
    );

    // Cross-tabulation: actual_type x expected_type
    let mut cross: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, h) in hits.iter().enumerate() {
        cross
            .entry((h.actual_type, h.expected_type))
            .or_insert_with(Vec::new)
            .push(i);
    }

    // Collect and sort by count descending
    let mut sorted_combos: Vec<((&str, &str), Vec<usize>)> = cross.into_iter().collect();
    sorted_combos.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    eprintln!("\n--- Cross-tabulation: actual-comment-type x expected-line-type ---");
    eprintln!(
        "{:<30} {:<15} {:>5}",
        "ACTUAL-TYPE", "EXPECTED-TYPE", "COUNT"
    );
    eprintln!("{}", "-".repeat(55));
    for &(ref combo, ref indices) in &sorted_combos {
        eprintln!("{:<30} {:<15} {:>5}", combo.0, combo.1, indices.len());
    }

    // For top 3 combinations, print 5 sample tests with full details
    eprintln!("\n--- Top 3 combinations: 5 samples each ---");
    for (rank, &(ref combo, ref indices)) in sorted_combos.iter().take(3).enumerate() {
        eprintln!(
            "\n=== #{} {} x {} ({} total) ===",
            rank + 1,
            combo.0,
            combo.1,
            indices.len()
        );
        for &idx in indices.iter().take(5) {
            let h = &hits[idx];
            eprintln!("  Test: {}", h.test_name);
            eprintln!("  Line: {}", h.line_number);
            eprintln!("  Expected: {}", h.expected_line);
            eprintln!("  Actual:   {}", h.actual_line);
            eprintln!("  ---");
        }
    }

    eprintln!("\n================================================================");
    eprintln!("END DIAG: EXTRA-COMMENT first mismatch patterns");
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_act_blank() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Record for each matching test:
    //   test name, line number, expected line (trimmed, first 100 chars),
    //   preceding 2 actual lines, following 2 actual lines
    struct ActBlankRecord {
        test_name: String,
        line_num: usize,
        expected_trimmed: String,
        prev_2_actual: Vec<String>,
        next_2_actual: Vec<String>,
        // Content pattern flags
        is_var_decl: bool,
        is_func_class_enum: bool,
        is_expression: bool,
        prev_is_closing: bool, // previous actual line ends with } or ;
    }

    let mut records: Vec<ActBlankRecord> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Find first differing line where actual is blank and expected is non-blank
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let act_t = act_lines[i].trim();
                let exp_t = exp_lines[i].trim();

                // ACT-BLANK: actual is blank/empty, expected is non-blank
                // and expected is NOT a comment or whitespace-only
                if act_t.is_empty()
                    && !exp_t.is_empty()
                    && !exp_t.starts_with("//")
                    && !exp_t.starts_with("/*")
                    && !exp_t.starts_with("*")
                {
                    // Gather preceding 2 actual lines
                    let mut prev_2: Vec<String> = Vec::new();
                    for offset in [2, 1] {
                        if i >= offset {
                            prev_2.push(act_lines[i - offset].to_string());
                        }
                    }

                    // Gather following 2 actual lines
                    let mut next_2: Vec<String> = Vec::new();
                    for offset in 1..=2 {
                        if i + offset < act_lines.len() {
                            next_2.push(act_lines[i + offset].to_string());
                        }
                    }

                    // Content pattern checks on the expected line
                    let exp_lower = exp_t.to_lowercase();
                    let is_var_decl = exp_t.starts_with("var ")
                        || exp_t.starts_with("let ")
                        || exp_t.starts_with("const ");
                    let is_func_class_enum = exp_t.starts_with("function ")
                        || exp_t.starts_with("class ")
                        || exp_t.starts_with("enum ")
                        || exp_t.starts_with("async function ")
                        || exp_lower.starts_with("function*")
                        || exp_t.starts_with("export function ")
                        || exp_t.starts_with("export class ")
                        || exp_t.starts_with("export enum ")
                        || exp_t.starts_with("export default function")
                        || exp_t.starts_with("export default class");
                    let is_expression = !is_var_decl
                        && !is_func_class_enum
                        && !exp_t.starts_with("//")
                        && !exp_t.starts_with("/*")
                        && !exp_t.starts_with("}")
                        && !exp_t.starts_with("{")
                        && !exp_t.is_empty();

                    // Check if previous actual line is a closing brace or semicolon
                    let prev_is_closing = if i > 0 {
                        let prev_t = act_lines[i - 1].trim();
                        prev_t.ends_with('}')
                            || prev_t.ends_with("};")
                            || prev_t.ends_with(';')
                            || prev_t == "}"
                            || prev_t == "};"
                            || prev_t == "});"
                            || prev_t == "})();"
                    } else {
                        false
                    };

                    let exp_trunc = if exp_t.len() > 100 {
                        format!("{}...", &exp_t[..safe_trunc(exp_t, 100).len()])
                    } else {
                        exp_t.to_string()
                    };

                    records.push(ActBlankRecord {
                        test_name: name.clone(),
                        line_num: i + 1,
                        expected_trimmed: exp_trunc,
                        prev_2_actual: prev_2,
                        next_2_actual: next_2,
                        is_var_decl,
                        is_func_class_enum,
                        is_expression,
                        prev_is_closing,
                    });
                }
                break; // Only consider the first mismatch per test
            }
        }
    }

    // -----------------------------------------------------------------------
    // Output section 1: Summary statistics
    // -----------------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!("DIAG: ACT-BLANK (exp: OTHER) ANALYSIS");
    eprintln!("================================================================");
    eprintln!("Total tests matching ACT-BLANK pattern: {}", records.len());
    eprintln!();

    // -----------------------------------------------------------------------
    // Output section 2: Content pattern breakdown
    // -----------------------------------------------------------------------
    let var_decl_count = records.iter().filter(|r| r.is_var_decl).count();
    let func_class_enum_count = records.iter().filter(|r| r.is_func_class_enum).count();
    let expression_count = records.iter().filter(|r| r.is_expression).count();
    let prev_closing_count = records.iter().filter(|r| r.prev_is_closing).count();

    eprintln!("--- EXPECTED LINE CONTENT PATTERNS ---");
    eprintln!(
        "  Variable declarations (var/let/const):  {:>4}",
        var_decl_count
    );
    eprintln!(
        "  Function/class/enum declarations:       {:>4}",
        func_class_enum_count
    );
    eprintln!(
        "  Expression content (other code):        {:>4}",
        expression_count
    );
    eprintln!(
        "  Previous actual line ends with }}/;:     {:>4}",
        prev_closing_count
    );
    eprintln!();

    // -----------------------------------------------------------------------
    // Output section 3: Subcategorize expected line content more finely
    // -----------------------------------------------------------------------
    let mut exp_subcats: HashMap<String, usize> = HashMap::new();
    for r in &records {
        let et = r.expected_trimmed.as_str();
        let subcat = if et.starts_with("var ") {
            "var-decl"
        } else if et.starts_with("let ") {
            "let-decl"
        } else if et.starts_with("const ") {
            "const-decl"
        } else if et.starts_with("function ")
            || et.starts_with("async function")
            || et.starts_with("function*")
        {
            "function-decl"
        } else if et.starts_with("class ") {
            "class-decl"
        } else if et.starts_with("enum ") {
            "enum-decl"
        } else if et.starts_with("export ") {
            "export-stmt"
        } else if et.starts_with("Object.defineProperty") {
            "Object.defineProperty"
        } else if et.starts_with("return ") || et == "return;" {
            "return-stmt"
        } else if et.starts_with("if ") || et.starts_with("if(") {
            "if-stmt"
        } else if et.starts_with("for ") || et.starts_with("for(") || et.starts_with("while ") {
            "loop-stmt"
        } else if et.starts_with("throw ") {
            "throw-stmt"
        } else if et.starts_with("switch ") || et.starts_with("switch(") {
            "switch-stmt"
        } else if et.starts_with("try ") || et.starts_with("try{") || et == "try {" {
            "try-stmt"
        } else if et.starts_with("catch") {
            "catch-clause"
        } else if et == "}" || et == "};" || et.starts_with("})") {
            "closing-brace"
        } else if et == "{" {
            "opening-brace"
        } else if et.contains("require(") {
            "require-call"
        } else if et.contains("exports.") || et.contains("module.exports") {
            "exports-access"
        } else if et.contains("\"use strict\"") || et.contains("'use strict'") {
            "use-strict"
        } else if et.contains("= ") && et.contains(";") {
            "assignment"
        } else if et.ends_with(";") {
            "expression-stmt"
        } else if et.ends_with("{") {
            "block-opener"
        } else if et.ends_with(",") {
            "comma-continuation"
        } else {
            "other"
        };
        *exp_subcats.entry(subcat.to_string()).or_insert(0) += 1;
    }

    eprintln!("--- EXPECTED LINE SUBCATEGORIES ---");
    let mut sorted_subcats: Vec<_> = exp_subcats.iter().collect();
    sorted_subcats.sort_by(|a, b| b.1.cmp(a.1));
    for (subcat, count) in &sorted_subcats {
        eprintln!("  {:>4}  {}", count, subcat);
    }
    eprintln!();

    // -----------------------------------------------------------------------
    // Output section 4: Previous-line pattern analysis
    // -----------------------------------------------------------------------
    let mut prev_line_cats: HashMap<String, usize> = HashMap::new();
    for r in &records {
        let prev_line = r.prev_2_actual.last().unwrap_or(&String::new()).clone();
        let pt = prev_line.trim();
        let cat = if pt.is_empty() {
            "blank"
        } else if pt == "}" {
            "}"
        } else if pt == "};" {
            "};"
        } else if pt.ends_with("};") {
            "ends-with-};"
        } else if pt.ends_with('}') {
            "ends-with-}"
        } else if pt.ends_with(';') {
            "ends-with-;"
        } else if pt.ends_with(',') {
            "ends-with-,"
        } else if pt.ends_with('{') {
            "ends-with-{"
        } else if pt.starts_with("//") || pt.starts_with("/*") {
            "comment"
        } else {
            "other"
        };
        *prev_line_cats.entry(cat.to_string()).or_insert(0) += 1;
    }

    eprintln!("--- PREVIOUS ACTUAL LINE CATEGORIES ---");
    let mut sorted_prev: Vec<_> = prev_line_cats.iter().collect();
    sorted_prev.sort_by(|a, b| b.1.cmp(a.1));
    for (cat, count) in &sorted_prev {
        eprintln!("  {:>4}  {}", count, cat);
    }
    eprintln!();

    // -----------------------------------------------------------------------
    // Output section 5: Top 10 sample tests with full context
    // -----------------------------------------------------------------------
    eprintln!("================================================================");
    eprintln!("TOP 10 SAMPLES WITH CONTEXT");
    eprintln!("(prev 2 actual lines | BLANK actual | expected line | next 2 actual lines)");
    eprintln!("================================================================\n");

    for (idx, r) in records.iter().enumerate() {
        if idx >= 10 {
            break;
        }
        eprintln!(
            "--- Sample #{} : {} (line {}) ---",
            idx + 1,
            r.test_name,
            r.line_num
        );

        // Show prev 2 actual lines
        for (j, line) in r.prev_2_actual.iter().enumerate() {
            let line_num = r.line_num as i64 - (r.prev_2_actual.len() as i64 - j as i64);
            eprintln!("  ACT L{:>4}: {:?}", line_num, safe_trunc(line, 120));
        }

        // The blank actual line
        eprintln!("  ACT L{:>4}: \"\"  <-- BLANK", r.line_num);

        // The expected line
        eprintln!("  EXP L{:>4}: {:?}", r.line_num, r.expected_trimmed);

        // Show next 2 actual lines
        for (j, line) in r.next_2_actual.iter().enumerate() {
            let line_num = r.line_num + 1 + j;
            eprintln!("  ACT L{:>4}: {:?}", line_num, safe_trunc(line, 120));
        }

        // Show flags
        let mut flags = Vec::new();
        if r.is_var_decl {
            flags.push("VAR-DECL");
        }
        if r.is_func_class_enum {
            flags.push("FUNC/CLASS/ENUM");
        }
        if r.is_expression {
            flags.push("EXPRESSION");
        }
        if r.prev_is_closing {
            flags.push("PREV-CLOSING");
        }
        eprintln!("  FLAGS: [{}]", flags.join(", "));
        eprintln!();
    }

    // -----------------------------------------------------------------------
    // Output section 6: Full test name list
    // -----------------------------------------------------------------------
    eprintln!("================================================================");
    eprintln!("ALL {} TEST NAMES", records.len());
    eprintln!("================================================================");
    let mut sorted_names: Vec<_> = records.iter().map(|r| r.test_name.as_str()).collect();
    sorted_names.sort();
    for name in &sorted_names {
        eprintln!("  {}", name);
    }
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_wrong_header() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Extract the file name from a "//// [someFile.js]" header line.
    fn extract_header_name(header: &str) -> &str {
        let trimmed = header.trim();
        if let Some(start) = trimmed.find('[') {
            if let Some(end) = trimmed.find(']') {
                return trimmed[start + 1..end].trim();
            }
        }
        trimmed
    }

    // Classify the type of header difference.
    fn classify_header_diff(expected_name: &str, actual_name: &str) -> &'static str {
        // OUTDIR-PREFIX: expected has a path prefix like "out/" that actual doesn't
        // e.g. expected="out/foo.js" actual="foo.js"
        if expected_name.contains('/') && !actual_name.contains('/') {
            let exp_file = expected_name.rsplit('/').next().unwrap_or(expected_name);
            if exp_file == actual_name {
                return "OUTDIR-PREFIX";
            }
        }
        if actual_name.contains('/') && !expected_name.contains('/') {
            let act_file = actual_name.rsplit('/').next().unwrap_or(actual_name);
            if act_file == expected_name {
                return "OUTDIR-PREFIX";
            }
        }

        // OUTFILE: expected has a combined file like "out.js" while actual has a regular name
        // e.g. expected="out.js" actual="myModule.js"
        let exp_stem = expected_name.rsplit('/').next().unwrap_or(expected_name);
        let act_stem = actual_name.rsplit('/').next().unwrap_or(actual_name);
        let exp_base = exp_stem.rsplit('.').last().unwrap_or(exp_stem);
        let act_base = act_stem.rsplit('.').last().unwrap_or(act_stem);
        // Detect outFile pattern: one is "out.js"/"out.d.ts" etc. while the other isn't
        if (exp_base == "out" || exp_base == "outFile" || exp_base == "bundle")
            && act_base != "out"
            && act_base != "outFile"
            && act_base != "bundle"
        {
            return "OUTFILE";
        }
        if (act_base == "out" || act_base == "outFile" || act_base == "bundle")
            && exp_base != "out"
            && exp_base != "outFile"
            && exp_base != "bundle"
        {
            return "OUTFILE";
        }

        // EXTENSION-DIFF: same base name but different extension (.jsx vs .js etc)
        let exp_no_ext = expected_name.rsplit('.').skip(1).collect::<Vec<_>>();
        let act_no_ext = actual_name.rsplit('.').skip(1).collect::<Vec<_>>();
        let exp_ext = expected_name.rsplit('.').next().unwrap_or("");
        let act_ext = actual_name.rsplit('.').next().unwrap_or("");
        if !exp_no_ext.is_empty() && !act_no_ext.is_empty() {
            // Reconstruct without extension
            let exp_without: String = exp_no_ext.into_iter().rev().collect::<Vec<_>>().join(".");
            let act_without: String = act_no_ext.into_iter().rev().collect::<Vec<_>>().join(".");
            if exp_without == act_without && exp_ext != act_ext {
                return "EXTENSION-DIFF";
            }
        }

        // PATH-DEPTH: same filename but different path depth
        let exp_filename = expected_name.rsplit('/').next().unwrap_or(expected_name);
        let act_filename = actual_name.rsplit('/').next().unwrap_or(actual_name);
        if exp_filename == act_filename && expected_name != actual_name {
            return "PATH-DEPTH";
        }

        // NAME-DIFF: completely different file names
        "NAME-DIFF"
    }

    // Category -> Vec<(test_name, expected_header, actual_header)>
    let mut categories: HashMap<&str, Vec<(String, String, String)>> = HashMap::new();
    let mut total_wrong_header = 0;

    // Track compiler options for these tests
    let mut has_full_emit_paths = 0;
    let mut has_out_dir = 0;
    let mut has_out_file = 0;
    let mut full_emit_paths_names: Vec<String> = Vec::new();
    let mut out_dir_names: Vec<String> = Vec::new();
    let mut out_file_names: Vec<String> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Find first mismatch
        let mut found = false;
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let et = exp_lines[i].trim();
                let at = act_lines[i].trim();
                if et.starts_with("//// [") && at.starts_with("//// [") {
                    total_wrong_header += 1;
                    let exp_name = extract_header_name(et);
                    let act_name = extract_header_name(at);
                    let cat = classify_header_diff(exp_name, act_name);
                    categories.entry(cat).or_insert_with(Vec::new).push((
                        name.clone(),
                        et.to_string(),
                        at.to_string(),
                    ));

                    // Check compiler options
                    let source = std::fs::read_to_string(&path).unwrap_or_default();
                    let source_lower = source.to_lowercase();

                    let has_fep = source_lower.lines().any(|l| {
                        let t = l.trim();
                        t.starts_with("//")
                            && t.to_lowercase().contains("@fullemitpaths")
                            && t.to_lowercase().contains("true")
                    });
                    let has_od = source_lower.lines().any(|l| {
                        let t = l.trim();
                        t.starts_with("//") && t.to_lowercase().contains("@outdir")
                    });
                    let has_of = source_lower.lines().any(|l| {
                        let t = l.trim();
                        t.starts_with("//") && t.to_lowercase().contains("@outfile")
                    });

                    if has_fep {
                        has_full_emit_paths += 1;
                        if full_emit_paths_names.len() < 10 {
                            full_emit_paths_names.push(name.clone());
                        }
                    }
                    if has_od {
                        has_out_dir += 1;
                        if out_dir_names.len() < 10 {
                            out_dir_names.push(name.clone());
                        }
                    }
                    if has_of {
                        has_out_file += 1;
                        if out_file_names.len() < 10 {
                            out_file_names.push(name.clone());
                        }
                    }
                    found = true;
                }
                break;
            }
        }
        let _ = found;
    }

    // Print results
    eprintln!("\n================================================================");
    eprintln!("WRONG-FILE-HEADER DIAGNOSTIC");
    eprintln!("================================================================");
    eprintln!(
        "Total tests with WRONG-FILE-HEADER first mismatch: {}",
        total_wrong_header
    );
    eprintln!("================================================================\n");

    // Sort categories by count descending
    let mut sorted_cats: Vec<_> = categories.iter().collect();
    sorted_cats.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (cat, samples) in &sorted_cats {
        eprintln!("=== {} ({} tests) ===", cat, samples.len());
        for (i, (test_name, exp_hdr, act_hdr)) in samples.iter().enumerate() {
            if i >= 5 {
                break;
            }
            eprintln!("  [{}]", test_name);
            eprintln!("    expected: {}", exp_hdr);
            eprintln!("    actual:   {}", act_hdr);
        }
        eprintln!();
    }

    // Summary table
    eprintln!("================================================================");
    eprintln!("CATEGORY SUMMARY");
    eprintln!("================================================================");
    eprintln!("{:<20} {:>8}", "Category", "Count");
    eprintln!("{:-<20} {:-^8}", "", "");
    for (cat, samples) in &sorted_cats {
        eprintln!("{:<20} {:>8}", cat, samples.len());
    }
    eprintln!("{:-<20} {:-^8}", "", "");
    eprintln!("{:<20} {:>8}", "TOTAL", total_wrong_header);
    eprintln!();

    // Compiler options analysis
    eprintln!("================================================================");
    eprintln!("COMPILER OPTIONS AMONG WRONG-FILE-HEADER TESTS");
    eprintln!("================================================================");
    eprintln!(
        "  @fullEmitPaths:true  {:>5} / {}",
        has_full_emit_paths, total_wrong_header
    );
    if !full_emit_paths_names.is_empty() {
        eprintln!("    examples: {}", full_emit_paths_names.join(", "));
    }
    eprintln!(
        "  @outDir              {:>5} / {}",
        has_out_dir, total_wrong_header
    );
    if !out_dir_names.is_empty() {
        eprintln!("    examples: {}", out_dir_names.join(", "));
    }
    eprintln!(
        "  @outFile             {:>5} / {}",
        has_out_file, total_wrong_header
    );
    if !out_file_names.is_empty() {
        eprintln!("    examples: {}", out_file_names.join(", "));
    }
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_code_diff() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Subcategory -> Vec<(test_name, expected_line, actual_line)>
    let mut subcats: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Find first mismatched line pair
        let mut found = false;
        for i in 0..std::cmp::min(exp_lines.len(), act_lines.len()) {
            if exp_lines[i] != act_lines[i] {
                let exp_t = exp_lines[i].trim();
                let act_t = act_lines[i].trim();

                // Filter: only keep CODE-DIFF (not matching any of the known specific patterns)
                let is_code_diff = {
                    if exp_t == act_t {
                        false // whitespace-only
                    } else if act_t.is_empty() || exp_t.is_empty() {
                        false // blank
                    } else if act_t == "=;" || act_t == "= ;" {
                        false // bogus-equals-semi
                    } else if act_t.starts_with("export {}") && !exp_t.starts_with("export {}") {
                        false // spurious-export-empty
                    } else if exp_t.starts_with("Object.defineProperty")
                        && !act_t.starts_with("Object.defineProperty")
                    {
                        false // missing-object-define
                    } else if exp_t.starts_with("//") && !act_t.starts_with("//") {
                        false // missing-comment
                    } else if act_t.starts_with("//") && !exp_t.starts_with("//") {
                        false // extra-comment
                    } else if exp_t.contains("exports.") && !act_t.contains("exports.") {
                        false // missing-exports-dot
                    } else if act_t.contains("exports.") && !exp_t.contains("exports.") {
                        false // extra-exports-dot
                    } else if exp_t.starts_with("\"use strict\"")
                        && !act_t.starts_with("\"use strict\"")
                    {
                        false // missing-use-strict
                    } else if act_t.starts_with("\"use strict\"")
                        && !exp_t.starts_with("\"use strict\"")
                    {
                        false // extra-use-strict
                    } else if exp_t.starts_with("//// [") || act_t.starts_with("//// [") {
                        false // file-header
                    } else if exp_t.contains("//")
                        || act_t.contains("//")
                        || exp_t.contains("/*")
                        || act_t.contains("/*")
                    {
                        false // comment
                    } else if exp_t.contains("use strict") || act_t.contains("use strict") {
                        false // use-strict
                    } else {
                        true // CODE-DIFF
                    }
                };

                if !is_code_diff {
                    break;
                }

                // Now subcategorize the CODE-DIFF
                let subcat = categorize_code_diff(exp_t, act_t);
                subcats.entry(subcat).or_insert_with(Vec::new).push((
                    name.clone(),
                    exp_lines[i].to_string(),
                    act_lines[i].to_string(),
                ));

                found = true;
                break;
            }
        }

        // If all overlapping lines match but different counts, also check the boundary
        if !found && exp_lines.len() != act_lines.len() {
            let all_match = exp_lines.iter().zip(act_lines.iter()).all(|(e, a)| e == a);
            if !all_match {
                continue;
            }
            // The mismatch is at the boundary (extra/missing lines at the end).
            // This is not a CODE-DIFF (it's extra/missing lines at end), skip.
        }
    }

    // Print summary
    let total: usize = subcats.values().map(|v| v.len()).sum();
    eprintln!("\n================================================================");
    eprintln!("CODE-DIFF SUBCATEGORY ANALYSIS");
    eprintln!("================================================================");
    eprintln!("Total CODE-DIFF failures analyzed: {}", total);
    eprintln!("================================================================\n");

    // Sort by count descending
    let mut sorted: Vec<_> = subcats.iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    eprintln!("{:<25} {:>6} {:>7}", "Subcategory", "Count", "Pct");
    eprintln!("{:-<25} {:-^6} {:-^7}", "", "", "");
    for (subcat, samples) in &sorted {
        eprintln!(
            "{:<25} {:>6} {:>6.1}%",
            subcat,
            samples.len(),
            samples.len() as f64 / total as f64 * 100.0
        );
    }
    eprintln!("{:-<25} {:-^6} {:-^7}", "", "", "");
    eprintln!("{:<25} {:>6}", "TOTAL", total);

    // For the top 5 subcategories, print 3 samples
    eprintln!("\n================================================================");
    eprintln!("SAMPLES FOR TOP 5 SUBCATEGORIES");
    eprintln!("================================================================\n");

    for (rank, (subcat, samples)) in sorted.iter().enumerate() {
        if rank >= 5 {
            break;
        }
        let limit = 3;
        eprintln!("=== {} (count: {}) ===", subcat, samples.len());
        for (j, (test_name, exp_line, act_line)) in samples.iter().enumerate() {
            if j >= limit {
                break;
            }
            eprintln!("  [{}] Test: {}", j + 1, test_name);
            eprintln!("       EXP: {:?}", safe_trunc(exp_line.trim(), 100));
            eprintln!("       ACT: {:?}", safe_trunc(act_line.trim(), 100));
        }
        if samples.len() > limit {
            eprintln!("  ... and {} more", samples.len() - limit);
        }
        eprintln!();
    }

    eprintln!("================================================================");
}

fn categorize_code_diff(exp_t: &str, act_t: &str) -> String {
    // VAR-TO-LET: expected has "var " but actual has "let " or "const "
    if exp_t.contains("var ")
        && (act_t.contains("let ") || act_t.contains("const "))
        && !exp_t.contains("let ")
        && !exp_t.contains("const ")
    {
        return "VAR-TO-LET".to_string();
    }

    // LET-TO-VAR: expected has "let " or "const " but actual has "var "
    if (exp_t.contains("let ") || exp_t.contains("const "))
        && act_t.contains("var ")
        && !exp_t.contains("var ")
        && !act_t.contains("let ")
        && !act_t.contains("const ")
    {
        return "LET-TO-VAR".to_string();
    }

    // MISSING-SEMICOLON: actual is expected without trailing semicolon
    if exp_t.ends_with(';') && !act_t.ends_with(';') {
        let exp_no_semi = &exp_t[..exp_t.len() - 1];
        if exp_no_semi == act_t {
            return "MISSING-SEMICOLON".to_string();
        }
    }

    // EXTRA-SEMICOLON: actual has trailing semicolon that expected doesn't
    if act_t.ends_with(';') && !exp_t.ends_with(';') {
        let act_no_semi = &act_t[..act_t.len() - 1];
        if act_no_semi == exp_t {
            return "EXTRA-SEMICOLON".to_string();
        }
    }

    // QUOTE-DIFF: lines differ only in quote style (' vs ")
    if exp_t.replace('\'', "\"") == act_t.replace('\'', "\"") {
        return "QUOTE-DIFF".to_string();
    }

    // FUNCTION-KEYWORD: one has "function " the other doesn't (arrow vs function)
    if (exp_t.contains("function ") && !act_t.contains("function "))
        || (!exp_t.contains("function ") && act_t.contains("function "))
    {
        // Check that it's likely arrow-vs-function conversion, not something else
        if exp_t.contains("=>")
            || act_t.contains("=>")
            || exp_t.contains("function ")
            || act_t.contains("function ")
        {
            return "FUNCTION-KEYWORD".to_string();
        }
    }

    // DECORATOR-SYNTAX: involves __decorate or __param patterns
    if exp_t.contains("__decorate")
        || act_t.contains("__decorate")
        || exp_t.contains("__param")
        || act_t.contains("__param")
    {
        return "DECORATOR-SYNTAX".to_string();
    }

    // TYPEOF-REQUIRE: involves typeof require or __importStar etc
    if exp_t.contains("typeof require")
        || act_t.contains("typeof require")
        || exp_t.contains("__importStar")
        || act_t.contains("__importStar")
        || exp_t.contains("__importDefault")
        || act_t.contains("__importDefault")
        || exp_t.contains("__awaiter")
        || act_t.contains("__awaiter")
        || exp_t.contains("__generator")
        || act_t.contains("__generator")
    {
        return "TYPEOF-REQUIRE".to_string();
    }

    // NAMESPACE-IIFE: involves (function or var ... || (... = {})
    if exp_t.contains("(function")
        || act_t.contains("(function")
        || exp_t.contains("})(")
        || act_t.contains("})(")
        || exp_t.contains("|| (")
        || act_t.contains("|| (")
        || (exp_t.contains("|| {}") || act_t.contains("|| {}"))
    {
        return "NAMESPACE-IIFE".to_string();
    }

    // AMD-DEFINE: involves define(["require"
    if exp_t.contains("define([")
        || act_t.contains("define([")
        || exp_t.contains("define(\"")
        || act_t.contains("define(\"")
        || exp_t.contains("define([\"require\"")
        || act_t.contains("define([\"require\"")
    {
        return "AMD-DEFINE".to_string();
    }

    // UMD-FACTORY: involves factory pattern
    if exp_t.contains("factory(")
        || act_t.contains("factory(")
        || exp_t.contains("typeof exports")
        || act_t.contains("typeof exports")
        || exp_t.contains("typeof define")
        || act_t.contains("typeof define")
    {
        return "UMD-FACTORY".to_string();
    }

    // ENUM-IIFE: involves enum IIFE pattern (e.g., MyEnum[MyEnum["X"] = 0] = "X")
    if (exp_t.contains("[\"") && exp_t.contains("] = ") && exp_t.contains("\""))
        || (act_t.contains("[\"") && act_t.contains("] = ") && act_t.contains("\""))
    {
        // Check for typical enum pattern: Name[Name["member"] = N] = "member"
        if exp_t.contains("]=")
            || act_t.contains("]=")
            || exp_t.contains("] =")
            || act_t.contains("] =")
        {
            return "ENUM-IIFE".to_string();
        }
    }

    "OTHER".to_string()
}

// ---------------------------------------------------------------------------
// Diagnostic: find tests where the first CODE-DIFF mismatch is a quote style
// difference (single quotes vs double quotes).
// ---------------------------------------------------------------------------

/// Normalize all quote characters in a string to double quotes.
fn normalize_quotes(s: &str) -> String {
    s.replace('\'', "\"")
}

/// Classify what the quoted content represents.
fn classify_quoted_content(line: &str) -> &'static str {
    let trimmed = line.trim();

    // 'use strict' or "use strict"
    if trimmed.contains("use strict") {
        return "use-strict";
    }

    // Module specifiers: require('...') / require("...")
    if trimmed.contains("require(") {
        return "module-specifier (require)";
    }

    // Import/export from '...' / from "..."
    if trimmed.contains(" from ") && (trimmed.contains('\'') || trimmed.contains('"')) {
        return "module-specifier (import/export from)";
    }

    // Dynamic import: import('...') / import("...")
    if trimmed.contains("import(") {
        return "module-specifier (dynamic import)";
    }

    // AMD define specifiers: define(["..."] ...)
    if trimmed.contains("define(") {
        return "module-specifier (AMD define)";
    }

    // Property names in object literals or computed properties: { 'key': ... }
    // Heuristic: line has a colon after a quoted string
    if let Some(q_start) = trimmed.find(|c: char| c == '\'' || c == '"') {
        let rest = &trimmed[q_start + 1..];
        if let Some(q_end) = rest.find(|c: char| c == '\'' || c == '"') {
            let after_quote = &rest[q_end + 1..].trim_start();
            if after_quote.starts_with(':') || after_quote.starts_with(']') {
                return "property-name / computed-property";
            }
        }
    }

    // Generic string literal (assignment, argument, etc.)
    if trimmed.contains('\'') || trimmed.contains('"') {
        return "string-literal";
    }

    "unknown"
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_quote_diff() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    struct QuoteDiffInfo {
        test_name: String,
        line_number: usize,
        expected_line: String,
        actual_line: String,
        direction: String,
        content_kind: String,
    }

    let mut results: Vec<QuoteDiffInfo> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);

        // Skip passed tests, missing baselines, panics/crashes
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Walk through lines and find the first mismatch
        let min_len = exp_lines.len().min(act_lines.len());
        let mut first_mismatch: Option<(usize, &str, &str)> = None;

        for i in 0..min_len {
            if exp_lines[i] != act_lines[i] {
                first_mismatch = Some((i, exp_lines[i], act_lines[i]));
                break;
            }
        }

        // If all shared lines match but lengths differ, skip -- not a quote issue
        let Some((idx, exp_line, act_line)) = first_mismatch else {
            continue;
        };

        // Check if the lines differ only in quote style:
        // Normalize both single and double quotes to the same char and compare.
        let exp_normalized = normalize_quotes(exp_line);
        let act_normalized = normalize_quotes(act_line);
        if exp_normalized != act_normalized {
            continue; // differs in more than just quote style
        }

        // Determine direction
        let direction = {
            // Find the first position where the original lines differ
            let exp_chars: Vec<char> = exp_line.chars().collect();
            let act_chars: Vec<char> = act_line.chars().collect();
            let mut dir = "unknown".to_string();
            for j in 0..exp_chars.len().min(act_chars.len()) {
                if exp_chars[j] != act_chars[j] {
                    if exp_chars[j] == '\'' && act_chars[j] == '"' {
                        dir = "expected=' actual=\"".to_string();
                    } else if exp_chars[j] == '"' && act_chars[j] == '\'' {
                        dir = "expected=\" actual='".to_string();
                    } else {
                        dir = format!("expected={:?} actual={:?}", exp_chars[j], act_chars[j]);
                    }
                    break;
                }
            }
            dir
        };

        let content_kind = classify_quoted_content(exp_line).to_string();

        results.push(QuoteDiffInfo {
            test_name: name,
            line_number: idx + 1, // 1-based
            expected_line: safe_trunc(exp_line, 120).to_string(),
            actual_line: safe_trunc(act_line, 120).to_string(),
            direction,
            content_kind,
        });
    }

    // Sort by test name for stable output
    results.sort_by(|a, b| a.test_name.cmp(&b.test_name));

    println!("\n========================================================");
    println!("QUOTE-STYLE DIFF DIAGNOSTIC");
    println!("Tests where the first mismatch is purely a quote-style difference");
    println!("========================================================\n");
    println!("Total matching tests: {}\n", results.len());

    for (i, info) in results.iter().enumerate() {
        println!("--- [{}/{}] {} ---", i + 1, results.len(), info.test_name);
        println!("  Line number : {}", info.line_number);
        println!("  Direction   : {}", info.direction);
        println!("  Content kind: {}", info.content_kind);
        println!("  Expected    : {}", info.expected_line);
        println!("  Actual      : {}", info.actual_line);
        println!();
    }

    // Summary by direction
    let mut by_direction: HashMap<String, usize> = HashMap::new();
    for info in &results {
        *by_direction.entry(info.direction.clone()).or_insert(0) += 1;
    }
    println!("--- Summary by direction ---");
    let mut dir_vec: Vec<_> = by_direction.into_iter().collect();
    dir_vec.sort_by(|a, b| b.1.cmp(&a.1));
    for (dir, count) in &dir_vec {
        println!("  {}: {}", dir, count);
    }

    // Summary by content kind
    let mut by_kind: HashMap<String, usize> = HashMap::new();
    for info in &results {
        *by_kind.entry(info.content_kind.clone()).or_insert(0) += 1;
    }
    println!("\n--- Summary by content kind ---");
    let mut kind_vec: Vec<_> = by_kind.into_iter().collect();
    kind_vec.sort_by(|a, b| b.1.cmp(&a.1));
    for (kind, count) in &kind_vec {
        println!("  {}: {}", kind, count);
    }

    println!("\n========================================================");
    println!("END QUOTE-STYLE DIFF DIAGNOSTIC");
    println!("========================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_missing_semi() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    // Record: (test_name, line_number, expected_line, actual_line, statement_type)
    struct SemiMatch {
        test_name: String,
        line_number: usize,
        expected_line: String,
        actual_line: String,
        statement_type: String,
        match_kind: String,  // "plain-semi" or "brace-semi"
        source_hint: String, // clue about structured-emit vs source-text-copy
    }

    fn classify_statement(trimmed: &str) -> String {
        // What kind of statement ends here (looking at the expected line with ";")
        let body = trimmed.trim_end_matches(';').trim();

        if body.ends_with('}') {
            // Closing brace -- could be function, class, if/else, namespace IIFE, etc.
            if body.contains("function") || body.contains("=>") {
                "CLOSING-BRACE (function/arrow)".to_string()
            } else if body.contains("class ") {
                "CLOSING-BRACE (class)".to_string()
            } else if body.starts_with('}') && body.len() == 1 {
                "CLOSING-BRACE (standalone })".to_string()
            } else if body.contains(")(") || body.contains("})") {
                "CLOSING-BRACE (IIFE/call)".to_string()
            } else if body.contains("Object.defineProperty") || body.contains("Object.assign") {
                "CLOSING-BRACE (Object.* call)".to_string()
            } else if body.contains("{ }") || body.contains("{}") {
                "CLOSING-BRACE (empty block)".to_string()
            } else {
                format!(
                    "CLOSING-BRACE (other: ...{})",
                    &body[body.len().saturating_sub(30)..]
                )
            }
        } else if body.contains(" = ") || body.ends_with(" =") {
            if body.starts_with("var ") || body.starts_with("let ") || body.starts_with("const ") {
                "ASSIGNMENT (var/let/const decl)".to_string()
            } else if body.starts_with("exports.") || body.starts_with("module.exports") {
                "ASSIGNMENT (exports)".to_string()
            } else if body.contains("Object.defineProperty") {
                "ASSIGNMENT (Object.defineProperty)".to_string()
            } else if body.starts_with("this.") {
                "ASSIGNMENT (this.prop)".to_string()
            } else {
                "ASSIGNMENT (other)".to_string()
            }
        } else if body.starts_with("var ") || body.starts_with("let ") || body.starts_with("const ")
        {
            "VAR-DECL (no assignment visible)".to_string()
        } else if body.starts_with("return ") || body == "return" {
            "RETURN".to_string()
        } else if body.starts_with("throw ") {
            "THROW".to_string()
        } else if body.starts_with("break") || body.starts_with("continue") {
            "BREAK/CONTINUE".to_string()
        } else if body.starts_with("export ") {
            "EXPORT".to_string()
        } else if body.starts_with("import ") {
            "IMPORT".to_string()
        } else if body.contains("require(") {
            "REQUIRE-CALL".to_string()
        } else if body.starts_with("super(") || body.starts_with("super.") {
            "SUPER-CALL".to_string()
        } else if body.contains('(') && body.contains(')') {
            "FUNCTION-CALL / EXPRESSION".to_string()
        } else if body.starts_with('"') || body.starts_with('\'') {
            "STRING-LITERAL (e.g. \"use strict\")".to_string()
        } else {
            format!("EXPRESSION ({})", &body[..body.len().min(40)])
        }
    }

    fn guess_emit_path(exp_trimmed: &str, act_trimmed: &str, test_source: &str) -> String {
        // Heuristic: structured emit tends to produce code that differs in
        // formatting (spacing, parens, semicolons).  Source-text copy would
        // reproduce the original exactly.
        //
        // Clues that this is structured emit:
        //   - The line contains synthesized patterns like Object.defineProperty,
        //     __decorate, __metadata, __param, __esModule
        //   - The test has @module or @target directives (triggers downlevel)
        //   - The content looks like it was reconstructed (enum IIFE, namespace)
        //
        // Clues that this is source-text copy:
        //   - The actual line is identical to a line in the source .ts file
        //     (minus types), suggesting it was copied verbatim.

        let has_module_directive = test_source
            .lines()
            .any(|l| l.to_lowercase().contains("@module") || l.to_lowercase().contains("@target"));
        let is_cjs_boilerplate = exp_trimmed.contains("Object.defineProperty")
            || exp_trimmed.contains("__esModule")
            || exp_trimmed.contains("exports.")
            || exp_trimmed.contains("require(");
        let is_downlevel = exp_trimmed.contains("__decorate")
            || exp_trimmed.contains("__metadata")
            || exp_trimmed.contains("__param")
            || exp_trimmed.contains("__awaiter")
            || exp_trimmed.contains("__generator");
        let is_namespace_iife = exp_trimmed.contains("(function")
            || exp_trimmed.contains("})(")
            || exp_trimmed.contains("|| (");
        let is_enum_iife = exp_trimmed.contains("[\"") && exp_trimmed.contains("] = ");

        if is_downlevel {
            "STRUCTURED-EMIT (downlevel helper)".to_string()
        } else if is_cjs_boilerplate {
            "STRUCTURED-EMIT (CJS boilerplate)".to_string()
        } else if is_namespace_iife || is_enum_iife {
            "STRUCTURED-EMIT (namespace/enum IIFE)".to_string()
        } else if has_module_directive {
            "LIKELY-STRUCTURED (has @module/@target directive)".to_string()
        } else {
            // Check if the actual output line (without semicolon concern) appears
            // verbatim in the source -- if so, it's likely source-text copy.
            let act_no_semi = act_trimmed.trim_end_matches(';').trim();
            let found_in_source = test_source
                .lines()
                .any(|l| l.trim().starts_with(act_no_semi) || l.contains(act_no_semi));
            if found_in_source && act_no_semi.len() > 5 {
                "LIKELY-SOURCE-TEXT-COPY (line found in .ts source)".to_string()
            } else {
                "UNKNOWN (no strong signal)".to_string()
            }
        }
    }

    let mut matches: Vec<SemiMatch> = Vec::new();
    let mut total_failures = 0u32;

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        total_failures += 1;

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Find the FIRST mismatch line pair
        let min_len = std::cmp::min(exp_lines.len(), act_lines.len());
        for i in 0..min_len {
            if exp_lines[i] != act_lines[i] {
                let exp_trimmed = exp_lines[i].trim();
                let act_trimmed = act_lines[i].trim();

                // Check 1: exp_trimmed ends with ";" and (act_trimmed + ";") == exp_trimmed
                let plain_semi =
                    exp_trimmed.ends_with(';') && format!("{};", act_trimmed) == exp_trimmed;

                // Check 2: exp_trimmed.ends_with("};") && act_trimmed.ends_with("}")
                let brace_semi = exp_trimmed.ends_with("};")
                    && act_trimmed.ends_with('}')
                    && format!("{};", act_trimmed) == exp_trimmed;

                if plain_semi || brace_semi {
                    let test_source = std::fs::read_to_string(&path).unwrap_or_default();
                    let stmt_type = classify_statement(exp_trimmed);
                    let match_kind = if brace_semi {
                        "brace-semi".to_string()
                    } else {
                        "plain-semi".to_string()
                    };
                    let source_hint = guess_emit_path(exp_trimmed, act_trimmed, &test_source);
                    matches.push(SemiMatch {
                        test_name: name.clone(),
                        line_number: i + 1,
                        expected_line: exp_lines[i].to_string(),
                        actual_line: act_lines[i].to_string(),
                        statement_type: stmt_type,
                        match_kind,
                        source_hint,
                    });
                }
                break; // only inspect the first mismatch
            }
        }
    }

    // -----------------------------------------------------------------------
    // Output
    // -----------------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!("DIAG: MISSING SEMICOLON ANALYSIS");
    eprintln!("================================================================");
    eprintln!("Total non-panic/crash failures scanned: {}", total_failures);
    eprintln!(
        "Tests where first mismatch is missing ';': {}",
        matches.len()
    );
    eprintln!("================================================================\n");

    // Statement type breakdown
    let mut stmt_type_counts: HashMap<String, usize> = HashMap::new();
    let mut match_kind_counts: HashMap<String, usize> = HashMap::new();
    let mut source_hint_counts: HashMap<String, usize> = HashMap::new();

    for m in &matches {
        *stmt_type_counts
            .entry(m.statement_type.clone())
            .or_insert(0) += 1;
        *match_kind_counts.entry(m.match_kind.clone()).or_insert(0) += 1;
        *source_hint_counts.entry(m.source_hint.clone()).or_insert(0) += 1;
    }

    eprintln!("--- MATCH KIND BREAKDOWN ---");
    let mut mk_sorted: Vec<_> = match_kind_counts.iter().collect();
    mk_sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (kind, count) in &mk_sorted {
        eprintln!("  {:>4}  {}", count, kind);
    }

    eprintln!("\n--- STATEMENT TYPE BREAKDOWN ---");
    let mut st_sorted: Vec<_> = stmt_type_counts.iter().collect();
    st_sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (stype, count) in &st_sorted {
        eprintln!("  {:>4}  {}", count, stype);
    }

    eprintln!("\n--- EMIT PATH HINT BREAKDOWN ---");
    let mut sh_sorted: Vec<_> = source_hint_counts.iter().collect();
    sh_sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (hint, count) in &sh_sorted {
        eprintln!("  {:>4}  {}", count, hint);
    }

    // Detailed listing of all matching tests
    eprintln!("\n================================================================");
    eprintln!("ALL MATCHING TESTS (sorted by name)");
    eprintln!("================================================================\n");

    let mut sorted_matches: Vec<&SemiMatch> = matches.iter().collect();
    sorted_matches.sort_by(|a, b| a.test_name.cmp(&b.test_name));

    for (i, m) in sorted_matches.iter().enumerate() {
        eprintln!("[{:>3}] Test: {}", i + 1, m.test_name);
        eprintln!("      Line: {}", m.line_number);
        eprintln!(
            "      Kind: {} | StmtType: {}",
            m.match_kind, m.statement_type
        );
        eprintln!("      Emit hint: {}", m.source_hint);
        eprintln!("      EXP: {:?}", safe_trunc(&m.expected_line, 120));
        eprintln!("      ACT: {:?}", safe_trunc(&m.actual_line, 120));
        eprintln!();
    }

    eprintln!("================================================================");
    eprintln!("SUMMARY TABLE");
    eprintln!("================================================================");
    eprintln!("{:<55} {:>5}", "Statement Type", "Count");
    eprintln!("{:-<55} {:-^5}", "", "");
    for (stype, count) in &st_sorted {
        eprintln!("{:<55} {:>5}", stype, count);
    }
    eprintln!("{:-<55} {:-^5}", "", "");
    eprintln!("{:<55} {:>5}", "TOTAL", matches.len());
    eprintln!();

    eprintln!("{:<55} {:>5}", "Emit Path Hint", "Count");
    eprintln!("{:-<55} {:-^5}", "", "");
    for (hint, count) in &sh_sorted {
        eprintln!("{:<55} {:>5}", hint, count);
    }
    eprintln!("{:-<55} {:-^5}", "", "");
    eprintln!("{:<55} {:>5}", "TOTAL", matches.len());
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_semi_detail() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let mut entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.path());

    // Collect tests where the FIRST line-level mismatch has:
    //   - expected line ends with "};"
    //   - actual line ends with "}"
    //   - (actual_line.trim_end().to_string() + ";") == expected_line.trim_end()
    struct SemiMatch {
        name: String,
        test_path: std::path::PathBuf,
        expected: String,
        actual: String,
        first_mismatch_line: usize,
    }
    let mut matches: Vec<SemiMatch> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let min_len = std::cmp::min(exp_lines.len(), act_lines.len());

        for i in 0..min_len {
            if exp_lines[i] != act_lines[i] {
                let exp_t = exp_lines[i].trim_end();
                let act_t = act_lines[i].trim_end();
                if exp_t.ends_with("};")
                    && act_t.ends_with("}")
                    && (act_t.to_string() + ";") == exp_t
                {
                    matches.push(SemiMatch {
                        name: path.file_stem().unwrap().to_string_lossy().to_string(),
                        test_path: path.clone(),
                        expected: result.expected_output.clone(),
                        actual: result.actual_output.clone(),
                        first_mismatch_line: i,
                    });
                }
                break; // only check first mismatch
            }
        }
    }

    eprintln!("\n================================================================");
    eprintln!(
        "DIAG_SEMI_DETAIL: found {} tests where first mismatch is '}}' vs '}};'",
        matches.len()
    );
    eprintln!("================================================================\n");

    for (idx, m) in matches.iter().take(5).enumerate() {
        eprintln!("────────────────────────────────────────────────────────────────");
        eprintln!(
            "[{}/{}] Test: {}  (first mismatch at line {})",
            idx + 1,
            matches.len().min(5),
            m.name,
            m.first_mismatch_line + 1
        );
        eprintln!("────────────────────────────────────────────────────────────────");

        // 1) Show first 50 lines of expected output
        eprintln!("--- EXPECTED (first 50 lines) ---");
        for (i, line) in m.expected.lines().enumerate().take(50) {
            eprintln!("  E{:>4}: {}", i + 1, line);
        }
        let exp_total = m.expected.lines().count();
        if exp_total > 50 {
            eprintln!("  ... ({} more lines)", exp_total - 50);
        }

        // 2) Show first 50 lines of actual output
        eprintln!("--- ACTUAL (first 50 lines) ---");
        for (i, line) in m.actual.lines().enumerate().take(50) {
            eprintln!("  A{:>4}: {}", i + 1, line);
        }
        let act_total = m.actual.lines().count();
        if act_total > 50 {
            eprintln!("  ... ({} more lines)", act_total - 50);
        }

        // 3) Show first 10 lines of the .ts source file (test source options)
        eprintln!("--- TEST SOURCE OPTIONS (first 10 lines of .ts) ---");
        match std::fs::read_to_string(&m.test_path) {
            Ok(src) => {
                for (i, line) in src.lines().enumerate().take(10) {
                    eprintln!("  S{:>4}: {}", i + 1, line);
                }
                let src_total = src.lines().count();
                if src_total > 10 {
                    eprintln!("  ... ({} more lines)", src_total - 10);
                }
            }
            Err(e) => eprintln!("  <failed to read: {}>", e),
        }
        eprintln!();
    }

    if matches.is_empty() {
        // If no line-level matches, show broader diagnostic about semicolon issues
        eprintln!("\nNo line-level '}}' vs '}};' mismatches found at first-mismatch position.");
        eprintln!("Broadening search: looking for ANY line where exp ends with ';' and act is missing it...\n");

        let mut broader_matches: Vec<(String, std::path::PathBuf, usize, String, String)> =
            Vec::new();
        for entry in &entries {
            let path = entry.path();
            let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
            if result.passed || !result.baseline_exists {
                continue;
            }
            if result
                .diff
                .as_ref()
                .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
                .unwrap_or(false)
            {
                continue;
            }

            let exp_lines: Vec<&str> = result.expected_output.lines().collect();
            let act_lines: Vec<&str> = result.actual_output.lines().collect();
            let min_len = std::cmp::min(exp_lines.len(), act_lines.len());

            for i in 0..min_len {
                if exp_lines[i] != act_lines[i] {
                    let exp_t = exp_lines[i].trim_end();
                    let act_t = act_lines[i].trim_end();
                    // Check if the only difference is a trailing semicolon
                    if !exp_t.is_empty() && !act_t.is_empty() && (act_t.to_string() + ";") == exp_t
                    {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();
                        broader_matches.push((
                            name,
                            path.clone(),
                            i,
                            exp_t.to_string(),
                            act_t.to_string(),
                        ));
                        break;
                    }
                    break;
                }
            }
        }

        eprintln!(
            "Broader search found {} tests where first mismatch is missing trailing ';'",
            broader_matches.len()
        );
        for (idx, (name, test_path, line_idx, exp_line, act_line)) in
            broader_matches.iter().take(5).enumerate()
        {
            eprintln!("\n────────────────────────────────────────────────────────────────");
            eprintln!(
                "[{}/{}] Test: {}  (mismatch at line {})",
                idx + 1,
                broader_matches.len().min(5),
                name,
                line_idx + 1
            );
            eprintln!("  EXP: {:?}", &exp_line[..exp_line.len().min(200)]);
            eprintln!("  ACT: {:?}", &act_line[..act_line.len().min(200)]);

            // Full expected/actual
            let result = runner.run_case(test_path, tsc_rs_harness::Suite::Compiler);
            eprintln!("--- EXPECTED (first 50 lines) ---");
            for (i, line) in result.expected_output.lines().enumerate().take(50) {
                eprintln!("  E{:>4}: {}", i + 1, line);
            }
            let exp_total = result.expected_output.lines().count();
            if exp_total > 50 {
                eprintln!("  ... ({} more lines)", exp_total - 50);
            }

            eprintln!("--- ACTUAL (first 50 lines) ---");
            for (i, line) in result.actual_output.lines().enumerate().take(50) {
                eprintln!("  A{:>4}: {}", i + 1, line);
            }
            let act_total = result.actual_output.lines().count();
            if act_total > 50 {
                eprintln!("  ... ({} more lines)", act_total - 50);
            }

            eprintln!("--- TEST SOURCE OPTIONS (first 10 lines of .ts) ---");
            match std::fs::read_to_string(test_path) {
                Ok(src) => {
                    for (i, line) in src.lines().enumerate().take(10) {
                        eprintln!("  S{:>4}: {}", i + 1, line);
                    }
                    let src_total = src.lines().count();
                    if src_total > 10 {
                        eprintln!("  ... ({} more lines)", src_total - 10);
                    }
                }
                Err(e) => eprintln!("  <failed to read: {}>", e),
            }
        }
    }

    eprintln!("\n================================================================");
    eprintln!(
        "DIAG_SEMI_DETAIL complete. {} total matches shown.",
        matches.len().min(5)
    );
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_spurious_export2() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let mut entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.path());

    eprintln!("\n================================================================");
    eprintln!("DIAG_SPURIOUS_EXPORT2: Finding all tests whose FIRST mismatch");
    eprintln!("has 'export {{}}' in actual but not in expected");
    eprintln!("================================================================\n");

    let mut found_count = 0u32;

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let min_len = std::cmp::min(exp_lines.len(), act_lines.len());

        // Find the first mismatch line
        let mut mismatch_idx: Option<usize> = None;
        for i in 0..min_len {
            if exp_lines[i] != act_lines[i] {
                mismatch_idx = Some(i);
                break;
            }
        }
        // If all shared lines match but lengths differ, the mismatch is at min_len
        if mismatch_idx.is_none() && exp_lines.len() != act_lines.len() {
            mismatch_idx = Some(min_len);
        }

        let Some(mi) = mismatch_idx else {
            continue;
        };

        let act_at_mismatch = act_lines.get(mi).unwrap_or(&"<missing>");
        let exp_at_mismatch = exp_lines.get(mi).unwrap_or(&"<missing>");

        // Check: actual has "export {}" but expected does not
        if !act_at_mismatch.trim().starts_with("export {}") {
            continue;
        }
        if exp_at_mismatch.trim().starts_with("export {}") {
            continue;
        }

        found_count += 1;
        let test_name = path.file_stem().unwrap().to_string_lossy().to_string();

        eprintln!("================================================================");
        eprintln!("MATCH #{}: {}", found_count, test_name);
        eprintln!("================================================================");

        // 1. Test name
        eprintln!("\n  1) Test name: {}", test_name);

        // 2. Test options (first 10 lines of .ts source)
        eprintln!("\n  2) Test options (first 10 lines of .ts source):");
        match std::fs::read_to_string(&path) {
            Ok(source) => {
                for (li, line) in source.lines().enumerate().take(10) {
                    eprintln!("     {:>3}: {}", li + 1, line);
                }
            }
            Err(e) => eprintln!("     <failed to read: {}>", e),
        }

        // 3. Line number of mismatch (1-based)
        eprintln!("\n  3) Line number of mismatch: {}", mi + 1);

        // 4. Expected line at mismatch
        eprintln!("\n  4) Expected line at mismatch: {:?}", exp_at_mismatch);

        // 5. Full actual output (first 30 lines)
        eprintln!("\n  5) Full actual output (first 30 lines):");
        for (li, line) in act_lines.iter().enumerate().take(30) {
            let marker = if li == mi { ">>>" } else { "   " };
            eprintln!("     {} {:>3}: {}", marker, li + 1, line);
        }
        if act_lines.len() > 30 {
            eprintln!("     ... ({} more lines)", act_lines.len() - 30);
        }

        // 6. Full expected output (first 30 lines)
        eprintln!("\n  6) Full expected output (first 30 lines):");
        for (li, line) in exp_lines.iter().enumerate().take(30) {
            let marker = if li == mi { ">>>" } else { "   " };
            eprintln!("     {} {:>3}: {}", marker, li + 1, line);
        }
        if exp_lines.len() > 30 {
            eprintln!("     ... ({} more lines)", exp_lines.len() - 30);
        }

        eprintln!();
    }

    eprintln!("================================================================");
    eprintln!(
        "DIAG_SPURIOUS_EXPORT2: Found {} tests with SPURIOUS-EXPORT-EMPTY",
        found_count
    );
    eprintln!("  as first mismatch pattern.");
    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn diag_near_pass() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    /// Strip trailing blank lines from a slice of line strings.
    fn strip_trailing_blanks<'a>(lines: &'a [&'a str]) -> &'a [&'a str] {
        let mut end = lines.len();
        while end > 0 && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        &lines[..end]
    }

    /// Categorize the nature of a single-line difference.
    fn categorize_diff(expected: &str, actual: &str) -> &'static str {
        let et = expected.trim();
        let at = actual.trim();

        // Check if only quotes differ (single vs double)
        let e_single_to_double = et.replace('\'', "\"");
        let a_single_to_double = at.replace('\'', "\"");
        if e_single_to_double == a_single_to_double && et != at {
            return "QUOTE";
        }

        // Check if only semicolons differ
        let e_no_semi = et.replace(';', "");
        let a_no_semi = at.replace(';', "");
        if e_no_semi == a_no_semi && et != at {
            return "SEMICOLON";
        }

        // Check if only whitespace differs
        if et.split_whitespace().collect::<Vec<_>>() == at.split_whitespace().collect::<Vec<_>>()
            && et != at
        {
            return "WHITESPACE";
        }

        // Check for comment differences
        if (et.starts_with("//") || et.starts_with("/*") || et.starts_with("*"))
            && (at.starts_with("//")
                || at.starts_with("/*")
                || at.starts_with("*")
                || at.is_empty())
        {
            return "COMMENT";
        }
        if (at.starts_with("//") || at.starts_with("/*") || at.starts_with("*"))
            && (et.starts_with("//")
                || et.starts_with("/*")
                || et.starts_with("*")
                || et.is_empty())
        {
            return "COMMENT";
        }

        // Check for "use strict" differences
        if et.contains("use strict") || at.contains("use strict") {
            return "USE-STRICT";
        }

        // Check for exports-related differences
        if et.contains("exports.")
            || et.contains("module.exports")
            || at.contains("exports.")
            || at.contains("module.exports")
            || et.contains("Object.defineProperty")
            || at.contains("Object.defineProperty")
        {
            return "EXPORTS";
        }

        "OTHER"
    }

    // Collect per-test diff counts and details for 1-diff tests.
    // diff_count -> number of tests with that many diffs
    let mut histogram: HashMap<usize, usize> = HashMap::new();

    struct OneDiffInfo {
        test_name: String,
        line_number: usize,
        expected_line: String,
        actual_line: String,
        category: &'static str,
    }
    let mut one_diff_tests: Vec<OneDiffInfo> = Vec::new();
    let mut total_failing = 0usize;

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);

        // Skip passing, no-baseline, and panic/crash tests
        if result.passed {
            continue;
        }
        if !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        total_failing += 1;

        let exp_lines_raw: Vec<&str> = result.expected_output.lines().collect();
        let act_lines_raw: Vec<&str> = result.actual_output.lines().collect();
        let exp_lines = strip_trailing_blanks(&exp_lines_raw);
        let act_lines = strip_trailing_blanks(&act_lines_raw);

        // Count differing lines using line-by-line comparison over the
        // shared range. Lines beyond the shorter side count as diffs too,
        // but we exclude pure line-count differences from the "1-diff"
        // bucket (i.e. only consider tests where both sides have the same
        // number of non-blank-trailing lines AND exactly 1 line differs).
        let max_len = exp_lines.len().max(act_lines.len());

        let mut diff_count = 0usize;
        let mut first_diff_idx: Option<usize> = None;
        for i in 0..max_len {
            let e = if i < exp_lines.len() {
                exp_lines[i]
            } else {
                ""
            };
            let a = if i < act_lines.len() {
                act_lines[i]
            } else {
                ""
            };
            if e != a {
                diff_count += 1;
                if first_diff_idx.is_none() {
                    first_diff_idx = Some(i);
                }
            }
        }

        // Record in histogram (cap at a reasonable bucket)
        if diff_count <= 5 {
            *histogram.entry(diff_count).or_insert(0) += 1;
        } else {
            *histogram.entry(6).or_insert(0) += 1; // "6+" bucket
        }

        // Only collect details for tests with exactly 1 differing line
        // AND same trimmed line count (exclude line-count-only differences).
        if diff_count == 1 && exp_lines.len() == act_lines.len() {
            let idx = first_diff_idx.unwrap();
            let e_line = exp_lines[idx];
            let a_line = act_lines[idx];
            let cat = categorize_diff(e_line, a_line);
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            one_diff_tests.push(OneDiffInfo {
                test_name: name,
                line_number: idx + 1,
                expected_line: e_line.to_string(),
                actual_line: a_line.to_string(),
                category: cat,
            });
        }
    }

    // Sort 1-diff tests by category then name for nice output
    one_diff_tests.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then(a.test_name.cmp(&b.test_name))
    });

    // ---------------------------------------------------------------
    // Output
    // ---------------------------------------------------------------
    eprintln!("\n================================================================");
    eprintln!("DIAG_NEAR_PASS: Near-passing test analysis");
    eprintln!("================================================================");
    eprintln!(
        "Total failing tests (non-panic, with baseline): {}",
        total_failing
    );

    // Histogram: 1-5 diffs plus 6+
    eprintln!("\n--- Diff-count histogram ---");
    for n in 1..=5 {
        let count = histogram.get(&n).copied().unwrap_or(0);
        eprintln!("  {} diff(s): {} tests", n, count);
    }
    let six_plus = histogram.get(&6).copied().unwrap_or(0);
    eprintln!("  6+ diffs:  {} tests", six_plus);
    let zero_diffs = histogram.get(&0).copied().unwrap_or(0);
    if zero_diffs > 0 {
        eprintln!("  0 diffs (trailing whitespace only): {} tests", zero_diffs);
    }

    // Summary counts by category for 1-diff tests
    let mut cat_counts: HashMap<&str, usize> = HashMap::new();
    for info in &one_diff_tests {
        *cat_counts.entry(info.category).or_insert(0) += 1;
    }
    let mut cat_sorted: Vec<_> = cat_counts.iter().collect();
    cat_sorted.sort_by(|a, b| b.1.cmp(a.1));

    eprintln!("\n--- 1-diff tests: {} total ---", one_diff_tests.len());
    eprintln!("Category breakdown:");
    for (cat, count) in &cat_sorted {
        eprintln!("  {:<15} {}", cat, count);
    }

    // Print ALL 1-diff test details
    eprintln!("\n--- ALL 1-diff test details ---");
    for info in &one_diff_tests {
        eprintln!(
            "  [{}] {} (line {})",
            info.category, info.test_name, info.line_number
        );
        eprintln!("    expected: {}", safe_trunc(&info.expected_line, 120));
        eprintln!("    actual:   {}", safe_trunc(&info.actual_line, 120));
    }

    eprintln!("\n================================================================");
    eprintln!("DIAG_NEAR_PASS complete.");
    eprintln!("================================================================");
}

/// Focused diagnostic: show full side-by-side comparison for the 11 ODP/__esModule + use-strict
/// diff-by-1 tests. These tests fail by exactly 1 line involving CJS module preamble.
#[test]
#[ignore = "manual debug emit investigation"]
fn exports_11_full_comparison() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());

    // The 7 ODP-__esModule tests (EXTRA in actual)
    let odp_tests = vec![
        "APISample_WatchWithDefaults",
        "APISample_WatchWithOwnWatchHost",
        "commentOnImportStatement3",
        "esModuleInteropDefaultMemberMustBeSyntacticallyDefaultExport",
        "importNotElidedWhenNotFound",
        "moduleAugmentationInAmbientModule1",
        "reexportDefaultIsCallable",
    ];

    // The 4 use-strict tests (MISSING from actual)
    let use_strict_tests = vec![
        "binopAssignmentShouldHaveType",
        "localClassesInLoop",
        "localClassesInLoop_ES6",
        "nodeNextImportModeImplicitIndexResolution2",
    ];

    eprintln!("\n================================================================");
    eprintln!("THE 11 EXPORTS DIFF-BY-1 TESTS: FULL SIDE-BY-SIDE COMPARISON");
    eprintln!("================================================================\n");

    eprintln!("=== GROUP 1: ODP-__esModule (7 tests) ===");
    eprintln!(
        "Issue: Emitter places Object.defineProperty(exports, \"__esModule\", ...) too early."
    );
    eprintln!(
        "Real tsc places it AFTER prologue comments/directives and/or __importDefault helper."
    );
    eprintln!("Our emitter places it immediately after \"use strict\";.\n");

    for tname in &odp_tests {
        let path = root.join(format!("tests/cases/compiler/{}.ts", tname));
        if !path.exists() {
            eprintln!("SKIP: {} not found", tname);
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();

        eprintln!(
            "  ---- {} (exp: {} lines, act: {} lines) ----",
            tname,
            exp_lines.len(),
            act_lines.len()
        );

        // Find the //// [*.js] header to locate the output section
        let js_header_exp = exp_lines
            .iter()
            .position(|l| l.starts_with("//// [") && l.ends_with(".js]") && !l.ends_with("////"));
        let js_header_act = act_lines
            .iter()
            .position(|l| l.starts_with("//// [") && l.ends_with(".js]") && !l.ends_with("////"));

        if let (Some(je), Some(ja)) = (js_header_exp, js_header_act) {
            // Show 15 lines starting from the JS output section header
            let end_e = (je + 15).min(exp_lines.len());
            let end_a = (ja + 15).min(act_lines.len());

            eprintln!("  EXPECTED (baseline):");
            for i in je..end_e {
                eprintln!("    {:>3}: {}", i + 1, exp_lines[i]);
            }
            eprintln!("  ACTUAL (emitter):");
            for i in ja..end_a {
                let marker = if i < act_lines.len() {
                    let corresponding_exp = je + (i - ja);
                    if corresponding_exp < exp_lines.len()
                        && act_lines[i] != exp_lines[corresponding_exp]
                    {
                        ">>>"
                    } else {
                        "   "
                    }
                } else {
                    "   "
                };
                eprintln!("    {:>3}: {} {}", i + 1, marker, act_lines[i]);
            }
        }
        eprintln!();
    }

    eprintln!("\n=== GROUP 2: use-strict (4 tests) ===");
    eprintln!("Issue: Emitter skips the source-level \"use strict\" directive when it already");
    eprintln!("injected its own. Real tsc keeps BOTH -- the injected one AND the source one.");
    eprintln!("(Or for nodenext/.cjs, emitter doesn't emit \"use strict\" at all.)\n");

    for tname in &use_strict_tests {
        let path = root.join(format!("tests/cases/compiler/{}.ts", tname));
        if !path.exists() {
            eprintln!("SKIP: {} not found", tname);
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();

        eprintln!(
            "  ---- {} (exp: {} lines, act: {} lines) ----",
            tname,
            exp_lines.len(),
            act_lines.len()
        );

        // Find JS header
        let js_header_exp = exp_lines.iter().position(|l| {
            l.starts_with("//// [")
                && (l.ends_with(".js]") || l.ends_with(".cjs]") || l.ends_with(".mjs]"))
                && !l.ends_with("////")
        });
        let js_header_act = act_lines.iter().position(|l| {
            l.starts_with("//// [")
                && (l.ends_with(".js]") || l.ends_with(".cjs]") || l.ends_with(".mjs]"))
                && !l.ends_with("////")
        });

        if let (Some(je), Some(ja)) = (js_header_exp, js_header_act) {
            let end_e = (je + 12).min(exp_lines.len());
            let end_a = (ja + 12).min(act_lines.len());

            eprintln!("  EXPECTED (baseline):");
            for i in je..end_e {
                eprintln!("    {:>3}: {}", i + 1, exp_lines[i]);
            }
            eprintln!("  ACTUAL (emitter):");
            for i in ja..end_a {
                let marker = if i < act_lines.len() {
                    let corresponding_exp = je + (i - ja);
                    if corresponding_exp < exp_lines.len()
                        && act_lines[i] != exp_lines[corresponding_exp]
                    {
                        ">>>"
                    } else {
                        "   "
                    }
                } else {
                    "   "
                };
                eprintln!("    {:>3}: {} {}", i + 1, marker, act_lines[i]);
            }
        } else {
            // For nodeNextImportModeImplicitIndexResolution2, find the .cjs section
            // Show the last few sections
            let mut last_headers: Vec<usize> = Vec::new();
            for (i, l) in exp_lines.iter().enumerate() {
                if l.starts_with("//// [") && !l.ends_with("////") {
                    last_headers.push(i);
                }
            }
            if let Some(&last) = last_headers.last() {
                let end_e = exp_lines.len();
                eprintln!("  EXPECTED (last output section):");
                for i in last..end_e {
                    eprintln!("    {:>3}: {}", i + 1, exp_lines[i]);
                }
            }
            let mut last_headers_a: Vec<usize> = Vec::new();
            for (i, l) in act_lines.iter().enumerate() {
                if l.starts_with("//// [") && !l.ends_with("////") {
                    last_headers_a.push(i);
                }
            }
            if let Some(&last) = last_headers_a.last() {
                let end_a = act_lines.len();
                eprintln!("  ACTUAL (last output section):");
                for i in last..end_a {
                    eprintln!("    {:>3}: {}", i + 1, act_lines[i]);
                }
            }
        }

        // Also show the .ts source header for context
        let test_src = std::fs::read_to_string(&path).unwrap_or_default();
        eprintln!("  .ts source (first 8 lines):");
        for (i, l) in test_src.lines().take(8).enumerate() {
            eprintln!("    {:>3}: {}", i + 1, l);
        }
        eprintln!();
    }

    eprintln!("================================================================");
    eprintln!("ROOT CAUSE ANALYSIS");
    eprintln!("================================================================");
    eprintln!("GROUP 1 (ODP-__esModule, 7 tests):");
    eprintln!(
        "  The emitter places Object.defineProperty(exports, \"__esModule\", {{ value: true }})"
    );
    eprintln!("  immediately after \"use strict\"; in emit_source_file().");
    eprintln!("  Real TypeScript (tsc) emits it AFTER:");
    eprintln!("    - Prologue comments (copyright comments, reference directives)");
    eprintln!("    - Helper functions (__importDefault, __importStar)");
    eprintln!("  The fix would be to emit ODP after prologue directives/comments and");
    eprintln!("  after esModuleInterop helper functions, matching tsc ordering.");
    eprintln!();
    eprintln!("GROUP 2 (use-strict, 4 tests):");
    eprintln!("  Sub-issue A (3 tests: binopAssignmentShouldHaveType, localClassesInLoop,");
    eprintln!("  localClassesInLoop_ES6):");
    eprintln!("    The source code contains a literal \"use strict\" directive. The emitter");
    eprintln!("    injects its own \"use strict\"; (for alwaysStrict mode) and then SKIPS the");
    eprintln!("    source-level \"use strict\" to avoid duplication. But real tsc emits BOTH.");
    eprintln!("    Fix: do not skip source-level \"use strict\" when alwaysStrict is active.");
    eprintln!();
    eprintln!("  Sub-issue B (1 test: nodeNextImportModeImplicitIndexResolution2):");
    eprintln!("    For @module: nodenext, the .cjs output file should get a \"use strict\";");
    eprintln!("    preamble, but the emitter does not produce one. This may be because the");
    eprintln!("    emitter's is_commonjs() check does not account for .cjs files under");
    eprintln!("    nodenext module resolution.");
    eprintln!("    Fix: treat .cjs output files as CJS when module=nodenext.");
    eprintln!("================================================================");
}

/// Diagnostic test: find tests where expected and actual differ by exactly 1 line
/// and the difference involves an exports-related pattern (exports.X,
/// Object.defineProperty, export {}, module.exports, __esModule, require(), "use strict").
///
/// For each such test, prints:
///   - Test name
///   - Expected line (from baseline)
///   - Actual line (from emitter)
///   - Context around the difference
#[test]
#[ignore = "manual debug emit investigation"]
fn exports_diff_by_1_investigation() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    fn is_exports_related(line: &str) -> bool {
        let t = line.trim();
        t.contains("exports.")
            || t.contains("module.exports")
            || t.contains("Object.defineProperty")
            || t.contains("__esModule")
            || t.starts_with("export ")
            || t.starts_with("export{")
            || t.starts_with("export {};")
            || t.contains("require(")
            || t.contains("\"use strict\"")
            || t.contains("'use strict'")
            || t.contains("define([")
            || t.starts_with("define(")
    }

    struct ExportsDiffInfo {
        test_name: String,
        direction: String,    // "EXTRA-IN-ACTUAL" or "MISSING-FROM-ACTUAL"
        diff_line: String,    // the line that is extra or missing
        diff_line_num: usize, // 1-based line number
        exp_line_count: usize,
        act_line_count: usize,
        context_before: Vec<String>, // up to 3 lines of matching context before the diff
        context_after: Vec<String>,  // up to 3 lines of matching context after the diff
        subcategory: String,         // fine-grained classification
        test_source_header: String,  // first few lines of the .ts file (for @module etc.)
    }

    let mut results: Vec<ExportsDiffInfo> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        let len_diff = act_lines.len() as i64 - exp_lines.len() as i64;
        if len_diff.unsigned_abs() != 1 {
            continue;
        }

        // Find the differing line using a walk from the start
        let (direction, diff_line, diff_line_num, context_before, context_after) = if len_diff == 1
        {
            // actual has one extra line -- find it
            let mut ai = 0usize;
            let mut ei = 0usize;
            let mut found: Option<(String, usize)> = None;
            while ai < act_lines.len() && ei < exp_lines.len() {
                if act_lines[ai] == exp_lines[ei] {
                    ai += 1;
                    ei += 1;
                } else {
                    found = Some((act_lines[ai].to_string(), ai));
                    break;
                }
            }
            let (line, idx) = found.unwrap_or_else(|| {
                (
                    act_lines.last().unwrap_or(&"").to_string(),
                    act_lines.len() - 1,
                )
            });
            // Gather context: lines before the diff (from the matching portion)
            let ctx_before: Vec<String> = {
                let start = if idx >= 3 { idx - 3 } else { 0 };
                (start..idx)
                    .map(|i| format!("  {:>4}: {}", i + 1, act_lines.get(i).unwrap_or(&"")))
                    .collect()
            };
            // Context after: the next few lines in BOTH expected and actual for comparison
            let ctx_after: Vec<String> = {
                let mut lines = Vec::new();
                // After the extra line, actual[idx+1] should match expected[idx]
                for off in 0..3 {
                    let e_idx = idx + off; // expected position (no shift)
                    let a_idx = idx + 1 + off; // actual position (shifted by 1)
                    let e = exp_lines.get(e_idx).unwrap_or(&"<EOF>");
                    let a = act_lines.get(a_idx).unwrap_or(&"<EOF>");
                    let marker = if e == a { " " } else { "!" };
                    lines.push(format!("  {:>4}: {}{}", a_idx + 1, marker, a));
                }
                lines
            };
            (
                "EXTRA-IN-ACTUAL".to_string(),
                line,
                idx + 1,
                ctx_before,
                ctx_after,
            )
        } else {
            // expected has one extra line (actual is missing one)
            let mut ai = 0usize;
            let mut ei = 0usize;
            let mut found: Option<(String, usize)> = None;
            while ai < act_lines.len() && ei < exp_lines.len() {
                if act_lines[ai] == exp_lines[ei] {
                    ai += 1;
                    ei += 1;
                } else {
                    found = Some((exp_lines[ei].to_string(), ei));
                    break;
                }
            }
            let (line, idx) = found.unwrap_or_else(|| {
                (
                    exp_lines.last().unwrap_or(&"").to_string(),
                    exp_lines.len() - 1,
                )
            });
            let ctx_before: Vec<String> = {
                let start = if idx >= 3 { idx - 3 } else { 0 };
                (start..idx)
                    .map(|i| format!("  {:>4}: {}", i + 1, exp_lines.get(i).unwrap_or(&"")))
                    .collect()
            };
            let ctx_after: Vec<String> = {
                let mut lines = Vec::new();
                for off in 0..3 {
                    let e_idx = idx + 1 + off; // expected position (shifted by 1)
                    let a_idx = idx + off; // actual position (no shift)
                    let e = exp_lines.get(e_idx).unwrap_or(&"<EOF>");
                    let a = act_lines.get(a_idx).unwrap_or(&"<EOF>");
                    let marker = if e == a { " " } else { "!" };
                    lines.push(format!("  {:>4}: {}{}", e_idx + 1, marker, e));
                }
                lines
            };
            (
                "MISSING-FROM-ACTUAL".to_string(),
                line,
                idx + 1,
                ctx_before,
                ctx_after,
            )
        };

        // Filter: only keep exports-related differences
        if !is_exports_related(&diff_line) {
            continue;
        }

        // Fine-grained subcategory
        let t = diff_line.trim();
        let subcategory = if t.contains("Object.defineProperty") && t.contains("__esModule") {
            "ODP-__esModule".to_string()
        } else if t.contains("Object.defineProperty") {
            "ODP-other".to_string()
        } else if t.starts_with("exports.") && t.contains(" = ") {
            format!("exports-assignment ({})", &t[..t.len().min(50)])
        } else if t.starts_with("exports.default") || t.contains("exports.default") {
            "exports-default".to_string()
        } else if t.contains("exports.") {
            "exports-dot".to_string()
        } else if t.starts_with("module.exports") {
            "module-exports".to_string()
        } else if t.starts_with("export {};") || t.starts_with("export { }") {
            "export-empty-braces".to_string()
        } else if t.starts_with("export ") {
            format!("export-stmt ({})", &t[..t.len().min(40)])
        } else if t.contains("require(") {
            format!("require-call ({})", &t[..t.len().min(50)])
        } else if t.contains("\"use strict\"") || t.contains("'use strict'") {
            "use-strict".to_string()
        } else if t.contains("define([") || t.starts_with("define(") {
            "amd-define".to_string()
        } else {
            format!("other-exports ({})", &t[..t.len().min(50)])
        };

        // Read first 10 lines of .ts source for module option context
        let test_source = std::fs::read_to_string(&path).unwrap_or_default();
        let test_source_header: String = test_source
            .lines()
            .take(10)
            .collect::<Vec<&str>>()
            .join("\n");

        results.push(ExportsDiffInfo {
            test_name: name,
            direction,
            diff_line,
            diff_line_num,
            exp_line_count: exp_lines.len(),
            act_line_count: act_lines.len(),
            context_before,
            context_after,
            subcategory,
            test_source_header,
        });
    }

    // Sort by subcategory then test name for grouping
    results.sort_by(|a, b| {
        a.subcategory
            .cmp(&b.subcategory)
            .then(a.test_name.cmp(&b.test_name))
    });

    eprintln!("\n================================================================");
    eprintln!("EXPORTS DIFF-BY-1 INVESTIGATION");
    eprintln!("================================================================");
    eprintln!(
        "Total tests with exports-related 1-line diff: {}",
        results.len()
    );

    // Summary by subcategory
    let mut subcat_counts: HashMap<String, (usize, usize)> = HashMap::new(); // (extra, missing)
    for r in &results {
        let entry = subcat_counts.entry(r.subcategory.clone()).or_insert((0, 0));
        if r.direction == "EXTRA-IN-ACTUAL" {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    let mut sorted_subcats: Vec<_> = subcat_counts.iter().collect();
    sorted_subcats.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)));

    eprintln!("\n--- SUBCATEGORY SUMMARY ---");
    eprintln!(
        "{:<55} {:>6} {:>6} {:>6}",
        "Subcategory", "Extra", "Miss", "Total"
    );
    eprintln!("{:-<55} {:-^6} {:-^6} {:-^6}", "", "", "", "");
    for (sub, (extra, missing)) in &sorted_subcats {
        eprintln!(
            "{:<55} {:>6} {:>6} {:>6}",
            sub,
            extra,
            missing,
            extra + missing
        );
    }
    let total_extra: usize = results
        .iter()
        .filter(|r| r.direction == "EXTRA-IN-ACTUAL")
        .count();
    let total_missing: usize = results
        .iter()
        .filter(|r| r.direction == "MISSING-FROM-ACTUAL")
        .count();
    eprintln!("{:-<55} {:-^6} {:-^6} {:-^6}", "", "", "", "");
    eprintln!(
        "{:<55} {:>6} {:>6} {:>6}",
        "TOTAL",
        total_extra,
        total_missing,
        results.len()
    );

    eprintln!("\n================================================================");
    eprintln!("DETAILED PER-TEST BREAKDOWN");
    eprintln!("================================================================\n");

    let mut current_subcat = String::new();
    for (idx, r) in results.iter().enumerate() {
        if r.subcategory != current_subcat {
            current_subcat = r.subcategory.clone();
            eprintln!("--- [{}] ---\n", current_subcat);
        }

        eprintln!("  TEST #{}: {}", idx + 1, r.test_name);
        eprintln!("    Direction:   {}", r.direction);
        eprintln!("    Diff line:   L{}", r.diff_line_num);
        eprintln!(
            "    Expected lines: {}   Actual lines: {}",
            r.exp_line_count, r.act_line_count
        );
        eprintln!("    Subcategory: {}", r.subcategory);
        eprintln!();

        // Show test file header (module options etc.)
        eprintln!("    .ts file header:");
        for hl in r.test_source_header.lines() {
            eprintln!("      | {}", hl);
        }
        eprintln!();

        // Show the diff line itself
        if r.direction == "EXTRA-IN-ACTUAL" {
            eprintln!("    EXTRA LINE in actual (emitter produces this, baseline does not):");
        } else {
            eprintln!("    MISSING LINE from actual (baseline has this, emitter does not):");
        }
        eprintln!("      >>> {:?}", &r.diff_line[..r.diff_line.len().min(200)]);
        eprintln!();

        // Show context
        if !r.context_before.is_empty() {
            eprintln!("    Context BEFORE diff:");
            for c in &r.context_before {
                eprintln!("      {}", c);
            }
        }
        eprintln!("    ----- diff point (L{}) -----", r.diff_line_num);
        if !r.context_after.is_empty() {
            eprintln!("    Context AFTER diff:");
            for c in &r.context_after {
                eprintln!("      {}", c);
            }
        }
        eprintln!("\n    {}", "~".repeat(60));
        eprintln!();
    }

    // Final: list just the test names for easy reference
    eprintln!("================================================================");
    eprintln!("ALL {} EXPORTS DIFF-BY-1 TEST NAMES:", results.len());
    eprintln!("================================================================");
    for r in &results {
        eprintln!("  {} [{}] [{}]", r.test_name, r.direction, r.subcategory);
    }
    eprintln!("================================================================");
}

/// Diagnostic: Deep investigation of the 7 SEMICOLON near-pass tests.
/// Finds tests where expected and actual differ by exactly 1 line and
/// the difference is a missing/extra semicolon. For each, prints the
/// test name, differing lines, line numbers, surrounding context from
/// both the TypeScript source and the expected/actual JS output, and
/// identifies the AST pattern responsible.
#[test]
#[ignore = "manual debug emit investigation"]
fn semicolon_near_pass_investigation() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");
    let entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "ts").unwrap_or(false))
        .collect();

    fn strip_trailing_blanks<'a>(lines: &'a [&'a str]) -> &'a [&'a str] {
        let mut end = lines.len();
        while end > 0 && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        &lines[..end]
    }

    struct SemiInfo {
        test_name: String,
        line_number: usize,
        expected_line: String,
        actual_line: String,
        semi_direction: String, // "MISSING-IN-ACTUAL" or "EXTRA-IN-ACTUAL"
        context_before: Vec<String>,
        context_after: Vec<String>,
        ts_source_snippet: String,
        pattern_description: String,
    }
    let mut results: Vec<SemiInfo> = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);

        if result.passed || !result.baseline_exists {
            continue;
        }
        if result
            .diff
            .as_ref()
            .map(|d| d.starts_with("PANIC") || d.starts_with("CRASH"))
            .unwrap_or(false)
        {
            continue;
        }

        let exp_lines_raw: Vec<&str> = result.expected_output.lines().collect();
        let act_lines_raw: Vec<&str> = result.actual_output.lines().collect();
        let exp_lines = strip_trailing_blanks(&exp_lines_raw);
        let act_lines = strip_trailing_blanks(&act_lines_raw);

        // Only consider tests where both sides have the same line count
        // and exactly 1 line differs
        if exp_lines.len() != act_lines.len() {
            continue;
        }

        let max_len = exp_lines.len();
        let mut diff_count = 0usize;
        let mut diff_idx: Option<usize> = None;
        for i in 0..max_len {
            if exp_lines[i] != act_lines[i] {
                diff_count += 1;
                if diff_idx.is_none() {
                    diff_idx = Some(i);
                }
            }
        }
        if diff_count != 1 {
            continue;
        }
        let idx = diff_idx.unwrap();

        let et = exp_lines[idx].trim();
        let at = act_lines[idx].trim();

        // Check if only semicolons differ
        let e_no_semi = et.replace(';', "");
        let a_no_semi = at.replace(';', "");
        if e_no_semi != a_no_semi || et == at {
            continue;
        }

        // Determine direction
        let e_semi_count = et.matches(';').count();
        let a_semi_count = at.matches(';').count();
        let direction = if e_semi_count > a_semi_count {
            "MISSING-IN-ACTUAL"
        } else {
            "EXTRA-IN-ACTUAL"
        };

        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        // Gather context (3 lines before and after)
        let ctx_start = if idx >= 3 { idx - 3 } else { 0 };
        let ctx_end = (idx + 4).min(max_len);
        let context_before: Vec<String> = (ctx_start..idx)
            .map(|i| format!("  {}| {}", i + 1, exp_lines[i]))
            .collect();
        let context_after: Vec<String> = (idx + 1..ctx_end)
            .map(|i| format!("  {}| {}", i + 1, exp_lines[i]))
            .collect();

        // Read the TypeScript source to provide context
        let ts_source = std::fs::read_to_string(&path).unwrap_or_default();
        let ts_lines: Vec<&str> = ts_source.lines().collect();
        // Find the relevant portion - show first 20 lines of TS source
        let ts_snippet_end = ts_lines.len().min(25);
        let ts_snippet: String = ts_lines[..ts_snippet_end]
            .iter()
            .enumerate()
            .map(|(i, l)| format!("  {}| {}", i + 1, l))
            .collect::<Vec<_>>()
            .join("\n");

        // Identify the pattern
        let pattern = if et.trim() == "yield;" && at.trim() == "yield" {
            "BARE-YIELD: yield statement without argument needs semicolon (ASI normalization)"
        } else if et.ends_with("};") && at.ends_with("}") {
            "CLOSING-BRACE-SEMI: expression statement ending with } needs trailing semicolon"
        } else if et.ends_with(");") && at.ends_with(")") {
            "CALL-EXPR-SEMI: call expression statement needs trailing semicolon"
        } else if et.ends_with("; /*")
            || (et.contains(";") && et.contains("/*") && !at.contains(";"))
        {
            "SEMI-BEFORE-COMMENT: semicolon needed before inline block comment"
        } else if et.ends_with(';') && !at.ends_with(';') {
            "TRAILING-SEMI: statement missing trailing semicolon"
        } else {
            "OTHER-SEMI: semicolon difference pattern"
        };

        results.push(SemiInfo {
            test_name: name,
            line_number: idx + 1,
            expected_line: exp_lines[idx].to_string(),
            actual_line: act_lines[idx].to_string(),
            semi_direction: direction.to_string(),
            context_before,
            context_after,
            ts_source_snippet: ts_snippet,
            pattern_description: pattern.to_string(),
        });
    }

    // Also specifically check the known 7 tests to see why some might be missed
    let known_tests = [
        "argumentsAsPropertyName",
        "awaitExpressionInnerCommentEmit",
        "emitThisInSuperMethodCall",
        "generatorES6_1",
        "noIterationTypeErrorsInCFA",
        "objectLiteralWithGetAccessorInsideFunction",
        "webworkerIterable",
    ];
    eprintln!("\n--- Known SEMICOLON test debug ---");
    for entry in &entries {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if !known_tests.contains(&stem.as_str()) {
            continue;
        }
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);
        if result.passed {
            eprintln!("  {} -> PASSED (no longer failing!)", stem);
            continue;
        }
        if !result.baseline_exists {
            eprintln!("  {} -> NO BASELINE", stem);
            continue;
        }
        let exp_lines_raw: Vec<&str> = result.expected_output.lines().collect();
        let act_lines_raw: Vec<&str> = result.actual_output.lines().collect();
        let exp_lines = strip_trailing_blanks(&exp_lines_raw);
        let act_lines = strip_trailing_blanks(&act_lines_raw);
        let max_len = exp_lines.len().max(act_lines.len());
        let mut diff_count = 0usize;
        let mut first_diff_idx: Option<usize> = None;
        for i in 0..max_len {
            let e = if i < exp_lines.len() {
                exp_lines[i]
            } else {
                ""
            };
            let a = if i < act_lines.len() {
                act_lines[i]
            } else {
                ""
            };
            if e != a {
                diff_count += 1;
                if first_diff_idx.is_none() {
                    first_diff_idx = Some(i);
                }
            }
        }
        let found_in_results = results.iter().any(|r| r.test_name == stem);
        eprintln!(
            "  {} -> exp_lines={}, act_lines={}, diff_count={}, in_results={}",
            stem,
            exp_lines.len(),
            act_lines.len(),
            diff_count,
            found_in_results
        );
        if diff_count > 0 && diff_count <= 3 {
            for i in 0..max_len {
                let e = if i < exp_lines.len() {
                    exp_lines[i]
                } else {
                    ""
                };
                let a = if i < act_lines.len() {
                    act_lines[i]
                } else {
                    ""
                };
                if e != a {
                    eprintln!(
                        "    diff at line {}: exp=|{}| act=|{}|",
                        i + 1,
                        safe_trunc(e, 100),
                        safe_trunc(a, 100)
                    );
                }
            }
        }
    }

    results.sort_by(|a, b| {
        a.pattern_description
            .cmp(&b.pattern_description)
            .then(a.test_name.cmp(&b.test_name))
    });

    eprintln!("\n================================================================");
    eprintln!("SEMICOLON NEAR-PASS INVESTIGATION");
    eprintln!("================================================================");
    eprintln!(
        "Found {} tests with exactly 1 semicolon-only difference\n",
        results.len()
    );

    // Group by pattern
    let mut pattern_counts: HashMap<&str, usize> = HashMap::new();
    for r in &results {
        *pattern_counts.entry(&r.pattern_description).or_insert(0) += 1;
    }
    eprintln!("--- Pattern Summary ---");
    let mut pc_sorted: Vec<_> = pattern_counts.iter().collect();
    pc_sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (pat, count) in &pc_sorted {
        eprintln!("  {} ({}x)", pat, count);
    }

    eprintln!("\n--- Detailed Analysis ---");
    for (i, r) in results.iter().enumerate() {
        eprintln!("\n  [{}/{}] TEST: {}", i + 1, results.len(), r.test_name);
        eprintln!("  Pattern: {}", r.pattern_description);
        eprintln!("  Direction: {}", r.semi_direction);
        eprintln!("  Line: {}", r.line_number);
        eprintln!("  Expected: |{}|", r.expected_line);
        eprintln!("  Actual:   |{}|", r.actual_line);
        eprintln!();
        eprintln!("  Context (expected output):");
        for c in &r.context_before {
            eprintln!("    {}", c);
        }
        eprintln!("    >>> {}| {}", r.line_number, r.expected_line);
        for c in &r.context_after {
            eprintln!("    {}", c);
        }
        eprintln!();
        eprintln!("  TypeScript source:");
        eprintln!("{}", r.ts_source_snippet);

        // Root cause analysis
        eprintln!();
        eprintln!("  ROOT CAUSE: The statement containing this line goes through");
        eprintln!("  emit_source_line (source-text copy) because neither the outer");
        eprintln!("  compound statement (function/for/etc.) nor its children require");
        eprintln!("  transformation. The source-text copy path adds a trailing");
        eprintln!("  semicolon to the outer statement but does NOT normalize");
        eprintln!("  semicolons on inner statements. TypeScript's emitter always");
        eprintln!("  adds explicit semicolons for ASI (Automatic Semicolon Insertion).");
        eprintln!("  ----------------------------------------------------------------");
    }

    eprintln!("\n================================================================");
    eprintln!(
        "SUMMARY: All {} SEMICOLON near-pass tests share the same root cause:",
        results.len()
    );
    eprintln!("  Compound statements (FnDecl, ForOf, ForIn, For, etc.) that don't");
    eprintln!("  need transformation go through emit_source_line which copies the");
    eprintln!("  entire statement text verbatim. Inner expression/var statements");
    eprintln!("  that lack explicit semicolons in the source (relying on ASI) are");
    eprintln!("  NOT normalized. The has_inner_missing_semicolons check on line 2041");
    eprintln!("  only applies to Expr/Var/Return/Throw statements, NOT to compound");
    eprintln!("  statements like FnDecl/ForOf that contain inner statements.");
    eprintln!("================================================================");
}

/// Direct harness test for the 4 remaining SEMICOLON near-pass cases.
/// Runs each through the actual harness pipeline and compares.
#[test]
#[ignore = "manual debug emit investigation"]
fn semicolon_harness_check() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());

    let test_names = [
        "generatorES6_1",
        "awaitExpressionInnerCommentEmit",
        "emitThisInSuperMethodCall",
        "objectLiteralWithGetAccessorInsideFunction",
    ];

    eprintln!("\n================================================================");
    eprintln!("HARNESS PIPELINE OUTPUT FOR SEMICOLON TESTS");
    eprintln!("================================================================\n");

    for name in &test_names {
        let path = root.join(format!("tests/cases/compiler/{}.ts", name));
        let result = runner.run_case(&path, tsc_rs_harness::Suite::Compiler);

        eprintln!("--- {} [passed={}] ---", name, result.passed);

        if !result.passed {
            // Find the differing line
            let exp_lines: Vec<&str> = result.expected_output.lines().collect();
            let act_lines: Vec<&str> = result.actual_output.lines().collect();
            let max_len = exp_lines.len().max(act_lines.len());
            for i in 0..max_len {
                let e = if i < exp_lines.len() {
                    exp_lines[i]
                } else {
                    "<missing>"
                };
                let a = if i < act_lines.len() {
                    act_lines[i]
                } else {
                    "<missing>"
                };
                if e != a {
                    eprintln!("  DIFF at line {} (of {}):", i + 1, max_len);
                    eprintln!("    exp: |{}|", e);
                    eprintln!("    act: |{}|", a);
                    // Show hex of last few chars for debugging
                    let e_bytes: Vec<u8> = e.bytes().rev().take(10).collect();
                    let a_bytes: Vec<u8> = a.bytes().rev().take(10).collect();
                    eprintln!("    exp trail bytes: {:?}", e_bytes);
                    eprintln!("    act trail bytes: {:?}", a_bytes);
                }
            }
        }
        eprintln!();
    }
    eprintln!("================================================================");
}

/// Direct emitter test for the 4 remaining SEMICOLON near-pass cases.
/// Parses and emits each test case using the same options as the harness,
/// then shows the exact output for semicolon analysis.
#[test]
#[ignore = "manual debug emit investigation"]
fn semicolon_direct_emit_check() {
    use tsc_rs_ast::*;

    let test_cases: Vec<(&str, &str, CompilerOptions, &str)> = vec![
        (
            "generatorES6_1",
            "function* foo() {\n    yield\n}",
            CompilerOptions {
                target: ScriptTarget::parse("es6"),
                strict: Some(false),
                ..Default::default()
            },
            "    yield;",  // expected line
        ),
        (
            "awaitExpressionInnerCommentEmit",
            "async function foo() {\n    /*comment1*/ await 1;\n    await /*comment2*/ 2;\n    await 3 /*comment3*/\n}",
            CompilerOptions {
                target: ScriptTarget::parse("esnext"),
                ..Default::default()
            },
            "    await 3; /*comment3*/",
        ),
        (
            "emitThisInSuperMethodCall",
            "class User {\n    sayHello() {\n    }\n}\n\nclass RegisteredUser extends User {\n    f() {\n        () => {\n            function inner() {\n                super.sayHello();\n            }\n        };\n    }\n    g() {\n        function inner() {\n            () => {\n                super.sayHello();\n            }\n        }\n    }\n    h() {\n        function inner() {\n            super.sayHello();\n        }\n    }\n}\n",
            CompilerOptions {
                target: ScriptTarget::parse("es2015"),
                ..Default::default()
            },
            "            };",
        ),
        (
            "objectLiteralWithGetAccessorInsideFunction",
            "function bar() {\n    var x = {\n        get _extraOccluded() {\n            var occluded = 0;\n            return occluded;\n        },\n    }\n}",
            CompilerOptions {
                target: ScriptTarget::parse("es2015"),
                ..Default::default()
            },
            "    };",
        ),
    ];

    eprintln!("\n================================================================");
    eprintln!("DIRECT EMITTER OUTPUT FOR SEMICOLON TESTS");
    eprintln!("================================================================\n");

    for (name, source, opts, expected_line) in &test_cases {
        let file = tsc_rs_parser::parse(&format!("{}.ts", name), source);
        let result = tsc_rs_emitter::emit(&file, opts);
        let js = &result.javascript;

        let contains_expected = js.lines().any(|l| l == *expected_line);
        let status = if contains_expected { "OK" } else { "MISSING" };

        eprintln!("--- {} [{}] ---", name, status);
        eprintln!("Source:\n{}", source);
        eprintln!("Emitted JS:");
        for (i, line) in js.lines().enumerate() {
            let marker = if line == *expected_line {
                " <-- EXPECTED"
            } else if line.trim() == expected_line.trim().replace(";", "").as_str()
                || (expected_line.trim().ends_with(";")
                    && line.trim() == expected_line.trim().trim_end_matches(";"))
            {
                " <-- MISSING SEMICOLON"
            } else {
                ""
            };
            eprintln!("  {:>3}| {}{}", i + 1, line, marker);
        }
        eprintln!("Expected line: |{}|", expected_line);
        eprintln!();
    }

    eprintln!("================================================================");
}

#[test]
#[ignore = "manual debug emit investigation"]
fn show_top_failures() {
    let root = workspace_root();
    let runner = tsc_rs_harness::BaselineRunner::new(root.clone());
    let cases_dir = root.join("tests/cases/compiler");

    // Pick a representative set of failing tests across categories
    let test_names = vec![
        // Whitespace/indentation
        "selfInLambdas",
        "thisBinding2",
        "multiLinePropertyAccessAndArrowFunctionIndent1",
        // Comment-related
        "commentOnClassAccessor1",
        "commentBeforeStaticMethod1",
        "noImplicitAnyParametersInModule",
        // Namespace/enum
        "interfaceAssignmentCompat",
        "mergedEnumDeclarationCodeGen",
        // Module/export
        "modulePreserve1",
        "defaultIsNotVisibleInLocalScope",
        // Import elision
        "importElisionEnum",
        "es6ImportNamedImportInExportAssignment",
        // Object.defineProperty
        "moduleAugmentationInAmbientModule1",
        // Use strict
        "localClassesInLoop",
        // Zero-diff code-diff
        "forInModule",
        "capturedLetConstInLoop10",
        "constDeclarations-access3",
    ];

    for name in &test_names {
        let test_path = cases_dir.join(format!("{name}.ts"));
        if !test_path.exists() {
            continue;
        }
        let result = runner.run_case(&test_path, tsc_rs_harness::Suite::Compiler);
        if result.passed {
            eprintln!("\n=== {} === PASS", name);
            continue;
        }

        eprintln!("\n============================================================");
        eprintln!("=== FAIL: {} ===", name);

        let exp_lines: Vec<&str> = result.expected_output.lines().collect();
        let act_lines: Vec<&str> = result.actual_output.lines().collect();

        eprintln!(
            "Expected lines: {}, Actual lines: {}",
            exp_lines.len(),
            act_lines.len()
        );

        // Show first 10 mismatched lines
        let max = exp_lines.len().max(act_lines.len()).min(100);
        let mut shown = 0;
        for i in 0..max {
            let e = exp_lines.get(i).copied().unwrap_or("<MISSING>");
            let a = act_lines.get(i).copied().unwrap_or("<MISSING>");
            if e != a {
                if shown < 10 {
                    eprintln!("  L{}: EXP |{}|", i + 1, e);
                    eprintln!("  L{}: ACT |{}|", i + 1, a);
                    eprintln!();
                }
                shown += 1;
            }
        }
        if shown > 10 {
            eprintln!("  ... {} more mismatches", shown - 10);
        }
    }
}
