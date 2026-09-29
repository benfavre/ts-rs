use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tsc_rs_harness::{
    baseline_variant::{
        canonical_variant_case_id, validate_normalized_oracle_path, ExpandedBaselineResult,
    },
    cache_key::compiler_source_hash,
    BaselineKind, BaselineResult, BaselineRunner, Suite,
};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p tsc_rs_harness --bin baseline-report -- [options]\n\
         \n\
         Options:\n\
           --suite <compiler|conformance|fourslash|project>   (default: compiler)\n\
           --root <workspace-root>                            (default: auto-detected)\n\
           --limit <n>                                        run only first N discovered cases\n\
           --top <n>                                          show top N mismatch buckets (default: 20)\n\
           --kind <bucket>                                    list cases for one bucket\n\
           --name-contains <text>                             filter listed cases by name substring\n\
           --show-samples <n>                                 show sample count per bucket (default: 3)\n\
           --json                                             print structured JSON output\n\
           --baseline <js|declarations|errors|symbols|types>    baseline kind to compare (default: js)\n\
           --expand-variants                                  opt in to one case per concrete JS oracle\n\
           --manifest-schema <1|2>                            manifest schema (v2 requires --expand-variants)\n\
           --js-skips-only                                    run the selected non-JS baseline only for cases with no JS oracle\n\
           --save-case-list <path>                            write selected case names to file\n\
           --save-manifest <path>                             atomically write a complete per-case JSON manifest\n\
           --no-cache                                         skip result cache\n\
           --incremental [n]                                  re-run cached failures + N sampled passing tests\n\
                                                              (default sample size: 500; merges with cached results)\n\
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailureRecord {
    name: String,
    bucket: String,
    line: Option<usize>,
    expected: Option<String>,
    actual: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ManifestStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ManifestRecord {
    case_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_case_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant_attributes: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle_path: Option<String>,
    name: String,
    oracle_exists: bool,
    status: ManifestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_bucket: Option<String>,
}

#[derive(Debug, Serialize)]
struct CaseManifest {
    schema_version: u32,
    suite: String,
    baseline: String,
    scope: String,
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    records: Vec<ManifestRecord>,
}

struct RunReport {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<FailureRecord>,
    /// Complete records are present only for cache-free, non-incremental runs.
    records: Option<Vec<ManifestRecord>>,
}

fn first_mismatch(expected: &str, actual: &str) -> (Option<usize>, Option<String>, Option<String>) {
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();
    let min_len = exp_lines.len().min(act_lines.len());

    for i in 0..min_len {
        if exp_lines[i] != act_lines[i] {
            return (
                Some(i + 1),
                Some(exp_lines[i].to_string()),
                Some(act_lines[i].to_string()),
            );
        }
    }

    if exp_lines.len() != act_lines.len() {
        return (
            Some(min_len + 1),
            exp_lines.get(min_len).map(|s| s.to_string()),
            act_lines.get(min_len).map(|s| s.to_string()),
        );
    }

    (None, None, None)
}

fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with("*")
}

fn classify_failure(result: &BaselineResult) -> FailureRecord {
    if !result.baseline_exists {
        return FailureRecord {
            name: result.name.clone(),
            bucket: "NO-BASELINE".to_string(),
            line: None,
            expected: None,
            actual: None,
        };
    }

    let diff = result.diff.as_deref().unwrap_or_default();
    if diff.starts_with("PANIC") {
        return FailureRecord {
            name: result.name.clone(),
            bucket: "PANIC".to_string(),
            line: None,
            expected: None,
            actual: None,
        };
    }
    if diff.starts_with("CRASH") {
        return FailureRecord {
            name: result.name.clone(),
            bucket: "CRASH".to_string(),
            line: None,
            expected: None,
            actual: None,
        };
    }
    if diff.starts_with("UNSUPPORTED-VARIANT-OPTION") {
        return FailureRecord {
            name: result.name.clone(),
            bucket: "UNSUPPORTED-VARIANT-OPTION".to_string(),
            line: None,
            expected: None,
            actual: None,
        };
    }

    let (line, expected, actual) = first_mismatch(&result.expected_output, &result.actual_output);
    let bucket = match (expected.as_deref(), actual.as_deref()) {
        (None, Some(_)) => "EXTRA-LINES-AT-END",
        (Some(_), None) => "MISSING-LINES-AT-END",
        (Some(exp), Some(act)) if exp.starts_with("//// [") && act.starts_with("//// [") => {
            "WRONG-FILE-HEADER"
        }
        (Some(exp), Some(act)) if exp.trim() == act.trim() => "WHITESPACE-ONLY",
        (Some(exp), Some(act)) if is_comment_line(exp) && !is_comment_line(act) => {
            "MISSING-COMMENT"
        }
        (Some(exp), Some(act)) if !is_comment_line(exp) && is_comment_line(act) => "EXTRA-COMMENT",
        _ => "CODE-DIFF",
    };

    FailureRecord {
        name: result.name.clone(),
        bucket: bucket.to_string(),
        line,
        expected,
        actual,
    }
}

fn trunc(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        return line.to_string();
    }
    let mut out = String::new();
    for ch in line.chars().take(max.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

// --- Result cache ---

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Serialize, Deserialize, Clone)]
struct TestEntry {
    name: String,
    status: TestStatus,
}

