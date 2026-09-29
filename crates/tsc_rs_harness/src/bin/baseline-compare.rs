use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use tsc_rs_harness::baseline_variant::{
    canonical_variant_case_id, validate_normalized_oracle_path,
};

const SUPPORTED_SCHEMA_VERSIONS: &[u32] = &[1, 2];

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    suite: String,
    baseline: String,
    scope: String,
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    records: Vec<CaseRecord>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Passed,
    Failed,
    Skipped,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct CaseRecord {
    case_id: String,
    #[serde(default)]
    source_case_id: Option<String>,
    #[serde(default)]
    variant_attributes: Option<BTreeMap<String, String>>,
    #[serde(default)]
    oracle_path: Option<String>,
    name: String,
    oracle_exists: bool,
    status: Status,
    failure_bucket: Option<String>,
}

#[derive(Debug)]
struct Args {
    base: PathBuf,
    candidate: PathBuf,
    json: bool,
    require_gain: bool,
    allow_skip_resolutions: bool,
}

#[derive(Debug, Serialize)]
struct Comparison {
    compatible: bool,
    regression: bool,
    require_gain: bool,
    allow_skip_resolutions: bool,
    required_gain_missing: bool,
    gains: Vec<Transition>,
    skip_resolutions: Vec<Transition>,
    losses: Vec<Transition>,
    added_cases: Vec<CaseDescription>,
    removed_cases: Vec<CaseDescription>,
    oracle_changes: Vec<OracleChange>,
    name_changes: Vec<NameChange>,
    base_skipped: usize,
    candidate_skipped: usize,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Transition {
    case_id: String,
    name: String,
    from: Status,
    to: Status,
}

#[derive(Debug, Serialize)]
struct CaseDescription {
    case_id: String,
    name: String,
    oracle_exists: bool,
    status: Status,
}

#[derive(Debug, Serialize)]
struct OracleChange {
    case_id: String,
    name: String,
    from: bool,
    to: bool,
}

#[derive(Debug, Serialize)]
struct NameChange {
    case_id: String,
    from: String,
    to: String,
}

impl Comparison {
    fn failed(&self) -> bool {
        self.regression || self.required_gain_missing
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(failed) if failed => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("baseline-compare: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let args = parse_args(env::args().skip(1))?;
    let base = read_manifest(&args.base)?;
    let candidate = read_manifest(&args.candidate)?;
    let comparison = compare(
        &base,
        &candidate,
        CompareOptions {
            require_gain: args.require_gain,
            allow_skip_resolutions: args.allow_skip_resolutions,
        },
    );

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&comparison)
                .map_err(|error| format!("could not serialize comparison: {error}"))?
        );
    } else {
        print_text(&comparison);
    }

    Ok(comparison.failed())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut base = None;
    let mut candidate = None;
    let mut json = false;
    let mut require_gain = false;
    let mut allow_skip_resolutions = false;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--base" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--base requires a path".to_string())?;
                if base.replace(PathBuf::from(value)).is_some() {
                    return Err("--base may only be specified once".to_string());
                }
            }
            "--candidate" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--candidate requires a path".to_string())?;
                if candidate.replace(PathBuf::from(value)).is_some() {
                    return Err("--candidate may only be specified once".to_string());
                }
            }
            "--json" => json = true,
            "--require-gain" => require_gain = true,
            "--allow-skip-resolutions" => allow_skip_resolutions = true,
            "-h" | "--help" => {
                return Err(
                    "usage: baseline-compare --base PATH --candidate PATH [--json] [--require-gain] [--allow-skip-resolutions]"
                        .to_string(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Args {
        base: base.ok_or_else(|| "missing required --base PATH".to_string())?,
        candidate: candidate.ok_or_else(|| "missing required --candidate PATH".to_string())?,
        json,
        require_gain,
        allow_skip_resolutions,
    })
}

fn read_manifest(path: &PathBuf) -> Result<Manifest, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("could not parse {}: {error}", path.display()))
}

#[derive(Clone, Copy, Debug, Default)]
struct CompareOptions {
    require_gain: bool,
    allow_skip_resolutions: bool,
}

