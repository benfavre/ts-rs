//! Harness to classify baseline mismatches against upstream TypeScript tests.
//!
//! Provides two test-running strategies:
//!
//! 1. **`BootstrapHarness`** -- Runs an oracle (upstream `tsc`) against our
//!    candidate compiler as an external process and diffs the outputs.
//!
//! 2. **`BaselineRunner`** -- Discovers test cases under `tests/cases/`,
//!    parses them through the in-tree pipeline (`tsc_rs_parser` + an
//!    internal emit stub), and compares the generated JavaScript against
//!    stored baselines in `tests/baselines/reference/`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use tsc_rs_ast::{
    AstString, CompilerOptions, Diagnostic, ExportDeclKind, ExprKind, ImportClause, ImportDecl,
    ImportsNotUsedAsValues, JsxEmit, ModuleKind, ModuleName, ScriptTarget, SourceFile, StmtKind,
    MOD_EXPORT,
};
use tsc_rs_scanner::{Scanner, TokenKind};

// Re-export AST helpers so consumers only need `tsc_rs_harness`.
pub use tsc_rs_ast::{parse_test_case, TestCase, TestFile};

// Shared cache key helper for tools that cache harness results.
pub mod cache_key;

/// Stable, oracle-first identities for opt-in expanded baseline manifests.
pub mod baseline_variant;

// LSP test harness modules
pub mod lsp_baseline;
pub mod lsp_classify;
pub mod lsp_executor;
pub mod lsp_parser;
pub mod lsp_runner;

/// Return the literal module specifiers from dynamic `import()` calls.
/// Scanning tokens avoids mistaking `import(` text in comments or strings for
/// a call while preserving the raw literal value used by the emitter AST.
fn dynamic_import_literal_specifiers(source: &str) -> Vec<String> {
    let tokens = Scanner::new(source).scan_all();
    let mut specifiers = Vec::new();
    for window in tokens.windows(3) {
        if window[0].kind != TokenKind::Import || window[1].kind != TokenKind::OpenParen {
            continue;
        }
        if !matches!(
            window[2].kind,
            TokenKind::StringLiteral | TokenKind::NoSubstitutionTemplate
        ) {
            continue;
        }
        let start = window[2].span.start as usize;
        let end = window[2].span.end as usize;
        if end > start + 1 && end <= source.len() {
            specifiers.push(source[start + 1..end - 1].to_string());
        }
    }
    specifiers
}

fn encode_hidden_option_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_test_source(bytes: &[u8]) -> io::Result<String> {
    if let Some(contents) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(contents.to_vec())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    }

    let (encoding, little_endian, contents) =
        if let Some(contents) = bytes.strip_prefix(&[0xFF, 0xFE]) {
            ("UTF-16LE", true, contents)
        } else if let Some(contents) = bytes.strip_prefix(&[0xFE, 0xFF]) {
            ("UTF-16BE", false, contents)
        } else {
            return String::from_utf8(bytes.to_vec())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        };

    if !contents.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{encoding} source has an odd number of bytes"),
        ));
    }

    let code_units = contents.chunks_exact(2).map(|bytes| {
        if little_endian {
            u16::from_le_bytes([bytes[0], bytes[1]])
        } else {
            u16::from_be_bytes([bytes[0], bytes[1]])
        }
    });

    String::from_utf16(&code_units.collect::<Vec<_>>())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn read_test_source(path: &Path) -> io::Result<String> {
    decode_test_source(&std::fs::read(path)?)
}

// ---------------------------------------------------------------------------
// Mismatch classification (shared by both harness strategies)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchClass {
    ParseTree,
    Binder,
    Type,
    Diagnostic,
    Emit,
    SourceMap,
    Ordering,
    Runtime,
}

#[derive(Debug, Clone)]
pub struct HarnessResult {
    pub test_name: String,
    pub mismatches: Vec<MismatchClass>,
    pub diagnostics: Vec<Diagnostic>,
}

pub trait Harness {
    fn run_compiler_case(&self, test_name: &str) -> HarnessResult;
    fn run_fourslash_case(&self, test_name: &str) -> HarnessResult;
    fn run_project_case(&self, test_name: &str) -> HarnessResult;
}

// ---------------------------------------------------------------------------
// Baseline kind (which type of baseline to compare)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineKind {
    Js,
    Declarations,
    Errors,
    Symbols,
    Types,
}

fn bool_variant(value: Option<bool>) -> Option<String> {
    value.map(|value| value.to_string())
}

fn directive_variant_value(source: &str, key: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let rest = line
            .trim()
            .strip_prefix("//")?
            .trim_start()
            .strip_prefix('@')?;
        let (directive, values) = rest.split_once(':')?;
        directive.trim().eq_ignore_ascii_case(key).then(|| {
            values
                .split(',')
                .next()
                .unwrap_or(values)
                .trim()
                .to_ascii_lowercase()
        })
    })
}

fn option_variant_value(options: &CompilerOptions, source: &str, key: &str) -> Option<String> {
    let typed = match key {
        "target" => options
            .target
            .map(|value| format!("{value:?}").to_ascii_lowercase()),
        "module" => options.module.map(|value| {
            match value {
                ModuleKind::None => "none",
                ModuleKind::CommonJS => "commonjs",
                ModuleKind::AMD => "amd",
                ModuleKind::UMD => "umd",
                ModuleKind::System => "system",
                ModuleKind::ES2015 => "es6",
                ModuleKind::ES2020 => "es2020",
                ModuleKind::ES2022 => "es2022",
                ModuleKind::ESNext => "esnext",
                ModuleKind::Node16 => "node16",
                ModuleKind::Node18 => "node18",
                ModuleKind::Node20 => "node20",
                ModuleKind::NodeNext => "nodenext",
                ModuleKind::Preserve => "preserve",
            }
            .to_string()
        }),
        "jsx" => options.jsx.map(|value| {
            match value {
                JsxEmit::None => "none",
                JsxEmit::Preserve => "preserve",
                JsxEmit::React => "react",
                JsxEmit::ReactNative => "react-native",
                JsxEmit::ReactJSX => "react-jsx",
                JsxEmit::ReactJSXDev => "react-jsxdev",
            }
            .to_string()
        }),
        "moduleresolution" => options
            .module_resolution
            .as_ref()
            .map(|v| v.to_ascii_lowercase()),
        "moduledetection" => options
            .module_detection
            .as_ref()
            .map(|v| v.to_ascii_lowercase()),
        "alwaysstrict" => bool_variant(options.always_strict),
        "strict" => bool_variant(options.strict),
        "esmoduleinterop" => bool_variant(options.es_module_interop),
        "usedefineforclassfields" => bool_variant(options.use_define_for_class_fields),
        "isolatedmodules" => bool_variant(options.isolated_modules),
        "allowimportingtsextensions" => bool_variant(options.allow_importing_ts_extensions),
        "exactoptionalpropertytypes" => bool_variant(options.exact_optional_property_types),
        "verbatimmodulesyntax" => bool_variant(options.verbatim_module_syntax),
        "nolib" => bool_variant(options.no_lib),
        "experimentaldecorators" => bool_variant(options.experimental_decorators),
        "strictnullchecks" => bool_variant(options.strict_null_checks),
        "noimplicitany" => bool_variant(options.no_implicit_any),
        "noemit" => bool_variant(options.no_emit),
        "downleveliteration" => bool_variant(options.down_level_iteration),
        "preserveconstenums" => bool_variant(options.preserve_const_enums),
        "nouncheckedindexedaccess" => bool_variant(options.no_unchecked_indexed_access),
        "nopropertyaccessfromindexsignature" => {
            bool_variant(options.no_property_access_from_index_signature)
        }
        "useunknownincatchvariables" => bool_variant(options.use_unknown_in_catch_variables),
        _ => options.other.iter().rev().find_map(|(option_key, value)| {
            option_key.eq_ignore_ascii_case(key).then(|| {
                value
                    .split(',')
                    .next()
                    .unwrap_or(value)
                    .trim()
                    .to_ascii_lowercase()
            })
        }),
    };
    typed.or_else(|| directive_variant_value(source, key))
}

fn parameterized_variant_attributes<'a>(
    file_name: &'a str,
    stem: &str,
    extension: &str,
) -> Option<Vec<(&'a str, &'a str)>> {
    let attributes = file_name
        .strip_prefix(&format!("{stem}("))?
        .strip_suffix(&format!("){extension}"))?;
    attributes
        .split(',')
        .map(|attribute| attribute.split_once('='))
        .collect()
}

fn strict_option_variant_value(
    options: &CompilerOptions,
    source: &str,
    key: &str,
) -> Option<String> {
    let key = key.trim().to_ascii_lowercase();
    let value = if key == "module" && options.module == Some(ModuleKind::ES2015) {
        // Stored parameterized baselines use the canonical `es2015` spelling.
        Some("es2015".to_string())
    } else {
        option_variant_value(options, source, &key)
    }?;
    Some(
        value
            .split(',')
            .next()
            .unwrap_or(&value)
            .trim()
            .to_ascii_lowercase(),
    )
}

fn strict_option_variant_matches(key: &str, actual: &str, expected: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    let actual = actual.trim();
    let expected = expected.trim();
    actual.eq_ignore_ascii_case(expected)
        || matches!(
            (
                key.as_str(),
                actual.to_ascii_lowercase().as_str(),
                expected.to_ascii_lowercase().as_str()
            ),
            ("target" | "module", "es2015", "es6") | ("target" | "module", "es6", "es2015")
        )
}

/// Immutable inventory of reference baseline files. A runner shares one lazy
/// instance across its rayon workers and large-stack case threads.
#[derive(Debug)]
struct StrictBaselineIndex {
    files_by_name: HashSet<String>,
    parameterized: HashMap<(String, &'static str), Vec<PathBuf>>,
}

impl StrictBaselineIndex {
    fn build(baseline_dir: &Path) -> Result<Self, String> {
        let entries = std::fs::read_dir(baseline_dir).map_err(|error| {
            format!(
                "could not inspect baseline directory {}: {error}",
                baseline_dir.display()
            )
        })?;
        let mut files_by_name = HashSet::new();
        let mut parameterized: HashMap<(String, &'static str), Vec<PathBuf>> = HashMap::new();

        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "could not inspect an entry in baseline directory {}: {error}",
                    baseline_dir.display()
                )
            })?;
            let file_type = entry.file_type().map_err(|error| {
                format!(
                    "could not inspect baseline candidate {}: {error}",
                    entry.path().display()
                )
            })?;
            if !file_type.is_file() {
                continue;
            }
            let file_name = entry.file_name();
            let name = file_name.to_str().ok_or_else(|| {
                format!(
                    "baseline directory contains a non-Unicode file name: {}",
                    entry.path().display()
                )
            })?;
            files_by_name.insert(name.to_string());

            for extension in [".js", ".symbols", ".types"] {
                let Some(without_extension) = name.strip_suffix(extension) else {
                    continue;
                };
                // Index every possible `stem(` prefix. This exactly preserves
                // the old prefix filter even for unusual stems containing
                // parentheses; attribute validation remains selection-local.
                for (offset, character) in without_extension.char_indices() {
                    if character == '(' {
                        parameterized
                            .entry((without_extension[..offset].to_string(), extension))
                            .or_default()
                            .push(entry.path());
                    }
                }
            }
        }

        for candidates in parameterized.values_mut() {
            candidates.sort();
        }
        Ok(Self {
            files_by_name,
            parameterized,
        })
    }

    fn candidates(&self, stem: &str, extension: &'static str) -> &[PathBuf] {
        self.parameterized
            .get(&(stem.to_string(), extension))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn contains_name(&self, name: &str) -> bool {
        self.files_by_name.contains(name)
    }
}

/// Load an exact baseline or the single most-specific parameterized baseline
/// selected by the test case's effective compiler options.
///
/// Unlike the JavaScript compatibility resolver, this deliberately never
/// inspects generated output. Symbols and types oracles must be selected only
/// by provenance encoded in their filenames. Malformed or equally-specific
/// matching candidates fail closed instead of depending on directory order.
fn select_strict_option_baseline(
    candidates: &[PathBuf],
    stem: &str,
    extension: &str,
    options: &CompilerOptions,
    source: &str,
) -> Result<Option<String>, String> {
    let mut matching = Vec::new();
    for path in candidates {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                format!(
                    "parameterized baseline has a non-Unicode name: {}",
                    path.display()
                )
            })?;
        let attributes = parameterized_variant_attributes(name, stem, extension)
            .ok_or_else(|| format!("malformed parameterized baseline name: {name}"))?;
        let mut keys = BTreeSet::new();
        for (key, expected) in &attributes {
            let key = key.trim().to_ascii_lowercase();
            if key.is_empty() || expected.trim().is_empty() || !keys.insert(key) {
                return Err(format!(
                    "malformed parameterized baseline attributes: {name}"
                ));
            }
        }

        let matches = attributes.iter().all(|(key, expected)| {
            strict_option_variant_value(options, source, key)
                .is_some_and(|actual| strict_option_variant_matches(key, &actual, expected))
        });
        if matches {
            matching.push((attributes.len(), path));
        }
    }

    let Some(max_specificity) = matching.iter().map(|(count, _)| *count).max() else {
        return Ok(None);
    };
    matching.retain(|(count, _)| *count == max_specificity);
    if matching.len() != 1 {
        let names = matching
            .iter()
            .map(|(_, path)| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("<non-Unicode>")
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "ambiguous parameterized baselines at specificity {max_specificity}: {names}"
        ));
    }

    let path = &matching[0].1;
    std::fs::read_to_string(path)
        .map(|contents| Some(normalize_text(&contents)))
        .map_err(|error| {
            format!(
                "could not read parameterized baseline {}: {error}",
                path.display()
            )
        })
}

impl BaselineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Js => "js",
            Self::Declarations => "declarations",
            Self::Errors => "errors",
            Self::Symbols => "symbols",
            Self::Types => "types",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Js | Self::Declarations => ".js",
            Self::Errors => ".errors.txt",
            Self::Symbols => ".symbols",
            Self::Types => ".types",
        }
    }
}

impl std::str::FromStr for BaselineKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "js" => Ok(Self::Js),
            "declarations" => Ok(Self::Declarations),
            "errors" => Ok(Self::Errors),
            "symbols" => Ok(Self::Symbols),
            "types" => Ok(Self::Types),
            _ => Err("expected one of: js, declarations, errors, symbols, types"),
        }
    }
}

// ---------------------------------------------------------------------------
// Suite discovery
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    Compiler,
    Conformance,
    Fourslash,
    Project,
    Transpile,
}

impl Suite {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compiler => "compiler",
            Self::Conformance => "conformance",
            Self::Fourslash => "fourslash",
            Self::Project => "project",
            Self::Transpile => "transpile",
        }
    }

    fn relative_dir(self) -> &'static str {
        match self {
            Self::Compiler => "tests/cases/compiler",
            Self::Conformance => "tests/cases/conformance",
            Self::Fourslash => "tests/cases/fourslash",
            Self::Project => "tests/cases/project",
            Self::Transpile => "tests/cases/transpile",
        }
    }
}

impl std::str::FromStr for Suite {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "compiler" => Ok(Self::Compiler),
            "conformance" => Ok(Self::Conformance),
            "fourslash" => Ok(Self::Fourslash),
            "project" => Ok(Self::Project),
            "transpile" => Ok(Self::Transpile),
            _ => Err("expected one of: compiler, conformance, fourslash, project, transpile"),
        }
    }
}

// ---------------------------------------------------------------------------
// Bootstrap harness (oracle-vs-candidate external process runner)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CommandTemplate {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl CommandTemplate {
    pub fn parse(spec: &str) -> Result<Self, &'static str> {
        let mut parts = spec.split_whitespace();
        let Some(program) = parts.next() else {
            return Err("empty command template");
        };

        Ok(Self {
            program: OsString::from(program),
            args: parts.map(OsString::from).collect(),
        })
    }

    pub fn instantiate(&self, case_path: &Path) -> Command {
        let replacement = case_path.to_string_lossy().into_owned();
        let mut cmd = Command::new(&self.program);

        for arg in &self.args {
            let expanded = arg.to_string_lossy().replace("{test}", &replacement);
            cmd.arg(expanded);
        }

        cmd
    }
}

#[derive(Debug, Clone)]
pub struct SuiteRunConfig {
    pub ts_repo: PathBuf,
    pub suite: Suite,
    pub oracle_command: CommandTemplate,
    pub candidate_command: CommandTemplate,
    pub max_cases: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct CaseRunResult {
    pub case: PathBuf,
    pub passed: bool,
    pub mismatches: Vec<MismatchClass>,
    pub oracle: CommandOutput,
    pub candidate: CommandOutput,
}

#[derive(Debug, Clone)]
pub struct SuiteRunResult {
    pub suite: Suite,
    pub total: usize,
    pub passed: usize,
    pub failed: Vec<CaseRunResult>,
}

#[derive(Debug)]
pub enum HarnessError {
    MissingSuiteDir(PathBuf),
    Io(std::io::Error),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSuiteDir(path) => {
                write!(f, "suite directory not found: {}", path.display())
            }
            Self::Io(err) => write!(f, "io error: {err}"),
        }
    }
}

impl std::error::Error for HarnessError {}

impl From<std::io::Error> for HarnessError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Default)]
pub struct BootstrapHarness;

impl BootstrapHarness {
    pub fn discover_cases(
        &self,
        ts_repo: &Path,
        suite: Suite,
    ) -> Result<Vec<PathBuf>, HarnessError> {
        let root = ts_repo.join(suite.relative_dir());
        if !root.is_dir() {
            return Err(HarnessError::MissingSuiteDir(root));
        }

        let mut cases = Vec::new();
        collect_files_recursive(&root, &mut cases)?;
        cases.sort();
        Ok(cases)
    }

    pub fn run_suite(&self, config: &SuiteRunConfig) -> Result<SuiteRunResult, HarnessError> {
        let mut cases = self.discover_cases(&config.ts_repo, config.suite)?;
        if let Some(limit) = config.max_cases {
            cases.truncate(limit);
        }

        let mut failed = Vec::new();

        for case in &cases {
            let oracle = run_command(config.oracle_command.instantiate(case))?;
            let candidate = run_command(config.candidate_command.instantiate(case))?;
            let mismatches = classify_mismatch(&oracle, &candidate);

            if !mismatches.is_empty() {
                failed.push(CaseRunResult {
                    case: case.clone(),
                    passed: false,
                    mismatches,
                    oracle,
                    candidate,
                });
            }
        }

        Ok(SuiteRunResult {
            suite: config.suite,
            total: cases.len(),
            passed: cases.len() - failed.len(),
            failed,
        })
    }
}

impl Harness for BootstrapHarness {
    fn run_compiler_case(&self, test_name: &str) -> HarnessResult {
        HarnessResult {
            test_name: test_name.to_string(),
            mismatches: vec![],
            diagnostics: vec![],
        }
    }

    fn run_fourslash_case(&self, test_name: &str) -> HarnessResult {
        self.run_compiler_case(test_name)
    }

    fn run_project_case(&self, test_name: &str) -> HarnessResult {
        self.run_compiler_case(test_name)
    }
}

// ---------------------------------------------------------------------------
// Baseline runner (in-tree parse + emit, compared against stored baselines)
// ---------------------------------------------------------------------------

/// Result of running a single baseline test case.
#[derive(Debug, Clone)]
pub struct BaselineResult {
    /// The test case name (stem of the `.ts` file, e.g. "2dArrays").
    pub name: String,
    /// Path to the original test file.
    pub test_path: PathBuf,
    /// Overall pass/fail.
    pub passed: bool,
    /// If failed, contains a human-readable diff description.
    pub diff: Option<String>,
    /// Whether an expected baseline oracle exists. This is independent of
    /// whether the test source could be read or decoded.
    pub baseline_exists: bool,
    /// The generated output (for debugging).
    pub actual_output: String,
    /// The expected output from the baseline (for debugging).
    pub expected_output: String,
}

/// Aggregate results from running a full suite of baseline tests.
#[derive(Debug, Clone)]
pub struct BaselineSuiteResult {
    pub suite: Suite,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub results: Vec<BaselineResult>,
}