#[derive(Serialize, Deserialize)]
struct ReportCache {
    source_hash: u64,
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<FailureRecord>,
    /// Per-test status for incremental re-runs. Absent in legacy caches.
    #[serde(default)]
    tests: Vec<TestEntry>,
}

fn cache_path(root: &Path, suite: &str, bin: &str) -> PathBuf {
    root.join(".harness-cache")
        .join(format!("{bin}-{suite}.json"))
}

fn load_cache(path: &Path) -> Option<ReportCache> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_cache(path: &Path, cache: &ReportCache) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, data);
    }
}

fn status_of(result: &BaselineResult) -> TestStatus {
    if !result.baseline_exists {
        TestStatus::Skipped
    } else if result.passed {
        TestStatus::Passed
    } else {
        TestStatus::Failed
    }
}

/// Deterministic-per-process pseudorandom sample using time-seeded xorshift.
fn sample_names(mut candidates: Vec<String>, n: usize) -> Vec<String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let take = n.min(candidates.len());
    if take == 0 {
        return Vec::new();
    }
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15);
    if state == 0 {
        state = 0x9E3779B97F4A7C15;
    }
    let len = candidates.len();
    for i in 0..take {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = i + (state as usize) % (len - i);
        candidates.swap(i, j);
    }
    candidates.truncate(take);
    candidates
}

fn build_failures(results: &[BaselineResult]) -> Vec<FailureRecord> {
    results
        .iter()
        .filter(|r| r.baseline_exists && !r.passed)
        .map(classify_failure)
        .collect()
}

fn build_tests(results: &[BaselineResult]) -> Vec<TestEntry> {
    results
        .iter()
        .map(|r| TestEntry {
            name: r.name.clone(),
            status: status_of(r),
        })
        .collect()
}