fn compare(base: &Manifest, candidate: &Manifest, options: CompareOptions) -> Comparison {
    let mut errors = Vec::new();
    let mut compatible = true;

    if !SUPPORTED_SCHEMA_VERSIONS.contains(&base.schema_version) {
        compatible = false;
        errors.push(format!(
            "unsupported base schema version {}; expected one of {:?}",
            base.schema_version, SUPPORTED_SCHEMA_VERSIONS
        ));
    }
    if !SUPPORTED_SCHEMA_VERSIONS.contains(&candidate.schema_version) {
        compatible = false;
        errors.push(format!(
            "unsupported candidate schema version {}; expected one of {:?}",
            candidate.schema_version, SUPPORTED_SCHEMA_VERSIONS
        ));
    }
    if base.schema_version != candidate.schema_version {
        compatible = false;
        errors.push(format!(
            "schema mismatch: base={} candidate={}",
            base.schema_version, candidate.schema_version
        ));
    }
    if base.suite != candidate.suite {
        compatible = false;
        errors.push(format!(
            "suite mismatch: base={} candidate={}",
            base.suite, candidate.suite
        ));
    }
    if base.baseline != candidate.baseline {
        compatible = false;
        errors.push(format!(
            "baseline mismatch: base={} candidate={}",
            base.baseline, candidate.baseline
        ));
    }
    if base.scope != candidate.scope {
        compatible = false;
        errors.push(format!(
            "scope mismatch: base={} candidate={}",
            base.scope, candidate.scope
        ));
    }

    for (label, manifest) in [("base", base), ("candidate", candidate)] {
        if !matches!(
            manifest.suite.as_str(),
            "compiler" | "conformance" | "fourslash" | "project"
        ) {
            compatible = false;
            errors.push(format!("unsupported {label} suite: {}", manifest.suite));
        }
        let supported_baseline = match manifest.schema_version {
            1 => matches!(
                manifest.baseline.as_str(),
                "js" | "errors" | "symbols" | "types"
            ),
            2 => matches!(manifest.baseline.as_str(), "js" | "declarations"),
            _ => false,
        };
        if !supported_baseline {
            compatible = false;
            errors.push(format!(
                "unsupported {label} schema-v{} baseline: {}",
                manifest.schema_version, manifest.baseline
            ));
        }
        if !matches!(manifest.scope.as_str(), "all" | "js-skips") {
            compatible = false;
            errors.push(format!("unsupported {label} scope: {}", manifest.scope));
        }
        if manifest.baseline == "js" && manifest.scope == "js-skips" {
            compatible = false;
            errors.push(format!("invalid {label} JS/js-skips matrix"));
        }
        if manifest.schema_version == 2 && manifest.scope != "all" {
            compatible = false;
            errors.push(format!(
                "invalid {label} schema-v2 scope: {} (expected all)",
                manifest.scope
            ));
        }
    }

    let errors_before_record_validation = errors.len();
    validate_manifest("base", base, &mut errors);
    validate_manifest("candidate", candidate, &mut errors);

    let base_cases = unique_cases("base", &base.records, &mut errors);
    let candidate_cases = unique_cases("candidate", &candidate.records, &mut errors);
    if errors.len() > errors_before_record_validation {
        compatible = false;
    }
    // Transition lists are evidence, not diagnostics. Once either manifest is
    // structurally invalid, no record from it may be reported as a gain or an
    // accepted skip resolution (or satisfy --require-gain).
    let transition_evidence_valid = compatible;
    let base_ids: BTreeSet<_> = base_cases.keys().copied().collect();
    let candidate_ids: BTreeSet<_> = candidate_cases.keys().copied().collect();

    let added_cases = candidate_ids
        .difference(&base_ids)
        .map(|id| describe(candidate_cases[id]))
        .collect::<Vec<_>>();
    let removed_cases = base_ids
        .difference(&candidate_ids)
        .map(|id| describe(base_cases[id]))
        .collect::<Vec<_>>();

    let mut gains = Vec::new();
    let mut skip_resolutions = Vec::new();
    let mut losses = Vec::new();
    let mut oracle_changes = Vec::new();
    let mut name_changes = Vec::new();
    let mut disallowed_oracle_changes = 0usize;
    let mut provenance_changes = 0usize;

    for id in base_ids.intersection(&candidate_ids) {
        let before = base_cases[id];
        let after = candidate_cases[id];

        if before.name != after.name {
            name_changes.push(NameChange {
                case_id: (*id).to_string(),
                from: before.name.clone(),
                to: after.name.clone(),
            });
        }
        if before.oracle_exists != after.oracle_exists {
            oracle_changes.push(OracleChange {
                case_id: (*id).to_string(),
                name: after.name.clone(),
                from: before.oracle_exists,
                to: after.oracle_exists,
            });
        }
        let skip_resolution = transition_evidence_valid
            && is_monotonic_skip_resolution(before, after, options.allow_skip_resolutions);
        let same_variant_identity = base.schema_version != 2
            || (before.source_case_id == after.source_case_id
                && before.variant_attributes == after.variant_attributes);
        let same_immutable_provenance = same_variant_identity
            && (base.schema_version != 2
                || before.oracle_path == after.oracle_path
                || skip_resolution);
        if base.schema_version == 2 {
            if !same_variant_identity {
                provenance_changes += 1;
                errors.push(format!("variant identity changed for case {id}"));
            }
            if before.oracle_path != after.oracle_path && !skip_resolution {
                provenance_changes += 1;
                errors.push(format!("oracle path changed for case {id}"));
            }
        }
        if before.oracle_exists != after.oracle_exists && !skip_resolution {
            disallowed_oracle_changes += 1;
        }
        if skip_resolution {
            skip_resolutions.push(transition(before, after));
        }
        if before.status == Status::Passed
            && matches!(after.status, Status::Failed | Status::Skipped)
        {
            losses.push(transition(before, after));
        }
        if transition_evidence_valid
            && before.name == after.name
            && before.status == Status::Failed
            && after.status == Status::Passed
            && before.oracle_exists
            && after.oracle_exists
            && same_immutable_provenance
        {
            gains.push(transition(before, after));
        }
        if skip_resolution && after.status == Status::Passed {
            gains.push(transition(before, after));
        }
    }

    let base_skipped = base
        .records
        .iter()
        .filter(|record| record.status == Status::Skipped)
        .count();
    let candidate_skipped = candidate
        .records
        .iter()
        .filter(|record| record.status == Status::Skipped)
        .count();

    if !added_cases.is_empty() {
        errors.push(format!("{} discovered case(s) added", added_cases.len()));
    }
    if !removed_cases.is_empty() {
        errors.push(format!(
            "{} discovered case(s) removed",
            removed_cases.len()
        ));
    }
    if disallowed_oracle_changes != 0 {
        errors.push(format!(
            "{disallowed_oracle_changes} oracle existence change(s)"
        ));
    }
    if provenance_changes != 0 {
        errors.push(format!(
            "{provenance_changes} immutable v2 provenance change(s)"
        ));
    }
    if !name_changes.is_empty() {
        errors.push(format!("{} case name change(s)", name_changes.len()));
    }
    if !losses.is_empty() {
        errors.push(format!("{} passed case regression(s)", losses.len()));
    }
    if candidate_skipped > base_skipped {
        errors.push(format!(
            "skipped count increased from {base_skipped} to {candidate_skipped}"
        ));
    }

    let regression = !errors.is_empty();
    let required_gain_missing = options.require_gain && gains.is_empty();
    Comparison {
        compatible,
        regression,
        require_gain: options.require_gain,
        allow_skip_resolutions: options.allow_skip_resolutions,
        required_gain_missing,
        gains,
        skip_resolutions,
        losses,
        added_cases,
        removed_cases,
        oracle_changes,
        name_changes,
        base_skipped,
        candidate_skipped,
        errors,
    }
}

