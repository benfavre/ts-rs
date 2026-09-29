//! `typecheck-report` — measures the type checker's diagnostic accuracy
//! against real `tsc` baselines.
//!
//! The emitter has `baseline-report` (compares emitted `.js`) and the LSP has
//! `lsp-report`. Neither measures whether the *type checker* reports the same
//! errors `tsc` does. This bin closes that gap.
//!
//! It reuses the existing `BaselineKind::Errors` machinery: for every test in
//! a suite that has a stored `tsc` `.errors.txt` baseline, the harness already
//! runs our checker and resolves the gold baseline (variant-aware). We diff the
//! two diagnostic lists at the `(file, line, code)` level — column is excluded
//! because our column reporting legitimately diverges from `tsc`, and messages
//! are excluded because wording differs while the *code* is what matters.
//!
//! Metrics:
//!   - recall    = matched_expected / total_expected   (how many tsc errors we find)
//!   - precision = matched_expected / total_actual      (how many of ours are real)
//!
//! Recall is the number to drive up: a checker that silently misses errors
//! (the dominant failure mode today) shows up as low recall. Precision near
//! 1.0 means we don't cry wolf — safe to surface as advisory signal.
//!
//! Usage:
//!   cargo run -p tsc_rs_harness --bin typecheck-report -- --suite compiler
//!   cargo run -p tsc_rs_harness --bin typecheck-report -- --suite compiler --limit 500
//!   cargo run -p tsc_rs_harness --bin typecheck-report -- --code 2322 --show-cases 10
//!   cargo run -p tsc_rs_harness --bin typecheck-report -- --json
//!   cargo run -p tsc_rs_harness --bin typecheck-report -- --save-misses /tmp/misses.txt

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::json;
use tsc_rs_harness::{BaselineKind, BaselineResult, BaselineRunner, Suite};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Option/config-deprecation codes — not type-checking. Excluded from the
/// accuracy metric by default so the recall number reflects real semantic
/// checking, not whether we mirror tsc's deprecation warnings.
const CONFIG_CODES: &[u32] = &[5101, 5102, 5107, 5108];

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p tsc_rs_harness --bin typecheck-report -- [options]\n\
         \n\
         Measures type-checker diagnostic recall/precision vs real tsc .errors.txt baselines.\n\
         \n\
         Options:\n\
           --suite <compiler|conformance>   suite to measure (default: compiler)\n\
           --root <workspace-root>          (default: auto-detected)\n\
           --limit <n>                      run only first N discovered cases\n\
           --top <n>                        show top N codes by expected count (default: 25)\n\
           --code <NNNN>                    drill into one TS code (lists missed/spurious cases)\n\
           --show-cases <n>                 sample cases to print when --code is set (default: 10)\n\
           --include-config-codes           include 51xx option-deprecation codes in the metric\n\
           --json                           print structured JSON\n\
           --save-misses <path>             write all false-negative (file,line,code,case) rows\n\
           -h, --help"
    );
}

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to find workspace root from crate path")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// Diagnostic parsing
// ---------------------------------------------------------------------------

/// One parsed diagnostic, keyed on what we can match reliably across both
/// checkers: file basename (lowercased), 1-based line, and TS code. Column is
/// intentionally dropped.
type DiagKey = (String, u32, u32);

fn basename_lower(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}

/// Parse either a `(line,col):` or pretty `:line:col -` location out of the
/// prefix before the `error TS...` marker. Returns `("", 0)` for global
/// (file-less) diagnostics.
fn parse_file_loc(prefix: &str) -> (String, u32) {
    let p = prefix.trim_end().trim_end_matches(':').trim_end();
    if let Some(open) = p.rfind('(') {
        if let Some(close) = p[open..].find(')').map(|i| i + open) {
            let inside = &p[open + 1..close];
            if let Some(line) = inside
                .split(',')
                .next()
                .and_then(|s| s.trim().parse::<u32>().ok())
            {
                return (basename_lower(p[..open].trim()), line);
            }
        }
    }

    // Pretty diagnostics use `path/to/file.ts:line:column - error TS...`.
    // Split from the right so Windows drive letters remain part of the path.
    let pretty = p.trim_end_matches('-').trim_end();
    let mut components = pretty.rsplitn(3, ':');
    if components
        .next()
        .and_then(|column| column.parse::<u32>().ok())
        .is_some()
    {
        if let (Some(line), Some(file)) = (components.next(), components.next()) {
            if let Ok(line) = line.parse::<u32>() {
                return (basename_lower(file), line);
            }
        }
    }

    (String::new(), 0)
}