fn case_id(root: &Path, test_path: &Path) -> String {
    let relative = test_path.strip_prefix(root).unwrap_or(test_path);
    relative
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn build_manifest_records(root: &Path, results: &[BaselineResult]) -> Vec<ManifestRecord> {
    let mut records: Vec<_> = results
        .iter()
        .map(|result| {
            let status = match status_of(result) {
                TestStatus::Passed => ManifestStatus::Passed,
                TestStatus::Failed => ManifestStatus::Failed,
                TestStatus::Skipped => ManifestStatus::Skipped,
            };
            let failure_bucket =
                (status == ManifestStatus::Failed).then(|| classify_failure(result).bucket);
            ManifestRecord {
                case_id: case_id(root, &result.test_path),
                source_case_id: None,
                variant_attributes: None,
                oracle_path: None,
                name: result.name.clone(),
                oracle_exists: result.baseline_exists,
                status,
                failure_bucket,
            }
        })
        .collect();
    records.sort_by(|a, b| a.case_id.cmp(&b.case_id).then_with(|| a.name.cmp(&b.name)));
    records
}

fn build_expanded_manifest_records(results: &[ExpandedBaselineResult]) -> Vec<ManifestRecord> {
    let mut records: Vec<_> = results
        .iter()
        .map(|expanded| {
            let result = &expanded.result;
            let status = match status_of(result) {
                TestStatus::Passed => ManifestStatus::Passed,
                TestStatus::Failed => ManifestStatus::Failed,
                TestStatus::Skipped => ManifestStatus::Skipped,
            };
            let failure_bucket =
                (status == ManifestStatus::Failed).then(|| classify_failure(result).bucket);
            ManifestRecord {
                case_id: expanded.variant.case_id.clone(),
                source_case_id: Some(expanded.variant.source_case_id.clone()),
                variant_attributes: Some(expanded.variant.attributes.clone()),
                oracle_path: expanded.variant.oracle_relative_path.clone(),
                name: expanded.variant.name.clone(),
                oracle_exists: result.baseline_exists,
                status,
                failure_bucket,
            }
        })
        .collect();
    records.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    records
}

fn write_manifest_atomic(path: &Path, manifest: &CaseManifest) -> Result<(), String> {
    if !matches!(manifest.schema_version, 1 | 2) {
        return Err(format!(
            "refusing unsupported manifest schema {}",
            manifest.schema_version
        ));
    }
    let valid_matrix = match manifest.schema_version {
        1 => {
            matches!(
                manifest.baseline.as_str(),
                "js" | "errors" | "symbols" | "types"
            ) && matches!(manifest.scope.as_str(), "all" | "js-skips")
                && !(manifest.baseline == "js" && manifest.scope == "js-skips")
        }
        2 => matches!(manifest.baseline.as_str(), "js" | "declarations") && manifest.scope == "all",
        _ => false,
    };
    if !valid_matrix {
        return Err(format!(
            "refusing invalid schema-v{} manifest matrix: baseline={} scope={}",
            manifest.schema_version, manifest.baseline, manifest.scope
        ));
    }
    let mut case_ids = HashSet::new();
    for record in &manifest.records {
        if record.case_id.is_empty() || record.name.is_empty() {
            return Err("refusing to write a manifest with an empty case id or name".to_string());
        }
        if !case_ids.insert(record.case_id.as_str()) {
            return Err(format!(
                "refusing to write duplicate manifest case id: {}",
                record.case_id
            ));
        }
        if record.oracle_exists == (record.status == ManifestStatus::Skipped) {
            return Err(format!(
                "refusing inconsistent oracle/status for manifest case {}",
                record.case_id
            ));
        }
        if (record.status == ManifestStatus::Failed) != record.failure_bucket.is_some() {
            return Err(format!(
                "refusing inconsistent failure bucket for manifest case {}",
                record.case_id
            ));
        }
        match manifest.schema_version {
            1 if record.source_case_id.is_some()
                || record.variant_attributes.is_some()
                || record.oracle_path.is_some() =>
            {
                return Err(format!(
                    "refusing v1 manifest record with variant provenance: {}",
                    record.case_id
                ));
            }
            2 if record.source_case_id.is_none()
                || record.variant_attributes.is_none()
                || (record.oracle_exists != record.oracle_path.is_some()) =>
            {
                return Err(format!(
                    "refusing v2 manifest record with incomplete variant provenance: {}",
                    record.case_id
                ));
            }
            2 => {
                if let Some(path) = record.oracle_path.as_deref() {
                    validate_normalized_oracle_path(path).map_err(|error| {
                        format!(
                            "refusing invalid v2 manifest oracle path for {}: {error}",
                            record.case_id
                        )
                    })?;
                }
                let canonical_id = canonical_variant_case_id(
                    record.source_case_id.as_deref().expect("checked above"),
                    record.variant_attributes.as_ref().expect("checked above"),
                )
                .map_err(|error| {
                    format!(
                        "refusing non-canonical v2 manifest provenance for {}: {error}",
                        record.case_id
                    )
                })?;
                if record.case_id != canonical_id {
                    return Err(format!(
                        "refusing non-canonical v2 manifest case id: {} (expected {canonical_id})",
                        record.case_id
                    ));
                }
            }
            _ => {}
        }
    }
    let manifest_passed = manifest
        .records
        .iter()
        .filter(|record| record.status == ManifestStatus::Passed)
        .count();
    let manifest_failed = manifest
        .records
        .iter()
        .filter(|record| record.status == ManifestStatus::Failed)
        .count();
    let manifest_skipped = manifest
        .records
        .iter()
        .filter(|record| record.status == ManifestStatus::Skipped)
        .count();
    if manifest.total != manifest.records.len()
        || manifest.passed != manifest_passed
        || manifest.failed != manifest_failed
        || manifest.skipped != manifest_skipped
    {
        return Err(format!(
            "refusing to write inconsistent manifest counts: summary={}/{}/{}/{} records={}/{}/{}/{}",
            manifest.total,
            manifest.passed,
            manifest.failed,
            manifest.skipped,
            manifest.records.len(),
            manifest_passed,
            manifest_failed,
            manifest_skipped
        ));
    }

    let mut data = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("failed to serialize manifest: {error}"))?;
    data.push(b'\n');

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create manifest directory {}: {error}",
            parent.display()
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid manifest path: {}", path.display()))?;

    let mut temp_path = None;
    let mut temp_file = None;
    for attempt in 0..100u32 {
        let candidate = parent.join(format!(
            ".{file_name}.tmp.{}.{}",
            std::process::id(),
            attempt
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "failed to create temporary manifest in {}: {error}",
                    parent.display()
                ));
            }
        }
    }

    let temp_path = temp_path.ok_or_else(|| {
        format!(
            "failed to allocate a temporary manifest path in {}",
            parent.display()
        )
    })?;
    let mut file = temp_file.expect("temporary manifest file must accompany its path");
    use std::io::Write;
    if let Err(error) = file.write_all(&data).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!(
            "failed to write manifest {}: {error}",
            path.display()
        ));
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!(
            "failed to install manifest {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

fn counts_from_statuses<'a>(
    entries: impl Iterator<Item = &'a TestStatus>,
) -> (usize, usize, usize, usize) {
    let mut total = 0usize;
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for s in entries {
        total += 1;
        match s {
            TestStatus::Passed => passed += 1,
            TestStatus::Failed => failed += 1,
            TestStatus::Skipped => skipped += 1,
        }
    }
    (total, passed, failed, skipped)
}