fn validate_manifest(label: &str, manifest: &Manifest, errors: &mut Vec<String>) {
    let passed = manifest
        .records
        .iter()
        .filter(|record| record.status == Status::Passed)
        .count();
    let failed = manifest
        .records
        .iter()
        .filter(|record| record.status == Status::Failed)
        .count();
    let skipped = manifest
        .records
        .iter()
        .filter(|record| record.status == Status::Skipped)
        .count();
    if manifest.total != manifest.records.len()
        || manifest.passed != passed
        || manifest.failed != failed
        || manifest.skipped != skipped
        || manifest.passed + manifest.failed + manifest.skipped != manifest.total
    {
        errors.push(format!(
            "{label} summary inconsistent: totals=({}, {}, {}, {}) records=({}, {}, {}, {})",
            manifest.total,
            manifest.passed,
            manifest.failed,
            manifest.skipped,
            manifest.records.len(),
            passed,
            failed,
            skipped
        ));
    }

    for record in &manifest.records {
        if record.case_id.is_empty() || record.name.is_empty() {
            errors.push(format!(
                "{label} manifest contains an empty case id or name"
            ));
        }
        if record.oracle_exists == (record.status == Status::Skipped) {
            errors.push(format!(
                "{label} case {} has inconsistent oracle/status: oracle={} status={}",
                record.case_id,
                record.oracle_exists,
                record.status.as_str()
            ));
        }
        if (record.status == Status::Failed) != record.failure_bucket.is_some() {
            errors.push(format!(
                "{label} case {} has inconsistent failure_bucket for status {}",
                record.case_id,
                record.status.as_str()
            ));
        }
        match manifest.schema_version {
            1 if record.source_case_id.is_some()
                || record.variant_attributes.is_some()
                || record.oracle_path.is_some() =>
            {
                errors.push(format!(
                    "{label} v1 case {} contains v2 variant provenance",
                    record.case_id
                ));
            }
            2 if record.source_case_id.is_none()
                || record.variant_attributes.is_none()
                || (record.oracle_exists != record.oracle_path.is_some()) =>
            {
                errors.push(format!(
                    "{label} v2 case {} has incomplete variant provenance",
                    record.case_id
                ));
            }
            2 => match canonical_variant_case_id(
                record.source_case_id.as_deref().expect("checked above"),
                record.variant_attributes.as_ref().expect("checked above"),
            ) {
                Ok(canonical_id) if record.case_id == canonical_id => {}
                Ok(canonical_id) => errors.push(format!(
                    "{label} v2 case {} is not canonical; expected {canonical_id}",
                    record.case_id
                )),
                Err(error) => errors.push(format!(
                    "{label} v2 case {} has non-canonical provenance: {error}",
                    record.case_id
                )),
            },
            _ => {}
        }
        if manifest.schema_version == 2 {
            if let Some(path) = record.oracle_path.as_deref() {
                if let Err(error) = validate_normalized_oracle_path(path) {
                    errors.push(format!(
                        "{label} v2 case {} has invalid oracle_path {path:?}: {error}",
                        record.case_id
                    ));
                }
            }
        }
    }
}

