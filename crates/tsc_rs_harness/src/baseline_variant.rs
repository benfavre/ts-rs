use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use tsc_rs_ast::{JsxEmit, ModuleKind, ScriptTarget, TestCase};

use crate::{
    extract_dts_sections, fixup_multi_value_options, normalize_text, read_test_source,
    strip_dts_sections, BaselineKind, BaselineResult, BaselineRunner, Suite,
    BASELINE_CASE_STACK_SIZE,
};

/// One immutable baseline identity discovered before compiler execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineVariant {
    pub case_id: String,
    pub source_case_id: String,
    pub name: String,
    pub source_path: PathBuf,
    pub oracle_path: Option<PathBuf>,
    pub oracle_relative_path: Option<String>,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ExpandedBaselineResult {
    pub variant: BaselineVariant,
    pub result: BaselineResult,
}

#[derive(Default)]
struct OracleSet {
    exact: Option<PathBuf>,
    parameterized: Vec<(BTreeMap<String, String>, PathBuf)>,
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn relative_id(root: &Path, path: &Path) -> String {
    slash_path(path.strip_prefix(root).unwrap_or(path))
        .trim_start_matches("./")
        .to_string()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn variant_suffix(attributes: &BTreeMap<String, String>) -> String {
    attributes
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join(",")
}

/// Derive the stable expanded-manifest identity from canonical provenance.
///
/// Attribute parsing lowercases and trims both halves before this point. Keep
/// that normalization part of the manifest contract so a case ID cannot be
/// paired with a second spelling of the same variant identity.
pub fn canonical_variant_case_id(
    source_case_id: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<String, String> {
    if source_case_id.is_empty() {
        return Err("source case id must not be empty".to_string());
    }
    for (key, value) in attributes {
        if key.is_empty()
            || value.is_empty()
            || key != key.trim()
            || value != value.trim()
            || key != &key.to_ascii_lowercase()
            || value != &value.to_ascii_lowercase()
        {
            return Err(format!(
                "variant attributes must be non-empty, trimmed, and lowercase: {key:?}={value:?}"
            ));
        }
    }
    if attributes.is_empty() {
        Ok(source_case_id.to_string())
    } else {
        Ok(format!(
            "{source_case_id}::variant({})",
            variant_suffix(attributes)
        ))
    }
}

/// Validate a workspace-relative path stored as immutable manifest provenance.
///
/// Manifest paths use `/` separators and must already be lexically normalized;
/// consumers must never reinterpret an empty, absolute, or traversal-containing
/// spelling as evidence for a different oracle.
pub fn validate_normalized_oracle_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path must not be empty".to_string());
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err("path must be relative and use forward slashes".to_string());
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err("path must be lexically normalized".to_string());
    }
    if !path.starts_with("tests/baselines/reference/") {
        return Err("path must be inside tests/baselines/reference".to_string());
    }
    Ok(())
}

fn parse_attributes(raw: &str, file_name: &str) -> Result<BTreeMap<String, String>, String> {
    if raw.is_empty() {
        return Err(format!(
            "parameterized baseline has no attributes: {file_name}"
        ));
    }

    let mut attributes = BTreeMap::new();
    for attribute in raw.split(',') {
        let (key, value) = attribute
            .split_once('=')
            .ok_or_else(|| format!("malformed parameterized baseline: {file_name}"))?;
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        if key.is_empty() || value.is_empty() || attributes.insert(key, value).is_some() {
            return Err(format!(
                "malformed or duplicate parameterized attribute: {file_name}"
            ));
        }
    }
    Ok(attributes)
}

fn is_boolean_variant_axis(key: &str) -> bool {
    matches!(
        key,
        "alwaysstrict"
            | "strict"
            | "esmoduleinterop"
            | "usedefineforclassfields"
            | "isolatedmodules"
            | "isolateddeclarations"
            | "allowimportingtsextensions"
            | "exactoptionalpropertytypes"
            | "verbatimmodulesyntax"
            | "nolib"
            | "experimentaldecorators"
            | "strictnullchecks"
            | "noimplicitany"
            | "noemit"
            | "downleveliteration"
            | "preserveconstenums"
            | "nouncheckedindexedaccess"
            | "nopropertyaccessfromindexsignature"
            | "useunknownincatchvariables"
            | "resolvejsonmodule"
    )
}

fn wildcard_variant_values(key: &str) -> Option<&'static [&'static str]> {
    if is_boolean_variant_axis(key) {
        return Some(&["false", "true"]);
    }
    match key {
        "target" => Some(&[
            "es3", "es5", "es6", "es2016", "es2017", "es2018", "es2019", "es2020", "es2021",
            "es2022", "es2023", "es2024", "es2025", "esnext",
        ]),
        "module" => Some(&[
            "none", "commonjs", "amd", "umd", "system", "es6", "es2020", "es2022", "esnext",
            "node16", "node18", "node20", "nodenext", "preserve",
        ]),
        "jsx" => Some(&[
            "preserve",
            "react",
            "react-native",
            "react-jsx",
            "react-jsxdev",
        ]),
        "moduleresolution" => Some(&["classic", "node", "node16", "nodenext", "bundler"]),
        "moduledetection" => Some(&["legacy", "auto", "force"]),
        _ => None,
    }
}

fn concrete_variant_values(key: &str, raw: &str) -> Option<Vec<String>> {
    let wildcard_values = wildcard_variant_values(key)?;
    let is_matrix = raw.contains(',') || raw.split(',').any(|value| value.trim() == "*");
    if !is_matrix {
        return None;
    }

    let mut values = BTreeSet::new();
    let mut excluded = BTreeSet::new();
    for raw_value in raw.split(',') {
        let value = raw_value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_ascii_lowercase();
        if value == "*" {
            values.extend(wildcard_values.iter().map(|value| (*value).to_string()));
        } else if let Some(excluded_value) = value.strip_prefix('-') {
            excluded.insert(excluded_value.to_string());
        } else if !value.is_empty() {
            values.insert(value);
        }
    }
    values.retain(|value| !excluded.contains(value));
    Some(values.into_iter().collect())
}