fn main() {
    let mut args = std::env::args().skip(1).peekable();

    let mut suite = Suite::Compiler;
    let mut root = workspace_root();
    let mut limit: Option<usize> = None;
    let mut top = 20usize;
    let mut filter_kind: Option<String> = None;
    let mut name_contains: Option<String> = None;
    let mut show_samples = 3usize;
    let mut json_output = false;
    let mut save_case_list: Option<PathBuf> = None;
    let mut save_manifest: Option<PathBuf> = None;
    let mut no_cache = false;
    let mut baseline_kind = BaselineKind::Js;
    let mut js_skips_only = false;
    let mut incremental: Option<usize> = None;
    let mut expand_variants = false;
    let mut manifest_schema = 1u32;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "--suite" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --suite requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                suite = match val.parse::<Suite>() {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: invalid suite '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--root" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --root requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                root = PathBuf::from(val);
            }
            "--limit" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --limit requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                limit = match val.parse::<usize>() {
                    Ok(v) => Some(v),
                    Err(e) => {
                        eprintln!("error: invalid --limit value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--top" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --top requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                top = match val.parse::<usize>() {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("error: invalid --top value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--kind" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --kind requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                filter_kind = Some(val);
            }
            "--name-contains" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --name-contains requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                name_contains = Some(val);
            }
            "--show-samples" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --show-samples requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                show_samples = match val.parse::<usize>() {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("error: invalid --show-samples value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--json" => {
                json_output = true;
            }
            "--no-cache" => {
                no_cache = true;
            }
            "--incremental" => {
                // Optional numeric arg; defaults to 500 if next token isn't a number.
                let sample = args.peek().and_then(|s| s.parse::<usize>().ok());
                if sample.is_some() {
                    args.next();
                }
                incremental = Some(sample.unwrap_or(500));
            }
            "--baseline" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --baseline requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                baseline_kind = match val.parse::<BaselineKind>() {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("error: invalid baseline kind '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--expand-variants" => {
                expand_variants = true;
            }
            "--manifest-schema" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --manifest-schema requires a value");
                    std::process::exit(2);
                };
                manifest_schema = match val.parse::<u32>() {
                    Ok(version @ (1 | 2)) => version,
                    _ => {
                        eprintln!("error: --manifest-schema must be 1 or 2");
                        std::process::exit(2);
                    }
                };
            }
            "--js-skips-only" => {
                js_skips_only = true;
            }
            "--save-case-list" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --save-case-list requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                save_case_list = Some(PathBuf::from(val));
            }
            "--save-manifest" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --save-manifest requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                save_manifest = Some(PathBuf::from(val));
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option '{other}'");
                print_usage();
                std::process::exit(2);
            }
            other => {
                eprintln!("error: unexpected argument '{other}'");
                print_usage();
                std::process::exit(2);
            }
        }
    }

    if js_skips_only && baseline_kind == BaselineKind::Js {
        eprintln!("error: --js-skips-only requires --baseline errors, symbols, or types");
        std::process::exit(2);
    }
    if expand_variants != (manifest_schema == 2) {
        eprintln!("error: --expand-variants and --manifest-schema 2 must be specified together");
        std::process::exit(2);
    }
    if expand_variants && !matches!(baseline_kind, BaselineKind::Js | BaselineKind::Declarations) {
        eprintln!("error: expanded variants currently support only js and declarations");
        std::process::exit(2);
    }
    if baseline_kind == BaselineKind::Declarations && !expand_variants {
        eprintln!("error: declarations require --expand-variants --manifest-schema 2");
        std::process::exit(2);
    }
    if expand_variants && (js_skips_only || incremental.is_some() || limit.is_some()) {
        eprintln!("error: expanded variants require a complete non-incremental all-case run");
        std::process::exit(2);
    }
    if js_skips_only && incremental.is_some() {
        eprintln!("error: --js-skips-only cannot be combined with --incremental");
        std::process::exit(2);
    }
    if save_manifest.is_some() && incremental.is_some() {
        eprintln!("error: --save-manifest cannot be combined with --incremental");
        std::process::exit(2);
    }
    if save_manifest.is_some() && limit.is_some() {
        eprintln!("error: --save-manifest cannot be combined with --limit");
        std::process::exit(2);
    }

    let source_hash = compiler_source_hash(&root);
    // A canonical manifest must contain fresh, complete per-case records; the
    // aggregate cache intentionally does not carry enough information for it.
    let use_cache = limit.is_none() && !no_cache && save_manifest.is_none() && !expand_variants;
    let cache_tag = format!("report-{}", baseline_kind.as_str());
    let cf = if use_cache {
        Some(cache_path(&root, suite.as_str(), &cache_tag))
    } else {
        None
    };

    let report = if expand_variants {
        run_full_expanded(&root, suite, baseline_kind)
    } else if js_skips_only {
        run_js_skips_only(&root, suite, baseline_kind)
    } else if let Some(sample_size) = incremental {
        run_incremental(
            &root,
            suite,
            baseline_kind,
            sample_size,
            cf.as_deref(),
            source_hash,
        )
    } else {
        // Normal mode: try cache, else full run.
        let cached = cf
            .as_deref()
            .and_then(load_cache)
            .filter(|c| source_hash == Some(c.source_hash));

        if let Some(c) = cached {
            eprintln!("(using cached results)");
            RunReport {
                total: c.total,
                passed: c.passed,
                failed: c.failed,
                skipped: c.skipped,
                failures: c.failures,
                records: None,
            }
        } else {
            run_full(
                &root,
                suite,
                baseline_kind,
                limit,
                cf.as_deref(),
                source_hash,
            )
        }
    };

    let RunReport {
        total,
        passed,
        failed,
        skipped,
        failures,
        records,
    } = report;

    if let Some(path) = save_manifest.as_deref() {
        let Some(records) = records else {
            eprintln!("error: complete per-case results unavailable for manifest");
            std::process::exit(2);
        };
        let manifest = CaseManifest {
            schema_version: manifest_schema,
            suite: suite.as_str().to_string(),
            baseline: baseline_kind.as_str().to_string(),
            scope: if js_skips_only {
                "js-skips".to_string()
            } else {
                "all".to_string()
            },
            total,
            passed,
            failed,
            skipped,
            records,
        };
        if let Err(error) = write_manifest_atomic(path, &manifest) {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    }

    let pass_rate = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64 * 100.0
    };

    if !json_output {
        println!(
            "suite={}{} total={} passed={} failed={} skipped={} pass_rate={:.1}%",
            suite.as_str(),
            if js_skips_only { " scope=js-skips" } else { "" },
            total,
            passed,
            failed,
            skipped,
            pass_rate
        );
    }

    let mut by_bucket: HashMap<String, Vec<&FailureRecord>> = HashMap::new();
    for failure in &failures {
        by_bucket
            .entry(failure.bucket.clone())
            .or_default()
            .push(failure);
    }

    let mut buckets: Vec<(String, Vec<&FailureRecord>)> = by_bucket.into_iter().collect();
    buckets.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

    let selected_cases: Vec<&FailureRecord> = if let Some(kind) = filter_kind.as_deref() {
        let name_filter = name_contains
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut matches: Vec<&FailureRecord> = failures
            .iter()
            .filter(|f| f.bucket.eq_ignore_ascii_case(kind))
            .filter(|f| {
                name_filter.is_empty() || f.name.to_ascii_lowercase().contains(&name_filter)
            })
            .collect();
        matches.sort_by(|a, b| a.name.cmp(&b.name));
        matches
    } else {
        let mut all: Vec<&FailureRecord> = failures.iter().collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    };

    if let Some(path) = save_case_list {
        let mut content = String::new();
        for case in &selected_cases {
            content.push_str(&case.name);
            content.push('\n');
        }
        if let Err(e) = std::fs::write(&path, content) {
            eprintln!("failed to write case list to {}: {e}", path.display());
            std::process::exit(2);
        }
    }

    if json_output {
        let bucket_rows: Vec<_> = buckets
            .iter()
            .take(top)
            .map(|(bucket, cases)| {
                let samples: Vec<_> = cases
                    .iter()
                    .take(show_samples)
                    .map(|s| s.name.clone())
                    .collect();
                json!({
                    "bucket": bucket,
                    "count": cases.len(),
                    "samples": samples
                })
            })
            .collect();

        let selected_rows: Vec<_> = selected_cases
            .iter()
            .map(|m| {
                json!({
                    "name": m.name,
                    "bucket": m.bucket,
                    "line": m.line,
                    "expected": m.expected,
                    "actual": m.actual
                })
            })
            .collect();

        let payload = json!({
            "suite": suite.as_str(),
            "baseline": baseline_kind.as_str(),
            "scope": if js_skips_only { "js-skips" } else { "all" },
            "total": total,
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
            "pass_rate": pass_rate,
            "top_buckets": bucket_rows,
            "filter": {
                "kind": filter_kind,
                "name_contains": name_contains
            },
            "selected_cases": selected_rows
        });

        match serde_json::to_string_pretty(&payload) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("failed to serialize json output: {e}");
                std::process::exit(2);
            }
        }
        return;
    }

    println!("\nTop buckets:");
    for (idx, (bucket, cases)) in buckets.iter().enumerate() {
        if idx >= top {
            break;
        }
        println!("{:>6}  {}", cases.len(), bucket);
        for sample in cases.iter().take(show_samples) {
            println!("        - {}", sample.name);
        }
    }

    if let Some(kind) = filter_kind.as_deref() {
        println!("\nCases in bucket '{}': {}", kind, selected_cases.len());
        for m in selected_cases {
            println!("- {}", m.name);
            if let Some(line) = m.line {
                println!("  line: {}", line);
            }
            if let Some(ref exp) = m.expected {
                println!("  expected: {}", trunc(exp, 120));
            }
            if let Some(ref act) = m.actual {
                println!("  actual:   {}", trunc(act, 120));
            }
        }
    }
}