fn unique_cases<'a>(
    label: &str,
    records: &'a [CaseRecord],
    errors: &mut Vec<String>,
) -> BTreeMap<&'a str, &'a CaseRecord> {
    let mut cases = BTreeMap::new();
    for record in records {
        if cases.insert(record.case_id.as_str(), record).is_some() {
            errors.push(format!(
                "{label} manifest contains duplicate case_id {}",
                record.case_id
            ));
        }
    }
    cases
}

/// The sole oracle mutation permitted by the opt-in policy.
///
/// Keep this predicate self-contained and exact: malformed records must not be
/// reported as permitted resolutions even though manifest validation will also
/// reject the comparison.
fn is_monotonic_skip_resolution(before: &CaseRecord, after: &CaseRecord, enabled: bool) -> bool {
    let provenance_allows_resolution = match (
        before.source_case_id.as_ref(),
        before.variant_attributes.as_ref(),
        before.oracle_path.as_ref(),
        after.source_case_id.as_ref(),
        after.variant_attributes.as_ref(),
        after.oracle_path.as_ref(),
    ) {
        // Schema v1 has no provenance fields.
        (None, None, None, None, None, None) => true,
        // Schema v2 may add only the previously absent oracle path.
        (
            Some(before_source),
            Some(before_attributes),
            None,
            Some(after_source),
            Some(after_attributes),
            Some(after_path),
        ) => {
            before_source == after_source
                && before_attributes == after_attributes
                && validate_normalized_oracle_path(after_path).is_ok()
        }
        _ => false,
    };
    enabled
        && before.case_id == after.case_id
        && before.name == after.name
        && !before.oracle_exists
        && before.status == Status::Skipped
        && before.failure_bucket.is_none()
        && provenance_allows_resolution
        && after.oracle_exists
        && match after.status {
            Status::Passed => after.failure_bucket.is_none(),
            Status::Failed => after.failure_bucket.is_some(),
            Status::Skipped => false,
        }
}

fn transition(before: &CaseRecord, after: &CaseRecord) -> Transition {
    Transition {
        case_id: before.case_id.clone(),
        name: after.name.clone(),
        from: before.status,
        to: after.status,
    }
}

fn describe(record: &CaseRecord) -> CaseDescription {
    CaseDescription {
        case_id: record.case_id.clone(),
        name: record.name.clone(),
        oracle_exists: record.oracle_exists,
        status: record.status,
    }
}