fn error_directive_variants(source: &str) -> Vec<BTreeMap<String, String>> {
    let mut axes = BTreeMap::<String, Vec<String>>::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let Some(after_slashes) = trimmed.strip_prefix("//") else {
            continue;
        };
        let Some(directive) = after_slashes.trim_start().strip_prefix('@') else {
            continue;
        };
        let Some((key, raw_value)) = directive.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if let Some(values) = concrete_variant_values(&key, raw_value.trim()) {
            if !values.is_empty() {
                axes.insert(key, values);
            }
        }
    }

    let mut variants = vec![BTreeMap::new()];
    for (key, values) in axes {
        let mut expanded = Vec::with_capacity(variants.len() * values.len());
        for attributes in variants {
            for value in &values {
                let mut concrete = attributes.clone();
                concrete.insert(key.clone(), value.clone());
                expanded.push(concrete);
            }
        }
        variants = expanded;
    }
    if variants.len() == 1 && variants[0].is_empty() {
        Vec::new()
    } else {
        variants
    }
}

fn equivalent_variant_value(key: &str, left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    matches!(
        (key, left, right),
        ("target" | "module", "es6", "es2015")
            | ("target" | "module", "es2015", "es6")
            | ("moduleresolution", "node", "node10")
            | ("moduleresolution", "node10", "node")
    )
}

fn oracle_attributes_match(
    oracle: &BTreeMap<String, String>,
    concrete: &BTreeMap<String, String>,
) -> bool {
    oracle.iter().all(|(key, expected)| {
        concrete
            .get(key)
            .is_some_and(|actual| equivalent_variant_value(key, expected, actual))
    })
}

impl BaselineRunner {
    /// Build a complete oracle inventory for a suite without compiling a
    /// candidate. Declarations use the compound JavaScript oracle inventory;
    /// errors use their own `.errors.txt` inventory.
    pub fn discover_baseline_variants(
        &self,
        suite: Suite,
        kind: BaselineKind,
    ) -> Result<Vec<BaselineVariant>, String> {
        let extension = match kind {
            BaselineKind::Js | BaselineKind::Declarations => ".js",
            BaselineKind::Errors => ".errors.txt",
            BaselineKind::Symbols | BaselineKind::Types => {
                return Err(format!(
                    "expanded variants are not implemented for {} baselines",
                    kind.as_str()
                ));
            }
        };
        let source_paths = self
            .discover_cases(suite)
            .map_err(|error| error.to_string())?;
        let mut sources_by_stem = HashMap::<String, PathBuf>::new();
        for source_path in &source_paths {
            let stem = source_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| format!("source has a non-Unicode stem: {}", source_path.display()))?
                .to_string();
            if let Some(previous) = sources_by_stem.insert(stem.clone(), source_path.clone()) {
                return Err(format!(
                    "ambiguous source stem {stem}: {} and {}",
                    previous.display(),
                    source_path.display()
                ));
            }
        }