/// Reclassify cases that have no JavaScript oracle against a real alternate
/// oracle. This never turns a missing `.js` file into a JavaScript pass: it
/// answers the narrower question "how many JS-skipped cases already match
/// their errors/types/symbols baseline?" and keeps the remaining failures
/// visible under the normal mismatch buckets.
fn run_js_skips_only(root: &Path, suite: Suite, baseline_kind: BaselineKind) -> RunReport {
    use rayon::prelude::*;

    let runner = BaselineRunner::new(root);
    let js_result = runner
        .run_suite_with_kind(suite, BaselineKind::Js, None)
        .unwrap_or_else(|e| {
            eprintln!("failed to run JavaScript discovery suite: {e}");
            std::process::exit(2);
        });
    let paths: Vec<PathBuf> = js_result
        .results
        .into_iter()
        .filter(|result| !result.baseline_exists)
        .map(|result| result.test_path)
        .collect();

    let requested_threads = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|count| count.get().min(8))
                .unwrap_or(4)
        });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(requested_threads.max(1))
        .stack_size(8 * 1024 * 1024)
        .build()
        .expect("failed to build rayon thread pool");
    let results: Vec<BaselineResult> = pool.install(|| {
        paths
            .par_iter()
            .map(|path| runner.run_case_with_kind(path, suite, baseline_kind))
            .collect()
    });

    let total = results.len();
    let passed = results
        .iter()
        .filter(|result| result.baseline_exists && result.passed)
        .count();
    let failed = results
        .iter()
        .filter(|result| result.baseline_exists && !result.passed)
        .count();
    let skipped = results
        .iter()
        .filter(|result| !result.baseline_exists)
        .count();
    let failures = build_failures(&results);
    let records = build_manifest_records(root, &results);
    RunReport {
        total,
        passed,
        failed,
        skipped,
        failures,
        records: Some(records),
    }
}