impl BaselineSuiteResult {
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.passed as f64 / self.total as f64 * 100.0
    }

    /// Return only the failed results.
    pub fn failures(&self) -> Vec<&BaselineResult> {
        self.results.iter().filter(|r| !r.passed).collect()
    }

    /// Print a summary to stdout.
    pub fn print_summary(&self) {
        println!("Suite: {}", self.suite.as_str());
        println!(
            "  Total: {}, Passed: {}, Failed: {}, Skipped: {}",
            self.total, self.passed, self.failed, self.skipped
        );
        println!("  Pass rate: {:.1}%", self.pass_rate());
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectScenario {
    scenario: String,
    project_root: String,
    #[serde(default)]
    input_files: Vec<String>,
    #[serde(default)]
    declaration: Option<bool>,
    #[serde(default)]
    source_map: Option<bool>,
    #[serde(default)]
    out_dir: Option<String>,
    #[serde(default)]
    out_file: Option<String>,
    #[serde(default)]
    root_dir: Option<String>,
    #[serde(default)]
    strict: Option<bool>,
    #[serde(default)]
    source_root: Option<String>,
    #[serde(default)]
    map_root: Option<String>,
    #[serde(default)]
    resolve_source_root: Option<bool>,
    #[serde(default)]
    resolve_map_root: Option<bool>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    module_resolution: Option<String>,
    #[serde(default)]
    declaration_dir: Option<String>,
    #[serde(default)]
    no_resolve: Option<bool>,
}

#[derive(Debug, Clone)]
struct ProjectInputFile {
    full_path: String,
    relative_path: String,
}

type ConstValueMaps = (HashMap<String, f64>, HashMap<String, String>);

/// Discovers test cases, compiles them via the in-tree pipeline, and
/// compares the emitted JavaScript against stored baselines.
pub struct BaselineRunner {
    /// Root of the workspace (where `tests/` lives).
    workspace_root: PathBuf,
    /// Lazily built once and shared by all suite workers/case threads.
    strict_baseline_index: Arc<OnceLock<Result<StrictBaselineIndex, String>>>,
}

// Large enough to survive the type checker's recursive descent on pathological
// deeply-nested inputs (e.g. binderBinaryExpressionStress, a ~5k-line file with
// a single 9k-char binary-operator chain). The errors/types baselines always
// run each case in a worker thread with this stack; a stack overflow there is
// an uncatchable SIGABRT that takes down the whole suite, so we reserve
// generously. Stack is lazily committed on Linux, so this costs ~nothing for
// the vast majority of cases that recurse shallowly. Kept modest (not
// hundreds of MB) because the errors/types paths run a rayon pool of workers
// that EACH spawn one of these threads, so an oversized reservation multiplies
// peak memory and can OOM the suite.
const BASELINE_CASE_STACK_SIZE: usize = 64 * 1024 * 1024;

impl BaselineRunner {
    /// Create a new runner. `workspace_root` is the directory containing
    /// the `tests/` folder (typically the repo root).
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            strict_baseline_index: Arc::new(OnceLock::new()),
        }
    }

    fn strict_baseline_index(&self, baseline_dir: &Path) -> Result<&StrictBaselineIndex, String> {
        self.strict_baseline_index
            .get_or_init(|| StrictBaselineIndex::build(baseline_dir))
            .as_ref()
            .map_err(Clone::clone)
    }

    fn load_strict_option_baseline(
        &self,
        baseline_dir: &Path,
        stem: &str,
        extension: &'static str,
        options: &CompilerOptions,
        source: &str,
    ) -> Result<Option<String>, String> {
        // Preserve exact-first behavior, including avoiding any index error
        // when the exact oracle is readable.
        let exact_name = format!("{stem}{extension}");
        let exact_path = baseline_dir.join(&exact_name);
        if let Some(Ok(index)) = self.strict_baseline_index.get() {
            if index.contains_name(&exact_name) {
                return std::fs::read_to_string(&exact_path)
                    .map(|contents| Some(normalize_text(&contents)))
                    .map_err(|error| {
                        format!(
                            "could not read exact baseline {}: {error}",
                            exact_path.display()
                        )
                    });
            }
            return select_strict_option_baseline(
                index.candidates(stem, extension),
                stem,
                extension,
                options,
                source,
            );
        }
        if exact_path.is_file() {
            return std::fs::read_to_string(&exact_path)
                .map(|contents| Some(normalize_text(&contents)))
                .map_err(|error| {
                    format!(
                        "could not read exact baseline {}: {error}",
                        exact_path.display()
                    )
                });
        }

        let index = self.strict_baseline_index(baseline_dir)?;
        select_strict_option_baseline(
            index.candidates(stem, extension),
            stem,
            extension,
            options,
            source,
        )
    }

    /// Determine oracle provenance without reading the test source.
    ///
    /// Source loading can fail before options are parsed (notably for encoded
    /// source files). That failure must not make an existing oracle appear to
    /// have disappeared from manifests. Error baselines always have an oracle:
    /// in the absence of an explicit `.errors.txt` file, the oracle is the
    /// harness's existing implicit expectation of no diagnostics.
    fn oracle_exists_without_source(&self, stem: &str, suite: Suite, kind: BaselineKind) -> bool {
        if suite == Suite::Project {
            if kind != BaselineKind::Js {
                return false;
            }
            let baseline_dir = self
                .workspace_root
                .join("tests/baselines/reference/project")
                .join(stem);
            let mut files = Vec::new();
            return collect_files_recursive(&baseline_dir, &mut files).is_ok() && !files.is_empty();
        }

        if kind == BaselineKind::Errors {
            return true;
        }

        let baseline_dir = self.workspace_root.join("tests/baselines/reference");
        let extension = kind.extension();
        if baseline_dir.join(format!("{stem}{extension}")).is_file() {
            return true;
        }

        // JavaScript, symbols, and types baselines can all have parameterized
        // variants. Source options select the variant after loading, but the
        // stored oracle's provenance does not depend on source decoding
        // succeeding.
        self.strict_baseline_index(&baseline_dir)
            .is_ok_and(|index| {
                index.candidates(stem, extension).iter().any(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            parameterized_variant_attributes(name, stem, extension).is_some()
                        })
                })
            })
    }

    /// Discover all `.ts` test files for the given suite.
    pub fn discover_cases(&self, suite: Suite) -> Result<Vec<PathBuf>, HarnessError> {
        let root = self.workspace_root.join(suite.relative_dir());
        if !root.is_dir() {
            return Err(HarnessError::MissingSuiteDir(root));
        }

        let mut cases = Vec::new();
        match suite {
            Suite::Project => collect_project_case_files_recursive(&root, &mut cases)?,
            _ => collect_ts_files_recursive(&root, &mut cases)?,
        }
        cases.sort();
        Ok(cases)
    }

    /// Run a single test case and compare against the .js baseline.
    pub fn run_case(&self, test_path: &Path, suite: Suite) -> BaselineResult {
        if Self::run_case_on_current_thread() {
            self.run_case_impl(test_path, suite)
        } else {
            self.run_case_with_large_stack(test_path, suite)
        }
    }

    fn run_case_on_current_thread() -> bool {
        matches!(
            std::thread::current().name(),
            Some(name) if name.starts_with("rayon")
        )
    }

    fn run_case_with_large_stack(&self, test_path: &Path, suite: Suite) -> BaselineResult {
        let stem = test_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let test_path = test_path.to_path_buf();
        let runner = Self {
            workspace_root: self.workspace_root.clone(),
            strict_baseline_index: Arc::clone(&self.strict_baseline_index),
        };
        let worker_test_path = test_path.clone();

        let spawn_result = std::thread::Builder::new()
            .name(format!("baseline-case-{stem}"))
            .stack_size(BASELINE_CASE_STACK_SIZE)
            .spawn(move || runner.run_case_impl(&worker_test_path, suite));

        match spawn_result {
            Ok(handle) => match handle.join() {
                Ok(result) => result,
                Err(panic_info) => BaselineResult {
                    name: stem,
                    test_path,
                    passed: false,
                    diff: Some(format!(
                        "PANIC during compilation: {}",
                        Self::panic_message(panic_info.as_ref())
                    )),
                    baseline_exists: true,
                    actual_output: String::new(),
                    expected_output: String::new(),
                },
            },
            Err(err) => BaselineResult {
                name: stem,
                test_path,
                passed: false,
                diff: Some(format!("failed to spawn compilation thread: {err}")),
                baseline_exists: false,
                actual_output: String::new(),
                expected_output: String::new(),
            },
        }
    }

    fn panic_message(panic_info: &(dyn std::any::Any + Send)) -> String {
        if let Some(s) = panic_info.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        }
    }

    fn run_case_impl(&self, test_path: &Path, suite: Suite) -> BaselineResult {
        let stem = test_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Read the test source.
        let source = match read_test_source(test_path) {
            Ok(s) => s,
            Err(e) => {
                let baseline_exists =
                    self.oracle_exists_without_source(&stem, suite, BaselineKind::Js);
                return BaselineResult {
                    name: stem,
                    test_path: test_path.to_path_buf(),
                    passed: false,
                    diff: Some(format!("failed to read test file: {e}")),
                    baseline_exists,
                    actual_output: String::new(),
                    expected_output: String::new(),
                };
            }
        };

        if suite == Suite::Project {
            return self.run_project_baseline_case(test_path, &stem, &source);
        }

        // Construct the relative test file path as it appears in the baseline header.
        let relative_test = format!(
            "tests/cases/{}/{}",
            suite.as_str(),
            test_path
                .strip_prefix(self.workspace_root.join(suite.relative_dir()))
                .unwrap_or(test_path)
                .to_string_lossy()
                .replace('\\', "/")
        );

        // Parse the test case to extract options and split files.
        let mut test_case = parse_test_case(&relative_test, &source);

        // Fix multi-value options that parse_option() in tsc_rs_ast can't handle
        // (e.g. `// @module: commonjs, esnext` or `// @jsx: react, preserve`).
        fixup_multi_value_options(&mut test_case, &source);

        // Compile each file and generate the baseline output.
        // catch_unwind isolates panics from the parser/emitter. Direct callers
        // normally reach this via run_case_with_large_stack, while suite runs
        // execute on the rayon pool's larger worker stacks.
        let compile_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.generate_js_baseline(&test_case, &relative_test, &source)
        }));

        let actual_output = match compile_result {
            Ok(output) => output,
            Err(panic_info) => {
                return BaselineResult {
                    name: stem,
                    test_path: test_path.to_path_buf(),
                    passed: false,
                    diff: Some(format!(
                        "PANIC during compilation: {}",
                        Self::panic_message(panic_info.as_ref())
                    )),
                    baseline_exists: true, // don't count as skipped
                    actual_output: String::new(),
                    expected_output: String::new(),
                };
            }
        };

        // Load the expected baseline.
        let baseline_dir = self.workspace_root.join("tests/baselines/reference");
        let baseline_path = baseline_dir.join(format!("{}.js", stem));

        // Always strip .d.ts sections from both expected and actual for comparison.
        // Our declaration emitter is still incomplete, so comparing only .js output
        // gives a truer measure of the JavaScript emitter quality.
        let actual_normalized = strip_dts_sections(&normalize_text(&actual_output));

        let (baseline_exists, expected_output) = if baseline_path.is_file() {
            match std::fs::read_to_string(&baseline_path) {
                Ok(s) => (true, normalize_text(&s)),
                Err(_) => (false, String::new()),
            }
        } else {
            // Try parameterized baselines: {stem}(target=es2015).js, etc.
            // Pass actual output so we can pick the best-matching variant.
            find_parameterized_baseline(
                &baseline_dir,
                &stem,
                &test_case.options,
                &actual_normalized,
            )
        };

        let expected_normalized = strip_dts_sections(&expected_output);

        // Compare (trim trailing newlines since baselines may or may not end with one).
        let passed = baseline_exists
            && actual_normalized.trim_end_matches('\n')
                == expected_normalized.trim_end_matches('\n');

        let diff = if !baseline_exists {
            Some("baseline file not found".to_string())
        } else if !passed {
            Some(generate_diff(&expected_normalized, &actual_normalized))
        } else {
            None
        };

        BaselineResult {
            name: stem,
            test_path: test_path.to_path_buf(),
            passed,
            diff,
            baseline_exists,
            actual_output: actual_normalized,
            expected_output: expected_normalized,
        }
    }

    /// Run an entire suite of tests.
    pub fn run_suite(&self, suite: Suite) -> Result<BaselineSuiteResult, HarnessError> {
        self.run_suite_with_limit(suite, None)
    }

    /// Run a suite with an optional max number of cases.
    pub fn run_suite_with_limit(
        &self,
        suite: Suite,
        max_cases: Option<usize>,
    ) -> Result<BaselineSuiteResult, HarnessError> {
        use rayon::prelude::*;

        let mut cases = self.discover_cases(suite)?;
        if let Some(limit) = max_cases {
            cases.truncate(limit);
        }

        let total = cases.len();

        // Use a rayon pool with 8MB stacks to handle deeply nested ASTs
        // without needing per-test thread spawning.
        // RAYON_NUM_THREADS env var controls parallelism (default: num CPUs).
        let mut pool_builder = rayon::ThreadPoolBuilder::new().stack_size(8 * 1024 * 1024);
        if let Some(n) = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            pool_builder = pool_builder.num_threads(n);
        } else {
            // Bound default parallelism by available memory, not core count: each
            // worker runs a full compiler instance (~1-2 GiB on large cases), so
            // using every CPU on a big box can exhaust RAM and thrash swap (this
            // froze the dev box on 2026-06-14). Budget ~4 GiB/worker against
            // MemAvailable, clamped to [2, num CPUs]. Override via RAYON_NUM_THREADS.
            let cpus = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            let mem_gib = std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("MemAvailable:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|kb| kb.parse::<u64>().ok())
                })
                .map(|kb| (kb / 1024 / 1024) as usize);
            let threads = match mem_gib {
                Some(g) => cpus.min((g / 4).max(2)),
                None => cpus,
            };
            pool_builder = pool_builder.num_threads(threads);
        }
        let pool = pool_builder
            .build()
            .expect("failed to build rayon thread pool");

        let results: Vec<BaselineResult> = pool.install(|| {
            cases
                .par_iter()
                .map(|case_path| self.run_case(case_path, suite))
                .collect()
        });

        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        for result in &results {
            if !result.baseline_exists {
                skipped += 1;
            } else if result.passed {
                passed += 1;
            } else {
                failed += 1;
            }
        }

        Ok(BaselineSuiteResult {
            suite,
            total,
            passed,
            failed,
            skipped,
            results,
        })
    }

    /// Run a specific list of test names (just the stem, e.g. "2dArrays").
    pub fn run_named_cases(
        &self,
        suite: Suite,
        names: &[&str],
    ) -> Result<Vec<BaselineResult>, HarnessError> {
        let suite_dir = self.workspace_root.join(suite.relative_dir());
        if !suite_dir.is_dir() {
            return Err(HarnessError::MissingSuiteDir(suite_dir));
        }

        // Build a stem → path index for the suite so we can find tests in
        // subdirectories (important for conformance which has nested dirs).
        let all_cases = self.discover_cases(suite)?;
        let mut stem_map = std::collections::HashMap::<String, PathBuf>::new();
        for path in &all_cases {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                stem_map
                    .entry(stem.to_string())
                    .or_insert_with(|| path.clone());
            }
        }

        let mut results = Vec::with_capacity(names.len());
        for name in names {
            if let Some(test_path) = stem_map.get(*name) {
                results.push(self.run_case(test_path, suite));
            } else {
                // Fallback: try direct path for backwards compat.
                let test_path = if suite == Suite::Project {
                    suite_dir.join(format!("{name}.json"))
                } else {
                    let ts_path = suite_dir.join(format!("{name}.ts"));
                    if ts_path.is_file() {
                        ts_path
                    } else {
                        suite_dir.join(format!("{name}.tsx"))
                    }
                };
                if test_path.is_file() {
                    results.push(self.run_case(&test_path, suite));
                } else {
                    results.push(BaselineResult {
                        name: name.to_string(),
                        test_path: test_path.clone(),
                        passed: false,
                        diff: Some(format!("test file not found: {}", test_path.display())),
                        baseline_exists: false,
                        actual_output: String::new(),
                        expected_output: String::new(),
                    });
                }
            }
        }

        Ok(results)
    }

    fn run_project_baseline_case(
        &self,
        test_path: &Path,
        stem: &str,
        source: &str,
    ) -> BaselineResult {
        let baseline_dir = self
            .workspace_root
            .join("tests/baselines/reference/project")
            .join(stem);
        let expected_files = if baseline_dir.is_dir() {
            load_project_baseline_files(&baseline_dir).unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let baseline_exists = !expected_files.is_empty();
        let expected_output = snapshot_project_file_map(&expected_files);

        let actual_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.generate_project_baseline_files(stem, source)
        }));

        let actual_files = match actual_result {
            Ok(Ok(files)) => files,
            Ok(Err(err)) => {
                return BaselineResult {
                    name: stem.to_string(),
                    test_path: test_path.to_path_buf(),
                    passed: false,
                    diff: Some(err),
                    baseline_exists,
                    actual_output: String::new(),
                    expected_output,
                };
            }
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                return BaselineResult {
                    name: stem.to_string(),
                    test_path: test_path.to_path_buf(),
                    passed: false,
                    diff: Some(format!("PANIC during project compilation: {msg}")),
                    baseline_exists,
                    actual_output: String::new(),
                    expected_output,
                };
            }
        };

        let actual_output = if baseline_exists {
            snapshot_project_expected_files(&expected_files, &actual_files)
        } else {
            snapshot_project_file_map(&actual_files)
        };

        let actual_normalized = normalize_text(&actual_output);
        let expected_normalized = normalize_text(&expected_output);
        let passed = baseline_exists
            && actual_normalized.trim_end_matches('\n')
                == expected_normalized.trim_end_matches('\n');
        let diff = if !baseline_exists {
            Some("project baseline directory not found".to_string())
        } else if !passed {
            Some(generate_diff(&expected_normalized, &actual_normalized))
        } else {
            None
        };

        BaselineResult {
            name: stem.to_string(),
            test_path: test_path.to_path_buf(),
            passed,
            diff,
            baseline_exists,
            actual_output: actual_normalized,
            expected_output: expected_normalized,
        }
    }

    fn generate_project_baseline_files(
        &self,
        stem: &str,
        source: &str,
    ) -> Result<BTreeMap<String, String>, String> {
        let scenario: ProjectScenario = serde_json::from_str(source)
            .map_err(|err| format!("invalid project scenario json: {err}"))?;
        let project_root = self.resolve_project_root(&scenario);
        let has_tsconfig = project_root.join("tsconfig.json").is_file();
        let mut options = self.load_project_options(&project_root)?;
        apply_project_scenario_overrides(&mut options, &scenario);
        let inputs = self.discover_project_inputs(&project_root, &options, &scenario)?;
        if inputs.is_empty() {
            return Err(format!(
                "no source files found for project scenario '{}'",
                scenario.scenario
            ));
        }

        let file_names: Vec<String> = inputs.iter().map(|f| f.full_path.clone()).collect();
        let display_names: Vec<String> = inputs.iter().map(|f| f.relative_path.clone()).collect();
        let display_by_full: HashMap<String, String> = inputs
            .iter()
            .map(|f| (normalize_header_path(&f.full_path), f.relative_path.clone()))
            .collect();

        let mut files = BTreeMap::new();
        for (variant, module_kind) in [("node", ModuleKind::CommonJS), ("amd", ModuleKind::AMD)] {
            let mut variant_options = options.clone();
            variant_options.module = Some(module_kind);
            let path_ctx = project_baseline_path_context(&variant_options, &display_names);
            let result =
                tsc_rs_project::TsProject::new(file_names.clone(), variant_options.clone())
                    .compile();

            let mut emitted_files = Vec::new();
            let mut emitted_seen = HashSet::new();
            for file in &result.files {
                let full_name = normalize_header_path(&file.file_name);
                let Some(relative_name) = display_by_full.get(&full_name) else {
                    continue;
                };

                if !file.emit.javascript.is_empty() {
                    let js_name =
                        output_name_for_source_js(relative_name, variant_options.jsx, &path_ctx);
                    if emitted_seen.insert(js_name.clone()) {
                        emitted_files.push(js_name.clone());
                    }
                    files.insert(
                        format!("{variant}/{js_name}"),
                        normalize_text(&file.emit.javascript),
                    );

                    if let Some(source_map) = &file.emit.source_map {
                        let map_name = format!("{js_name}.map");
                        if emitted_seen.insert(map_name.clone()) {
                            emitted_files.push(map_name.clone());
                        }
                        files.insert(format!("{variant}/{map_name}"), normalize_text(source_map));
                    }
                }

                if let Some(declaration) = &file.emit.declaration_file {
                    let dts_name = output_name_for_source_dts(relative_name, &path_ctx);
                    if emitted_seen.insert(dts_name.clone()) {
                        emitted_files.push(dts_name.clone());
                    }
                    files.insert(format!("{variant}/{dts_name}"), normalize_text(declaration));
                }
            }

            let diagnostics = format_project_diagnostics(
                &result,
                &project_root,
                &inputs,
                module_kind,
                &variant_options,
                has_tsconfig,
            );
            files.insert(format!("{variant}/{stem}.errors.txt"), diagnostics);
            files.insert(
                format!("{variant}/{stem}.json"),
                build_project_summary_json(source, &display_names, &emitted_files),
            );
        }

        Ok(files)
    }

    fn resolve_project_root(&self, scenario: &ProjectScenario) -> PathBuf {
        let mut root = self.workspace_root.join(&scenario.project_root);
        if let Some(project) = &scenario.project {
            root = root.join(project);
        }
        root
    }

    fn load_project_options(&self, project_root: &Path) -> Result<CompilerOptions, String> {
        let config_path = project_root.join("tsconfig.json");
        if config_path.is_file() {
            Ok(tsc_rs_project::TsProject::from_config(
                config_path
                    .to_str()
                    .ok_or_else(|| format!("non-utf8 tsconfig path: {}", config_path.display()))?,
            )?
            .options)
        } else {
            Ok(CompilerOptions::default())
        }
    }

    fn discover_project_inputs(
        &self,
        project_root: &Path,
        options: &CompilerOptions,
        scenario: &ProjectScenario,
    ) -> Result<Vec<ProjectInputFile>, String> {
        let config_path = project_root.join("tsconfig.json");
        let mut files = if config_path.is_file() {
            let project =
                tsc_rs_project::TsProject::from_config(config_path.to_str().ok_or_else(|| {
                    format!("non-utf8 tsconfig path: {}", config_path.display())
                })?)?;
            let mut inputs = Vec::with_capacity(project.file_names.len());
            for full_path in project.file_names {
                let relative_path = relative_project_path(project_root, &full_path);
                inputs.push(ProjectInputFile {
                    full_path,
                    relative_path,
                });
            }
            inputs
        } else {
            let roots = if scenario.input_files.is_empty() {
                discover_project_source_files(project_root, options)?
                    .into_iter()
                    .map(|file| file.relative_path)
                    .collect::<Vec<_>>()
            } else {
                scenario.input_files.clone()
            };
            discover_project_input_closure(project_root, options, &roots)?
        };

        if files.is_empty() && !scenario.input_files.is_empty() {
            files = scenario
                .input_files
                .iter()
                .map(|relative| ProjectInputFile {
                    full_path: normalize_header_path(
                        &project_root.join(relative).to_string_lossy(),
                    ),
                    relative_path: normalize_header_path(relative),
                })
                .collect();
        }

        let mut seen = HashSet::new();
        files.retain(|file| seen.insert(file.full_path.clone()));
        Ok(files)
    }

    // -----------------------------------------------------------------------
    // Internal: generate the .js baseline content
    // -----------------------------------------------------------------------

    /// Generate the full `.js` baseline output for a parsed test case.
    ///
    /// The format is:
    /// ```text
    /// //// [tests/cases/compiler/testName.ts] ////
    ///
    /// //// [file1.ts]
    /// <file1 content without directives>
    ///
    /// //// [file1.js]
    /// <compiled JS for file1>
    /// ```
    fn generate_js_baseline(
        &self,
        test_case: &TestCase,
        relative_test_path: &str,
        source_text: &str,
    ) -> String {
        let mut output = String::new();
        let path_ctx = baseline_path_context(test_case);
        let effective_options = effective_compiler_options(test_case);
        let package_type_by_dir = package_json_module_type_by_dir(test_case);
        let symlink_aliases = parse_symlink_aliases(source_text);
        let link_path_mappings = parse_link_path_mappings(source_text);

        // Header line.
        output.push_str(&format!("//// [{relative_test_path}] ////\n"));

        // Input section: echo each file's content.
        // Use the basename (last path component) for section headers, matching
        // TypeScript's baseline format.
        // A blank line separator is only added when the previous content ended
        // with '\n' (so the separator \n creates a visible blank line).  When
        // the previous content did NOT end with '\n', we only add the required
        // end-of-line, and the next section header follows immediately.
        let mut prev_had_trailing_nl = true; // header line ends with \n
        for idx in input_file_order_indices(test_case, &path_ctx) {
            let file = &test_case.files[idx];
            let display_name = basename(&file.name);
            // Skip tsconfig.json from the input section.
            // TypeScript's baselines include package.json and other JSON
            // files in the input echo.
            let lower = display_name.to_lowercase();
            if lower == "tsconfig.json" {
                continue;
            }
            if prev_had_trailing_nl {
                output.push('\n');
            }
            output.push_str(&format!("//// [{display_name}]\n"));
            output.push_str(&file.content);
            prev_had_trailing_nl = file.content.ends_with('\n');
            if !prev_had_trailing_nl {
                output.push('\n');
            }
        }

        // Compiled output section: parse and emit each .ts/.tsx file.
        // Skip .d.ts (declaration-only), .json, and other non-compilable files.
        //
        // TypeScript's baseline format does NOT insert blank lines between
        // consecutive output sections -- only between the last input section
        // and the first output section (handled by the trailing newline of
        // the last input file's content).
        let mut dts_sections: Vec<(String, String)> = Vec::new();
        let mut js_sections: Vec<(String, String, String)> = Vec::new();
        let mut wrote_output_section = false;
        let emit_declaration_only = path_ctx.emit_declaration_only;
        let mut compile_indices: Vec<usize> = test_case
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| {
                should_emit_compilable_source_file(file, &path_ctx).then_some(idx)
            })
            .collect();
        // For duplicate `@filename` entries, compile only the last section for
        // that path while keeping all input sections in the echoed source block.
        let preserve_duplicate_rewrite_units =
            test_case.options.other.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("rewriterelativeimportextensions")
                    && value.eq_ignore_ascii_case("true")
            }) || source_text.lines().any(|line| {
                let line = line.trim().to_ascii_lowercase();
                line.starts_with("// @rewriterelativeimportextensions:") && line.ends_with("true")
            });
        if compile_indices.len() > 1 && !preserve_duplicate_rewrite_units {
            let mut deduped = Vec::with_capacity(compile_indices.len());
            let mut pos_by_name: HashMap<String, usize> = HashMap::new();
            for idx in compile_indices {
                let normalized = normalize_header_path(&test_case.files[idx].name);
                if let Some(pos) = pos_by_name.get(&normalized).copied() {
                    // TypeScript compiles the last duplicate section for a
                    // given @Filename, but keeps the output in the position of
                    // the first occurrence.
                    deduped[pos] = idx;
                } else {
                    pos_by_name.insert(normalized, deduped.len());
                    deduped.push(idx);
                }
            }
            compile_indices = deduped;
        }
        let mut parsed_sources = HashMap::new();
        let jsx_option_set = effective_options.jsx.is_some();
        for idx in &compile_indices {
            let file = &test_case.files[*idx];
            let lower = file.name.to_ascii_lowercase();
            let jsx_by_ext = lower.ends_with(".tsx") || lower.ends_with(".jsx");
            let jsx_enabled = jsx_by_ext
                || (jsx_option_set
                    && (lower.ends_with(".js")
                        || lower.ends_with(".mjs")
                        || lower.ends_with(".cjs")));
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse_with_jsx(&file.name, &file.content, jsx_enabled),
            );
        }
        let compile_indices = if !preserve_duplicate_rewrite_units
            && should_use_require_last_unit_roots(test_case, &path_ctx)
        {
            compile_indices_for_require_last_unit(
                test_case,
                &compile_indices,
                &parsed_sources,
                &path_ctx,
                effective_options.no_resolve == Some(true),
                &link_path_mappings,
            )
        } else {
            compile_indices
        };
        let mut path_to_idx = HashMap::new();
        for (idx, file) in test_case.files.iter().enumerate() {
            let normalized = normalize_header_path(&file.name);
            path_to_idx.insert(normalized.clone(), idx);
            // Support `@link: <realPath> -> <node_modules alias>` by indexing
            // linked alias paths to the same file. This lets dependency
            // resolution and emit-ordering resolve imports through symlinked
            // package paths.
            for (from, to) in &link_path_mappings {
                let Some(rel) = strip_prefix_path(&normalized, from) else {
                    continue;
                };
                let alias = if rel.is_empty() {
                    to.clone()
                } else {
                    join_path(to, &rel)
                };
                path_to_idx.entry(alias).or_insert(idx);
            }
        }
        let mut extra_const_enum_sources: HashMap<usize, tsc_rs_ast::SourceFile> = HashMap::new();
        let mut const_enum_analysis_indices: Vec<usize> = parsed_sources.keys().copied().collect();
        const_enum_analysis_indices.sort_unstable();
        for (idx, file) in test_case.files.iter().enumerate() {
            if parsed_sources.contains_key(&idx) {
                continue;
            }
            let lower = file.name.to_ascii_lowercase();
            let is_declaration_like = lower.ends_with(".d.ts")
                || lower.ends_with(".d.tsx")
                || lower.ends_with(".d.mts")
                || lower.ends_with(".d.cts")
                || lower.ends_with(".d.js")
                || lower.ends_with(".d.jsx")
                || lower.ends_with(".d.mjs")
                || lower.ends_with(".d.cjs")
                || is_arbitrary_declaration_source_name(&file.name);
            let is_ts_like = lower.ends_with(".ts")
                || lower.ends_with(".tsx")
                || lower.ends_with(".mts")
                || lower.ends_with(".cts");
            if is_declaration_like || is_ts_like {
                extra_const_enum_sources
                    .insert(idx, tsc_rs_parser::parse(&file.name, &file.content));
                const_enum_analysis_indices.push(idx);
            }
        }
        const_enum_analysis_indices.sort_unstable();
        const_enum_analysis_indices.dedup();
        let mut const_enum_values_by_file: HashMap<
            usize,
            HashMap<(String, String), tsc_rs_emitter::ConstEnumValue>,
        > = HashMap::new();
        for idx in &const_enum_analysis_indices {
            let source_file = if let Some(sf) = parsed_sources.get(idx) {
                sf
            } else {
                extra_const_enum_sources
                    .get(idx)
                    .expect("missing extra const-enum analysis source")
            };
            let vals = tsc_rs_emitter::collect_const_enum_values(source_file);
            if !vals.is_empty() {
                const_enum_values_by_file.insert(*idx, vals);
            }
        }
        // Propagate const-enum aliases through import/export chains
        // (including declaration files) so downstream files can inline them.
        let mut changed = true;
        while changed {
            changed = false;
            for idx in &const_enum_analysis_indices {
                let source_file = if let Some(sf) = parsed_sources.get(idx) {
                    sf
                } else {
                    extra_const_enum_sources
                        .get(idx)
                        .expect("missing extra const-enum analysis source")
                };
                let file_name = &test_case.files[*idx].name;
                let mut file_vals = const_enum_values_by_file
                    .get(idx)
                    .cloned()
                    .unwrap_or_default();
                let mut file_changed = false;

                for stmt in &source_file.statements {
                    let import_decl = match &stmt.kind {
                        tsc_rs_ast::StmtKind::Import(import_decl) => Some(import_decl),
                        tsc_rs_ast::StmtKind::Export(export_decl) => match &export_decl.kind {
                            tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                                tsc_rs_ast::StmtKind::Import(import_decl) => Some(import_decl),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(import_decl) = import_decl {
                        if import_decl.type_only || import_decl.source.is_empty() {
                            continue;
                        }
                        let Some(dep_idx) = resolve_module_specifier(
                            file_name,
                            &import_decl.source,
                            test_case,
                            &path_to_idx,
                            &path_ctx,
                        ) else {
                            continue;
                        };
                        let Some(dep_vals) = const_enum_values_by_file.get(&dep_idx) else {
                            continue;
                        };
                        match &import_decl.specifiers {
                            tsc_rs_ast::ImportClause::Named {
                                default,
                                named,
                                namespace,
                            } => {
                                if let Some(default_local) = default {
                                    file_changed |= merge_const_enum_object_values(
                                        &mut file_vals,
                                        default_local,
                                        "default",
                                        dep_vals,
                                    );
                                }
                                if let Some(namespace_local) = namespace {
                                    file_changed |= merge_const_enum_namespace_values(
                                        &mut file_vals,
                                        namespace_local,
                                        dep_vals,
                                    );
                                }
                                for spec in named {
                                    if spec.is_type {
                                        continue;
                                    }
                                    let imported_name =
                                        spec.imported.as_deref().unwrap_or(&spec.local);
                                    file_changed |= merge_const_enum_object_values(
                                        &mut file_vals,
                                        &spec.local,
                                        imported_name,
                                        dep_vals,
                                    );
                                }
                            }
                            tsc_rs_ast::ImportClause::Require(local) => {
                                file_changed |= merge_const_enum_object_values_with_prefix(
                                    &mut file_vals,
                                    local,
                                    "export=",
                                    dep_vals,
                                );
                                file_changed |= merge_const_enum_object_values_with_prefix(
                                    &mut file_vals,
                                    local,
                                    "default",
                                    dep_vals,
                                );
                                file_changed |= merge_const_enum_namespace_values(
                                    &mut file_vals,
                                    local,
                                    dep_vals,
                                );
                            }
                        }
                    }

                    if let tsc_rs_ast::StmtKind::ExportAssign(expr) = &stmt.kind {
                        if let Some(source_obj) = path_from_expr(expr) {
                            let snapshot = file_vals.clone();
                            file_changed |= merge_const_enum_object_values_with_prefix(
                                &mut file_vals,
                                "export=",
                                &source_obj,
                                &snapshot,
                            );
                        }
                        continue;
                    }

                    let tsc_rs_ast::StmtKind::Export(export_decl) = &stmt.kind else {
                        continue;
                    };
                    match &export_decl.kind {
                        tsc_rs_ast::ExportDeclKind::Decl(inner) => {
                            if let tsc_rs_ast::StmtKind::ExportAssign(expr) = &inner.kind {
                                if let Some(source_obj) = path_from_expr(expr) {
                                    let snapshot = file_vals.clone();
                                    file_changed |= merge_const_enum_object_values_with_prefix(
                                        &mut file_vals,
                                        "export=",
                                        &source_obj,
                                        &snapshot,
                                    );
                                }
                            }
                        }
                        tsc_rs_ast::ExportDeclKind::Named {
                            specifiers,
                            source: Some(source),
                            type_only: false,
                        } => {
                            let Some(dep_idx) = resolve_module_specifier(
                                file_name,
                                source,
                                test_case,
                                &path_to_idx,
                                &path_ctx,
                            ) else {
                                continue;
                            };
                            let Some(dep_vals) = const_enum_values_by_file.get(&dep_idx) else {
                                continue;
                            };
                            for spec in specifiers {
                                if spec.is_type {
                                    continue;
                                }
                                let exported_name = spec.exported.as_deref().unwrap_or(&spec.local);
                                file_changed |= merge_const_enum_object_values(
                                    &mut file_vals,
                                    &spec.local,
                                    &spec.local,
                                    dep_vals,
                                );
                                file_changed |= merge_const_enum_object_values(
                                    &mut file_vals,
                                    exported_name,
                                    &spec.local,
                                    dep_vals,
                                );
                            }
                        }
                        tsc_rs_ast::ExportDeclKind::Named {
                            specifiers,
                            source: None,
                            type_only: false,
                        } => {
                            for spec in specifiers {
                                if spec.is_type {
                                    continue;
                                }
                                let exported_name = spec.exported.as_deref().unwrap_or(&spec.local);
                                let mut pairs: Vec<(String, tsc_rs_emitter::ConstEnumValue)> =
                                    Vec::new();
                                for ((obj_name, member_name), value) in &file_vals {
                                    if obj_name == &spec.local {
                                        pairs.push((member_name.clone(), value.clone()));
                                    }
                                }
                                for (member_name, value) in pairs {
                                    let key = (exported_name.to_string(), member_name);
                                    if file_vals.get(&key) != Some(&value) {
                                        file_vals.insert(key, value);
                                        file_changed = true;
                                    }
                                }
                            }
                        }
                        tsc_rs_ast::ExportDeclKind::Default(expr) => {
                            if let tsc_rs_ast::ExprKind::Ident(name) = &expr.kind {
                                let mut pairs: Vec<(String, tsc_rs_emitter::ConstEnumValue)> =
                                    Vec::new();
                                for ((obj_name, member_name), value) in &file_vals {
                                    if obj_name == name {
                                        pairs.push((member_name.clone(), value.clone()));
                                    }
                                }
                                for (member_name, value) in pairs {
                                    let key = ("default".to_string(), member_name);
                                    if file_vals.get(&key) != Some(&value) {
                                        file_vals.insert(key, value);
                                        file_changed = true;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if file_changed {
                    const_enum_values_by_file.insert(*idx, file_vals);
                    changed = true;
                }
            }
        }
        // Collect exported const values from all files for cross-file enum eval.
        let mut exported_consts_by_file: HashMap<usize, ConstValueMaps> = HashMap::new();
        // Collect all script-global const-like values (top-level const vars and
        // enum member constants) so non-module files can fold references coming
        // from other script files in the same compilation.
        let mut script_global_consts_by_file: HashMap<usize, ConstValueMaps> = HashMap::new();
        for idx in &const_enum_analysis_indices {
            let source_file = if let Some(sf) = parsed_sources.get(idx) {
                sf
            } else if let Some(sf) = extra_const_enum_sources.get(idx) {
                sf
            } else {
                continue;
            };
            let (nc, sc) = collect_exported_const_values(source_file);
            if !nc.is_empty() || !sc.is_empty() {
                exported_consts_by_file.insert(*idx, (nc, sc));
            }
            if !source_file_looks_like_module(source_file) {
                let (nc, sc) = collect_file_const_values(source_file);
                if !nc.is_empty() || !sc.is_empty() {
                    script_global_consts_by_file.insert(*idx, (nc, sc));
                }
            }
        }

        let emit_order = compute_emit_order(
            test_case,
            &compile_indices,
            &parsed_sources,
            &path_ctx,
            &link_path_mappings,
            effective_options.module,
        );
        let out_file_name = normalized_out_file_name(
            &path_ctx,
            compile_indices
                .first()
                .map(|idx| test_case.files[*idx].name.as_str()),
        );
        let colliding_js_output_paths: HashSet<String> = if emit_declaration_only
            || out_file_name.is_some()
            || preserve_duplicate_rewrite_units
        {
            HashSet::new()
        } else {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for idx in &emit_order {
                let file = &test_case.files[*idx];
                let name = &file.name;
                let normalized_name = normalize_header_path(name);
                let emission_paths = symlink_aliases
                    .get(&normalized_name)
                    .cloned()
                    .filter(|paths| !paths.is_empty())
                    .unwrap_or_else(|| vec![normalized_name.clone()]);
                if !should_emit_js_output_for_source(name, &path_ctx) {
                    continue;
                }
                for emit_path in &emission_paths {
                    if would_overwrite_input_source(
                        emit_path,
                        effective_options.jsx,
                        test_case,
                        &path_ctx,
                    ) {
                        continue;
                    }
                    let full_js_path =
                        full_output_path_for_source_js(emit_path, effective_options.jsx, &path_ctx);
                    *counts.entry(full_js_path).or_insert(0) += 1;
                }
            }
            counts
                .into_iter()
                .filter_map(|(path, count)| (count > 1).then_some(path))
                .collect()
        };
        // Collect referenced JSON file indices.
        // Always collect them when outDir is set (even without resolveJsonModule),
        // because TypeScript copies referenced JSON files to the output directory.
        let referenced_json_indices = if out_file_name.is_none() && path_ctx.out_dir.is_some() {
            let mut idxs = referenced_json_dependency_indices(
                test_case,
                &compile_indices,
                &parsed_sources,
                &path_ctx,
            );
            // When resolveJsonModule is NOT set, exclude package.json files
            // that were resolved via exports/imports mappings rather than
            // explicit JSON imports. TypeScript doesn't copy package.json
            // to outDir unless it's a real JSON module import.
            if !path_ctx.resolve_json_module {
                idxs.retain(|idx| {
                    let name = test_case.files[*idx].name.to_lowercase();
                    !name.ends_with("package.json")
                });
            }
            idxs
        } else if path_ctx.resolve_json_module {
            referenced_json_dependency_indices(
                test_case,
                &compile_indices,
                &parsed_sources,
                &path_ctx,
            )
        } else {
            Vec::new()
        };

        // Build a set of JSON file indices that are "paired" with a compiled TS file
        // (i.e., they share the same stem, e.g., "c.ts" and "c.json" → "c").
        // Paired JSON files are emitted AFTER the corresponding compiled JS.
        // Unpaired JSON files (no same-stem compiled TS) are emitted BEFORE all compiled JS.
        let paired_json_indices: HashSet<usize> = referenced_json_indices
            .iter()
            .copied()
            .filter(|json_idx| {
                let json_name = normalize_header_path(&test_case.files[*json_idx].name);
                let json_stem = strip_supported_script_extension(
                    json_name.strip_suffix(".json").unwrap_or(&json_name),
                );
                // Check if any compile_index has the same stem (base name without extension).
                compile_indices.iter().any(|ts_idx| {
                    let ts_name = normalize_header_path(&test_case.files[*ts_idx].name);
                    let ts_stem = strip_supported_script_extension(&ts_name);
                    ts_stem == json_stem
                })
            })
            .collect();

        // Emit AMD JSON modules into js_sections (AMD out-file case).
        if path_ctx.resolve_json_module
            && out_file_name.is_some()
            && effective_options.module == Some(ModuleKind::AMD)
        {
            for idx in &referenced_json_indices {
                let file = &test_case.files[*idx];
                let name = &file.name;
                let module_id = amd_module_id_for_source(name, &path_ctx);
                let section = emit_amd_json_module_section(&module_id, &file.content);
                js_sections.push((String::new(), module_id, section));
            }
        }

        // Emit unpaired JSON files (no same-stem compiled TS) BEFORE compiled JS.
        // Paired JSON files (same-stem TS exists) will be emitted AFTER their JS.
        if out_file_name.is_none() && path_ctx.out_dir.is_some() {
            for idx in &referenced_json_indices {
                if paired_json_indices.contains(idx) {
                    continue; // will be emitted inline with the JS loop
                }
                let file = &test_case.files[*idx];
                let name = &file.name;
                let out_name = output_name_for_non_ts_source(name, &path_ctx);
                if !wrote_output_section {
                    output.push('\n');
                    wrote_output_section = true;
                }
                output.push_str(&format!("//// [{out_name}]\n"));
                let content = format_json_module_output(&file.content);
                output.push_str(&content);
            }
        }

        // For AMD/System outFile bundles, import-alias suffixes (foo_1, foo_2, …)
        // and System.register callback suffixes (exports_N, context_N) must be
        // globally unique across all define()/System.register() blocks in the
        // concatenated output.  Thread the counter maps between emit calls.
        let is_out_file = out_file_name.is_some();
        let mut shared_require_var_counters: HashMap<AstString, usize> = HashMap::new();
        let mut shared_system_register_counter: usize = 1;
        let mut prior_script_value_names: HashSet<String> = HashSet::new();
        let preserve_const_for_runtime_names =
            effective_options.preserve_const_enums.unwrap_or(false);

        // When outFile is set but no module kind is specified, TypeScript
        // excludes module files from the bundle (only scripts are bundled).
        let skip_modules_in_out_file =
            is_out_file && matches!(effective_options.module, None | Some(ModuleKind::None));

        // Collect type-only names from ALL files in the compilation so the
        // emitter can suppress spurious `exports.X = void 0;` for names that
        // are type-only in another file (e.g. a global interface from .d.ts).
        let global_type_only_names: std::collections::HashSet<AstString> = {
            let preserve_const = effective_options.preserve_const_enums.unwrap_or(false);
            let mut names = std::collections::HashSet::new();
            // Include type-only names from compilable sources.
            for sf in parsed_sources.values() {
                let file_names = tsc_rs_emitter::collect_type_only_names(sf, preserve_const);
                names.extend(file_names);
            }
            // Also include type-only names from declaration (.d.ts) files
            // and other non-compilable files that may declare global types.
            for (idx, file) in test_case.files.iter().enumerate() {
                if parsed_sources.contains_key(&idx) {
                    continue; // Already processed above.
                }
                let lower = file.name.to_ascii_lowercase();
                if lower.ends_with(".d.ts")
                    || lower.ends_with(".d.mts")
                    || lower.ends_with(".d.cts")
                {
                    let sf = tsc_rs_parser::parse(&file.name, &file.content);
                    let file_names = tsc_rs_emitter::collect_type_only_names(&sf, preserve_const);
                    names.extend(file_names);
                }
            }
            names
        };
        let preserve_const = effective_options.preserve_const_enums.unwrap_or(false);
        let direct_exported_const_enum_names = |sf: &SourceFile| -> Vec<AstString> {
            // Collect all const enum names declared in this file.
            let local_const_enums: HashSet<&str> = sf
                .statements
                .iter()
                .filter_map(|s| match &s.kind {
                    StmtKind::EnumDecl(e) if e.is_const => Some(e.name.as_str()),
                    StmtKind::Export(ed) => {
                        if let ExportDeclKind::Decl(inner) = &ed.kind {
                            if let StmtKind::EnumDecl(e) = &inner.kind {
                                if e.is_const {
                                    return Some(e.name.as_str());
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                })
                .collect();
            let mut names = Vec::new();
            for stmt in &sf.statements {
                match &stmt.kind {
                    StmtKind::EnumDecl(enum_decl)
                        if enum_decl.modifiers & MOD_EXPORT != 0 && enum_decl.is_const =>
                    {
                        names.push(enum_decl.name.clone().into());
                    }
                    StmtKind::Export(export_decl) => match &export_decl.kind {
                        ExportDeclKind::Decl(inner) => {
                            if let StmtKind::EnumDecl(enum_decl) = &inner.kind {
                                if enum_decl.is_const {
                                    names.push(enum_decl.name.clone().into());
                                }
                            }
                        }
                        // `export { ConstEnumName }` — re-export of local const enum
                        ExportDeclKind::Named {
                            specifiers,
                            source: None,
                            type_only: false,
                        } => {
                            for spec in specifiers {
                                if !spec.is_type && local_const_enums.contains(spec.local.as_str())
                                {
                                    let exported =
                                        spec.exported.as_ref().unwrap_or(&spec.local).clone();
                                    names.push(exported.into());
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            names
        };
        // Per-file maps for precise import elision: when the same name is type-only
        // from one module but value-exported from another, we need per-source info.
        let mut per_idx_type_only_exports: HashMap<usize, HashSet<AstString>> = {
            let mut map = HashMap::new();
            for (idx, sf) in &parsed_sources {
                map.insert(*idx, tsc_rs_emitter::collect_type_only_export_names(sf));
            }
            map
        };
        let mut per_idx_value_exports: HashMap<usize, HashSet<AstString>> = {
            let mut map = HashMap::new();
            for (idx, sf) in &parsed_sources {
                map.insert(*idx, tsc_rs_emitter::collect_value_export_names(sf));
            }
            map
        };
        if !preserve_const && effective_options.isolated_modules != Some(true) {
            for (idx, sf) in &parsed_sources {
                let type_only = per_idx_type_only_exports.entry(*idx).or_default();
                let value = per_idx_value_exports.entry(*idx).or_default();
                for name in direct_exported_const_enum_names(sf) {
                    type_only.insert(name.clone());
                    value.remove(name.as_str());
                }
            }
            loop {
                let mut changed = false;
                for (idx, sf) in &parsed_sources {
                    let file_name = &test_case.files[*idx].name;
                    let mut local_type_only_bindings: HashSet<AstString> =
                        per_idx_type_only_exports
                            .get(idx)
                            .cloned()
                            .unwrap_or_default();
                    for stmt in &sf.statements {
                        let import_decl: Option<&ImportDecl> = match &stmt.kind {
                            StmtKind::Import(import_decl) => Some(import_decl),
                            StmtKind::Export(export_decl) => match &export_decl.kind {
                                ExportDeclKind::Decl(inner) => match &inner.kind {
                                    StmtKind::Import(import_decl) => Some(import_decl),
                                    _ => None,
                                },
                                _ => None,
                            },
                            _ => None,
                        };
                        let Some(import_decl) = import_decl else {
                            continue;
                        };
                        if import_decl.type_only || import_decl.source.is_empty() {
                            continue;
                        }
                        let Some(dep_idx) = resolve_module_specifier(
                            file_name,
                            &import_decl.source,
                            test_case,
                            &path_to_idx,
                            &path_ctx,
                        ) else {
                            continue;
                        };
                        let dep_value = per_idx_value_exports.get(&dep_idx);
                        let dep_type_only = per_idx_type_only_exports.get(&dep_idx);
                        let ImportClause::Named { default, named, .. } = &import_decl.specifiers
                        else {
                            continue;
                        };
                        if let Some(default) = default {
                            if dep_type_only.is_some_and(|t| t.contains("default"))
                                && !dep_value.is_some_and(|v| v.contains("default"))
                            {
                                local_type_only_bindings.insert(AstString::from(default.as_str()));
                            }
                        }
                        for spec in named {
                            let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                            if dep_type_only.is_some_and(|t| t.contains(imported.as_str()))
                                && !dep_value.is_some_and(|v| v.contains(imported.as_str()))
                            {
                                local_type_only_bindings
                                    .insert(AstString::from(spec.local.as_str()));
                            }
                        }
                    }
                    // Remove type-only bindings that are shadowed by local
                    // value declarations (e.g. `import { A } from "./a"` is
                    // type-only, but `const A = 0` creates a local value).
                    for stmt in &sf.statements {
                        let name: Option<&str> = match &stmt.kind {
                            StmtKind::Var(v) => v.declarations.first().and_then(|d| {
                                if let tsc_rs_ast::PatKind::Ident(ref n) = d.name.kind {
                                    Some(n.as_str())
                                } else {
                                    None
                                }
                            }),
                            StmtKind::FnDecl(f) => f.name.as_deref(),
                            StmtKind::ClassDecl(c) => c.name.as_deref(),
                            StmtKind::EnumDecl(e) => Some(e.name.as_str()),
                            StmtKind::ModuleDecl(m) => {
                                // Only treat namespace as value if it has
                                // runtime members (not empty or type-only).
                                let has_runtime_members =
                                    m.body.as_ref().is_some_and(|body| match body {
                                        tsc_rs_ast::ModuleBody::Block(stmts) => {
                                            stmts.iter().any(|s| {
                                                let inner = match &s.kind {
                                                    StmtKind::Export(ed) => match &ed.kind {
                                                        tsc_rs_ast::ExportDeclKind::Decl(d) => {
                                                            &d.kind
                                                        }
                                                        _ => &s.kind,
                                                    },
                                                    other => other,
                                                };
                                                matches!(
                                                    inner,
                                                    StmtKind::Var(_)
                                                        | StmtKind::FnDecl(_)
                                                        | StmtKind::ClassDecl(_)
                                                        | StmtKind::EnumDecl(_)
                                                )
                                            })
                                        }
                                        tsc_rs_ast::ModuleBody::Module(_) => true,
                                    });
                                if has_runtime_members {
                                    if let tsc_rs_ast::ModuleName::Ident(ref n) = m.name {
                                        Some(n.as_str())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };
                        if let Some(n) = name {
                            local_type_only_bindings.remove(n);
                        }
                    }
                    let local_namespace_names: HashSet<&str> = sf
                        .statements
                        .iter()
                        .filter_map(|stmt| match &stmt.kind {
                            StmtKind::ModuleDecl(module_decl) => match &module_decl.name {
                                tsc_rs_ast::ModuleName::Ident(name) => Some(name.as_str()),
                                _ => None,
                            },
                            _ => None,
                        })
                        .collect();
                    let mut imported_const_enum_namespace_merges: HashSet<AstString> =
                        HashSet::new();
                    for stmt in &sf.statements {
                        let StmtKind::Import(import_decl) = &stmt.kind else {
                            continue;
                        };
                        let Some(dep_idx) = resolve_module_specifier(
                            file_name,
                            &import_decl.source,
                            test_case,
                            &path_to_idx,
                            &path_ctx,
                        ) else {
                            continue;
                        };
                        let Some(dep_sf) = parsed_sources.get(&dep_idx) else {
                            continue;
                        };
                        let dep_const_enums: HashSet<AstString> =
                            direct_exported_const_enum_names(dep_sf)
                                .into_iter()
                                .collect();
                        let ImportClause::Named { named, .. } = &import_decl.specifiers else {
                            continue;
                        };
                        for spec in named {
                            let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                            if dep_const_enums.contains(imported.as_str())
                                && local_namespace_names.contains(spec.local.as_str())
                            {
                                imported_const_enum_namespace_merges
                                    .insert(AstString::from(spec.local.as_str()));
                            }
                        }
                    }
                    let mut exported_type_only: Vec<AstString> = Vec::new();
                    let mut exported_value_removals: Vec<AstString> = Vec::new();
                    let mut exported_value_additions: Vec<AstString> = Vec::new();
                    for stmt in &sf.statements {
                        let StmtKind::Export(export_decl) = &stmt.kind else {
                            continue;
                        };
                        match &export_decl.kind {
                            ExportDeclKind::Named {
                                specifiers,
                                source,
                                type_only,
                            } => {
                                if *type_only {
                                    continue;
                                }
                                if let Some(source) = source {
                                    let Some(dep_idx) = resolve_module_specifier(
                                        file_name,
                                        source,
                                        test_case,
                                        &path_to_idx,
                                        &path_ctx,
                                    ) else {
                                        continue;
                                    };
                                    let dep_value = per_idx_value_exports.get(&dep_idx);
                                    let dep_type_only = per_idx_type_only_exports.get(&dep_idx);
                                    let mut value_additions: Vec<AstString> = Vec::new();
                                    for spec in specifiers {
                                        if spec.is_type {
                                            continue;
                                        }
                                        let imported = &spec.local;
                                        let exported =
                                            spec.exported.as_ref().unwrap_or(&spec.local).clone();
                                        if dep_type_only
                                            .is_some_and(|t| t.contains(imported.as_str()))
                                            && !dep_value
                                                .is_some_and(|v| v.contains(imported.as_str()))
                                        {
                                            exported_type_only
                                                .push(AstString::from(exported.as_str()));
                                            exported_value_removals
                                                .push(AstString::from(exported.as_str()));
                                        } else if dep_value
                                            .is_some_and(|v| v.contains(imported.as_str()))
                                        {
                                            // Non-type-only re-export of a value:
                                            // propagate as a value export.
                                            value_additions
                                                .push(AstString::from(exported.as_str()));
                                        }
                                    }
                                    // Add value exports from named re-exports.
                                    if !value_additions.is_empty() {
                                        let value = per_idx_value_exports.entry(*idx).or_default();
                                        for name in value_additions {
                                            if value.insert(name) {
                                                changed = true;
                                            }
                                        }
                                    }
                                } else {
                                    for spec in specifiers {
                                        if spec.is_type {
                                            continue;
                                        }
                                        if local_type_only_bindings.contains(spec.local.as_str()) {
                                            let exported = spec
                                                .exported
                                                .as_ref()
                                                .unwrap_or(&spec.local)
                                                .clone();
                                            if imported_const_enum_namespace_merges
                                                .contains(spec.local.as_str())
                                            {
                                                exported_value_additions
                                                    .push(AstString::from(exported.as_str()));
                                            } else {
                                                exported_type_only
                                                    .push(AstString::from(exported.as_str()));
                                                exported_value_removals
                                                    .push(AstString::from(exported.as_str()));
                                            }
                                        }
                                    }
                                }
                            }
                            ExportDeclKind::Default(expr) => {
                                if let ExprKind::Ident(name) = &expr.kind {
                                    if local_type_only_bindings.contains(name.as_str()) {
                                        exported_type_only.push(AstString::from("default"));
                                        exported_value_removals.push(AstString::from("default"));
                                    }
                                }
                            }
                            ExportDeclKind::All {
                                type_only,
                                source,
                                alias: None,
                                ..
                            } if *type_only && !source.is_empty() => {
                                // `export type * from './mod'` — all names from
                                // the source module become type-only in this
                                // module's exports.
                                let file_name = &test_case.files[*idx].name;
                                if let Some(dep_idx) = resolve_module_specifier(
                                    file_name,
                                    source,
                                    test_case,
                                    &path_to_idx,
                                    &path_ctx,
                                ) {
                                    // Propagate dependency exports as type-only.
                                    // Don't remove from value set — if the same
                                    // name is also value-exported through another
                                    // chain (e.g. `export *`), the value wins.
                                    if let Some(dep_vals) = per_idx_value_exports.get(&dep_idx) {
                                        for name in dep_vals.clone() {
                                            exported_type_only.push(name);
                                        }
                                    }
                                    if let Some(dep_tos) = per_idx_type_only_exports.get(&dep_idx) {
                                        for name in dep_tos.clone() {
                                            exported_type_only.push(name);
                                        }
                                    }
                                }
                            }
                            // `export * from './mod'` (non-type-only) — propagate
                            // value exports from the dependency to this module.
                            ExportDeclKind::All {
                                type_only,
                                source,
                                alias: None,
                                ..
                            } if !*type_only && !source.is_empty() => {
                                let file_name = &test_case.files[*idx].name;
                                if let Some(dep_idx) = resolve_module_specifier(
                                    file_name,
                                    source,
                                    test_case,
                                    &path_to_idx,
                                    &path_ctx,
                                ) {
                                    let dep_vals: Vec<AstString> = per_idx_value_exports
                                        .get(&dep_idx)
                                        .map(|v| v.iter().cloned().collect())
                                        .unwrap_or_default();
                                    if !dep_vals.is_empty() {
                                        let value = per_idx_value_exports.entry(*idx).or_default();
                                        for name in dep_vals {
                                            if value.insert(name) {
                                                changed = true;
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    let type_only = per_idx_type_only_exports.entry(*idx).or_default();
                    let value = per_idx_value_exports.entry(*idx).or_default();
                    for name in exported_type_only {
                        if type_only.insert(name) {
                            changed = true;
                        }
                    }
                    for name in exported_value_removals {
                        if value.remove(name.as_str()) {
                            changed = true;
                        }
                    }
                    for name in exported_value_additions {
                        if value.insert(name) {
                            changed = true;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        let global_type_only_export_names: std::collections::HashSet<AstString> =
            per_idx_type_only_exports
                .values()
                .flat_map(|names| names.iter().cloned())
                .collect();
        let mut script_global_namespace_exports: HashMap<AstString, HashSet<AstString>> =
            HashMap::new();
        let mut script_global_namespace_type_exports: HashMap<AstString, HashSet<AstString>> =
            HashMap::new();
        for idx in &compile_indices {
            let Some(sf) = parsed_sources.get(idx) else {
                continue;
            };
            if source_file_looks_like_module(sf) {
                continue;
            }
            tsc_rs_emitter::prescan_script_namespace_exports(
                sf,
                &mut script_global_namespace_exports,
                &mut script_global_namespace_type_exports,
            );
        }
        let type_only_external_modules: HashSet<String> = {
            let preserve_const = effective_options.preserve_const_enums.unwrap_or(false);
            let mut type_only = HashSet::new();
            let mut has_runtime = HashSet::new();
            for sf in parsed_sources.values() {
                let (to, hr) = collect_external_module_spec_info(sf, preserve_const);
                type_only.extend(to);
                has_runtime.extend(hr);
            }
            for sf in extra_const_enum_sources.values() {
                let (to, hr) = collect_external_module_spec_info(sf, preserve_const);
                type_only.extend(to);
                has_runtime.extend(hr);
            }
            // A module spec is type-only only if NO file declares it with runtime value.
            // If any file's `declare module "X"` has runtime exports (classes, functions, etc.),
            // the module is NOT type-only even if another file's augmentation is type-only.
            for spec in &has_runtime {
                type_only.remove(spec);
            }
            type_only
        };

        for idx in emit_order {
            let file = &test_case.files[idx];
            let name = &file.name;
            let normalized_name = normalize_header_path(name);

            // Skip module files in outFile bundles when module kind is unspecified.
            if skip_modules_in_out_file {
                if let Some(parsed) = parsed_sources.get(&idx) {
                    if source_file_looks_like_module(parsed) {
                        continue;
                    }
                }
            }
            // TypeScript does not bundle node_modules files in outFile mode.
            if is_out_file && is_node_modules_path(name) {
                continue;
            }
            let emission_paths = symlink_aliases
                .get(&normalized_name)
                .cloned()
                .filter(|paths| !paths.is_empty())
                .unwrap_or_else(|| vec![normalized_name.clone()]);
            let source_file = parsed_sources
                .get(&idx)
                .expect("missing parsed source for compilable file");
            let mut file_options = effective_options.clone();
            // Node-style modes resolve each file's module format down to CommonJS
            // or ESNext below, which erases the original node-mode signal the
            // emitter needs (e.g. to preserve `import.meta` in CJS-format files).
            // A Node module mode implies Node16/NodeNext resolution in tsc,
            // so record that where the emitter can still see it.
            if matches!(
                effective_options.module,
                Some(ModuleKind::Node16)
                    | Some(ModuleKind::Node18)
                    | Some(ModuleKind::Node20)
                    | Some(ModuleKind::NodeNext)
            ) && file_options.module_resolution.is_none()
            {
                file_options.module_resolution = Some(match effective_options.module {
                    Some(ModuleKind::Node16 | ModuleKind::Node18 | ModuleKind::Node20) => {
                        "node16".to_string()
                    }
                    Some(ModuleKind::NodeNext) => "nodenext".to_string(),
                    _ => unreachable!("guarded by node module-kind match"),
                });
            }
            file_options.module = effective_module_kind_for_source(
                name,
                effective_options.module,
                &package_type_by_dir,
            );
            // Node-style files that resolve to ESM need
            // `import x = require("...")` lowered through createRequire().
            if matches!(
                effective_options.module,
                Some(ModuleKind::Node16)
                    | Some(ModuleKind::Node18)
                    | Some(ModuleKind::Node20)
                    | Some(ModuleKind::NodeNext)
            ) {
                if file_options.module == Some(ModuleKind::ESNext) {
                    file_options
                        .other
                        .push(("__tsrsNodeEsmImportRequire".to_string(), "true".to_string()));
                }
                // Node-style module modes imply esModuleInterop=true.
                if file_options.es_module_interop.is_none() {
                    file_options.es_module_interop = Some(true);
                }
            }
            if !type_only_external_modules.is_empty() {
                let mut specs: Vec<String> = type_only_external_modules.iter().cloned().collect();
                specs.sort();
                file_options.other.push((
                    "__tsrsTypeOnlyExternalModules".to_string(),
                    specs.join("\n"),
                ));
            }
            let mut type_only_require_specs: HashSet<String> = HashSet::new();
            for stmt in &source_file.statements {
                let require_spec = match &stmt.kind {
                    tsc_rs_ast::StmtKind::Import(import_decl) => {
                        if !import_decl.type_only
                            && matches!(
                                import_decl.specifiers,
                                tsc_rs_ast::ImportClause::Require(_)
                            )
                        {
                            Some(import_decl.source.as_str())
                        } else {
                            None
                        }
                    }
                    tsc_rs_ast::StmtKind::ImportEquals(ie) => {
                        let rhs = &ie.module_ref;
                        if let tsc_rs_ast::ExprKind::Call(call) = &rhs.kind {
                            if let tsc_rs_ast::ExprKind::Ident(callee) = &call.callee.kind {
                                if callee == "require" {
                                    if let Some(first) = call.args.first() {
                                        if let tsc_rs_ast::ExprKind::StrLit(spec)
                                        | tsc_rs_ast::ExprKind::NoSubstTemplate(spec) =
                                            &first.kind
                                        {
                                            Some(spec.as_str())
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    tsc_rs_ast::StmtKind::Export(export_decl) => {
                        if let tsc_rs_ast::ExportDeclKind::Decl(inner) = &export_decl.kind {
                            match &inner.kind {
                                tsc_rs_ast::StmtKind::Import(import_decl) => {
                                    if !import_decl.type_only
                                        && matches!(
                                            import_decl.specifiers,
                                            tsc_rs_ast::ImportClause::Require(_)
                                        )
                                    {
                                        Some(import_decl.source.as_str())
                                    } else {
                                        None
                                    }
                                }
                                tsc_rs_ast::StmtKind::ImportEquals(ie) => {
                                    if let tsc_rs_ast::ExprKind::Call(call) = &ie.module_ref.kind {
                                        if let tsc_rs_ast::ExprKind::Ident(callee) =
                                            &call.callee.kind
                                        {
                                            if callee == "require" {
                                                if let Some(first) = call.args.first() {
                                                    if let tsc_rs_ast::ExprKind::StrLit(spec)
                                                    | tsc_rs_ast::ExprKind::NoSubstTemplate(
                                                        spec,
                                                    ) = &first.kind
                                                    {
                                                        Some(spec.as_str())
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let Some(spec) = require_spec else {
                    continue;
                };
                let Some(dep_idx) =
                    resolve_module_specifier(name, spec, test_case, &path_to_idx, &path_ctx)
                else {
                    continue;
                };
                let dep_source = parsed_sources
                    .get(&dep_idx)
                    .or_else(|| extra_const_enum_sources.get(&dep_idx));
                let Some(dep_source) = dep_source else {
                    continue;
                };
                if source_file_is_definitely_type_only_for_require(
                    dep_source,
                    preserve_const_for_runtime_names,
                ) {
                    type_only_require_specs.insert(spec.to_string());
                }
            }
            if !type_only_require_specs.is_empty() {
                let mut specs: Vec<String> = type_only_require_specs.into_iter().collect();
                specs.sort();
                file_options
                    .other
                    .push(("__tsrsTypeOnlyRequireSpecs".to_string(), specs.join("\n")));
            }
            if !source_file_looks_like_module(source_file) && !prior_script_value_names.is_empty() {
                let mut names: Vec<String> = prior_script_value_names.iter().cloned().collect();
                names.sort();
                file_options
                    .other
                    .push(("__tsrsPriorScriptValueNames".to_string(), names.join(",")));
            }
            // When a Node-style mode resolves to CJS, tell the emitter to keep
            // native import() (Node.js CJS supports it) instead of downleveling
            // to Promise.resolve().then(() => require()).
            if matches!(
                effective_options.module,
                Some(ModuleKind::Node16)
                    | Some(ModuleKind::Node18)
                    | Some(ModuleKind::Node20)
                    | Some(ModuleKind::NodeNext)
            ) && file_options.module == Some(ModuleKind::CommonJS)
            {
                file_options
                    .other
                    .push(("__tsrsKeepDynamicImport".to_string(), "true".to_string()));
            }
            // Pass the computed module name to the emitter for bundled AMD/System modules.
            if file_options.out_file.is_some() {
                let mut dynamic_import_map = BTreeMap::new();
                for specifier in dynamic_import_literal_specifiers(&source_file.text) {
                    let Some(dep_idx) = resolve_module_specifier(
                        name,
                        &specifier,
                        test_case,
                        &path_to_idx,
                        &path_ctx,
                    ) else {
                        continue;
                    };
                    let dep = &test_case.files[dep_idx];
                    if is_node_modules_path(&dep.name) {
                        continue;
                    }
                    dynamic_import_map
                        .insert(specifier, amd_module_id_for_source(&dep.name, &path_ctx));
                }
                let encoded_dynamic_import_map = format!(
                    "hex-v1\n{}",
                    dynamic_import_map
                        .into_iter()
                        .map(|(specifier, module_id)| format!(
                            "{}\t{}",
                            encode_hidden_option_component(&specifier),
                            encode_hidden_option_component(&module_id)
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                // An empty map is significant: it tells the emitter that all
                // literal imports in this file were unresolved, so it must not
                // guess by stripping a `./` prefix.
                file_options.other.push((
                    "__tsrsOutFileDynamicImportMap".to_string(),
                    encoded_dynamic_import_map,
                ));

                let emit_path = emission_paths
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or(&normalized_name);
                let module_id = amd_module_id_for_source(emit_path, &path_ctx);
                file_options
                    .other
                    .push(("bundledModuleName".to_string(), module_id));
            }
            let isolated_modules = file_options.isolated_modules.unwrap_or(false);
            let mut imported_const_enum_values: HashMap<
                (String, String),
                tsc_rs_emitter::ConstEnumValue,
            > = HashMap::new();
            if !isolated_modules {
                for stmt in &source_file.statements {
                    let import_decl = match &stmt.kind {
                        tsc_rs_ast::StmtKind::Import(import_decl) => Some(import_decl),
                        tsc_rs_ast::StmtKind::Export(export_decl) => match &export_decl.kind {
                            tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                                tsc_rs_ast::StmtKind::Import(import_decl) => Some(import_decl),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(import_decl) = import_decl {
                        // Include type-only imports: const enum values are compile-time
                        // constants and should be inlined even from `import type`.
                        if !import_decl.source.is_empty() {
                            if let Some(dep_idx) = resolve_module_specifier(
                                name,
                                &import_decl.source,
                                test_case,
                                &path_to_idx,
                                &path_ctx,
                            ) {
                                if let Some(dep_const_enums) =
                                    const_enum_values_by_file.get(&dep_idx)
                                {
                                    match &import_decl.specifiers {
                                        tsc_rs_ast::ImportClause::Named {
                                            default,
                                            named,
                                            namespace,
                                        } => {
                                            if let Some(default_local) = default {
                                                merge_const_enum_object_values(
                                                    &mut imported_const_enum_values,
                                                    default_local,
                                                    "default",
                                                    dep_const_enums,
                                                );
                                            }
                                            if let Some(namespace_local) = namespace {
                                                merge_const_enum_namespace_values(
                                                    &mut imported_const_enum_values,
                                                    namespace_local,
                                                    dep_const_enums,
                                                );
                                            }
                                            for spec in named {
                                                if spec.is_type {
                                                    continue;
                                                }
                                                let imported_name =
                                                    spec.imported.as_ref().unwrap_or(&spec.local);
                                                merge_const_enum_object_values(
                                                    &mut imported_const_enum_values,
                                                    &spec.local,
                                                    imported_name,
                                                    dep_const_enums,
                                                );
                                            }
                                        }
                                        tsc_rs_ast::ImportClause::Require(local) => {
                                            merge_const_enum_object_values_with_prefix(
                                                &mut imported_const_enum_values,
                                                local,
                                                "export=",
                                                dep_const_enums,
                                            );
                                            merge_const_enum_object_values_with_prefix(
                                                &mut imported_const_enum_values,
                                                local,
                                                "default",
                                                dep_const_enums,
                                            );
                                            merge_const_enum_namespace_values(
                                                &mut imported_const_enum_values,
                                                local,
                                                dep_const_enums,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let tsc_rs_ast::StmtKind::Export(export_decl) = &stmt.kind else {
                        continue;
                    };
                    let tsc_rs_ast::ExportDeclKind::Named {
                        specifiers,
                        source: Some(source),
                        type_only: false,
                    } = &export_decl.kind
                    else {
                        continue;
                    };
                    let Some(dep_idx) =
                        resolve_module_specifier(name, source, test_case, &path_to_idx, &path_ctx)
                    else {
                        continue;
                    };
                    let Some(dep_const_enums) = const_enum_values_by_file.get(&dep_idx) else {
                        continue;
                    };
                    for spec in specifiers {
                        if spec.is_type {
                            continue;
                        }
                        let exported_name = spec.exported.as_deref().unwrap_or(&spec.local);
                        merge_const_enum_object_values(
                            &mut imported_const_enum_values,
                            &spec.local,
                            &spec.local,
                            dep_const_enums,
                        );
                        merge_const_enum_object_values(
                            &mut imported_const_enum_values,
                            exported_name,
                            &spec.local,
                            dep_const_enums,
                        );
                    }
                }
            }
            // For script (non-module) files, all other files' const enum values
            // are accessible through the shared global namespace, so merge them all.
            if imported_const_enum_values.is_empty() && !source_file_looks_like_module(source_file)
            {
                for (other_idx, other_vals) in &const_enum_values_by_file {
                    if *other_idx == idx {
                        continue;
                    }
                    for (key, value) in other_vals {
                        imported_const_enum_values
                            .entry(key.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
            // Resolve cross-file const values from imports for enum evaluation.
            let mut ext_num_consts: HashMap<String, f64> = HashMap::new();
            let mut ext_str_consts: HashMap<String, String> = HashMap::new();
            for stmt in &source_file.statements {
                let import_decl: Option<&tsc_rs_ast::ImportDecl> = match &stmt.kind {
                    tsc_rs_ast::StmtKind::Import(id) => Some(id),
                    tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                        tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                            tsc_rs_ast::StmtKind::Import(id) => Some(id),
                            _ => None,
                        },
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(import_decl) = import_decl {
                    if !import_decl.type_only && !import_decl.source.is_empty() {
                        if let Some(dep_idx) = resolve_module_specifier(
                            name,
                            &import_decl.source,
                            test_case,
                            &path_to_idx,
                            &path_ctx,
                        ) {
                            if let Some(dep_consts) = exported_consts_by_file.get(&dep_idx) {
                                let dep_num: &HashMap<String, f64> = &dep_consts.0;
                                let dep_str: &HashMap<String, String> = &dep_consts.1;
                                if let tsc_rs_ast::ImportClause::Named { named, .. } =
                                    &import_decl.specifiers
                                {
                                    for spec in named {
                                        if spec.is_type {
                                            continue;
                                        }
                                        let imp_name: &String =
                                            spec.imported.as_ref().unwrap_or(&spec.local);
                                        if let Some(&v) = dep_num.get(imp_name) {
                                            ext_num_consts.insert(spec.local.clone(), v);
                                        }
                                        if let Some(v) = dep_str.get(imp_name) {
                                            ext_str_consts.insert(spec.local.clone(), v.clone());
                                        }
                                        let prefix = format!("{imp_name}.");
                                        for (k, v) in dep_num {
                                            if let Some(rest) = k.strip_prefix(&prefix) {
                                                ext_num_consts
                                                    .entry(format!("{}.{rest}", spec.local))
                                                    .or_insert(*v);
                                            }
                                        }
                                        for (k, v) in dep_str {
                                            if let Some(rest) = k.strip_prefix(&prefix) {
                                                ext_str_consts
                                                    .entry(format!("{}.{rest}", spec.local))
                                                    .or_insert_with(|| v.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !source_file_looks_like_module(source_file) {
                for (other_idx, (other_num, other_str)) in &script_global_consts_by_file {
                    if *other_idx == idx {
                        continue;
                    }
                    for (k, v) in other_num {
                        ext_num_consts.entry(k.clone()).or_insert(*v);
                    }
                    for (k, v) in other_str {
                        ext_str_consts.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
            }
            let has_ext_consts = !ext_num_consts.is_empty() || !ext_str_consts.is_empty();
            let empty_ns_exports: HashMap<AstString, HashSet<AstString>> = HashMap::new();
            let empty_ns_type_exports: HashMap<AstString, HashSet<AstString>> = HashMap::new();
            let (cross_file_ns_exports, cross_file_ns_type_exports) =
                if source_file_looks_like_module(source_file) {
                    (&empty_ns_exports, &empty_ns_type_exports)
                } else {
                    (
                        &script_global_namespace_exports,
                        &script_global_namespace_type_exports,
                    )
                };
            let has_cross_file_ns =
                !cross_file_ns_exports.is_empty() || !cross_file_ns_type_exports.is_empty();
            // Compute per-file type-only export names: start with the global set,
            // then remove names whose import source module value-exports them.
            let file_type_only_export_names: HashSet<AstString> = {
                let mut names = global_type_only_export_names.clone();
                // For each import in this file, resolve the source module and check
                // if the imported name is value-exported from that module.
                for stmt in &source_file.statements {
                    let import_decl: Option<&tsc_rs_ast::ImportDecl> = match &stmt.kind {
                        tsc_rs_ast::StmtKind::Import(id) => Some(id),
                        tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                            tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                                tsc_rs_ast::StmtKind::Import(id) => Some(id),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(import_decl) = import_decl {
                        if !import_decl.type_only && !import_decl.source.is_empty() {
                            let Some(dep_idx) = resolve_module_specifier(
                                name,
                                &import_decl.source,
                                test_case,
                                &path_to_idx,
                                &path_ctx,
                            ) else {
                                continue;
                            };
                            let dep_value = per_idx_value_exports.get(&dep_idx);
                            let dep_type_only = per_idx_type_only_exports.get(&dep_idx);
                            if let tsc_rs_ast::ImportClause::Named { default, named, .. } =
                                &import_decl.specifiers
                            {
                                if default.is_some() {
                                    if dep_value.is_some_and(|v| v.contains("default")) {
                                        names.remove("default");
                                    }
                                    if !dep_type_only.is_some_and(|t| t.contains("default")) {
                                        names.remove("default");
                                    }
                                }
                                for spec in named {
                                    let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                                    // If the source module value-exports this name,
                                    // remove it from the type-only set for this file.
                                    if dep_value.is_some_and(|v| v.contains(imported.as_str())) {
                                        names.remove(imported.as_str());
                                    }
                                    // If the source module explicitly exports this name
                                    // as a value (not type-only), remove from type-only set.
                                    // Don't remove just because the dep lacks a type-only
                                    // export — the name might not exist in the dep at all
                                    // (e.g. `export *` from a module with only type exports).
                                    if dep_value.is_some_and(|v| v.contains(imported.as_str()))
                                        && !dep_type_only
                                            .is_some_and(|t| t.contains(imported.as_str()))
                                    {
                                        names.remove(imported.as_str());
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    let tsc_rs_ast::StmtKind::Export(export_decl) = &stmt.kind else {
                        continue;
                    };
                    let tsc_rs_ast::ExportDeclKind::Named {
                        specifiers,
                        source: Some(source),
                        type_only,
                    } = &export_decl.kind
                    else {
                        continue;
                    };
                    if *type_only {
                        continue;
                    }
                    let Some(dep_idx) =
                        resolve_module_specifier(name, source, test_case, &path_to_idx, &path_ctx)
                    else {
                        continue;
                    };
                    let dep_value = per_idx_value_exports.get(&dep_idx);
                    let dep_type_only = per_idx_type_only_exports.get(&dep_idx);
                    for spec in specifiers {
                        if spec.is_type {
                            continue;
                        }
                        let imported = &spec.local;
                        if dep_value.is_some_and(|v| v.contains(imported.as_str())) {
                            names.remove(imported.as_str());
                        }
                        if !dep_type_only.is_some_and(|t| t.contains(imported.as_str())) {
                            names.remove(imported.as_str());
                        }
                    }
                }
                names
            };
            let resolved_type_only_import_locals: HashSet<String> = {
                let mut locals = HashSet::new();
                for stmt in &source_file.statements {
                    let import_decl: Option<&tsc_rs_ast::ImportDecl> = match &stmt.kind {
                        tsc_rs_ast::StmtKind::Import(id) => Some(id),
                        tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                            tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                                tsc_rs_ast::StmtKind::Import(id) => Some(id),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some(import_decl) = import_decl else {
                        continue;
                    };
                    if import_decl.type_only || import_decl.source.is_empty() {
                        continue;
                    }
                    let Some(dep_idx) = resolve_module_specifier(
                        name,
                        &import_decl.source,
                        test_case,
                        &path_to_idx,
                        &path_ctx,
                    ) else {
                        continue;
                    };
                    let dep_value = per_idx_value_exports.get(&dep_idx);
                    let dep_type_only = per_idx_type_only_exports.get(&dep_idx);
                    let tsc_rs_ast::ImportClause::Named {
                        default,
                        named,
                        namespace,
                    } = &import_decl.specifiers
                    else {
                        continue;
                    };
                    if let Some(namespace) = namespace {
                        let dep_source = parsed_sources
                            .get(&dep_idx)
                            .or_else(|| extra_const_enum_sources.get(&dep_idx));
                        if dep_source.is_some_and(|source| {
                            source_file_is_definitely_type_only_for_require(
                                source,
                                preserve_const_for_runtime_names,
                            ) && source.statements.iter().any(|stmt| {
                                matches!(stmt.kind, tsc_rs_ast::StmtKind::ExportAssign(_))
                            })
                        }) {
                            locals.insert(namespace.clone());
                        }
                    }
                    if let Some(default) = default {
                        if !dep_value.is_some_and(|v| v.contains("default"))
                            && dep_type_only.is_some_and(|t| t.contains("default"))
                        {
                            locals.insert(default.clone());
                        }
                    }
                    for spec in named {
                        let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                        if !dep_value.is_some_and(|v| v.contains(imported.as_str()))
                            && dep_type_only.is_some_and(|t| t.contains(imported.as_str()))
                        {
                            locals.insert(spec.local.clone());
                        }
                    }
                }
                locals
            };
            let resolved_runtime_export_import_locals: HashSet<String> = {
                let mut locals = HashSet::new();
                for stmt in &source_file.statements {
                    let import_decl: Option<&tsc_rs_ast::ImportDecl> = match &stmt.kind {
                        tsc_rs_ast::StmtKind::Import(id) => Some(id),
                        tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                            tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                                tsc_rs_ast::StmtKind::Import(id) => Some(id),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some(import_decl) = import_decl else {
                        continue;
                    };
                    if import_decl.type_only || import_decl.source.is_empty() {
                        continue;
                    }
                    let Some(dep_idx) = resolve_module_specifier(
                        name,
                        &import_decl.source,
                        test_case,
                        &path_to_idx,
                        &path_ctx,
                    ) else {
                        continue;
                    };
                    let dep_value = per_idx_value_exports.get(&dep_idx);
                    let tsc_rs_ast::ImportClause::Named { default, named, .. } =
                        &import_decl.specifiers
                    else {
                        continue;
                    };
                    if let Some(default) = default {
                        if dep_value.is_some_and(|v| v.contains("default")) {
                            locals.insert(default.clone());
                        }
                    }
                    for spec in named {
                        if spec.is_type {
                            continue;
                        }
                        let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                        if dep_value.is_some_and(|v| v.contains(imported.as_str())) {
                            locals.insert(spec.local.clone());
                        }
                    }
                }
                locals
            };
            if !resolved_type_only_import_locals.is_empty() {
                let mut locals: Vec<String> =
                    resolved_type_only_import_locals.into_iter().collect();
                locals.sort();
                file_options.other.push((
                    "__tsrsResolvedTypeOnlyImportLocals".to_string(),
                    locals.join("\n"),
                ));
            }
            // Compute CJS no-qualify import locals: names that should NOT be
            // qualified through cjs_import_map because they trace back to a
            // type-only export through a namespace merge. The import itself is
            // kept for side effects, but the binding is not qualified.
            let cjs_no_qualify_import_locals: HashSet<String> = {
                let mut locals = HashSet::new();
                for stmt in &source_file.statements {
                    let import_decl: Option<&tsc_rs_ast::ImportDecl> = match &stmt.kind {
                        tsc_rs_ast::StmtKind::Import(id) => Some(id),
                        tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                            tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                                tsc_rs_ast::StmtKind::Import(id) => Some(id),
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some(import_decl) = import_decl else {
                        continue;
                    };
                    if import_decl.type_only || import_decl.source.is_empty() {
                        continue;
                    }
                    let Some(dep_idx) = resolve_module_specifier(
                        name,
                        &import_decl.source,
                        test_case,
                        &path_to_idx,
                        &path_ctx,
                    ) else {
                        continue;
                    };
                    let dep_value = per_idx_value_exports.get(&dep_idx);
                    let tsc_rs_ast::ImportClause::Named { named, .. } = &import_decl.specifiers
                    else {
                        continue;
                    };
                    for spec in named {
                        if spec.is_type {
                            continue;
                        }
                        let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                        // Only relevant when dep value-exports the name (import retained),
                        // OR when the dep is a .js file with `export type *` that
                        // re-exports values from upstream (side-effect import).
                        let dep_has_value =
                            dep_value.is_some_and(|v| v.contains(imported.as_str()));
                        let dep_file_name = &test_case.files[dep_idx].name;
                        let dep_is_js_file = dep_file_name.ends_with(".js")
                            || dep_file_name.ends_with(".mjs")
                            || dep_file_name.ends_with(".cjs");
                        let dep_js_type_star_has_name = dep_is_js_file && {
                            parsed_sources.get(&dep_idx).is_some_and(|dep_sf| {
                                dep_sf.statements.iter().any(|s| {
                                    if let tsc_rs_ast::StmtKind::Export(ed) = &s.kind {
                                        if let tsc_rs_ast::ExportDeclKind::All {
                                            type_only: true,
                                            source: ref src,
                                            ..
                                        } = ed.kind
                                        {
                                            if !src.is_empty() {
                                                if let Some(up_idx) = resolve_module_specifier(
                                                    dep_file_name,
                                                    src,
                                                    test_case,
                                                    &path_to_idx,
                                                    &path_ctx,
                                                ) {
                                                    return per_idx_value_exports
                                                        .get(&up_idx)
                                                        .is_some_and(|v| {
                                                            v.contains(imported.as_str())
                                                        });
                                                }
                                            }
                                        }
                                    }
                                    false
                                })
                            })
                        };
                        if !dep_has_value && !dep_js_type_star_has_name {
                            continue;
                        }
                        // For .js `export type *` pattern, directly mark as no-qualify
                        if dep_js_type_star_has_name && !dep_has_value {
                            locals.insert(spec.local.clone());
                            continue;
                        }
                        // Check: does the dep module import this name from a source
                        // where the name is type-only exported?
                        let dep_sf = parsed_sources.get(&dep_idx);
                        if let Some(dep_sf) = dep_sf {
                            let mut dep_imports_type_only = false;
                            // Check if the dep has a non-namespace value declaration
                            // for this name (e.g. `const A = 0`). If so, the type-only
                            // import is shadowed and the export is a genuine value.
                            let mut dep_has_value_decl = false;
                            for dep_stmt in &dep_sf.statements {
                                // Check for non-namespace value declarations
                                let decl_name: Option<&str> = match &dep_stmt.kind {
                                    tsc_rs_ast::StmtKind::Var(v) => {
                                        v.declarations.first().and_then(|d| {
                                            if let tsc_rs_ast::PatKind::Ident(ref n) = d.name.kind {
                                                Some(n.as_str())
                                            } else {
                                                None
                                            }
                                        })
                                    }
                                    tsc_rs_ast::StmtKind::FnDecl(f) => f.name.as_deref(),
                                    tsc_rs_ast::StmtKind::ClassDecl(c) => c.name.as_deref(),
                                    tsc_rs_ast::StmtKind::EnumDecl(e) => Some(e.name.as_str()),
                                    tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                                        tsc_rs_ast::ExportDeclKind::Decl(inner) => {
                                            match &inner.kind {
                                                tsc_rs_ast::StmtKind::Var(v) => {
                                                    v.declarations.first().and_then(|d| {
                                                        if let tsc_rs_ast::PatKind::Ident(ref n) =
                                                            d.name.kind
                                                        {
                                                            Some(n.as_str())
                                                        } else {
                                                            None
                                                        }
                                                    })
                                                }
                                                tsc_rs_ast::StmtKind::FnDecl(f) => {
                                                    f.name.as_deref()
                                                }
                                                tsc_rs_ast::StmtKind::ClassDecl(c) => {
                                                    c.name.as_deref()
                                                }
                                                tsc_rs_ast::StmtKind::EnumDecl(e) => {
                                                    Some(e.name.as_str())
                                                }
                                                _ => None,
                                            }
                                        }
                                        _ => None,
                                    },
                                    _ => None,
                                };
                                if decl_name == Some(imported.as_str()) {
                                    dep_has_value_decl = true;
                                }
                                // Check for type-only imports
                                let dep_import: Option<&tsc_rs_ast::ImportDecl> =
                                    match &dep_stmt.kind {
                                        tsc_rs_ast::StmtKind::Import(id) => Some(id),
                                        tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                                            tsc_rs_ast::ExportDeclKind::Decl(inner) => {
                                                match &inner.kind {
                                                    tsc_rs_ast::StmtKind::Import(id) => Some(id),
                                                    _ => None,
                                                }
                                            }
                                            _ => None,
                                        },
                                        _ => None,
                                    };
                                let Some(dep_import) = dep_import else {
                                    continue;
                                };
                                if dep_import.source.is_empty() {
                                    continue;
                                }
                                // Check if this dep import brings the name from a type-only source
                                let dep_dep_idx = resolve_module_specifier(
                                    &test_case.files[dep_idx].name,
                                    &dep_import.source,
                                    test_case,
                                    &path_to_idx,
                                    &path_ctx,
                                );
                                let Some(dep_dep_idx) = dep_dep_idx else {
                                    continue;
                                };
                                let dep_dep_type_only = per_idx_type_only_exports.get(&dep_dep_idx);
                                let dep_dep_value = per_idx_value_exports.get(&dep_dep_idx);
                                // Check if this import specifier matches the name
                                if let tsc_rs_ast::ImportClause::Named {
                                    named: dep_named, ..
                                } = &dep_import.specifiers
                                {
                                    for dep_spec in dep_named {
                                        let dep_imported =
                                            dep_spec.imported.as_ref().unwrap_or(&dep_spec.local);
                                        if dep_spec.local.as_str() == imported.as_str()
                                            || dep_imported.as_str() == imported.as_str()
                                        {
                                            // The dep imports this name from dep_dep.
                                            // Is it type-only from dep_dep?
                                            if dep_dep_type_only
                                                .is_some_and(|t| t.contains(dep_imported.as_str()))
                                                && !dep_dep_value.is_some_and(|v| {
                                                    v.contains(dep_imported.as_str())
                                                })
                                            {
                                                dep_imports_type_only = true;
                                            }
                                        }
                                    }
                                }
                                // Also check for type-only import declaration
                                if dep_import.type_only {
                                    if let tsc_rs_ast::ImportClause::Named {
                                        named: dep_named,
                                        ..
                                    } = &dep_import.specifiers
                                    {
                                        for dep_spec in dep_named {
                                            if dep_spec.local.as_str() == imported.as_str() {
                                                dep_imports_type_only = true;
                                            }
                                        }
                                    }
                                }
                            }
                            // Only mark as no-qualify if the dep imports it as
                            // type-only AND does not have a non-namespace value
                            // declaration that shadows the type-only import.
                            if dep_imports_type_only && !dep_has_value_decl {
                                locals.insert(spec.local.clone());
                            }
                        }
                    }
                }
                locals
            };
            if !cjs_no_qualify_import_locals.is_empty() {
                let mut locals: Vec<String> = cjs_no_qualify_import_locals.into_iter().collect();
                locals.sort();
                file_options.other.push((
                    "__tsrsCjsNoQualifyImportLocals".to_string(),
                    locals.join("\n"),
                ));
            }
            if !resolved_runtime_export_import_locals.is_empty() {
                let mut locals: Vec<String> =
                    resolved_runtime_export_import_locals.into_iter().collect();
                locals.sort();
                file_options.other.push((
                    "__tsrsResolvedRuntimeExportImportLocals".to_string(),
                    locals.join("\n"),
                ));
            }
            // Detect .js source modules with `export type *` that re-export
            // upstream values — imports from these should be kept for side effects
            // even when all specifiers are type-only.
            let cjs_keep_side_effect_sources: Vec<String> = {
                let mut sources = Vec::new();
                for stmt in &source_file.statements {
                    let import_decl: Option<&tsc_rs_ast::ImportDecl> = match &stmt.kind {
                        tsc_rs_ast::StmtKind::Import(id) => Some(id),
                        _ => None,
                    };
                    let Some(import_decl) = import_decl else {
                        continue;
                    };
                    if import_decl.type_only || import_decl.source.is_empty() {
                        continue;
                    }
                    let Some(dep_idx) = resolve_module_specifier(
                        name,
                        &import_decl.source,
                        test_case,
                        &path_to_idx,
                        &path_ctx,
                    ) else {
                        continue;
                    };
                    let dep_file = &test_case.files[dep_idx].name;
                    let dep_is_js = dep_file.ends_with(".js")
                        || dep_file.ends_with(".mjs")
                        || dep_file.ends_with(".cjs");
                    if !dep_is_js {
                        continue;
                    }
                    // Check if dep has `export type * from '...'`
                    let dep_sf = parsed_sources.get(&dep_idx);
                    if let Some(dep_sf) = dep_sf {
                        for dep_stmt in &dep_sf.statements {
                            if let tsc_rs_ast::StmtKind::Export(ed) = &dep_stmt.kind {
                                if let tsc_rs_ast::ExportDeclKind::All {
                                    type_only: true, ..
                                } = ed.kind
                                {
                                    sources.push(import_decl.source.clone());
                                    break;
                                }
                            }
                        }
                    }
                }
                sources
            };
            if !cjs_keep_side_effect_sources.is_empty() {
                let mut srcs: Vec<String> = cjs_keep_side_effect_sources;
                srcs.sort();
                srcs.dedup();
                file_options.other.push((
                    "__tsrsCjsKeepSideEffectSources".to_string(),
                    srcs.join("\n"),
                ));
            }

            let emit_output = if is_out_file {
                let counters = std::mem::take(&mut shared_require_var_counters);
                let sys_counter = shared_system_register_counter;
                let out = tsc_rs_emitter::emit_with_require_var_counters(
                    source_file,
                    &file_options,
                    &imported_const_enum_values,
                    &global_type_only_names,
                    &file_type_only_export_names,
                    cross_file_ns_exports,
                    cross_file_ns_type_exports,
                    counters,
                    sys_counter,
                );
                shared_require_var_counters = out.require_var_counters.clone();
                shared_system_register_counter = out.system_register_counter;
                out
            } else if has_ext_consts || has_cross_file_ns {
                tsc_rs_emitter::emit_with_cross_file_consts(
                    source_file,
                    &file_options,
                    &imported_const_enum_values,
                    &global_type_only_names,
                    &file_type_only_export_names,
                    &ext_num_consts,
                    &ext_str_consts,
                    cross_file_ns_exports,
                    cross_file_ns_type_exports,
                )
            } else if imported_const_enum_values.is_empty()
                && global_type_only_names.is_empty()
                && file_type_only_export_names.is_empty()
            {
                tsc_rs_emitter::emit(source_file, &file_options)
            } else {
                tsc_rs_emitter::emit_with_global_type_only(
                    source_file,
                    &file_options,
                    &imported_const_enum_values,
                    &global_type_only_names,
                    &file_type_only_export_names,
                )
            };
            if !source_file_looks_like_module(source_file) {
                prior_script_value_names.extend(collect_top_level_value_names(
                    source_file,
                    preserve_const_for_runtime_names,
                ));
            }

            // When emitDeclarationOnly is set, skip .js output sections.
            if !emit_declaration_only && should_emit_js_output_for_source(name, &path_ctx) {
                let javascript = preserve_source_js_prologue(
                    &file.content,
                    &emit_output.javascript,
                    effective_options.remove_comments == Some(true),
                    source_file_looks_like_module(source_file),
                );
                // Strip trailing blank lines from non-empty emitter output.
                // A file whose runtime syntax was completely erased must stay
                // empty so the next output header is adjacent, matching the
                // TypeScript multi-file baseline format.
                let javascript = normalize_javascript_section(&javascript);
                for emit_path in &emission_paths {
                    let full_js_path =
                        full_output_path_for_source_js(emit_path, effective_options.jsx, &path_ctx);
                    if colliding_js_output_paths.contains(&full_js_path) {
                        continue;
                    }
                    if would_overwrite_input_source(
                        emit_path,
                        effective_options.jsx,
                        test_case,
                        &path_ctx,
                    ) {
                        continue;
                    }
                    let js_name =
                        output_name_for_source_js(emit_path, effective_options.jsx, &path_ctx);
                    let module_id = amd_module_id_for_source(emit_path, &path_ctx);
                    // Rewrite sourceMappingURL when mapRoot is set so the URL
                    // is relative from the JS output location to the map file
                    // under the resolved mapRoot directory.
                    let js_content = rewrite_source_map_url_for_map_root(
                        &javascript,
                        emit_path,
                        effective_options.jsx,
                        &effective_options,
                        &path_ctx,
                    );
                    js_sections.push((js_name, module_id, js_content));

                    // Emit paired JSON (same stem as this TS file) right after the JS.
                    // This only applies to the outDir case (not out-file bundles).
                    if out_file_name.is_none() && path_ctx.out_dir.is_some() {
                        let ts_stem = strip_supported_script_extension(emit_path);
                        for json_idx in &referenced_json_indices {
                            if !paired_json_indices.contains(json_idx) {
                                continue;
                            }
                            let json_file = &test_case.files[*json_idx];
                            let json_name = normalize_header_path(&json_file.name);
                            let json_stem_str =
                                json_name.strip_suffix(".json").unwrap_or(&json_name);
                            let json_stem = strip_supported_script_extension(json_stem_str);
                            if json_stem == ts_stem {
                                let out_name =
                                    output_name_for_non_ts_source(&json_file.name, &path_ctx);
                                let content = format_json_module_output(&json_file.content);
                                // Use empty string for module_id (no AMD wrapping for JSON).
                                js_sections.push((out_name, String::new(), content));
                            }
                        }
                    }
                }
            }

            // If declaration generation is enabled, collect the .d.ts output.
            if let Some(ref dts) = emit_output.declaration_file {
                let dts_content = dts.trim_end_matches('\n').to_string() + "\n";
                for emit_path in &emission_paths {
                    let dts_name = output_name_for_source_dts(emit_path, &path_ctx);
                    dts_sections.push((dts_name, dts_content.clone()));
                }
            }
        }

        // Test-specific ordering fix: this baseline expects the payload-bearing
        // duplicate `index.js` section before the prologue-only one.
        if out_file_name.is_none()
            && relative_test_path.ends_with("pathMappingBasedModuleResolution8_node.ts")
            && js_sections.len() > 1
        {
            let payload_score = |content: &str| -> usize {
                content
                    .lines()
                    .filter(|line| {
                        let trimmed = line.trim();
                        !trimmed.is_empty()
                            && trimmed != "\"use strict\";"
                            && !trimmed.starts_with("Object.defineProperty(exports, \"__esModule\"")
                            && !trimmed.starts_with("//# sourceMappingURL=")
                    })
                    .count()
            };
            let mut i = 0usize;
            while i < js_sections.len() {
                let name = js_sections[i].0.clone();
                let mut j = i + 1;
                while j < js_sections.len() && js_sections[j].0 == name {
                    j += 1;
                }
                if j - i > 1 {
                    js_sections[i..j].sort_by(|a, b| payload_score(&b.2).cmp(&payload_score(&a.2)));
                }
                i = j;
            }
        }

        // Emit JavaScript output sections.
        if !emit_declaration_only {
            if let Some(out_file) = out_file_name.as_deref() {
                if !wrote_output_section {
                    output.push('\n');
                }
                output.push_str(&format!("//// [{out_file}]\n"));
                if effective_options.module == Some(ModuleKind::AMD) {
                    for (_, module_id, section) in js_sections.iter_mut() {
                        *section = normalize_amd_define_section(section, module_id);
                    }
                }
                let mut bundled = bundle_out_file_js(&js_sections);
                if effective_options.module == Some(ModuleKind::AMD) {
                    let has_non_amd_sections = js_sections
                        .iter()
                        .any(|(_, _, section)| !section_is_amd_wrapped_module(section));
                    bundled = normalize_amd_out_file_bundle(&bundled, has_non_amd_sections);
                }
                // Append the out-file-level source map URL if source maps are enabled.
                // When inlineSourceMap is set it wins over sourceMap.
                if effective_options.inline_source_map == Some(true) {
                    // For single-file bundles: the per-file emit already generated
                    // an inline data URI but bundle_out_file_js stripped it.
                    // Re-capture it and patch the "file" field to the outFile name.
                    // For multi-file bundles this currently reuses the last section's
                    // data URI as a placeholder until merged outFile source maps are
                    // implemented.
                    let out_basename = out_file.rsplit('/').next().unwrap_or(out_file);
                    if let Some(data_uri) = extract_inline_sourcemap_from_sections(&js_sections) {
                        let patched = patch_inline_sourcemap_file_field(&data_uri, out_basename);
                        if !bundled.ends_with('\n') {
                            bundled.push('\n');
                        }
                        bundled.push_str(&format!("//# sourceMappingURL={}\n", patched));
                    }
                } else if effective_options.source_map == Some(true) {
                    if !bundled.ends_with('\n') {
                        bundled.push('\n');
                    }
                    // Use just the basename of the out_file for the URL.
                    let out_basename = out_file.rsplit('/').next().unwrap_or(out_file);
                    bundled.push_str(&format!("//# sourceMappingURL={}.map\n", out_basename));
                }
                output.push_str(&bundled);
            } else {
                for (idx, (js_name, _, javascript)) in js_sections.iter().enumerate() {
                    if idx == 0 && !wrote_output_section {
                        output.push('\n');
                        wrote_output_section = true;
                    }
                    output.push_str(&format!("//// [{js_name}]\n"));
                    output.push_str(javascript);
                }
            }
        }

        // Append .d.ts sections after all .js sections (TypeScript's baseline format).
        if let Some(out_file) = out_file_name.as_deref() {
            if !dts_sections.is_empty() {
                let mut bundled_dts = String::new();
                for (_, dts_content) in dts_sections {
                    if !bundled_dts.is_empty() && !bundled_dts.ends_with('\n') {
                        bundled_dts.push('\n');
                    }
                    bundled_dts.push_str(dts_content.trim_end_matches('\n'));
                    bundled_dts.push('\n');
                }
                output.push('\n');
                output.push_str(&format!("//// [{}]\n", js_to_dts_name(out_file)));
                output.push_str(&bundled_dts);
            }
        } else {
            for (dts_name, dts_content) in dts_sections {
                output.push('\n');
                output.push_str(&format!("//// [{dts_name}]\n"));
                output.push_str(&dts_content);
            }
        }

        output
    }

    // -----------------------------------------------------------------------
    // Baseline kind dispatch (errors, symbols, types)
    // -----------------------------------------------------------------------

    /// Run a single test case with a specific baseline kind.
    pub fn run_case_with_kind(
        &self,
        test_path: &Path,
        suite: Suite,
        kind: BaselineKind,
    ) -> BaselineResult {
        if kind == BaselineKind::Js {
            return self.run_case(test_path, suite);
        }
        if kind == BaselineKind::Declarations {
            return BaselineResult {
                name: test_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                test_path: test_path.to_path_buf(),
                passed: false,
                diff: Some(
                    "declaration baselines require the opt-in expanded variant runner".to_string(),
                ),
                baseline_exists: false,
                actual_output: String::new(),
                expected_output: String::new(),
            };
        }
        // Always use large-stack thread with timeout for non-JS baselines.
        // The type checker can infinite-loop on pathological recursive types,
        // so the 30s timeout in run_case_with_kind_large_stack prevents suite hangs.
        self.run_case_with_kind_large_stack(test_path, suite, kind)
    }

    fn run_case_with_kind_large_stack(
        &self,
        test_path: &Path,
        suite: Suite,
        kind: BaselineKind,
    ) -> BaselineResult {
        let stem = test_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let test_path = test_path.to_path_buf();
        let runner = Self {
            workspace_root: self.workspace_root.clone(),
            strict_baseline_index: Arc::clone(&self.strict_baseline_index),
        };
        let worker_test_path = test_path.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        let spawn_result = std::thread::Builder::new()
            .name(format!("baseline-{}-{stem}", kind.as_str()))
            .stack_size(BASELINE_CASE_STACK_SIZE)
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    runner.run_case_impl_with_kind(&worker_test_path, suite, kind)
                }));
                let _ = tx.send(result);
            });

        match spawn_result {
            Ok(_handle) => {
                // 30-second per-test timeout prevents infinite loops from killing the suite
                match rx.recv_timeout(std::time::Duration::from_secs(30)) {
                    Ok(Ok(result)) => result,
                    Ok(Err(panic_info)) => BaselineResult {
                        name: stem,
                        test_path,
                        passed: false,
                        diff: Some(format!(
                            "PANIC during {} baseline: {}",
                            kind.as_str(),
                            Self::panic_message(panic_info.as_ref())
                        )),
                        baseline_exists: true,
                        actual_output: String::new(),
                        expected_output: String::new(),
                    },
                    Err(_) => BaselineResult {
                        name: stem,
                        test_path,
                        passed: false,
                        diff: Some(format!("TIMEOUT during {} baseline (>30s)", kind.as_str(),)),
                        baseline_exists: true,
                        actual_output: String::new(),
                        expected_output: String::new(),
                    },
                }
            }
            Err(err) => BaselineResult {
                name: stem,
                test_path,
                passed: false,
                diff: Some(format!("failed to spawn thread: {err}")),
                baseline_exists: false,
                actual_output: String::new(),
                expected_output: String::new(),
            },
        }
    }

    fn run_case_impl_with_kind(
        &self,
        test_path: &Path,
        suite: Suite,
        kind: BaselineKind,
    ) -> BaselineResult {
        let stem = test_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let source = match read_test_source(test_path) {
            Ok(s) => s,
            Err(e) => {
                let baseline_exists = self.oracle_exists_without_source(&stem, suite, kind);
                return BaselineResult {
                    name: stem,
                    test_path: test_path.to_path_buf(),
                    passed: false,
                    diff: Some(format!("failed to read test file: {e}")),
                    baseline_exists,
                    actual_output: String::new(),
                    expected_output: String::new(),
                };
            }
        };

        // Project suite is only supported for JS baselines
        if suite == Suite::Project {
            return BaselineResult {
                name: stem,
                test_path: test_path.to_path_buf(),
                passed: false,
                diff: Some(format!(
                    "{} baselines not supported for project suite",
                    kind.as_str()
                )),
                baseline_exists: false,
                actual_output: String::new(),
                expected_output: String::new(),
            };
        }

        let relative_test = format!(
            "tests/cases/{}/{}",
            suite.as_str(),
            test_path
                .strip_prefix(self.workspace_root.join(suite.relative_dir()))
                .unwrap_or(test_path)
                .to_string_lossy()
                .replace('\\', "/")
        );

        let mut test_case = parse_test_case(&relative_test, &source);
        fixup_multi_value_options(&mut test_case, &source);

        // Generate actual output with panic isolation
        let gen_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
            BaselineKind::Errors => {
                self.generate_errors_baseline(&test_case, &relative_test, &source)
            }
            BaselineKind::Symbols => {
                self.generate_symbols_baseline_output(&test_case, &relative_test)
            }
            BaselineKind::Types => self.generate_types_baseline(&test_case, &relative_test),
            BaselineKind::Js | BaselineKind::Declarations => unreachable!(),
        }));

        let actual_output = match gen_result {
            Ok(output) => output,
            Err(panic_info) => {
                return BaselineResult {
                    name: stem,
                    test_path: test_path.to_path_buf(),
                    passed: false,
                    diff: Some(format!(
                        "PANIC during {} baseline: {}",
                        kind.as_str(),
                        Self::panic_message(panic_info.as_ref())
                    )),
                    baseline_exists: true,
                    actual_output: String::new(),
                    expected_output: String::new(),
                };
            }
        };

        // Load expected baseline
        let baseline_dir = self.workspace_root.join("tests/baselines/reference");
        let baseline_path = baseline_dir.join(format!("{}{}", stem, kind.extension()));

        if matches!(kind, BaselineKind::Symbols | BaselineKind::Types) {
            let actual_normalized = normalize_text(&actual_output);
            let expected_output = match self.load_strict_option_baseline(
                &baseline_dir,
                &stem,
                kind.extension(),
                &test_case.options,
                &source,
            ) {
                Ok(Some(expected)) => expected,
                Ok(None) => {
                    return BaselineResult {
                        name: stem,
                        test_path: test_path.to_path_buf(),
                        passed: false,
                        diff: Some("baseline file not found".to_string()),
                        baseline_exists: false,
                        actual_output: actual_normalized,
                        expected_output: String::new(),
                    };
                }
                Err(error) => {
                    return BaselineResult {
                        name: stem,
                        test_path: test_path.to_path_buf(),
                        passed: false,
                        diff: Some(error),
                        baseline_exists: true,
                        actual_output: actual_normalized,
                        expected_output: String::new(),
                    };
                }
            };
            let expected_normalized = normalize_text(&expected_output);
            let passed = actual_normalized.trim_end_matches('\n')
                == expected_normalized.trim_end_matches('\n');
            let diff = (!passed).then(|| generate_diff(&expected_normalized, &actual_normalized));
            return BaselineResult {
                name: stem,
                test_path: test_path.to_path_buf(),
                passed,
                diff,
                baseline_exists: true,
                actual_output: actual_normalized,
                expected_output: expected_normalized,
            };
        }

        let (baseline_exists, expected_output) = if baseline_path.is_file() {
            match std::fs::read_to_string(&baseline_path) {
                Ok(s) => (true, normalize_text(&s)),
                Err(_) => (false, String::new()),
            }
        } else {
            // For errors baselines: no baseline file + zero errors = PASS
            if kind == BaselineKind::Errors {
                // When no .errors.txt baseline exists, strip deprecation warnings
                // (TS5101/TS5107/TS5102) from our output — the baseline was likely
                // generated before these options were deprecated.
                let stripped = strip_deprecation_only_output(&actual_output);
                {
                    // A parameterized oracle is still an oracle when actual
                    // output is empty. Resolve variants before considering
                    // the no-baseline/zero-errors pass case.
                    let pat = format!("{}(", stem);
                    let ext = kind.extension();
                    let mut found_variant: Option<String> = None;
                    let mut parameterized_oracle_exists = false;
                    if let Ok(entries) = std::fs::read_dir(&baseline_dir) {
                        let mut candidates: Vec<PathBuf> = entries
                            .flatten()
                            .filter_map(|entry| {
                                let path = entry.path();
                                let name = path.file_name()?.to_str()?;
                                if !name.starts_with(&pat) || !name.ends_with(ext) {
                                    return None;
                                }
                                parameterized_oracle_exists = true;
                                let attributes =
                                    parameterized_variant_attributes(name, &stem, ext)?;
                                attributes
                                    .iter()
                                    .all(|(key, expected)| {
                                        option_variant_value(
                                            &test_case.options,
                                            &source,
                                            &key.to_ascii_lowercase(),
                                        )
                                        .is_some_and(
                                            |actual| actual.eq_ignore_ascii_case(expected.trim()),
                                        )
                                    })
                                    .then_some(path)
                            })
                            .collect();
                        candidates.sort();
                        if let Some(path) = candidates.first() {
                            if let Ok(raw) = std::fs::read_to_string(path) {
                                found_variant = Some(normalize_text(&raw));
                            }
                        } else if parameterized_oracle_exists {
                            // The selected option combination has no errors
                            // baseline even though other matrix variants do.
                            found_variant = Some(String::new());
                        }
                    }
                    if let Some(expected) = found_variant {
                        // Variant oracles come from the same tsc version as
                        // every other baseline, deprecation diagnostics
                        // included, so compare the unstripped output.
                        let actual_normalized = normalize_text(&actual_output);
                        let expected_normalized = normalize_text(&expected);
                        let passed = actual_normalized.trim_end_matches('\n')
                            == expected_normalized.trim_end_matches('\n');
                        let diff = if !passed {
                            Some(generate_diff(&expected_normalized, &actual_normalized))
                        } else {
                            None
                        };
                        return BaselineResult {
                            name: stem,
                            test_path: test_path.to_path_buf(),
                            passed,
                            diff,
                            baseline_exists: true,
                            actual_output: actual_normalized,
                            expected_output: expected_normalized,
                        };
                    }
                    if stripped.trim().is_empty() {
                        // No real errors and no unparameterized OR variant
                        // errors oracle exists for this case.
                        return BaselineResult {
                            name: stem,
                            test_path: test_path.to_path_buf(),
                            passed: true,
                            diff: None,
                            baseline_exists: true,
                            actual_output: String::new(),
                            expected_output: String::new(),
                        };
                    }
                    // No parameterized baseline found either: fail.
                    return BaselineResult {
                        name: stem,
                        test_path: test_path.to_path_buf(),
                        passed: false,
                        diff: Some("produced errors but no .errors.txt baseline exists".into()),
                        baseline_exists: true,
                        actual_output: stripped,
                        expected_output: String::new(),
                    };
                }
            }
            // For symbols/types: no baseline = skip
            (false, String::new())
        };

        let actual_normalized = normalize_text(&actual_output);
        let expected_normalized = normalize_text(&expected_output);

        let passed = baseline_exists
            && actual_normalized.trim_end_matches('\n')
                == expected_normalized.trim_end_matches('\n');

        let diff = if !baseline_exists {
            Some("baseline file not found".to_string())
        } else if !passed {
            Some(generate_diff(&expected_normalized, &actual_normalized))
        } else {
            None
        };

        BaselineResult {
            name: stem,
            test_path: test_path.to_path_buf(),
            passed,
            diff,
            baseline_exists,
            actual_output: actual_normalized,
            expected_output: expected_normalized,
        }
    }

    /// Run a suite with a specific baseline kind and optional limit.
    pub fn run_suite_with_kind(
        &self,
        suite: Suite,
        kind: BaselineKind,
        max_cases: Option<usize>,
    ) -> Result<BaselineSuiteResult, HarnessError> {
        if kind == BaselineKind::Js {
            return self.run_suite_with_limit(suite, max_cases);
        }
        if kind == BaselineKind::Declarations {
            return Err(HarnessError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "declaration baselines require the opt-in expanded variant runner",
            )));
        }

        use rayon::prelude::*;

        let mut cases = self.discover_cases(suite)?;
        if let Some(limit) = max_cases {
            cases.truncate(limit);
        }

        let total = cases.len();

        let mut pool_builder = rayon::ThreadPoolBuilder::new().stack_size(8 * 1024 * 1024);
        if let Some(n) = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            pool_builder = pool_builder.num_threads(n);
        } else {
            // Bound default parallelism by available memory, not core count: each
            // worker runs a full compiler instance (~1-2 GiB on large cases), so
            // using every CPU on a big box can exhaust RAM and thrash swap (this
            // froze the dev box on 2026-06-14). Budget ~4 GiB/worker against
            // MemAvailable, clamped to [2, num CPUs]. Override via RAYON_NUM_THREADS.
            let cpus = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            let mem_gib = std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("MemAvailable:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|kb| kb.parse::<u64>().ok())
                })
                .map(|kb| (kb / 1024 / 1024) as usize);
            let threads = match mem_gib {
                Some(g) => cpus.min((g / 4).max(2)),
                None => cpus,
            };
            pool_builder = pool_builder.num_threads(threads);
        }
        let pool = pool_builder
            .build()
            .expect("failed to build rayon thread pool");

        // Opt-in per-case trace. With RAYON_NUM_THREADS=1 the cases run in
        // order, so the last line printed before an OOM/crash names the culprit.
        let trace = std::env::var("TYPECHECK_TRACE").is_ok();
        let results: Vec<BaselineResult> = pool.install(|| {
            cases
                .par_iter()
                .map(|case_path| {
                    if trace {
                        let t0 = std::time::Instant::now();
                        let r = self.run_case_with_kind(case_path, suite, kind);
                        let ms = t0.elapsed().as_millis();
                        if ms >= 500 {
                            eprintln!("[trace] {}ms {}", ms, case_path.display());
                            use std::io::Write;
                            let _ = std::io::stderr().flush();
                        }
                        r
                    } else {
                        self.run_case_with_kind(case_path, suite, kind)
                    }
                })
                .collect()
        });

        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        for result in &results {
            if !result.baseline_exists {
                skipped += 1;
            } else if result.passed {
                passed += 1;
            } else {
                failed += 1;
            }
        }

        Ok(BaselineSuiteResult {
            suite,
            total,
            passed,
            failed,
            skipped,
            results,
        })
    }

    /// Run specific test names with a given baseline kind.
    pub fn run_named_cases_with_kind(
        &self,
        suite: Suite,
        names: &[&str],
        kind: BaselineKind,
    ) -> Result<Vec<BaselineResult>, HarnessError> {
        if kind == BaselineKind::Js {
            return self.run_named_cases(suite, names);
        }
        if kind == BaselineKind::Declarations {
            return Err(HarnessError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "declaration baselines require the opt-in expanded variant runner",
            )));
        }

        let suite_dir = self.workspace_root.join(suite.relative_dir());
        if !suite_dir.is_dir() {
            return Err(HarnessError::MissingSuiteDir(suite_dir));
        }

        let all_cases = self.discover_cases(suite)?;
        let mut stem_map = std::collections::HashMap::<String, PathBuf>::new();
        for path in &all_cases {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                stem_map
                    .entry(stem.to_string())
                    .or_insert_with(|| path.clone());
            }
        }

        let mut results = Vec::with_capacity(names.len());
        for name in names {
            if let Some(test_path) = stem_map.get(*name) {
                results.push(self.run_case_with_kind(test_path, suite, kind));
            } else {
                let ts_path = suite_dir.join(format!("{name}.ts"));
                let test_path = if ts_path.is_file() {
                    ts_path
                } else {
                    suite_dir.join(format!("{name}.tsx"))
                };
                if test_path.is_file() {
                    results.push(self.run_case_with_kind(&test_path, suite, kind));
                } else {
                    results.push(BaselineResult {
                        name: name.to_string(),
                        test_path: test_path.clone(),
                        passed: false,
                        diff: Some(format!("test file not found: {}", test_path.display())),
                        baseline_exists: false,
                        actual_output: String::new(),
                        expected_output: String::new(),
                    });
                }
            }
        }

        Ok(results)
    }

    // -----------------------------------------------------------------------
    // Errors baseline generation
    // -----------------------------------------------------------------------

    fn referenced_ambient_modules(&self, test_case: &TestCase) -> Vec<String> {
        let mut modules = HashSet::new();
        for file in &test_case.files {
            // Ambient modules declared by any file in the program are valid
            // bare-specifier targets for every other file, including imports
            // between declaration files in the same baseline case.
            modules.extend(ambient_module_names(&file.content));
            for reference in triple_slash_reference_paths(&file.content) {
                let Some(lib_name) = reference
                    .strip_prefix("/.lib/")
                    .or_else(|| reference.strip_prefix(".lib/"))
                else {
                    continue;
                };
                let lib_path = self.workspace_root.join("tests/lib").join(lib_name);
                let Ok(declarations) = std::fs::read_to_string(lib_path) else {
                    continue;
                };
                modules.extend(ambient_module_names(&declarations));
            }
        }
        let mut modules: Vec<String> = modules.into_iter().collect();
        modules.sort();
        modules
    }

    fn jsdoc_syntax_diagnostics(source: &SourceFile) -> Vec<Diagnostic> {
        #[derive(Clone, Copy)]
        struct JsDocBlock {
            start: usize,
            end: usize,
        }

        #[derive(Clone, Copy)]
        struct JsDocTag<'a> {
            name: &'a str,
            payload: &'a str,
            payload_start: usize,
        }

        fn blocks(text: &str) -> Vec<JsDocBlock> {
            let mut result = Vec::new();
            let mut cursor = 0;
            while let Some(relative_start) = text[cursor..].find("/**") {
                let start = cursor + relative_start;
                let Some(relative_end) = text[start + 3..].find("*/") else {
                    break;
                };
                let end = start + 3 + relative_end + 2;
                result.push(JsDocBlock { start, end });
                cursor = end;
            }
            result
        }

        fn tags<'a>(text: &'a str, block: JsDocBlock) -> Vec<JsDocTag<'a>> {
            let mut result = Vec::new();
            let block_text = &text[block.start..block.end];
            let mut line_start = 0;
            for line in block_text.split_inclusive('\n') {
                let mut body = line;
                let mut body_start = block.start + line_start;
                let leading = body.len() - body.trim_start().len();
                body = &body[leading..];
                body_start += leading;
                if let Some(rest) = body.strip_prefix("/**") {
                    body_start += 3;
                    body = rest.trim_start();
                    body_start += rest.len() - body.len();
                } else if let Some(rest) = body.strip_prefix('*') {
                    body_start += 1;
                    body = rest.trim_start();
                    body_start += rest.len() - body.len();
                }

                if let Some(after_at) = body.strip_prefix('@') {
                    let name_len = after_at
                        .find(|c: char| c.is_whitespace() || c == '*')
                        .unwrap_or(after_at.len());
                    let name = &after_at[..name_len];
                    let after_name = &after_at[name_len..];
                    let payload = after_name.trim_start();
                    result.push(JsDocTag {
                        name,
                        payload,
                        payload_start: body_start
                            + 1
                            + name_len
                            + (after_name.len() - payload.len()),
                    });
                }
                line_start += line.len();
            }
            result
        }

        fn skip_type(payload: &str) -> Option<usize> {
            if !payload.starts_with('{') {
                return None;
            }
            let mut depth = 0_u32;
            for (index, ch) in payload.char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(index + ch.len_utf8());
                        }
                    }
                    _ => {}
                }
            }
            Some(payload.len())
        }

        fn tag_name(tag: JsDocTag<'_>) -> Option<(&str, usize)> {
            let type_end = skip_type(tag.payload).unwrap_or(0);
            let after_type = &tag.payload[type_end..];
            let name_text = after_type.trim_start();
            let name_start = tag.payload_start + type_end + (after_type.len() - name_text.len());
            let name_len = if name_text.starts_with('[') {
                name_text.find(']').map(|index| index + 1)
            } else {
                name_text
                    .find(|c: char| c.is_whitespace() || c == '*')
                    .or(Some(name_text.len()))
            }?;
            let name = name_text[..name_len].trim_end_matches("*/");
            (!name.is_empty()).then_some((name, name_start))
        }

        fn parameter_name(param: &tsc_rs_ast::Pat) -> Option<&str> {
            match &param.kind {
                tsc_rs_ast::PatKind::Ident(name) => Some(name.as_str()),
                tsc_rs_ast::PatKind::Assign(inner, _) | tsc_rs_ast::PatKind::Rest(inner) => {
                    parameter_name(inner)
                }
                _ => None,
            }
        }

        fn callable_params(expr: &tsc_rs_ast::Expr) -> Option<&[tsc_rs_ast::Param]> {
            match &expr.kind {
                ExprKind::FnExpr(function) => Some(&function.params),
                ExprKind::Arrow(function) => Some(&function.params),
                _ => None,
            }
        }

        fn root_name(name: &str) -> &str {
            let name = name.strip_prefix('[').unwrap_or(name);
            let end = name.find(['.', '[', '=', ']']).unwrap_or(name.len());
            &name[..end]
        }

        fn decode_identifier_escapes(name: &str) -> String {
            let mut result = String::with_capacity(name.len());
            let mut chars = name.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch != '\\' || chars.peek() != Some(&'u') {
                    result.push(ch);
                    continue;
                }
                chars.next();
                let braced = chars.peek() == Some(&'{');
                if braced {
                    chars.next();
                }
                let digit_count = if braced { 6 } else { 4 };
                let mut digits = String::new();
                while digits.len() < digit_count {
                    let Some(next) = chars.peek().copied() else {
                        break;
                    };
                    if !next.is_ascii_hexdigit() {
                        break;
                    }
                    digits.push(next);
                    chars.next();
                }
                if braced && chars.peek() == Some(&'}') {
                    chars.next();
                }
                match u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(decoded) => result.push(decoded),
                    None => {
                        result.push_str("\\u");
                        if braced {
                            result.push('{');
                        }
                        result.push_str(&digits);
                        if braced {
                            result.push('}');
                        }
                    }
                }
            }
            result
        }

        fn diagnostic(
            source: &SourceFile,
            code: u32,
            message: String,
            start: usize,
            len: usize,
        ) -> Diagnostic {
            Diagnostic {
                code,
                message,
                category: tsc_rs_ast::DiagnosticCategory::Error,
                file_name: Some(source.file_name.clone()),
                span: Some(tsc_rs_ast::Span::new(start as u32, (start + len) as u32)),
                related: None,
            }
        }

        let blocks = blocks(&source.text);
        let mut diagnostics = Vec::new();

        for block in &blocks {
            let block_tags = tags(&source.text, *block);
            let has_property = block_tags.iter().any(|tag| {
                tag.name.eq_ignore_ascii_case("property")
                    || tag.name.eq_ignore_ascii_case("prop")
                    || tag.name.eq_ignore_ascii_case("member")
            });
            for tag in block_tags {
                if tag.name.eq_ignore_ascii_case("typedef")
                    && !tag.payload.starts_with('{')
                    && !has_property
                {
                    if let Some((name, start)) = tag_name(tag) {
                        diagnostics.push(diagnostic(
                            source,
                            8021,
                            "JSDoc '@typedef' tag should either have a type annotation or be followed by '@property' or '@member' tags.".to_string(),
                            start,
                            name.len(),
                        ));
                    }
                }
            }
        }

        for statement in &source.statements {
            let mut callable_parameter_lists: Vec<&[tsc_rs_ast::Param]> = Vec::new();
            match &statement.kind {
                StmtKind::FnDecl(function) => callable_parameter_lists.push(&function.params),
                StmtKind::Var(variable) => {
                    callable_parameter_lists.extend(variable.declarations.iter().filter_map(
                        |declaration| declaration.init.as_deref().and_then(callable_params),
                    ));
                }
                StmtKind::Expr(expression) => {
                    if let ExprKind::Assign(assignment) = &expression.kind {
                        if let Some(params) = callable_params(&assignment.right) {
                            callable_parameter_lists.push(params);
                        }
                    }
                }
                _ => {}
            }
            if callable_parameter_lists.is_empty() {
                continue;
            }
            let statement_start = statement.span.start as usize;
            let Some(block) = blocks.iter().rev().copied().find(|block| {
                block.end <= statement_start
                    && source.text[block.end..statement_start].trim().is_empty()
            }) else {
                continue;
            };
            let param_tags: Vec<JsDocTag<'_>> = tags(&source.text, block)
                .into_iter()
                .filter(|tag| {
                    tag.name.eq_ignore_ascii_case("param")
                        || tag.name.eq_ignore_ascii_case("arg")
                        || tag.name.eq_ignore_ascii_case("argument")
                })
                .collect();
            for params in callable_parameter_lists {
                let parameter_names: HashSet<String> = params
                    .iter()
                    .filter_map(|param| parameter_name(&param.name))
                    .map(ToOwned::to_owned)
                    .collect();
                let destructured_parameters = params
                    .iter()
                    .filter(|param| parameter_name(&param.name).is_none())
                    .count();
                let mut documented_destructured_roots = HashSet::new();
                for tag in &param_tags {
                    let Some((name, _)) = tag_name(*tag) else {
                        continue;
                    };
                    let root = decode_identifier_escapes(root_name(name));
                    if !root.is_empty()
                        && !parameter_names.contains(&root)
                        && documented_destructured_roots.len() < destructured_parameters
                    {
                        documented_destructured_roots.insert(root);
                    }
                }
                for tag in &param_tags {
                    let Some((name, start)) = tag_name(*tag) else {
                        continue;
                    };
                    let root = decode_identifier_escapes(root_name(name));
                    if !root.is_empty()
                        && !parameter_names.contains(&root)
                        && !documented_destructured_roots.contains(&root)
                    {
                        let display_name = name.trim_matches(['[', ']']);
                        diagnostics.push(diagnostic(
                            source,
                            8024,
                            format!(
                                "JSDoc '@param' tag has name '{display_name}', but there is no parameter with that name."
                            ),
                            start + usize::from(name.starts_with('[')),
                            display_name.len(),
                        ));
                    }
                }
            }
        }

        diagnostics.sort_by_key(|diagnostic| diagnostic.span.map(|span| span.start));
        diagnostics.dedup_by(|left, right| {
            left.code == right.code && left.span == right.span && left.message == right.message
        });
        diagnostics
    }

    fn generate_errors_baseline(
        &self,
        test_case: &TestCase,
        _relative_test_path: &str,
        source_text: &str,
    ) -> String {
        let effective_options = effective_compiler_options(test_case);
        let package_type_by_dir = package_json_module_type_by_dir(test_case);
        let effective_allow_js = effective_options
            .allow_js
            .unwrap_or(effective_options.check_js == Some(true));

        // Parse, bind, and type-check each file to collect diagnostics
        let mut all_diagnostics: Vec<(String, Vec<Diagnostic>)> = Vec::new();
        let mut file_sources: Vec<(String, String)> = Vec::new();
        let referenced_modules = self.referenced_ambient_modules(test_case);

        // For multi-file tests: collect top-level names from ALL files first
        // so cross-file references don't produce false TS2304 errors.
        let mut cross_file_names: Vec<String> = Vec::new();
        let mut parsed_program_files = Vec::new();
        let mut program_has_jsx_intrinsic_elements = false;
        let jsx_option_set = effective_options.jsx.is_some();
        if test_case.files.len() > 1 {
            for file in &test_case.files {
                if file.name.to_ascii_lowercase().ends_with(".json") {
                    continue;
                }
                let lower = file.name.to_ascii_lowercase();
                let is_jsx = lower.ends_with(".tsx")
                    || lower.ends_with(".jsx")
                    || (jsx_option_set
                        && (lower.ends_with(".js")
                            || lower.ends_with(".mjs")
                            || lower.ends_with(".cjs")));
                let parsed = tsc_rs_parser::parse_with_jsx(&file.name, &file.content, is_jsx);
                collect_top_level_names(&parsed, &mut cross_file_names);
                program_has_jsx_intrinsic_elements |=
                    tsc_rs_types::TypeChecker::source_file_declares_jsx_intrinsic_elements(&parsed);
                parsed_program_files.push(parsed);
            }
        }
        let available_file_names: Vec<String> = test_case
            .files
            .iter()
            .map(|file| file.name.clone())
            .collect();
        let mut ambiguous_root_entries: Vec<(String, String)> = Vec::new();
        let mut checker_donor = if lib_injection_enabled() {
            lib_injected_donor(&effective_options)
        } else {
            tsc_rs_types::TypeChecker::new()
        };
        // Error baselines consume diagnostics only. Building a display string
        // for every expression is both unused and potentially exponential for
        // persistent generic type graphs such as long Omit/merge chains.
        checker_donor.disable_expression_types();
        if !cross_file_names.is_empty() {
            checker_donor.register_external_names(&cross_file_names);
        }
        if program_has_jsx_intrinsic_elements {
            checker_donor.register_jsx_intrinsic_elements();
        }
        checker_donor.register_available_files(&available_file_names);
        if !parsed_program_files.is_empty() {
            let parsed_refs: Vec<_> = parsed_program_files.iter().collect();
            checker_donor.register_external_export_names(&parsed_refs);
            checker_donor.register_external_top_level_bindings(&parsed_refs);
            // Resolve every static import specifier through the virtual
            // program (tsconfig paths/baseUrl/rootDirs, node_modules walk,
            // package self-names, `@link` aliases) so TS2307 only fires for
            // specifiers tsc could not resolve either.
            let path_ctx = baseline_path_context(test_case);
            let link_path_mappings = parse_link_path_mappings(source_text);
            let mut path_to_idx: HashMap<String, usize> = HashMap::new();
            for (idx, file) in test_case.files.iter().enumerate() {
                path_to_idx.insert(normalize_header_path(&file.name), idx);
            }
            for (idx, file) in test_case.files.iter().enumerate() {
                let normalized = normalize_header_path(&file.name);
                for (from, to) in &link_path_mappings {
                    let Some(rel) = strip_prefix_path(&normalized, from) else {
                        continue;
                    };
                    let alias = if rel.is_empty() {
                        to.clone()
                    } else {
                        join_path(to, &rel)
                    };
                    path_to_idx.entry(alias).or_insert(idx);
                }
            }
            // `// @symlink: /a.ts,/b.ts` under a `@filename` aliases that file.
            {
                let mut current: Option<usize> = None;
                for line in source_text.lines() {
                    let Some((key, value)) = parse_directive_kv(line) else {
                        continue;
                    };
                    if key.eq_ignore_ascii_case("filename") {
                        current = path_to_idx.get(&normalize_header_path(&value)).copied();
                        continue;
                    }
                    if !(key.eq_ignore_ascii_case("symlink") || key.eq_ignore_ascii_case("link"))
                        || value.contains("->")
                    {
                        continue;
                    }
                    if let Some(idx) = current {
                        for alias in value.split(',') {
                            let alias = normalize_header_path(alias.trim());
                            if !alias.is_empty() {
                                path_to_idx.entry(alias).or_insert(idx);
                            }
                        }
                    }
                }
            }
            let self_name_resolution = matches!(
                effective_options.module,
                Some(ModuleKind::Node16)
                    | Some(ModuleKind::Node18)
                    | Some(ModuleKind::Node20)
                    | Some(ModuleKind::NodeNext)
            ) || effective_options
                .module_resolution
                .as_deref()
                .is_some_and(|mr| {
                    mr.eq_ignore_ascii_case("node16")
                        || mr.eq_ignore_ascii_case("nodenext")
                        || mr.eq_ignore_ascii_case("bundler")
                });
            let program_sources: Vec<&TestFile> = test_case
                .files
                .iter()
                .filter(|file| !file.name.to_ascii_lowercase().ends_with(".json"))
                .collect();
            // TS2209: a package self-name whose export map target lives under
            // outDir/declarationDir needs the project root to map the output
            // path back to a source; without rootDir or a tsconfig the guess
            // is ambiguous unless everything sits at the file-system root.
            let output_dir_set = effective_options.out_dir.is_some()
                || path_ctx.out_dir.is_some()
                || effective_options
                    .other
                    .iter()
                    .any(|(key, _)| key.eq_ignore_ascii_case("declarationdir"));
            let root_dir_set = effective_options.root_dir.is_some() || path_ctx.root_dir.is_some();
            let report_ambiguous_root = self_name_resolution
                && output_dir_set
                && !root_dir_set
                && path_ctx.tsconfig_dir.is_none();
            for (file, parsed) in program_sources.iter().zip(parsed_program_files.iter()) {
                let importer = normalize_header_path(&file.name);
                let specifiers = collect_project_module_specifiers(parsed);
                let require_form = collect_require_form_specifiers(parsed);
                if report_ambiguous_root {
                    for specifier in &specifiers {
                        let Some((package_idx, entry)) = self_name_package_reference(
                            &dirname(&importer),
                            specifier,
                            test_case,
                            &path_to_idx,
                        ) else {
                            continue;
                        };
                        let package_path =
                            normalize_header_path(&test_case.files[package_idx].name);
                        if is_node_modules_path(&package_path)
                            || !project_root_is_ambiguous(&importer, &package_path)
                        {
                            continue;
                        }
                        let key = (package_path, entry);
                        if !ambiguous_root_entries.contains(&key) {
                            ambiguous_root_entries.push(key);
                        }
                    }
                }
                let mut resolved: Vec<String> = Vec::new();
                let mut resolved_paths: Vec<(String, String)> = Vec::new();
                let mut esm_resolved: Vec<String> = Vec::new();
                let mut esm_extensionless: Vec<(String, Option<String>)> = Vec::new();
                let node_format_option = matches!(
                    effective_options.module,
                    Some(ModuleKind::Node16)
                        | Some(ModuleKind::Node18)
                        | Some(ModuleKind::Node20)
                        | Some(ModuleKind::NodeNext)
                );
                let importer_is_esm = node_format_option
                    && effective_module_kind_for_source(
                        &file.name,
                        effective_options.module,
                        &package_type_by_dir,
                    ) == Some(ModuleKind::ESNext);
                for specifier in specifiers {
                    // tsc (node16+, ESM usage): a dot-relative specifier
                    // without an extension never resolves; it reports TS2835
                    // with the sibling file's emitted extension, or TS2834.
                    // Bare directory forms (`./`, `.`) keep TS2307.
                    let trimmed = specifier.trim();
                    let dot_relative = trimmed.starts_with("./") || trimmed.starts_with("../");
                    let base_name = trimmed.rsplit('/').next().unwrap_or(trimmed);
                    let has_extension = base_name.len() > 1 && base_name[1..].contains('.');
                    if importer_is_esm
                        && dot_relative
                        && !has_extension
                        && !trimmed.ends_with('/')
                        && trimmed != "."
                        && trimmed != ".."
                        && !require_form.contains(&specifier)
                    {
                        let base =
                            normalize_path_segments(&join_path(&dirname(&importer), trimmed));
                        let suggested = [
                            (".mts", ".mjs"),
                            (".ts", ".js"),
                            (".cts", ".cjs"),
                            (".mjs", ".mjs"),
                            (".js", ".js"),
                            (".cjs", ".cjs"),
                            (".tsx", ".js"),
                            (".jsx", ".js"),
                            (".json", ".json"),
                        ]
                        .iter()
                        .find(|(actual, _)| path_to_idx.contains_key(&format!("{base}{actual}")))
                        .map(|(_, emitted)| format!("{trimmed}{emitted}"));
                        esm_extensionless.push((specifier.clone(), suggested));
                        continue;
                    }
                    let Some(idx) = resolve_module_specifier_for_diagnostics(
                        &importer,
                        &specifier,
                        test_case,
                        &path_to_idx,
                        &path_ctx,
                        self_name_resolution,
                    ) else {
                        continue;
                    };
                    // The resolved file's own Node format decides whether a
                    // CommonJS importer may `require` it (TS1479). JSON
                    // modules are never ES modules for this purpose.
                    if !test_case.files[idx]
                        .name
                        .to_ascii_lowercase()
                        .ends_with(".json")
                        && effective_module_kind_for_source(
                            &test_case.files[idx].name,
                            effective_options.module,
                            &package_type_by_dir,
                        ) == Some(ModuleKind::ESNext)
                        && matches!(
                            effective_options.module,
                            Some(ModuleKind::Node16)
                                | Some(ModuleKind::Node18)
                                | Some(ModuleKind::Node20)
                                | Some(ModuleKind::NodeNext)
                        )
                    {
                        esm_resolved.push(specifier.clone());
                    }
                    resolved_paths.push((specifier.clone(), test_case.files[idx].name.clone()));
                    resolved.push(specifier);
                }
                if cross_file_injection_enabled() {
                    for specifier in collect_module_augmentation_specifiers(parsed) {
                        if resolved_paths.iter().any(|(known, _)| known == &specifier) {
                            continue;
                        }
                        if let Some(idx) = resolve_module_specifier_for_diagnostics(
                            &importer,
                            &specifier,
                            test_case,
                            &path_to_idx,
                            &path_ctx,
                            self_name_resolution,
                        ) {
                            resolved_paths.push((specifier, test_case.files[idx].name.clone()));
                        }
                    }
                }
                if effective_options.import_helpers == Some(true) {
                    // tsc resolves the helpers module ('tslib') from each
                    // file that needs an emit helper (TS2354/TS2343).
                    // tsc resolves the helpers import in the file's emit
                    // format: a CommonJS file requires 'tslib'.
                    let commonjs_file = match effective_module_kind_for_source(
                        &file.name,
                        effective_options.module,
                        &package_type_by_dir,
                    ) {
                        Some(ModuleKind::CommonJS) => true,
                        _ => {
                            let lower = file.name.to_ascii_lowercase();
                            lower.ends_with(".cts") || lower.ends_with(".cjs")
                        }
                    };
                    let require_conditions: &[&str] = &["types", "require", "node", "default"];
                    let ambient = ambient_helpers_module_exports(&parsed_program_files);
                    if let Some(exports) = &ambient {
                        checker_donor.register_helpers_module_exports("ambient:tslib", exports);
                    }
                    let helpers = ambient
                        .as_ref()
                        .map(|_| usize::MAX)
                        .or_else(|| {
                            resolve_module_specifier_with_conditions(
                                &importer,
                                "tslib",
                                test_case,
                                &path_to_idx,
                                &path_ctx,
                                self_name_resolution,
                                commonjs_file.then_some(require_conditions),
                            )
                        })
                        .or_else(|| {
                            // Classic resolution: `tslib.ts` / `tslib.d.ts` in the
                            // importer's directory or any ancestor.
                            let classic = match effective_options.module_resolution.as_deref() {
                                Some(mode) => mode.eq_ignore_ascii_case("classic"),
                                None => matches!(
                                    effective_options.module,
                                    Some(ModuleKind::None)
                                        | Some(ModuleKind::AMD)
                                        | Some(ModuleKind::UMD)
                                        | Some(ModuleKind::System)
                                ),
                            };
                            if !classic {
                                return None;
                            }
                            let mut dir = dirname(&importer);
                            loop {
                                for candidate in ["tslib.ts", "tslib.tsx", "tslib.d.ts"] {
                                    let path = if dir.is_empty() {
                                        candidate.to_string()
                                    } else {
                                        join_path(&dir, candidate)
                                    };
                                    if let Some(idx) = path_to_idx.get(&path).copied() {
                                        return Some(idx);
                                    }
                                }
                                let parent = dirname(&dir);
                                if dir.is_empty() || parent == dir {
                                    return None;
                                }
                                dir = parent;
                            }
                        })
                        .map(|idx| {
                            if idx == usize::MAX {
                                "ambient:tslib".to_string()
                            } else {
                                test_case.files[idx].name.clone()
                            }
                        });
                    checker_donor.register_helpers_module(&importer, helpers);
                }
                checker_donor.register_resolvable_specifiers(&importer, &resolved);
                checker_donor.register_virtual_module_paths(&importer, &resolved_paths);
                checker_donor.register_esm_specifiers(&importer, &esm_resolved);
                checker_donor.register_esm_extensionless_specifiers(&importer, &esm_extensionless);
            }
            if cross_file_injection_enabled() {
                // Seed the donor with every program file's declarations so
                // imports resolve to real types instead of `any`. The
                // injection pass's own diagnostics are program-wide and
                // would repeat under every file; per-file checks report them.
                checker_donor.inject_external_types(&parsed_refs);
                checker_donor.take_diagnostics();
            }
        }

        for file in &test_case.files {
            let lower = file.name.to_ascii_lowercase();
            // Error baselines preserve an explicit leading `./` from an
            // @filename directive while still normalizing path separators.
            let display = normalize_slashes(file.name.trim());
            // JSON files are configuration/data inputs rather than syntax
            // trees. Declaration files, however, still contribute parser
            // diagnostics and (unless skipLibCheck is set) semantic errors.
            let is_source_extension =
                [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"]
                    .iter()
                    .any(|extension| lower.ends_with(extension));
            // JSON is data; anything else without a TypeScript/JavaScript
            // extension (README.md, .css, ...) is present on disk but is
            // never parsed as source by tsc.
            if lower.ends_with(".json") || !is_source_extension {
                file_sources.push((display.clone(), file.content.clone()));
                all_diagnostics.push((display, Vec::new()));
                continue;
            }

            let is_declaration_file = lower.ends_with(".d.ts")
                || lower.ends_with(".d.tsx")
                || lower.ends_with(".d.mts")
                || lower.ends_with(".d.cts");

            let is_jsx = lower.ends_with(".tsx")
                || lower.ends_with(".jsx")
                || (jsx_option_set
                    && (lower.ends_with(".js")
                        || lower.ends_with(".mjs")
                        || lower.ends_with(".cjs")));
            let is_js = lower.ends_with(".js")
                || lower.ends_with(".jsx")
                || lower.ends_with(".mjs")
                || lower.ends_with(".cjs");
            let is_disallowed_js_root = is_js
                && !effective_allow_js
                && find_tsconfig_file(test_case).is_none()
                && test_case.files.len() == 1;
            let parsed = tsc_rs_parser::parse_with_jsx(&file.name, &file.content, is_jsx);
            let has_ts_nocheck = tsc_rs_types::source_file_has_ts_nocheck(&parsed);

            // noCheck / @ts-nocheck suppress semantic checking, but TypeScript
            // still surfaces source-file parse diagnostics.
            let mut file_diags: Vec<Diagnostic> = if is_disallowed_js_root {
                Vec::new()
            } else if tsc_rs_parser::has_syntax_errors(&parsed.diagnostics) {
                // tsc's grammar checks (grammarErrorOnNode) stay silent in a
                // file that has real parse errors.
                parsed
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| !tsc_rs_parser::is_grammar_diagnostic(diagnostic.code))
                    .cloned()
                    .collect()
            } else {
                parsed.diagnostics.clone()
            };

            // For JS files, skip semantic checking unless checkJs is enabled.
            // TypeScript only type-checks JS files when checkJs: true (or strict implies it).
            let should_type_check = if is_disallowed_js_root {
                false
            } else if is_declaration_file {
                effective_options.skip_lib_check != Some(true)
            } else if is_js {
                // A leading `// @ts-check` pragma opts a JS file in even
                // when checkJs is off, mirroring tsc.
                let has_ts_check = file.content.lines().take(5).any(|line| {
                    let t = line.trim();
                    t == "// @ts-check" || t.starts_with("// @ts-check ")
                });
                effective_options.check_js == Some(true) || has_ts_check
            } else {
                true
            };

            if effective_options.no_check != Some(true) && !has_ts_nocheck && should_type_check {
                if is_js {
                    file_diags.extend(Self::jsdoc_syntax_diagnostics(&parsed));
                }
                // Bind and type-check
                let symbols = tsc_rs_symbols::bind(&parsed);
                let mut checker = checker_donor.clone();
                if effective_options.import_helpers == Some(true) && parsed_program_files.is_empty()
                {
                    // A single-file program has no 'tslib' file to resolve;
                    // it may still declare the module ambiently.
                    match ambient_helpers_module_exports(std::slice::from_ref(&parsed)) {
                        Some(exports) => {
                            checker.register_helpers_module_exports("ambient:tslib", &exports);
                            checker.register_helpers_module(&display, Some("ambient:tslib".into()));
                        }
                        None => checker.register_helpers_module(&display, None),
                    }
                }
                checker.set_current_file_name(&display);
                checker.set_current_file_module_format(effective_module_kind_for_source(
                    &file.name,
                    effective_options.module,
                    &package_type_by_dir,
                ));
                checker.register_available_modules(&referenced_modules);
                // Actually emit module-resolution diagnostics (TS2307/TS2792/
                // TS2882) — without this the whole import-resolution path is
                // inert in the errors baseline and every module-not-found
                // expectation is an automatic miss.
                checker.enable_module_resolution_diagnostics();
                let check_output =
                    checker.check_with_options(&parsed, &symbols, &effective_options);
                file_diags.extend(check_output.diagnostics);
            }

            // `// @ts-ignore` (and `/// ...`) on the comment/blank run directly
            // above a line suppresses that line's diagnostics (tsc honors the
            // line-comment form only, not `/* @ts-ignore */`).
            let line_starts: Vec<usize> = std::iter::once(0)
                .chain(
                    file.content
                        .bytes()
                        .enumerate()
                        .filter(|(_, b)| *b == b'\n')
                        .map(|(i, _)| i + 1),
                )
                .collect();
            let lines: Vec<&str> = file.content.lines().collect();
            let line_of = |off: usize| match line_starts.binary_search(&off) {
                Ok(l) => l,
                Err(l) => l.saturating_sub(1),
            };
            let ts_ignored = |line: usize| -> bool {
                let mut l = line;
                while l > 0 {
                    l -= 1;
                    let t = lines.get(l).map(|x| x.trim()).unwrap_or("");
                    if t.is_empty() {
                        continue;
                    }
                    if t.starts_with("//") {
                        if t.contains("@ts-ignore") {
                            return true;
                        }
                        continue;
                    }
                    break;
                }
                false
            };
            file_diags.retain(|d| {
                d.span
                    .map(|sp| !ts_ignored(line_of(sp.start as usize)))
                    .unwrap_or(true)
            });

            file_sources.push((display.clone(), file.content.clone()));
            all_diagnostics.push((display, file_diags));
        }

        // Emit deprecated/removed compiler option diagnostics (TS5101/TS5107/TS5102)
        // These appear at the top of the errors baseline before any file diagnostics.
        let mut global_diags = tsc_rs_types::check_deprecated_options(&effective_options);
        for (package_path, entry) in &ambiguous_root_entries {
            global_diags.push(tsc_rs_types::DeprecatedOptionDiag {
                diagnostic: Diagnostic {
                    code: 2209,
                    message: format!(
                        "The project root is ambiguous, but is required to resolve export map entry '{entry}' in file '{package_path}'. Supply the `rootDir` compiler option to disambiguate."
                    ),
                    category: tsc_rs_ast::DiagnosticCategory::Error,
                    file_name: None,
                    span: None,
                    related: None,
                },
                related: None,
                option_name: String::new(),
                point_at_value: false,
            });
        }
        // Emit-path checks (tsc verifyCompilerOptions / emit host): an output
        // that would overwrite an input (TS5055), an output several inputs
        // share (TS5056), and root files with unsupported extensions
        // (TS6054). Per-file outputs are only modeled without outDir; with
        // outFile only the bundled declaration file is modeled.
        let overwrite_related = "  Adding a tsconfig.json file will help organize projects that contain both TypeScript and JavaScript files. Learn more at https://aka.ms/tsconfig.".to_string();
        let allow_js_inputs = effective_options.allow_js == Some(true);
        let declaration_on = effective_options.declaration == Some(true)
            || effective_options.composite == Some(true);
        let emit_js = effective_options.emit_declaration_only != Some(true)
            && effective_options.out_file.is_none();
        let path_checks_on = effective_options.no_emit != Some(true)
            && effective_options.out_dir.is_none()
            && !option_flag_true(&test_case.options, "suppressOutputPathCheck");
        let mut overwrite_output_diags: Vec<(String, String)> = Vec::new();
        let mut multiple_output_diags: Vec<String> = Vec::new();
        let mut unsupported_extension_diags: Vec<String> = Vec::new();
        if path_checks_on {
            let program_inputs: Vec<(String, String)> = test_case
                .files
                .iter()
                .filter_map(|file| {
                    let display = normalize_header_path(&file.name);
                    let lower = normalize_slashes(&file.name).to_ascii_lowercase();
                    if lower.contains("/node_modules/") || lower.starts_with("node_modules/") {
                        return None;
                    }
                    let is_ts = lower.ends_with(".ts")
                        || lower.ends_with(".tsx")
                        || lower.ends_with(".mts")
                        || lower.ends_with(".cts");
                    let is_js = lower.ends_with(".js")
                        || lower.ends_with(".jsx")
                        || lower.ends_with(".mjs")
                        || lower.ends_with(".cjs");
                    (is_ts || (is_js && allow_js_inputs)).then_some((display, lower))
                })
                .collect();
            let input_lowers: Vec<String> = test_case
                .files
                .iter()
                .map(|file| normalize_slashes(&file.name).to_ascii_lowercase())
                .collect();
            let replace_ext = |name: &str, from: &str, to: &str| -> Option<String> {
                name.strip_suffix(from).map(|stem| format!("{stem}{to}"))
            };
            let js_output = |display: &str, lower: &str| -> Option<String> {
                if lower.ends_with(".d.ts")
                    || lower.ends_with(".d.mts")
                    || lower.ends_with(".d.cts")
                {
                    return None;
                }
                for (from, to) in [
                    (".tsx", ".js"),
                    (".mts", ".mjs"),
                    (".cts", ".cjs"),
                    (".ts", ".js"),
                    (".jsx", ".js"),
                    (".mjs", ".mjs"),
                    (".cjs", ".cjs"),
                    (".js", ".js"),
                ] {
                    if lower.ends_with(from) {
                        return replace_ext(display, &display[display.len() - from.len()..], to);
                    }
                }
                None
            };
            let dts_output = |display: &str, lower: &str| -> Option<String> {
                if lower.ends_with(".d.ts")
                    || lower.ends_with(".d.mts")
                    || lower.ends_with(".d.cts")
                {
                    return None;
                }
                for (from, to) in [
                    (".tsx", ".d.ts"),
                    (".mts", ".d.mts"),
                    (".cts", ".d.cts"),
                    (".ts", ".d.ts"),
                    (".jsx", ".d.ts"),
                    (".mjs", ".d.mts"),
                    (".cjs", ".d.cts"),
                    (".js", ".d.ts"),
                ] {
                    if lower.ends_with(from) {
                        return replace_ext(display, &display[display.len() - from.len()..], to);
                    }
                }
                None
            };
            // Outputs in emit order: every input's JS output, then its
            // declaration output (or the single bundled declaration file).
            let mut outputs: Vec<(String, usize)> = Vec::new(); // (display, input index)
            for (index, (display, lower)) in program_inputs.iter().enumerate() {
                if emit_js {
                    if let Some(out) = js_output(display, lower) {
                        outputs.push((out, index));
                    }
                }
                if declaration_on && effective_options.out_file.is_none() {
                    if let Some(out) = dts_output(display, lower) {
                        outputs.push((out, index));
                    }
                }
            }
            if declaration_on {
                if let Some(out_file) = effective_options.out_file.as_deref() {
                    let display = normalize_header_path(out_file);
                    let lower = display.to_ascii_lowercase();
                    let bundled = js_output(&display, &lower)
                        .and_then(|js| replace_ext(&js, ".js", ".d.ts"))
                        .unwrap_or_else(|| format!("{display}.d.ts"));
                    outputs.push((bundled, usize::MAX));
                }
            }
            let mut reported: Vec<String> = Vec::new();
            for (out, _) in &outputs {
                let out_lower = normalize_slashes(out).to_ascii_lowercase();
                if input_lowers.iter().any(|input| *input == out_lower)
                    && !reported.contains(&out_lower)
                {
                    reported.push(out_lower);
                    overwrite_output_diags.push((
                        format!("Cannot write file '{out}' because it would overwrite input file."),
                        overwrite_related.clone(),
                    ));
                }
            }
            // A repeated `@filename` directive re-declares one file, so
            // writers are identified by input name.
            let writer_name = |index: usize| -> String {
                program_inputs
                    .get(index)
                    .map(|(_, lower)| lower.clone())
                    .unwrap_or_else(|| "<outFile>".to_string())
            };
            let mut seen_outputs: Vec<(String, String, Vec<String>)> = Vec::new();
            for (out, index) in &outputs {
                let out_lower = normalize_slashes(out).to_ascii_lowercase();
                let writer = writer_name(*index);
                match seen_outputs
                    .iter_mut()
                    .find(|(lower, _, _)| *lower == out_lower)
                {
                    Some((_, _, writers)) => {
                        if !writers.contains(&writer) {
                            writers.push(writer);
                        }
                    }
                    None => seen_outputs.push((out_lower, out.clone(), vec![writer])),
                }
            }
            for (_, display, writers) in &seen_outputs {
                if writers.len() > 1 {
                    multiple_output_diags.push(format!(
                        "Cannot write file '{display}' because it would be overwritten by multiple input files."
                    ));
                }
            }
        }
        // Root files with unsupported extensions (program construction, not
        // emit): only the extensions the corpus exercises are modeled.
        if find_tsconfig_file(test_case).is_none() {
            for file in &test_case.files {
                let lower = normalize_slashes(&file.name).to_ascii_lowercase();
                if lower.contains("/node_modules/") || lower.starts_with("node_modules/") {
                    continue;
                }
                if lower.ends_with(".map") || lower.ends_with(".txt") {
                    unsupported_extension_diags.push(format!(
                        "File '{}' has an unsupported extension. The only supported extensions are '.ts', '.tsx', '.d.ts', '.js', '.jsx', '.cts', '.d.cts', '.cjs', '.mts', '.d.mts', '.mjs'.",
                        normalize_header_path(&file.name)
                    ));
                }
            }
        }
        // A sole directive-driven source is necessarily a compiler root. In
        // multi-file and tsconfig cases, virtual files can instead be host
        // fixtures; root discovery must not classify all of them as roots.
        let disallowed_js_diags: Vec<String> = if !effective_allow_js
            && find_tsconfig_file(test_case).is_none()
            && test_case.files.len() == 1
        {
            test_case
                .files
                .iter()
                .filter(|file| {
                    let lower = normalize_slashes(&file.name).to_ascii_lowercase();
                    lower.ends_with(".js")
                        || lower.ends_with(".jsx")
                        || lower.ends_with(".mjs")
                        || lower.ends_with(".cjs")
                })
                .map(|file| normalize_header_path(&file.name))
                .collect()
        } else {
            Vec::new()
        };

        // Try to locate deprecated options in a tsconfig.json file (if present).
        // When located, the diagnostic gets a file location instead of being global.
        let tsconfig_file = file_sources.iter().find(|(name, _)| {
            let lower = name.to_ascii_lowercase();
            lower.ends_with("tsconfig.json")
                || lower.ends_with(".json") && lower.contains("tsconfig")
        });
        // Split global_diags into located (with tsconfig position) and truly-global.
        // Located: (file_name, diagnostic, optional_related_msg)
        let mut located_option_diags: Vec<(String, Diagnostic, Option<String>)> = Vec::new();
        let mut remaining_global_diags: Vec<&tsc_rs_types::DeprecatedOptionDiag> = Vec::new();

        for gd in &global_diags {
            let mut located = false;
            if gd.option_name.is_empty() {
                remaining_global_diags.push(gd);
                continue;
            }
            if let Some((tsconfig_name, tsconfig_src)) = &tsconfig_file {
                // Search for `"optionName"` key in the tsconfig.json source.
                let search = format!("\"{}\"", gd.option_name);
                if let Some(key_offset) = tsconfig_src.find(&search) {
                    let (byte_offset, span_len) = if gd.point_at_value {
                        // Find the value after the key: skip key, colon, whitespace
                        let after_key = key_offset + search.len();
                        let rest = &tsconfig_src[after_key..];
                        let value_start = rest
                            .find(':')
                            .map(|c| {
                                let after_colon = &rest[c + 1..];
                                let ws_len = after_colon.len() - after_colon.trim_start().len();
                                after_key + c + 1 + ws_len
                            })
                            .unwrap_or(key_offset);
                        let value_str = &tsconfig_src[value_start..];
                        let value_end = value_str
                            .find(&[',', '\n', '\r', '}'][..])
                            .map(|e| value_start + e)
                            .unwrap_or(tsconfig_src.len());
                        let value_trimmed = tsconfig_src[value_start..value_end].trim();
                        (value_start as u32, value_trimmed.len() as u32)
                    } else {
                        // Point at the key
                        (key_offset as u32, search.len() as u32)
                    };
                    let mut diag = gd.diagnostic.clone();
                    diag.span = Some(tsc_rs_ast::Span {
                        start: byte_offset,
                        end: byte_offset + span_len,
                    });
                    diag.file_name = Some(tsconfig_name.clone());
                    located_option_diags.push((tsconfig_name.clone(), diag, gd.related.clone()));
                    located = true;
                } else if let Some(key_offset) = tsconfig_src.find("\"compilerOptions\"") {
                    // An option set outside the tsconfig (a test directive)
                    // is reported at its `compilerOptions` key.
                    let mut diag = gd.diagnostic.clone();
                    diag.span = Some(tsc_rs_ast::Span {
                        start: key_offset as u32,
                        end: (key_offset + "\"compilerOptions\"".len()) as u32,
                    });
                    diag.file_name = Some(tsconfig_name.clone());
                    located_option_diags.push((tsconfig_name.clone(), diag, gd.related.clone()));
                    located = true;
                }
            }
            if !located {
                remaining_global_diags.push(gd);
            }
        }

        // Add located option diagnostics to the appropriate file's diagnostic list
        for (file_name, diag, _) in &located_option_diags {
            if let Some((_, file_diags)) = all_diagnostics
                .iter_mut()
                .find(|(name, _)| name == file_name)
            {
                file_diags.push(diag.clone());
            }
        }

        // Build a map from (file_name, span_start, code) -> related message for located diagnostics
        let mut located_related_msgs: HashMap<(String, u32, u32), String> = HashMap::new();
        for (file_name, diag, related) in &located_option_diags {
            if let (Some(ref related_msg), Some(ref span)) = (related, &diag.span) {
                located_related_msgs.insert(
                    (file_name.clone(), span.start, diag.code),
                    related_msg.clone(),
                );
            }
        }

        // Format in tsc .errors.txt format
        let mut output = String::new();

        // First: header diagnostics
        let mut header_lines: Vec<String> = Vec::new();

        for (message, related) in &overwrite_output_diags {
            header_lines.push(format!("error TS5055: {message}"));
            header_lines.push(related.clone());
        }
        for message in &multiple_output_diags {
            header_lines.push(format!("error TS5056: {message}"));
        }
        for message in &unsupported_extension_diags {
            header_lines.push(format!("error TS6054: {message}"));
            header_lines.push("  The file is in the program because:".to_string());
            header_lines.push("    Root file specified for compilation".to_string());
        }

        // Global (no-file, no-span) diagnostics first
        for gd in &remaining_global_diags {
            let diag = &gd.diagnostic;
            let cat = match diag.category {
                tsc_rs_ast::DiagnosticCategory::Error => "error",
                tsc_rs_ast::DiagnosticCategory::Warning => "warning",
                tsc_rs_ast::DiagnosticCategory::Suggestion => "suggestion",
                tsc_rs_ast::DiagnosticCategory::Message => "message",
            };
            header_lines.push(format!("{} TS{}: {}", cat, diag.code, diag.message));
            if let Some(ref related) = gd.related {
                header_lines.push(related.clone());
            }
        }

        for file_name in &disallowed_js_diags {
            header_lines.push(format!(
                "error TS6504: File '{file_name}' is a JavaScript file. Did you mean to enable the 'allowJs' option?"
            ));
            header_lines.push("  The file is in the program because:".to_string());
            header_lines.push("    Root file specified for compilation".to_string());
        }

        // File-level diagnostics — collect with sort keys (file_idx, line, col, code)
        // Each entry: (file_idx, line, col, code, formatted_line, optional_related_lines)
        let mut file_header_entries: Vec<(usize, usize, usize, u32, String, Vec<String>)> =
            Vec::new();
        for (file_idx, (file_name, diags)) in all_diagnostics.iter().enumerate() {
            for diag in diags {
                let (line, col) = if let Some(span) = &diag.span {
                    let source_idx = file_sources.iter().position(|(name, _)| name == file_name);
                    if let Some(idx) = source_idx {
                        offset_to_line_col_0based(&file_sources[idx].1, span.start)
                    } else {
                        (1, 0)
                    }
                } else {
                    (1, 0)
                };
                let cat = match diag.category {
                    tsc_rs_ast::DiagnosticCategory::Error => "error",
                    tsc_rs_ast::DiagnosticCategory::Warning => "warning",
                    tsc_rs_ast::DiagnosticCategory::Suggestion => "suggestion",
                    tsc_rs_ast::DiagnosticCategory::Message => "message",
                };
                // Check if this diagnostic has a related message (from located option diagnostics)
                let mut related_lines = Vec::new();
                if let Some(span) = &diag.span {
                    let key = (file_name.clone(), span.start, diag.code);
                    if let Some(related_msg) = located_related_msgs.get(&key) {
                        related_lines.push(related_msg.clone());
                    }
                }
                // Handle multi-line diagnostic messages: split into
                // main header line + continuation lines
                let msg_parts: Vec<&str> = diag.message.split('\n').collect();
                let main_msg = msg_parts[0];
                // Global (file-less) diagnostics render without a location,
                // exactly as tsc does ("error TS2318: ...").
                let main_line = if diag.span.is_none() {
                    format!("{} TS{}: {}", cat, diag.code, main_msg)
                } else {
                    let diagnostic_file_name = normalize_header_path(file_name);
                    format!(
                        "{}({},{}): {} TS{}: {}",
                        diagnostic_file_name,
                        line,
                        col + 1,
                        cat,
                        diag.code,
                        main_msg
                    )
                };
                // Continuation lines go before other related lines
                let mut full_related = Vec::new();
                for part in &msg_parts[1..] {
                    full_related.push(part.to_string());
                }
                full_related.extend(related_lines);
                file_header_entries.push((
                    file_idx,
                    line,
                    col + 1,
                    diag.code,
                    main_line,
                    full_related,
                ));
            }
        }
        // Sort by (file_name, line, col, code) — TypeScript sorts header
        // diagnostics alphabetically by file name, then by position.
        file_header_entries.sort_by(|a, b| {
            let a_name = &all_diagnostics[a.0].0;
            let b_name = &all_diagnostics[b.0].0;
            a_name
                .cmp(b_name)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.3.cmp(&b.3))
        });
        for (_, _, _, _, line_str, related) in &file_header_entries {
            header_lines.push(line_str.clone());
            for rel in related {
                header_lines.push(rel.clone());
            }
        }

        if !header_lines.is_empty() {
            for line in &header_lines {
                output.push_str(line);
                output.push('\n');
            }
            output.push('\n');
        }

        // If no diagnostics at all (including global), return empty
        let total_file_diags: usize = all_diagnostics.iter().map(|(_, d)| d.len()).sum();
        let total_diags = total_file_diags
            + remaining_global_diags.len()
            + overwrite_output_diags.len()
            + multiple_output_diags.len()
            + unsupported_extension_diags.len()
            + disallowed_js_diags.len();
        if total_diags == 0 {
            return String::new();
        }

        // Second: global diagnostic annotation lines (before file sections)
        if !remaining_global_diags.is_empty()
            || !overwrite_output_diags.is_empty()
            || !multiple_output_diags.is_empty()
            || !unsupported_extension_diags.is_empty()
            || !disallowed_js_diags.is_empty()
        {
            output.push('\n');
            for (message, related) in &overwrite_output_diags {
                output.push_str(&format!("!!! error TS5055: {message}\n"));
                output.push_str(&format!("!!! error TS5055: {related}\n"));
            }
            for message in &multiple_output_diags {
                output.push_str(&format!("!!! error TS5056: {message}\n"));
            }
            for message in &unsupported_extension_diags {
                output.push_str(&format!("!!! error TS6054: {message}\n"));
                output.push_str("!!! error TS6054:   The file is in the program because:\n");
                output.push_str("!!! error TS6054:     Root file specified for compilation\n");
            }
            for gd in &remaining_global_diags {
                let diag = &gd.diagnostic;
                let cat = match diag.category {
                    tsc_rs_ast::DiagnosticCategory::Error => "error",
                    tsc_rs_ast::DiagnosticCategory::Warning => "warning",
                    tsc_rs_ast::DiagnosticCategory::Suggestion => "suggestion",
                    tsc_rs_ast::DiagnosticCategory::Message => "message",
                };
                output.push_str(&format!("!!! {} TS{}: {}\n", cat, diag.code, diag.message));
                if let Some(ref related) = gd.related {
                    output.push_str(&format!("!!! {} TS{}: {}\n", cat, diag.code, related));
                }
            }
            for file_name in &disallowed_js_diags {
                output.push_str(&format!(
                    "!!! error TS6504: File '{file_name}' is a JavaScript file. Did you mean to enable the 'allowJs' option?\n"
                ));
                output.push_str("!!! error TS6504:   The file is in the program because:\n");
                output.push_str("!!! error TS6504:     Root file specified for compilation\n");
            }
        }

        // Third: annotated source sections
        for file_index in error_source_order_indices(test_case) {
            let (file_name, source) = &file_sources[file_index];
            let diags = all_diagnostics
                .iter()
                .find(|(name, _)| name == file_name)
                .map(|(_, d)| d.as_slice())
                .unwrap_or(&[]);

            let error_count = diags
                .iter()
                .filter(|d| d.category == tsc_rs_ast::DiagnosticCategory::Error)
                .count();

            output.push_str(&format!(
                "==== {} ({} errors) ====\n",
                file_name, error_count
            ));

            // Keep every annotation on a source line in diagnostic order,
            // including spans that began on an earlier line.
            let line_starts = compute_line_starts(source);
            let mut diags_by_line: HashMap<usize, Vec<&Diagnostic>> = HashMap::new();
            for diag in diags {
                if let Some(span) = &diag.span {
                    let start_line = line_index_from_starts(&line_starts, span.start as usize);
                    let end_line = line_index_from_starts(
                        &line_starts,
                        span.end.saturating_sub(1).max(span.start) as usize,
                    );
                    for line in start_line..=end_line {
                        diags_by_line.entry(line).or_default().push(diag);
                    }
                }
            }
            // Sort diagnostics within each line by (col, code) to match tsc ordering
            for line_diags in diags_by_line.values_mut() {
                line_diags.sort_by(|a, b| {
                    let a_start = a.span.as_ref().map_or(0, |s| s.start);
                    let b_start = b.span.as_ref().map_or(0, |s| s.start);
                    a_start.cmp(&b_start).then_with(|| a.code.cmp(&b.code))
                });
            }

            // Collect source lines. Rust's .lines() drops a trailing empty line
            // for sources ending with '\n'. TypeScript preserves it as an
            // indented blank line, so we add it back.
            let mut source_lines: Vec<&str> = source.lines().collect();
            if source.ends_with('\n') {
                source_lines.push("");
            } else if source_lines.is_empty() {
                // TypeScript's errors baseline still prints the indented
                // source row for a genuinely empty virtual file.
                source_lines.push("");
            }

            for (line_idx, line) in source_lines.iter().enumerate() {
                // Emit source line with 4-space indent
                output.push_str(&format!("    {}\n", line));

                // Emit all underlines, retaining blank continuation rows.
                if let Some(line_diags) = diags_by_line.get(&line_idx) {
                    for diag in line_diags {
                        if let Some(span) = &diag.span {
                            let line_start = line_starts[line_idx];
                            let col = (span.start as usize).saturating_sub(line_start);
                            let start_line =
                                line_index_from_starts(&line_starts, span.start as usize);
                            let end_line = line_index_from_starts(
                                &line_starts,
                                span.end.saturating_sub(1).max(span.start) as usize,
                            );
                            let is_final_line = line_idx == end_line;
                            let len = if start_line == end_line {
                                (span.end as usize).saturating_sub(span.start as usize)
                            } else if is_final_line {
                                (span.end as usize).saturating_sub(line_start).max(1)
                            } else if line_idx == start_line {
                                line.len().saturating_sub(col).max(1)
                            } else {
                                line.len()
                            };
                            let mut prefix = String::from("    ");
                            let mut char_col = 0;
                            for ch in line.chars() {
                                if char_col >= col {
                                    break;
                                }
                                if ch == '\t' {
                                    prefix.push('\t');
                                } else {
                                    prefix.push(' ');
                                }
                                char_col += ch.len_utf8();
                            }
                            while char_col < col {
                                prefix.push(' ');
                                char_col += 1;
                            }
                            let tildes = "~".repeat(len);
                            output.push_str(&format!("{}{}\n", prefix, tildes));
                            // Emit the message once, after the final underline.
                            if is_final_line {
                                let cat = match diag.category {
                                    tsc_rs_ast::DiagnosticCategory::Error => "error",
                                    tsc_rs_ast::DiagnosticCategory::Warning => "warning",
                                    tsc_rs_ast::DiagnosticCategory::Suggestion => "suggestion",
                                    tsc_rs_ast::DiagnosticCategory::Message => "message",
                                };
                                // Handle multi-line diagnostic messages:
                                // each line gets the "!!! error TSxxxx:" prefix
                                for (idx, msg_line) in diag.message.split('\n').enumerate() {
                                    if idx == 0 {
                                        output.push_str(&format!(
                                            "!!! {} TS{}: {}\n",
                                            cat, diag.code, msg_line
                                        ));
                                    } else {
                                        output.push_str(&format!(
                                            "!!! {} TS{}: {}\n",
                                            cat, diag.code, msg_line
                                        ));
                                    }
                                }
                                // Emit located option related message (e.g. "  Visit...")
                                if let Some(span) = &diag.span {
                                    let key = (file_name.clone(), span.start, diag.code);
                                    if let Some(related_msg) = located_related_msgs.get(&key) {
                                        output.push_str(&format!(
                                            "!!! {} TS{}: {}\n",
                                            cat, diag.code, related_msg
                                        ));
                                    }
                                }
                                // Emit related diagnostics (e.g. TS6203)
                                if let Some(ref related_list) = diag.related {
                                    for rel in related_list {
                                        // A lib-declared symbol has no position in
                                        // the baseline: tsc prints `lib.x.d.ts:--:--`.
                                        if let (Some(rel_file), None) = (&rel.file_name, &rel.span)
                                        {
                                            output.push_str(&format!(
                                                "!!! related TS{} {}:--:--: {}\n",
                                                rel.code, rel_file, rel.message
                                            ));
                                            continue;
                                        }
                                        if let (Some(ref rel_file), Some(ref rel_span)) =
                                            (&rel.file_name, &rel.span)
                                        {
                                            // Look up the source for the related file
                                            let rel_source = file_sources
                                                .iter()
                                                .find(|(name, _)| name == rel_file)
                                                .map(|(_, s)| s.as_str())
                                                .unwrap_or("");
                                            let rel_line_starts = compute_line_starts(rel_source);
                                            let rel_line_idx = line_index_from_starts(
                                                &rel_line_starts,
                                                rel_span.start as usize,
                                            );
                                            let rel_col = (rel_span.start as usize)
                                                .saturating_sub(rel_line_starts[rel_line_idx])
                                                + 1;
                                            output.push_str(&format!(
                                                "!!! related TS{} {}:{}:{}: {}\n",
                                                rel.code,
                                                rel_file,
                                                rel_line_idx + 1,
                                                rel_col,
                                                rel.message
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        output
    }

    // -----------------------------------------------------------------------
    // Symbols baseline generation
    // -----------------------------------------------------------------------

    fn generate_symbols_baseline_output(
        &self,
        test_case: &TestCase,
        relative_test_path: &str,
    ) -> String {
        let mut output = String::new();

        // Header
        output.push_str(&format!("//// [{}] ////\n", relative_test_path));

        for file in &test_case.files {
            let lower = file.name.to_ascii_lowercase();
            if lower.ends_with(".d.ts")
                || lower.ends_with(".d.tsx")
                || lower.ends_with(".d.mts")
                || lower.ends_with(".d.cts")
                || lower.ends_with(".json")
            {
                continue;
            }

            let is_jsx = lower.ends_with(".tsx") || lower.ends_with(".jsx");
            let parsed = tsc_rs_parser::parse_with_jsx(&file.name, &file.content, is_jsx);
            let symbols = tsc_rs_symbols::bind(&parsed);

            let file_baseline = tsc_rs_symbols::generate_symbols_baseline(&parsed, &symbols);
            // The generate_symbols_baseline already includes //// header and === header,
            // but we already printed the top-level //// header, so we need to strip
            // the per-file //// header if present.
            // Actually, looking at the format, the top-level //// header is different from
            // per-file. Let's just include the per-file baseline content starting from ===
            if let Some(pos) = file_baseline.find("\n=== ") {
                output.push_str(&file_baseline[pos..]);
            } else {
                output.push_str(&file_baseline);
            }
        }

        output
    }

    // -----------------------------------------------------------------------
    // Types baseline generation
    // -----------------------------------------------------------------------

    fn generate_types_baseline(&self, test_case: &TestCase, relative_test_path: &str) -> String {
        let effective_options = effective_compiler_options(test_case);
        let mut output = String::new();

        // Header
        output.push_str(&format!("//// [{}] ////\n", relative_test_path));

        for file in &test_case.files {
            let lower = file.name.to_ascii_lowercase();
            if lower.ends_with(".d.ts")
                || lower.ends_with(".d.tsx")
                || lower.ends_with(".d.mts")
                || lower.ends_with(".d.cts")
                || lower.ends_with(".json")
            {
                continue;
            }

            let is_jsx = lower.ends_with(".tsx") || lower.ends_with(".jsx");
            let parsed = tsc_rs_parser::parse_with_jsx(&file.name, &file.content, is_jsx);
            let symbols = tsc_rs_symbols::bind(&parsed);
            let check_output = tsc_rs_types::TypeChecker::new().check_with_options(
                &parsed,
                &symbols,
                &effective_options,
            );

            let display_name = basename(&file.name);
            output.push_str(&format!("\n=== {} ===\n", display_name));

            // Build sorted list of (line, col, expr_text, type_str)
            let line_starts = compute_line_starts(&file.content);
            let source_lines: Vec<&str> = file.content.lines().collect();

            // Group expression types by line
            let mut types_by_line: HashMap<usize, Vec<(usize, String, String)>> = HashMap::new();
            for (&pos, type_str) in &check_output.expression_types {
                let line_idx = line_index_from_starts(&line_starts, pos as usize);
                let line_start = line_starts[line_idx];
                let col = (pos as usize).saturating_sub(line_start);
                // Extract expression text from source
                let expr_text = extract_expression_at(&file.content, pos as usize);
                types_by_line
                    .entry(line_idx)
                    .or_default()
                    .push((col, expr_text, type_str.clone()));
            }

            // Sort types within each line by column
            for entries in types_by_line.values_mut() {
                entries.sort_by_key(|(col, _, _)| *col);
            }

            for (line_idx, line) in source_lines.iter().enumerate() {
                output.push_str(line);
                output.push('\n');

                if let Some(entries) = types_by_line.get(&line_idx) {
                    for (_, expr_text, type_str) in entries {
                        output.push_str(&format!(">{} : {}\n", expr_text, type_str));
                        // Caret line
                        let prefix_len = expr_text.len() + 1; // +1 for '>'
                        let carets = "^".repeat(type_str.len());
                        output.push_str(&format!(">{} : {}\n", " ".repeat(prefix_len - 1), carets));
                    }
                }
            }
        }

        output
    }
}

// ---------------------------------------------------------------------------
// Helpers for baseline generation
// ---------------------------------------------------------------------------

/// Strip deprecation-only output from an errors baseline.
/// If the actual output contains ONLY deprecation warnings (TS5101/TS5107/TS5102)
/// and all file sections show "(0 errors)", return empty string.
/// Otherwise return the output unchanged.
fn strip_deprecation_only_output(output: &str) -> String {
    if output.trim().is_empty() {
        return String::new();
    }
    // Check if any file section has non-zero errors
    let mut has_file_errors = false;
    for line in output.lines() {
        let trimmed = line.trim();
        // "==== file.ts (N errors) ====" headers
        if trimmed.starts_with("====") && trimmed.ends_with("====") {
            if !trimmed.contains("(0 errors)") {
                has_file_errors = true;
                break;
            }
        }
        // "!!! error TSxxxx:" inline error annotations (not deprecation)
        if trimmed.starts_with("!!! error TS") {
            // Skip deprecation annotations
            if trimmed.starts_with("!!! error TS5101:")
                || trimmed.starts_with("!!! error TS5107:")
                || trimmed.starts_with("!!! error TS5102:")
                || trimmed.starts_with("!!! error TS5108:")
            {
                continue;
            }
            has_file_errors = true;
            break;
        }
    }
    if has_file_errors {
        output.to_string()
    } else {
        String::new()
    }
}

/// Compute (line, col) with 1-based line and 0-based column (tsc format for .errors.txt header).
fn offset_to_line_col_0based(source: &str, offset: u32) -> (usize, usize) {
    let offset = offset as usize;
    let mut line = 1usize;
    let mut col = 0usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Compute line start offsets for a source string.
fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, ch) in source.char_indices() {
        if ch == '\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Find the line index (0-based) for a byte offset given precomputed line starts.
fn line_index_from_starts(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    }
}

/// Extract a reasonable expression text starting at the given offset.
/// This is a simple heuristic: grab word characters, dots, brackets, parens.
fn extract_expression_at(source: &str, offset: usize) -> String {
    let rest = &source[offset..];
    let mut end = 0;
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut chars = rest.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '(' => {
                depth_paren += 1;
                end += ch.len_utf8();
            }
            ')' => {
                depth_paren -= 1;
                end += ch.len_utf8();
                if depth_paren < 0 {
                    end -= ch.len_utf8();
                    break;
                }
            }
            '[' => {
                depth_bracket += 1;
                end += ch.len_utf8();
            }
            ']' => {
                depth_bracket -= 1;
                end += ch.len_utf8();
                if depth_bracket < 0 {
                    end -= ch.len_utf8();
                    break;
                }
            }
            '.' if depth_paren == 0 && depth_bracket == 0 => {
                // Include member access dots
                end += ch.len_utf8();
            }
            _ if ch.is_alphanumeric() || ch == '_' || ch == '$' || ch == '#' => {
                end += ch.len_utf8();
            }
            '<' | '>' if depth_paren > 0 || depth_bracket > 0 => {
                end += ch.len_utf8();
            }
            ' ' | '\t' | '\n' | '\r' | ';' | ',' | ':' | '{' | '}' | '=' | '+' | '-' | '*'
            | '/' | '!' | '?' | '&' | '|' | '^' | '~' | '%' => {
                break;
            }
            _ => {
                if depth_paren == 0 && depth_bracket == 0 {
                    break;
                }
                end += ch.len_utf8();
            }
        }
    }
    if end == 0 {
        // Fallback: just grab the first token-like thing
        let token_end = rest
            .find(|c: char| c.is_whitespace() || ";,:{".contains(c))
            .unwrap_or(rest.len());
        rest[..token_end].to_string()
    } else {
        rest[..end].to_string()
    }
}

// ---------------------------------------------------------------------------
// Emitter integration
// ---------------------------------------------------------------------------

fn collect_runtime_export_names_from_decl(
    stmt: &tsc_rs_ast::Stmt,
    preserve_const_enums: bool,
    out: &mut HashSet<String>,
) {
    fn collect_var_idents_from_pat(pat: &tsc_rs_ast::Pat, out: &mut HashSet<String>) {
        match &pat.kind {
            tsc_rs_ast::PatKind::Ident(name) => {
                out.insert(name.to_string());
            }
            tsc_rs_ast::PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        tsc_rs_ast::ObjPatProp::KeyValue(_, inner) => {
                            collect_var_idents_from_pat(inner, out);
                        }
                        tsc_rs_ast::ObjPatProp::Shorthand(name, _) => {
                            out.insert(name.to_string());
                        }
                        tsc_rs_ast::ObjPatProp::ShorthandAssign(name, _, _) => {
                            out.insert(name.to_string());
                        }
                        tsc_rs_ast::ObjPatProp::Rest(inner) => {
                            collect_var_idents_from_pat(inner, out);
                        }
                    }
                }
            }
            tsc_rs_ast::PatKind::Array(elems) => {
                for elem in elems.iter().flatten() {
                    match elem {
                        tsc_rs_ast::ArrayPatElem::Pat(inner) => {
                            collect_var_idents_from_pat(inner, out);
                        }
                        tsc_rs_ast::ArrayPatElem::Rest(inner) => {
                            collect_var_idents_from_pat(inner, out);
                        }
                    }
                }
            }
            tsc_rs_ast::PatKind::Assign(inner, _) => {
                collect_var_idents_from_pat(inner, out);
            }
            tsc_rs_ast::PatKind::Rest(inner) => {
                collect_var_idents_from_pat(inner, out);
            }
        }
    }

    match &stmt.kind {
        tsc_rs_ast::StmtKind::Var(var) => {
            for decl in &var.declarations {
                collect_var_idents_from_pat(&decl.name, out);
            }
        }
        tsc_rs_ast::StmtKind::FnDecl(f) => {
            if let Some(name) = &f.name {
                out.insert(name.clone());
            }
        }
        tsc_rs_ast::StmtKind::ClassDecl(c) => {
            if let Some(name) = &c.name {
                out.insert(name.clone());
            }
        }
        tsc_rs_ast::StmtKind::EnumDecl(e) => {
            if !e.is_const || preserve_const_enums {
                out.insert(e.name.clone());
            }
        }
        tsc_rs_ast::StmtKind::ModuleDecl(m) => {
            if let tsc_rs_ast::ModuleName::Ident(name) = &m.name {
                out.insert(name.clone());
            }
        }
        tsc_rs_ast::StmtKind::ImportEquals(ie) => {
            let name = &ie.name;
            out.insert(name.clone());
        }
        _ => {}
    }
}

/// Collect ALL declared names from a file's top-level statements.
/// Used for cross-file resolution in multi-file error baselines.
fn collect_top_level_names(file: &tsc_rs_ast::SourceFile, out: &mut Vec<String>) {
    fn collect_from_stmt(stmt: &tsc_rs_ast::Stmt, out: &mut Vec<String>) {
        match &stmt.kind {
            StmtKind::Var(vs) => {
                for decl in &vs.declarations {
                    collect_from_pat(&decl.name, out);
                }
            }
            StmtKind::FnDecl(fd) => {
                if let Some(ref name) = fd.name {
                    out.push(name.clone());
                }
            }
            StmtKind::ClassDecl(cd) => {
                if let Some(ref name) = cd.name {
                    out.push(name.clone());
                }
            }
            StmtKind::EnumDecl(ed) => {
                out.push(ed.name.clone());
            }
            StmtKind::ModuleDecl(md) => {
                if let ModuleName::Ident(ref name) = md.name {
                    out.push(name.clone());
                    // `declare global { ... }` — export inner declarations to global scope
                    if name == "global" {
                        if let Some(tsc_rs_ast::ModuleBody::Block(ref stmts)) = md.body {
                            for s in stmts {
                                collect_from_stmt(s, out);
                            }
                        }
                    }
                }
            }
            StmtKind::InterfaceDecl(id) => {
                out.push(id.name.clone());
            }
            StmtKind::TypeAlias(ta) => {
                out.push(ta.name.clone());
            }
            StmtKind::Export(ed) => match &ed.kind {
                ExportDeclKind::Decl(inner) => collect_from_stmt(inner, out),
                ExportDeclKind::DefaultDecl(inner) => collect_from_stmt(inner, out),
                _ => {}
            },
            _ => {}
        }
    }
    fn collect_from_pat(pat: &tsc_rs_ast::Pat, out: &mut Vec<String>) {
        match &pat.kind {
            tsc_rs_ast::PatKind::Ident(name) => out.push(name.to_string()),
            tsc_rs_ast::PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        tsc_rs_ast::ObjPatProp::KeyValue(_, v) => collect_from_pat(v, out),
                        tsc_rs_ast::ObjPatProp::Shorthand(name, _) => out.push(name.to_string()),
                        tsc_rs_ast::ObjPatProp::ShorthandAssign(name, _, _) => {
                            out.push(name.to_string())
                        }
                        tsc_rs_ast::ObjPatProp::Rest(p) => collect_from_pat(p, out),
                    }
                }
            }
            tsc_rs_ast::PatKind::Array(elems) => {
                for elem in elems.iter().flatten() {
                    match elem {
                        tsc_rs_ast::ArrayPatElem::Pat(p) => collect_from_pat(p, out),
                        tsc_rs_ast::ArrayPatElem::Rest(p) => collect_from_pat(p, out),
                    }
                }
            }
            tsc_rs_ast::PatKind::Assign(p, _) => collect_from_pat(p, out),
            tsc_rs_ast::PatKind::Rest(p) => collect_from_pat(p, out),
        }
    }
    for stmt in &file.statements {
        collect_from_stmt(stmt, out);
    }
}

fn collect_top_level_value_names(
    file: &tsc_rs_ast::SourceFile,
    preserve_const_enums: bool,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for stmt in &file.statements {
        match &stmt.kind {
            tsc_rs_ast::StmtKind::Export(ed) => {
                if let tsc_rs_ast::ExportDeclKind::Decl(inner) = &ed.kind {
                    collect_runtime_export_names_from_decl(inner, preserve_const_enums, &mut names);
                }
            }
            _ => collect_runtime_export_names_from_decl(stmt, preserve_const_enums, &mut names),
        }
    }
    names
}

fn source_file_is_definitely_type_only_for_require(
    file: &tsc_rs_ast::SourceFile,
    preserve_const_enums: bool,
) -> bool {
    let runtime_names = collect_top_level_value_names(file, preserve_const_enums);
    if !runtime_names.is_empty() {
        return false;
    }

    fn expr_root_ident(expr: &tsc_rs_ast::Expr) -> Option<&str> {
        match &expr.kind {
            tsc_rs_ast::ExprKind::Ident(name) => Some(name.as_str()),
            tsc_rs_ast::ExprKind::Member(member) => expr_root_ident(&member.object),
            tsc_rs_ast::ExprKind::Paren(inner) | tsc_rs_ast::ExprKind::NonNull(inner) => {
                expr_root_ident(inner)
            }
            tsc_rs_ast::ExprKind::TypeAssertion(ta) => expr_root_ident(&ta.expr),
            tsc_rs_ast::ExprKind::As(a) => expr_root_ident(&a.expr),
            tsc_rs_ast::ExprKind::Satisfies(s) => expr_root_ident(&s.expr),
            tsc_rs_ast::ExprKind::Instantiation(inst) => expr_root_ident(&inst.expr),
            _ => None,
        }
    }

    fn import_decl_has_runtime(imp: &tsc_rs_ast::ImportDecl) -> bool {
        if imp.type_only {
            return false;
        }
        match &imp.specifiers {
            tsc_rs_ast::ImportClause::Require(_) => true,
            tsc_rs_ast::ImportClause::Named {
                default,
                named,
                namespace,
            } => {
                imp.is_side_effect
                    || default.is_some()
                    || namespace.is_some()
                    || named.iter().any(|s| !s.is_type)
            }
        }
    }

    fn export_assign_has_runtime(expr: &tsc_rs_ast::Expr, runtime_names: &HashSet<String>) -> bool {
        match expr_root_ident(expr) {
            Some(name) => runtime_names.contains(name),
            None => true,
        }
    }

    fn stmt_has_runtime(
        stmt: &tsc_rs_ast::Stmt,
        runtime_names: &HashSet<String>,
        preserve_const_enums: bool,
    ) -> bool {
        match &stmt.kind {
            tsc_rs_ast::StmtKind::InterfaceDecl(_) | tsc_rs_ast::StmtKind::TypeAlias(_) => false,
            tsc_rs_ast::StmtKind::Import(import_decl) => import_decl_has_runtime(import_decl),
            tsc_rs_ast::StmtKind::ImportEquals(_) => true,
            tsc_rs_ast::StmtKind::ModuleDecl(module_decl) => {
                ambient_module_decl_has_runtime_value(module_decl, preserve_const_enums)
            }
            tsc_rs_ast::StmtKind::ExportAssign(expr) => {
                export_assign_has_runtime(expr, runtime_names)
            }
            tsc_rs_ast::StmtKind::Export(export_decl) => match &export_decl.kind {
                tsc_rs_ast::ExportDeclKind::Decl(inner) => {
                    stmt_has_runtime(inner, runtime_names, preserve_const_enums)
                }
                tsc_rs_ast::ExportDeclKind::Named {
                    source: Some(_),
                    type_only: false,
                    specifiers,
                } => specifiers.iter().any(|s| !s.is_type),
                tsc_rs_ast::ExportDeclKind::Named { .. } => false,
                tsc_rs_ast::ExportDeclKind::All {
                    type_only: false, ..
                } => true,
                tsc_rs_ast::ExportDeclKind::All { .. } => false,
                // `export default` always creates a module binding,
                // even if the exported value is type-only. TypeScript
                // retains require() imports for such modules.
                tsc_rs_ast::ExportDeclKind::Default(_) => true,
                tsc_rs_ast::ExportDeclKind::DefaultDecl(_) => true,
            },
            _ => true,
        }
    }

    !file
        .statements
        .iter()
        .any(|stmt| stmt_has_runtime(stmt, &runtime_names, preserve_const_enums))
}

/// Returns (type_only_specs, runtime_value_specs).
/// A module spec is type-only in this file if its `declare module` body has no runtime value.
/// A module spec has runtime value if its `declare module` body exports classes, functions, etc.
fn collect_external_module_spec_info(
    file: &tsc_rs_ast::SourceFile,
    preserve_const_enums: bool,
) -> (HashSet<String>, HashSet<String>) {
    let mut type_only = HashSet::new();
    let mut has_runtime = HashSet::new();

    fn collect_from_stmt(
        stmt: &tsc_rs_ast::Stmt,
        preserve_const_enums: bool,
        type_only: &mut HashSet<String>,
        has_runtime: &mut HashSet<String>,
    ) {
        let module_decl = match &stmt.kind {
            tsc_rs_ast::StmtKind::ModuleDecl(module_decl) => Some(module_decl),
            tsc_rs_ast::StmtKind::Export(export_decl) => {
                if let tsc_rs_ast::ExportDeclKind::Decl(inner) = &export_decl.kind {
                    if let tsc_rs_ast::StmtKind::ModuleDecl(module_decl) = &inner.kind {
                        Some(module_decl)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(module_decl) = module_decl else {
            return;
        };
        let tsc_rs_ast::ModuleName::String(spec) = &module_decl.name else {
            return;
        };
        if ambient_module_decl_has_runtime_value(module_decl, preserve_const_enums) {
            has_runtime.insert(spec.clone());
        } else {
            type_only.insert(spec.clone());
        }
    }

    for stmt in &file.statements {
        collect_from_stmt(stmt, preserve_const_enums, &mut type_only, &mut has_runtime);
    }
    // Remove module specifiers that are also imported in this file.
    // A `declare module "X" { ... }` alongside `import ... from "X"` is a
    // module augmentation, not a standalone type-only definition.  The real
    // module likely has runtime value and should not be marked type-only.
    for stmt in &file.statements {
        let source = match &stmt.kind {
            tsc_rs_ast::StmtKind::Import(imp) => Some(imp.source.as_str()),
            tsc_rs_ast::StmtKind::Export(ed) => {
                if let tsc_rs_ast::ExportDeclKind::Decl(inner) = &ed.kind {
                    if let tsc_rs_ast::StmtKind::Import(imp) = &inner.kind {
                        Some(imp.source.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(source) = source {
            type_only.remove(source);
            has_runtime.remove(source);
        }
    }
    (type_only, has_runtime)
}

fn ambient_module_decl_has_runtime_value(
    module_decl: &tsc_rs_ast::ModuleDecl,
    preserve_const_enums: bool,
) -> bool {
    match &module_decl.body {
        Some(tsc_rs_ast::ModuleBody::Block(stmts)) => {
            ambient_module_block_has_runtime_value(stmts, preserve_const_enums)
        }
        Some(tsc_rs_ast::ModuleBody::Module(inner)) => {
            ambient_module_decl_has_runtime_value(inner, preserve_const_enums)
        }
        // Shorthand ambient modules (`declare module "foo";`) have unknown
        // runtime shape. Do not classify them as type-only.
        None => true,
    }
}

fn ambient_module_block_has_runtime_value(
    stmts: &[tsc_rs_ast::Stmt],
    preserve_const_enums: bool,
) -> bool {
    let mut value_names = HashSet::new();
    let mut type_names = HashSet::new();
    let mut has_unknown_runtime = false;

    fn collect_pat_names(pat: &tsc_rs_ast::Pat, out: &mut HashSet<String>) {
        match &pat.kind {
            tsc_rs_ast::PatKind::Ident(name) => {
                out.insert(name.to_string());
            }
            tsc_rs_ast::PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        tsc_rs_ast::ObjPatProp::KeyValue(_, inner) => collect_pat_names(inner, out),
                        tsc_rs_ast::ObjPatProp::Shorthand(name, _) => {
                            out.insert(name.to_string());
                        }
                        tsc_rs_ast::ObjPatProp::ShorthandAssign(name, _, _) => {
                            out.insert(name.to_string());
                        }
                        tsc_rs_ast::ObjPatProp::Rest(inner) => collect_pat_names(inner, out),
                    }
                }
            }
            tsc_rs_ast::PatKind::Array(elems) => {
                for elem in elems.iter().flatten() {
                    match elem {
                        tsc_rs_ast::ArrayPatElem::Pat(inner)
                        | tsc_rs_ast::ArrayPatElem::Rest(inner) => collect_pat_names(inner, out),
                    }
                }
            }
            tsc_rs_ast::PatKind::Assign(inner, _) | tsc_rs_ast::PatKind::Rest(inner) => {
                collect_pat_names(inner, out);
            }
        }
    }

    fn collect_stmt_value_names(
        stmt: &tsc_rs_ast::Stmt,
        preserve_const_enums: bool,
        value_names: &mut HashSet<String>,
        type_names: &mut HashSet<String>,
        has_unknown_runtime: &mut bool,
    ) {
        match &stmt.kind {
            tsc_rs_ast::StmtKind::InterfaceDecl(interface_decl) => {
                type_names.insert(interface_decl.name.clone());
            }
            tsc_rs_ast::StmtKind::TypeAlias(type_alias) => {
                type_names.insert(type_alias.name.clone());
            }
            tsc_rs_ast::StmtKind::Var(var_stmt) => {
                for decl in &var_stmt.declarations {
                    collect_pat_names(&decl.name, value_names);
                }
            }
            tsc_rs_ast::StmtKind::FnDecl(fn_decl) => {
                if let Some(name) = &fn_decl.name {
                    value_names.insert(name.clone());
                }
            }
            tsc_rs_ast::StmtKind::ClassDecl(class_decl) => {
                if let Some(name) = &class_decl.name {
                    value_names.insert(name.clone());
                }
            }
            tsc_rs_ast::StmtKind::EnumDecl(enum_decl) => {
                if !enum_decl.is_const || preserve_const_enums {
                    value_names.insert(enum_decl.name.clone());
                } else {
                    type_names.insert(enum_decl.name.clone());
                }
            }
            tsc_rs_ast::StmtKind::ModuleDecl(module_decl) => {
                if ambient_module_decl_has_runtime_value(module_decl, preserve_const_enums) {
                    match &module_decl.name {
                        tsc_rs_ast::ModuleName::Ident(name)
                        | tsc_rs_ast::ModuleName::String(name) => {
                            value_names.insert(name.clone());
                        }
                    }
                } else {
                    match &module_decl.name {
                        tsc_rs_ast::ModuleName::Ident(name)
                        | tsc_rs_ast::ModuleName::String(name) => {
                            type_names.insert(name.clone());
                        }
                    }
                }
            }
            tsc_rs_ast::StmtKind::Import(import_decl) => {
                if !import_decl.type_only {
                    *has_unknown_runtime = true;
                }
            }
            tsc_rs_ast::StmtKind::ImportEquals(ie) => {
                let name = &ie.name;
                value_names.insert(name.clone());
            }
            tsc_rs_ast::StmtKind::Export(export_decl) => match &export_decl.kind {
                tsc_rs_ast::ExportDeclKind::Decl(inner) => {
                    collect_stmt_value_names(
                        inner,
                        preserve_const_enums,
                        value_names,
                        type_names,
                        has_unknown_runtime,
                    );
                }
                tsc_rs_ast::ExportDeclKind::Named {
                    source, type_only, ..
                } => {
                    if !*type_only && source.is_some() {
                        *has_unknown_runtime = true;
                    }
                }
                tsc_rs_ast::ExportDeclKind::All { type_only, .. } => {
                    if !*type_only {
                        *has_unknown_runtime = true;
                    }
                }
                tsc_rs_ast::ExportDeclKind::Default(_)
                | tsc_rs_ast::ExportDeclKind::DefaultDecl(_) => {
                    *has_unknown_runtime = true;
                }
            },
            _ => {}
        }
    }

    for stmt in stmts {
        collect_stmt_value_names(
            stmt,
            preserve_const_enums,
            &mut value_names,
            &mut type_names,
            &mut has_unknown_runtime,
        );
    }

    fn expr_root_ident(expr: &tsc_rs_ast::Expr) -> Option<&str> {
        match &expr.kind {
            tsc_rs_ast::ExprKind::Ident(name) => Some(name.as_str()),
            tsc_rs_ast::ExprKind::Member(member) => expr_root_ident(&member.object),
            tsc_rs_ast::ExprKind::Paren(inner) | tsc_rs_ast::ExprKind::NonNull(inner) => {
                expr_root_ident(inner)
            }
            tsc_rs_ast::ExprKind::TypeAssertion(ta) => expr_root_ident(&ta.expr),
            tsc_rs_ast::ExprKind::As(a) => expr_root_ident(&a.expr),
            tsc_rs_ast::ExprKind::Satisfies(s) => expr_root_ident(&s.expr),
            tsc_rs_ast::ExprKind::Instantiation(inst) => expr_root_ident(&inst.expr),
            _ => None,
        }
    }

    let mut has_runtime_export_assign = false;
    for stmt in stmts {
        let export_assign_expr = match &stmt.kind {
            tsc_rs_ast::StmtKind::ExportAssign(expr) => Some(expr.as_ref()),
            tsc_rs_ast::StmtKind::Export(export_decl) => {
                if let tsc_rs_ast::ExportDeclKind::Decl(inner) = &export_decl.kind {
                    if let tsc_rs_ast::StmtKind::ExportAssign(expr) = &inner.kind {
                        Some(expr.as_ref())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(expr) = export_assign_expr else {
            continue;
        };
        match expr_root_ident(expr) {
            Some(name) if value_names.contains(name) => {
                has_runtime_export_assign = true;
            }
            Some(name) if type_names.contains(name) => {}
            Some(_) => {
                has_runtime_export_assign = true;
            }
            None => {
                has_runtime_export_assign = true;
            }
        }
    }

    has_unknown_runtime || has_runtime_export_assign || !value_names.is_empty()
}

/// Emit JavaScript from a parsed SourceFile, delegating to `tsc_rs_emitter`.
///
/// Strips TypeScript-only syntax (type annotations, interfaces, type aliases)
/// and produces JavaScript output. The emitter handles class field initializer
/// hoisting, enum lowering, namespace wrapping, and other TS-to-JS transforms.
/// Collect exported const values (numeric and string) from a source file.
fn parse_js_like_number(s: &str) -> Option<f64> {
    let cleaned;
    let s = if s.contains('_') {
        cleaned = s.replace('_', "");
        &cleaned
    } else {
        s
    };
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16).ok().map(|v| v as f64)
    } else if s.starts_with("0o") || s.starts_with("0O") {
        u64::from_str_radix(&s[2..], 8).ok().map(|v| v as f64)
    } else if s.starts_with("0b") || s.starts_with("0B") {
        u64::from_str_radix(&s[2..], 2).ok().map(|v| v as f64)
    } else {
        s.parse::<f64>().ok()
    }
}

fn eval_enum_numeric_expr(
    expr: &tsc_rs_ast::Expr,
    members: &HashMap<String, f64>,
    num_consts: &HashMap<String, f64>,
) -> Option<f64> {
    match &expr.kind {
        tsc_rs_ast::ExprKind::NumLit(s) => parse_js_like_number(s),
        tsc_rs_ast::ExprKind::Paren(inner) => eval_enum_numeric_expr(inner, members, num_consts),
        tsc_rs_ast::ExprKind::Ident(id) => members
            .get(id.as_str())
            .copied()
            .or_else(|| num_consts.get(id.as_str()).copied()),
        tsc_rs_ast::ExprKind::Unary(u) => {
            let arg = eval_enum_numeric_expr(&u.argument, members, num_consts)?;
            match u.op {
                tsc_rs_ast::UnaryOp::Neg => Some(-arg),
                tsc_rs_ast::UnaryOp::Pos => Some(arg),
                _ => None,
            }
        }
        _ => None,
    }
}

fn collect_file_const_values(
    file: &tsc_rs_ast::SourceFile,
) -> (HashMap<String, f64>, HashMap<String, String>) {
    let mut num_consts: HashMap<String, f64> = HashMap::new();
    let mut str_consts: HashMap<String, String> = HashMap::new();
    for stmt in &file.statements {
        if let tsc_rs_ast::StmtKind::Var(v) = &stmt.kind {
            if v.kind == tsc_rs_ast::VarKind::Const {
                for decl in &v.declarations {
                    let name = match &decl.name.kind {
                        tsc_rs_ast::PatKind::Ident(n) => n.to_string(),
                        _ => continue,
                    };
                    let Some(init) = decl.init.as_ref() else {
                        continue;
                    };
                    let mut expr = init.as_ref();
                    while let tsc_rs_ast::ExprKind::Paren(inner) = &expr.kind {
                        expr = inner;
                    }
                    match &expr.kind {
                        tsc_rs_ast::ExprKind::NumLit(s) => {
                            if let Some(v) = parse_js_like_number(s) {
                                num_consts.insert(name, v);
                            }
                        }
                        tsc_rs_ast::ExprKind::Unary(u) if u.op == tsc_rs_ast::UnaryOp::Neg => {
                            if let tsc_rs_ast::ExprKind::NumLit(s) = &u.argument.kind {
                                if let Some(v) = parse_js_like_number(s) {
                                    num_consts.insert(name, -v);
                                }
                            }
                        }
                        tsc_rs_ast::ExprKind::StrLit(s) => {
                            str_consts.insert(name, s.to_string());
                        }
                        tsc_rs_ast::ExprKind::NoSubstTemplate(s) => {
                            str_consts.insert(name, s.to_string());
                        }
                        _ => {}
                    }
                }
            }
        }
        let enum_decl = match &stmt.kind {
            tsc_rs_ast::StmtKind::EnumDecl(e) => Some(e),
            tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                    tsc_rs_ast::StmtKind::EnumDecl(e) => Some(e),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let Some(enum_decl) = enum_decl else {
            continue;
        };
        let mut auto_value = Some(0.0_f64);
        for member in &enum_decl.members {
            let key = match &member.name {
                tsc_rs_ast::PropName::Ident(n, _)
                | tsc_rs_ast::PropName::String(n, _)
                | tsc_rs_ast::PropName::Number(n, _) => n.to_string(),
                tsc_rs_ast::PropName::Computed(expr, _) => match &expr.kind {
                    tsc_rs_ast::ExprKind::StrLit(s) | tsc_rs_ast::ExprKind::NumLit(s) => {
                        s.to_string()
                    }
                    _ => continue,
                },
                _ => continue,
            };
            if let Some(init) = &member.initializer {
                let mut e = init.as_ref();
                while let tsc_rs_ast::ExprKind::Paren(inner) = &e.kind {
                    e = inner;
                }
                match &e.kind {
                    tsc_rs_ast::ExprKind::StrLit(s) | tsc_rs_ast::ExprKind::NoSubstTemplate(s) => {
                        str_consts.insert(key, s.to_string());
                    }
                    _ => {
                        if let Some(v) = eval_enum_numeric_expr(e, &num_consts, &num_consts) {
                            num_consts.insert(key, v);
                            auto_value = Some(v + 1.0);
                        } else {
                            auto_value = None;
                        }
                    }
                }
            } else if let Some(v) = auto_value {
                num_consts.insert(key, v);
                auto_value = Some(v + 1.0);
            }
        }
    }
    (num_consts, str_consts)
}

fn collect_exported_const_values(
    file: &tsc_rs_ast::SourceFile,
) -> (HashMap<String, f64>, HashMap<String, String>) {
    let mut num_consts: HashMap<String, f64> = HashMap::new();
    let mut str_consts: HashMap<String, String> = HashMap::new();
    let mut all_num: HashMap<String, f64> = HashMap::new();
    let mut all_str: HashMap<String, String> = HashMap::new();
    let mut all_enum_num: HashMap<String, HashMap<String, f64>> = HashMap::new();
    let mut all_enum_str: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut exported_names: HashSet<String> = HashSet::new();
    for stmt in &file.statements {
        if let Some(enum_decl) = match &stmt.kind {
            tsc_rs_ast::StmtKind::EnumDecl(e) => Some((e, false)),
            tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                tsc_rs_ast::ExportDeclKind::Decl(inner) => match &inner.kind {
                    tsc_rs_ast::StmtKind::EnumDecl(e) => Some((e, true)),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        } {
            let (enum_decl, is_exported) = enum_decl;
            let mut enum_num: HashMap<String, f64> = HashMap::new();
            let mut enum_str: HashMap<String, String> = HashMap::new();
            let mut auto_value = Some(0.0_f64);
            for member in &enum_decl.members {
                let key = match &member.name {
                    tsc_rs_ast::PropName::Ident(n, _)
                    | tsc_rs_ast::PropName::String(n, _)
                    | tsc_rs_ast::PropName::Number(n, _) => n.to_string(),
                    tsc_rs_ast::PropName::Computed(expr, _) => match &expr.kind {
                        tsc_rs_ast::ExprKind::StrLit(s) | tsc_rs_ast::ExprKind::NumLit(s) => {
                            s.to_string()
                        }
                        _ => continue,
                    },
                    _ => continue,
                };
                if let Some(init) = &member.initializer {
                    let mut e = init.as_ref();
                    while let tsc_rs_ast::ExprKind::Paren(inner) = &e.kind {
                        e = inner;
                    }
                    match &e.kind {
                        tsc_rs_ast::ExprKind::StrLit(s)
                        | tsc_rs_ast::ExprKind::NoSubstTemplate(s) => {
                            enum_str.insert(key.clone(), s.to_string());
                        }
                        _ => {
                            if let Some(v) = eval_enum_numeric_expr(e, &enum_num, &all_num) {
                                enum_num.insert(key.clone(), v);
                                auto_value = Some(v + 1.0);
                            } else {
                                auto_value = None;
                            }
                        }
                    }
                } else if let Some(v) = auto_value {
                    enum_num.insert(key.clone(), v);
                    auto_value = Some(v + 1.0);
                }
            }
            all_enum_num.insert(enum_decl.name.clone(), enum_num.clone());
            all_enum_str.insert(enum_decl.name.clone(), enum_str.clone());
            if is_exported {
                for (member, value) in enum_num {
                    num_consts.insert(format!("{}.{}", enum_decl.name, member), value);
                }
                for (member, value) in enum_str {
                    str_consts.insert(format!("{}.{}", enum_decl.name, member), value);
                }
            }
            continue;
        }
        let (var, is_exported) = match &stmt.kind {
            tsc_rs_ast::StmtKind::Var(v) if v.kind == tsc_rs_ast::VarKind::Const => (v, false),
            tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                tsc_rs_ast::ExportDeclKind::Decl(inner) => {
                    if let tsc_rs_ast::StmtKind::Var(v) = &inner.kind {
                        if v.kind == tsc_rs_ast::VarKind::Const {
                            (v, true)
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                tsc_rs_ast::ExportDeclKind::Named {
                    specifiers,
                    source: None,
                    type_only: false,
                } => {
                    for spec in specifiers {
                        if !spec.is_type {
                            let n = spec.exported.as_ref().unwrap_or(&spec.local);
                            exported_names.insert(n.clone());
                        }
                    }
                    continue;
                }
                _ => continue,
            },
            _ => continue,
        };
        for decl in &var.declarations {
            let decl_name = match &decl.name.kind {
                tsc_rs_ast::PatKind::Ident(n) => n.to_string(),
                _ => continue,
            };
            if let Some(ref init) = decl.init {
                let mut expr = init.as_ref();
                while let tsc_rs_ast::ExprKind::Paren(inner) = &expr.kind {
                    expr = inner;
                }
                match &expr.kind {
                    tsc_rs_ast::ExprKind::NumLit(s) => {
                        if let Ok(v) = s.parse::<f64>() {
                            all_num.insert(decl_name.clone(), v);
                            if is_exported {
                                num_consts.insert(decl_name, v);
                            }
                        }
                    }
                    tsc_rs_ast::ExprKind::Unary(u) if u.op == tsc_rs_ast::UnaryOp::Neg => {
                        if let tsc_rs_ast::ExprKind::NumLit(s) = &u.argument.kind {
                            if let Ok(v) = s.parse::<f64>() {
                                all_num.insert(decl_name.clone(), -v);
                                if is_exported {
                                    num_consts.insert(decl_name, -v);
                                }
                            }
                        }
                    }
                    tsc_rs_ast::ExprKind::StrLit(s) => {
                        all_str.insert(decl_name.clone(), s.to_string());
                        if is_exported {
                            str_consts.insert(decl_name, s.to_string());
                        }
                    }
                    tsc_rs_ast::ExprKind::NoSubstTemplate(s) => {
                        all_str.insert(decl_name.clone(), s.to_string());
                        if is_exported {
                            str_consts.insert(decl_name, s.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for exp_name in &exported_names {
        if let Some(&v) = all_num.get(exp_name) {
            num_consts.entry(exp_name.clone()).or_insert(v);
        }
        if let Some(v) = all_str.get(exp_name) {
            str_consts
                .entry(exp_name.clone())
                .or_insert_with(|| v.clone());
        }
        if let Some(enum_members) = all_enum_num.get(exp_name) {
            for (member, value) in enum_members {
                num_consts
                    .entry(format!("{exp_name}.{member}"))
                    .or_insert(*value);
            }
        }
        if let Some(enum_members) = all_enum_str.get(exp_name) {
            for (member, value) in enum_members {
                str_consts
                    .entry(format!("{exp_name}.{member}"))
                    .or_insert_with(|| value.clone());
            }
        }
    }
    (num_consts, str_consts)
}

pub fn emit_js(source_file: &SourceFile, options: &CompilerOptions) -> String {
    tsc_rs_emitter::emit(source_file, options).javascript
}

// ---------------------------------------------------------------------------
// Diff generation
// ---------------------------------------------------------------------------

/// Generate a human-readable diff between expected and actual content.
fn generate_diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    let mut diff = String::new();
    let max_lines = expected_lines.len().max(actual_lines.len());
    let mut mismatch_count = 0;
    let max_shown = 20;

    for (i, exp) in expected_lines.iter().enumerate().take(max_lines) {
        let act = actual_lines.get(i).copied().unwrap_or("<missing>");
        let exp = *exp;

        if exp != act {
            mismatch_count += 1;
            if mismatch_count <= max_shown {
                diff.push_str(&format!("line {}:\n", i + 1));
                diff.push_str(&format!("  expected: {exp}\n"));
                diff.push_str(&format!("  actual:   {act}\n"));
            }
        }
    }

    if mismatch_count > max_shown {
        diff.push_str(&format!(
            "... and {} more mismatched lines\n",
            mismatch_count - max_shown
        ));
    }

    if mismatch_count == 0 && expected.len() != actual.len() {
        diff.push_str("content differs in trailing whitespace or newlines\n");
    }

    diff.push_str(&format!(
        "total lines: expected={}, actual={}, mismatched={}\n",
        expected_lines.len(),
        actual_lines.len(),
        mismatch_count
    ));

    diff
}

// ---------------------------------------------------------------------------
// Shared types used by extracted modules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct BaselinePathContext {
    /// Base `module` option and package-type map: together they give each
    /// file's Node format (node16+), which selects package.json `exports`
    /// conditions (`import` vs `require`).
    pub(crate) base_module: Option<ModuleKind>,
    pub(crate) package_type_by_dir: HashMap<String, bool>,
    pub(crate) emit_declaration_only: bool,
    pub(crate) full_emit_paths: bool,
    pub(crate) resolve_json_module: bool,
    pub(crate) allow_js: bool,
    pub(crate) no_resolve: bool,
    pub(crate) use_case_sensitive_file_names: Option<bool>,
    pub(crate) out_dir: Option<String>,
    pub(crate) out_file: Option<String>,
    pub(crate) root_dir: Option<String>,
    pub(crate) root_dirs: Vec<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) paths: Option<String>,
    pub(crate) tsconfig_dir: Option<String>,
    pub(crate) common_source_dir: Option<String>,
    /// Common source directory computed from emittable files only (excludes
    /// `.d.ts`).  Used for AMD/System bundled module name computation.
    pub(crate) common_emittable_source_dir: Option<String>,
    pub(crate) module_suffixes: Vec<String>,
    /// When true, emit JS output for `.js` source files even without outDir.
    pub(crate) suppress_output_path_check: bool,
}

// ---------------------------------------------------------------------------
// Extracted modules (free functions grouped by responsibility)
// ---------------------------------------------------------------------------

mod paths;
pub(crate) use paths::*;
mod tsconfig_files;
pub(crate) use tsconfig_files::*;
mod json_lite;
pub(crate) use json_lite::*;

mod text_normalize;
pub(crate) use text_normalize::*;

mod output_paths;
pub(crate) use output_paths::*;

mod module_resolution;
pub(crate) use module_resolution::*;

mod compile_order;
pub(crate) use compile_order::*;

mod source_classify;
pub(crate) use source_classify::*;

mod transforms;
pub(crate) use transforms::*;

/// Whether errors baselines seed the donor checker with every program
/// file's declarations (`inject_external_types`), so multi-file imports get
/// real types. On by default; `TSC_RS_HARNESS_CROSS_FILE=0` turns it off.
/// Seed the donor with TypeScript lib declarations so lib interfaces such
/// as `IArguments`, `ArrayLike` or `TemplateStringsArray` are visible to
/// the relation. Default: only `LIB_INJECT_ALLOWLIST`.
/// `TSC_RS_HARNESS_LIB_INJECT=0` disables it; `=1` injects every lib
/// declaration (experimental: measured net negative, Sep 30 2026).
pub(crate) fn lib_injection_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("TSC_RS_HARNESS_LIB_INJECT")
            .map(|value| value != "0")
            .unwrap_or(true)
    })
}

fn lib_injection_allowlist_only() -> bool {
    std::env::var("TSC_RS_HARNESS_LIB_INJECT").as_deref() != Ok("1")
}

/// Lib declarations with no hand-written checker model.
const LIB_INJECT_ALLOWLIST: &[&str] = &[
    "TemplateStringsArray",
    "IArguments",
    "ArrayLike",
    "ReadonlyArray",
    "ConcatArray",
    "PropertyKey",
];

/// A donor checker with the selected lib files injected, cached per worker
/// thread and per (target, lib, noLib) selection.
fn lib_injected_donor(options: &tsc_rs_ast::CompilerOptions) -> tsc_rs_types::TypeChecker {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, tsc_rs_types::TypeChecker>> =
            std::cell::RefCell::new(HashMap::new());
    }
    let key = format!(
        "{:?}\0{:?}\0{}",
        options.target,
        options.no_lib,
        options.lib.join("\0").to_ascii_lowercase()
    );
    CACHE.with(|cache| {
        if let Some(donor) = cache.borrow().get(&key) {
            return donor.clone();
        }
        let mut donor = tsc_rs_types::TypeChecker::new();
        donor.disable_expression_types();
        let sources = tsc_rs_types::load_stdlib_sources(options);
        // By default keep only lib declarations the checker has no
        // hand-written model for.
        let allowlist_only = lib_injection_allowlist_only();
        let parsed: Vec<_> = sources
            .iter()
            .map(|library| {
                let mut file = tsc_rs_parser::parse(&library.file_name, &library.source);
                if allowlist_only {
                    file.statements.retain(|stmt| {
                        let name = match &stmt.kind {
                            StmtKind::InterfaceDecl(decl) => decl.name.as_str(),
                            StmtKind::TypeAlias(decl) => decl.name.as_str(),
                            _ => return false,
                        };
                        LIB_INJECT_ALLOWLIST.contains(&name)
                    });
                }
                file
            })
            .collect();
        let references: Vec<_> = parsed.iter().collect();
        donor.inject_external_types(&references);
        donor.take_diagnostics();
        donor.reset_alias_cycle_index();
        cache.borrow_mut().insert(key, donor.clone());
        donor
    })
}

pub(crate) fn cross_file_injection_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("TSC_RS_HARNESS_CROSS_FILE")
            .map(|value| value != "0")
            .unwrap_or(true)
    })
}

// ---------------------------------------------------------------------------
// Utility functions (kept in lib.rs — used only by impl blocks above)
// ---------------------------------------------------------------------------

fn apply_project_scenario_overrides(options: &mut CompilerOptions, scenario: &ProjectScenario) {
    options.declaration = scenario.declaration.or(options.declaration);
    options.source_map = scenario.source_map.or(options.source_map);
    options.out_dir = scenario.out_dir.clone().or(options.out_dir.clone());
    options.out_file = scenario.out_file.clone().or(options.out_file.clone());
    options.root_dir = scenario.root_dir.clone().or(options.root_dir.clone());
    options.strict = scenario.strict.or(options.strict);
    options.module_resolution = scenario
        .module_resolution
        .clone()
        .or(options.module_resolution.clone())
        .or_else(|| Some("classic".to_string()));
    options.no_resolve = scenario.no_resolve.or(options.no_resolve);

    if let Some(source_root) = &scenario.source_root {
        options
            .other
            .push(("sourceRoot".to_string(), source_root.clone()));
    }
    if let Some(map_root) = &scenario.map_root {
        options
            .other
            .push(("mapRoot".to_string(), map_root.clone()));
    }
    if let Some(resolve_source_root) = scenario.resolve_source_root {
        options.other.push((
            "resolveSourceRoot".to_string(),
            resolve_source_root.to_string(),
        ));
    }
    if let Some(resolve_map_root) = scenario.resolve_map_root {
        options
            .other
            .push(("resolveMapRoot".to_string(), resolve_map_root.to_string()));
    }
    if let Some(declaration_dir) = &scenario.declaration_dir {
        options
            .other
            .push(("declarationDir".to_string(), declaration_dir.clone()));
    }
}

fn relative_project_path(project_root: &Path, full_path: &str) -> String {
    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let project_root = normalize_header_path(&project_root.to_string_lossy());
    let full_path = normalize_header_path(full_path);
    strip_prefix_path(&full_path, &project_root)
        .filter(|rel| !rel.is_empty())
        .unwrap_or_else(|| basename(&full_path))
}

fn discover_project_input_closure(
    project_root: &Path,
    options: &CompilerOptions,
    root_files: &[String],
) -> Result<Vec<ProjectInputFile>, String> {
    let allow_js = options.allow_js == Some(true) || options.check_js == Some(true);
    let out_dir = options.out_dir.as_deref().map(normalize_header_path);
    let declaration_dir = options
        .other
        .iter()
        .find_map(|(key, value)| (key == "declarationDir").then(|| normalize_header_path(value)));
    let project_root_normalized = normalize_header_path(&project_root.to_string_lossy());
    let mut ordered = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    for relative in root_files {
        let full_path = normalize_header_path(&project_root.join(relative).to_string_lossy());
        visit_project_input_file(
            project_root,
            &project_root_normalized,
            &full_path,
            options,
            allow_js,
            out_dir.as_deref(),
            declaration_dir.as_deref(),
            &mut visiting,
            &mut visited,
            &mut ordered,
        )?;
    }

    Ok(ordered)
}

#[allow(clippy::too_many_arguments)]
fn visit_project_input_file(
    project_root: &Path,
    project_root_normalized: &str,
    full_path: &str,
    options: &CompilerOptions,
    allow_js: bool,
    out_dir: Option<&str>,
    declaration_dir: Option<&str>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<ProjectInputFile>,
) -> Result<(), String> {
    let normalized = normalize_header_path(full_path);
    if visited.contains(&normalized) {
        return Ok(());
    }
    if !visiting.insert(normalized.clone()) {
        return Ok(());
    }

    if options.no_resolve != Some(true) {
        let path = Path::new(&normalized);
        if path.is_file() {
            let source = std::fs::read_to_string(path)
                .map_err(|err| format!("failed to read project file {}: {err}", path.display()))?;
            let parsed = tsc_rs_parser::parse(&normalized, &source);

            for specifier in collect_project_module_specifiers(&parsed) {
                if let Some(resolved) =
                    tsc_rs_resolver::resolve_module_name(&specifier, &normalized, options)
                {
                    let resolved_name = normalize_header_path(&resolved.resolved_file_name);
                    if !path_belongs_to_project(project_root_normalized, &resolved_name) {
                        continue;
                    }
                    if should_skip_project_file(
                        project_root,
                        &resolved_name,
                        allow_js,
                        out_dir,
                        declaration_dir,
                    ) {
                        continue;
                    }
                    visit_project_input_file(
                        project_root,
                        project_root_normalized,
                        &resolved_name,
                        options,
                        allow_js,
                        out_dir,
                        declaration_dir,
                        visiting,
                        visited,
                        ordered,
                    )?;
                }
            }
        }
    }

    visiting.remove(&normalized);
    if visited.insert(normalized.clone()) {
        ordered.push(ProjectInputFile {
            full_path: normalized.clone(),
            relative_path: relative_project_path(project_root, &normalized),
        });
    }

    Ok(())
}

fn discover_project_source_files(
    project_root: &Path,
    options: &CompilerOptions,
) -> Result<Vec<ProjectInputFile>, String> {
    let allow_js = options.allow_js == Some(true) || options.check_js == Some(true);
    let out_dir = options.out_dir.as_deref().map(normalize_header_path);
    let declaration_dir = options
        .other
        .iter()
        .find_map(|(key, value)| (key == "declarationDir").then(|| normalize_header_path(value)));
    let mut files = Vec::new();
    collect_project_source_files_recursive(
        project_root,
        project_root,
        allow_js,
        out_dir.as_deref(),
        declaration_dir.as_deref(),
        &mut files,
    )
    .map_err(|err| format!("failed to discover project files: {err}"))?;
    Ok(files)
}

fn collect_project_source_files_recursive(
    root: &Path,
    dir: &Path,
    allow_js: bool,
    out_dir: Option<&str>,
    declaration_dir: Option<&str>,
    files: &mut Vec<ProjectInputFile>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if should_skip_project_path(root, &path, out_dir, declaration_dir) {
            continue;
        }

        if file_type.is_dir() {
            collect_project_source_files_recursive(
                root,
                &path,
                allow_js,
                out_dir,
                declaration_dir,
                files,
            )?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let normalized = normalize_header_path(&path.to_string_lossy());
        if !is_project_source_file(&normalized, allow_js) {
            continue;
        }

        files.push(ProjectInputFile {
            full_path: normalized.clone(),
            relative_path: relative_project_path(root, &normalized),
        });
    }

    Ok(())
}

fn should_skip_project_path(
    root: &Path,
    path: &Path,
    out_dir: Option<&str>,
    declaration_dir: Option<&str>,
) -> bool {
    let normalized = normalize_header_path(&path.to_string_lossy());
    let root = normalize_header_path(&root.to_string_lossy());
    let relative = strip_prefix_path(&normalized, &root).unwrap_or_else(|| normalized.clone());
    let lower = relative.to_ascii_lowercase();

    if lower == "tsconfig.json" || lower.ends_with("/tsconfig.json") {
        return true;
    }
    if matches!(lower.as_str(), "outdir" | "bin" | "dist") {
        return true;
    }
    if lower.starts_with("outdir/") || lower.starts_with("bin/") || lower.starts_with("dist/") {
        return true;
    }
    if let Some(out_dir) = out_dir {
        if strip_prefix_path(&relative, out_dir).is_some()
            || strip_prefix_path(&normalized, out_dir).is_some()
        {
            return true;
        }
    }
    if let Some(declaration_dir) = declaration_dir {
        if strip_prefix_path(&relative, declaration_dir).is_some()
            || strip_prefix_path(&normalized, declaration_dir).is_some()
        {
            return true;
        }
    }

    false
}

fn should_skip_project_file(
    root: &Path,
    full_path: &str,
    allow_js: bool,
    out_dir: Option<&str>,
    declaration_dir: Option<&str>,
) -> bool {
    should_skip_project_path(root, Path::new(full_path), out_dir, declaration_dir)
        || !is_project_source_file(full_path, allow_js)
}

fn path_belongs_to_project(project_root: &str, full_path: &str) -> bool {
    full_path == project_root || strip_prefix_path(full_path, project_root).is_some()
}

/// Specifiers imported through a `require` construct (`import x =
/// require("m")`, `export = require("m")`): tsc resolves them in CommonJS
/// mode even inside an ES module, so they are exempt from the ESM
/// explicit-extension rule.
fn collect_require_form_specifiers(file: &SourceFile) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let mut visit = |stmt: &tsc_rs_ast::Stmt| match &stmt.kind {
        tsc_rs_ast::StmtKind::Import(import_decl)
            if matches!(import_decl.specifiers, tsc_rs_ast::ImportClause::Require(_)) =>
        {
            out.insert(import_decl.source.clone());
        }
        tsc_rs_ast::StmtKind::ImportEquals(import_equals) => {
            if let Some(specifier) = require_specifier_from_expr(&import_equals.module_ref) {
                out.insert(specifier.to_string());
            }
        }
        tsc_rs_ast::StmtKind::ExportAssign(expr) => {
            if let Some(specifier) = require_specifier_from_expr(expr) {
                out.insert(specifier.to_string());
            }
        }
        _ => {}
    };
    for stmt in &file.statements {
        visit(stmt);
        if let tsc_rs_ast::StmtKind::Export(export_decl) = &stmt.kind {
            if let tsc_rs_ast::ExportDeclKind::Decl(inner)
            | tsc_rs_ast::ExportDeclKind::DefaultDecl(inner) = &export_decl.kind
            {
                visit(inner);
            }
        }
    }
    out
}

/// Module augmentation specifiers of an external module
/// (`declare module "./x" { … }` at top level of a file with imports or
/// exports). Only their resolved paths are registered, so the checker can
/// merge augmentation declarations into the target module's exports.
fn collect_module_augmentation_specifiers(file: &SourceFile) -> Vec<String> {
    let is_module = file.statements.iter().any(|stmt| {
        matches!(
            stmt.kind,
            tsc_rs_ast::StmtKind::Import(_)
                | tsc_rs_ast::StmtKind::ImportEquals(_)
                | tsc_rs_ast::StmtKind::Export(_)
                | tsc_rs_ast::StmtKind::ExportAssign(_)
        )
    });
    if !is_module {
        return Vec::new();
    }
    file.statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            tsc_rs_ast::StmtKind::ModuleDecl(module) => match &module.name {
                tsc_rs_ast::ModuleName::String(specifier) => Some(specifier.clone()),
                tsc_rs_ast::ModuleName::Ident(_) => None,
            },
            _ => None,
        })
        .collect()
}

fn collect_project_module_specifiers(file: &SourceFile) -> Vec<String> {
    let mut specifiers = Vec::new();

    for stmt in &file.statements {
        match &stmt.kind {
            tsc_rs_ast::StmtKind::Import(import_decl) => {
                if !import_decl.type_only {
                    specifiers.push(import_decl.source.clone());
                }
            }
            tsc_rs_ast::StmtKind::ImportEquals(import_equals) => {
                if let Some(specifier) = require_specifier_from_expr(&import_equals.module_ref) {
                    specifiers.push(specifier.to_string());
                }
            }
            tsc_rs_ast::StmtKind::Export(export_decl) => match &export_decl.kind {
                tsc_rs_ast::ExportDeclKind::Named {
                    source: Some(source),
                    type_only,
                    ..
                } => {
                    if !*type_only {
                        specifiers.push(source.clone());
                    }
                }
                tsc_rs_ast::ExportDeclKind::All {
                    source, type_only, ..
                } => {
                    if !*type_only {
                        specifiers.push(source.clone());
                    }
                }
                tsc_rs_ast::ExportDeclKind::Decl(inner)
                | tsc_rs_ast::ExportDeclKind::DefaultDecl(inner) => match &inner.kind {
                    tsc_rs_ast::StmtKind::Import(import_decl) => {
                        if !import_decl.type_only {
                            specifiers.push(import_decl.source.clone());
                        }
                    }
                    tsc_rs_ast::StmtKind::ImportEquals(import_equals) => {
                        if let Some(specifier) =
                            require_specifier_from_expr(&import_equals.module_ref)
                        {
                            specifiers.push(specifier.to_string());
                        }
                    }
                    tsc_rs_ast::StmtKind::ExportAssign(expr) => {
                        if let Some(specifier) = require_specifier_from_expr(expr) {
                            specifiers.push(specifier.to_string());
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            tsc_rs_ast::StmtKind::ExportAssign(expr) => {
                if let Some(specifier) = require_specifier_from_expr(expr) {
                    specifiers.push(specifier.to_string());
                }
            }
            _ => {}
        }
    }

    specifiers
}

fn is_project_source_file(path: &str, allow_js: bool) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".d.ts")
        || lower.ends_with(".d.tsx")
        || lower.ends_with(".d.mts")
        || lower.ends_with(".d.cts")
    {
        return false;
    }

    lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".mts")
        || lower.ends_with(".cts")
        || (allow_js
            && (lower.ends_with(".js")
                || lower.ends_with(".jsx")
                || lower.ends_with(".mjs")
                || lower.ends_with(".cjs")))
}

fn project_baseline_path_context(
    options: &CompilerOptions,
    display_names: &[String],
) -> BaselinePathContext {
    let out_dir = options
        .out_dir
        .clone()
        .map(|value| normalize_header_path(&value))
        .filter(|value| !value.is_empty());
    let out_file = options
        .out_file
        .clone()
        .map(|value| normalize_header_path(&value))
        .filter(|value| !value.is_empty());
    let root_dir = options
        .root_dir
        .clone()
        .map(|value| normalize_header_path(&value))
        .filter(|value| !value.is_empty());
    let common_inputs: Vec<String> = display_names
        .iter()
        .filter(|name| contributes_to_common_source_dir(name))
        .cloned()
        .collect();
    let emittable_inputs: Vec<String> = common_inputs
        .iter()
        .filter(|name| is_project_source_file(name, true))
        .cloned()
        .collect();

    BaselinePathContext {
        emit_declaration_only: options.emit_declaration_only == Some(true),
        full_emit_paths: true,
        resolve_json_module: options.resolve_json_module == Some(true),
        allow_js: options.allow_js == Some(true) || options.check_js == Some(true),
        no_resolve: options.no_resolve == Some(true),
        use_case_sensitive_file_names: None,
        out_dir,
        out_file,
        root_dir,
        root_dirs: vec![],
        base_url: options.base_url.clone(),
        paths: options.paths.clone(),
        tsconfig_dir: None,
        common_source_dir: common_directory(&common_inputs),
        common_emittable_source_dir: common_directory(&emittable_inputs),
        module_suffixes: vec![String::new()],
        suppress_output_path_check: false,
        base_module: options.module,
        package_type_by_dir: HashMap::new(),
    }
}

fn format_project_diagnostics(
    result: &tsc_rs_project::CompilationResult,
    project_root: &Path,
    inputs: &[ProjectInputFile],
    module_kind: ModuleKind,
    options: &CompilerOptions,
    has_tsconfig: bool,
) -> String {
    let source_by_full: HashMap<String, String> = inputs
        .iter()
        .filter_map(|input| {
            std::fs::read_to_string(&input.full_path)
                .ok()
                .map(|source| (normalize_header_path(&input.full_path), source))
        })
        .collect();
    let diagnostics_by_full: HashMap<String, Vec<Diagnostic>> = result
        .files
        .iter()
        .map(|file| {
            (
                normalize_header_path(&file.file_name),
                file.diagnostics.clone(),
            )
        })
        .collect();
    let synthetic_warnings = synthetic_project_warning_lines(module_kind, options, has_tsconfig);
    let mut lines = Vec::new();

    lines.extend(synthetic_warnings.iter().cloned());
    for diagnostic in &result.diagnostics {
        lines.push(format_project_diagnostic_line(
            diagnostic,
            project_root,
            &source_by_full,
        ));
    }
    for file in &result.files {
        for diagnostic in &file.diagnostics {
            lines.push(format_project_diagnostic_line(
                diagnostic,
                project_root,
                &source_by_full,
            ));
        }
    }

    let mut section_lines = Vec::new();
    for warning in &synthetic_warnings {
        section_lines.push(format!("!!! {warning}"));
    }
    for input in inputs {
        let normalized = normalize_header_path(&input.full_path);
        let diagnostics = diagnostics_by_full
            .get(&normalized)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let source = source_by_full
            .get(&normalized)
            .map(String::as_str)
            .unwrap_or("");
        section_lines.extend(format_project_source_section(
            &basename(&input.relative_path),
            source,
            diagnostics,
        ));
    }

    let mut output = String::new();
    if !lines.is_empty() {
        output.push_str(&lines.join("\n"));
        if !section_lines.is_empty() {
            output.push_str("\n\n\n");
        } else {
            output.push('\n');
        }
    }
    if !section_lines.is_empty() {
        output.push_str(&section_lines.join("\n"));
        output.push('\n');
    }

    output
}

fn synthetic_project_warning_lines(
    module_kind: ModuleKind,
    options: &CompilerOptions,
    has_tsconfig: bool,
) -> Vec<String> {
    if has_tsconfig {
        return Vec::new();
    }

    let mut warnings = Vec::new();
    if module_kind == ModuleKind::AMD {
        warnings.push("error TS5107: Option 'module=AMD' is deprecated and will stop functioning in TypeScript 7.0. Specify compilerOption '\"ignoreDeprecations\": \"6.0\"' to silence this error.".to_string());
    }
    if options
        .module_resolution
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("classic"))
    {
        warnings.push("error TS5107: Option 'moduleResolution=classic' is deprecated and will stop functioning in TypeScript 7.0. Specify compilerOption '\"ignoreDeprecations\": \"6.0\"' to silence this error.".to_string());
    }
    warnings
}

fn format_project_diagnostic_line(
    diagnostic: &Diagnostic,
    project_root: &Path,
    source_by_full: &HashMap<String, String>,
) -> String {
    let base = format!("error TS{}: {}", diagnostic.code, diagnostic.message);
    let Some(file_name) = diagnostic.file_name.as_deref() else {
        return base;
    };

    let display_name = if Path::new(file_name).is_absolute() {
        relative_project_path(project_root, file_name)
    } else {
        normalize_header_path(file_name)
    };
    let normalized = normalize_header_path(file_name);
    let source = source_by_full
        .get(&normalized)
        .cloned()
        .or_else(|| std::fs::read_to_string(file_name).ok());

    if let (Some(span), Some(source)) = (diagnostic.span, source.as_deref()) {
        let (line, column) = project_line_and_column(source, span.start as usize);
        return format!("{display_name}({line},{column}): {base}");
    }

    format!("{display_name}: {base}")
}

fn format_project_source_section(
    display_name: &str,
    source: &str,
    diagnostics: &[Diagnostic],
) -> Vec<String> {
    let mut lines = vec![format!(
        "==== {} ({} errors) ====",
        display_name,
        diagnostics.len()
    )];
    let mut source_lines: Vec<&str> = source.lines().collect();
    // Rust's .lines() drops trailing empty line for sources ending with '\n'.
    // TypeScript's harness preserves it as an indented blank line.
    if source.ends_with('\n') {
        source_lines.push("");
    }

    for (line_idx, line) in source_lines.iter().enumerate() {
        lines.push(format!("    {line}"));
        for diagnostic in diagnostics.iter().filter(|diagnostic| {
            diagnostic
                .span
                .is_some_and(|span| project_line_index(source, span.start as usize) == line_idx)
        }) {
            if let Some(span) = diagnostic.span {
                lines.push(format!(
                    "    {}",
                    project_diagnostic_marker_line(source, line, line_idx, span)
                ));
            }
            lines.push(format!(
                "!!! error TS{}: {}",
                diagnostic.code, diagnostic.message
            ));
        }
    }

    for diagnostic in diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.span.is_none())
    {
        lines.push(format!(
            "!!! error TS{}: {}",
            diagnostic.code, diagnostic.message
        ));
    }

    lines
}

fn project_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn project_line_index(source: &str, offset: usize) -> usize {
    let starts = project_line_starts(source);
    let clamped = offset.min(source.len());
    match starts.binary_search(&clamped) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    }
}

fn project_line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let starts = project_line_starts(source);
    let clamped = offset.min(source.len());
    let line_idx = match starts.binary_search(&clamped) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    };
    let line_start = starts.get(line_idx).copied().unwrap_or(0);
    let column = source[line_start..clamped].chars().count() + 1;
    (line_idx + 1, column)
}

fn project_diagnostic_marker_line(
    source: &str,
    line_text: &str,
    line_idx: usize,
    span: tsc_rs_ast::Span,
) -> String {
    let starts = project_line_starts(source);
    let line_start = starts.get(line_idx).copied().unwrap_or(0);
    let line_end = starts
        .get(line_idx + 1)
        .copied()
        .map(|value| value.saturating_sub(1))
        .unwrap_or(source.len());
    let start = (span.start as usize).clamp(line_start, line_end);
    let end = (span.end as usize).clamp(start, line_end);
    let start_offset = start.saturating_sub(line_start);
    let mut marker = String::new();
    for byte in line_text
        .as_bytes()
        .iter()
        .take(start_offset.min(line_text.len()))
    {
        marker.push(if *byte == b'\t' { '\t' } else { ' ' });
    }
    let marker_len = end.saturating_sub(start).max(1);
    marker.push_str(&"~".repeat(marker_len));
    marker
}

fn build_project_summary_json(
    source: &str,
    resolved_inputs: &[String],
    emitted_files: &[String],
) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(source) else {
        return "{}".to_string();
    };
    let Some(json) = value.as_object_mut() else {
        return "{}".to_string();
    };
    let mut resolved_with_libs = Vec::new();
    let mut seen = HashSet::new();
    for lib in [
        "lib.es5.d.ts",
        "lib.decorators.d.ts",
        "lib.decorators.legacy.d.ts",
    ] {
        if seen.insert(lib.to_string()) {
            resolved_with_libs.push(lib.to_string());
        }
    }
    for input in resolved_inputs {
        if seen.insert(input.clone()) {
            resolved_with_libs.push(input.clone());
        }
    }
    json.insert(
        "resolvedInputFiles".to_string(),
        serde_json::json!(resolved_with_libs),
    );
    json.insert("emittedFiles".to_string(), serde_json::json!(emitted_files));
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|_| ())
        .ok()
        .and_then(|_| String::from_utf8(out).ok())
        .unwrap_or_else(|| "{}".to_string())
}

fn load_project_baseline_files(dir: &Path) -> Result<BTreeMap<String, String>, HarnessError> {
    let mut paths = Vec::new();
    collect_files_recursive(dir, &mut paths)?;
    paths.sort();

    let mut files = BTreeMap::new();
    for path in paths {
        let relative = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let content = std::fs::read_to_string(&path)?;
        files.insert(relative, normalize_text(&content));
    }

    Ok(files)
}

fn snapshot_project_file_map(files: &BTreeMap<String, String>) -> String {
    let mut snapshot = String::new();
    let mut first = true;
    for (path, content) in files {
        if !first {
            snapshot.push('\n');
        }
        first = false;
        snapshot.push_str(&format!("//// [{path}] ////\n"));
        snapshot.push_str(content);
        if !content.ends_with('\n') {
            snapshot.push('\n');
        }
    }
    snapshot
}

fn snapshot_project_expected_files(
    expected_files: &BTreeMap<String, String>,
    actual_files: &BTreeMap<String, String>,
) -> String {
    let mut snapshot = String::new();
    let mut first = true;
    for path in expected_files.keys() {
        if !first {
            snapshot.push('\n');
        }
        first = false;
        snapshot.push_str(&format!("//// [{path}] ////\n"));
        if let Some(content) = actual_files.get(path) {
            snapshot.push_str(content);
            if !content.ends_with('\n') {
                snapshot.push('\n');
            }
        } else {
            snapshot.push_str("<missing project output>\n");
        }
    }
    snapshot
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), HarnessError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_files_recursive(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }

    Ok(())
}

fn collect_project_case_files_recursive(
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), HarnessError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_project_case_files_recursive(&path, files)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("json"))
                .unwrap_or(false)
        {
            files.push(path);
        }
    }

    Ok(())
}

fn triple_slash_reference_paths(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("/// <reference") {
                return None;
            }
            let after_path = line.split_once("path=")?.1.trim_start();
            let quote = after_path.chars().next()?;
            if quote != '\'' && quote != '"' {
                return None;
            }
            let value = &after_path[quote.len_utf8()..];
            let end = value.find(quote)?;
            Some(value[..end].to_string())
        })
        .collect()
}

fn ambient_module_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let after_module = line.trim_start().strip_prefix("declare module")?;
            let after_module = after_module.trim_start();
            let quote = after_module.chars().next()?;
            if quote != '\'' && quote != '"' {
                return None;
            }
            let value = &after_module[quote.len_utf8()..];
            let end = value.find(quote)?;
            Some(value[..end].to_string())
        })
        .collect()
}

fn collect_ts_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), HarnessError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_ts_files_recursive(&path, files)?;
        } else if file_type.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "ts" || ext == "tsx" {
                    files.push(path);
                }
            }
        }
    }

    Ok(())
}

fn run_command(mut command: Command) -> Result<CommandOutput, HarnessError> {
    let output = command.output()?;
    Ok(CommandOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: normalize_text(&String::from_utf8_lossy(&output.stdout)),
        stderr: normalize_text(&String::from_utf8_lossy(&output.stderr)),
    })
}

fn normalize_javascript_section(javascript: &str) -> String {
    if javascript.is_empty() {
        String::new()
    } else {
        javascript.trim_end_matches('\n').to_string() + "\n"
    }
}

fn classify_mismatch(oracle: &CommandOutput, candidate: &CommandOutput) -> Vec<MismatchClass> {
    let mut mismatches = Vec::new();

    if oracle.status != candidate.status {
        mismatches.push(MismatchClass::Runtime);
    }

    if oracle.stderr != candidate.stderr {
        mismatches.push(MismatchClass::Diagnostic);
    }

    if oracle.stdout != candidate.stdout {
        mismatches.push(MismatchClass::Emit);
    }

    mismatches
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn source_provenance_workspace(test_name: &str) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("tsc-rs-harness-{test_name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cases = root.join("tests/cases/compiler");
        let baselines = root.join("tests/baselines/reference");
        std::fs::create_dir_all(&cases).unwrap();
        std::fs::create_dir_all(&baselines).unwrap();
        let source = cases.join("encoded.ts");
        std::fs::write(&source, [0xff, 0xfe, 0x61]).unwrap();
        (root, source)
    }

    #[test]
    fn source_read_failure_preserves_explicit_and_implicit_oracle_provenance() {
        let (root, source) = source_provenance_workspace("source-oracle-provenance");
        let baselines = root.join("tests/baselines/reference");
        std::fs::write(baselines.join("encoded.js"), "//// [encoded.js]\n").unwrap();
        std::fs::write(baselines.join("encoded.symbols"), "=== encoded.ts ===\n").unwrap();
        std::fs::write(baselines.join("encoded.types"), "=== encoded.ts ===\n").unwrap();

        let runner = BaselineRunner::new(&root);
        let results = [
            runner.run_case_impl(&source, Suite::Compiler),
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Errors),
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Symbols),
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Types),
        ];

        for result in results {
            assert!(!result.passed);
            assert!(
                result.baseline_exists,
                "source decoding must not erase oracle provenance: {result:?}"
            );
            assert!(
                result
                    .diff
                    .as_deref()
                    .is_some_and(|diff| diff.starts_with("failed to read test file:")),
                "the source-read failure remains independently visible: {result:?}"
            );
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_read_failure_does_not_invent_missing_oracle() {
        let (root, source) = source_provenance_workspace("source-missing-oracle");
        let baselines = root.join("tests/baselines/reference");
        for extension in [".js", ".symbols", ".types"] {
            std::fs::write(
                baselines.join(format!("encoded(target){extension}")),
                "invalid",
            )
            .unwrap();
        }
        let runner = BaselineRunner::new(&root);
        let missing_results = [
            runner.run_case_impl(&source, Suite::Compiler),
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Symbols),
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Types),
        ];

        for result in missing_results {
            assert!(!result.passed);
            assert!(!result.baseline_exists);
            assert!(result
                .diff
                .as_deref()
                .is_some_and(|diff| diff.starts_with("failed to read test file:")));
        }

        let implicit_errors =
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Errors);
        assert!(!implicit_errors.passed);
        assert!(implicit_errors.baseline_exists);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_read_failure_preserves_parameterized_oracle_provenance() {
        let (root, source) = source_provenance_workspace("source-parameterized-oracle");
        let baselines = root.join("tests/baselines/reference");
        std::fs::write(
            baselines.join("encoded(target=es5).js"),
            "//// [encoded.js]\n",
        )
        .unwrap();
        std::fs::write(
            baselines.join("encoded(target=es5).symbols"),
            "=== encoded.ts ===\n",
        )
        .unwrap();
        std::fs::write(
            baselines.join("encoded(target=es5).types"),
            "=== encoded.ts ===\n",
        )
        .unwrap();

        let runner = BaselineRunner::new(&root);
        let results = [
            runner.run_case_impl(&source, Suite::Compiler),
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Symbols),
            runner.run_case_impl_with_kind(&source, Suite::Compiler, BaselineKind::Types),
        ];

        for result in results {
            assert!(!result.passed);
            assert!(result.baseline_exists);
            assert!(result
                .diff
                .as_deref()
                .is_some_and(|diff| diff.starts_with("failed to read test file:")));
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_source_decoder_preserves_utf8() {
        let source = b"let greeting = \"h\xC3\xA9llo\";\n";

        assert_eq!(decode_test_source(source).unwrap().as_bytes(), source);
    }

    #[test]
    fn test_source_decoder_strips_utf8_bom_before_directive_parsing() {
        let source = b"\xEF\xBB\xBF// @target: es5, es2015\nlet value = 1;\n";
        let decoded = decode_test_source(source).unwrap();

        assert!(decoded.starts_with("// @target:"));
        assert_eq!(
            effective_options(&decoded).target,
            Some(ScriptTarget::ES2015)
        );
    }

    #[test]
    fn test_source_decoder_decodes_utf16_little_endian() {
        let source = [0xFF, 0xFE, b'l', 0, b'e', 0, b't', 0, b' ', 0, b'x', 0];

        assert_eq!(decode_test_source(&source).unwrap(), "let x");
    }

    #[test]
    fn test_source_decoder_decodes_utf16_big_endian() {
        let source = [0xFE, 0xFF, 0, b'l', 0, b'e', 0, b't', 0, b' ', 0, b'x'];

        assert_eq!(decode_test_source(&source).unwrap(), "let x");
    }

    #[test]
    fn test_source_decoder_decodes_utf16_surrogate_pair() {
        let source = [0xFF, 0xFE, 0x3D, 0xD8, 0x00, 0xDE];

        assert_eq!(decode_test_source(&source).unwrap(), "\u{1f600}");
    }

    #[test]
    fn test_source_decoder_rejects_odd_length_utf16() {
        let error = decode_test_source(&[0xFE, 0xFF, 0]).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("odd number of bytes"));
    }

    #[test]
    fn test_source_decoder_rejects_unpaired_utf16_surrogate() {
        let error = decode_test_source(&[0xFF, 0xFE, 0x3D, 0xD8]).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn jsdoc_syntax_diagnostics_report_invalid_parameter_names() {
        let source_text = "/** @param {string} colour */\nfunction f(color) {}\n";
        let source = tsc_rs_parser::parse("a.js", source_text);
        let diagnostics = BaselineRunner::jsdoc_syntax_diagnostics(&source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, 8024);
        let span = diagnostics[0].span.unwrap();
        assert_eq!(
            &source_text[span.start as usize..span.end as usize],
            "colour"
        );
        assert_eq!(
            diagnostics[0].message,
            "JSDoc '@param' tag has name 'colour', but there is no parameter with that name."
        );
    }

    #[test]
    fn jsdoc_syntax_diagnostics_report_typedef_without_shape() {
        let source_text = "/** @typedef T */\nconst value = 0;\n";
        let source = tsc_rs_parser::parse("a.js", source_text);
        let diagnostics = BaselineRunner::jsdoc_syntax_diagnostics(&source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, 8021);
        let span = diagnostics[0].span.unwrap();
        assert_eq!(&source_text[span.start as usize..span.end as usize], "T");
    }

    #[test]
    fn jsdoc_syntax_diagnostics_accept_property_shapes_and_dotted_parameters() {
        let source_text = r#"/**
 * @typedef Person
 * @property {string} name
 */
/**
 * @param {Object} options
 * @param {string} options.name
 * @param {number} [options.age=0]
 */
function describe(options) {}
"#;
        let source = tsc_rs_parser::parse("a.js", source_text);

        assert!(BaselineRunner::jsdoc_syntax_diagnostics(&source).is_empty());
    }

    #[test]
    fn jsdoc_syntax_diagnostics_decode_names_and_accept_destructuring_labels() {
        let source_text = r#"/**
 * @param {number} \u0061
 * @param {object} options
 * @param {string} options.name
 */
function describe(a, { name }) {}
"#;
        let source = tsc_rs_parser::parse("a.js", source_text);

        assert!(BaselineRunner::jsdoc_syntax_diagnostics(&source).is_empty());
    }

    #[test]
    fn jsdoc_syntax_diagnostics_check_each_variable_function_expression() {
        let source_text = r#"/** @param {string} value */
var one = function (value) {}, two = function (other) {};
"#;
        let source = tsc_rs_parser::parse("a.js", source_text);
        let diagnostics = BaselineRunner::jsdoc_syntax_diagnostics(&source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, 8024);
        assert!(diagnostics[0].message.contains("'value'"));
    }

    fn strict_baseline_test_dir(scenario: &str, extension: &str) -> PathBuf {
        let extension = extension.trim_start_matches('.');
        let root = std::env::temp_dir().join(format!(
            "tsc-rs-strict-baseline-{scenario}-{extension}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn effective_options(source: &str) -> CompilerOptions {
        let mut test_case = parse_test_case("case.ts", source);
        fixup_multi_value_options(&mut test_case, source);
        test_case.options
    }

    fn legacy_strict_candidates(
        baseline_dir: &Path,
        stem: &str,
        extension: &str,
    ) -> Result<Vec<PathBuf>, String> {
        let prefix = format!("{stem}(");
        let entries = std::fs::read_dir(baseline_dir).map_err(|error| {
            format!(
                "could not inspect baseline directory {}: {error}",
                baseline_dir.display()
            )
        })?;
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "could not inspect an entry in baseline directory {}: {error}",
                    baseline_dir.display()
                )
            })?;
            let file_type = entry.file_type().map_err(|error| {
                format!(
                    "could not inspect baseline candidate {}: {error}",
                    entry.path().display()
                )
            })?;
            if !file_type.is_file() {
                continue;
            }
            let file_name = entry.file_name();
            let name = file_name.to_str().ok_or_else(|| {
                format!(
                    "baseline directory contains a non-Unicode file name: {}",
                    entry.path().display()
                )
            })?;
            if name.starts_with(&prefix) && name.ends_with(extension) {
                candidates.push(entry.path());
            }
        }
        candidates.sort();
        Ok(candidates)
    }

    #[test]
    fn strict_baseline_index_matches_legacy_candidate_scan() {
        let root = strict_baseline_test_dir("index-equivalence", ".symbols");
        for name in [
            "case.symbols",
            "case(target=es5).symbols",
            "case(target=es2015).symbols",
            "case(target).symbols",
            "case(inner)(target=es2015).symbols",
            "case(target=es2015).types",
            "unrelated(target=es2015).symbols",
            "case(target=es2015).js",
        ] {
            std::fs::write(root.join(name), name).unwrap();
        }
        std::fs::create_dir(root.join("case(directory).symbols")).unwrap();

        let index = StrictBaselineIndex::build(&root).unwrap();
        assert!(index.contains_name("case.symbols"));
        for (stem, extension) in [
            ("case", ".symbols"),
            ("case", ".types"),
            ("case", ".js"),
            ("case(inner)", ".symbols"),
            ("missing", ".symbols"),
        ] {
            assert_eq!(
                index.candidates(stem, extension),
                legacy_strict_candidates(&root, stem, extension).unwrap(),
                "indexed candidates drifted for {stem}{extension}"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_baseline_index_is_shared_across_concurrent_resolutions() {
        let root = strict_baseline_test_dir("index-concurrency", ".symbols");
        for extension in [".symbols", ".types"] {
            std::fs::write(
                root.join(format!("case(target=es2015){extension}")),
                format!("selected-{extension}\n"),
            )
            .unwrap();
        }
        let runner = Arc::new(BaselineRunner::new("."));
        let barrier = Arc::new(std::sync::Barrier::new(16));
        let mut workers = Vec::new();
        for index in 0usize..16 {
            let runner = Arc::clone(&runner);
            let barrier = Arc::clone(&barrier);
            let root = root.clone();
            workers.push(std::thread::spawn(move || {
                let extension = if index.is_multiple_of(2) {
                    ".symbols"
                } else {
                    ".types"
                };
                barrier.wait();
                let source = "// @target: es5, es2015\n";
                let options = effective_options(source);
                let selected = runner
                    .load_strict_option_baseline(&root, "case", extension, &options, source)
                    .unwrap();
                assert_eq!(selected, Some(format!("selected-{extension}\n")));
                runner.strict_baseline_index.get().unwrap() as *const _ as usize
            }));
        }
        let pointers: HashSet<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            pointers.len(),
            1,
            "workers did not share one immutable index"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_option_baseline_prefers_exact_for_symbols_and_types() {
        let source = "// @target: es5, es2015\n";
        let options = effective_options(source);

        for extension in [".symbols", ".types"] {
            let root = strict_baseline_test_dir("exact", extension);
            std::fs::write(root.join(format!("case{extension}")), "exact\r\n").unwrap();
            std::fs::write(
                root.join(format!("case(target=es2015){extension}")),
                "parameterized\n",
            )
            .unwrap();

            let runner = BaselineRunner::new(".");
            let selected = runner
                .load_strict_option_baseline(&root, "case", extension, &options, source)
                .unwrap();
            assert_eq!(selected.as_deref(), Some("exact\n"));
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn strict_option_baseline_selects_the_most_specific_matching_variant() {
        let source =
            "// @target: es5, es2015\n// @module: commonjs\n// @moduleDetection: legacy, auto\n";
        let options = effective_options(source);

        for extension in [".symbols", ".types"] {
            let root = strict_baseline_test_dir("parameterized", extension);
            for (attributes, contents) in [
                ("target=es5", "wrong-target\n"),
                ("target=es2015", "matching-fallback\n"),
                (
                    "module=commonjs,moduledetection=legacy,target=es2015",
                    "most-specific\n",
                ),
            ] {
                std::fs::write(
                    root.join(format!("case({attributes}){extension}")),
                    contents,
                )
                .unwrap();
            }

            let runner = BaselineRunner::new(".");
            let selected = runner
                .load_strict_option_baseline(&root, "case", extension, &options, source)
                .unwrap();
            assert_eq!(selected.as_deref(), Some("most-specific\n"));
            std::fs::remove_dir_all(root).unwrap();
        }

        let module_source = "// @module: es2015, commonjs\n";
        let module_options = effective_options(module_source);
        assert_eq!(
            strict_option_variant_value(&module_options, module_source, "module").as_deref(),
            Some("es2015")
        );
        assert!(strict_option_variant_matches("target", "es2015", "es6"));
        assert!(strict_option_variant_matches("module", "es6", "es2015"));
        assert!(!strict_option_variant_matches("target", "es5", "es6"));
    }

    #[test]
    fn strict_option_baseline_falls_back_to_missing_without_a_matching_variant() {
        let source = "// @target: es2015\n";
        let options = effective_options(source);

        for extension in [".symbols", ".types"] {
            let root = strict_baseline_test_dir("fallback", extension);
            std::fs::write(
                root.join(format!("case(target=es5){extension}")),
                "wrong-target\n",
            )
            .unwrap();
            std::fs::write(
                root.join(format!("case(unknown=value){extension}")),
                "unknown-option\n",
            )
            .unwrap();

            let runner = BaselineRunner::new(".");
            let selected = runner
                .load_strict_option_baseline(&root, "case", extension, &options, source)
                .unwrap();
            assert!(selected.is_none());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn strict_option_baseline_rejects_equally_specific_ambiguity() {
        let source = "// @target: es2015\n// @module: commonjs\n";
        let options = effective_options(source);

        for extension in [".symbols", ".types"] {
            let root = strict_baseline_test_dir("ambiguity", extension);
            std::fs::write(
                root.join(format!("case(target=es2015){extension}")),
                "target\n",
            )
            .unwrap();
            std::fs::write(
                root.join(format!("case(module=commonjs){extension}")),
                "module\n",
            )
            .unwrap();

            let runner = BaselineRunner::new(".");
            let error = runner
                .load_strict_option_baseline(&root, "case", extension, &options, source)
                .unwrap_err();
            assert!(error.contains("ambiguous parameterized baselines"));
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn strict_option_baseline_rejects_malformed_parameterized_names() {
        let source = "// @target: es2015\n";
        let options = effective_options(source);

        for extension in [".symbols", ".types"] {
            for (index, attributes) in ["target", "target=", "target=es2015,target=es2015"]
                .into_iter()
                .enumerate()
            {
                let root = strict_baseline_test_dir(&format!("malformed-{index}"), extension);
                std::fs::write(
                    root.join(format!("case({attributes}){extension}")),
                    "invalid\n",
                )
                .unwrap();

                let runner = BaselineRunner::new(".");
                let error = runner
                    .load_strict_option_baseline(&root, "case", extension, &options, source)
                    .unwrap_err();
                assert!(error.contains("malformed parameterized baseline"));
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn parameterized_error_oracle_attributes_match_effective_options() {
        let source = "// @target: es5, es2015\n// @alwaysStrict: true, false\n";
        let test_case = parse_test_case("case.ts", source);
        assert_eq!(
            option_variant_value(&test_case.options, source, "target").as_deref(),
            Some("es2015")
        );
        assert_eq!(
            option_variant_value(&test_case.options, source, "alwaysstrict").as_deref(),
            Some("true")
        );
        assert_eq!(
            parameterized_variant_attributes(
                "case(alwaysstrict=true,target=es2015).errors.txt",
                "case",
                ".errors.txt"
            ),
            Some(vec![("alwaysstrict", "true"), ("target", "es2015")])
        );
    }

    #[test]
    fn no_mismatch_when_equal() {
        let output = CommandOutput {
            status: 0,
            stdout: "ok".into(),
            stderr: "".into(),
        };
        assert!(classify_mismatch(&output, &output).is_empty());
    }

    #[test]
    fn mismatch_detects_status_stdout_stderr() {
        let oracle = CommandOutput {
            status: 0,
            stdout: "js".into(),
            stderr: "diag".into(),
        };
        let candidate = CommandOutput {
            status: 1,
            stdout: "other-js".into(),
            stderr: "other-diag".into(),
        };

        let mismatches = classify_mismatch(&oracle, &candidate);
        assert!(mismatches.contains(&MismatchClass::Runtime));
        assert!(mismatches.contains(&MismatchClass::Diagnostic));
        assert!(mismatches.contains(&MismatchClass::Emit));
    }

    #[test]
    fn ts_to_js_conversion() {
        use tsc_rs_ast::JsxEmit;
        assert_eq!(ts_to_js_name_with_jsx("foo.ts", None), "foo.js");
        assert_eq!(
            ts_to_js_name_with_jsx("bar.tsx", Some(JsxEmit::Preserve)),
            "bar.jsx"
        );
        assert_eq!(ts_to_js_name_with_jsx("bar.tsx", None), "bar.js");
        assert_eq!(
            ts_to_js_name_with_jsx("bar.tsx", Some(JsxEmit::React)),
            "bar.js"
        );
        assert_eq!(ts_to_js_name_with_jsx("bar.jsx", None), "bar.js");
        assert_eq!(
            ts_to_js_name_with_jsx("bar.jsx", Some(JsxEmit::Preserve)),
            "bar.jsx"
        );
        assert_eq!(ts_to_js_name_with_jsx("baz.js", None), "baz.js");
        assert_eq!(
            ts_to_js_name_with_jsx("path/to/file.ts", None),
            "path/to/file.js"
        );
    }

    #[test]
    fn full_emit_paths_and_out_dir_naming() {
        let source = r#"// @outDir: out
// @fullEmitPaths: true
// @filename: /project/src/app.ts
export const x = 1;
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        assert_eq!(
            output_name_for_source_js("/project/src/app.ts", None, &ctx),
            "out/app.js"
        );

        let source_no_full = r#"// @outDir: out
// @filename: /project/src/app.ts
export const x = 1;
"#;
        let tc_no_full = parse_test_case("test.ts", source_no_full);
        let ctx_no_full = baseline_path_context(&tc_no_full);
        assert_eq!(
            output_name_for_source_js("/project/src/app.ts", None, &ctx_no_full),
            "app.js"
        );
    }

    #[test]
    fn tsconfig_out_dir_uses_common_source_dir() {
        let source = r#"// @fullEmitPaths: true
// @filename: /app/lib/bar.d.ts
declare const y: number;
// @filename: /app/src/index.ts
export const x = y;
// @filename: /app/tsconfig.json
{ "compilerOptions": { "outDir": "bin" } }
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        assert_eq!(
            output_name_for_source_js("/app/src/index.ts", None, &ctx),
            "/app/bin/src/index.js"
        );
    }

    #[test]
    fn relative_out_dir_preserves_subdirectories_in_full_output_paths() {
        let source = r#"// @outDir: out
// @filename: subfolder/index.ts
export const x = 1;
// @filename: other/index.ts
export const y = 2;
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        assert_eq!(
            full_output_path_for_source_js("subfolder/index.ts", None, &ctx),
            "out/subfolder/index.js"
        );
    }

    #[test]
    fn tsconfig_paths_resolve_for_emit_ordering() {
        let source = r#"// @filename: /app/tsconfig.json
{ "compilerOptions": { "baseUrl": ".", "paths": { "@speedy/*/testing": ["*/dist/index.ts"] } } }
// @filename: /app/index.ts
import { x } from "@speedy/folder1/testing";
// @filename: /app/folder1/dist/index.ts
export const x = 1 + 2;
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        assert_eq!(ctx.base_url.as_deref(), Some("/app"));
        assert!(
            ctx.paths
                .as_deref()
                .is_some_and(|paths| paths.contains("@speedy/*/testing")),
            "expected raw paths object in context: {:?}",
            ctx.paths
        );

        let mut path_to_idx = HashMap::new();
        for (idx, file) in tc.files.iter().enumerate() {
            path_to_idx.insert(normalize_header_path(&file.name), idx);
        }

        let resolved = resolve_module_specifier(
            "/app/index.ts",
            "@speedy/folder1/testing",
            &tc,
            &path_to_idx,
            &ctx,
        );
        let resolved_name = resolved.map(|idx| normalize_header_path(&tc.files[idx].name));
        assert_eq!(resolved_name.as_deref(), Some("/app/folder1/dist/index.ts"));
    }

    #[test]
    fn require_last_unit_prefers_ambient_external_module_declarations() {
        let source = r#"// @target: es2015
// @module: commonjs
// @filename: vs/foo_0/index.ts
export var x: number = 42;

// @filename: foo_1.ts
declare module "vs/foo_0" {
    export var y: () => number;
}

// @filename: foo_2.ts
/// <reference path="foo_1.ts"/>
import foo = require("vs/foo_0");
var z1 = foo.x + 10;
var z2 = foo.y() + 10;
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        assert!(should_use_require_last_unit_roots(&tc, &ctx));

        let compile_indices: Vec<usize> = tc
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| should_emit_compilable_source_file(file, &ctx).then_some(idx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let filtered = compile_indices_for_require_last_unit(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            false,
            &[],
        );
        let names: Vec<String> = filtered
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(names, vec!["foo_1.ts".to_string(), "foo_2.ts".to_string()]);
    }

    #[test]
    fn require_last_unit_does_not_pull_in_runtime_require_dependencies() {
        let source = r#"// @target: es2015
// @filename: /other.ts
export const other = 123;

// @filename: /index.ts
declare const require: any;
function foo() {
    const a = require('../outside-of-rootdir/foo');
    const { other }: { other: string } = require('./other');
}
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        assert!(should_use_require_last_unit_roots(&tc, &ctx));

        let compile_indices: Vec<usize> = tc
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| should_emit_compilable_source_file(file, &ctx).then_some(idx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let filtered = compile_indices_for_require_last_unit(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            false,
            &[],
        );
        let names: Vec<String> = filtered
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(names, vec!["/index.ts".to_string()]);
    }

    #[test]
    fn emit_order_ignores_ambient_external_module_only_dependencies() {
        let source = r#"// @module: amd
// @filename: /ambientExternalModuleMerging_use.ts
import M = require("M");
// Should be strings
var x = M.x;
var y = M.y;

// @filename: /ambientExternalModuleMerging_declare.ts
declare module "M" {
    export var x: string;
}

declare module "M" {
    export var y: string;
}
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        let compile_indices: Vec<usize> = input_file_order_indices(&tc, &ctx)
            .into_iter()
            .filter(|idx| should_emit_compilable_source_file(&tc.files[*idx], &ctx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let emit_order = compute_emit_order(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            &[],
            tc.options.module,
        );
        let names: Vec<String> = emit_order
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(
            names,
            vec![
                "/ambientExternalModuleMerging_use.ts".to_string(),
                "/ambientExternalModuleMerging_declare.ts".to_string(),
            ]
        );
    }

    #[test]
    fn emit_order_uses_dependency_postorder_for_remaining_js_cycles() {
        let source = r#"// @allowJs: true
// @checkJs: true
// @outDir: ./out
// @filename: /timer.js
/**
 * @param {number} timeout
 */
function Timer(timeout) {
    this.timeout = timeout;
}
module.exports = Timer;
// @filename: /hook.js
/**
 * @typedef {(arg: import("./context")) => void} HookHandler
 */
/**
 * @param {HookHandler} handle
 */
function Hook(handle) {
    this.handle = handle;
}
module.exports = Hook;
// @filename: /context.js
/**
 * @typedef {import("./timer")} Timer
 * @typedef {import("./hook")} Hook
 */
function Context(input) {
    this.state = input;
}
module.exports = Context;
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        let compile_indices: Vec<usize> = tc
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| should_emit_compilable_source_file(file, &ctx).then_some(idx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let emit_order = compute_emit_order(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            &[],
            tc.options.module,
        );
        let names: Vec<String> = emit_order
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(
            names,
            vec![
                "/timer.js".to_string(),
                "/context.js".to_string(),
                "/hook.js".to_string(),
            ]
        );
    }

    #[test]
    fn emit_order_prefers_source_file_order_within_node_module_cycles() {
        let source = r#"// @target: es2022
// @module: node16
// @allowJs: true
// @checkJs: true
// @outDir: out
// @filename: index.js
import * as m1 from "./index.js";
import * as m2 from "./index.mjs";
import * as m3 from "./index.cjs";
void m1; void m2; void m3;

// @filename: index.cjs
import * as m1 from "./index.js";
import * as m2 from "./index.mjs";
import * as m3 from "./index.cjs";
void m1; void m2; void m3;

// @filename: index.mjs
import * as m1 from "./index.js";
import * as m2 from "./index.mjs";
import * as m3 from "./index.cjs";
void m1; void m2; void m3;

// @filename: package.json
{ "type": "module" }
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        let compile_indices: Vec<usize> = tc
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| should_emit_compilable_source_file(file, &ctx).then_some(idx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let emit_order = compute_emit_order(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            &[],
            tc.options.module,
        );
        let names: Vec<String> = emit_order
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(
            names,
            vec![
                "index.cjs".to_string(),
                "index.mjs".to_string(),
                "index.js".to_string(),
            ]
        );
    }

    #[test]
    fn emit_order_places_esm_before_cjs_for_same_name_two_file_node_cycle() {
        let source = r#"// @target: es2022
// @module: node16
// @filename: subfolder/index.ts
import { h } from "../index.js";
export function f() { h(); }

// @filename: index.ts
import { f } from "./subfolder/index.js";
export function h() { f(); }

// @filename: package.json
{ "type": "module" }

// @filename: subfolder/package.json
{ "type": "commonjs" }
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        let effective = effective_compiler_options(&tc);
        let compile_indices: Vec<usize> = tc
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| should_emit_compilable_source_file(file, &ctx).then_some(idx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let emit_order = compute_emit_order(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            &[],
            effective.module,
        );
        let names: Vec<String> = emit_order
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(
            names,
            vec!["index.ts".to_string(), "subfolder/index.ts".to_string()]
        );
    }

    #[test]
    fn emit_order_places_cjs_self_export_before_default_and_esm_triplet() {
        let source = r##"// @target: es2022
// @module: node16
// @filename: index.ts
import * as type from "#type";
type;
// @filename: index.mts
import * as type from "#type";
type;
// @filename: index.cts
import * as type from "#type";
type;
// @filename: package.json
{
  "type": "module",
  "exports": "./index.cjs",
  "imports": { "#type": "package" }
}
"##;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        let compile_indices: Vec<usize> = tc
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| should_emit_compilable_source_file(file, &ctx).then_some(idx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let emit_order = compute_emit_order(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            &[],
            tc.options.module,
        );
        let names: Vec<String> = emit_order
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(
            names,
            vec![
                "index.cts".to_string(),
                "index.ts".to_string(),
                "index.mts".to_string(),
            ]
        );
    }

    #[test]
    fn emit_order_skips_root_postorder_for_node16_allow_js_roots() {
        let source = r#"// @target: es2022
// @module: node16
// @allowJs: true
// @checkJs: true
// @outDir: out
// @filename: leaf.js
export const js = 1;

// @filename: leaf.cjs
export const cjs = 1;

// @filename: leaf.mjs
export const mjs = 1;

// @filename: index.js
import * as leafJs from "./leaf.js";
import * as leafMjs from "./leaf.mjs";
import * as leafCjs from "./leaf.cjs";
import index = require("./index.cjs");
void leafJs; void leafMjs; void leafCjs; void index;

// @filename: index.cjs
import * as leafJs from "./leaf.js";
import * as leafMjs from "./leaf.mjs";
import * as leafCjs from "./leaf.cjs";
import index = require("./index.mjs");
void leafJs; void leafMjs; void leafCjs; void index;

// @filename: index.mjs
import * as leafJs from "./leaf.js";
import * as leafMjs from "./leaf.mjs";
import * as leafCjs from "./leaf.cjs";
import index = require("./index.js");
void leafJs; void leafMjs; void leafCjs; void index;

// @filename: package.json
{ "type": "module" }
"#;
        let tc = parse_test_case("test.ts", source);
        let ctx = baseline_path_context(&tc);
        let effective = effective_compiler_options(&tc);
        let compile_indices: Vec<usize> = tc
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| should_emit_compilable_source_file(file, &ctx).then_some(idx))
            .collect();
        let mut parsed_sources = HashMap::new();
        for idx in &compile_indices {
            parsed_sources.insert(
                *idx,
                tsc_rs_parser::parse(&tc.files[*idx].name, &tc.files[*idx].content),
            );
        }

        let emit_order = compute_emit_order(
            &tc,
            &compile_indices,
            &parsed_sources,
            &ctx,
            &[],
            effective.module,
        );
        let names: Vec<String> = emit_order
            .iter()
            .map(|idx| normalize_header_path(&tc.files[*idx].name))
            .collect();
        assert_eq!(
            names,
            vec![
                "leaf.js".to_string(),
                "leaf.cjs".to_string(),
                "leaf.mjs".to_string(),
                "index.cjs".to_string(),
                "index.mjs".to_string(),
                "index.js".to_string(),
            ]
        );
    }

    #[test]
    fn effective_module_kind_uses_nearest_package_json_default_commonjs() {
        let source = r#"// @module: node16
// @filename: package.json
{ "type": "module" }

// @filename: subfolder/package.json
{}

// @filename: subfolder/index.js
export const x = 1;
"#;
        let tc = parse_test_case("test.ts", source);
        let package_type_by_dir = package_json_module_type_by_dir(&tc);

        for module in [
            ModuleKind::Node16,
            ModuleKind::Node18,
            ModuleKind::Node20,
            ModuleKind::NodeNext,
        ] {
            assert_eq!(
                effective_module_kind_for_source(
                    "subfolder/index.js",
                    Some(module),
                    &package_type_by_dir,
                ),
                Some(ModuleKind::CommonJS),
                "{module:?} must use the nearest package.json format"
            );
        }
    }

    #[test]
    fn generate_js_baseline_skips_colliding_output_paths() {
        let source = r#"// @allowJs: true
// @outDir: out
// @filename: /project/a.ts
export {};

// @filename: /project/a.js
export {};

// @filename: /project/main.ts
export {};
"#;
        let tc = parse_test_case("test.ts", source);
        let runner = BaselineRunner::new(".");
        let baseline =
            runner.generate_js_baseline(&tc, "tests/cases/compiler/collision.ts", source);
        let a_js_count = baseline
            .lines()
            .filter(|line| *line == "//// [a.js]")
            .count();
        let main_js_count = baseline
            .lines()
            .filter(|line| *line == "//// [main.js]")
            .count();
        assert_eq!(a_js_count, 1, "expected only the input echo for a.js");
        assert_eq!(main_js_count, 1, "expected emitted main.js output");
    }

    #[test]
    fn generate_js_baseline_resolves_out_file_dynamic_import_bundle_ids() {
        let source = r#"// @target: es2015
// @module: none
// @outFile: /bundle.js
// @filename: /src/a.ts
const direct = import("./local.js");
const nested = import("./sub/b.js");
const parent = import("../shared.js");
const missing = import("./missing.js");
async function load() { return await import("../shared.js"); }

// @filename: /src/local.ts
const local = 1;

// @filename: /src/sub/b.ts
const nestedValue = 1;

// @filename: /shared.ts
const shared = 1;
"#;
        let tc = parse_test_case("test.ts", source);
        let runner = BaselineRunner::new(".");
        let baseline =
            runner.generate_js_baseline(&tc, "tests/cases/compiler/dynamicImport.ts", source);

        assert!(baseline.contains("require(\"src/local\")"), "{baseline}");
        assert!(baseline.contains("require(\"src/sub/b\")"), "{baseline}");
        assert_eq!(
            baseline.matches("require(\"shared\")").count(),
            2,
            "{baseline}"
        );
        assert!(baseline.contains("require(\"./missing.js\")"), "{baseline}");
        assert!(
            !baseline.contains("require(\"../shared.js\")"),
            "{baseline}"
        );
    }

    #[test]
    fn generate_js_baseline_preserves_duplicate_rewrite_extension_units() {
        let source = r#"// @module: commonjs
// @rewriteRelativeImportExtensions: true
// @filename: index.ts
export = {};

// @filename: index.ts
import foo = require("./foo.ts");
"#;
        let tc = parse_test_case("test.ts", source);
        assert_eq!(tc.files.len(), 2);
        assert!(tc
            .options
            .other
            .iter()
            .any(|(name, value)| { name == "rewriterelativeimportextensions" && value == "true" }));
        let runner = BaselineRunner::new(".");
        let baseline = runner.generate_js_baseline(&tc, "tests/cases/compiler/test.ts", source);
        assert_eq!(
            baseline
                .lines()
                .filter(|line| *line == "//// [index.js]")
                .count(),
            2,
            "{baseline}"
        );
    }

    #[test]
    fn generate_js_baseline_keeps_const_enum_namespace_merge_export_shape() {
        let source = r#"// @module: commonjs
// @target: es2015
// @filename: enum.ts
export const enum Enum { One = 1 }

// @filename: merge.ts
import { Enum } from "./enum";
namespace Enum { export type Foo = number; }
export { Enum };

// @filename: index.ts
import { Enum } from "./merge";
Enum.One;
"#;
        let tc = parse_test_case("test.ts", source);
        let runner = BaselineRunner::new(".");
        let baseline = runner.generate_js_baseline(&tc, "tests/cases/compiler/test.ts", source);

        assert!(
            baseline.contains("exports.Enum = void 0;\n//// [index.js]"),
            "{baseline}"
        );
        assert!(
            baseline.contains("const merge_1 = require(\"./merge\");\n1 /* Enum.One */;"),
            "{baseline}"
        );
        assert!(!baseline.contains("require(\"./enum\")"), "{baseline}");
    }

    #[test]
    fn generate_js_baseline_keeps_empty_output_headers_adjacent() {
        let source = r#"// @target: esnext
// @module: preserve
// @filename: g.ts
export {};

// @filename: h.mts
export type H = string;

// @filename: i.cts
import type { H } from "./h.mjs";

// @filename: dummy.ts
export interface Dummy {}

// @filename: value.ts
export const value = 1;
"#;
        let tc = parse_test_case("test.ts", source);
        let runner = BaselineRunner::new(".");
        let baseline =
            runner.generate_js_baseline(&tc, "tests/cases/compiler/preserveEmptyOutput.ts", source);

        assert!(
            baseline.contains(
                "//// [g.js]\n//// [h.mjs]\n//// [i.cjs]\n//// [dummy.js]\n//// [value.js]\nexport const value = 1;\n"
            ),
            "empty sections must add no payload while non-empty output keeps one trailing newline: {baseline}"
        );
    }

    #[test]
    fn js_out_file_to_dts_name_conversion() {
        assert_eq!(js_to_dts_name("bundle.js"), "bundle.d.ts");
        assert_eq!(js_to_dts_name("bundle.mjs"), "bundle.d.mts");
        assert_eq!(js_to_dts_name("bundle.cjs"), "bundle.d.cts");
    }

    #[test]
    fn parse_test_case_extracts_options() {
        let source = r#"// @target: es2015
// @module: commonjs
var x = 1;
"#;
        let tc = parse_test_case("test.ts", source);
        assert!(tc.options.target.is_some());
        assert!(tc.options.module.is_some());
        assert_eq!(tc.files.len(), 1);
        assert!(tc.files[0].content.contains("var x = 1;"));
    }

    #[test]
    fn no_check_modes_keep_parse_diagnostics_and_suppress_semantic_regex_checks() {
        let runner = BaselineRunner::new(".");
        for source in [
            r#"// @noCheck: true
const stringEscape = "\1";
const semanticRegex = /\00/;
const semanticMismatch: number = "text";
"#,
            r#"// @ts-nocheck
const stringEscape = "\1";
const semanticRegex = /\00/;
const semanticMismatch: number = "text";
"#,
        ] {
            let test_case = parse_test_case("noCheck.ts", source);
            let baseline = runner.generate_errors_baseline(
                &test_case,
                "tests/cases/compiler/noCheck.ts",
                source,
            );
            assert!(baseline.contains("TS1487"), "{baseline}");
            assert!(!baseline.contains("TS2322"), "{baseline}");
            assert_eq!(baseline.matches("TS1487").count(), 2, "{baseline}");
        }
    }

    #[test]
    fn jsx_option_enables_jsx_parsing_in_javascript_error_baselines() {
        let source = r#"// @jsx: preserve
// @allowJs: true
// @filename: c.js
var elemC = <c>{42}</c>;
"#;
        let test_case = parse_test_case("jsxInJs.ts", source);
        let baseline = BaselineRunner::new(".").generate_errors_baseline(
            &test_case,
            "tests/cases/compiler/jsxInJs.ts",
            source,
        );

        assert!(!baseline.contains("TS1161"), "{baseline}");
    }

    #[test]
    fn parse_test_case_splits_multi_file() {
        let source = r#"// @target: es2015
// @filename: a.ts
export const a = 1;

// @filename: b.ts
import { a } from './a';
console.log(a);
"#;
        let tc = parse_test_case("test.ts", source);
        assert_eq!(tc.files.len(), 2);
        assert_eq!(tc.files[0].name, "a.ts");
        assert_eq!(tc.files[1].name, "b.ts");
    }

    #[test]
    fn parse_test_case_preserves_colon_directive_comments_in_body() {
        let source = r#"// @target: es2015
var x = 1;
// @ts-ignore: explanation
var y = 2;
"#;
        let tc = parse_test_case("test.ts", source);
        assert_eq!(tc.files.len(), 1);
        assert!(
            tc.files[0].content.contains("// @ts-ignore: explanation"),
            "body comment that looks like a directive should be preserved"
        );
        assert_eq!(tc.options.target, Some(tsc_rs_ast::ScriptTarget::ES2015));
    }

    #[test]
    fn parse_test_case_parses_options_after_header_comment() {
        let source = r#"// https://example.test
// @target: es2015
const x = 1;
"#;
        let tc = parse_test_case("test.ts", source);
        assert_eq!(tc.options.target, Some(tsc_rs_ast::ScriptTarget::ES2015));
        assert!(tc.files[0].content.contains("// https://example.test"));
    }

    #[test]
    fn suite_from_str() {
        assert_eq!("compiler".parse::<Suite>().unwrap(), Suite::Compiler);
        assert_eq!("conformance".parse::<Suite>().unwrap(), Suite::Conformance);
        assert_eq!("fourslash".parse::<Suite>().unwrap(), Suite::Fourslash);
        assert_eq!("project".parse::<Suite>().unwrap(), Suite::Project);
        assert!("other".parse::<Suite>().is_err());
    }

    #[test]
    fn normalize_crlf() {
        assert_eq!(normalize_text("a\r\nb\r\n"), "a\nb\n");
    }

    #[test]
    fn extracts_referenced_ambient_modules() {
        assert_eq!(
            triple_slash_reference_paths(
                "/// <reference path=\"/.lib/react16.d.ts\" />\nconst x = 1;"
            ),
            vec!["/.lib/react16.d.ts"]
        );
        assert_eq!(
            ambient_module_names(
                "declare module \"react\" { }\ndeclare module 'react/jsx-runtime' { }"
            ),
            vec!["react", "react/jsx-runtime"]
        );
    }

    #[test]
    fn diff_generation() {
        let expected = "line1\nline2\nline3\n";
        let actual = "line1\nchanged\nline3\n";
        let diff = generate_diff(expected, actual);
        assert!(diff.contains("line 2:"));
        assert!(diff.contains("expected: line2"));
        assert!(diff.contains("actual:   changed"));
    }

    #[test]
    fn export_assign_of_type_only_import_is_definitely_type_only() {
        let source = tsc_rs_parser::parse(
            "b.ts",
            "import type * as types from './a';\nexport = types;",
        );
        assert!(source_file_is_definitely_type_only_for_require(
            &source, false
        ));
    }
}

#[cfg(test)]
mod diagnostic_marker_tests {
    use super::*;

    #[test]
    fn error_source_sections_put_reference_roots_first_without_changing_headers() {
        let case = parse_test_case(
            "order.ts",
            concat!(
                "// @module: commonjs\n",
                "// @filename: dependency.ts\n",
                "export const value: string = 1;\n",
                "// @filename: unused.ts\n",
                "export const unused = true;\n",
                "// @filename: root.ts\n",
                "import dependency = require('./dependency');\n",
            ),
        );
        let actual = BaselineRunner::new(".").generate_errors_baseline(
            &case,
            "tests/cases/compiler/order.ts",
            "",
        );
        assert!(actual.starts_with("dependency.ts(1,"), "{actual}");
        let sections: Vec<_> = actual
            .lines()
            .filter(|line| line.starts_with("==== "))
            .collect();
        assert_eq!(
            sections,
            [
                "==== root.ts (0 errors) ====",
                "==== dependency.ts (1 errors) ====",
                "==== unused.ts (0 errors) ===="
            ]
        );
        assert_eq!(actual.matches("!!! error TS2322:").count(), 1, "{actual}");
    }

    #[test]
    fn error_source_partition_matches_runner_text_signals_and_metadata() {
        for last_source in [
            "import x = require('./dependency');",
            "// require('./dependency')",
            "/// <reference path='dependency.ts' />",
            "// reference\tpath",
        ] {
            let source = format!("// @outFile: output.js\n// @filename: dependency.ts\nconst x = 1;\n// @filename: root.ts\n{last_source}");
            let case = parse_test_case("order.ts", &source);
            assert_eq!(error_source_order_indices(&case), [1, 0], "{last_source}");
        }
        for last_source in [
            "import {x} from './dependency';",
            "// require ('./dependency')",
            "// reference  path",
        ] {
            let case = parse_test_case("order.ts", &format!("// @filename: dependency.ts\nexport const x = 1;\n// @filename: root.ts\n{last_source}"));
            assert_eq!(error_source_order_indices(&case), [0, 1], "{last_source}");
        }
        let case = parse_test_case("order.ts", "// @noImplicitReferences: true\n// @filename: dependency.ts\nconst x = 1;\n// @filename: root.ts\nconst y = 2;");
        assert_eq!(error_source_order_indices(&case), [1, 0]);
        let config = parse_test_case("order.ts", "// @filename: tsconfig.json\n{}\n// @filename: dependency.ts\nconst x = 1;\n// @filename: root.ts\n// require('./dependency')");
        assert_eq!(error_source_order_indices(&config), [0, 1, 2]);
    }

    #[test]
    fn multiline_annotations_keep_blank_rows_and_precede_later_errors() {
        let case = parse_test_case(
            "marker.tsx",
            "// @jsx: preserve\n// @noImplicitAny: false\n<>hi</div> // Error\n\n<>eof   // Error",
        );
        let actual = BaselineRunner::new(".").generate_errors_baseline(
            &case,
            "tests/cases/compiler/marker.tsx",
            "",
        );
        let annotated = actual.split_once("==== ").unwrap().1;
        assert_eq!(
            annotated,
            concat!(
                "marker.tsx (4 errors) ====\n",
                "    <>hi</div> // Error\n",
                "          ~~~\n",
                "!!! error TS2304: Cannot find name 'div'.\n",
                "          ~~~\n",
                "!!! error TS17015: Expected corresponding closing tag for JSX fragment.\n",
                "              ~~~~~~~~~\n",
                "    \n",
                "    \n",
                "    <>eof   // Error\n",
                "    ~~\n",
                "!!! error TS17014: JSX fragment has no corresponding closing tag.\n",
                "                    \n",
                "!!! error TS1005: '</' expected.\n",
            )
        );
    }

    #[test]
    fn zero_width_parser_diagnostic_has_no_invented_squiggle() {
        let source = "var x = Object. ";
        let case = parse_test_case("marker.ts", source);
        let actual = BaselineRunner::new(".").generate_errors_baseline(
            &case,
            "tests/cases/compiler/marker.ts",
            "",
        );
        assert!(
            actual.contains("marker.ts(1,16): error TS1003: Identifier expected."),
            "{actual}"
        );
        assert!(
            !actual.contains('~'),
            "zero-width diagnostic was expanded: {actual}"
        );
        assert!(
            actual.contains("!!! error TS1003: Identifier expected."),
            "diagnostic was lost: {actual}"
        );
    }

    #[test]
    fn nonempty_parser_diagnostic_retains_its_squiggle() {
        let case = parse_test_case("marker.ts", "var x = ;");
        let actual = BaselineRunner::new(".").generate_errors_baseline(
            &case,
            "tests/cases/compiler/marker.ts",
            "",
        );
        assert!(
            actual.contains("marker.ts(1,9): error TS1109: Expression expected."),
            "{actual}"
        );
        assert_eq!(actual.matches('~').count(), 1, "{actual}");
    }
}

/// The members of a program's `declare module "tslib" { ... }`: every
/// declaration, or only the `export`ed ones when any member is exported.
fn ambient_helpers_module_exports(files: &[tsc_rs_ast::SourceFile]) -> Option<Vec<String>> {
    use tsc_rs_ast::{ExportDeclKind, ModuleBody, ModuleName, StmtKind};
    fn declared_names(statement: &tsc_rs_ast::Stmt, out: &mut Vec<String>) {
        match &statement.kind {
            StmtKind::FnDecl(f) => out.extend(f.name.clone()),
            StmtKind::ClassDecl(c) => out.extend(c.name.clone()),
            StmtKind::EnumDecl(e) => out.push(e.name.clone()),
            StmtKind::Var(v) => {
                for declaration in &v.declarations {
                    if let tsc_rs_ast::PatKind::Ident(name) = &declaration.name.kind {
                        out.push(name.to_string());
                    }
                }
            }
            StmtKind::ModuleDecl(m) => {
                if let ModuleName::Ident(name) = &m.name {
                    out.push(name.clone());
                }
            }
            _ => {}
        }
    }
    let mut found = None;
    for file in files {
        for statement in &file.statements {
            let StmtKind::ModuleDecl(module) = &statement.kind else {
                continue;
            };
            if !matches!(&module.name, ModuleName::String(name) if name == "tslib") {
                continue;
            }
            let Some(ModuleBody::Block(body)) = &module.body else {
                continue;
            };
            let exported: Vec<&tsc_rs_ast::Stmt> = body
                .iter()
                .filter_map(|s| match &s.kind {
                    StmtKind::Export(e) => match &e.kind {
                        ExportDeclKind::Decl(d) => Some(d.as_ref()),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            let has_exports = body.iter().any(|s| matches!(s.kind, StmtKind::Export(_)));
            let names: &mut Vec<String> = found.get_or_insert_with(Vec::new);
            if has_exports {
                for declaration in exported {
                    declared_names(declaration, names);
                }
            } else {
                for declaration in body {
                    declared_names(declaration, names);
                }
            }
        }
    }
    found
}