        let baseline_dir = self.workspace_root.join("tests/baselines/reference");
        let entries = std::fs::read_dir(&baseline_dir).map_err(|error| {
            format!(
                "could not inspect baseline directory {}: {error}",
                baseline_dir.display()
            )
        })?;
        let mut oracles = HashMap::<String, OracleSet>::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "could not inspect an entry in {}: {error}",
                    baseline_dir.display()
                )
            })?;
            if !entry
                .file_type()
                .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?
                .is_file()
            {
                continue;
            }
            let file_name = entry
                .file_name()
                .to_str()
                .ok_or_else(|| format!("non-Unicode baseline name: {}", entry.path().display()))?
                .to_string();
            let Some(without_extension) = file_name.strip_suffix(extension) else {
                continue;
            };

            if let Some(open) = without_extension.rfind('(') {
                let stem = &without_extension[..open];
                let Some(raw_attributes) = without_extension[open + 1..].strip_suffix(')') else {
                    continue;
                };
                if !sources_by_stem.contains_key(stem) {
                    continue;
                }
                let attributes = parse_attributes(raw_attributes, &file_name)?;
                oracles
                    .entry(stem.to_string())
                    .or_default()
                    .parameterized
                    .push((attributes, entry.path()));
            } else if sources_by_stem.contains_key(without_extension) {
                let slot = &mut oracles
                    .entry(without_extension.to_string())
                    .or_default()
                    .exact;
                if slot.replace(entry.path()).is_some() {
                    return Err(format!(
                        "duplicate exact {} oracle for {without_extension}",
                        kind.as_str()
                    ));
                }
            }
        }

        let mut variants = Vec::new();
        let mut case_ids = BTreeSet::new();
        for source_path in source_paths {
            let stem = source_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("source stems were validated");
            let source_case_id = relative_id(&self.workspace_root, &source_path);
            let mut oracle_set = oracles.remove(stem).unwrap_or_default();
            oracle_set.parameterized.sort_by(|left, right| {
                variant_suffix(&left.0)
                    .cmp(&variant_suffix(&right.0))
                    .then_with(|| left.1.cmp(&right.1))
            });

            let concrete_error_variants = if kind == BaselineKind::Errors {
                let source = read_test_source(&source_path).map_err(|error| {
                    format!(
                        "failed to read source while expanding variants {}: {error}",
                        source_path.display()
                    )
                })?;
                error_directive_variants(&source)
            } else {
                Vec::new()
            };
            let source_variant_start = variants.len();
            let mut used_parameterized_oracles = BTreeSet::new();

            if !concrete_error_variants.is_empty() {
                for attributes in concrete_error_variants {
                    let selected = oracle_set
                        .parameterized
                        .iter()
                        .filter(|(oracle_attributes, _)| {
                            oracle_attributes_match(oracle_attributes, &attributes)
                        })
                        .max_by(|left, right| {
                            let exact_matches = |oracle_attributes: &BTreeMap<String, String>| {
                                oracle_attributes
                                    .iter()
                                    .filter(|(key, expected)| {
                                        attributes
                                            .get(*key)
                                            .is_some_and(|actual| actual == *expected)
                                    })
                                    .count()
                            };
                            left.0
                                .len()
                                .cmp(&right.0.len())
                                .then_with(|| exact_matches(&left.0).cmp(&exact_matches(&right.0)))
                                .then_with(|| right.1.cmp(&left.1))
                        });
                    let oracle_path = selected
                        .map(|(_, path)| {
                            used_parameterized_oracles.insert(path.clone());
                            path.clone()
                        })
                        .or_else(|| oracle_set.exact.clone());
                    let variant = BaselineVariant {
                        case_id: canonical_variant_case_id(&source_case_id, &attributes)
                            .expect("expanded directive attributes were canonicalized"),
                        source_case_id: source_case_id.clone(),
                        name: format!("{stem} ({})", variant_suffix(&attributes)),
                        source_path: source_path.clone(),
                        oracle_relative_path: oracle_path
                            .as_deref()
                            .map(|path| relative_id(&self.workspace_root, path)),
                        oracle_path,
                        attributes,
                    };
                    if !case_ids.insert(variant.case_id.clone()) {
                        return Err(format!("duplicate expanded case id: {}", variant.case_id));
                    }
                    variants.push(variant);
                }
            } else if let Some(oracle_path) = oracle_set.exact.clone() {
                let variant = BaselineVariant {
                    case_id: source_case_id.clone(),
                    source_case_id: source_case_id.clone(),
                    name: stem.to_string(),
                    source_path: source_path.clone(),
                    oracle_relative_path: Some(relative_id(&self.workspace_root, &oracle_path)),
                    oracle_path: Some(oracle_path),
                    attributes: BTreeMap::new(),
                };
                if !case_ids.insert(variant.case_id.clone()) {
                    return Err(format!("duplicate expanded case id: {}", variant.case_id));
                }
                variants.push(variant);
            }

            for (attributes, oracle_path) in oracle_set.parameterized {
                if used_parameterized_oracles.contains(&oracle_path) {
                    continue;
                }
                let suffix = variant_suffix(&attributes);
                let variant = BaselineVariant {
                    case_id: canonical_variant_case_id(&source_case_id, &attributes)
                        .expect("discovered attributes were canonicalized"),
                    source_case_id: source_case_id.clone(),
                    name: format!("{stem} ({suffix})"),
                    source_path: source_path.clone(),
                    oracle_relative_path: Some(relative_id(&self.workspace_root, &oracle_path)),
                    oracle_path: Some(oracle_path),
                    attributes,
                };
                if !case_ids.insert(variant.case_id.clone()) {
                    return Err(format!("duplicate expanded case id: {}", variant.case_id));
                }
                variants.push(variant);
            }

            if variants.len() == source_variant_start {
                let variant = BaselineVariant {
                    case_id: source_case_id.clone(),
                    source_case_id,
                    name: stem.to_string(),
                    source_path,
                    oracle_path: None,
                    oracle_relative_path: None,
                    attributes: BTreeMap::new(),
                };
                case_ids.insert(variant.case_id.clone());
                variants.push(variant);
            }
        }
        variants.sort_by(|left, right| left.case_id.cmp(&right.case_id));
        Ok(variants)
    }

    pub fn discover_js_variants(&self, suite: Suite) -> Result<Vec<BaselineVariant>, String> {
        self.discover_baseline_variants(suite, BaselineKind::Js)
    }

    pub fn discover_error_variants(&self, suite: Suite) -> Result<Vec<BaselineVariant>, String> {
        self.discover_baseline_variants(suite, BaselineKind::Errors)
    }

    /// Execute a concrete baseline oracle selected entirely by its
    /// prebuilt identity. Candidate output is never consulted during oracle
    /// selection.
    pub fn run_expanded_variant(
        &self,
        variant: &BaselineVariant,
        suite: Suite,
        kind: BaselineKind,
    ) -> ExpandedBaselineResult {
        assert!(matches!(
            kind,
            BaselineKind::Js | BaselineKind::Declarations | BaselineKind::Errors
        ));

        let worker_variant = variant.clone();
        let worker_runner = Self {
            workspace_root: self.workspace_root.clone(),
            strict_baseline_index: std::sync::Arc::clone(&self.strict_baseline_index),
        };
        let stem = variant
            .source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown");
        let spawn_result = std::thread::Builder::new()
            .name(format!("baseline-{}-variant-{stem}", kind.as_str()))
            .stack_size(BASELINE_CASE_STACK_SIZE)
            .spawn(move || worker_runner.run_expanded_variant_impl(&worker_variant, suite, kind));
        let result = match spawn_result {
            Ok(handle) => match handle.join() {
                Ok(result) => result,
                Err(panic_info) => self.expanded_variant_failure(
                    variant,
                    kind,
                    format!(
                        "PANIC during {} baseline: {}",
                        kind.as_str(),
                        Self::panic_message(panic_info.as_ref())
                    ),
                ),
            },
            Err(error) => self.expanded_variant_failure(
                variant,
                kind,
                format!("failed to spawn compilation thread: {error}"),
            ),
        };
        ExpandedBaselineResult {
            variant: variant.clone(),
            result,
        }
    }

    fn expanded_variant_failure(
        &self,
        variant: &BaselineVariant,
        kind: BaselineKind,
        diff: String,
    ) -> BaselineResult {
        BaselineResult {
            name: variant.name.clone(),
            test_path: variant.source_path.clone(),
            passed: false,
            diff: Some(diff),
            baseline_exists: variant.oracle_path.is_some() || kind == BaselineKind::Errors,
            actual_output: String::new(),
            expected_output: String::new(),
        }
    }

    fn run_expanded_variant_impl(
        &self,
        variant: &BaselineVariant,
        suite: Suite,
        kind: BaselineKind,
    ) -> BaselineResult {
        let failure = |diff: String, expected_output: String| BaselineResult {
            name: variant.name.clone(),
            test_path: variant.source_path.clone(),
            passed: false,
            diff: Some(diff),
            baseline_exists: variant.oracle_path.is_some() || kind == BaselineKind::Errors,
            actual_output: String::new(),
            expected_output,
        };

        // Read and project the immutable oracle before generating candidate
        // output so read failures remain failures, never false skips.
        let expected_output = if let Some(oracle_path) = variant.oracle_path.as_deref() {
            let raw_expected = match std::fs::read_to_string(oracle_path) {
                Ok(expected) => normalize_text(&expected),
                Err(error) => {
                    return failure(
                        format!("failed to read oracle {}: {error}", oracle_path.display()),
                        String::new(),
                    );
                }
            };
            project_baseline(&raw_expected, kind)
        } else if kind == BaselineKind::Errors {
            // Missing error baselines are the upstream harness's implicit
            // oracle that this variant should produce no diagnostics.
            String::new()
        } else {
            return failure("baseline file not found".to_string(), String::new());
        };

        let source = match read_test_source(&variant.source_path) {
            Ok(source) => source,
            Err(error) => {
                return failure(
                    format!("failed to read test file: {error}"),
                    expected_output,
                );
            }
        };
        let relative_test = format!(
            "tests/cases/{}/{}",
            suite.as_str(),
            variant
                .source_path
                .strip_prefix(self.workspace_root.join(suite.relative_dir()))
                .unwrap_or(&variant.source_path)
                .to_string_lossy()
                .replace('\\', "/")
        );
        let mut test_case = tsc_rs_ast::parse_test_case(&relative_test, &source);
        fixup_multi_value_options(&mut test_case, &source);
        if let Err(error) = apply_variant_options(&mut test_case, &variant.attributes) {
            return failure(
                format!("UNSUPPORTED-VARIANT-OPTION: {error}"),
                expected_output,
            );
        }

        let generated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
            BaselineKind::Js | BaselineKind::Declarations => project_baseline(
                &normalize_text(&self.generate_js_baseline(&test_case, &relative_test, &source)),
                kind,
            ),
            BaselineKind::Errors => {
                normalize_text(&self.generate_errors_baseline(&test_case, &relative_test, &source))
            }
            BaselineKind::Symbols | BaselineKind::Types => unreachable!(),
        }));
        let actual_output = match generated {
            Ok(output) => output,
            Err(panic_info) => {
                return failure(
                    format!(
                        "PANIC during {} baseline: {}",
                        kind.as_str(),
                        Self::panic_message(panic_info.as_ref())
                    ),
                    expected_output,
                );
            }
        };

        let passed = actual_output.trim_end_matches('\n') == expected_output.trim_end_matches('\n');
        let diff = (!passed).then(|| crate::generate_diff(&expected_output, &actual_output));
        BaselineResult {
            name: variant.name.clone(),
            test_path: variant.source_path.clone(),
            passed,
            diff,
            baseline_exists: variant.oracle_path.is_some() || kind == BaselineKind::Errors,
            actual_output,
            expected_output,
        }
    }

    pub fn run_expanded_suite(
        &self,
        suite: Suite,
        kind: BaselineKind,
        max_source_cases: Option<usize>,
    ) -> Result<Vec<ExpandedBaselineResult>, String> {
        let mut results = Vec::new();
        self.visit_expanded_suite(suite, kind, max_source_cases, |result| {
            results.push(result);
        })?;
        Ok(results)
    }

    /// Execute an expanded suite in bounded parallel waves and visit results
    /// in stable case-ID order.
    ///
    /// The callback runs serially on the calling thread. Each wave contains at
    /// most one result per Rayon worker, so streaming consumers can process and
    /// drop large actual/expected baselines instead of retaining the whole
    /// suite in memory.
    pub fn visit_expanded_suite<F>(
        &self,
        suite: Suite,
        kind: BaselineKind,
        max_source_cases: Option<usize>,
        visit: F,
    ) -> Result<usize, String>
    where
        F: FnMut(ExpandedBaselineResult),
    {
        if !matches!(
            kind,
            BaselineKind::Js | BaselineKind::Declarations | BaselineKind::Errors
        ) {
            return Err(format!(
                "expanded variants are not implemented for {} baselines",
                kind.as_str()
            ));
        }
        let mut variants = self.discover_baseline_variants(suite, kind)?;
        if let Some(max_source_cases) = max_source_cases {
            let allowed_sources: BTreeSet<_> = variants
                .iter()
                .map(|variant| variant.source_case_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(max_source_cases)
                .collect();
            variants.retain(|variant| allowed_sources.contains(&variant.source_case_id));
        }
        let requested_threads = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|count| count.get())
                    .unwrap_or(4)
            });
        let total = variants.len();
        self.visit_expanded_variants(&variants, suite, kind, requested_threads, visit)?;
        Ok(total)
    }

    fn visit_expanded_variants<F>(
        &self,
        variants: &[BaselineVariant],
        suite: Suite,
        kind: BaselineKind,
        requested_threads: usize,
        mut visit: F,
    ) -> Result<(), String>
    where
        F: FnMut(ExpandedBaselineResult),
    {
        use rayon::prelude::*;
        let requested_threads = requested_threads.max(1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(requested_threads)
            .stack_size(8 * 1024 * 1024)
            .build()
            .map_err(|error| format!("failed to build expanded baseline pool: {error}"))?;
        for wave in variants.chunks(requested_threads) {
            let mut results: Vec<ExpandedBaselineResult> = pool.install(|| {
                wave.par_iter()
                    .map(|variant| self.run_expanded_variant(variant, suite, kind))
                    .collect()
            });
            results.sort_by(
                |left: &ExpandedBaselineResult, right: &ExpandedBaselineResult| {
                    left.variant.case_id.cmp(&right.variant.case_id)
                },
            );
            for result in results {
                visit(result);
            }
        }
        Ok(())
    }

    pub fn run_expanded_js_suite(
        &self,
        suite: Suite,
        kind: BaselineKind,
    ) -> Result<Vec<ExpandedBaselineResult>, String> {
        if !matches!(kind, BaselineKind::Js | BaselineKind::Declarations) {
            return Err(format!(
                "expanded variants are not implemented for {} baselines",
                kind.as_str()
            ));
        }
        self.run_expanded_suite(suite, kind, None)
    }
}