fn run_full(
    root: &Path,
    suite: Suite,
    baseline_kind: BaselineKind,
    limit: Option<usize>,
    cache_file: Option<&Path>,
    source_hash: Option<u64>,
) -> RunReport {
    let runner = BaselineRunner::new(root);
    let suite_result = match runner.run_suite_with_kind(suite, baseline_kind, limit) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("failed to run suite: {e}");
            std::process::exit(2);
        }
    };

    let failures = build_failures(&suite_result.results);
    let tests = build_tests(&suite_result.results);
    let records = build_manifest_records(root, &suite_result.results);

    if let (Some(path), Some(hash)) = (cache_file, source_hash) {
        save_cache(
            path,
            &ReportCache {
                source_hash: hash,
                total: suite_result.total,
                passed: suite_result.passed,
                failed: suite_result.failed,
                skipped: suite_result.skipped,
                failures: failures.clone(),
                tests,
            },
        );
    }

    RunReport {
        total: suite_result.total,
        passed: suite_result.passed,
        failed: suite_result.failed,
        skipped: suite_result.skipped,
        failures,
        records: Some(records),
    }
}

fn run_full_expanded(root: &Path, suite: Suite, baseline_kind: BaselineKind) -> RunReport {
    let runner = BaselineRunner::new(root);
    let expanded = match runner.run_expanded_js_suite(suite, baseline_kind) {
        Ok(results) => results,
        Err(error) => {
            eprintln!("failed to run expanded suite: {error}");
            std::process::exit(2);
        }
    };
    let results: Vec<_> = expanded
        .iter()
        .map(|expanded| expanded.result.clone())
        .collect();
    let total = results.len();
    let passed = results
        .iter()
        .filter(|result| result.baseline_exists && result.passed)
        .count();
    let failed = results
        .iter()
        .filter(|result| result.baseline_exists && !result.passed)
        .count();
    let skipped = results
        .iter()
        .filter(|result| !result.baseline_exists)
        .count();
    RunReport {
        total,
        passed,
        failed,
        skipped,
        failures: build_failures(&results),
        records: Some(build_expanded_manifest_records(&expanded)),
    }
}