fn print_text(comparison: &Comparison) {
    println!(
        "baseline comparison: {} gain(s), {} skip resolution(s), {} loss(es)",
        comparison.gains.len(),
        comparison.skip_resolutions.len(),
        comparison.losses.len()
    );
    for gain in &comparison.gains {
        println!(
            "GAIN\t{}\t{}\t{} -> {}",
            gain.case_id,
            gain.name,
            gain.from.as_str(),
            gain.to.as_str()
        );
    }
    for loss in &comparison.losses {
        println!(
            "LOSS\t{}\t{}\t{} -> {}",
            loss.case_id,
            loss.name,
            loss.from.as_str(),
            loss.to.as_str()
        );
    }
    for resolution in &comparison.skip_resolutions {
        println!(
            "RESOLVED\t{}\t{}\t{} -> {}",
            resolution.case_id,
            resolution.name,
            resolution.from.as_str(),
            resolution.to.as_str()
        );
    }
    for case in &comparison.added_cases {
        println!("ADDED\t{}\t{}", case.case_id, case.name);
    }
    for case in &comparison.removed_cases {
        println!("REMOVED\t{}\t{}", case.case_id, case.name);
    }
    for change in &comparison.oracle_changes {
        println!(
            "ORACLE\t{}\t{}\t{} -> {}",
            change.case_id, change.name, change.from, change.to
        );
    }
    for change in &comparison.name_changes {
        println!(
            "RENAMED\t{}\t{} -> {}",
            change.case_id, change.from, change.to
        );
    }
    for error in &comparison.errors {
        eprintln!("ERROR\t{error}");
    }
    if comparison.required_gain_missing {
        eprintln!(
            "ERROR\t--require-gain requested but no failed -> passed or skipped -> passed gain was found"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn manifest(records: Value) -> Manifest {
        let records = records.as_array().unwrap();
        let passed = records
            .iter()
            .filter(|record| record["status"] == "passed")
            .count();
        let failed = records
            .iter()
            .filter(|record| record["status"] == "failed")
            .count();
        let skipped = records.len() - passed - failed;
        serde_json::from_value(json!({
            "schema_version": 1,
            "suite": "compiler",
            "baseline": "errors",
            "scope": "all",
            "total": records.len(),
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
            "records": records
        }))
        .unwrap()
    }

    fn record(id: &str, status: &str, oracle_exists: bool) -> Value {
        json!({
            "case_id": id,
            "name": format!("case-{id}"),
            "oracle_exists": oracle_exists,
            "status": status,
            "failure_bucket": if status == "failed" { Some("CODE-DIFF") } else { None }
        })
    }

    fn options(require_gain: bool, allow_skip_resolutions: bool) -> CompareOptions {
        CompareOptions {
            require_gain,
            allow_skip_resolutions,
        }
    }

    fn as_v2(mut manifest: Manifest) -> Manifest {
        manifest.schema_version = 2;
        manifest.baseline = "js".to_string();
        for record in &mut manifest.records {
            record.source_case_id = Some(record.case_id.clone());
            record.variant_attributes = Some(BTreeMap::new());
            record.oracle_path = record
                .oracle_exists
                .then(|| format!("tests/baselines/reference/{}.js", record.name));
        }
        manifest
    }

    fn v2_variant(status: &str, oracle_exists: bool) -> Manifest {
        let source_case_id = "tests/cases/compiler/a.ts";
        let attributes = BTreeMap::from([("target".to_string(), "es5".to_string())]);
        let case_id = canonical_variant_case_id(source_case_id, &attributes).unwrap();
        let mut manifest = manifest(json!([record(&case_id, status, oracle_exists)]));
        manifest.schema_version = 2;
        manifest.baseline = "js".to_string();
        manifest.records[0].name = "a (target=es5)".to_string();
        manifest.records[0].source_case_id = Some(source_case_id.to_string());
        manifest.records[0].variant_attributes = Some(attributes);
        manifest.records[0].oracle_path =
            oracle_exists.then(|| "tests/baselines/reference/a(target=es5).js".to_string());
        manifest
    }

    #[test]
    fn accepts_matching_v2_manifests_and_rejects_mixed_schemas() {
        let v1 = manifest(json!([record("a", "passed", true)]));
        let v2 = as_v2(manifest(json!([record("a", "passed", true)])));
        let matching = compare(&v2, &v2, options(false, false));
        assert!(!matching.failed(), "{matching:?}");

        let mixed = compare(&v1, &v2, options(false, false));
        assert!(mixed.failed());
        assert!(!mixed.compatible);
        assert!(mixed
            .errors
            .iter()
            .any(|error| error.contains("schema mismatch")));
    }

    #[test]
    fn rejects_incomplete_v2_provenance() {
        let mut invalid = as_v2(manifest(json!([record("a", "passed", true)])));
        invalid.records[0].variant_attributes = None;
        let result = compare(&invalid, &invalid, options(false, false));
        assert!(result.failed());
        assert!(!result.compatible);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("incomplete variant provenance")));
    }

    #[test]
    fn malformed_manifest_cannot_report_or_satisfy_a_gain() {
        let base = manifest(json!([record("a", "failed", true)]));
        let mut candidate = manifest(json!([record("a", "passed", true)]));
        candidate.failed = 1;
        candidate.passed = 0;

        let result = compare(&base, &candidate, options(true, false));
        assert!(result.failed());
        assert!(!result.compatible);
        assert!(result.gains.is_empty());
        assert!(result.required_gain_missing);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("summary inconsistent")));
    }

    #[test]
    fn malformed_record_cannot_report_or_satisfy_a_gain() {
        let base = manifest(json!([record("a", "failed", true)]));
        let mut candidate = manifest(json!([record("a", "passed", true)]));
        candidate.records[0].failure_bucket = Some("fabricated".to_string());

        let result = compare(&base, &candidate, options(true, false));
        assert!(result.failed());
        assert!(!result.compatible);
        assert!(result.gains.is_empty());
        assert!(result.required_gain_missing);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("inconsistent failure_bucket")));
    }

    #[test]
    fn empty_or_non_normalized_oracle_path_cannot_resolve_a_skip() {
        for path in [
            "",
            "./tests/baselines/reference/a(target=es5).js",
            "tests/baselines/../reference/a(target=es5).js",
        ] {
            let base = v2_variant("skipped", false);
            let mut candidate = v2_variant("passed", true);
            candidate.records[0].oracle_path = Some(path.to_string());

            let result = compare(&base, &candidate, options(true, true));
            assert!(
                result.failed(),
                "unexpectedly accepted {path:?}: {result:?}"
            );
            assert!(!result.compatible);
            assert!(result.gains.is_empty());
            assert!(result.skip_resolutions.is_empty());
            assert!(result.required_gain_missing);
            assert!(result
                .errors
                .iter()
                .any(|error| error.contains("invalid oracle_path")));
        }
    }

    #[test]
    fn rejects_false_gain_with_mutated_v2_identity_and_oracle() {
        let base = v2_variant("failed", true);
        let mut candidate = v2_variant("passed", true);
        candidate.records[0].source_case_id = Some("tests/cases/compiler/other.ts".to_string());
        candidate.records[0].variant_attributes = Some(BTreeMap::from([(
            "target".to_string(),
            "es2022".to_string(),
        )]));
        candidate.records[0].oracle_path =
            Some("tests/baselines/reference/different.js".to_string());

        let result = compare(&base, &candidate, options(true, false));
        assert!(result.failed(), "mutated provenance must reject the gain");
        assert!(result.regression);
        assert!(
            result.gains.is_empty(),
            "invalid identity must not earn a gain"
        );
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("variant identity changed")));
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("oracle path changed")));
    }

    #[test]
    fn rejects_oracle_path_change_for_the_same_v2_identity() {
        let base = v2_variant("failed", true);
        let mut candidate = v2_variant("passed", true);
        candidate.records[0].oracle_path =
            Some("tests/baselines/reference/other(target=es5).js".to_string());

        let result = compare(&base, &candidate, options(true, false));
        assert!(result.failed());
        assert!(result.gains.is_empty());
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("oracle path changed")));
    }

    #[test]
    fn allows_only_same_identity_v2_skip_resolution() {
        let base = v2_variant("skipped", false);
        let candidate = v2_variant("passed", true);
        let allowed = compare(&base, &candidate, options(true, true));
        assert!(!allowed.failed(), "{allowed:?}");
        assert_eq!(allowed.gains.len(), 1);
        assert_eq!(allowed.skip_resolutions.len(), 1);

        let mut changed = v2_variant("passed", true);
        changed.records[0].source_case_id = Some("tests/cases/compiler/other.ts".to_string());
        let rejected = compare(&base, &changed, options(true, true));
        assert!(rejected.failed());
        assert!(rejected.gains.is_empty());
        assert!(rejected.skip_resolutions.is_empty());
    }

    #[test]
    fn enforces_schema_specific_matrix_contracts() {
        let mut v1_declarations = manifest(json!([record("a", "passed", true)]));
        v1_declarations.baseline = "declarations".to_string();
        let v1_result = compare(&v1_declarations, &v1_declarations, options(false, false));
        assert!(v1_result.failed());
        assert!(!v1_result.compatible);

        let mut v2_errors = as_v2(manifest(json!([record("a", "passed", true)])));
        v2_errors.baseline = "errors".to_string();
        let v2_baseline_result = compare(&v2_errors, &v2_errors, options(false, false));
        assert!(v2_baseline_result.failed());
        assert!(!v2_baseline_result.compatible);

        let mut v2_partial = as_v2(manifest(json!([record("a", "passed", true)])));
        v2_partial.scope = "js-skips".to_string();
        let v2_scope_result = compare(&v2_partial, &v2_partial, options(false, false));
        assert!(v2_scope_result.failed());
        assert!(!v2_scope_result.compatible);
    }

    #[test]
    fn exhaustively_checks_every_valid_status_transition_in_both_modes() {
        let states = [("skipped", false), ("failed", true), ("passed", true)];

        for allow_skip_resolutions in [false, true] {
            for (from, from_oracle) in states {
                for (to, to_oracle) in states {
                    let base = manifest(json!([record("a", from, from_oracle)]));
                    let candidate = manifest(json!([record("a", to, to_oracle)]));
                    let result = compare(&base, &candidate, options(false, allow_skip_resolutions));

                    let is_skip_resolution = allow_skip_resolutions
                        && from == "skipped"
                        && matches!(to, "failed" | "passed");
                    let expected_regression = match (from, to) {
                        ("skipped", "failed" | "passed") => !allow_skip_resolutions,
                        ("failed", "skipped") | ("passed", "skipped") | ("passed", "failed") => {
                            true
                        }
                        _ => false,
                    };
                    let expected_gain = (from == "failed" && to == "passed")
                        || (is_skip_resolution && to == "passed");

                    assert_eq!(
                        result.regression, expected_regression,
                        "unexpected regression result for {from} -> {to}, allow={allow_skip_resolutions}: {result:?}"
                    );
                    assert_eq!(
                        result.gains.len(),
                        usize::from(expected_gain),
                        "unexpected gains for {from} -> {to}, allow={allow_skip_resolutions}: {result:?}"
                    );
                    assert_eq!(
                        result.skip_resolutions.len(),
                        usize::from(is_skip_resolution),
                        "unexpected resolutions for {from} -> {to}, allow={allow_skip_resolutions}: {result:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn accepts_exact_failed_to_passed_gain() {
        let base = manifest(json!([record("a", "failed", true)]));
        let candidate = manifest(json!([record("a", "passed", true)]));
        let result = compare(&base, &candidate, options(true, false));

        assert!(!result.failed());
        assert_eq!(result.gains.len(), 1);
        assert!(result.losses.is_empty());
    }

    #[test]
    fn rejects_pass_loss_and_increased_skip() {
        let base = manifest(json!([record("a", "passed", true)]));
        let candidate = manifest(json!([record("a", "skipped", true)]));
        let result = compare(&base, &candidate, options(false, false));

        assert!(result.failed());
        assert_eq!(result.losses.len(), 1);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("skipped count increased")));
    }

    #[test]
    fn rejects_discovery_and_oracle_changes() {
        let base = manifest(json!([
            record("a", "skipped", false),
            record("removed", "failed", true)
        ]));
        let candidate = manifest(json!([
            record("a", "failed", true),
            record("added", "failed", true)
        ]));
        let result = compare(&base, &candidate, options(false, true));

        assert!(result.failed());
        assert_eq!(result.added_cases.len(), 1);
        assert_eq!(result.removed_cases.len(), 1);
        assert_eq!(result.oracle_changes.len(), 1);
    }

    #[test]
    fn skip_to_passed_is_an_opt_in_required_gain() {
        let base = manifest(json!([record("a", "skipped", false)]));
        let candidate = manifest(json!([record("a", "passed", true)]));

        let strict = compare(&base, &candidate, options(true, false));
        assert!(strict.failed());
        assert!(strict.regression);
        assert!(strict.required_gain_missing);
        assert!(strict.gains.is_empty());

        let allowed = compare(&base, &candidate, options(true, true));
        assert!(!allowed.failed());
        assert_eq!(allowed.gains.len(), 1);
        assert_eq!(allowed.skip_resolutions.len(), 1);
    }

    #[test]
    fn skip_to_failed_surfaces_coverage_without_becoming_a_gain_or_regression() {
        let base = manifest(json!([record("a", "skipped", false)]));
        let candidate = manifest(json!([record("a", "failed", true)]));

        let result = compare(&base, &candidate, options(false, true));
        assert!(!result.failed());
        assert!(result.gains.is_empty());
        assert_eq!(result.skip_resolutions.len(), 1);
        assert_eq!(result.oracle_changes.len(), 1);

        let require_gain = compare(&base, &candidate, options(true, true));
        assert!(require_gain.failed());
        assert!(!require_gain.regression);
        assert!(require_gain.required_gain_missing);
    }

    #[test]
    fn opt_in_does_not_hide_pass_loss_or_oracle_removal_when_skip_count_is_flat() {
        let base = manifest(json!([
            record("resolved", "skipped", false),
            record("lost", "passed", true),
            record("removed-oracle", "failed", true)
        ]));
        let candidate = manifest(json!([
            record("resolved", "passed", true),
            record("lost", "failed", true),
            record("removed-oracle", "skipped", false)
        ]));

        let result = compare(&base, &candidate, options(false, true));
        assert!(result.failed());
        assert_eq!(result.base_skipped, result.candidate_skipped);
        assert_eq!(result.skip_resolutions.len(), 1);
        assert_eq!(result.losses.len(), 1);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("oracle existence change")));
    }

    #[test]
    fn opt_in_does_not_allow_name_drift_during_skip_resolution() {
        let base = manifest(json!([record("a", "skipped", false)]));
        let mut candidate = manifest(json!([record("a", "passed", true)]));
        candidate.records[0].name = "renamed".to_string();

        let result = compare(&base, &candidate, options(false, true));
        assert!(result.failed());
        assert!(result.skip_resolutions.is_empty());
        assert_eq!(result.name_changes.len(), 1);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("oracle existence change")));
    }

    #[test]
    fn opt_in_does_not_classify_malformed_records_as_skip_resolutions() {
        let base = manifest(json!([record("a", "skipped", false)]));
        let mut candidate = manifest(json!([record("a", "failed", true)]));
        candidate.records[0].failure_bucket = None;

        let result = compare(&base, &candidate, options(false, true));
        assert!(result.failed());
        assert!(!result.compatible);
        assert!(result.skip_resolutions.is_empty());
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("inconsistent failure_bucket")));
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("oracle existence change")));
    }

    #[test]
    fn rejects_duplicate_ids_and_bad_summary() {
        let mut candidate = manifest(json!([
            record("same", "failed", true),
            record("same", "passed", true)
        ]));
        candidate.passed = 0;
        let base = manifest(json!([record("same", "failed", true)]));
        let result = compare(&base, &candidate, options(false, false));

        assert!(result.failed());
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("duplicate case_id")));
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("summary inconsistent")));
    }

    #[test]
    fn require_gain_rejects_unchanged_manifests() {
        let base = manifest(json!([record("a", "failed", true)]));
        let candidate = manifest(json!([record("a", "failed", true)]));
        let result = compare(&base, &candidate, options(true, false));

        assert!(result.failed());
        assert!(!result.regression);
        assert!(result.gains.is_empty());
    }

    #[test]
    fn parses_manual_cli_arguments() {
        let args = parse_args([
            "--base".into(),
            "base.json".into(),
            "--candidate".into(),
            "candidate.json".into(),
            "--json".into(),
            "--require-gain".into(),
            "--allow-skip-resolutions".into(),
        ])
        .unwrap();

        assert_eq!(args.base, PathBuf::from("base.json"));
        assert_eq!(args.candidate, PathBuf::from("candidate.json"));
        assert!(args.json);
        assert!(args.require_gain);
        assert!(args.allow_skip_resolutions);

        let defaults = parse_args([
            "--base".into(),
            "base.json".into(),
            "--candidate".into(),
            "candidate.json".into(),
        ])
        .unwrap();
        assert!(!defaults.allow_skip_resolutions);
    }

    #[test]
    fn rejects_incompatible_and_unsupported_schemas() {
        let base = manifest(json!([record("a", "failed", true)]));
        let mut candidate = manifest(json!([record("a", "failed", true)]));
        candidate.schema_version = 3;
        let result = compare(&base, &candidate, options(false, true));

        assert!(result.failed());
        assert!(!result.compatible);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("schema mismatch")));
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("unsupported candidate schema")));
    }

    #[test]
    fn rejects_unsupported_matrix_and_malformed_case_state() {
        let base = manifest(json!([record("a", "failed", true)]));
        let mut candidate = manifest(json!([record("a", "skipped", true)]));
        candidate.scope = "partial".to_string();
        let result = compare(&base, &candidate, options(false, true));

        assert!(result.failed());
        assert!(!result.compatible);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("unsupported candidate scope")));
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("inconsistent oracle/status")));
    }
}