/// Parse a single header-section line into a diagnostic key, if it is one.
fn parse_header_line(line: &str) -> Option<DiagKey> {
    let mut markers: Vec<_> = ["error TS", "warning TS", "message TS"]
        .iter()
        .flat_map(|marker| {
            line.match_indices(marker)
                .map(|(position, _)| (position, marker.len()))
        })
        .collect();
    markers.sort_unstable_by_key(|(position, _)| *position);

    for (position, marker_len) in markers {
        let after = &line[position + marker_len..];
        let code_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(code) = code_str.parse::<u32>() else {
            continue;
        };
        let prefix = &line[..position];
        let (file, line_number) = parse_file_loc(prefix);
        // File-less diagnostics begin directly with the category. A marker
        // embedded in source text, a path, or another diagnostic's message is
        // not itself a header.
        if prefix.trim().is_empty() || !file.is_empty() {
            return Some((file, line_number, code));
        }
    }
    None
}

/// Remove ANSI CSI sequences used by `@pretty: true` error baselines before
/// looking for diagnostic header markers.
fn strip_ansi_csi(text: &str) -> Cow<'_, str> {
    if !text.as_bytes().contains(&0x1b) {
        return Cow::Borrowed(text);
    }

    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut copied_through = 0;
    let mut cursor = 0;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != 0x1b || bytes[cursor + 1] != b'[' {
            cursor += 1;
            continue;
        }

        output.push_str(&text[copied_through..cursor]);
        cursor += 2;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            cursor += 1;
            if (0x40..=0x7e).contains(&byte) {
                break;
            }
        }
        copied_through = cursor;
    }
    output.push_str(&text[copied_through..]);
    Cow::Owned(output)
}

/// Parse an `.errors.txt`-format string into the flat diagnostic list. Only the
/// header section (before the first `====` file banner) is read, and `!!!`
/// annotation lines are skipped, so each diagnostic is counted exactly once and
/// source lines that happen to contain "error TS..." can't create phantoms.
fn parse_diags(text: &str) -> Vec<DiagKey> {
    let mut out = Vec::new();
    let text = strip_ansi_csi(text);
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("====") {
            break;
        }
        if t.starts_with("!!!") {
            continue;
        }
        if let Some(d) = parse_header_line(line) {
            out.push(d);
        }
    }
    out
}