fn run_incremental(
    root: &Path,
    suite: Suite,
    baseline_kind: BaselineKind,
    sample_size: usize,
    cache_file: Option<&Path>,
    source_hash: Option<u64>,
) -> RunReport {
    let Some(cf) = cache_file else {
        eprintln!("error: --incremental requires a writable cache (don't combine with --limit or --no-cache)");
        std::process::exit(2);
    };
    let Some(prior) = load_cache(cf) else {
        eprintln!("(no prior cache — running full suite)");
        return run_full(root, suite, baseline_kind, None, Some(cf), source_hash);
    };
    if prior.tests.is_empty() {
        eprintln!("(legacy cache without per-test status — running full suite)");
        return run_full(root, suite, baseline_kind, None, Some(cf), source_hash);
    }

    let runner = BaselineRunner::new(root);
    let all_paths = match runner.discover_cases(suite) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("failed to discover cases: {e}");
            std::process::exit(2);
        }
    };
    let mut stem_to_path: HashMap<String, PathBuf> = HashMap::new();
    for path in &all_paths {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            stem_to_path
                .entry(stem.to_string())
                .or_insert_with(|| path.clone());
        }
    }

    // Build prior status map.
    let mut prior_status: HashMap<String, TestStatus> = prior
        .tests
        .iter()
        .map(|t| (t.name.clone(), t.status))
        .collect();

    let failing_names: HashSet<String> = prior.failures.iter().map(|f| f.name.clone()).collect();

    // Sample pool: known-passing tests (exclude failures and skipped).
    let pass_candidates: Vec<String> = prior
        .tests
        .iter()
        .filter(|t| t.status == TestStatus::Passed)
        .map(|t| t.name.clone())
        .collect();
    let sampled = sample_names(pass_candidates, sample_size);

    // Rerun set = all prior failures ∪ sampled passing.
    let mut rerun: Vec<String> = failing_names.iter().cloned().collect();
    rerun.extend(sampled.iter().cloned());

    // Resolve names to paths; drop unknown (deleted) tests.
    let rerun_paths: Vec<(String, PathBuf)> = rerun
        .iter()
        .filter_map(|name| stem_to_path.get(name).map(|p| (name.clone(), p.clone())))
        .collect();

    eprintln!(
        "(incremental: re-running {} failing + {} sampled passing = {} tests)",
        failing_names.len(),
        sampled.len(),
        rerun_paths.len()
    );

    // Parallel rerun.
    use rayon::prelude::*;
    let mut pool_builder = rayon::ThreadPoolBuilder::new().stack_size(8 * 1024 * 1024);
    if let Some(n) = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        pool_builder = pool_builder.num_threads(n);
    }
    let pool = pool_builder
        .build()
        .expect("failed to build rayon thread pool");
    let fresh: Vec<BaselineResult> = pool.install(|| {
        rerun_paths
            .par_iter()
            .map(|(_, p)| runner.run_case_with_kind(p, suite, baseline_kind))
            .collect()
    });

    // Update status map and failure list.
    let mut failures_by_name: HashMap<String, FailureRecord> = prior
        .failures
        .into_iter()
        .map(|f| (f.name.clone(), f))
        .collect();

    let mut regressions: Vec<String> = Vec::new();
    for result in &fresh {
        let new_status = status_of(result);
        let old_status = prior_status
            .get(&result.name)
            .copied()
            .unwrap_or(TestStatus::Skipped);

        if old_status == TestStatus::Passed && new_status == TestStatus::Failed {
            regressions.push(result.name.clone());
        }

        prior_status.insert(result.name.clone(), new_status);

        match new_status {
            TestStatus::Failed => {
                failures_by_name.insert(result.name.clone(), classify_failure(result));
            }
            _ => {
                failures_by_name.remove(&result.name);
            }
        }
    }

    let failures: Vec<FailureRecord> = failures_by_name.into_values().collect();

    // Rebuild test entries from updated status map.
    let tests: Vec<TestEntry> = prior_status
        .iter()
        .map(|(name, status)| TestEntry {
            name: name.clone(),
            status: *status,
        })
        .collect();

    let (total, passed, failed, skipped) = counts_from_statuses(prior_status.values());

    if !regressions.is_empty() {
        eprintln!(
            "\n⚠  {} regression(s) detected in sampled passing tests:",
            regressions.len()
        );
        for name in regressions.iter().take(10) {
            eprintln!("   - {}", name);
        }
        if regressions.len() > 10 {
            eprintln!("   ... and {} more", regressions.len() - 10);
        }
        eprintln!("   Run without --incremental to fully re-verify.");
    }

    if let Some(hash) = source_hash {
        save_cache(
            cf,
            &ReportCache {
                source_hash: hash,
                total,
                passed,
                failed,
                skipped,
                failures: failures.clone(),
                tests,
            },
        );
    }

    RunReport {
        total,
        passed,
        failed,
        skipped,
        failures,
        records: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(name: &str, path: &str, passed: bool, baseline_exists: bool) -> BaselineResult {
        BaselineResult {
            name: name.to_string(),
            test_path: PathBuf::from(path),
            passed,
            diff: (!passed).then(|| "output differs".to_string()),
            baseline_exists,
            actual_output: if passed { "same" } else { "actual" }.to_string(),
            expected_output: if passed { "same" } else { "expected" }.to_string(),
        }
    }

    #[test]
    fn manifest_records_are_sorted_complete_and_classified() {
        let root = Path::new("/repo");
        let results = vec![
            result(
                "z_case",
                "/repo/tests/cases/compiler/z_case.ts",
                false,
                false,
            ),
            result(
                "b_case",
                "/repo/tests/cases/compiler/b_case.ts",
                false,
                true,
            ),
            result("a_case", "/repo/tests/cases/compiler/a_case.ts", true, true),
        ];

        let records = build_manifest_records(root, &results);
        assert_eq!(records.len(), 3);
        assert_eq!(
            records
                .iter()
                .map(|record| record.case_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "tests/cases/compiler/a_case.ts",
                "tests/cases/compiler/b_case.ts",
                "tests/cases/compiler/z_case.ts"
            ]
        );
        assert_eq!(records[0].status, ManifestStatus::Passed);
        assert_eq!(records[0].failure_bucket, None);
        assert_eq!(records[1].status, ManifestStatus::Failed);
        assert_eq!(records[1].failure_bucket.as_deref(), Some("CODE-DIFF"));
        assert_eq!(records[2].status, ManifestStatus::Skipped);
        assert_eq!(records[2].failure_bucket, None);
    }

    #[test]
    fn atomic_manifest_write_emits_schema_and_replaces_existing_file() {
        let temp_dir =
            std::env::temp_dir().join(format!("tsc-rs-baseline-report-{}", std::process::id(),));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let path = temp_dir.join("manifest.json");
        std::fs::write(&path, "stale").unwrap();
        let manifest = CaseManifest {
            schema_version: 1,
            suite: "compiler".to_string(),
            baseline: "errors".to_string(),
            scope: "all".to_string(),
            total: 1,
            passed: 1,
            failed: 0,
            skipped: 0,
            records: vec![ManifestRecord {
                case_id: "tests/cases/compiler/a.ts".to_string(),
                source_case_id: None,
                variant_attributes: None,
                oracle_path: None,
                name: "a".to_string(),
                oracle_exists: true,
                status: ManifestStatus::Passed,
                failure_bucket: None,
            }],
        };

        write_manifest_atomic(&path, &manifest).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["baseline"], "errors");
        assert_eq!(value["records"][0]["status"], "passed");
        assert!(value["records"][0].get("failure_bucket").is_none());
        assert_eq!(
            std::fs::read_dir(&temp_dir).unwrap().count(),
            1,
            "temporary manifest should be renamed or cleaned up"
        );
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn manifest_write_rejects_inconsistent_summary_counts() {
        let manifest = CaseManifest {
            schema_version: 1,
            suite: "compiler".to_string(),
            baseline: "errors".to_string(),
            scope: "all".to_string(),
            total: 1,
            passed: 1,
            failed: 0,
            skipped: 0,
            records: Vec::new(),
        };
        let error = write_manifest_atomic(Path::new("/unused/manifest.json"), &manifest)
            .expect_err("inconsistent counts must be rejected before filesystem access");
        assert!(error.contains("inconsistent manifest counts"));
    }

    #[test]
    fn v2_manifest_requires_and_emits_variant_provenance() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("manifest.json");
        let case_id = "tests/cases/compiler/a.ts::variant(target=es5)";
        let manifest = CaseManifest {
            schema_version: 2,
            suite: "compiler".to_string(),
            baseline: "js".to_string(),
            scope: "all".to_string(),
            total: 1,
            passed: 1,
            failed: 0,
            skipped: 0,
            records: vec![ManifestRecord {
                case_id: case_id.to_string(),
                source_case_id: Some("tests/cases/compiler/a.ts".to_string()),
                variant_attributes: Some(BTreeMap::from([(
                    "target".to_string(),
                    "es5".to_string(),
                )])),
                oracle_path: Some("tests/baselines/reference/a(target=es5).js".to_string()),
                name: "a (target=es5)".to_string(),
                oracle_exists: true,
                status: ManifestStatus::Passed,
                failure_bucket: None,
            }],
        };
        write_manifest_atomic(&path, &manifest).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["records"][0]["case_id"], case_id);
        assert_eq!(value["records"][0]["variant_attributes"]["target"], "es5");

        let mut invalid = manifest;
        invalid.records[0].case_id = "tests/cases/compiler/a.ts".to_string();
        assert!(
            write_manifest_atomic(Path::new("/unused/manifest.json"), &invalid)
                .unwrap_err()
                .contains("non-canonical v2 manifest case id")
        );
        invalid.records[0].case_id = case_id.to_string();
        invalid.records[0].variant_attributes = None;
        assert!(
            write_manifest_atomic(Path::new("/unused/manifest.json"), &invalid)
                .unwrap_err()
                .contains("incomplete variant provenance")
        );
    }

    #[test]
    fn v2_manifest_rejects_empty_or_non_normalized_oracle_paths() {
        let source_case_id = "tests/cases/compiler/a.ts";
        for path in [
            "",
            "./tests/baselines/reference/a.js",
            "tests/baselines/../reference/a.js",
        ] {
            let manifest = CaseManifest {
                schema_version: 2,
                suite: "compiler".to_string(),
                baseline: "js".to_string(),
                scope: "all".to_string(),
                total: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
                records: vec![ManifestRecord {
                    case_id: source_case_id.to_string(),
                    source_case_id: Some(source_case_id.to_string()),
                    variant_attributes: Some(BTreeMap::new()),
                    oracle_path: Some(path.to_string()),
                    name: "a".to_string(),
                    oracle_exists: true,
                    status: ManifestStatus::Passed,
                    failure_bucket: None,
                }],
            };
            let error = write_manifest_atomic(Path::new("/unused/manifest.json"), &manifest)
                .expect_err("invalid oracle path must fail before filesystem access");
            assert!(error.contains("invalid v2 manifest oracle path"), "{error}");
        }
    }

    #[test]
    fn manifest_write_rejects_schema_specific_matrix_violations() {
        let record = ManifestRecord {
            case_id: "tests/cases/compiler/a.ts".to_string(),
            source_case_id: None,
            variant_attributes: None,
            oracle_path: None,
            name: "a".to_string(),
            oracle_exists: true,
            status: ManifestStatus::Passed,
            failure_bucket: None,
        };
        for (schema_version, baseline, scope) in [
            (1, "declarations", "all"),
            (2, "errors", "all"),
            (2, "js", "js-skips"),
        ] {
            let manifest = CaseManifest {
                schema_version,
                suite: "compiler".to_string(),
                baseline: baseline.to_string(),
                scope: scope.to_string(),
                total: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
                records: vec![record.clone()],
            };
            assert!(
                write_manifest_atomic(Path::new("/unused/manifest.json"), &manifest)
                    .unwrap_err()
                    .contains("invalid schema")
            );
        }
    }

    #[test]
    fn manifest_write_rejects_duplicate_ids_and_invalid_case_state() {
        let record = ManifestRecord {
            case_id: "tests/cases/compiler/a.ts".to_string(),
            source_case_id: None,
            variant_attributes: None,
            oracle_path: None,
            name: "a".to_string(),
            oracle_exists: true,
            status: ManifestStatus::Passed,
            failure_bucket: None,
        };
        let duplicate = CaseManifest {
            schema_version: 1,
            suite: "compiler".to_string(),
            baseline: "errors".to_string(),
            scope: "all".to_string(),
            total: 2,
            passed: 2,
            failed: 0,
            skipped: 0,
            records: vec![record.clone(), record.clone()],
        };
        assert!(
            write_manifest_atomic(Path::new("/unused/manifest.json"), &duplicate)
                .unwrap_err()
                .contains("duplicate manifest case id")
        );

        let invalid = CaseManifest {
            schema_version: 1,
            suite: "compiler".to_string(),
            baseline: "errors".to_string(),
            scope: "all".to_string(),
            total: 1,
            passed: 0,
            failed: 0,
            skipped: 1,
            records: vec![ManifestRecord {
                status: ManifestStatus::Skipped,
                ..record
            }],
        };
        assert!(
            write_manifest_atomic(Path::new("/unused/manifest.json"), &invalid)
                .unwrap_err()
                .contains("oracle/status")
        );
    }
}