fn project_baseline(output: &str, kind: BaselineKind) -> String {
    match kind {
        BaselineKind::Js => strip_dts_sections(output),
        BaselineKind::Declarations => extract_dts_sections(output),
        BaselineKind::Errors => output.to_string(),
        BaselineKind::Symbols | BaselineKind::Types => {
            unreachable!("expanded baseline projection is unavailable")
        }
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("unsupported {key} variant value {value:?}")),
    }
}

/// Apply a concrete oracle identity to a parsed test case. Every accepted
/// attribute changes the compiler input; values that cannot be represented
/// fail closed instead of selecting a nearby oracle.
pub fn apply_variant_options(
    test_case: &mut TestCase,
    attributes: &BTreeMap<String, String>,
) -> Result<(), String> {
    for (key, value) in attributes {
        match key.as_str() {
            "target" => {
                test_case.options.target = Some(
                    ScriptTarget::parse(value)
                        .ok_or_else(|| format!("unsupported target variant value {value:?}"))?,
                );
            }
            "module" => {
                test_case.options.module = Some(
                    ModuleKind::parse(value)
                        .ok_or_else(|| format!("unsupported module variant value {value:?}"))?,
                );
            }
            "jsx" => {
                test_case.options.jsx = Some(
                    JsxEmit::parse(value)
                        .ok_or_else(|| format!("unsupported jsx variant value {value:?}"))?,
                );
            }
            "moduleresolution" => {
                if !matches!(
                    value.as_str(),
                    "node" | "node10" | "node16" | "nodenext" | "classic"
                ) {
                    return Err(format!(
                        "unsupported moduleResolution variant value {value:?}"
                    ));
                }
                test_case.options.module_resolution = Some(value.clone());
            }
            "moduledetection" => {
                if value != "force" {
                    return Err(format!(
                        "unsupported moduleDetection variant value {value:?}"
                    ));
                }
                test_case.options.module_detection = Some(value.clone());
            }
            "alwaysstrict" => test_case.options.always_strict = Some(parse_bool(key, value)?),
            "strict" => test_case.options.strict = Some(parse_bool(key, value)?),
            "esmoduleinterop" => {
                test_case.options.es_module_interop = Some(parse_bool(key, value)?)
            }
            "usedefineforclassfields" => {
                test_case.options.use_define_for_class_fields = Some(parse_bool(key, value)?)
            }
            "isolatedmodules" => test_case.options.isolated_modules = Some(parse_bool(key, value)?),
            "isolateddeclarations" => {
                test_case.options.isolated_declarations = Some(parse_bool(key, value)?)
            }
            "allowimportingtsextensions" => {
                test_case.options.allow_importing_ts_extensions = Some(parse_bool(key, value)?)
            }
            "exactoptionalpropertytypes" => {
                test_case.options.exact_optional_property_types = Some(parse_bool(key, value)?)
            }
            "verbatimmodulesyntax" => {
                test_case.options.verbatim_module_syntax = Some(parse_bool(key, value)?)
            }
            "nolib" => test_case.options.no_lib = Some(parse_bool(key, value)?),
            "experimentaldecorators" => {
                test_case.options.experimental_decorators = Some(parse_bool(key, value)?)
            }
            "strictnullchecks" => {
                test_case.options.strict_null_checks = Some(parse_bool(key, value)?)
            }
            "noimplicitany" => test_case.options.no_implicit_any = Some(parse_bool(key, value)?),
            "noemit" => test_case.options.no_emit = Some(parse_bool(key, value)?),
            "downleveliteration" => {
                test_case.options.down_level_iteration = Some(parse_bool(key, value)?)
            }
            "preserveconstenums" => {
                test_case.options.preserve_const_enums = Some(parse_bool(key, value)?)
            }
            "nouncheckedindexedaccess" => {
                test_case.options.no_unchecked_indexed_access = Some(parse_bool(key, value)?)
            }
            "nopropertyaccessfromindexsignature" => {
                test_case.options.no_property_access_from_index_signature =
                    Some(parse_bool(key, value)?)
            }
            "useunknownincatchvariables" => {
                test_case.options.use_unknown_in_catch_variables = Some(parse_bool(key, value)?)
            }
            "resolvejsonmodule" => {
                test_case.options.resolve_json_module = Some(parse_bool(key, value)?)
            }
            "allowarbitraryextensions"
            | "noimplicitoverride"
            | "nouncheckedsideeffectimports"
            | "resolvepackagejsonexports"
            | "stabletypeordering"
            | "strictbuiltiniteratorreturn" => {
                return Err(format!(
                    "variant option {key:?} has no implemented compiler semantics"
                ));
            }
            _ => return Err(format!("unsupported variant option {key:?}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_canonical_and_case_ids_escape_delimiters() {
        let attrs = parse_attributes("Target=ES2015,module=CommonJS", "x.js").unwrap();
        assert_eq!(variant_suffix(&attrs), "module=commonjs,target=es2015");
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(
            canonical_variant_case_id("tests/cases/compiler/x.ts", &attrs).unwrap(),
            "tests/cases/compiler/x.ts::variant(module=commonjs,target=es2015)"
        );
        assert!(canonical_variant_case_id(
            "tests/cases/compiler/x.ts",
            &BTreeMap::from([("Target".to_string(), "es2015".to_string())])
        )
        .is_err());
    }

    #[test]
    fn duplicate_or_malformed_attributes_fail_closed() {
        assert!(parse_attributes("target=es5,TARGET=es2015", "x.js").is_err());
        assert!(parse_attributes("target", "x.js").is_err());
        assert!(parse_attributes("=es5", "x.js").is_err());
    }

    #[test]
    fn manifest_paths_must_be_nonempty_relative_and_normalized() {
        assert!(
            validate_normalized_oracle_path("tests/baselines/reference/a(target=es5).js").is_ok()
        );
        for path in [
            "",
            "/tests/baselines/reference/a.js",
            "./tests/baselines/reference/a.js",
            "tests//baselines/reference/a.js",
            "tests/baselines/../reference/a.js",
            "tests\\baselines\\reference\\a.js",
            "other/a.js",
            "C:/tests/baselines/reference/a.js",
        ] {
            assert!(
                validate_normalized_oracle_path(path).is_err(),
                "unexpectedly accepted {path:?}"
            );
        }
    }

    #[test]
    fn unsupported_enum_values_fail_closed() {
        let mut test_case = tsc_rs_ast::parse_test_case("x.ts", "let x = 1;");
        let attrs = BTreeMap::from([("module".to_string(), "node22".to_string())]);
        assert!(apply_variant_options(&mut test_case, &attrs)
            .unwrap_err()
            .contains("node22"));

        for (key, value) in [
            ("moduleresolution", "not-a-resolution"),
            ("moduledetection", "not-a-detection-mode"),
        ] {
            let mut test_case = tsc_rs_ast::parse_test_case("x.ts", "let x = 1;");
            let attrs = BTreeMap::from([(key.to_string(), value.to_string())]);
            assert!(apply_variant_options(&mut test_case, &attrs)
                .unwrap_err()
                .contains(value));
        }
    }

    #[test]
    fn semantically_unimplemented_boolean_variants_fail_closed() {
        for key in [
            "allowarbitraryextensions",
            "noimplicitoverride",
            "nouncheckedsideeffectimports",
            "resolvepackagejsonexports",
            "stabletypeordering",
            "strictbuiltiniteratorreturn",
        ] {
            let mut test_case = tsc_rs_ast::parse_test_case("x.ts", "let x = 1;");
            let attrs = BTreeMap::from([(key.to_string(), "true".to_string())]);
            let error = apply_variant_options(&mut test_case, &attrs).unwrap_err();
            assert!(
                error.contains("no implemented compiler semantics"),
                "{error}"
            );
            assert!(!test_case
                .options
                .other
                .iter()
                .any(|(existing, _)| existing.eq_ignore_ascii_case(key)));
        }
    }

    #[test]
    fn supported_string_variant_enums_are_applied_exactly() {
        for (key, value) in [("moduleresolution", "node16"), ("moduledetection", "force")] {
            let mut test_case = tsc_rs_ast::parse_test_case("x.ts", "let x = 1;");
            let attrs = BTreeMap::from([(key.to_string(), value.to_string())]);
            apply_variant_options(&mut test_case, &attrs).unwrap();
            let applied = if key == "moduleresolution" {
                test_case.options.module_resolution.as_deref()
            } else {
                test_case.options.module_detection.as_deref()
            };
            assert_eq!(applied, Some(value));
        }
    }

    #[test]
    fn semantically_collapsed_string_variants_fail_closed() {
        for (key, value) in [
            ("moduleresolution", "bundler"),
            ("moduledetection", "auto"),
            ("moduledetection", "legacy"),
        ] {
            let mut test_case = tsc_rs_ast::parse_test_case("x.ts", "let x = 1;");
            let attrs = BTreeMap::from([(key.to_string(), value.to_string())]);
            let error = apply_variant_options(&mut test_case, &attrs).unwrap_err();
            assert!(error.contains(value), "{error}");
        }
    }

    #[test]
    fn current_node_and_target_variants_remain_distinct_and_executable() {
        for (value, expected) in [
            ("node18", ModuleKind::Node18),
            ("node20", ModuleKind::Node20),
        ] {
            let mut test_case = tsc_rs_ast::parse_test_case("x.ts", "export const x = 1;");
            apply_variant_options(
                &mut test_case,
                &BTreeMap::from([("module".to_string(), value.to_string())]),
            )
            .unwrap();
            assert_eq!(test_case.options.module, Some(expected));
        }

        let mut test_case = tsc_rs_ast::parse_test_case("x.ts", "let x = 1;");
        apply_variant_options(
            &mut test_case,
            &BTreeMap::from([("target".to_string(), "es2025".to_string())]),
        )
        .unwrap();
        assert_eq!(test_case.options.target, Some(ScriptTarget::ES2025));
    }

    #[test]
    fn error_directive_matrices_include_implicit_empty_combinations() {
        let root = tempfile::tempdir().unwrap();
        let cases = root.path().join("tests/cases/compiler");
        let baselines = root.path().join("tests/baselines/reference");
        std::fs::create_dir_all(&cases).unwrap();
        std::fs::create_dir_all(&baselines).unwrap();
        std::fs::write(
            cases.join("matrix.ts"),
            "// @target: es5, es2015\n\
             // @module: commonjs, esnext\n\
             export const value = 1;\n",
        )
        .unwrap();
        std::fs::write(
            baselines.join("matrix(target=es5).errors.txt"),
            "matrix.ts(3,1): error TS9999: oracle\n",
        )
        .unwrap();

        let runner = BaselineRunner::new(root.path());
        let variants = runner.discover_error_variants(Suite::Compiler).unwrap();
        assert_eq!(variants.len(), 4);
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.case_id.as_str())
                .collect::<Vec<_>>(),
            [
                "tests/cases/compiler/matrix.ts::variant(module=commonjs,target=es2015)",
                "tests/cases/compiler/matrix.ts::variant(module=commonjs,target=es5)",
                "tests/cases/compiler/matrix.ts::variant(module=esnext,target=es2015)",
                "tests/cases/compiler/matrix.ts::variant(module=esnext,target=es5)",
            ]
        );
        assert_eq!(
            variants
                .iter()
                .filter(|variant| variant.oracle_path.is_some())
                .count(),
            2,
            "the target-only oracle applies to both matching module variants"
        );
        assert!(variants
            .iter()
            .filter(|variant| variant
                .attributes
                .get("target")
                .is_some_and(|v| v == "es2015"))
            .all(|variant| variant.oracle_path.is_none()));

        let first = runner
            .run_expanded_suite(Suite::Compiler, BaselineKind::Errors, None)
            .unwrap();
        let second = runner
            .run_expanded_suite(Suite::Compiler, BaselineKind::Errors, None)
            .unwrap();
        let first_ids: Vec<_> = first
            .iter()
            .map(|result| result.variant.case_id.as_str())
            .collect();
        let second_ids: Vec<_> = second
            .iter()
            .map(|result| result.variant.case_id.as_str())
            .collect();
        assert_eq!(first_ids, second_ids);
        assert!(first_ids.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(first
            .iter()
            .filter(|result| {
                result
                    .variant
                    .attributes
                    .get("target")
                    .is_some_and(|value| value == "es2015")
            })
            .all(|result| {
                result.result.baseline_exists && result.result.expected_output.is_empty()
            }));
    }

    #[test]
    fn wildcard_boolean_expands_both_oracle_and_empty_variants() {
        let root = tempfile::tempdir().unwrap();
        let cases = root.path().join("tests/cases/compiler");
        let baselines = root.path().join("tests/baselines/reference");
        std::fs::create_dir_all(&cases).unwrap();
        std::fs::create_dir_all(&baselines).unwrap();
        std::fs::write(
            cases.join("decorators.ts"),
            "// @experimentalDecorators: *\nclass C {}\n",
        )
        .unwrap();
        std::fs::write(
            baselines.join("decorators(experimentaldecorators=false).errors.txt"),
            "decorators.ts(2,1): error TS1206: Decorators are not valid here.\n",
        )
        .unwrap();

        let runner = BaselineRunner::new(root.path());
        let variants = runner.discover_error_variants(Suite::Compiler).unwrap();
        assert_eq!(variants.len(), 2);
        let disabled = variants
            .iter()
            .find(|variant| {
                variant
                    .attributes
                    .get("experimentaldecorators")
                    .is_some_and(|value| value == "false")
            })
            .unwrap();
        let enabled = variants
            .iter()
            .find(|variant| {
                variant
                    .attributes
                    .get("experimentaldecorators")
                    .is_some_and(|value| value == "true")
            })
            .unwrap();
        assert!(disabled.oracle_path.is_some());
        assert!(enabled.oracle_path.is_none());
    }

    #[test]
    fn wildcard_enum_exclusions_use_upstream_variant_labels() {
        let variants = error_directive_variants("// @target: *,-es3\n");
        assert_eq!(variants.len(), 13);
        assert!(variants
            .iter()
            .any(|attributes| attributes.get("target").is_some_and(|value| value == "es6")));
        assert!(variants.iter().all(|attributes| {
            !attributes
                .get("target")
                .is_some_and(|value| value == "es3" || value == "es2015")
        }));
    }

    #[test]
    fn utf16_sources_are_decoded_before_directive_expansion_and_execution() {
        let root = tempfile::tempdir().unwrap();
        let cases = root.path().join("tests/cases/compiler");
        std::fs::create_dir_all(&cases).unwrap();
        std::fs::create_dir_all(root.path().join("tests/baselines/reference")).unwrap();
        let source = "// @target: es5, es2015\nconst café = 1;\n";
        let mut encoded = vec![0xff, 0xfe];
        for unit in source.encode_utf16() {
            encoded.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(cases.join("unicode.ts"), encoded).unwrap();

        let runner = BaselineRunner::new(root.path());
        let variants = runner.discover_error_variants(Suite::Compiler).unwrap();
        assert_eq!(variants.len(), 2);
        let result = runner.run_expanded_variant(
            variants
                .iter()
                .find(|variant| {
                    variant
                        .attributes
                        .get("target")
                        .is_some_and(|value| value == "es2015")
                })
                .unwrap(),
            Suite::Compiler,
            BaselineKind::Errors,
        );
        assert!(result.result.baseline_exists);
        assert!(result.result.expected_output.is_empty());
        assert!(result.result.actual_output.is_empty());
        assert!(result.result.passed);
    }

    #[test]
    fn expanded_error_variant_uses_large_stack_for_ordinary_deep_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("harness crate must be inside the workspace");
        let runner = BaselineRunner::new(root);
        let variants = runner.discover_error_variants(Suite::Compiler).unwrap();
        let variant = variants
            .iter()
            .find(|variant| {
                variant
                    .source_case_id
                    .ends_with("/binderBinaryExpressionStress.ts")
            })
            .expect("deep binary-expression fixture must remain in the compiler inventory");

        assert_eq!(
            variant.case_id,
            "tests/cases/compiler/binderBinaryExpressionStress.ts"
        );
        assert!(variant.attributes.is_empty());
        assert!(
            variant.oracle_path.is_none(),
            "the ordinary fixture uses the implicit empty errors oracle"
        );

        let expanded = runner.run_expanded_variant(variant, Suite::Compiler, BaselineKind::Errors);
        assert!(
            expanded.result.passed,
            "{}",
            expanded
                .result
                .diff
                .as_deref()
                .unwrap_or("unexpected failure")
        );
        assert!(expanded.result.actual_output.is_empty());
        assert!(expanded.result.expected_output.is_empty());
    }

    #[test]
    fn bounded_visitor_is_byte_identical_with_one_and_four_workers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("harness crate must be inside the workspace");
        let runner = BaselineRunner::new(root);
        let variants: Vec<_> = runner
            .discover_error_variants(Suite::Compiler)
            .unwrap()
            .into_iter()
            .filter(|variant| {
                std::fs::metadata(&variant.source_path)
                    .is_ok_and(|metadata| metadata.len() < 4 * 1024)
            })
            .take(8)
            .collect();
        assert_eq!(variants.len(), 8);

        let run = |threads| {
            let mut observed = Vec::new();
            runner
                .visit_expanded_variants(
                    &variants,
                    Suite::Compiler,
                    BaselineKind::Errors,
                    threads,
                    |expanded| {
                        observed.push((
                            expanded.variant.case_id,
                            expanded.result.passed,
                            expanded.result.baseline_exists,
                            expanded.result.diff,
                            expanded.result.actual_output,
                            expanded.result.expected_output,
                        ));
                    },
                )
                .unwrap();
            observed
        };

        let serial = run(1);
        let parallel = run(4);
        assert_eq!(serial, parallel);
        assert!(serial
            .windows(2)
            .all(|pair| pair[0].0.as_str() < pair[1].0.as_str()));
    }

    #[test]
    fn workspace_js_inventory_matches_the_audited_expansion() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("harness crate must be inside the workspace");
        let runner = BaselineRunner::new(root);
        let compiler = runner.discover_js_variants(Suite::Compiler).unwrap();
        let conformance = runner.discover_js_variants(Suite::Conformance).unwrap();

        let compiler_oracles = compiler
            .iter()
            .filter(|variant| variant.oracle_path.is_some())
            .count();
        let conformance_oracles = conformance
            .iter()
            .filter(|variant| variant.oracle_path.is_some())
            .count();
        assert_eq!(
            (compiler.len(), compiler_oracles),
            (7_201, 6_704),
            "compiler inventory drifted"
        );
        assert_eq!(
            (conformance.len(), conformance_oracles),
            (7_618, 7_099),
            "conformance inventory drifted"
        );
        assert_eq!(compiler.len() + conformance.len(), 14_819);
        assert_eq!(compiler_oracles + conformance_oracles, 13_803);
    }

    #[test]
    fn execution_uses_the_preselected_oracle_and_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let cases = root.path().join("tests/cases/compiler");
        let baselines = root.path().join("tests/baselines/reference");
        std::fs::create_dir_all(&cases).unwrap();
        std::fs::create_dir_all(&baselines).unwrap();
        std::fs::write(cases.join("exact.ts"), "export const exact = 1;\n").unwrap();
        std::fs::write(
            cases.join("matrix.ts"),
            "// @module: node20, commonjs\nexport const matrix = 1;\n",
        )
        .unwrap();
        std::fs::write(cases.join("missing.ts"), "export const missing = 1;\n").unwrap();
        std::fs::write(baselines.join("exact.js"), "EXACT ORACLE\n").unwrap();
        std::fs::write(
            baselines.join("matrix(module=commonjs).js"),
            "COMMONJS ORACLE\n",
        )
        .unwrap();
        std::fs::write(
            baselines.join("matrix(module=node20).js"),
            "NODE20 ORACLE\n",
        )
        .unwrap();

        let runner = BaselineRunner::new(root.path());
        let variants = runner.discover_js_variants(Suite::Compiler).unwrap();
        assert_eq!(variants.len(), 4);
        assert!(variants
            .iter()
            .any(|variant| variant.case_id == "tests/cases/compiler/exact.ts"));
        assert!(variants.iter().any(|variant| {
            variant.case_id == "tests/cases/compiler/matrix.ts::variant(module=commonjs)"
        }));
        assert!(variants.iter().any(|variant| {
            variant.case_id == "tests/cases/compiler/matrix.ts::variant(module=node20)"
        }));
        assert!(variants.iter().any(|variant| {
            variant.case_id == "tests/cases/compiler/missing.ts" && variant.oracle_path.is_none()
        }));

        let commonjs = variants
            .iter()
            .find(|variant| {
                variant
                    .attributes
                    .get("module")
                    .is_some_and(|v| v == "commonjs")
            })
            .unwrap();
        let commonjs_result =
            runner.run_expanded_variant(commonjs, Suite::Compiler, BaselineKind::Js);
        assert_eq!(commonjs_result.result.expected_output, "COMMONJS ORACLE\n");

        let node20 = variants
            .iter()
            .find(|variant| {
                variant
                    .attributes
                    .get("module")
                    .is_some_and(|v| v == "node20")
            })
            .unwrap();
        let node20_result = runner.run_expanded_variant(node20, Suite::Compiler, BaselineKind::Js);
        assert!(node20_result.result.baseline_exists);
        assert_eq!(node20_result.result.expected_output, "NODE20 ORACLE\n");
        assert!(!node20_result
            .result
            .diff
            .as_deref()
            .unwrap_or_default()
            .starts_with("UNSUPPORTED-VARIANT-OPTION"));
    }
}