fn to_multiset(diags: &[DiagKey], include_config: bool) -> HashMap<DiagKey, u32> {
    let mut m: HashMap<DiagKey, u32> = HashMap::new();
    for d in diags {
        if !include_config && CONFIG_CODES.contains(&d.2) {
            continue;
        }
        *m.entry(d.clone()).or_insert(0) += 1;
    }
    m
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct CodeStat {
    expected: u32,
    matched: u32,
    spurious: u32, // false positives we emit under this code
}

#[derive(Default)]
struct Totals {
    expected: u64,
    matched: u64, // true positives
    actual: u64,
    // Location-level (file,line) ignoring code: did we flag the right *spot*,
    // even if we used a less-specific code (e.g. TS2304 where tsc says TS2662)?
    // The gap between location-recall and code-recall is "found it, wrong code".
    loc_expected: u64,
    loc_matched: u64,
    loc_actual: u64,
    per_code: HashMap<u32, CodeStat>,
    cases_with_expected: usize,
    cases_perfect: usize, // every expected diag found, no spurious
}

struct WrongCodeRow {
    case: String,
    file: String,
    line: u32,
    expected_code: u32,
    our_code: u32,
}

struct MissRow {
    case: String,
    file: String,
    line: u32,
    code: u32,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut suite = Suite::Compiler;
    let mut root: Option<PathBuf> = None;
    let mut limit: Option<usize> = None;
    let mut top = 25usize;
    let mut focus_code: Option<u32> = None;
    let mut show_cases = 10usize;
    let mut include_config = false;
    let mut as_json = false;
    let mut save_misses: Option<PathBuf> = None;
    let mut save_wrongcode: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "--suite" => {
                i += 1;
                suite = match args.get(i).map(|s| s.as_str()) {
                    Some("compiler") => Suite::Compiler,
                    Some("conformance") => Suite::Conformance,
                    Some(other) => {
                        eprintln!("typecheck-report: unsupported --suite '{other}' (use compiler|conformance)");
                        std::process::exit(2);
                    }
                    None => {
                        eprintln!("typecheck-report: --suite needs a value");
                        std::process::exit(2);
                    }
                };
            }
            "--root" => {
                i += 1;
                root = args.get(i).map(PathBuf::from);
            }
            "--limit" => {
                i += 1;
                limit = args.get(i).and_then(|s| s.parse().ok());
            }
            "--top" => {
                i += 1;
                top = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(top);
            }
            "--code" => {
                i += 1;
                focus_code = args
                    .get(i)
                    .and_then(|s| s.trim_start_matches("TS").parse().ok());
            }
            "--show-cases" => {
                i += 1;
                show_cases = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(show_cases);
            }
            "--include-config-codes" => include_config = true,
            "--json" => as_json = true,
            "--save-wrongcode" => {
                i += 1;
                save_wrongcode = args.get(i).map(PathBuf::from);
            }
            "--save-misses" => {
                i += 1;
                save_misses = args.get(i).map(PathBuf::from);
            }
            other => {
                eprintln!("typecheck-report: unknown option '{other}'");
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let root = root.unwrap_or_else(workspace_root);
    let runner = BaselineRunner::new(root);

    let mut totals = Totals::default();
    let mut all_misses: Vec<MissRow> = Vec::new();
    let mut wrongcode_rows: Vec<WrongCodeRow> = Vec::new();
    let mut focus_missed: Vec<MissRow> = Vec::new();
    let mut focus_spurious: Vec<MissRow> = Vec::new();

    let total_cases =
        match runner.visit_expanded_suite(suite, BaselineKind::Errors, limit, |expanded| {
            accumulate(
                &expanded.result,
                include_config,
                focus_code,
                &mut totals,
                &mut all_misses,
                &mut focus_missed,
                &mut focus_spurious,
                &mut wrongcode_rows,
            );
        }) {
            Ok(total) => total,
            Err(e) => {
                eprintln!("typecheck-report: suite run failed: {e:?}");
                std::process::exit(1);
            }
        };
    all_misses.sort_by(|left, right| {
        (&left.case, &left.file, left.line, left.code).cmp(&(
            &right.case,
            &right.file,
            right.line,
            right.code,
        ))
    });
    wrongcode_rows.sort_by(|left, right| {
        (
            &left.case,
            &left.file,
            left.line,
            left.expected_code,
            left.our_code,
        )
            .cmp(&(
                &right.case,
                &right.file,
                right.line,
                right.expected_code,
                right.our_code,
            ))
    });
    focus_missed.sort_by(|left, right| {
        (&left.case, &left.file, left.line, left.code).cmp(&(
            &right.case,
            &right.file,
            right.line,
            right.code,
        ))
    });
    focus_spurious.sort_by(|left, right| {
        (&left.case, &left.file, left.line, left.code).cmp(&(
            &right.case,
            &right.file,
            right.line,
            right.code,
        ))
    });

    let recall = ratio(totals.matched, totals.expected);
    let precision = ratio(totals.matched, totals.actual);
    let false_neg = totals.expected.saturating_sub(totals.matched);
    let false_pos = totals.actual.saturating_sub(totals.matched);

    if let Some(path) = &save_wrongcode {
        let mut out = String::new();
        for w in &wrongcode_rows {
            out.push_str(&format!(
                "{}\t{}\t{}\texpected TS{}\tours TS{}\n",
                w.case, w.file, w.line, w.expected_code, w.our_code
            ));
        }
        if let Err(e) = std::fs::write(path, out) {
            eprintln!("typecheck-report: failed to write --save-wrongcode: {e}");
        } else {
            println!(
                "wrote {} wrong-code rows to {}",
                wrongcode_rows.len(),
                path.display()
            );
        }
    }
    if let Some(path) = &save_misses {
        let mut buf = String::from("# case\tfile\tline\tcode\n");
        for m in &all_misses {
            buf.push_str(&format!(
                "{}\t{}\t{}\tTS{}\n",
                m.case, m.file, m.line, m.code
            ));
        }
        if let Err(e) = std::fs::write(path, buf) {
            eprintln!("typecheck-report: failed to write --save-misses: {e}");
        } else {
            eprintln!(
                "Wrote {} false-negative rows to {}",
                all_misses.len(),
                path.display()
            );
        }
    }

    if as_json {
        let mut codes: Vec<_> = totals.per_code.iter().collect();
        codes.sort_by(|a, b| b.1.expected.cmp(&a.1.expected).then_with(|| a.0.cmp(b.0)));
        let per_code: Vec<_> = codes
            .iter()
            .map(|(code, st)| {
                json!({
                    "code": format!("TS{code}"),
                    "expected": st.expected,
                    "matched": st.matched,
                    "recall": ratio(st.matched as u64, st.expected as u64),
                    "spurious": st.spurious,
                })
            })
            .collect();
        let out = json!({
            "suite": suite.as_str(),
            "cases_total": total_cases,
            "cases_with_expected": totals.cases_with_expected,
            "cases_perfect": totals.cases_perfect,
            "expected_diagnostics": totals.expected,
            "matched": totals.matched,
            "false_negatives": false_neg,
            "false_positives": false_pos,
            "recall": recall,
            "precision": precision,
            "loc_recall": ratio(totals.loc_matched, totals.loc_expected),
            "loc_precision": ratio(totals.loc_matched, totals.loc_actual),
            "per_code": per_code,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return;
    }

    println!(
        "\n========== Type-Check Accuracy ({}) ==========",
        suite.as_str()
    );
    println!("Cases discovered:            {total_cases}");
    println!(
        "Cases with tsc errors:       {}",
        totals.cases_with_expected
    );
    println!(
        "Cases fully reproduced:      {} ({:.1}%)",
        totals.cases_perfect,
        100.0
            * ratio(
                totals.cases_perfect as u64,
                totals.cases_with_expected as u64
            )
    );
    println!(
        "Config-deprecation codes:    {}",
        if include_config {
            "included"
        } else {
            "excluded (51xx)"
        }
    );
    println!();
    println!("Expected diagnostics (tsc):  {}", totals.expected);
    println!("Matched (true positives):    {}", totals.matched);
    println!("Missed (false negatives):    {}", false_neg);
    println!("Spurious (false positives):  {}", false_pos);
    println!();
    println!(
        "RECALL    = {:.1}%   (of tsc's errors, how many we find — exact code)",
        100.0 * recall
    );
    println!(
        "PRECISION = {:.1}%   (of our errors, how many are real — exact code)",
        100.0 * precision
    );
    let loc_recall = ratio(totals.loc_matched, totals.loc_expected);
    let loc_precision = ratio(totals.loc_matched, totals.loc_actual);
    println!();
    println!(
        "LOC-RECALL    = {:.1}%   (right line flagged, any code)",
        100.0 * loc_recall
    );
    println!(
        "LOC-PRECISION = {:.1}%   (our flagged lines that tsc also flags)",
        100.0 * loc_precision
    );
    println!(
        "  -> {:.1}% of our \"misses\" are wrong-code-right-line; {:.1}% of our \"spurious\" are wrong-code-right-line",
        100.0 * (loc_recall - recall) / recall.max(1e-9),
        100.0 * (loc_precision - precision) / precision.max(1e-9),
    );

    if let Some(code) = focus_code {
        print_focus(code, &focus_missed, &focus_spurious, show_cases);
        return;
    }

    // Per-code table: where is recall leaking?
    let mut codes: Vec<(u32, CodeStat)> = totals
        .per_code
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    codes.sort_by(|a, b| {
        let miss_a = a.1.expected - a.1.matched;
        let miss_b = b.1.expected - b.1.matched;
        miss_b
            .cmp(&miss_a)
            .then(b.1.expected.cmp(&a.1.expected))
            .then(a.0.cmp(&b.0))
    });
    println!("\n--- Top codes by MISSED count (where recall leaks) ---");
    println!(
        "{:<10} {:>8} {:>8} {:>8} {:>8} {:>9}",
        "code", "expected", "matched", "missed", "spurious", "recall"
    );
    for (code, st) in codes.iter().take(top) {
        let missed = st.expected - st.matched;
        println!(
            "TS{:<8} {:>8} {:>8} {:>8} {:>8} {:>8.1}%",
            code,
            st.expected,
            st.matched,
            missed,
            st.spurious,
            100.0 * ratio(st.matched as u64, st.expected as u64)
        );
    }
    println!("\nDrill into a code:  --code <NNNN> --show-cases 10");
    println!("Dump all misses:    --save-misses /tmp/misses.txt");
}

fn ratio(num: u64, den: u64) -> f64 {
    if den == 0 {
        1.0
    } else {
        num as f64 / den as f64
    }
}

#[allow(clippy::too_many_arguments)]
fn accumulate(
    r: &BaselineResult,
    include_config: bool,
    focus_code: Option<u32>,
    totals: &mut Totals,
    all_misses: &mut Vec<MissRow>,
    focus_missed: &mut Vec<MissRow>,
    focus_spurious: &mut Vec<MissRow>,
    wrongcode: &mut Vec<WrongCodeRow>,
) {
    // Only cases where a tsc baseline was resolved carry signal.
    if !r.baseline_exists {
        return;
    }
    let expected = to_multiset(&parse_diags(&r.expected_output), include_config);
    let actual = to_multiset(&parse_diags(&r.actual_output), include_config);

    if expected.is_empty() && actual.is_empty() {
        return; // no type errors either way — nothing to measure
    }

    // Location-level multisets: collapse code, key on (file, line).
    let mut exp_loc: HashMap<(String, u32), u32> = HashMap::new();
    let mut act_loc: HashMap<(String, u32), u32> = HashMap::new();
    for (k, n) in &expected {
        *exp_loc.entry((k.0.clone(), k.1)).or_insert(0) += n;
    }
    for (k, n) in &actual {
        *act_loc.entry((k.0.clone(), k.1)).or_insert(0) += n;
    }
    for (k, &en) in &exp_loc {
        let an = act_loc.get(k).copied().unwrap_or(0);
        totals.loc_expected += en as u64;
        totals.loc_matched += en.min(an) as u64;
    }
    totals.loc_actual += act_loc.values().map(|v| *v as u64).sum::<u64>();

    // Wrong-code-right-line pairs: an expected code we miss and an actual
    // code we emit on the SAME (file, line) — the cheapest conversions.
    for (key, &exp_n) in &expected {
        let act_n = actual.get(key).copied().unwrap_or(0);
        if exp_n <= act_n {
            continue; // fully matched
        }
        for (akey, &a_n) in &actual {
            if akey.0 == key.0 && akey.1 == key.1 && akey.2 != key.2 {
                let a_matched = expected.get(akey).copied().unwrap_or(0);
                if a_n > a_matched {
                    wrongcode.push(WrongCodeRow {
                        case: r.name.clone(),
                        file: key.0.clone(),
                        line: key.1,
                        expected_code: key.2,
                        our_code: akey.2,
                    });
                }
            }
        }
    }

    let exp_total: u32 = expected.values().sum();
    if exp_total > 0 {
        totals.cases_with_expected += 1;
    }

    let mut case_matched = 0u32;
    let mut case_spurious = 0u32;

    // Walk expected keys: matched = min(exp, act).
    for (key, &exp_n) in &expected {
        let act_n = actual.get(key).copied().unwrap_or(0);
        let matched = exp_n.min(act_n);
        let missed = exp_n - matched;
        totals.expected += exp_n as u64;
        totals.matched += matched as u64;
        case_matched += matched;
        let st = totals.per_code.entry(key.2).or_default();
        st.expected += exp_n;
        st.matched += matched;
        for _ in 0..missed {
            all_misses.push(MissRow {
                case: r.name.clone(),
                file: key.0.clone(),
                line: key.1,
                code: key.2,
            });
            if focus_code == Some(key.2) {
                focus_missed.push(MissRow {
                    case: r.name.clone(),
                    file: key.0.clone(),
                    line: key.1,
                    code: key.2,
                });
            }
        }
    }

    // Walk actual keys for spurious (false positives).
    for (key, &act_n) in &actual {
        let exp_n = expected.get(key).copied().unwrap_or(0);
        let extra = act_n.saturating_sub(exp_n);
        totals.actual += act_n as u64;
        if extra > 0 {
            case_spurious += extra;
            let st = totals.per_code.entry(key.2).or_default();
            st.spurious += extra;
            if focus_code == Some(key.2) {
                for _ in 0..extra {
                    focus_spurious.push(MissRow {
                        case: r.name.clone(),
                        file: key.0.clone(),
                        line: key.1,
                        code: key.2,
                    });
                }
            }
        }
    }

    if exp_total > 0 && case_matched == exp_total && case_spurious == 0 {
        totals.cases_perfect += 1;
    }
}

fn print_focus(code: u32, missed: &[MissRow], spurious: &[MissRow], show_cases: usize) {
    println!("\n--- TS{code}: missed ({} total) ---", missed.len());
    for m in missed.iter().take(show_cases) {
        println!("  MISS  {}  {}:{}", m.case, m.file, m.line);
    }
    if missed.len() > show_cases {
        println!("  ... and {} more", missed.len() - show_cases);
    }
    println!("\n--- TS{code}: spurious ({} total) ---", spurious.len());
    for m in spurious.iter().take(show_cases) {
        println!("  FP    {}  {}:{}", m.case, m.file, m.line);
    }
    if spurious.len() > show_cases {
        println!("  ... and {} more", spurious.len() - show_cases);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_colored_oracle_headers_are_counted_once() {
        let baseline = include_str!(
            "../../../../tests/baselines/reference/manyCompilerErrorsInTheTwoFiles.errors.txt"
        );
        let ts1002: Vec<_> = parse_diags(baseline)
            .into_iter()
            .filter(|diagnostic| diagnostic.2 == 1002)
            .collect();

        assert_eq!(ts1002, [("a.ts".to_string(), 4, 1002)]);
    }

    #[test]
    fn plain_pretty_and_global_headers_reject_embedded_marker_text() {
        assert!(matches!(
            strip_ansi_csi("plain.ts:3:7 - error TS1206: message"),
            Cow::Borrowed(_)
        ));
        assert_eq!(
            parse_diags(
                "plain.ts(2,4): error TS1002: Unterminated string literal.\n\
                 src/pretty.ts:3:7 - error TS1206: Decorators are not valid here.\n\
                 C:\\work\\windows.ts:4:9 - warning TS9998: warning\n\
                 error TS9997: global\n\
                 const sample = \"error TS1111: not a header\";\n\
                 folder/error TS2222/file.ts:5:1 - error TS9996: real header\n\
                 ==== plain.ts (1 errors) ====\n\
                 !!! error TS3333: annotation\n"
            ),
            [
                ("plain.ts".to_string(), 2, 1002),
                ("pretty.ts".to_string(), 3, 1206),
                ("windows.ts".to_string(), 4, 9998),
                (String::new(), 0, 9997),
                ("file.ts".to_string(), 5, 9996),
            ]
        );
    }

    #[test]
    fn ansi_stripping_preserves_utf8_around_csi_sequences() {
        let baseline = "\u{1b}[96mΔ.ts\u{1b}[0m:\u{1b}[93m6\u{1b}[0m:\u{1b}[93m2\u{1b}[0m - \
             \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS1002\u{1b}[0m: message\n==== Δ.ts ====\n";
        assert_eq!(parse_diags(baseline), [("Δ.ts".to_string(), 6, 1002)]);
    }

    #[test]
    fn expanded_error_inventory_preserves_matrix_and_wildcard_oracles() {
        let root = workspace_root();
        let runner = BaselineRunner::new(&root);

        let conformance = runner.discover_error_variants(Suite::Conformance).unwrap();
        let unicode: Vec<_> = conformance
            .iter()
            .filter(|variant| {
                variant
                    .source_case_id
                    .ends_with("/unicodeExtendedEscapesInStrings25.ts")
            })
            .collect();
        assert_eq!(unicode.len(), 2);
        assert_eq!(
            unicode
                .iter()
                .filter_map(|variant| variant.attributes.get("target").map(String::as_str))
                .collect::<Vec<_>>(),
            ["es5", "es6"]
        );
        for variant in &unicode {
            let target = variant.attributes.get("target").unwrap();
            assert!(
                variant
                    .oracle_path
                    .as_deref()
                    .and_then(std::path::Path::file_name)
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.contains(&format!("target={target}"))),
                "variant {target} selected the wrong alias oracle: {:?}",
                variant.oracle_path
            );
        }
        let unicode_ts1002 = unicode
            .iter()
            .map(|variant| {
                let expanded =
                    runner.run_expanded_variant(variant, Suite::Conformance, BaselineKind::Errors);
                parse_diags(&expanded.result.expected_output)
                    .into_iter()
                    .filter(|diagnostic| diagnostic.2 == 1002)
                    .count()
            })
            .sum::<usize>();
        assert_eq!(unicode_ts1002, 2);

        let compiler = runner.discover_error_variants(Suite::Compiler).unwrap();
        let decorator_variants: Vec<_> = compiler
            .iter()
            .filter(|variant| {
                variant
                    .source_case_id
                    .ends_with("/useBeforeDeclaration_classDecorators.2.ts")
            })
            .collect();
        assert_eq!(decorator_variants.len(), 2);
        let decorators = decorator_variants
            .iter()
            .copied()
            .find(|variant| {
                variant.attributes.get("experimentaldecorators") == Some(&"false".to_string())
            })
            .expect("false wildcard decorator case should have an errors oracle");
        assert_eq!(
            decorators.attributes.get("experimentaldecorators"),
            Some(&"false".to_string())
        );
        let source = std::fs::read_to_string(&decorators.source_path).unwrap();
        assert!(source.contains("// @experimentalDecorators: *"));
        let expanded =
            runner.run_expanded_variant(decorators, Suite::Compiler, BaselineKind::Errors);
        assert_eq!(
            parse_diags(&expanded.result.expected_output)
                .into_iter()
                .filter(|diagnostic| diagnostic.2 == 1206)
                .count(),
            9
        );
        let enabled = decorator_variants
            .iter()
            .copied()
            .find(|variant| {
                variant.attributes.get("experimentaldecorators") == Some(&"true".to_string())
            })
            .expect("true wildcard decorator case should be an implicit empty oracle");
        assert!(enabled.oracle_path.is_none());
        let enabled = runner.run_expanded_variant(enabled, Suite::Compiler, BaselineKind::Errors);
        assert!(enabled.result.baseline_exists);
        assert!(enabled.result.expected_output.is_empty());
        assert_eq!(
            parse_diags(&enabled.result.actual_output)
                .into_iter()
                .filter(|diagnostic| diagnostic.2 == 1206)
                .count(),
            0,
            "the concrete true option must suppress legacy-decorator TS1206"
        );
    }
}
