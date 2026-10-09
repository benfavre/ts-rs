//! Project graph + build orchestration contracts.
//!
//! Provides tsconfig.json parsing, file discovery (include/exclude/files),
//! extends inheritance, basic project references support, and incremental
//! compilation via `.tsbuildinfo` files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

pub use tsc_rs_resolver::{
    disable_and_clear_path_caches, enable_canonical_path_cache, path_caches_enabled,
};

static ONE_SHOT_PROCESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Declares that this process compiles once and exits (a CLI run, not
/// watch, pipe or LSP mode). Filesystem answers are then remembered
/// ([`enable_canonical_path_cache`]) and a compile leaves its ASTs, symbol
/// tables and checker state for the OS to reclaim at exit instead of
/// freeing them node by node (about a second on a 6,000-file project).
pub fn set_one_shot_process() {
    ONE_SHOT_PROCESS.store(true, std::sync::atomic::Ordering::Relaxed);
    enable_canonical_path_cache();
}

/// See [`set_one_shot_process`].
pub fn is_one_shot_process() -> bool {
    ONE_SHOT_PROCESS.load(std::sync::atomic::Ordering::Relaxed)
}
use tsc_rs_ast::{CompilerOptions, Diagnostic, DiagnosticCategory, SourceFile, StmtKind};
use tsc_rs_emitter::EmitOutput;
use tsc_rs_query::QueryEngine;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub root_names: Vec<String>,
}

pub trait ProjectSystem {
    fn parse_tsconfig(&self, config_path: &str) -> ProjectConfig;
    fn build_once(&self, config_path: &str) -> Vec<String>;
}

#[derive(Default)]
pub struct BootstrapProjectSystem;

impl ProjectSystem for BootstrapProjectSystem {
    fn parse_tsconfig(&self, _config_path: &str) -> ProjectConfig {
        ProjectConfig { root_names: vec![] }
    }

    fn build_once(&self, _config_path: &str) -> Vec<String> {
        vec!["bootstrap build".to_string()]
    }
}

/// Result of parsing a tsconfig.json file.
#[derive(Debug, Clone)]
pub struct ParsedCommandLine {
    pub options: CompilerOptions,
    pub file_names: Vec<String>,
    pub errors: Vec<Diagnostic>,
}

/// A reference to another project (from the `references` field).
#[derive(Debug, Clone)]
pub struct ProjectReference {
    pub path: String,
    pub prepend: bool,
}

// ---------------------------------------------------------------------------
// Multi-file compilation pipeline
// ---------------------------------------------------------------------------

/// Result of a single file's compilation.
#[derive(Debug, Clone)]
pub struct FileOutput {
    pub file_name: String,
    pub emit: EmitOutput,
    pub diagnostics: Vec<Diagnostic>,
}

/// Result of compiling an entire project.
#[derive(Debug, Clone)]
pub struct CompilationResult {
    pub files: Vec<FileOutput>,
    pub diagnostics: Vec<Diagnostic>,
}

impl CompilationResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.category == DiagnosticCategory::Error)
            || self.files.iter().any(|f| {
                f.diagnostics
                    .iter()
                    .any(|d| d.category == DiagnosticCategory::Error)
            })
    }

    pub fn total_diagnostics(&self) -> usize {
        self.diagnostics.len()
            + self
                .files
                .iter()
                .map(|f| f.diagnostics.len())
                .sum::<usize>()
    }
}

/// Result of compiling an entire project into a query engine.
pub struct QueryCompilationResult {
    pub engine: QueryEngine,
    pub diagnostics: Vec<Diagnostic>,
}

impl QueryCompilationResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.category == DiagnosticCategory::Error)
    }

    pub fn total_diagnostics(&self) -> usize {
        self.diagnostics.len()
    }
}

/// A TypeScript project that can parse, bind, check, and emit multiple files.
pub struct TsProject {
    pub options: CompilerOptions,
    pub file_names: Vec<String>,
}

impl TsProject {
    /// Create a project from a tsconfig.json path.
    pub fn from_config(config_path: &str) -> Result<Self, String> {
        let parsed = parse_tsconfig(config_path)?;
        Ok(Self {
            options: parsed.options,
            file_names: parsed.file_names,
        })
    }

    /// Create a project from explicit file names and options.
    pub fn new(file_names: Vec<String>, options: CompilerOptions) -> Self {
        Self {
            options,
            file_names,
        }
    }

    /// Run the full compilation pipeline: parse -> resolve -> bind -> link -> check -> emit.
    pub fn compile(&self) -> CompilationResult {
        self.compile_selected(&self.file_names)
    }

    /// Transpile-only mode: parse + emit without type checking.
    /// Produces JS output regardless of type errors, import resolution, etc.
    /// Use for fast JSX/TS → JS transforms where correctness is validated elsewhere.
    pub fn transpile_only(&self) -> CompilationResult {
        // Files are independent without type information: emit in parallel.
        let file_outputs: Vec<FileOutput> = self
            .file_names
            .par_iter()
            .map(|file_name| match std::fs::read_to_string(file_name) {
                Ok(source) => {
                    let sf = tsc_rs_parser::parse(file_name, &source);
                    let emit = tsc_rs_emitter::emit(&sf, &self.options);
                    FileOutput {
                        file_name: file_name.clone(),
                        emit,
                        // Skipping the TYPE checker does not make the file
                        // syntactically valid. The parser recovers from syntax
                        // errors so that the emitter can still produce output,
                        // but that output is corrupt — a stray backtick inside
                        // a template literal, for instance, silently truncates
                        // the string and invents an export. Dropping these
                        // diagnostics is what let that ship twice; carry them.
                        diagnostics: sf.diagnostics.clone(),
                    }
                }
                Err(e) => FileOutput {
                    file_name: file_name.clone(),
                    emit: tsc_rs_emitter::EmitOutput {
                        javascript: String::new(),
                        source_map: None,
                        declaration_file: None,
                        require_var_counters: Default::default(),
                        system_register_counter: 0,
                    },
                    diagnostics: vec![Diagnostic {
                        code: 6053,
                        message: format!("Cannot read file '{}': {}", file_name, e),
                        category: DiagnosticCategory::Error,
                        file_name: Some(file_name.clone()),
                        span: None,
                        related: None,
                    }],
                },
            })
            .collect();

        CompilationResult {
            files: file_outputs,
            diagnostics: Vec::new(),
        }
    }

    fn compile_selected(&self, selected_files: &[String]) -> CompilationResult {
        let timing = std::env::var("TSC_RS_INIT_TIMING").is_ok();
        let mut stage_start = std::time::Instant::now();
        let mut stage = |name: &str| {
            if timing {
                eprintln!(
                    "[check]  {name:<14} {:>7.1}ms",
                    stage_start.elapsed().as_secs_f64() * 1000.0
                );
            }
            stage_start = std::time::Instant::now();
        };
        // Parse the full project graph in parallel so that selected outputs
        // still type-check against unchanged dependencies. Read+parse is
        // independent per file and dominates wall time on large projects.
        // `par_iter` keeps Vec order stable so downstream indexing matches.
        let parse_results: Vec<Result<SourceFile, (String, std::io::Error)>> = self
            .file_names
            .par_iter()
            .map(|file_name| match std::fs::read_to_string(file_name) {
                Ok(source) => Ok(tsc_rs_parser::parse(file_name, &source)),
                Err(e) => Err((file_name.clone(), e)),
            })
            .collect();

        let mut parsed_files: Vec<SourceFile> = Vec::with_capacity(parse_results.len());
        let mut project_diagnostics = Vec::new();
        for r in parse_results {
            match r {
                Ok(sf) => parsed_files.push(sf),
                Err((file_name, e)) => {
                    project_diagnostics.push(Diagnostic {
                        code: 6053,
                        message: format!("Cannot read file '{}': {}", file_name, e),
                        category: DiagnosticCategory::Error,
                        file_name: Some(file_name),
                        span: None,
                        related: None,
                    });
                }
            }
        }

        for lib in tsc_rs_types::load_stdlib_sources(&self.options) {
            parsed_files.push(tsc_rs_parser::parse(&lib.file_name, &lib.source));
        }

        // Phase 1.5: Autodiscovery of `.d.ts` files reachable via imports.
        stage("read + parse");
        let listed_file_count = parsed_files.len();
        autodiscover_dts(&mut parsed_files, &self.options);
        stage("autodiscover");

        // Phase 2: Bind all files in parallel — bind is referentially
        // transparent per SourceFile and the resulting SymbolTables are
        // independent.
        let mut symbol_tables: Vec<tsc_rs_symbols::SymbolTable> =
            parsed_files.par_iter().map(tsc_rs_symbols::bind).collect();

        // Phase 3: Resolve imports and link cross-file symbols
        // Build a map from file path -> index for lookup
        stage("bind");
        let file_index: HashMap<String, usize> = parsed_files
            .iter()
            .enumerate()
            .map(|(i, sf)| (sf.file_name.clone(), i))
            .collect();
        let mut available_file_names: HashSet<String> =
            parsed_files.iter().map(|sf| sf.file_name.clone()).collect();

        for file_idx in 0..parsed_files.len() {
            let imports = collect_imports(&parsed_files[file_idx]);
            for import in &imports {
                // Resolve the import to a file path
                let resolved = tsc_rs_resolver::resolve_module_name(
                    &import.source,
                    &parsed_files[file_idx].file_name,
                    &self.options,
                );

                if let Some(resolved_module) = resolved {
                    available_file_names.insert(resolved_module.resolved_file_name.clone());
                    // Find the target file in our project
                    if let Some(&target_idx) = file_index.get(&resolved_module.resolved_file_name) {
                        if target_idx != file_idx {
                            // Borrow the two tables disjointly: cloning the
                            // target's whole table per import edge copied a
                            // heavily imported file's symbols once per importer.
                            let (importer, target) = if file_idx < target_idx {
                                let (head, tail) = symbol_tables.split_at_mut(target_idx);
                                (&mut head[file_idx], &tail[0])
                            } else {
                                let (head, tail) = symbol_tables.split_at_mut(file_idx);
                                (&mut tail[0], &head[target_idx])
                            };
                            tsc_rs_symbols::link_imports(importer, target, import);
                        }
                    }
                }
            }
        }

        stage("link imports");
        let mut available_file_names: Vec<String> = available_file_names.into_iter().collect();
        available_file_names.sort();
        let all_parsed_refs: Vec<&SourceFile> = parsed_files.iter().collect();
        let mut selected: HashSet<&str> = selected_files.iter().map(String::as_str).collect();
        // A full compile also checks the sources that imports pulled into
        // the program (tsc checks every non-declaration file of a program,
        // not only the tsconfig's own files).
        if selected_files.len() == self.file_names.len() {
            selected.extend(
                parsed_files[listed_file_count.min(parsed_files.len())..]
                    .iter()
                    .map(|sf| sf.file_name.as_str())
                    .filter(|name| is_workspace_ts_source(name)),
            );
        }

        // Pre-build a "donor" TypeChecker with the project-wide cross-file
        // type tables (class_info, interface_info, type_aliases, enum_info,
        // global decls in scope[0]) populated ONCE. Each per-file check then
        // clones this donor instead of re-walking every other file's AST.
        //
        // Without this, `inject_external_types(&other_files)` is called N
        // times per project, each pass walking N-1 file ASTs and rebuilding
        // the same interface/class/enum/type-alias tables → O(N^2) total
        // CPU. On a 5 700-file Next.js codebase that was ~19 minutes of
        // user CPU even after rayon parallelism.
        //
        // Injecting the current file into the donor is fine: the per-file
        // check re-runs the file's decls normally and `inject_stmt` is
        // idempotent (it bails on duplicate keys), so the local scope ends
        // up identical to the per-file inject case. TypeChecker arena and
        // dedup_map are cloned together, so all interned TypeIds remain
        // valid in the cloned target.
        // Project-wide "donor" TypeChecker, built ONCE on the main thread
        // with all cross-file type tables (class_info, interface_info,
        // enum_info, type_aliases, global scope) populated. Each per-file
        // checker clones the donor in parallel — TypeChecker is now `Sync`
        // (atomic recursion counter + Mutex cycle-detection set), so the
        // shared `&donor` reference travels safely across rayon workers
        // and each fork operates on its own owned copy.
        //
        // Without this, `inject_external_types(&other_files)` walked every
        // other file's AST once per file → O(N²) project-wide. On a 5 700-
        // file Next.js codebase that was ~19 minutes of user CPU even
        // after rayon parallelism. Sharing the donor turns the redundant
        // AST traversals into cheap HashMap+Vec memcopies.
        //
        // Injecting the file being checked is harmless: `inject_stmt` is
        // idempotent on top-level decls (skips when the key is already
        // present), so the local scope ends up identical to the per-file
        // inject case after the file's check pass re-runs its own decls.
        let donor_start = std::time::Instant::now();
        let donor: Option<tsc_rs_types::TypeChecker> = if all_parsed_refs.is_empty() {
            None
        } else {
            let mut d = tsc_rs_types::TypeChecker::new();
            d.enable_module_resolution_diagnostics();
            // CLI / compile_selected never reads back `expression_types`
            // (FileOutput drops it). Skipping the per-expression
            // `display_string()` saves the ~3.5% of CPU profile spent
            // formatting type names that nothing consumes.
            d.disable_expression_types();
            d.mark_donor();
            d.inject_external_types(&all_parsed_refs);
            if let Ok(names) = std::env::var("TSC_RS_WHO_DECLARES") {
                for name in names.split(',') {
                    eprintln!("[declares] {}", d.describe_type_name(name.trim()));
                }
            }
            d.register_available_files(&available_file_names);
            // Drop project-wide inject diagnostics so they don't get
            // duplicated under every per-file check.
            d.take_diagnostics();
            Some(d)
        };

        if timing {
            eprintln!(
                "[check]  donor build    {:>7.1}ms",
                donor_start.elapsed().as_secs_f64() * 1000.0
            );
        }
        // Summed over worker threads (CPU time, not wall).
        let slow_checks: std::sync::Mutex<Vec<(u64, String)>> = std::sync::Mutex::new(Vec::new());
        let clone_nanos = std::sync::atomic::AtomicU64::new(0);
        let check_nanos = std::sync::atomic::AtomicU64::new(0);
        let files_start = std::time::Instant::now();

        // Pre-materialize the selected (index, sf) pairs into a Vec so
        // that par_iter exposes an IndexedParallelIterator and `.collect()`
        // preserves the original `parsed_files` order.
        let selected_indices: Vec<(usize, &SourceFile)> = parsed_files
            .iter()
            .enumerate()
            .filter(|(_, sf)| selected.contains(sf.file_name.as_str()))
            .collect();

        let file_outputs: Vec<FileOutput> = selected_indices
            .par_iter()
            .map(|&(i, sf)| {
                // Under skipLibCheck tsc reports a declaration file's syntax
                // errors but neither its semantic nor its checker grammar
                // errors (TS1046, …), and never runs the checker on it.
                let skip_check = self.options.skip_lib_check == Some(true)
                    && is_declaration_file_name(&sf.file_name);
                let semantic_diagnostics = if skip_check {
                    Vec::new()
                } else {
                    let clone_start = std::time::Instant::now();
                    let mut checker = match &donor {
                        Some(d) => d.clone(),
                        None => tsc_rs_types::TypeChecker::new(),
                    };
                    let check_start = std::time::Instant::now();
                    checker.set_current_file_name(&sf.file_name);
                    let diagnostics = checker
                        .check_with_options(sf, &symbol_tables[i], &self.options)
                        .diagnostics;
                    if timing {
                        use std::sync::atomic::Ordering::Relaxed;
                        clone_nanos
                            .fetch_add((check_start - clone_start).as_nanos() as u64, Relaxed);
                        let spent = check_start.elapsed();
                        check_nanos.fetch_add(spent.as_nanos() as u64, Relaxed);
                        if spent.as_millis() >= 250 {
                            slow_checks
                                .lock()
                                .expect("slow check list poisoned")
                                .push((spent.as_millis() as u64, sf.file_name.clone()));
                        }
                    }
                    diagnostics
                };

                // Skip JS / source-map / .d.ts emission in `--noEmit` mode.
                // The emit pass is non-trivial (a full AST walk producing a
                // string) and its output is dropped immediately by
                // `compile_and_emit`'s `if !no_emit { write_outputs(...) }`
                // gate. On apps/app that's ~5 700 wasted emit passes.
                let emit = if self.options.no_emit.unwrap_or(false) {
                    tsc_rs_emitter::EmitOutput {
                        javascript: String::new(),
                        source_map: None,
                        declaration_file: None,
                        require_var_counters: HashMap::new(),
                        system_register_counter: 0,
                    }
                } else {
                    tsc_rs_emitter::emit(sf, &self.options)
                };
                FileOutput {
                    file_name: sf.file_name.clone(),
                    emit,
                    diagnostics: dedup_diagnostics(
                        sf.diagnostics
                            .iter()
                            .filter(|diag| {
                                !(skip_check && tsc_rs_parser::is_grammar_diagnostic(diag.code))
                            })
                            .cloned()
                            .chain(semantic_diagnostics)
                            .collect(),
                        &sf.file_name,
                    ),
                }
            })
            .collect();

        if timing {
            use std::sync::atomic::Ordering::Relaxed;
            eprintln!(
                "[check]  per-file wall  {:>7.1}ms; cpu: donor clones {:.1}s, check+drop {:.1}s",
                files_start.elapsed().as_secs_f64() * 1000.0,
                clone_nanos.load(Relaxed) as f64 / 1e9,
                check_nanos.load(Relaxed) as f64 / 1e9,
            );
        }
        if timing {
            let mut slow = slow_checks.into_inner().expect("slow check list poisoned");
            slow.sort_by(|a, b| b.0.cmp(&a.0));
            for (ms, name) in slow.iter().take(8) {
                eprintln!("[check]    slow file {ms:>6}ms {name}");
            }
        }
        let teardown_start = std::time::Instant::now();
        if is_one_shot_process() {
            std::mem::forget((donor, symbol_tables, parsed_files));
        } else {
            drop(donor);
            drop(symbol_tables);
            drop(parsed_files);
        }
        if timing {
            eprintln!(
                "[check]  teardown       {:>7.1}ms",
                teardown_start.elapsed().as_secs_f64() * 1000.0
            );
        }

        CompilationResult {
            files: file_outputs,
            diagnostics: project_diagnostics,
        }
    }

    /// Build a [`QueryEngine`] for the full project.
    ///
    /// Unlike `compile()`, this keeps queryable analysis state available even
    /// when diagnostics are present.
    pub fn compile_query(&self) -> QueryCompilationResult {
        let mut engine = QueryEngine::with_options(self.options.clone());
        engine.set_one_shot(true);
        let mut project_diagnostics = Vec::new();

        for file_name in &self.file_names {
            match std::fs::read_to_string(file_name) {
                Ok(source) => {
                    if let Err(err) = engine.add_source(file_name.clone(), source) {
                        project_diagnostics.push(Diagnostic {
                            code: 6054,
                            message: format!("Cannot add source '{}': {}", file_name, err),
                            category: DiagnosticCategory::Error,
                            file_name: Some(file_name.clone()),
                            span: None,
                            related: None,
                        });
                    }
                }
                Err(e) => {
                    project_diagnostics.push(Diagnostic {
                        code: 6053,
                        message: format!("Cannot read file '{}': {}", file_name, e),
                        category: DiagnosticCategory::Error,
                        file_name: Some(file_name.clone()),
                        span: None,
                        related: None,
                    });
                }
            }
        }

        let mut diagnostics = match engine.check_all() {
            Ok(()) => engine.raw_diagnostics().to_vec(),
            Err(diags) => diags,
        };
        diagnostics.extend(project_diagnostics);

        QueryCompilationResult {
            engine,
            diagnostics,
        }
    }

    /// Run incremental compilation: only recompile files whose content hash
    /// has changed since the last build (tracked via `.tsbuildinfo`).
    /// Returns the compilation result and the updated build info.
    pub fn compile_incremental(&self, build_info_path: &str) -> (CompilationResult, TsBuildInfo) {
        let previous = TsBuildInfo::load(build_info_path);
        let options_hash = hash_compiler_options(&self.options);

        // If options changed, do a full rebuild
        let options_changed = previous
            .as_ref()
            .is_none_or(|prev| prev.options_hash != options_hash);

        // Compute current file hashes
        let mut current_hashes: HashMap<String, String> = HashMap::new();
        for file_name in &self.file_names {
            if let Ok(source) = std::fs::read_to_string(file_name) {
                current_hashes.insert(file_name.clone(), simple_hash(&source));
            }
        }

        // Determine which files need recompilation
        let files_to_compile: Vec<String> = if options_changed {
            self.file_names.clone()
        } else if let Some(ref prev) = previous {
            self.file_names
                .iter()
                .filter(|f| {
                    let current = current_hashes.get(*f);
                    let previous_hash = prev.file_hashes.get(*f);
                    // Recompile if: new file, hash changed, or any dependency changed
                    match (current, previous_hash) {
                        (Some(c), Some(p)) => {
                            if c != p {
                                return true;
                            }
                            // Check if any dependency changed
                            if let Some(deps) = prev.dependencies.get(*f) {
                                for dep in deps {
                                    let dep_current = current_hashes.get(dep);
                                    let dep_prev = prev.file_hashes.get(dep);
                                    if dep_current != dep_prev {
                                        return true;
                                    }
                                }
                            }
                            false
                        }
                        _ => true,
                    }
                })
                .cloned()
                .collect()
        } else {
            self.file_names.clone()
        };

        let result = self.compile_selected(&files_to_compile);

        // Collect dependencies from imports
        let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
        for file_name in &self.file_names {
            if let Ok(source) = std::fs::read_to_string(file_name) {
                let sf = tsc_rs_parser::parse(file_name, &source);
                let imports = collect_imports(&sf);
                let deps: Vec<String> = imports
                    .iter()
                    .filter_map(|import| {
                        let resolved = tsc_rs_resolver::resolve_module_name(
                            &import.source,
                            file_name,
                            &self.options,
                        );
                        resolved.map(|r| r.resolved_file_name)
                    })
                    .collect();
                if !deps.is_empty() {
                    dependencies.insert(file_name.clone(), deps);
                }
            }
        }

        let build_info = TsBuildInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            file_hashes: current_hashes,
            options_hash,
            dependencies,
        };

        (result, build_info)
    }

    /// Build a long-lived [`CheckSession`] for use by editor / agent
    /// integrations (the `--check-pipe` CLI mode). Parses + binds every
    /// project file once, runs `inject_external_types` once into a
    /// "donor" TypeChecker, then keeps the whole graph in memory.
    /// Subsequent `CheckSession::check_file` calls clone the donor and
    /// run only that file's check pass — typically tens of milliseconds.
    pub fn open_check_session(&self) -> CheckSession {
        // Phase timing for the cold init (`TSC_RS_INIT_TIMING=1`). The daemon
        // pays this once per tsconfig; on apps/app (7.5k files) it dominates, so
        // knowing the split — parse / autodiscover / bind / link / donor — is
        // how any speedup gets prioritised. `Instant::now()` is unavailable in
        // some sandboxes; fall back to no timing if it panics is not needed —
        // it is a normal std call here.
        let timing = std::env::var("TSC_RS_INIT_TIMING").is_ok();
        let t0 = std::time::Instant::now();
        macro_rules! phase {
            ($label:expr, $prev:expr) => {{
                if timing {
                    let now = std::time::Instant::now();
                    eprintln!(
                        "[init] {:<14} {:>7.1}ms  rss {} MB",
                        $label,
                        now.duration_since($prev).as_secs_f64() * 1000.0,
                        resident_mb()
                    );
                    now
                } else {
                    $prev
                }
            }};
        }

        // Parse the full project graph in parallel.
        let parse_results: Vec<Result<SourceFile, (String, std::io::Error)>> = self
            .file_names
            .par_iter()
            .map(|file_name| match std::fs::read_to_string(file_name) {
                Ok(source) => Ok(tsc_rs_parser::parse(file_name, &source)),
                Err(e) => Err((file_name.clone(), e)),
            })
            .collect();
        let t_parse = phase!("parse", t0);

        let mut parsed_files: Vec<SourceFile> = Vec::with_capacity(parse_results.len());
        let mut init_diagnostics = Vec::new();
        for r in parse_results {
            match r {
                Ok(sf) => parsed_files.push(sf),
                Err((file_name, e)) => {
                    init_diagnostics.push(Diagnostic {
                        code: 6053,
                        message: format!("Cannot read file '{}': {}", file_name, e),
                        category: DiagnosticCategory::Error,
                        file_name: Some(file_name),
                        span: None,
                        related: None,
                    });
                }
            }
        }

        // Append stdlib so the donor can intern its types into class_info /
        // interface_info exactly as compile_selected does.
        let stdlib_start = parsed_files.len();
        for lib in tsc_rs_types::load_stdlib_sources(&self.options) {
            parsed_files.push(tsc_rs_parser::parse(&lib.file_name, &lib.source));
        }
        let t_stdlib = phase!("stdlib", t_parse);

        // Autodiscovery — see `autodiscover_dts` for rationale. This is the
        // daemon-mode equivalent of `compile_selected`'s Phase 1.5: follow
        // imports / re-exports to pull cross-package `.d.ts` files into
        // the project graph once, so per-file checks see the full type
        // universe via the donor.
        autodiscover_dts(&mut parsed_files, &self.options);
        let t_autodisc = phase!("autodiscover", t_stdlib);

        let symbol_tables: Vec<tsc_rs_symbols::SymbolTable> =
            parsed_files.par_iter().map(tsc_rs_symbols::bind).collect();
        let t_bind = phase!("bind", t_autodisc);

        // Resolve cross-file imports for the original (non-stdlib) files so
        // the symbol tables have the right per-file cross-imports linked.
        let file_index: HashMap<String, usize> = parsed_files
            .iter()
            .enumerate()
            .map(|(i, sf)| (sf.file_name.clone(), i))
            .collect();
        let mut available_set: HashSet<String> =
            parsed_files.iter().map(|sf| sf.file_name.clone()).collect();

        let mut symbol_tables = symbol_tables;
        for file_idx in 0..stdlib_start {
            let imports = collect_imports(&parsed_files[file_idx]);
            for import in &imports {
                let resolved = tsc_rs_resolver::resolve_module_name(
                    &import.source,
                    &parsed_files[file_idx].file_name,
                    &self.options,
                );
                if let Some(resolved_module) = resolved {
                    available_set.insert(resolved_module.resolved_file_name.clone());
                    if let Some(&target_idx) = file_index.get(&resolved_module.resolved_file_name) {
                        if target_idx != file_idx {
                            // Borrow the two tables disjointly: cloning the
                            // target's whole table per import edge copied a
                            // heavily imported file's symbols once per importer.
                            let (importer, target) = if file_idx < target_idx {
                                let (head, tail) = symbol_tables.split_at_mut(target_idx);
                                (&mut head[file_idx], &tail[0])
                            } else {
                                let (head, tail) = symbol_tables.split_at_mut(file_idx);
                                (&mut tail[0], &head[target_idx])
                            };
                            tsc_rs_symbols::link_imports(importer, target, import);
                        }
                    }
                }
            }
        }

        let mut available_file_names: Vec<String> = available_set.into_iter().collect();
        available_file_names.sort();
        // Linked; from here on requests only link against the tables (the
        // donor is built from the ASTs), so drop everything else in them
        // before the donor adds its own peak.
        symbol_tables
            .par_iter_mut()
            .for_each(tsc_rs_symbols::SymbolTable::retain_link_surface);
        let t_link = phase!("link", t_bind);

        // Build the project-wide donor TypeChecker. Same recipe as
        // compile_selected: enable_module_resolution_diagnostics +
        // disable_expression_types + inject_external_types + register
        // available files. Drop the inject pass's project-wide diagnostics
        // — they would otherwise be reported under every per-file check.
        let all_parsed_refs: Vec<&SourceFile> = parsed_files.iter().collect();
        let mut donor = tsc_rs_types::TypeChecker::new();
        donor.enable_module_resolution_diagnostics();
        donor.disable_expression_types();
        donor.mark_donor();
        donor.inject_external_types(&all_parsed_refs);
        donor.register_available_files(&available_file_names);
        donor.take_diagnostics();
        let _ = phase!("donor", t_link);
        if timing {
            eprintln!(
                "[init] {:<14} {:>7.1}ms  ({} files)",
                "TOTAL",
                t0.elapsed().as_secs_f64() * 1000.0,
                parsed_files.len()
            );
        }

        // Requests need the donor, the symbol tables and the file index;
        // the ASTs (most of the session's memory) and the available-file
        // list were only inputs to the donor, which keeps its own copies.
        let project_file_count = parsed_files
            .iter()
            .filter(|sf| !sf.file_name.starts_with("__lib"))
            .count();
        drop(all_parsed_refs);
        drop(parsed_files);
        drop(available_file_names);

        CheckSession {
            options: self.options.clone(),
            project_file_count,
            symbol_tables,
            file_index,
            donor,
            init_diagnostics,
        }
    }
}

/// Resident memory in MB (Linux `/proc/self/statm`; 0 elsewhere), for the
/// init timing lines.
fn resident_mb() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map_or(0, |pages| pages * 4096 / (1024 * 1024))
}

/// Long-lived state for the `--check-pipe` daemon (and similar editor
/// integrations). Holds the parsed + bound project graph plus a donor
/// TypeChecker pre-populated with cross-file type info. Per-file
/// rechecks fork the donor and reuse the graph, so each request is
/// ~tens of ms instead of a 30 s full-project recompile.
pub struct CheckSession {
    options: CompilerOptions,
    project_file_count: usize,
    symbol_tables: Vec<tsc_rs_symbols::SymbolTable>,
    file_index: HashMap<String, usize>,
    donor: tsc_rs_types::TypeChecker,
    init_diagnostics: Vec<Diagnostic>,
}

impl CheckSession {
    /// Number of project files (excluding stdlib stubs) loaded.
    pub fn project_file_count(&self) -> usize {
        self.project_file_count
    }

    /// Diagnostics produced during the initial parse pass (file-read
    /// errors etc.).
    pub fn init_diagnostics(&self) -> &[Diagnostic] {
        &self.init_diagnostics
    }

    /// Re-check a single file against the cached project donor.
    ///
    /// `source` is the contents to check. If `None`, the file is read
    /// from disk. The file does not need to have been part of the
    /// original project; if it is, its cached parse/bind are replaced
    /// for this check (and only this check — subsequent calls re-read
    /// unless `source` is supplied again).
    ///
    /// STALENESS: the donor's cross-file type tables reflect the file
    /// contents at `open_check_session` time. Edits to OTHER files
    /// since then are not visible to this check. For most "did I break
    /// this file?" agent workflows that's the right trade-off; for
    /// "did changing file A break file B?", call `check_file` against
    /// both files (or rebuild the session).
    pub fn check_file(&self, file_name: &str, source: Option<&str>) -> Vec<Diagnostic> {
        // Resolve the new (or existing) source.
        let owned_source: std::string::String;
        let src: &str = match source {
            Some(s) => s,
            None => match std::fs::read_to_string(file_name) {
                Ok(s) => {
                    owned_source = s;
                    &owned_source
                }
                Err(e) => {
                    return vec![Diagnostic {
                        code: 6053,
                        message: format!("Cannot read file '{}': {}", file_name, e),
                        category: DiagnosticCategory::Error,
                        file_name: Some(file_name.to_string()),
                        span: None,
                        related: None,
                    }];
                }
            },
        };

        // Parse + bind the (possibly new) source.
        let sf = tsc_rs_parser::parse(file_name, src);
        let mut symbols = tsc_rs_symbols::bind(&sf);

        // Link cross-file imports against the cached project graph.
        let imports = collect_imports(&sf);
        for import in &imports {
            let resolved =
                tsc_rs_resolver::resolve_module_name(&import.source, &sf.file_name, &self.options);
            if let Some(resolved_module) = resolved {
                if let Some(&target_idx) = self.file_index.get(&resolved_module.resolved_file_name)
                {
                    tsc_rs_symbols::link_imports(
                        &mut symbols,
                        &self.symbol_tables[target_idx],
                        import,
                    );
                }
            }
        }

        // Fork the donor and run the per-file check.
        let mut checker = self.donor.clone();
        checker.set_current_file_name(&sf.file_name);
        let check_output = checker.check_with_options(&sf, &symbols, &self.options);
        let raw: Vec<Diagnostic> = sf
            .diagnostics
            .iter()
            .cloned()
            .chain(check_output.diagnostics)
            .map(|mut d| {
                if d.file_name.is_none() {
                    d.file_name = Some(sf.file_name.clone());
                }
                d
            })
            .collect();
        // Dedup by (file, span.start, span.end, code, message).
        // Real-world apps/app emitted the same chained-call diagnostic
        // 16× — the checker re-walks subexpressions and the error fires
        // for each re-walk. Per-file output is the right deduplication
        // layer; the harness's downstream consumers see one error per
        // distinct location.
        let mut seen: std::collections::HashSet<(String, u32, u32, u32, String)> =
            std::collections::HashSet::new();
        raw.into_iter()
            .filter(|d| {
                let key = (
                    d.file_name.clone().unwrap_or_default(),
                    d.span.map(|s| s.start).unwrap_or(0),
                    d.span.map(|s| s.end).unwrap_or(0),
                    d.code,
                    d.message.clone(),
                );
                seen.insert(key)
            })
            .collect()
    }

    /// Re-check several files in parallel against the cached donor.
    pub fn check_files(
        &self,
        files: &[(String, Option<std::string::String>)],
    ) -> Vec<(String, Vec<Diagnostic>)> {
        files
            .par_iter()
            .map(|(name, src)| {
                let diags = self.check_file(name, src.as_deref());
                (name.clone(), diags)
            })
            .collect()
    }
}

/// Collect all import declarations from a source file.
/// Dedup diagnostics by (file, start, end, code, message). The checker
/// re-walks expression subtrees during chained calls (`a.b().c().d()`)
/// and the same diagnostic ends up emitted once per visit. Keep the
/// first occurrence per location/code/message pair so downstream
/// consumers see one error per real location.
fn dedup_diagnostics(diags: Vec<Diagnostic>, file_name: &str) -> Vec<Diagnostic> {
    let mut seen: std::collections::HashSet<(String, u32, u32, u32, String)> =
        std::collections::HashSet::new();
    diags
        .into_iter()
        .filter(|d| {
            let fname = d.file_name.clone().unwrap_or_else(|| file_name.to_string());
            let key = (
                fname,
                d.span.map(|s| s.start).unwrap_or(0),
                d.span.map(|s| s.end).unwrap_or(0),
                d.code,
                d.message.clone(),
            );
            seen.insert(key)
        })
        .collect()
}

fn collect_imports(file: &SourceFile) -> Vec<tsc_rs_ast::ImportDecl> {
    let mut imports = Vec::new();
    for stmt in &file.statements {
        if let StmtKind::Import(import_decl) = &stmt.kind {
            imports.push((**import_decl).clone());
        }
    }
    imports
}

/// Autodiscovery feature gate. Returns true when `TSC_RS_AUTODISCOVER=1`
/// (or `=true`) is set in the environment.
///
/// Default OFF until perf + correctness are tuned for real codebases.
/// Empirically on a 5 810-file Next.js + Prisma monorepo:
///   * uncapped autodiscovery pulled in ~6 000 transitive `.d.ts`,
///     ballooned RSS past 6 GB, and tripped both the daemon's
///     `TSC_RS_PIPE_MAX_MEMORY_MB=2048` ceiling and (on respawn) a
///     stack overflow inside the type checker — likely cycles in
///     Prisma's recursive generated typings.
///   * Even with a 500 KB per-file cap, the result was 11 769 parsed
///     files and ~7 GB RSS.
/// Pulling in cross-package `.d.ts` files also surfaces ts-rs's
/// generic-inference gaps: `useState({...})` no longer falls back to
/// `Type::Any` and trips ~1 800 phantom TS2353 errors because the
/// `useState<S>(initialState: S | (() => S))` type parameter isn't
/// inferred from the object literal.
///
/// The infrastructure ships so callers can opt in (typically narrower
/// projects where the type universe is manageable) while we iterate
/// on the type-checker side: cycle-safe inheritance walks, generic
/// inference for arrow / inline-literal arguments, and a slim Prisma
/// model adapter that skips the multi-megabyte query permutation
/// types.
/// Autodiscovery is **on by default** as of 2026-05-15.
///
/// History: was opt-in via `TSC_RS_AUTODISCOVER=1` because an early
/// implementation OOMed on apps/app (uncapped expansion hit 11 769 parsed
/// files / ~7 GB RSS). The cross-package follow gate
/// (`autodiscover_should_follow`) plus the type-checker correctness
/// work since then (default-unsolved-to-never, contextual literal
/// narrowing, spread-tombstone leniency, etc.) closed those holes.
///
/// You CAN still opt out via `TSC_RS_AUTODISCOVER=0` / `=false` if a
/// project trips a regression we haven't caught yet. The default-on
/// behaviour is what makes `import { z } from "zod"` resolve to a
/// real ZodObject instead of `any` for packages whose tsconfig
/// `include` glob doesn't cover node_modules — which is essentially
/// every package outside `apps/app`.
fn autodiscover_enabled() -> bool {
    match std::env::var("TSC_RS_AUTODISCOVER").as_deref() {
        Ok("0") | Ok("false") | Ok("FALSE") | Ok("no") => false,
        _ => true,
    }
}

/// Maximum per-file size (in bytes) parsed during `.d.ts` autodiscovery.
///
/// Set generously by default to handle realistic package typings (Next.js,
/// React, Prisma's `internal/class.d.ts` ~480 KB) while still skipping
/// the multi-megabyte generated giants that Prisma emits per model
/// (`tenant.d.ts` ~14 MB, `user.d.ts` ~7 MB, `commonInputTypes.d.ts`
/// ~1.8 MB).
///
/// Override via `TSC_RS_AUTODISCOVER_MAX_FILE_BYTES`; set to 0 to
/// disable the size cap entirely (parses everything reachable).
fn autodiscover_max_bytes() -> u64 {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_FILE_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        // A one-shot run builds the whole program, as tsc does; the caps
        // bound the memory of long-lived sessions.
        .unwrap_or(if is_one_shot_process() { 0 } else { 500_000 })
}

/// Maximum number of files added by autodiscovery. Defaults to 1500;
/// override via `TSC_RS_AUTODISCOVER_MAX_FILES`.
///
/// 1500 is enough for apps/app's typical fanout (zod + react + next +
/// prisma + ~25 workspace packages = ~600–900 unique `.d.ts`) with
/// headroom, while keeping the daemon under the 2 GB RSS ceiling.
/// A lower cap (e.g. 200) is the recommended floor for narrow packages
/// like `packages/api-routers` that only need zod + a few utilities.
fn autodiscover_max_files() -> usize {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_FILES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if is_one_shot_process() {
            usize::MAX
        } else {
            1500
        })
}

/// Maximum BFS rounds. Limits how deep autodiscovery follows
/// transitive imports starting from the workspace files. Round 0 is
/// direct imports from workspace; each subsequent round expands one
/// level. Defaults to 6 — zod v4's user-facing surface needs ~6 rounds
/// (`schemas.ts → zod/v4 → classic → core → core/checks → …`); most
/// other packages converge in 2–4. The per-package cap stops any
/// single package from monopolizing the budget at the larger depth.
///
/// Override via `TSC_RS_AUTODISCOVER_MAX_ROUNDS`.
fn autodiscover_max_rounds() -> usize {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if is_one_shot_process() { 1000 } else { 6 })
}

/// Maximum files to autodiscover per node_modules package. Without this
/// cap, deep typing libraries (Prisma's generated `Prisma*Args`,
/// `@types/node`'s 200+ files, vite's rollup type chain) explode the
/// per-package count and the overall total even though most of those
/// files are never directly relevant to user code.
///
/// 250 fits zod (210 .d.cts files for the v4 core + classic + mini
/// runtime), Prisma's generated client surface, and most other
/// schema-style libraries without dragging in `@types/node`'s entire
/// Node-builtin tree (also 200+ but typically only `path`/`fs`/`url`
/// are used). Combined with `max_files`, a single package still can't
/// monopolize the budget.
///
/// Override via `TSC_RS_AUTODISCOVER_MAX_PER_PACKAGE`. Set to 0 to
/// disable the per-package cap.
fn autodiscover_max_per_package() -> usize {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_PER_PACKAGE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if is_one_shot_process() { 0 } else { 250 })
}

/// Autodiscover `.d.ts` files reachable via imports + `export … from "…"`
/// from the tsconfig-included set.
///
/// Monorepo apps typically scope their tsconfig include to their own
/// sources. Cross-package imports like `import { db } from
/// "@acme/db"` then resolve to a path (via tsconfig path
/// mapping or node_modules) whose target `.d.ts` was never in the
/// initial `parsed_files` set — so project-wide type tables don't know
/// `PrismaClient` and member access leaks. Following imports
/// transitively closes the gap.
///
/// Bounded breadth-first traversal in parallel rounds. Each round
/// resolves every fresh file's imports / re-exports and parses the
/// unique `.d.ts` targets that aren't already in the set. The frontier
/// shrinks naturally; 3-6 rounds are typical for Prisma + Next +
/// monorepo packages. The 32-round cap is a guard against pathological
/// cycles in upstream typings.
///
/// Only `.d.ts` is followed (pulling in transitively-imported `.ts`
/// source from node_modules would explode the graph), and files larger
/// than [`autodiscover_max_bytes`] are skipped (see the rationale on
/// that function). Skipped files have their TypeReference resolved to
/// `Type::Any` at use sites — strictly better than the pre-fix state
/// where the missing type would silently fall back to whatever
/// same-named local declaration polluted the global scope first.
/// Returns the canonical "package root" for a path inside `node_modules`,
/// or `None` for paths outside `node_modules` (which are workspace-local
/// and never need package scoping).
///
/// Examples:
///   `/x/node_modules/preact/hooks/src/index.d.ts` → `Some("preact")`
///   `/x/node_modules/@types/react/index.d.ts`      → `Some("@types/react")`
///   `/x/node_modules/a/node_modules/b/y.d.ts`      → `Some("b")` (innermost wins)
///   `/x/packages/db/dist/index.d.ts`               → `None` (workspace file)
fn node_modules_package(path: &str) -> Option<&str> {
    let marker = "/node_modules/";
    let last = path.rfind(marker)?;
    let after = &path[last + marker.len()..];
    // Scoped package: `@scope/pkg`
    if after.starts_with('@') {
        let scope_end = after.find('/')?;
        let pkg_end = after[scope_end + 1..]
            .find('/')
            .unwrap_or(after.len() - scope_end - 1);
        Some(&after[..scope_end + 1 + pkg_end])
    } else {
        let pkg_end = after.find('/').unwrap_or(after.len());
        Some(&after[..pkg_end])
    }
}

/// Should autodiscovery follow this import edge?
///
/// We always allow:
///   * `from` is a workspace file (NOT in node_modules)
///   * `from` and `to` are in the SAME node_modules package
///     (e.g. `preact/hooks/dist/index.d.ts` → `preact/jsx-runtime/index.d.ts`)
///
/// We REJECT:
///   * `from` is in node_modules and `to` is in a DIFFERENT node_modules package
///     (e.g. `next-auth → @auth/core → preact`). This stops transitive
///     pollution from random unrelated typings while still letting a
///     package's own `.d.ts` graph fan out.
///
/// Workspace files always reach their direct imports (e.g. apps/app
/// pulling `next-auth/index.d.ts`), but those imports don't get to drag
/// in the rest of the universe.
/// Recognise the three TypeScript declaration extensions a typed package can
/// legitimately publish: `.d.ts` (classic / dual-package or UMD), `.d.cts`
/// (CJS-only typings, e.g. `zod/v4/index.d.cts`), and `.d.mts` (ESM-only
/// typings). Used by autodiscovery to decide whether a resolved import
/// target is a typing file worth following.
fn is_dts_path(path: &str) -> bool {
    path.ends_with(".d.ts") || path.ends_with(".d.cts") || path.ends_with(".d.mts")
}

/// A TypeScript (not declaration, not JavaScript) source file outside
/// `node_modules`.
fn is_workspace_ts_source(path: &str) -> bool {
    !is_dts_path(path)
        && [".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|ext| path.ends_with(ext))
        && node_modules_package(path).is_none()
}

fn autodiscover_should_follow(from: &str, to: &str) -> bool {
    let from_pkg = node_modules_package(from);
    let to_pkg = node_modules_package(to);
    match (from_pkg, to_pkg) {
        (None, _) => true,            // workspace → anywhere
        (Some(_), None) => true,      // package → workspace (rare; unblock anyway)
        (Some(a), Some(b)) => a == b, // package → same-package files only
    }
}

pub fn autodiscover_dts(parsed_files: &mut Vec<SourceFile>, options: &CompilerOptions) {
    if !autodiscover_enabled() {
        return;
    }
    let debug = std::env::var("TSC_RS_AUTODISCOVER_DEBUG").is_ok();
    let max_bytes = autodiscover_max_bytes();
    let max_files = autodiscover_max_files();
    let max_rounds = autodiscover_max_rounds();
    let max_per_package = autodiscover_max_per_package();
    let initial_count = parsed_files.len();
    let mut discovered: HashSet<String> =
        parsed_files.iter().map(|sf| sf.file_name.clone()).collect();
    // Per-package file count, used to bound the BFS in
    // wide-import workspaces (apps/app's 280 direct imports balloon
    // to 2000+ in round 1 via Prisma's `Prisma*Args` and `@types/node`'s
    // 200+ files). Workspace files have no per-package limit; only
    // node_modules packages do.
    //
    // Seed with already-loaded node_modules files so a project that
    // pre-loaded a heavy package doesn't get a fresh quota.
    let mut pkg_counts: HashMap<String, usize> = HashMap::new();
    for sf in parsed_files.iter() {
        if let Some(pkg) = node_modules_package(&sf.file_name) {
            *pkg_counts.entry(pkg.to_string()).or_insert(0) += 1;
        }
    }
    let mut frontier: Vec<usize> = (0..parsed_files.len()).collect();
    if debug {
        eprintln!(
            "[autodisc] start initial={} max_bytes={} max_files={} max_rounds={} max_per_pkg={}",
            initial_count, max_bytes, max_files, max_rounds, max_per_package
        );
    }
    for round in 0..max_rounds {
        if frontier.is_empty() {
            break;
        }
        if parsed_files.len().saturating_sub(initial_count) >= max_files {
            if debug {
                eprintln!("[autodisc] hit max_files cap");
            }
            break;
        }
        let work: Vec<String> = frontier
            .par_iter()
            .flat_map_iter(|&idx| {
                let sf = &parsed_files[idx];
                let from = sf.file_name.clone();
                collect_module_sources(sf)
                    .into_iter()
                    .filter_map(move |source| {
                        let resolved =
                            tsc_rs_resolver::resolve_module_name(&source, &from, options)?;
                        let path = resolved.resolved_file_name;
                        // Accept all TypeScript declaration extensions:
                        //   .d.ts   — classic CJS/UMD typings (most packages)
                        //   .d.cts  — CJS-only typings (e.g. zod/v4 ships
                        //             `index.d.cts` as its `types` export)
                        //   .d.mts  — ESM-only typings
                        // Skipping .d.cts/.d.mts means a CJS package's entire
                        // type surface is invisible to autodiscovery — that's
                        // why zod/v4 inference collapsed to `any` (only its
                        // entry `.d.ts` re-exported `.d.cts` files, which
                        // were rejected here).
                        //
                        // A TypeScript source outside node_modules is part
                        // of the program too: tsc loads (and checks) every
                        // source an import resolves to, which is how a
                        // workspace package consumed from source
                        // (`@scope/services` → `../services/src/…`) gets
                        // its exports known and its own errors reported.
                        if !is_dts_path(&path) && !is_workspace_ts_source(&path) {
                            return None;
                        }
                        // Cross-package transitive scoping: a workspace
                        // file can pull in any `.d.ts`, but once we're
                        // inside a node_modules package we only follow
                        // its own internal links. Stops e.g.
                        // `next-auth → @auth/core → preact` from
                        // polluting the global scope with Preact's
                        // `useEffect(deps: Inputs)` signature.
                        if !autodiscover_should_follow(&from, &path) {
                            return None;
                        }
                        Some(path)
                    })
            })
            .collect();
        let mut to_parse: Vec<String> = Vec::new();
        // Per-package quota accounting for this round. Once a package
        // would push past `max_per_package`, drop subsequent candidates
        // from that package. Without this, apps/app's @types/node alone
        // pulls 200+ files in a single round.
        for path in work {
            if !discovered.insert(path.clone()) {
                continue;
            }
            if max_per_package > 0 {
                if let Some(pkg) = node_modules_package(&path) {
                    let count = pkg_counts.entry(pkg.to_string()).or_insert(0);
                    if *count >= max_per_package {
                        continue;
                    }
                    *count += 1;
                }
            }
            to_parse.push(path);
        }
        if to_parse.is_empty() {
            break;
        }
        let newly_parsed: Vec<SourceFile> = to_parse
            .par_iter()
            .filter_map(|path| {
                // Cheap stat probe; skip giant generated files to keep
                // memory + parse time bounded. The membership in
                // `discovered` already happened, so the file won't be
                // re-tried in a later round.
                if max_bytes > 0 {
                    if let Ok(meta) = std::fs::metadata(path) {
                        if meta.len() > max_bytes {
                            return None;
                        }
                    }
                    if prisma_model_without_namespace(path, max_bytes) {
                        return None;
                    }
                }
                std::fs::read_to_string(path)
                    .ok()
                    .map(|src| tsc_rs_parser::parse(path, &src))
            })
            .collect();
        if debug {
            eprintln!(
                "[autodisc] round {} candidates={} new={} (total={})",
                round,
                to_parse.len(),
                newly_parsed.len(),
                parsed_files.len() + newly_parsed.len()
            );
        }
        let start = parsed_files.len();
        parsed_files.extend(newly_parsed);
        frontier = (start..parsed_files.len()).collect();
    }
    if debug {
        eprintln!(
            "[autodisc] done added={} total={}",
            parsed_files.len() - initial_count,
            parsed_files.len()
        );
    }
}

/// A Prisma-generated per-model typing (`<client>/models/<Model>.d.ts`) whose
/// client namespace (`<client>/internal/prismaNamespace.d.ts`) is over the
/// size cap, so it will not be loaded.
///
/// A model file's types are written against that namespace
/// (`Prisma.$ProductPayload`, `Prisma.StringFilter`, …), so without it they
/// are hollow shells: the payload and result types already resolve to
/// `any`, and only the top-level keys of the `*WhereInput` / `*Select`
/// shapes survive. Skipping them leaves those names unresolved. The files
/// are not small, though: a large production client has ~1 300 models
/// under the cap, 92 MB of `.d.ts` that took a `packages/scripts` daemon
/// from 0.85 to 3.2 GB RSS and an `apps/app` one from 2.3 to 5 GB. One-shot
/// runs set no cap and load both, as tsc does.
fn prisma_model_without_namespace(path: &str, max_bytes: u64) -> bool {
    let p = Path::new(path);
    let Some(models_dir) = p.parent() else {
        return false;
    };
    if models_dir.file_name().and_then(|n| n.to_str()) != Some("models") || !is_dts_path(path) {
        return false;
    }
    let Some(client_dir) = models_dir.parent() else {
        return false;
    };
    match std::fs::metadata(client_dir.join("internal").join("prismaNamespace.d.ts")) {
        Ok(meta) => meta.len() > max_bytes,
        Err(_) => false,
    }
}

/// Collect every module specifier referenced by a file's imports AND its
/// `export … from "…"` re-exports.
///
/// Used by autodiscovery to follow the full type-resolution graph. Plain
/// `import` covers value/type imports; the export-from forms cover
/// `export { x as y } from "./mod"` and `export * from "./mod"` — both
/// of which extend the project's type universe even though no local
/// binding is introduced.
fn collect_module_sources(file: &SourceFile) -> Vec<String> {
    let mut sources = Vec::new();
    for stmt in &file.statements {
        match &stmt.kind {
            StmtKind::Import(import_decl) => {
                sources.push(import_decl.source.to_string());
            }
            StmtKind::Export(ed) => match &ed.kind {
                tsc_rs_ast::ExportDeclKind::Named {
                    source: Some(s), ..
                } => sources.push(s.to_string()),
                tsc_rs_ast::ExportDeclKind::All { source, .. } => sources.push(source.to_string()),
                _ => {}
            },
            _ => {}
        }
    }
    sources
}

// ---------------------------------------------------------------------------
// Incremental build info (.tsbuildinfo)
// ---------------------------------------------------------------------------

/// Persisted build state for incremental compilation.
#[derive(Debug, Clone)]
pub struct TsBuildInfo {
    pub version: String,
    pub file_hashes: HashMap<String, String>,
    pub options_hash: String,
    pub dependencies: HashMap<String, Vec<String>>,
}

impl TsBuildInfo {
    /// Load a `.tsbuildinfo` file. Returns `None` if the file doesn't exist
    /// or can't be parsed.
    pub fn load(path: &str) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Self::parse_json(&content)
    }

    /// Save this build info to a `.tsbuildinfo` file.
    pub fn save(&self, path: &str) -> Result<(), String> {
        let json = self.to_json();
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create directory for {}: {}", path, e))?;
        }
        std::fs::write(path, json).map_err(|e| format!("Cannot write {}: {}", path, e))
    }

    /// Serialize to a JSON string (no serde dependency).
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        out.push_str(&format!("  \"version\": \"{}\",\n", self.version));
        out.push_str(&format!("  \"optionsHash\": \"{}\",\n", self.options_hash));

        // fileHashes
        out.push_str("  \"fileHashes\": {");
        let entries: Vec<_> = {
            let mut v: Vec<_> = self.file_hashes.iter().collect();
            v.sort_by_key(|(k, _)| (*k).clone());
            v
        };
        if entries.is_empty() {
            out.push('}');
        } else {
            out.push('\n');
            for (i, (k, v)) in entries.iter().enumerate() {
                out.push_str(&format!("    \"{}\": \"{}\"", k, v));
                if i + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  }");
        }
        out.push_str(",\n");

        // dependencies
        out.push_str("  \"dependencies\": {");
        let dep_entries: Vec<_> = {
            let mut v: Vec<_> = self.dependencies.iter().collect();
            v.sort_by_key(|(k, _)| (*k).clone());
            v
        };
        if dep_entries.is_empty() {
            out.push('}');
        } else {
            out.push('\n');
            for (i, (k, deps)) in dep_entries.iter().enumerate() {
                let deps_str: Vec<String> = deps.iter().map(|d| format!("\"{}\"", d)).collect();
                out.push_str(&format!("    \"{}\": [{}]", k, deps_str.join(", ")));
                if i + 1 < dep_entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  }");
        }
        out.push('\n');

        out.push_str("}\n");
        out
    }

    /// Parse a `.tsbuildinfo` JSON string.
    fn parse_json(content: &str) -> Option<Self> {
        let version = extract_json_string(content, "version")?;
        let options_hash = extract_json_string(content, "optionsHash").unwrap_or_default();

        // Parse fileHashes
        let mut file_hashes = HashMap::new();
        if let Some(hashes_obj) = extract_json_object(content, "fileHashes") {
            // Simple parsing: find all "key": "value" pairs
            let mut remaining = hashes_obj.as_str();
            while let Some(key_start) = remaining.find('"') {
                remaining = &remaining[key_start + 1..];
                let key_end = remaining.find('"')?;
                let key = remaining[..key_end].to_string();
                remaining = &remaining[key_end + 1..];
                // Skip to colon and value
                let colon = remaining.find(':')?;
                remaining = &remaining[colon + 1..];
                let val_start = remaining.find('"')?;
                remaining = &remaining[val_start + 1..];
                let val_end = remaining.find('"')?;
                let val = remaining[..val_end].to_string();
                remaining = &remaining[val_end + 1..];

                // Skip internal keys like the outer object keys
                if key != "fileHashes"
                    && key != "dependencies"
                    && key != "version"
                    && key != "optionsHash"
                {
                    file_hashes.insert(key, val);
                }
            }
        }

        // Parse dependencies
        let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(deps_obj) = extract_json_object(content, "dependencies") {
            // Find each "key": [...] pair
            let mut remaining = deps_obj.as_str();
            while let Some(key_start) = remaining.find('"') {
                remaining = &remaining[key_start + 1..];
                let Some(key_end) = remaining.find('"') else {
                    break;
                };
                let key = remaining[..key_end].to_string();
                remaining = &remaining[key_end + 1..];

                let Some(arr_start) = remaining.find('[') else {
                    break;
                };
                remaining = &remaining[arr_start + 1..];
                let Some(arr_end) = remaining.find(']') else {
                    break;
                };
                let arr_content = &remaining[..arr_end];
                remaining = &remaining[arr_end + 1..];

                let mut deps = Vec::new();
                let mut arr_remaining = arr_content;
                while let Some(s) = arr_remaining.find('"') {
                    arr_remaining = &arr_remaining[s + 1..];
                    if let Some(e) = arr_remaining.find('"') {
                        deps.push(arr_remaining[..e].to_string());
                        arr_remaining = &arr_remaining[e + 1..];
                    } else {
                        break;
                    }
                }

                if key != "dependencies" {
                    dependencies.insert(key, deps);
                }
            }
        }

        Some(TsBuildInfo {
            version,
            file_hashes,
            options_hash,
            dependencies,
        })
    }
}

/// Compute a simple hash of a string (DJB2 algorithm).
pub fn simple_hash(s: &str) -> String {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    format!("{:016x}", hash)
}

/// Hash compiler options into a single string for change detection.
pub fn hash_compiler_options(opts: &CompilerOptions) -> String {
    let mut parts = Vec::new();
    if let Some(t) = &opts.target {
        parts.push(format!("target:{:?}", t));
    }
    if let Some(m) = &opts.module {
        parts.push(format!("module:{:?}", m));
    }
    if let Some(s) = opts.strict {
        parts.push(format!("strict:{}", s));
    }
    if let Some(d) = opts.declaration {
        parts.push(format!("declaration:{}", d));
    }
    if let Some(sm) = opts.source_map {
        parts.push(format!("sourceMap:{}", sm));
    }
    if let Some(ref od) = opts.out_dir {
        parts.push(format!("outDir:{}", od));
    }
    if let Some(ne) = opts.no_emit {
        parts.push(format!("noEmit:{}", ne));
    }
    if let Some(j) = &opts.jsx {
        parts.push(format!("jsx:{:?}", j));
    }
    simple_hash(&parts.join("|"))
}

/// Determine the path for the `.tsbuildinfo` file.
pub fn resolve_build_info_path(options: &CompilerOptions, config_path: Option<&str>) -> String {
    // 1. Explicit tsBuildInfoFile
    if let Some(ref explicit) = options.ts_build_info_file {
        return explicit.clone();
    }
    // 2. Derive from outDir or config path
    let base = if let Some(ref out_dir) = options.out_dir {
        PathBuf::from(out_dir)
    } else if let Some(cfg) = config_path {
        Path::new(cfg)
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf()
    } else {
        PathBuf::from(".")
    };
    let name = if let Some(cfg) = config_path {
        let stem = Path::new(cfg)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("tsconfig");
        format!("{}.tsbuildinfo", stem)
    } else {
        "tsconfig.tsbuildinfo".to_string()
    };
    base.join(name).to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// Project reference build orchestration
// ---------------------------------------------------------------------------

/// Result of building a project reference graph.
#[derive(Debug)]
pub struct ProjectReferenceBuildResult {
    pub results: Vec<(String, CompilationResult)>,
    pub build_order: Vec<String>,
}

/// Build a project and all its referenced projects in topological order.
/// Each referenced project is built first, and its declaration outputs are
/// made available to downstream projects.
pub fn build_project_references(config_path: &str) -> Result<ProjectReferenceBuildResult, String> {
    let refs = parse_project_references(config_path)?;
    let config_dir = tsconfig_dir(Path::new(config_path));

    // Collect all project config paths (references first, then root)
    let mut all_configs: Vec<String> = Vec::new();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();

    // Resolve reference paths to tsconfig.json paths
    let root_config = normalize_path(Path::new(config_path));
    for r in &refs {
        let ref_dir = config_dir.join(&r.path);
        let ref_config = if ref_dir.is_dir() {
            ref_dir.join("tsconfig.json")
        } else {
            ref_dir
        };
        let ref_config_str = normalize_path(&ref_config);
        if !all_configs.contains(&ref_config_str) {
            all_configs.push(ref_config_str.clone());
        }
        adjacency
            .entry(root_config.clone())
            .or_default()
            .push(ref_config_str);
    }
    if !all_configs.contains(&root_config) {
        all_configs.push(root_config.clone());
    }

    // Topological sort using Kahn's algorithm
    let build_order = topological_sort(&all_configs, &adjacency)?;

    // Build each project in order
    let mut results = Vec::new();
    for cfg in &build_order {
        match TsProject::from_config(cfg) {
            Ok(project) => {
                let result = project.compile();
                results.push((cfg.clone(), result));
            }
            Err(e) => {
                return Err(format!("Failed to build referenced project {}: {}", cfg, e));
            }
        }
    }

    Ok(ProjectReferenceBuildResult {
        results,
        build_order,
    })
}

/// Topological sort of project configs. Dependencies (referenced projects)
/// should appear before the projects that reference them.
pub fn topological_sort(
    nodes: &[String],
    adjacency: &HashMap<String, Vec<String>>,
) -> Result<Vec<String>, String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for node in nodes {
        in_degree.entry(node.as_str()).or_insert(0);
    }

    // Build reverse adjacency: if A depends on B (A -> B in adjacency),
    // then B must come before A. in_degree[A] += 1 for each dependency.
    for (node, deps) in adjacency {
        for dep in deps {
            if in_degree.contains_key(dep.as_str()) {
                *in_degree.entry(node.as_str()).or_insert(0) += 1;
            }
        }
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&node, _)| node)
        .collect();
    queue.sort(); // deterministic ordering

    let mut result = Vec::new();
    while let Some(node) = queue.pop() {
        result.push(node.to_string());
        // Find all nodes that depend on this node and decrement their in-degree
        for (dependent, deps) in adjacency {
            if deps.iter().any(|d| d.as_str() == node) {
                if let Some(deg) = in_degree.get_mut(dependent.as_str()) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(dependent.as_str());
                        queue.sort();
                    }
                }
            }
        }
    }

    if result.len() != nodes.len() {
        return Err("Circular project reference detected".to_string());
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Directory containing a tsconfig file, resolved relative to the current
/// working directory. `Path::parent` on a bare filename like
/// `"tsconfig.json"` returns `Some("")` rather than `None`, so the naive
/// `parent().unwrap_or(Path::new("."))` falls through to an empty path —
/// which `is_dir()` reports false for, breaking the `**/*.ts` include glob
/// expansion (only directory-prefixed globs like `.next/types/**/*.ts`
/// resolved). Treat an empty parent as `.`.
fn tsconfig_dir(config_path: &Path) -> &Path {
    match config_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// Parse a tsconfig.json file and return the compiler options, file names,
/// and any errors encountered.
pub fn parse_tsconfig(path: &str) -> Result<ParsedCommandLine, String> {
    let mut visited = std::collections::HashSet::new();
    let mut result = parse_tsconfig_with_visited(path, &mut visited)?;

    // Auto-load @types/* entry points so ambient declarations (e.g.
    // `declare var global: typeof globalThis` in @types/node/globals.d.ts)
    // land in the donor scope. Matches TypeScript's default behavior: if
    // `compilerOptions.types` is unspecified, every @types/* package found
    // under `typeRoots` (default `node_modules/@types`, walking up parent
    // directories) is included.
    //
    // Once, on the merged config: done per config of an `extends` chain it
    // made a child without `include` look like it listed files, and the
    // merge then dropped the files it inherits from its base.
    let _at = std::time::Instant::now();
    augment_with_at_types(
        tsconfig_dir(Path::new(path)),
        &result.options,
        &mut result.file_names,
    );
    if std::env::var("TSC_RS_INIT_TIMING").is_ok() {
        eprintln!(
            "[init]   at_types       {:>7.1}ms ({} total)",
            _at.elapsed().as_secs_f64() * 1000.0,
            result.file_names.len()
        );
    }
    Ok(result)
}

fn parse_tsconfig_with_visited(
    path: &str,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
) -> Result<ParsedCommandLine, String> {
    let config_path = Path::new(path);
    let config_dir = tsconfig_dir(config_path);

    // Cycle detection: canonicalize and check for revisit
    let canonical = config_path
        .canonicalize()
        .unwrap_or_else(|_| config_path.to_path_buf());
    if !visited.insert(canonical.clone()) {
        eprintln!(
            "warning: tsconfig '{}' extends itself — loading options directly",
            path
        );
        // Break the cycle: parse this config without following extends
        let content = std::fs::read_to_string(config_path)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;
        return parse_tsconfig_content(&content, config_dir);
    }

    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("failed to read {}: {}", path, e))?;

    let mut result = parse_tsconfig_content(&content, config_dir)?;

    // Handle extends
    if let Some(extends_path) = extract_json_string(&content, "extends") {
        let base_path = resolve_extends_path(&extends_path, config_dir);
        if let Ok(base_result) =
            parse_tsconfig_with_visited(base_path.to_str().unwrap_or(""), visited)
        {
            result = merge_configs(base_result, result);
        } else {
            result.errors.push(Diagnostic {
                code: 6053,
                message: format!("File '{}' not found.", extends_path),
                category: DiagnosticCategory::Error,
                file_name: Some(path.to_string()),
                span: None,
                related: None,
            });
        }
    }

    Ok(result)
}

/// Parse project references from a tsconfig.json file.
pub fn parse_project_references(path: &str) -> Result<Vec<ProjectReference>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {}: {}", path, e))?;

    Ok(parse_references_field(&content))
}

// ---------------------------------------------------------------------------
// Internal: tsconfig content parsing
// ---------------------------------------------------------------------------

fn parse_tsconfig_content(content: &str, config_dir: &Path) -> Result<ParsedCommandLine, String> {
    let mut options = CompilerOptions::default();
    let mut errors = Vec::new();

    // Parse compilerOptions
    if let Some(compiler_options_str) = extract_json_object(content, "compilerOptions") {
        parse_compiler_options(&compiler_options_str, &mut options);
    }

    // TS 4.1+: when `paths` is set without `baseUrl`, paths are resolved
    // relative to the tsconfig directory. Default base_url so the resolver
    // (which always needs base_url as the join root) treats path entries
    // as relative-to-config-dir. Also expand a relative baseUrl against
    // config_dir, since the resolver later joins it with relative path
    // targets and `Path::new("./").join("./src/*")` won't resolve unless
    // cwd happens to be config_dir.
    if options.paths.is_some() {
        match &options.base_url {
            None => {
                options.base_url = config_dir.to_str().map(str::to_string);
            }
            Some(b) => {
                let p = Path::new(b);
                if p.is_relative() {
                    let abs = config_dir.join(p);
                    options.base_url = abs.to_str().map(str::to_string);
                }
            }
        }
    } else if let Some(b) = &options.base_url {
        // baseUrl alone (no paths): same relativization for consistency.
        let p = Path::new(b);
        if p.is_relative() {
            let abs = config_dir.join(p);
            options.base_url = abs.to_str().map(str::to_string);
        }
    }

    // Discover files
    let _dt = std::time::Instant::now();
    let file_names = discover_files(content, config_dir, &mut errors);
    if std::env::var("TSC_RS_INIT_TIMING").is_ok() {
        eprintln!(
            "[init]   discover_files {:>7.1}ms ({} files)",
            _dt.elapsed().as_secs_f64() * 1000.0,
            file_names.len()
        );
    }

    Ok(ParsedCommandLine {
        options,
        file_names,
        errors,
    })
}

/// Append the `.d.ts` entry points of every relevant `@types/*` package to
/// `file_names`. See `compilerOptions.types` / `typeRoots` for the policy.
fn augment_with_at_types(
    config_dir: &Path,
    options: &CompilerOptions,
    file_names: &mut Vec<String>,
) {
    // Per-tsconfig opt-out (TypeScript: `noLib` + `types: []` are the two
    // canonical escape hatches).
    if options.no_lib == Some(true) {
        return;
    }
    // `-p tsconfig.json` gives an empty relative `config_dir`, which the
    // upward `node_modules` walks below cannot climb: use its absolute form.
    let absolute_config_dir =
        std::path::absolute(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
    let config_dir = absolute_config_dir.as_path();

    // Resolve the type roots. Default = `node_modules/@types` walked up
    // from config_dir until found or filesystem root.
    let type_roots: Vec<PathBuf> = match &options.type_roots {
        Some(list) => list
            .iter()
            .map(|r| {
                let p = Path::new(r);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    config_dir.join(p)
                }
            })
            .filter(|p| p.is_dir())
            .collect(),
        None => {
            let mut found = Vec::new();
            let mut search = config_dir.to_path_buf();
            loop {
                let candidate = search.join("node_modules").join("@types");
                if candidate.is_dir() {
                    found.push(candidate);
                    // Don't break — TypeScript walks all ancestors and
                    // merges. Monorepos can stage @types both at the package
                    // root and at the workspace root.
                }
                if !search.pop() {
                    break;
                }
            }
            found
        }
    };

    // Determine which packages to load.
    let packages_to_load: Option<Vec<String>> = match &options.types {
        Some(list) => {
            if list.is_empty() {
                // `types: []` → load nothing.
                return;
            }
            Some(list.clone())
        }
        None => None, // None → discover all under each typeRoots dir
    };

    let mut seen = std::collections::HashSet::new();
    for root in &type_roots {
        let pkg_names: Vec<String> = match &packages_to_load {
            Some(list) => list.clone(),
            None => match std::fs::read_dir(root) {
                Ok(entries) => entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|name| !name.starts_with('.') && !name.starts_with('@'))
                    .collect(),
                Err(_) => continue,
            },
        };

        for pkg_name in pkg_names {
            // De-dup across roots: a package found in a closer root wins —
            // matches TypeScript's "nearest typeRoot wins" merge.
            if seen.contains(&pkg_name) {
                continue;
            }
            let pkg_dir = root.join(&pkg_name);
            if !pkg_dir.is_dir() {
                continue;
            }
            seen.insert(pkg_name.clone());
            if let Some(entry) = find_types_pkg_entry(&pkg_dir) {
                let norm = normalize_path(Path::new(&entry));
                if !file_names.contains(&norm) {
                    file_names.push(norm.clone());
                }
                // `@types/node`'s `index.d.ts` is a thin shell of
                // `/// <reference path="..." />` directives — the real
                // ambient declarations (`declare var global`, Buffer,
                // process, etc.) live in the referenced files. Walk the
                // graph transitively so they all reach the donor inject
                // pass.
                collect_reference_paths(&norm, file_names);
            }
        }
    }

    // An explicit `types` entry that no type root holds resolves like a
    // `/// <reference types>` directive: through `node_modules` (tsc).
    // `"types": ["bun-types"]` names a plain package, not an `@types` one.
    for pkg_name in packages_to_load.iter().flatten() {
        if seen.contains(pkg_name) {
            continue;
        }
        if let Some(entry) = resolve_type_reference(pkg_name, config_dir) {
            seen.insert(pkg_name.clone());
            if !file_names.contains(&entry) {
                file_names.push(entry.clone());
            }
            collect_reference_paths(&entry, file_names);
        }
    }
}

fn is_declaration_file_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    [".d.ts", ".d.tsx", ".d.mts", ".d.cts"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Resolves a type reference (`types` entry or `/// <reference types>`)
/// from `from_dir` up: `node_modules/@types/<name>` first, then
/// `node_modules/<name>` (tsc's secondary lookup).
fn resolve_type_reference(name: &str, from_dir: &Path) -> Option<String> {
    let at_types_name = match name.strip_prefix('@') {
        Some(scoped) => scoped.replacen('/', "__", 1),
        None => name.to_string(),
    };
    let mut dir = std::path::absolute(from_dir).ok()?;
    loop {
        let node_modules = dir.join("node_modules");
        for pkg_dir in [
            node_modules.join("@types").join(&at_types_name),
            node_modules.join(name),
        ] {
            if pkg_dir.is_dir() {
                if let Some(entry) = find_types_pkg_entry(&pkg_dir) {
                    return Some(normalize_path(Path::new(&entry)));
                }
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Recursively follow `/// <reference path="..." />` triple-slash directives
/// starting at `entry_path` and append the resolved files to `file_names`.
/// Visits each path at most once so cycles (rare but possible across
/// `@types/node`'s many cross-references) terminate cleanly.
fn collect_reference_paths(entry_path: &str, file_names: &mut Vec<String>) {
    let mut to_visit: Vec<String> = vec![entry_path.to_string()];
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    while let Some(path) = to_visit.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(parent) = Path::new(&path).parent() else {
            continue;
        };
        let mut in_block_comment = false;
        for line in content.lines() {
            let trimmed = line.trim_start();
            // Coarse block-comment tracker so we don't terminate when a
            // file leads with `/** … */` JSDoc — `@types/node`'s index.d.ts
            // does that and would otherwise hide its references.
            if in_block_comment {
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
                continue;
            }
            if trimmed.starts_with("/*") && !trimmed.contains("*/") {
                in_block_comment = true;
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with("//") && !trimmed.starts_with("///") {
                continue;
            }
            // Stop at the first real declaration. Triple-slash references
            // must precede any statement per the TS spec.
            let Some(rest) = trimmed.strip_prefix("///") else {
                // not a triple-slash; if it's neither a comment nor empty,
                // we've reached the body — stop.
                if trimmed.starts_with("/*") {
                    continue;
                }
                break;
            };
            let rest = rest.trim_start();
            // Match `<reference path="..."` with permissive whitespace.
            let after_lt = match rest.strip_prefix("<reference") {
                Some(s) => s.trim_start(),
                None => continue,
            };
            // `path` names a file; `types` names a package (resolved
            // through node_modules below).
            let (is_types, after_path) = if let Some(s) = after_lt.strip_prefix("path") {
                (false, s.trim_start())
            } else if let Some(s) = after_lt.strip_prefix("types") {
                (true, s.trim_start())
            } else {
                continue;
            };
            let after_eq = match after_path.strip_prefix('=') {
                Some(s) => s.trim_start(),
                None => continue,
            };
            // Quoted path (either '"' or '\'').
            let (quote, rest_after_quote) = if let Some(r) = after_eq.strip_prefix('"') {
                ('"', r)
            } else if let Some(r) = after_eq.strip_prefix('\'') {
                ('\'', r)
            } else {
                continue;
            };
            let Some(end) = rest_after_quote.find(quote) else {
                continue;
            };
            let referenced = &rest_after_quote[..end];
            if is_types {
                if let Some(entry) = resolve_type_reference(referenced, parent) {
                    if !file_names.contains(&entry) {
                        file_names.push(entry.clone());
                    }
                    to_visit.push(entry);
                }
                continue;
            }
            let mut target = parent.join(referenced);
            // `<reference path>` paths can omit the `.d.ts` extension; try
            // adding it if the literal isn't a file.
            if !target.is_file() {
                let with_ext = target.with_extension("d.ts");
                if with_ext.is_file() {
                    target = with_ext;
                }
            }
            if !target.is_file() {
                continue;
            }
            let norm = normalize_path(&target);
            if !file_names.contains(&norm) {
                file_names.push(norm.clone());
            }
            to_visit.push(norm);
        }
    }
}

/// Find the `.d.ts` entry point for an @types package.
///
/// Checks `package.json` (`types` / `typings` / `main` fields, in that order)
/// and falls back to `index.d.ts`. Mirrors `tsc_rs_server::find_types_entry`
/// and TypeScript's own resolution order.
fn find_types_pkg_entry(pkg_dir: &Path) -> Option<String> {
    let pkg_json = pkg_dir.join("package.json");
    if let Ok(content) = std::fs::read_to_string(&pkg_json) {
        for field in ["types", "typings", "main"] {
            if let Some(value) = extract_json_string(&content, field) {
                // Only return paths that point at a `.d.ts` — `main` often
                // names a `.js` runtime entry we should NOT add to the
                // checker's file list.
                let mut resolved = pkg_dir.join(&value);
                if resolved.extension().is_none() {
                    // bare `./foo` form → try `.d.ts`
                    resolved.set_extension("d.ts");
                }
                if resolved.is_file()
                    && resolved
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".d.ts")
                {
                    return resolved.to_str().map(str::to_string);
                }
            }
        }
    }
    let index = pkg_dir.join("index.d.ts");
    if index.is_file() {
        return index.to_str().map(str::to_string);
    }
    None
}

/// Discover files based on `files`, `include`, and `exclude` fields.
fn discover_files(content: &str, config_dir: &Path, errors: &mut Vec<Diagnostic>) -> Vec<String> {
    let mut file_names = Vec::new();

    // 1. Explicit "files" array
    let explicit_files = extract_json_string_array(content, "files");
    for f in &explicit_files {
        let full_path = config_dir.join(f);
        if full_path.is_file() {
            file_names.push(normalize_path(&full_path));
        } else {
            errors.push(Diagnostic {
                code: 6053,
                message: format!("File '{}' not found.", full_path.display()),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: None,
                related: None,
            });
        }
    }

    // 2. Include patterns
    let include_patterns = extract_json_string_array(content, "include");
    let exclude_patterns = extract_json_string_array(content, "exclude");

    if !include_patterns.is_empty() {
        // Directory excludes (e.g. `"exclude": ["node_modules", ".next/cache"]`)
        // need prefix matching against the discovered file list — the old
        // exact-match-only logic silently ignored every directory pattern,
        // so checks against real projects walked thousands of files that
        // belonged in excluded subtrees (.next/dev/server, .next/dev/static).
        //
        // Computed BEFORE the walk and passed INTO it so the recursion prunes
        // excluded subtrees instead of walking them and discarding the result.
        // On apps/app the include is `**/*.ts` from the config dir and
        // `.next/{cache,static,server,dev/*}` is a 16 GB / ~35k-entry webpack
        // cache — excluded, but the old flow still `readdir`+`stat`-ed every
        // entry before filtering it out (the whole ~3.4s discover cost).
        let excluded_dirs = expand_exclude_directories(&exclude_patterns, config_dir);
        let included = expand_include_patterns(&include_patterns, config_dir, &excluded_dirs);
        let excluded = expand_exclude_set(&exclude_patterns, config_dir);

        let mut seen: std::collections::HashSet<String> = file_names.iter().cloned().collect();
        for path in included {
            let norm = normalize_path(&path);
            if excluded.contains(&norm)
                || excluded_dirs.iter().any(|d| norm.starts_with(d.as_str()))
            {
                continue;
            }
            // O(1) dedup; the old `file_names.contains` was O(n) per push →
            // O(n²) over the whole include set.
            if seen.insert(norm.clone()) {
                file_names.push(norm);
            }
        }
    }

    // 3. Default behavior: if neither files nor include is specified,
    //    include all .ts/.tsx files in the directory (non-recursive for simplicity)
    if explicit_files.is_empty() && include_patterns.is_empty() {
        if let Ok(entries) = std::fs::read_dir(config_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "ts" || ext == "tsx" {
                            let norm = normalize_path(&path);
                            if !file_names.contains(&norm) {
                                file_names.push(norm);
                            }
                        }
                    }
                }
            }
        }
    }

    file_names.sort();
    file_names
}

/// Expand include glob patterns to file paths.
/// Supports simple patterns like `src/**/*.ts` and `*.ts`.
fn expand_include_patterns(
    patterns: &[String],
    config_dir: &Path,
    excluded_dirs: &[String],
) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // Group recursive `**` globs by their base directory so a base is walked
    // ONCE even when several patterns target it with different suffixes. A
    // typical Next.js tsconfig lists `**/*.mts`, `**/*.ts`, `**/*.tsx` — all
    // rooted at config_dir — which the naive per-pattern loop walked as three
    // full-tree traversals. Preserve first-seen base order for determinism.
    let mut recursive_bases: Vec<(PathBuf, Vec<String>)> = Vec::new();

    for pattern in patterns {
        if pattern.contains("**") {
            // Recursive glob
            let parts: Vec<&str> = pattern.splitn(2, "**").collect();
            let base = config_dir.join(parts[0].trim_end_matches('/'));
            let suffix = parts
                .get(1)
                .map(|s| s.trim_start_matches('/'))
                .unwrap_or("")
                .to_string();
            match recursive_bases.iter_mut().find(|(b, _)| *b == base) {
                Some((_, suffixes)) => {
                    if !suffixes.contains(&suffix) {
                        suffixes.push(suffix);
                    }
                }
                None => recursive_bases.push((base, vec![suffix])),
            }
        } else if pattern.contains('*') {
            // Simple wildcard
            let dir_part = Path::new(pattern).parent().unwrap_or(Path::new(""));
            let file_pattern = Path::new(pattern)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let search_dir = config_dir.join(dir_part);
            if let Ok(entries) = std::fs::read_dir(&search_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if matches_simple_glob(name, file_pattern) {
                                files.push(path);
                            }
                        }
                    }
                }
            }
        } else {
            // Literal path
            let path = config_dir.join(pattern);
            if path.is_file() {
                files.push(path);
            }
        }
    }

    // One walk per unique base, matching any of its suffixes.
    for (base, suffixes) in &recursive_bases {
        collect_files_matching(base, suffixes, &mut files, excluded_dirs);
    }

    files
}

/// Build a set of excluded file paths (normalized).
fn expand_exclude_set(patterns: &[String], config_dir: &Path) -> Vec<String> {
    // No dir-pruning here: this expands the exclude GLOBS themselves to the
    // file set they match, so there is nothing to prune against.
    let expanded = expand_include_patterns(patterns, config_dir, &[]);
    expanded.iter().map(|p| normalize_path(p)).collect()
}

/// Collect directory-shaped exclude patterns and return their normalized
/// absolute path with a trailing `/`. The discovery pass uses `starts_with`
/// against this prefix to reject anything under the directory.
///
/// TypeScript's tsconfig semantics treat a bare folder name in `exclude`
/// (e.g. `"node_modules"`, `".next/cache"`) as "exclude this directory and
/// everything inside it." `expand_exclude_set` alone can't express that
/// because `expand_include_patterns` filters to `is_file()` and silently
/// drops directories.
fn expand_exclude_directories(patterns: &[String], config_dir: &Path) -> Vec<String> {
    let mut dirs = Vec::new();
    for pattern in patterns {
        if pattern.contains('*') {
            // Glob patterns are handled by `expand_exclude_set` via the
            // include expander. Skip here.
            continue;
        }
        let path = config_dir.join(pattern);
        if path.is_dir() {
            let mut norm = normalize_path(&path);
            if !norm.ends_with('/') {
                norm.push('/');
            }
            dirs.push(norm);
        }
    }
    dirs
}

/// Recursively collect files matching a suffix pattern (extension).
///
/// `excluded_dirs` are normalized absolute prefixes (trailing `/`) whose
/// subtrees are pruned WITHOUT being read — TypeScript's `exclude` folder
/// semantics. Pruning here (not post-filtering) is what keeps the walk off a
/// project's giant excluded caches (`.next/cache`, `dist`, …).
fn collect_files_matching(
    dir: &Path,
    suffixes: &[String],
    files: &mut Vec<PathBuf>,
    excluded_dirs: &[String],
) {
    // Precompute the extension matchers once. A `*.ts` suffix matches by the
    // `.ts` tail; an empty suffix means "the default TS extensions".
    let exts: Vec<&str> = suffixes
        .iter()
        .filter_map(|s| s.strip_prefix('*'))
        .collect();
    let has_default = suffixes.iter().any(|s| s.is_empty());
    let mut visited = std::collections::HashSet::new();
    collect_files_matching_inner(dir, &exts, has_default, files, &mut visited, excluded_dirs);
}

/// What identifies a directory for cycle detection: its device and inode
/// where the platform exposes them, else its canonical path.
#[cfg(unix)]
type DirIdentity = (u64, u64);
#[cfg(not(unix))]
type DirIdentity = PathBuf;

/// `dir`'s identity, or `None` when it is not a directory.
fn dir_identity(dir: &Path) -> Option<DirIdentity> {
    let meta = std::fs::metadata(dir).ok()?;
    if !meta.is_dir() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    }
    #[cfg(not(unix))]
    {
        std::fs::canonicalize(dir).ok()
    }
}

fn collect_files_matching_inner(
    dir: &Path,
    exts: &[&str],
    has_default: bool,
    files: &mut Vec<PathBuf>,
    visited: &mut std::collections::HashSet<DirIdentity>,
    excluded_dirs: &[String],
) {
    // Skip node_modules — never useful to recurse into.
    if dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "node_modules")
    {
        return;
    }
    // One `stat` both checks the directory and identifies it for symlink
    // cycle detection (a `canonicalize` was a `readlink` per path
    // component, most of a 6 000-file project's discovery time).
    let Some(identity) = dir_identity(dir) else {
        return;
    };
    // Prune excluded subtrees up front — the whole point is to not `readdir`
    // them at all. Match the same normalized-prefix form the post-walk filter
    // uses so behavior is identical, just earlier.
    if !excluded_dirs.is_empty() {
        let mut norm = normalize_path(dir);
        if !norm.ends_with('/') {
            norm.push('/');
        }
        if excluded_dirs.iter().any(|d| norm.starts_with(d.as_str())) {
            return;
        }
    }
    if !visited.insert(identity) {
        return; // Already visited — symlink cycle.
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // `entry.file_type()` reads the type from the directory entry
            // (readdir's `d_type`) without a per-entry `stat`. The old
            // `path.is_dir()` / `path.is_file()` were two stat syscalls each,
            // the dominant cost on a large tree. Fall back to `path.is_dir()`
            // only for the filesystems that return Unknown (rare) / symlinks.
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let path = entry.path();
            let is_dir = ft.is_dir() || (ft.is_symlink() && path.is_dir());
            if is_dir {
                collect_files_matching_inner(
                    &path,
                    exts,
                    has_default,
                    files,
                    visited,
                    excluded_dirs,
                );
                continue;
            }
            let is_file = ft.is_file() || (ft.is_symlink() && path.is_file());
            if !is_file {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // Match if the name ends with any requested extension (`*.ts` →
            // `.ts`), or — for a bare `**` with no suffix — the default TS
            // extensions.
            let matched = exts.iter().any(|e| name.ends_with(e))
                || (has_default
                    && path
                        .extension()
                        .is_some_and(|ext| ext == "ts" || ext == "tsx"));
            if matched {
                files.push(path);
            }
        }
    }
}

/// Simple glob matching: only supports `*` as "match anything".
fn matches_simple_glob(name: &str, pattern: &str) -> bool {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        name.starts_with(prefix)
            && name.ends_with(suffix)
            && name.len() >= prefix.len() + suffix.len()
    } else {
        name == pattern
    }
}

// ---------------------------------------------------------------------------
// Compiler options parsing from JSON
// ---------------------------------------------------------------------------

fn parse_compiler_options(json: &str, opts: &mut CompilerOptions) {
    // Parse known fields
    set_option_target(json, opts);
    set_option_module(json, opts);
    set_option_bool(json, "strict", &mut opts.strict);
    set_option_bool(json, "noImplicitAny", &mut opts.no_implicit_any);
    set_option_bool(json, "noImplicitReturns", &mut opts.no_implicit_returns);
    set_option_bool(json, "noUnusedLocals", &mut opts.no_unused_locals);
    set_option_bool(json, "noUnusedParameters", &mut opts.no_unused_parameters);
    set_option_bool(json, "strictNullChecks", &mut opts.strict_null_checks);
    set_option_bool(json, "strictFunctionTypes", &mut opts.strict_function_types);
    set_option_bool(json, "noLib", &mut opts.no_lib);
    set_option_bool(json, "noEmit", &mut opts.no_emit);
    set_option_bool(json, "declaration", &mut opts.declaration);
    set_option_bool(json, "sourceMap", &mut opts.source_map);
    set_option_bool(json, "inlineSourceMap", &mut opts.inline_source_map);
    set_option_bool(json, "allowJs", &mut opts.allow_js);
    set_option_bool(json, "checkJs", &mut opts.check_js);
    set_option_bool(json, "esModuleInterop", &mut opts.es_module_interop);
    set_option_bool(
        json,
        "allowSyntheticDefaultImports",
        &mut opts.allow_synthetic_default_imports,
    );
    set_option_bool(json, "skipLibCheck", &mut opts.skip_lib_check);
    set_option_bool(
        json,
        "experimentalDecorators",
        &mut opts.experimental_decorators,
    );
    set_option_bool(
        json,
        "emitDecoratorMetadata",
        &mut opts.emit_decorator_metadata,
    );
    set_option_bool(
        json,
        "useDefineForClassFields",
        &mut opts.use_define_for_class_fields,
    );
    set_option_bool(json, "isolatedModules", &mut opts.isolated_modules);
    set_option_bool(json, "preserveConstEnums", &mut opts.preserve_const_enums);
    set_option_bool(json, "resolveJsonModule", &mut opts.resolve_json_module);
    set_option_bool(
        json,
        "noUncheckedIndexedAccess",
        &mut opts.no_unchecked_indexed_access,
    );
    set_option_bool(json, "incremental", &mut opts.incremental);
    set_option_bool(json, "composite", &mut opts.composite);
    set_option_bool(json, "removeComments", &mut opts.remove_comments);
    set_option_bool(
        json,
        "preserveTypeAnnotations",
        &mut opts.preserve_type_annotations,
    );
    set_option_bool(json, "preserveComments", &mut opts.preserve_comments);
    set_option_bool(json, "preserveWhitespace", &mut opts.preserve_whitespace);

    set_option_string(json, "outDir", &mut opts.out_dir);
    set_option_string(json, "outFile", &mut opts.out_file);
    set_option_string(json, "rootDir", &mut opts.root_dir);
    set_option_string(json, "baseUrl", &mut opts.base_url);
    set_option_string(json, "moduleResolution", &mut opts.module_resolution);
    set_option_string(json, "tsBuildInfoFile", &mut opts.ts_build_info_file);

    if let Some(jsx_str) = extract_json_string(json, "jsx") {
        opts.jsx = tsc_rs_ast::JsxEmit::parse(&jsx_str);
    }

    if let Some(lib_str) = extract_json_string_array_raw(json, "lib") {
        opts.lib = lib_str;
    }

    // `types` and `typeRoots` — used by the @types/* auto-discovery pass in
    // `augment_with_at_types`. `extract_json_string_array_raw` distinguishes
    // "field absent" (returns None → auto-discover all) from "explicit `[]`"
    // (returns Some(vec![]) → load nothing).
    if let Some(types) = extract_json_string_array_raw(json, "types") {
        opts.types = Some(types);
    }
    if let Some(roots) = extract_json_string_array_raw(json, "typeRoots") {
        opts.type_roots = Some(roots);
    }

    if let Some(paths_obj) = extract_json_object(json, "paths") {
        opts.paths = Some(paths_obj);
    }
}

fn set_option_target(json: &str, opts: &mut CompilerOptions) {
    if let Some(target_str) = extract_json_string(json, "target") {
        opts.target = tsc_rs_ast::ScriptTarget::parse(&target_str);
    }
}

fn set_option_module(json: &str, opts: &mut CompilerOptions) {
    if let Some(module_str) = extract_json_string(json, "module") {
        opts.module = tsc_rs_ast::ModuleKind::parse(&module_str);
    }
}

fn set_option_bool(json: &str, field: &str, target: &mut Option<bool>) {
    if let Some(val) = extract_json_bool(json, field) {
        *target = Some(val);
    }
}

fn set_option_string(json: &str, field: &str, target: &mut Option<String>) {
    if let Some(val) = extract_json_string(json, field) {
        *target = Some(val);
    }
}

// ---------------------------------------------------------------------------
// Extends handling
// ---------------------------------------------------------------------------

fn resolve_extends_path(extends: &str, config_dir: &Path) -> PathBuf {
    // TypeScript resolves a NON-relative `extends` (one that does not start with
    // `./`, `../`, or `/`) through Node module resolution — walking up
    // `node_modules` — not relative to the config dir. Monorepos lean on this:
    // `extends: "@acme/typescript-config/base.json"` lives at
    // `node_modules/@acme/typescript-config/base.json`. The old
    // `config_dir.join(extends)` produced a path INSIDE the package that never
    // exists, so the whole base config (target, lib, strict flags…) was
    // silently dropped — e.g. `target` fell back below ES2015 and every
    // `for..of` over a Set on a large monorepo project mis-fired TS2802.
    let is_relative =
        extends.starts_with("./") || extends.starts_with("../") || extends.starts_with('/') || {
            // Windows-style / bare `.`; treat a leading `.` component as relative.
            let p = Path::new(extends);
            p.is_absolute()
        };

    if !is_relative {
        // Bare / scoped package specifier → node_modules walk. `-p
        // tsconfig.json` gives an empty relative `config_dir`, which `pop`
        // cannot climb out of: walk from its absolute form.
        let mut dir = std::path::absolute(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
        loop {
            let nm = dir.join("node_modules").join(extends);
            // 1. exact file (the common `.../base.json` case)
            if nm.is_file() {
                return nm;
            }
            // 2. add `.json` (extends without extension)
            if nm.extension().is_none() {
                let with_json = PathBuf::from(format!("{}.json", nm.display()));
                if with_json.is_file() {
                    return with_json;
                }
                // 3. package dir → its tsconfig.json
                let pkg_tsconfig = nm.join("tsconfig.json");
                if pkg_tsconfig.is_file() {
                    return pkg_tsconfig;
                }
            }
            if !dir.pop() {
                break;
            }
        }
        // Fall through to the relative form below so the caller still emits a
        // sensible "not found" against a concrete path.
    }

    let path = config_dir.join(extends);
    if path.extension().is_none() {
        // Try adding .json
        let with_json = PathBuf::from(format!("{}.json", path.display()));
        if with_json.is_file() {
            return with_json;
        }
    }
    path
}

/// Merge a base config with an override config.
/// The override's explicit settings take precedence.
fn merge_configs(base: ParsedCommandLine, override_config: ParsedCommandLine) -> ParsedCommandLine {
    let mut options = base.options;

    // Override fields that are set in the child config
    let o = &override_config.options;
    if o.target.is_some() {
        options.target = o.target;
    }
    if o.module.is_some() {
        options.module = o.module;
    }
    if o.strict.is_some() {
        options.strict = o.strict;
    }
    if o.no_implicit_any.is_some() {
        options.no_implicit_any = o.no_implicit_any;
    }
    if o.no_emit.is_some() {
        options.no_emit = o.no_emit;
    }
    if o.declaration.is_some() {
        options.declaration = o.declaration;
    }
    if o.source_map.is_some() {
        options.source_map = o.source_map;
    }
    if o.inline_source_map.is_some() {
        options.inline_source_map = o.inline_source_map;
    }
    if o.jsx.is_some() {
        options.jsx = o.jsx;
    }
    if o.out_dir.is_some() {
        options.out_dir = o.out_dir.clone();
    }
    if o.out_file.is_some() {
        options.out_file = o.out_file.clone();
    }
    if o.root_dir.is_some() {
        options.root_dir = o.root_dir.clone();
    }
    if o.base_url.is_some() {
        options.base_url = o.base_url.clone();
    }
    if o.module_resolution.is_some() {
        options.module_resolution = o.module_resolution.clone();
    }
    if o.es_module_interop.is_some() {
        options.es_module_interop = o.es_module_interop;
    }
    if o.allow_synthetic_default_imports.is_some() {
        options.allow_synthetic_default_imports = o.allow_synthetic_default_imports;
    }
    if o.skip_lib_check.is_some() {
        options.skip_lib_check = o.skip_lib_check;
    }
    if !o.lib.is_empty() {
        options.lib = o.lib.clone();
    }
    if o.paths.is_some() {
        options.paths = o.paths.clone();
    }
    if o.incremental.is_some() {
        options.incremental = o.incremental;
    }
    if o.composite.is_some() {
        options.composite = o.composite;
    }
    if o.ts_build_info_file.is_some() {
        options.ts_build_info_file = o.ts_build_info_file.clone();
    }
    // Every other option a child sets also overrides its base. (These were
    // dropped: a child's `types`, `strictNullChecks`, `allowJs`, … had no
    // effect under `extends`.)
    macro_rules! override_set {
        ($($field:ident),* $(,)?) => {
            $(if o.$field.is_some() {
                options.$field = o.$field.clone();
            })*
        };
    }
    override_set!(
        no_implicit_returns,
        no_unused_locals,
        no_unused_parameters,
        strict_null_checks,
        strict_function_types,
        strict_bind_call_apply,
        strict_property_initialization,
        jsx_factory,
        jsx_fragment_factory,
        jsx_import_source,
        types,
        type_roots,
        allow_js,
        check_js,
        allow_importing_ts_extensions,
        skip_default_lib_check,
        no_lib,
        exact_optional_property_types,
        no_emit_on_error,
        isolated_modules,
        preserve_const_enums,
        filename,
        no_error_truncation,
        experimental_decorators,
        emit_decorator_metadata,
        use_define_for_class_fields,
        module_detection,
        verbatim_module_syntax,
        no_check,
        no_resolve,
        down_level_iteration,
        import_helpers,
        emit_bom,
        new_line,
        remove_comments,
        no_emit_helpers,
        always_strict,
        pretty,
        no_fallthrough_cases_in_switch,
        allow_unreachable_code,
        force_consistent_casing_in_file_names,
        use_unknown_in_catch_variables,
        no_property_access_from_index_signature,
        no_unchecked_indexed_access,
        resolve_json_module,
        isolated_declarations,
        emit_declaration_only,
        imports_not_used_as_values,
        preserve_type_annotations,
        preserve_comments,
        preserve_whitespace,
        fast_emit,
    );
    // Unmodelled options: the child's value replaces the base's by name.
    for (name, value) in &o.other {
        options.other.retain(|(existing, _)| existing != name);
        options.other.push((name.clone(), value.clone()));
    }

    // File names come from the child config
    let file_names = if override_config.file_names.is_empty() {
        base.file_names
    } else {
        override_config.file_names
    };

    let mut errors = base.errors;
    errors.extend(override_config.errors);

    ParsedCommandLine {
        options,
        file_names,
        errors,
    }
}

// ---------------------------------------------------------------------------
// Project references
// ---------------------------------------------------------------------------

fn parse_references_field(content: &str) -> Vec<ProjectReference> {
    let mut refs = Vec::new();

    // Find "references" array
    let search = "\"references\"";
    let Some(idx) = content.find(search) else {
        return refs;
    };
    let after = &content[idx + search.len()..];
    let Some(arr_start) = after.find('[') else {
        return refs;
    };
    let after_arr = &after[arr_start + 1..];

    // Parse each object in the array
    let mut remaining = after_arr;
    while let Some(obj_start) = remaining.find('{') {
        let after_obj = &remaining[obj_start + 1..];
        let Some(obj_end) = after_obj.find('}') else {
            break;
        };
        let obj_content = &after_obj[..obj_end];

        let path = extract_json_string(obj_content, "path").unwrap_or_default();
        let prepend = extract_json_bool(obj_content, "prepend").unwrap_or(false);

        if !path.is_empty() {
            refs.push(ProjectReference { path, prepend });
        }

        remaining = &after_obj[obj_end + 1..];
        if remaining.starts_with(']') {
            break;
        }
    }

    refs
}

// ---------------------------------------------------------------------------
// Minimal JSON helpers (no serde dependency)
// ---------------------------------------------------------------------------

/// Extract a string value for a key from JSON.
fn extract_json_string(json: &str, field: &str) -> Option<String> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start();
    let after_colon = after_colon.strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    let after_colon = after_colon.strip_prefix('"')?;
    let end = after_colon.find('"')?;
    Some(after_colon[..end].to_string())
}

/// Extract a boolean value for a key from JSON.
fn extract_json_bool(json: &str, field: &str) -> Option<bool> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start();
    let after_colon = after_colon.strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    if after_colon.starts_with("true") {
        Some(true)
    } else if after_colon.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Extract an array of strings for a key from JSON.
fn extract_json_string_array(json: &str, field: &str) -> Vec<String> {
    extract_json_string_array_raw(json, field).unwrap_or_default()
}

fn extract_json_string_array_raw(json: &str, field: &str) -> Option<Vec<String>> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    let after_arr = after_colon.strip_prefix('[')?;
    let arr_end = after_arr.find(']')?;
    let arr_content = &after_arr[..arr_end];

    let mut result = Vec::new();
    let mut remaining = arr_content;
    while let Some(str_start) = remaining.find('"') {
        remaining = &remaining[str_start + 1..];
        if let Some(str_end) = remaining.find('"') {
            result.push(remaining[..str_end].to_string());
            remaining = &remaining[str_end + 1..];
        } else {
            break;
        }
    }

    Some(result)
}

/// Extract a nested JSON object as a raw string.
fn extract_json_object(json: &str, field: &str) -> Option<String> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start();

    if !after_colon.starts_with('{') {
        return None;
    }

    let mut depth = 0i32;
    let mut end_idx = 0;
    for (i, ch) in after_colon.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = i;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth == 0 && end_idx > 0 {
        Some(after_colon[..=end_idx].to_string())
    } else {
        None
    }
}

fn normalize_path(path: &Path) -> String {
    tsc_rs_resolver::dedupe_package_file(
        tsc_rs_resolver::canonicalize(path)
            .unwrap_or_else(|| path.to_path_buf())
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// The discovered files under `root`. Default discovery also loads every
    /// ancestor's `node_modules/@types` (as tsc does), so a test project in a
    /// temp dir sees whatever typings the machine has above it (a
    /// `/tmp/node_modules`, `~/node_modules`).
    fn own_files(file_names: &[String], root: &Path) -> Vec<String> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let prefix = root.to_string_lossy().replace('\\', "/");
        file_names
            .iter()
            .filter(|f| f.starts_with(prefix.as_str()))
            .cloned()
            .collect()
    }

    #[test]
    fn parse_simple_tsconfig() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": {
                    "target": "es2015",
                    "module": "commonjs",
                    "strict": true,
                    "outDir": "./dist"
                },
                "files": ["src/main.ts"]
            }"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.ts"), "const x = 1;").unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        let own = own_files(&result.file_names, &root);
        assert!(result.options.target.is_some());
        assert!(result.options.module.is_some());
        assert_eq!(result.options.strict, Some(true));
        assert_eq!(result.options.out_dir.as_deref(), Some("./dist"));
        assert_eq!(own.len(), 1);
        assert!(own[0].contains("main.ts"));
    }

    #[test]
    fn parse_tsconfig_with_include() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), "const a = 1;").unwrap();
        fs::write(root.join("src/b.ts"), "const b = 2;").unwrap();
        fs::write(root.join("src/c.js"), "const c = 3;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "target": "es2015" },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        let own = own_files(&result.file_names, &root);
        assert_eq!(own.len(), 2);
        assert!(own.iter().any(|f| f.contains("a.ts")));
        assert!(own.iter().any(|f| f.contains("b.ts")));
    }

    #[test]
    fn parse_tsconfig_with_bare_path_resolves_cwd_relative_globs() {
        // Regression: invoking tsc-rs with no `-p` flag passes "tsconfig.json"
        // verbatim to parse_tsconfig. `Path::parent()` on a bare filename
        // returns Some("") (not None), so the older
        // `parent().unwrap_or(Path::new("."))` fallback never fired and
        // recursive globs like `**/*.ts` resolved their base to "" → not a
        // directory → 0 files. Only directory-prefixed globs (e.g.
        // `.next/types/**/*.ts`) accidentally worked.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), "const a = 1;").unwrap();
        fs::write(root.join("src/b.ts"), "const b = 2;").unwrap();
        fs::write(root.join("tsconfig.json"), r#"{ "include": ["**/*.ts"] }"#).unwrap();

        // chdir into the test root so the bare "tsconfig.json" path resolves
        // relative to it — this is the codepath the CLI takes.
        let prev_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(root).unwrap();
        let result = parse_tsconfig("tsconfig.json");
        std::env::set_current_dir(prev_cwd).unwrap();

        let parsed = result.unwrap();
        assert!(
            parsed.file_names.iter().any(|f| f.ends_with("a.ts")),
            "expected a.ts in {:?}",
            parsed.file_names
        );
        assert!(
            parsed.file_names.iter().any(|f| f.ends_with("b.ts")),
            "expected b.ts in {:?}",
            parsed.file_names
        );
    }

    #[test]
    fn parse_tsconfig_exclude_directory_prunes_subtree() {
        // Regression: TypeScript treats bare folder entries in `exclude`
        // (e.g. "node_modules", ".next/cache") as "prune this whole subtree."
        // Our discovery used to filter excludes via exact-match against the
        // file list, which never matches a directory path → every .ts file
        // under .next/dev/server, .next/dev/static, etc. was walked and
        // type-checked on real Next.js projects.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/keep.ts"), "const k = 1;").unwrap();
        fs::create_dir_all(root.join("dist/inner")).unwrap();
        fs::write(root.join("dist/skip.ts"), "const s = 1;").unwrap();
        fs::write(root.join("dist/inner/skip2.ts"), "const s2 = 1;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "include": ["**/*.ts"],
                "exclude": ["dist"]
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        assert!(
            result.file_names.iter().any(|f| f.ends_with("keep.ts")),
            "expected keep.ts to be discovered: {:?}",
            result.file_names
        );
        assert!(
            !result.file_names.iter().any(|f| f.contains("/dist/")),
            "dist/ files should be pruned: {:?}",
            result.file_names
        );
    }

    #[test]
    fn multiple_recursive_suffixes_share_one_walk_and_match_all() {
        // The Next.js tsconfig lists `**/*.mts`, `**/*.ts`, `**/*.tsx` — all
        // rooted at config_dir. These are now grouped into a single tree walk
        // that matches ANY of the three suffixes (was one full walk per
        // pattern). The observable contract: every extension is still found,
        // each file exactly once, and non-matching extensions are excluded.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/one.ts"), "const a = 1;").unwrap();
        fs::write(root.join("a/two.tsx"), "const b = 1;").unwrap();
        fs::write(root.join("a/b/three.mts"), "const c = 1;").unwrap();
        fs::write(root.join("a/b/four.ts"), "const d = 1;").unwrap();
        fs::write(root.join("a/b/skip.js"), "const e = 1;").unwrap();
        fs::write(root.join("a/b/skip.d.mts.map"), "{}").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{ "include": ["**/*.mts", "**/*.ts", "**/*.tsx"] }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        let ts: Vec<&String> = result
            .file_names
            .iter()
            .filter(|f| !f.contains("node_modules"))
            .collect();
        for want in ["one.ts", "two.tsx", "three.mts", "four.ts"] {
            assert!(
                ts.iter().any(|f| f.ends_with(want)),
                "expected {want} discovered: {:?}",
                ts
            );
        }
        assert!(
            !ts.iter().any(|f| f.ends_with(".js")),
            ".js must not be included: {:?}",
            ts
        );
        // Each file exactly once (the group must not double-count a file that
        // could be reached by more than one base).
        let mut sorted = ts.clone();
        sorted.sort();
        let mut dedup = sorted.clone();
        dedup.dedup();
        assert_eq!(sorted.len(), dedup.len(), "duplicate entries: {:?}", sorted);
    }

    #[test]
    fn extends_resolves_a_package_name_config_through_node_modules() {
        // Regression: a non-relative `extends` (a bare/scoped package specifier)
        // resolves via node_modules, not relative to the config dir. The whole
        // a pnpm monorepo does
        // `extends: "@acme/typescript-config/base.json"`; the old
        // `config_dir.join(extends)` produced a path inside the package that
        // never existed, so the base config was silently dropped — `target`
        // fell back below ES2015 and every `for..of`/spread of a Set fired a
        // bogus TS2802 across the router package.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let base_dir = root.join("node_modules/@scope/tsconfig");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(
            base_dir.join("base.json"),
            r#"{ "compilerOptions": { "target": "ES2022", "strict": true } }"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), "const a = 1;").unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "@scope/tsconfig/base.json",
                "compilerOptions": { "noEmit": true },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        // The base's `target` must be merged in — proof the package-name
        // `extends` resolved.
        assert!(
            result.options.target.is_some(),
            "target from the package-name base must be applied: {:?}",
            result.options
        );
        assert_eq!(result.options.strict, Some(true));
        assert!(
            !result.errors.iter().any(|e| e.code == 6053),
            "extends must not report a missing file: {:?}",
            result.errors
        );
    }

    #[test]
    fn imported_type_alias_honors_default_type_params() {
        // Regression: an imported type alias whose params all have defaults —
        // e.g. Prisma's `type PrismaClient<L = never, O = ..., E = DefaultArgs>`
        // used as a bare `PrismaClient` — mis-fired TS2314 "requires 3 type
        // argument(s)". The same-file alias path counted defaults; the cross-file
        // INJECTION path hard-coded `required = type_params.len()`.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/dts.d.ts"),
            "export type Client<A extends string = never, B = undefined, C extends object = {}> = { q(): Promise<A>; _b: B; _c: C };\n",
        )
        .unwrap();
        fs::write(
            root.join("src/use.ts"),
            "import type { Client } from \"./dts\";\nexport function f(ctx: { db: Client }): void { void ctx; }\n",
        )
        .unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "strict": true, "noEmit": true, "skipLibCheck": true },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let project = TsProject::from_config(root.join("tsconfig.json").to_str().unwrap())
            .expect("project should load");
        let session = project.open_check_session();
        let use_path = root.join("src/use.ts");
        let diagnostics = session.check_file(use_path.to_str().unwrap(), None);
        assert!(
            !diagnostics.iter().any(|d| d.code == 2314),
            "bare use of an all-defaulted imported alias must not require type args: {:?}",
            diagnostics
        );
    }

    #[test]
    fn parse_tsconfig_with_no_lib() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("main.ts"), "const x = 1;").unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "noLib": true }
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        assert_eq!(result.options.no_lib, Some(true));
    }

    #[test]
    fn parse_tsconfig_with_exclude() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("test")).unwrap();
        fs::write(root.join("src/main.ts"), "const x = 1;").unwrap();
        fs::write(root.join("test/test.ts"), "const y = 2;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "target": "es2015" },
                "include": ["src/**/*.ts", "test/**/*.ts"],
                "exclude": ["test/**/*.ts"]
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        let own = own_files(&result.file_names, &root);
        assert_eq!(own.len(), 1);
        assert!(own[0].contains("main.ts"));
    }

    #[test]
    fn parse_tsconfig_extends() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(
            root.join("base.json"),
            r#"{
                "compilerOptions": {
                    "target": "es2015",
                    "strict": true
                }
            }"#,
        )
        .unwrap();

        fs::write(root.join("main.ts"), "const x = 1;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./base",
                "compilerOptions": {
                    "module": "commonjs"
                }
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        // Should inherit target from base
        assert!(result.options.target.is_some());
        // Should have module from child
        assert!(result.options.module.is_some());
        // Should inherit strict from base
        assert_eq!(result.options.strict, Some(true));
    }

    #[test]
    fn extends_child_overrides_every_option_and_inherits_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.ts"), "const x = 1;").unwrap();
        fs::write(
            root.join("base.json"),
            r#"{
                "compilerOptions": { "strict": true, "strictNullChecks": true, "types": ["node"] },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();
        // No `include` of its own; `types: []` also keeps the result free
        // of whatever @types the temp dir's ancestors hold.
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./base.json",
                "compilerOptions": {
                    "strictNullChecks": false,
                    "types": [],
                    "experimentalDecorators": true
                }
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        assert_eq!(result.options.strict, Some(true));
        assert_eq!(result.options.strict_null_checks, Some(false));
        assert_eq!(result.options.types.as_deref(), Some(&[][..]));
        assert_eq!(result.options.experimental_decorators, Some(true));
        // The base's `include` is inherited.
        assert_eq!(result.file_names.len(), 1);
        assert!(result.file_names[0].ends_with("main.ts"));
    }

    #[test]
    fn parse_project_references_basic() {
        let content = r#"{
            "references": [
                { "path": "./packages/core" },
                { "path": "./packages/utils", "prepend": true }
            ]
        }"#;

        let refs = parse_references_field(content);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "./packages/core");
        assert!(!refs[0].prepend);
        assert_eq!(refs[1].path, "./packages/utils");
        assert!(refs[1].prepend);
    }

    #[test]
    fn extract_json_helpers() {
        let json = r#"{ "target": "es2015", "strict": true, "outDir": "./dist" }"#;
        assert_eq!(
            extract_json_string(json, "target"),
            Some("es2015".to_string())
        );
        assert_eq!(extract_json_bool(json, "strict"), Some(true));
        assert_eq!(
            extract_json_string(json, "outDir"),
            Some("./dist".to_string())
        );
        assert_eq!(extract_json_string(json, "missing"), None);
    }

    #[test]
    fn extract_json_array_helper() {
        let json = r#"{ "files": ["a.ts", "b.ts", "c.ts"] }"#;
        let arr = extract_json_string_array(json, "files");
        assert_eq!(arr, vec!["a.ts", "b.ts", "c.ts"]);
    }

    #[test]
    fn extract_json_object_helper() {
        let json = r#"{ "compilerOptions": { "target": "es2015" }, "files": [] }"#;
        let obj = extract_json_object(json, "compilerOptions");
        assert!(obj.is_some());
        let obj_str = obj.unwrap();
        assert!(obj_str.contains("target"));
    }

    #[test]
    fn default_file_discovery() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("a.ts"), "const a = 1;").unwrap();
        fs::write(root.join("b.tsx"), "const b = 2;").unwrap();
        fs::write(root.join("c.js"), "const c = 3;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{ "compilerOptions": { "target": "es2015" } }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        let own = own_files(&result.file_names, &root);
        // Should find .ts and .tsx but not .js
        assert_eq!(own.len(), 2);
    }

    #[test]
    fn missing_tsconfig_returns_error() {
        let result = parse_tsconfig("/nonexistent/tsconfig.json");
        assert!(result.is_err());
    }

    #[test]
    fn simple_glob_matching() {
        assert!(matches_simple_glob("foo.ts", "*.ts"));
        assert!(matches_simple_glob("bar.tsx", "*.tsx"));
        assert!(!matches_simple_glob("foo.js", "*.ts"));
        assert!(matches_simple_glob("test.spec.ts", "*.ts"));
        assert!(matches_simple_glob("test.spec.ts", "test.*"));
    }

    // -----------------------------------------------------------------------
    // Incremental build info tests
    // -----------------------------------------------------------------------

    #[test]
    fn tsbuildinfo_roundtrip() {
        let info = TsBuildInfo {
            version: "0.1.0".to_string(),
            file_hashes: {
                let mut m = HashMap::new();
                m.insert("/src/a.ts".to_string(), "abc123".to_string());
                m.insert("/src/b.ts".to_string(), "def456".to_string());
                m
            },
            options_hash: "opts_hash_1".to_string(),
            dependencies: {
                let mut m = HashMap::new();
                m.insert("/src/a.ts".to_string(), vec!["/src/b.ts".to_string()]);
                m
            },
        };

        let json = info.to_json();
        assert!(json.contains("\"version\": \"0.1.0\""));
        assert!(json.contains("abc123"));
        assert!(json.contains("def456"));
        assert!(json.contains("opts_hash_1"));

        let parsed = TsBuildInfo::parse_json(&json).expect("parse should succeed");
        assert_eq!(parsed.version, "0.1.0");
        assert_eq!(parsed.file_hashes.len(), 2);
        assert_eq!(parsed.file_hashes.get("/src/a.ts").unwrap(), "abc123");
        assert_eq!(parsed.file_hashes.get("/src/b.ts").unwrap(), "def456");
        assert_eq!(parsed.options_hash, "opts_hash_1");
        assert_eq!(parsed.dependencies.len(), 1);
        assert_eq!(
            parsed.dependencies.get("/src/a.ts").unwrap(),
            &vec!["/src/b.ts".to_string()]
        );
    }

    #[test]
    fn tsbuildinfo_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let info_path = dir.path().join("test.tsbuildinfo");
        let info_path_str = info_path.to_str().unwrap();

        let info = TsBuildInfo {
            version: "0.1.0".to_string(),
            file_hashes: {
                let mut m = HashMap::new();
                m.insert("file1.ts".to_string(), "hash1".to_string());
                m
            },
            options_hash: "opt_hash".to_string(),
            dependencies: HashMap::new(),
        };

        info.save(info_path_str).expect("save should succeed");
        assert!(info_path.exists());

        let loaded = TsBuildInfo::load(info_path_str).expect("load should succeed");
        assert_eq!(loaded.version, "0.1.0");
        assert_eq!(loaded.file_hashes.get("file1.ts").unwrap(), "hash1");
        assert_eq!(loaded.options_hash, "opt_hash");
    }

    #[test]
    fn simple_hash_deterministic() {
        let h1 = simple_hash("hello world");
        let h2 = simple_hash("hello world");
        let h3 = simple_hash("hello world!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn incremental_skip_unchanged_files() {
        // Simulate: first build creates hashes, second build with same content
        // should produce the same hashes
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("a.ts"), "const a: number = 1;").unwrap();
        fs::write(root.join("b.ts"), "const b: string = 'hello';").unwrap();

        let hash_a1 = simple_hash(&fs::read_to_string(root.join("a.ts")).unwrap());
        let hash_b1 = simple_hash(&fs::read_to_string(root.join("b.ts")).unwrap());

        // "Second build" - same content
        let hash_a2 = simple_hash(&fs::read_to_string(root.join("a.ts")).unwrap());
        let hash_b2 = simple_hash(&fs::read_to_string(root.join("b.ts")).unwrap());

        assert_eq!(hash_a1, hash_a2, "unchanged file should have same hash");
        assert_eq!(hash_b1, hash_b2, "unchanged file should have same hash");

        // Modify a file
        fs::write(root.join("a.ts"), "const a: number = 42;").unwrap();
        let hash_a3 = simple_hash(&fs::read_to_string(root.join("a.ts")).unwrap());
        assert_ne!(hash_a1, hash_a3, "changed file should have different hash");
    }

    #[test]
    fn incremental_dependency_invalidation() {
        // Test that dependency tracking correctly identifies when to recompile
        let prev = TsBuildInfo {
            version: "0.1.0".to_string(),
            file_hashes: {
                let mut m = HashMap::new();
                m.insert("a.ts".to_string(), "hash_a".to_string());
                m.insert("b.ts".to_string(), "hash_b".to_string());
                m
            },
            options_hash: "opts".to_string(),
            dependencies: {
                let mut m = HashMap::new();
                // a.ts depends on b.ts
                m.insert("a.ts".to_string(), vec!["b.ts".to_string()]);
                m
            },
        };

        // b.ts changed - a.ts should be invalidated because it depends on b.ts
        let mut current_hashes = HashMap::new();
        current_hashes.insert("a.ts".to_string(), "hash_a".to_string()); // unchanged
        current_hashes.insert("b.ts".to_string(), "hash_b_changed".to_string()); // changed

        let files = vec!["a.ts".to_string(), "b.ts".to_string()];
        let files_to_compile: Vec<String> = files
            .iter()
            .filter(|f| {
                let current = current_hashes.get(*f);
                let previous_hash = prev.file_hashes.get(*f);
                match (current, previous_hash) {
                    (Some(c), Some(p)) => {
                        if c != p {
                            return true;
                        }
                        if let Some(deps) = prev.dependencies.get(*f) {
                            for dep in deps {
                                let dep_current = current_hashes.get(dep);
                                let dep_prev = prev.file_hashes.get(dep);
                                if dep_current != dep_prev {
                                    return true;
                                }
                            }
                        }
                        false
                    }
                    _ => true,
                }
            })
            .cloned()
            .collect();

        // Both should be recompiled: b.ts changed directly, a.ts depends on b.ts
        assert!(files_to_compile.contains(&"a.ts".to_string()));
        assert!(files_to_compile.contains(&"b.ts".to_string()));
    }

    // -----------------------------------------------------------------------
    // Project reference ordering tests
    // -----------------------------------------------------------------------

    #[test]
    fn topological_sort_simple() {
        let nodes = vec!["root".to_string(), "lib-a".to_string(), "lib-b".to_string()];
        let mut adj = HashMap::new();
        // root depends on lib-a and lib-b
        adj.insert(
            "root".to_string(),
            vec!["lib-a".to_string(), "lib-b".to_string()],
        );

        let order = topological_sort(&nodes, &adj).unwrap();
        // lib-a and lib-b should come before root
        let root_pos = order.iter().position(|x| x == "root").unwrap();
        let lib_a_pos = order.iter().position(|x| x == "lib-a").unwrap();
        let lib_b_pos = order.iter().position(|x| x == "lib-b").unwrap();
        assert!(lib_a_pos < root_pos);
        assert!(lib_b_pos < root_pos);
    }

    #[test]
    fn topological_sort_chain() {
        let nodes = vec!["app".to_string(), "core".to_string(), "utils".to_string()];
        let mut adj = HashMap::new();
        // app -> core -> utils
        adj.insert("app".to_string(), vec!["core".to_string()]);
        adj.insert("core".to_string(), vec!["utils".to_string()]);

        let order = topological_sort(&nodes, &adj).unwrap();
        let app_pos = order.iter().position(|x| x == "app").unwrap();
        let core_pos = order.iter().position(|x| x == "core").unwrap();
        let utils_pos = order.iter().position(|x| x == "utils").unwrap();
        assert!(utils_pos < core_pos);
        assert!(core_pos < app_pos);
    }

    #[test]
    fn topological_sort_circular_detected() {
        let nodes = vec!["a".to_string(), "b".to_string()];
        let mut adj = HashMap::new();
        adj.insert("a".to_string(), vec!["b".to_string()]);
        adj.insert("b".to_string(), vec!["a".to_string()]);

        let result = topological_sort(&nodes, &adj);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular"));
    }

    #[test]
    fn topological_sort_no_deps() {
        let nodes = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let adj = HashMap::new();

        let order = topological_sort(&nodes, &adj).unwrap();
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn resolve_build_info_path_explicit() {
        let mut opts = CompilerOptions::default();
        opts.ts_build_info_file = Some("/custom/path.tsbuildinfo".to_string());
        assert_eq!(
            resolve_build_info_path(&opts, Some("tsconfig.json")),
            "/custom/path.tsbuildinfo"
        );
    }

    #[test]
    fn resolve_build_info_path_from_outdir() {
        let mut opts = CompilerOptions::default();
        opts.out_dir = Some("./dist".to_string());
        let result = resolve_build_info_path(&opts, Some("tsconfig.json"));
        assert!(result.contains("dist"));
        assert!(result.contains("tsbuildinfo"));
    }

    #[test]
    fn resolve_build_info_path_default() {
        let opts = CompilerOptions::default();
        let result = resolve_build_info_path(&opts, Some("tsconfig.json"));
        assert!(result.contains("tsconfig.tsbuildinfo"));
    }

    #[test]
    fn parse_incremental_composite_options() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("a.ts"), "const a = 1;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": {
                    "target": "es2015",
                    "incremental": true,
                    "composite": true,
                    "tsBuildInfoFile": "./build/.tsbuildinfo"
                }
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        assert_eq!(result.options.incremental, Some(true));
        assert_eq!(result.options.composite, Some(true));
        assert_eq!(
            result.options.ts_build_info_file.as_deref(),
            Some("./build/.tsbuildinfo")
        );
    }

    #[test]
    fn hash_compiler_options_changes() {
        let opts1 = CompilerOptions::default();
        let mut opts2 = CompilerOptions::default();
        opts2.strict = Some(true);

        let h1 = hash_compiler_options(&opts1);
        let h2 = hash_compiler_options(&opts2);
        assert_ne!(h1, h2, "different options should produce different hashes");
    }

    #[test]
    fn parse_tsconfig_reports_missing_explicit_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "files": ["src/missing.ts"]
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        let own = own_files(&result.file_names, &root);
        assert_eq!(own.len(), 0);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].code, 6053);
    }

    #[test]
    fn parse_tsconfig_missing_extends_adds_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("main.ts"), "const x = 1;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./does-not-exist",
                "files": ["main.ts"]
            }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        assert!(!result.errors.is_empty());
        assert!(result
            .errors
            .iter()
            .any(|e| e.code == 6053 && e.message.contains("does-not-exist")));
    }

    #[test]
    fn parse_project_references_empty_when_not_present() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("tsconfig.json"),
            r#"{ "compilerOptions": { "target": "es2015" } }"#,
        )
        .unwrap();

        let refs = parse_project_references(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn resolve_build_info_path_without_config_uses_default_name() {
        let opts = CompilerOptions::default();
        let result = resolve_build_info_path(&opts, None);
        assert!(result.ends_with("tsconfig.tsbuildinfo"));
    }

    #[test]
    fn build_project_references_orders_dependencies_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("lib/src")).unwrap();
        fs::create_dir_all(root.join("app/src")).unwrap();

        fs::write(root.join("lib/src/lib.ts"), "export const n: number = 1;").unwrap();
        fs::write(
            root.join("lib/tsconfig.json"),
            r#"{
                "compilerOptions": { "target": "es2015", "module": "commonjs" },
                "files": ["src/lib.ts"]
            }"#,
        )
        .unwrap();

        fs::write(
            root.join("app/src/main.ts"),
            "import { n } from '../lib/src/lib';\nexport const v = n + 1;",
        )
        .unwrap();
        fs::write(
            root.join("app/tsconfig.json"),
            r#"{
                "compilerOptions": { "target": "es2015", "module": "commonjs" },
                "references": [{ "path": "../lib" }],
                "files": ["src/main.ts"]
            }"#,
        )
        .unwrap();

        let result = build_project_references(root.join("app/tsconfig.json").to_str().unwrap())
            .expect("expected reference build to succeed");
        assert_eq!(result.build_order.len(), 2);

        let app_cfg = normalize_path(&root.join("app/tsconfig.json"));
        let lib_cfg = normalize_path(&root.join("lib/tsconfig.json"));
        let app_pos = result
            .build_order
            .iter()
            .position(|p| p == &app_cfg)
            .unwrap();
        let lib_pos = result
            .build_order
            .iter()
            .position(|p| p == &lib_cfg)
            .unwrap();
        assert!(lib_pos < app_pos, "dependency project should build first");
    }

    #[test]
    fn compile_incremental_rebuilds_all_when_options_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = root.join("a.ts");
        let b = root.join("b.ts");
        fs::write(&a, "export const a: number = 1;").unwrap();
        fs::write(&b, "export const b: number = 2;").unwrap();

        let a_str = a.to_string_lossy().to_string();
        let b_str = b.to_string_lossy().to_string();
        let build_info_path = root.join(".cache/tsconfig.tsbuildinfo");
        let build_info_path_str = build_info_path.to_string_lossy().to_string();

        let base_opts = CompilerOptions::default();
        let base_project = TsProject::new(vec![a_str.clone(), b_str.clone()], base_opts.clone());
        let (_first_result, first_info) = base_project.compile_incremental(&build_info_path_str);
        first_info.save(&build_info_path_str).unwrap();

        let mut changed_opts = base_opts;
        changed_opts.strict = Some(true);
        let changed_project = TsProject::new(vec![a_str, b_str], changed_opts);
        let (result, _info) = changed_project.compile_incremental(&build_info_path_str);

        assert_eq!(
            result.files.len(),
            2,
            "options change should force full rebuild"
        );
    }

    #[test]
    fn tsbuildinfo_load_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.tsbuildinfo");
        let loaded = TsBuildInfo::load(missing.to_str().unwrap());
        assert!(loaded.is_none());
    }

    #[test]
    fn tsbuildinfo_parse_invalid_json_returns_none() {
        let content = r#"{ "fileHashes": { "a.ts": "x" } }"#;
        let parsed = TsBuildInfo::parse_json(content);
        assert!(
            parsed.is_none(),
            "version is required for valid tsbuildinfo"
        );
    }

    #[test]
    fn parse_project_references_missing_file_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope-tsconfig.json");
        let result = parse_project_references(missing.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn topological_sort_ignores_unknown_dependency_nodes() {
        let nodes = vec!["app".to_string(), "lib".to_string()];
        let mut adj = HashMap::new();
        adj.insert(
            "app".to_string(),
            vec!["lib".to_string(), "ghost".to_string()],
        );

        let order = topological_sort(&nodes, &adj).unwrap();
        assert_eq!(order.len(), 2);
        let app_pos = order.iter().position(|x| x == "app").unwrap();
        let lib_pos = order.iter().position(|x| x == "lib").unwrap();
        assert!(lib_pos < app_pos);
    }

    #[test]
    fn default_discovery_is_not_recursive() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("top.ts"), "const t = 1;").unwrap();
        fs::write(root.join("src/nested/deep.ts"), "const d = 1;").unwrap();

        fs::write(
            root.join("tsconfig.json"),
            r#"{ "compilerOptions": { "target": "es2015" } }"#,
        )
        .unwrap();

        let result = parse_tsconfig(root.join("tsconfig.json").to_str().unwrap()).unwrap();
        let own = own_files(&result.file_names, &root);
        assert_eq!(own.len(), 1);
        assert!(own[0].ends_with("/top.ts"));
    }

    #[test]
    fn compile_reports_missing_file_and_has_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let missing = root.join("missing.ts").to_string_lossy().to_string();

        let project = TsProject::new(vec![missing], CompilerOptions::default());
        let result = project.compile();

        assert_eq!(result.files.len(), 0);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, 6053);
        assert!(result.has_errors());
        assert_eq!(result.total_diagnostics(), 1);
    }

    #[test]
    fn compile_reports_missing_module_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let main = root.join("main.ts");
        fs::write(
            &main,
            "import { value } from \"./missing\";\nconsole.log(value);\n",
        )
        .unwrap();

        let project = TsProject::new(
            vec![main.to_string_lossy().to_string()],
            CompilerOptions::default(),
        );
        let result = project.compile();
        let main_output = result
            .files
            .iter()
            .find(|file| file.file_name == main.to_string_lossy())
            .expect("expected main.ts output");

        assert!(
            main_output.diagnostics.iter().any(|diag| diag.code == 2307),
            "expected TS2307 in diagnostics: {:?}",
            main_output.diagnostics
        );
    }

    #[test]
    fn compile_allows_nested_relative_module_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let app = root.join("app.ts");
        let nested_dir = root.join("main");
        let consume = nested_dir.join("consume.ts");

        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(&consume, "export function call() { return 1; }\n").unwrap();
        fs::write(
            &app,
            "import consume = require(\"./main/consume\");\nconsume.call();\n",
        )
        .unwrap();

        let project = TsProject::new(
            vec![
                app.to_string_lossy().to_string(),
                consume.to_string_lossy().to_string(),
            ],
            CompilerOptions::default(),
        );
        let result = project.compile();
        let app_output = result
            .files
            .iter()
            .find(|file| file.file_name == app.to_string_lossy())
            .expect("expected app.ts output");

        assert!(
            !app_output.diagnostics.iter().any(|diag| diag.code == 2307),
            "did not expect nested relative import to report TS2307: {:?}",
            app_output.diagnostics
        );
    }

    #[test]
    fn compile_reports_strict_null_access_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let main = root.join("main.ts");
        fs::write(&main, "let x: { a: number } | null = null;\nx.a;\n").unwrap();

        let mut options = CompilerOptions::default();
        options.strict_null_checks = Some(true);

        let project = TsProject::new(vec![main.to_string_lossy().to_string()], options);
        let result = project.compile();
        let main_output = result
            .files
            .iter()
            .find(|file| file.file_name == main.to_string_lossy())
            .expect("expected main.ts output");

        assert!(
            main_output
                .diagnostics
                .iter()
                .any(|diag| diag.code == 18047),
            "expected TS18047 in diagnostics: {:?}",
            main_output.diagnostics
        );
    }

    #[test]
    fn compile_reports_cross_file_import_type_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let decl = root.join("decl.ts");
        let main = root.join("main.ts");

        fs::write(&decl, "export interface Point { x: number; y: number; }\n").unwrap();
        fs::write(
            &main,
            "import { Point } from \"./decl\";\nconst p: Point = { x: \"oops\", y: 1 };\n",
        )
        .unwrap();

        let project = TsProject::new(
            vec![
                decl.to_string_lossy().to_string(),
                main.to_string_lossy().to_string(),
            ],
            CompilerOptions::default(),
        );
        let result = project.compile();
        let main_output = result
            .files
            .iter()
            .find(|file| file.file_name == main.to_string_lossy())
            .expect("expected main.ts output");

        assert!(
            main_output.diagnostics.iter().any(|diag| diag.code == 2322),
            "expected imported interface mismatch to report TS2322: {:?}",
            main_output.diagnostics
        );
    }

    #[test]
    fn compile_reports_duplicate_identifier_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("dup.ts");

        fs::write(&file, "class Duplicate {}\nclass Duplicate {}\n").unwrap();

        let project = TsProject::new(
            vec![file.to_string_lossy().to_string()],
            CompilerOptions::default(),
        );
        let result = project.compile();
        let output = result
            .files
            .iter()
            .find(|compiled| compiled.file_name == file.to_string_lossy())
            .expect("expected dup.ts output");

        assert!(
            output.diagnostics.iter().any(|diag| diag.code == 2300
                && diag.message.contains("Duplicate identifier 'Duplicate'")),
            "expected TS2300 duplicate identifier diagnostics: {:?}",
            output.diagnostics
        );
    }

    #[test]
    fn compile_uses_real_typescript_stdlib_when_available() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let main = root.join("main.ts");

        fs::write(&main, "const withResolvers = Promise.withResolvers;\n").unwrap();

        let mut options = CompilerOptions::default();
        options.target = Some(tsc_rs_ast::ScriptTarget::ES2024);

        let project = TsProject::new(vec![main.to_string_lossy().to_string()], options);
        let result = project.compile();
        let output = result
            .files
            .iter()
            .find(|compiled| compiled.file_name == main.to_string_lossy())
            .expect("expected main.ts output");

        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diag| diag.message.contains("withResolvers")),
            "expected real lib.es2024.promise.d.ts to provide Promise.withResolvers: {:?}",
            output.diagnostics
        );
    }

    #[test]
    fn compile_and_check_session_surface_parser_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("octal.ts");
        fs::write(&file, r#"const value = "\1";"#).unwrap();
        let file_name = file.to_string_lossy().into_owned();
        let project = TsProject::new(vec![file_name.clone()], CompilerOptions::default());

        let result = project.compile();
        let compiled = result
            .files
            .iter()
            .find(|output| output.file_name == file_name)
            .expect("compiled file output");
        assert_eq!(
            compiled
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 1487)
                .count(),
            1,
            "{:#?}",
            compiled.diagnostics
        );

        let session = project.open_check_session();
        let diagnostics = session.check_file(&file_name, None);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 1487)
                .count(),
            1,
            "{diagnostics:#?}"
        );
    }

    #[test]
    fn no_check_modes_keep_parser_diagnostics_and_suppress_semantic_diagnostics() {
        fn assert_only_parser_octal(diagnostics: &[Diagnostic]) {
            assert_eq!(
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code)
                    .collect::<Vec<_>>(),
                vec![1487],
                "{diagnostics:#?}"
            );
        }

        let dir = tempfile::tempdir().unwrap();
        let option_parse_file = dir.path().join("option_parse.ts");
        let option_semantic_file = dir.path().join("option_semantic.ts");
        fs::write(
            &option_parse_file,
            r#"const escaped = "\1"; const value: string = 1;"#,
        )
        .unwrap();
        fs::write(
            &option_semantic_file,
            r#"const regex = /\00/; const value: string = 1;"#,
        )
        .unwrap();

        let option_parse_name = option_parse_file.to_string_lossy().into_owned();
        let option_semantic_name = option_semantic_file.to_string_lossy().into_owned();
        let mut no_check_options = CompilerOptions::default();
        no_check_options.no_check = Some(true);
        let no_check_project = TsProject::new(
            vec![option_parse_name.clone(), option_semantic_name.clone()],
            no_check_options,
        );

        let result = no_check_project.compile();
        let parse_output = result
            .files
            .iter()
            .find(|output| output.file_name == option_parse_name)
            .expect("noCheck parse file output");
        assert_only_parser_octal(&parse_output.diagnostics);
        let semantic_output = result
            .files
            .iter()
            .find(|output| output.file_name == option_semantic_name)
            .expect("noCheck semantic file output");
        assert!(
            semantic_output.diagnostics.is_empty(),
            "{:#?}",
            semantic_output.diagnostics
        );

        let session = no_check_project.open_check_session();
        assert_only_parser_octal(&session.check_file(&option_parse_name, None));
        assert!(
            session.check_file(&option_semantic_name, None).is_empty(),
            "CheckSession must honor noCheck"
        );

        let pragma_parse_file = dir.path().join("pragma_parse.ts");
        let pragma_semantic_file = dir.path().join("pragma_semantic.ts");
        fs::write(
            &pragma_parse_file,
            "// Copyright header\n// @ts-nocheck: generated\n\
             const escaped = \"\\1\"; const value: string = 1;",
        )
        .unwrap();
        fs::write(
            &pragma_semantic_file,
            "// @ts-nocheck additional context\n\
             const regex = /\\00/; const value: string = 1;",
        )
        .unwrap();

        let pragma_parse_name = pragma_parse_file.to_string_lossy().into_owned();
        let pragma_semantic_name = pragma_semantic_file.to_string_lossy().into_owned();
        let pragma_project = TsProject::new(
            vec![pragma_parse_name.clone(), pragma_semantic_name.clone()],
            CompilerOptions::default(),
        );
        let result = pragma_project.compile();
        let parse_output = result
            .files
            .iter()
            .find(|output| output.file_name == pragma_parse_name)
            .expect("@ts-nocheck parse file output");
        assert_only_parser_octal(&parse_output.diagnostics);
        let semantic_output = result
            .files
            .iter()
            .find(|output| output.file_name == pragma_semantic_name)
            .expect("@ts-nocheck semantic file output");
        assert!(
            semantic_output.diagnostics.is_empty(),
            "{:#?}",
            semantic_output.diagnostics
        );

        let session = pragma_project.open_check_session();
        assert_only_parser_octal(&session.check_file(&pragma_parse_name, None));
        assert!(
            session.check_file(&pragma_semantic_name, None).is_empty(),
            "CheckSession must honor @ts-nocheck"
        );
    }

    #[test]
    fn compile_query_builds_engine_for_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("main.ts");
        let source = "const x = 1;\n";
        fs::write(&file, source).unwrap();

        let file_name = file.to_string_lossy().to_string();
        let project = TsProject::new(vec![file_name.clone()], CompilerOptions::default());
        let result = project.compile_query();

        assert!(
            !result.has_errors(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        assert!(
            result.engine.get_symbol_at(&file_name, 6).is_some(),
            "expected symbol lookup to work through query engine"
        );
        assert_eq!(result.total_diagnostics(), 0);
    }

    #[test]
    fn compile_query_collects_project_and_checker_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("err.ts");
        fs::write(&file, "let x: number = \"oops\";\n").unwrap();
        let missing = root.join("missing.ts").to_string_lossy().to_string();

        let file_name = file.to_string_lossy().to_string();
        let project = TsProject::new(vec![file_name.clone(), missing], CompilerOptions::default());
        let result = project.compile_query();

        assert!(result.has_errors());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == 6053 && d.message.contains("Cannot read file")),
            "expected missing-file diagnostic"
        );
        assert!(
            result.diagnostics.iter().any(|d| {
                d.category == DiagnosticCategory::Error
                    && d.file_name.as_deref() == Some(file_name.as_str())
            }),
            "expected checker/binder diagnostics for existing file"
        );
    }

    #[test]
    fn compile_allows_import_equals_require_usage_across_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let decl = root.join("decl.ts");
        let main = root.join("main.ts");

        fs::write(
            &decl,
            "export interface Point { x: number; y: number; }\n\
             export function point(x: number, y: number): Point { return { x, y }; }\n",
        )
        .unwrap();
        fs::write(
            &main,
            "import g = require(\"./decl\");\n\
             let p: g.Point = g.point(1, 2);\n",
        )
        .unwrap();

        let project = TsProject::new(
            vec![
                decl.to_string_lossy().to_string(),
                main.to_string_lossy().to_string(),
            ],
            CompilerOptions::default(),
        );
        let result = project.compile();

        assert!(
            result
                .diagnostics
                .iter()
                .all(|diag| diag.file_name.as_deref() != Some(main.to_string_lossy().as_ref())),
            "unexpected project diagnostics: {:?}",
            result.diagnostics
        );
        let main_output = result
            .files
            .iter()
            .find(|file| file.file_name == main.to_string_lossy())
            .expect("expected main.ts output");
        assert!(
            main_output.diagnostics.is_empty(),
            "unexpected file diagnostics: {:?}",
            main_output.diagnostics
        );
    }

    #[test]
    fn compile_incremental_second_run_no_changes_compiles_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("main.ts");
        fs::write(&file, "export const x = 1;").unwrap();

        let file_name = file.to_string_lossy().to_string();
        let build_info_path = root.join("cache/tsconfig.tsbuildinfo");
        let build_info_path_str = build_info_path.to_string_lossy().to_string();

        let project = TsProject::new(vec![file_name.clone()], CompilerOptions::default());
        let (first_result, first_info) = project.compile_incremental(&build_info_path_str);
        assert_eq!(
            first_result.files.len(),
            1,
            "first run compiles changed/new files"
        );
        first_info.save(&build_info_path_str).unwrap();

        let project2 = TsProject::new(vec![file_name], CompilerOptions::default());
        let (second_result, _second_info) = project2.compile_incremental(&build_info_path_str);
        assert_eq!(
            second_result.files.len(),
            0,
            "second run with unchanged file and options should be incremental no-op"
        );
        assert!(second_result.diagnostics.is_empty());
    }

    #[test]
    fn compile_incremental_keeps_full_graph_for_changed_importer() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let decl = root.join("decl.ts");
        let main = root.join("main.ts");
        fs::write(&decl, "export interface Point { x: number; y: number; }\n").unwrap();
        fs::write(
            &main,
            "import { Point } from \"./decl\";\nconst p: Point = { x: 1, y: 2 };\n",
        )
        .unwrap();

        let build_info_path = root.join("cache/tsconfig.tsbuildinfo");
        let build_info_path_str = build_info_path.to_string_lossy().to_string();
        let file_names = vec![
            decl.to_string_lossy().to_string(),
            main.to_string_lossy().to_string(),
        ];

        let project = TsProject::new(file_names.clone(), CompilerOptions::default());
        let (_first_result, first_info) = project.compile_incremental(&build_info_path_str);
        first_info.save(&build_info_path_str).unwrap();

        fs::write(
            &main,
            "import { Point } from \"./decl\";\nconst p: Point = { x: \"oops\", y: 2 };\n",
        )
        .unwrap();

        let project = TsProject::new(file_names, CompilerOptions::default());
        let (result, _info) = project.compile_incremental(&build_info_path_str);
        let main_output = result
            .files
            .iter()
            .find(|file| file.file_name == main.to_string_lossy())
            .expect("expected main.ts output");

        assert_eq!(
            result.files.len(),
            1,
            "only the changed importer should be emitted on the incremental rebuild"
        );
        assert!(
            main_output.diagnostics.iter().any(|diag| diag.code == 2322),
            "expected incremental rebuild to preserve imported interface context: {:?}",
            main_output.diagnostics
        );
    }

    #[test]
    fn member_access_on_self_resolving_namespace_alias_terminates() {
        // Regression: `tsc-rs --check-pipe -p packages/api-routers/tsconfig.json`
        // on a large pnpm monorepo died with
        //   thread 'check-pipe' has overflowed its stack
        //   fatal runtime error: stack overflow, aborting
        // Raising TSC_RS_STACK_MB from 128 to 512 only bought more frames, and
        // the process ABORTS — there is no panic to catch, so a whole project
        // became uncheckable.
        //
        // The cycle is in `resolve_member_on_type`'s alias-expansion arm:
        // `resolve_type_for_assignability("$Class.PrismaClient")` finds no entry
        // for the qualified name, falls back to the bare suffix "PrismaClient"
        // (legitimate — that is how `z.infer` resolves), and that alias's body
        // is `$Class.PrismaClient` again. Expansion returns its own input, and
        // member resolution re-enters on an identical type forever.
        //
        // Shape below is Prisma's generated client reduced to its essentials:
        // a namespace import plus a same-named local alias whose body is the
        // qualified reference. `$Class.PrismaClient` must NOT be resolvable to
        // an object type — when it is, the structural branch answers first and
        // the alias arm is never reached (which is why the real trigger only
        // appeared once the interface's own file was outside the donor).
        //
        // NOTE: a regression here does not fail this test — it aborts the whole
        // test binary. That is the bug's actual signature.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();

        fs::write(
            root.join("src/class.ts"),
            "export type Unrelated = number;\n",
        )
        .unwrap();
        fs::write(
            root.join("src/client.ts"),
            r#"import * as $Class from "./class";
export type PrismaClient<A = never, B = never, C = never> = $Class.PrismaClient<A, B, C>;
"#,
        )
        .unwrap();
        fs::write(
            root.join("src/use.ts"),
            r#"import type { PrismaClient } from "./client";
declare const prisma: PrismaClient;
export const rows = prisma.$queryRawUnsafe("select 1");
"#,
        )
        .unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "strict": true, "noEmit": true, "skipLibCheck": true },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let project = TsProject::from_config(root.join("tsconfig.json").to_str().unwrap())
            .expect("project should load");
        let session = project.open_check_session();
        let use_path = root.join("src/use.ts");
        let diagnostics = session.check_file(use_path.to_str().unwrap(), None);

        // The cut-off degrades member resolution to `Any`, so the unresolvable
        // alias must not manufacture an error about the property itself. The
        // arity error on the alias reference is expected and unrelated.
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("$queryRawUnsafe")),
            "cycle cut-off should stay silent about the member: {:?}",
            diagnostics
        );
    }

    #[test]
    fn conditional_expansion_of_mutually_recursive_types_stays_bounded() {
        // Regression: `--check-pipe -p packages/creative-editor-core/tsconfig.json`
        // grew ~1 GB every 2s and died on
        //   memory allocation of 112 bytes failed
        // against the 12 GB RLIMIT_AS that `install_vmsize_rlimit` sets.
        //
        // `deep_expand_for_conditional` was depth-capped but not breadth-capped,
        // and it rebuilds the FULL property list of every object it expands. So
        // the cost is the PRODUCT of the property counts along a path, not their
        // sum — at the depth-50 cap even a few properties per level is more
        // memory than exists. The real trigger was the DOM lib's own cycles:
        // Document -> WindowProxy -> Document, and Function -> Function.
        //
        // The mutual reference below is that shape. Peak RSS for the real
        // package went 12 GB (fatal) -> 109 MB after the re-entry guard.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();

        // Each level widens as well as recurses: without the guard the expansion
        // is products-of-widths, with it each name expands once per path.
        fs::write(
            root.join("src/dom.ts"),
            r#"export interface Doc {
  view: Win;
  a1: Win; a2: Win; a3: Win; a4: Win; a5: Win; a6: Win;
}
export interface Win {
  doc: Doc;
  b1: Doc; b2: Doc; b3: Doc; b4: Doc; b5: Doc; b6: Doc;
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("src/use.ts"),
            r#"import type { Doc } from "./dom";
type ViewOf<T> = T extends { view: infer V } ? V : never;
declare const d: ViewOf<Doc>;
export const out = d;
"#,
        )
        .unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "strict": true, "noEmit": true, "skipLibCheck": true },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let project = TsProject::from_config(root.join("tsconfig.json").to_str().unwrap())
            .expect("project should load");
        let session = project.open_check_session();
        let use_path = root.join("src/use.ts");
        let diagnostics = session.check_file(use_path.to_str().unwrap(), None);

        // Bounded work is the property under test; a recursive type is not an
        // error, so the file must come back clean rather than merely finishing.
        assert!(
            diagnostics.is_empty(),
            "recursive type expansion should terminate cleanly: {:?}",
            diagnostics
        );
    }

    #[test]
    fn generic_method_type_param_shadows_class_type_param() {
        // Regression: a method whose type parameter shares a name with its
        // class's type parameter mis-bound to the class's argument. zod's
        // `or<T extends SomeType>(option: T)` lives on the base interface and
        // `ZodOptional<T>` reuses the name `T`; on a `ZodOptional<ZodString>`
        // receiver, tsc-rs bound the method's `T` to `ZodString`, so
        // `z.string().optional().or(z.number())` reported TS2345 "ZodNumber not
        // assignable to ZodString" — 40 spurious errors across the router package,
        // and the `z.literal("")` variant surfaced as `ZodLiteral<string[number]>`.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.d.ts"),
            r#"export interface Base {
  optional(): Opt<this>;
  or<T>(option: T): Union<this, T>;
}
export interface Str extends Base {}
export interface Num extends Base {}
export interface Opt<T> extends Base { unwrap(): T; }
export interface Union<A, B> extends Base {}
export declare const z: { string(): Str; number(): Num };
"#,
        )
        .unwrap();
        fs::write(
            root.join("src/use.ts"),
            r#"import { z } from "./lib";
export const ok = z.string().or(z.number());
export const viaOptional = z.string().optional().or(z.number());
"#,
        )
        .unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "strict": true, "noEmit": true, "skipLibCheck": true },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let project = TsProject::from_config(root.join("tsconfig.json").to_str().unwrap())
            .expect("project should load");
        let session = project.open_check_session();
        let use_path = root.join("src/use.ts");
        let diagnostics = session.check_file(use_path.to_str().unwrap(), None);
        assert!(
            diagnostics.is_empty(),
            "the method's own `T` must stay generic on a generic receiver: {:?}",
            diagnostics
        );
    }

    #[test]
    fn or_fallback_and_loose_null_guard_narrow_out_undefined() {
        // Regression for two common CFA-narrowing gaps that mis-fired TS2532:
        //   - `const e = maybe || fallback` kept `undefined` in `e`'s type
        //     (`||` must yield the TRUTHY part of the left | right). The
        //     `map.get(k) || {...}` idiom is everywhere in the routers.
        //   - `if (x == null) return; x.foo()` — loose `==`/`!=` null matches
        //     BOTH null and undefined, so the guard must narrow out both.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/use.ts"),
            r#"declare function get(): string | undefined;
export function orFallback(): string { const e = get() || "x"; return e.toUpperCase(); }
export function orObject(m: Map<string, { n: number }>, k: string): number {
  const e = m.get(k) || { n: 0 };
  return e.n;
}
export function looseNull(): string {
  const x = get();
  if (x == null) return "";
  return x.toUpperCase();
}
export function looseNotNull(): string {
  const x = get();
  if (x != null) { return x.toUpperCase(); }
  return "";
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "strict": true, "noEmit": true, "skipLibCheck": true, "lib": ["es2022"] },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let project = TsProject::from_config(root.join("tsconfig.json").to_str().unwrap())
            .expect("project should load");
        let session = project.open_check_session();
        let use_path = root.join("src/use.ts");
        let diagnostics = session.check_file(use_path.to_str().unwrap(), None);
        assert!(
            !diagnostics.iter().any(|d| d.code == 2532 || d.code == 2531),
            "`|| fallback` and loose `== null` must narrow out undefined: {:?}",
            diagnostics
        );
    }

    #[test]
    fn logical_and_narrows_its_right_operand() {
        // Regression: `a && a.foo()` checked the right operand WITHOUT the
        // left's truthy facts, so both `x && x.toUpperCase()` and the member
        // form `o.v && o.v.toUpperCase()` mis-fired TS2532. The ternary
        // `o.v ? o.v.foo() : x` already narrowed via its own arm; this brings
        // `&&` to parity.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/use.ts"),
            r#"export function ident(x: string | undefined): string | false {
  return x && x.toUpperCase();
}
export function member(o: { v?: string }): string | undefined {
  return o.v && o.v.toUpperCase();
}
export function optChainObj(o?: { v?: string }): string | undefined {
  return o?.v && o.v.toUpperCase();
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": { "strict": true, "noEmit": true, "skipLibCheck": true, "lib": ["es2022"] },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let project = TsProject::from_config(root.join("tsconfig.json").to_str().unwrap())
            .expect("project should load");
        let session = project.open_check_session();
        let use_path = root.join("src/use.ts");
        let diagnostics = session.check_file(use_path.to_str().unwrap(), None);
        assert!(
            !diagnostics.iter().any(|d| d.code == 2532 || d.code == 2531),
            "`&&` must narrow its right operand: {:?}",
            diagnostics
        );
    }

    #[test]
    fn logical_and_narrows_multi_level_and_optional_chain_paths() {
        // Regression: chained member-path guards. The dominant router shape is
        // `a?.b && a.b.c` / `a.b && a.b.n && a.b.n.foo()`; each `&&` link must
        // see the guards to its left (multi-level paths + optional chains), and
        // an assignment to a narrowed nullable property must still check
        // against its DECLARED type (`if (typeof u.ref === "string") u.ref = null`
        // is legal — ref is `string | null`), and `typeof anyVal === "object"`
        // must NOT narrow an `any` down to `object` (would break a later cast).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/use.ts"),
            r#"export function multi(a: { b?: { n?: string } }): string | undefined {
  return a.b && a.b.n && a.b.n.toUpperCase();
}
export function deepOptional(a?: { b?: { c?: string } }): string | undefined {
  return a?.b?.c && a.b.c.toUpperCase();
}
export function assignNarrowedNullable(u: { reference?: string | null }): void {
  if (typeof u.reference === "string" && u.reference.trim() === "") {
    u.reference = null;
  }
}
export function anyCastStaysValid(segmentFilters: any): unknown[] {
  if (segmentFilters && typeof segmentFilters === "object") {
    return segmentFilters as { field: string }[];
  }
  return [];
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{
                "compilerOptions": {
                    "strict": true,
                    "noUncheckedIndexedAccess": true,
                    "noEmit": true,
                    "skipLibCheck": true,
                    "lib": ["es2022"]
                },
                "include": ["src/**/*.ts"]
            }"#,
        )
        .unwrap();

        let project = TsProject::from_config(root.join("tsconfig.json").to_str().unwrap())
            .expect("project should load");
        let session = project.open_check_session();
        let use_path = root.join("src/use.ts");
        let diagnostics = session.check_file(use_path.to_str().unwrap(), None);
        // real tsc is clean on all four functions.
        assert!(
            diagnostics.is_empty(),
            "chained member-path guards + narrowed-nullable assign + any-cast must be clean: {:?}",
            diagnostics
        );
    }

    #[test]
    fn project_donor_preserves_imported_unique_symbol_property_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let keys = root.join("keys.ts");
        let class_keys = root.join("class-keys.ts");
        let named = root.join("named.ts");
        let namespace = root.join("namespace.ts");
        let class_static = root.join("class-static.ts");
        let distinct_a = root.join("distinct-a.ts");
        let distinct_b = root.join("distinct-b.ts");
        let distinct_use = root.join("distinct-use.ts");
        let class_collision = root.join("class-collision.ts");
        let exported_namespace = root.join("exported-namespace.ts");
        let namespace_use = root.join("namespace-use.ts");
        let class_a = root.join("class-a.ts");
        let class_b = root.join("class-b.ts");
        let class_distinct_use = root.join("class-distinct-use.ts");
        let inherited_keys = root.join("inherited-keys.ts");
        let inherited_use = root.join("inherited-use.ts");
        let alias_a = root.join("alias-a.ts");
        let alias_b = root.join("alias-b.ts");
        let alias_use = root.join("alias-use.ts");
        let enum_a = root.join("enum-a.ts");
        let enum_b = root.join("enum-b.ts");
        let enum_use = root.join("enum-use.ts");
        let default_key = root.join("default-key.ts");
        let default_use = root.join("default-use.ts");
        let default_class = root.join("default-class.ts");
        let default_class_use = root.join("default-class-use.ts");
        let imported_root = root.join("imported-root.ts");
        let imported_derived = root.join("imported-derived.ts");
        let imported_derived_use = root.join("imported-derived-use.ts");
        let qualified_alias_a = root.join("qualified-alias-a.ts");
        let qualified_alias_b = root.join("qualified-alias-b.ts");
        let qualified_alias_use = root.join("qualified-alias-use.ts");
        let export_list_a = root.join("export-list-a.ts");
        let export_list_b = root.join("export-list-b.ts");
        let export_list_use = root.join("export-list-use.ts");
        let imported_alias = root.join("imported-alias.ts");
        let imported_alias_use = root.join("imported-alias-use.ts");
        let inferred_key = root.join("inferred-key.ts");
        let inferred_key_use = root.join("inferred-key-use.ts");
        let inferred_class = root.join("inferred-class.ts");
        let inferred_class_use = root.join("inferred-class-use.ts");
        let default_alias_a = root.join("default-alias-a.ts");
        let default_alias_b = root.join("default-alias-b.ts");
        let default_alias_use = root.join("default-alias-use.ts");
        let namespace_qualified_a = root.join("namespace-qualified-a.ts");
        let namespace_qualified_b = root.join("namespace-qualified-b.ts");
        let namespace_qualified_use = root.join("namespace-qualified-use.ts");
        let static_alias_a = root.join("static-alias-a.ts");
        let static_alias_b = root.join("static-alias-b.ts");
        let static_alias_use = root.join("static-alias-use.ts");
        let literal_class_a = root.join("literal-class-a.ts");
        let literal_class_b = root.join("literal-class-b.ts");
        let literal_class_use = root.join("literal-class-use.ts");
        let literal_object_a = root.join("literal-object-a.ts");
        let literal_object_b = root.join("literal-object-b.ts");
        let literal_object_use = root.join("literal-object-use.ts");
        let destructured_direct_a = root.join("destructured-direct-a.ts");
        let destructured_direct_b = root.join("destructured-direct-b.ts");
        let destructured_direct_use = root.join("destructured-direct-use.ts");
        let destructured_list_a = root.join("destructured-list-a.ts");
        let destructured_list_b = root.join("destructured-list-b.ts");
        let destructured_list_use = root.join("destructured-list-use.ts");
        let qualified_static_a = root.join("qualified-static-a.ts");
        let qualified_static_b = root.join("qualified-static-b.ts");
        let qualified_static_distinct_use = root.join("qualified-static-distinct-use.ts");
        let qualified_static_positive_use = root.join("qualified-static-positive-use.ts");
        let static_getter = root.join("static-getter.ts");
        let static_getter_use = root.join("static-getter-use.ts");
        let destructured_default = root.join("destructured-default.ts");
        let destructured_default_use = root.join("destructured-default-use.ts");
        let destructured_object_rest = root.join("destructured-object-rest.ts");
        let destructured_object_rest_use = root.join("destructured-object-rest-use.ts");
        let destructured_array_rest = root.join("destructured-array-rest.ts");
        let destructured_array_rest_use = root.join("destructured-array-rest-use.ts");
        let reopened_static_a = root.join("reopened-static-a.ts");
        let reopened_static_b = root.join("reopened-static-b.ts");
        let reopened_static_distinct_use = root.join("reopened-static-distinct-use.ts");
        let reopened_static_positive_use = root.join("reopened-static-positive-use.ts");
        let imported_getter_a = root.join("imported-getter-a.ts");
        let imported_getter_b = root.join("imported-getter-b.ts");
        let imported_getter_keys = root.join("imported-getter-keys.ts");
        let imported_getter_use = root.join("imported-getter-use.ts");
        let computed_binding = root.join("computed-binding.ts");
        let computed_binding_use = root.join("computed-binding-use.ts");
        let mutable_const_key = root.join("mutable-const-key.ts");
        let mutable_const_key_use = root.join("mutable-const-key-use.ts");
        let computed_local_a = root.join("computed-local-a.ts");
        let computed_local_b = root.join("computed-local-b.ts");
        let computed_local_use = root.join("computed-local-use.ts");
        fs::write(&keys, "export declare const sym: unique symbol;\n").unwrap();
        fs::write(
            &class_keys,
            "export declare class Keys { static readonly sym: unique symbol; }\n",
        )
        .unwrap();
        fs::write(
            &named,
            r#"import { sym } from "./keys";
abstract class NamedBase {
    abstract [sym]: string;
    constructor() { const { [sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &namespace,
            r#"import * as Keys from "./keys";
abstract class NamespaceBase {
    abstract [Keys.sym]: string;
    constructor() { const { [Keys.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &class_static,
            r#"import { Keys as ImportedKeys } from "./class-keys";
abstract class ClassStaticBase {
    abstract [ImportedKeys.sym]: string;
    constructor() { const { [ImportedKeys.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(&distinct_a, "export declare const sym: unique symbol;\n").unwrap();
        fs::write(&distinct_b, "export declare const sym: unique symbol;\n").unwrap();
        fs::write(
            &distinct_use,
            r#"import { sym as aSym } from "./distinct-a";
import { sym as bSym } from "./distinct-b";
abstract class DistinctBase {
    abstract [aSym]: string;
    constructor() { const { [bSym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &class_collision,
            r#"import { Keys as Imported } from "./class-keys";
class Keys {}
abstract class CollisionBase {
    abstract [Imported.sym]: string;
    constructor() { const { [Imported.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &exported_namespace,
            r#"export declare namespace ExportedKeys {
    const sym: unique symbol;
}
"#,
        )
        .unwrap();
        fs::write(
            &namespace_use,
            r#"import { ExportedKeys } from "./exported-namespace";
abstract class ExportedNamespaceBase {
    abstract [ExportedKeys.sym]: string;
    constructor() { const { [ExportedKeys.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &class_a,
            "export declare class Keys { static readonly sym: unique symbol; }\n",
        )
        .unwrap();
        fs::write(
            &class_b,
            "export declare class Keys { static readonly sym: unique symbol; }\n",
        )
        .unwrap();
        fs::write(
            &class_distinct_use,
            r#"import { Keys as A } from "./class-a";
import { Keys as B } from "./class-b";
abstract class DistinctClassBase {
    abstract [A.sym]: string;
    constructor() { const { [B.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &inherited_keys,
            r#"export declare class Root {
    static readonly sym: unique symbol;
}
export declare class Keys extends Root {}
"#,
        )
        .unwrap();
        fs::write(
            &inherited_use,
            r#"import { Keys as Imported } from "./inherited-keys";
class Keys {}
abstract class InheritedBase {
    abstract [Imported.sym]: string;
    constructor() { const { [Imported.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &alias_a,
            r#"export declare const root: unique symbol;
export declare const alias: typeof root;
"#,
        )
        .unwrap();
        fs::write(
            &alias_b,
            r#"export declare const root: unique symbol;
export declare const alias: typeof root;
"#,
        )
        .unwrap();
        fs::write(
            &alias_use,
            r#"import { alias as A } from "./alias-a";
import { alias as B } from "./alias-b";
abstract class AliasBase {
    abstract [A]: string;
    constructor() { const { [B]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(&enum_a, "export enum Keys { X = \"x\" }\n").unwrap();
        fs::write(&enum_b, "export enum Keys { X = \"y\" }\n").unwrap();
        fs::write(
            &enum_use,
            r#"import { Keys as B } from "./enum-b";
abstract class EnumBase {
    abstract x: string;
    y = "";
    constructor() { const { [B.X]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &default_key,
            r#"const sym: unique symbol = Symbol();
export default sym;
"#,
        )
        .unwrap();
        fs::write(
            &default_use,
            r#"import sym from "./default-key";
abstract class DefaultBase {
    abstract [sym]: string;
    constructor() { const { [sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &default_class,
            r#"export default class Keys {
    static readonly sym: unique symbol;
}
"#,
        )
        .unwrap();
        fs::write(
            &default_class_use,
            r#"import Imported from "./default-class";
abstract class DefaultClassBase {
    abstract [Imported.sym]: string;
    constructor() { const { [Imported.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &imported_root,
            r#"export declare class Root {
    static readonly sym: unique symbol;
}
"#,
        )
        .unwrap();
        fs::write(
            &imported_derived,
            r#"import { Root } from "./imported-root";
export declare class Keys extends Root {}
"#,
        )
        .unwrap();
        fs::write(
            &imported_derived_use,
            r#"import { Keys as Imported } from "./imported-derived";
class Keys {}
abstract class ImportedDerivedBase {
    abstract [Imported.sym]: string;
    constructor() { const { [Imported.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        for file in [&qualified_alias_a, &qualified_alias_b] {
            fs::write(
                file,
                r#"export declare namespace N {
    const sym: unique symbol;
}
export declare const alias: typeof N.sym;
"#,
            )
            .unwrap();
        }
        fs::write(
            &qualified_alias_use,
            r#"import { alias as A } from "./qualified-alias-a";
import { alias as B } from "./qualified-alias-b";
abstract class QualifiedAliasBase {
    abstract [A]: string;
    constructor() { const { [B]: value } = this; }
}
"#,
        )
        .unwrap();
        for file in [&export_list_a, &export_list_b] {
            fs::write(
                file,
                r#"declare const root: unique symbol;
export { root as alias };
"#,
            )
            .unwrap();
        }
        fs::write(
            &export_list_use,
            r#"import { alias as A } from "./export-list-a";
import { alias as B } from "./export-list-b";
abstract class ExportListBase {
    abstract [A]: string;
    constructor() { const { [B]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &imported_alias,
            r#"import { sym } from "./keys";
export const alias: typeof sym = sym;
"#,
        )
        .unwrap();
        fs::write(
            &imported_alias_use,
            r#"import { sym } from "./keys";
import { alias } from "./imported-alias";
abstract class ImportedAliasBase {
    abstract [sym]: string;
    constructor() { const { [alias]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(&inferred_key, "export const sym = Symbol();\n").unwrap();
        fs::write(
            &inferred_key_use,
            r#"import { sym } from "./inferred-key";
abstract class InferredKeyBase {
    abstract [sym]: string;
    constructor() { const { [sym]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &inferred_class,
            r#"export class Keys {
    static readonly sym = Symbol();
}
"#,
        )
        .unwrap();
        fs::write(
            &inferred_class_use,
            r#"import { Keys } from "./inferred-class";
abstract class InferredClassBase {
    abstract [Keys.sym]: string;
    constructor() { const { [Keys.sym]: value } = this; }
}
"#,
        )
        .unwrap();
        for file in [&default_alias_a, &default_alias_b] {
            fs::write(
                file,
                r#"declare const root: unique symbol;
declare const alias: typeof root;
export default alias;
"#,
            )
            .unwrap();
        }
        fs::write(
            &default_alias_use,
            r#"import A from "./default-alias-a";
import B from "./default-alias-b";
abstract class DefaultAliasBase {
    abstract [A]: string;
    constructor() { const { [B]: value } = this; }
}
"#,
        )
        .unwrap();
        for file in [&namespace_qualified_a, &namespace_qualified_b] {
            fs::write(
                file,
                r#"export declare namespace N {
    const sym: unique symbol;
    const alias: typeof N.sym;
}
"#,
            )
            .unwrap();
        }
        fs::write(
            &namespace_qualified_use,
            r#"import { N as A } from "./namespace-qualified-a";
import { N as B } from "./namespace-qualified-b";
abstract class NamespaceQualifiedBase {
    abstract [A.alias]: string;
    constructor() { const { [B.alias]: value } = this; }
}
"#,
        )
        .unwrap();
        for file in [&static_alias_a, &static_alias_b] {
            fs::write(
                file,
                r#"export declare const root: unique symbol;
export declare class Keys {
    static readonly alias: typeof root;
}
"#,
            )
            .unwrap();
        }
        fs::write(
            &static_alias_use,
            r#"import { Keys as A } from "./static-alias-a";
import { Keys as B } from "./static-alias-b";
abstract class StaticAliasBase {
    abstract [A.alias]: string;
    constructor() { const { [B.alias]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &literal_class_a,
            "export class Keys { static readonly key = \"x\" as const; }\n",
        )
        .unwrap();
        fs::write(
            &literal_class_b,
            "export class Keys { static readonly key = \"y\" as const; }\n",
        )
        .unwrap();
        fs::write(
            &literal_class_use,
            r#"import { Keys as B } from "./literal-class-b";
abstract class LiteralClassBase {
    abstract x: string;
    y = "";
    constructor() { const { [B.key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &literal_object_a,
            "export const Keys = { key: \"x\" } as const;\n",
        )
        .unwrap();
        fs::write(
            &literal_object_b,
            "export const Keys = { key: \"y\" } as const;\n",
        )
        .unwrap();
        fs::write(
            &literal_object_use,
            r#"import { Keys as B } from "./literal-object-b";
abstract class LiteralObjectBase {
    abstract x: string;
    y = "";
    constructor() { const { [B.key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &destructured_direct_a,
            r#"export const { key } = { key: "x" } as const;
"#,
        )
        .unwrap();
        fs::write(
            &destructured_direct_b,
            r#"export const { key } = { key: "y" } as const;
"#,
        )
        .unwrap();
        fs::write(
            &destructured_direct_use,
            r#"import { key } from "./destructured-direct-b";
abstract class DestructuredDirectBase {
    abstract y: string;
    x = "";
    constructor() { const { [key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &destructured_list_a,
            r#"const { nested: { key }, tuple: [first] } = {
    nested: { key: "x" },
    tuple: ["w"],
} as const;
export { key, first };
"#,
        )
        .unwrap();
        fs::write(
            &destructured_list_b,
            r#"const { nested: { key }, tuple: [first] } = {
    nested: { key: "y" },
    tuple: ["z"],
} as const;
export { key, first };
"#,
        )
        .unwrap();
        fs::write(
            &destructured_list_use,
            r#"import { key, first } from "./destructured-list-b";
abstract class DestructuredListBase {
    abstract y: string;
    abstract z: string;
    x = "";
    constructor() {
        const { [key]: nestedValue } = this;
        const { [first]: tupleValue } = this;
    }
}
"#,
        )
        .unwrap();
        for file in [&qualified_static_a, &qualified_static_b] {
            fs::write(
                file,
                r#"export declare namespace N {
    const sym: unique symbol;
}
export declare class Keys {
    static readonly alias: typeof N.sym;
}
"#,
            )
            .unwrap();
        }
        fs::write(
            &qualified_static_distinct_use,
            r#"import { Keys as A } from "./qualified-static-a";
import { Keys as B } from "./qualified-static-b";
abstract class QualifiedStaticDistinctBase {
    abstract [A.alias]: string;
    constructor() { const { [B.alias]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &qualified_static_positive_use,
            r#"import { Keys } from "./qualified-static-a";
abstract class QualifiedStaticPositiveBase {
    abstract [Keys.alias]: string;
    constructor() { const { [Keys.alias]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &static_getter,
            r#"export class Keys {
    static get key(): "y" { return "y"; }
}
"#,
        )
        .unwrap();
        fs::write(
            &static_getter_use,
            r#"import { Keys } from "./static-getter";
abstract class StaticGetterBase {
    x = "";
    abstract y: string;
    constructor() { const { [Keys.key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &destructured_default,
            r#"export const { key = "x" } = {} as const;
"#,
        )
        .unwrap();
        fs::write(
            &destructured_default_use,
            r#"import { key } from "./destructured-default";
abstract class DestructuredDefaultBase {
    abstract x: string;
    constructor() { const { [key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &destructured_object_rest,
            r#"export const { key, ...rest } = { key: "x", other: "y" } as const;
"#,
        )
        .unwrap();
        fs::write(
            &destructured_object_rest_use,
            r#"import { rest } from "./destructured-object-rest";
export const invalid = rest.key;
"#,
        )
        .unwrap();
        fs::write(
            &destructured_array_rest,
            r#"export const [head, ...tail] = ["x", "y"] as const;
"#,
        )
        .unwrap();
        fs::write(
            &destructured_array_rest_use,
            r#"import { tail } from "./destructured-array-rest";
abstract class DestructuredArrayRestBase {
    x = "";
    abstract y: string;
    constructor() { const { [tail[0]]: value } = this; }
}
"#,
        )
        .unwrap();
        for file in [&reopened_static_a, &reopened_static_b] {
            fs::write(
                file,
                r#"export declare namespace N {
    const sym: unique symbol;
}
export declare namespace N {
    const alias: typeof N.sym;
}
export declare class Keys {
    static readonly key: typeof N.alias;
}
"#,
            )
            .unwrap();
        }
        fs::write(
            &reopened_static_distinct_use,
            r#"import { Keys as A } from "./reopened-static-a";
import { Keys as B } from "./reopened-static-b";
abstract class ReopenedStaticDistinctBase {
    abstract [A.key]: string;
    constructor() { const { [B.key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &reopened_static_positive_use,
            r#"import { Keys } from "./reopened-static-a";
abstract class ReopenedStaticPositiveBase {
    abstract [Keys.key]: string;
    constructor() { const { [Keys.key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &imported_getter_a,
            "export declare const sym: unique symbol;\n",
        )
        .unwrap();
        fs::write(
            &imported_getter_b,
            "export declare const sym: unique symbol;\n",
        )
        .unwrap();
        fs::write(
            &imported_getter_keys,
            r#"import { sym } from "./imported-getter-b";
export class Keys {
    static get key(): typeof sym { return sym; }
}
"#,
        )
        .unwrap();
        fs::write(
            &imported_getter_use,
            r#"import { sym } from "./imported-getter-b";
import { Keys } from "./imported-getter-keys";
abstract class ImportedGetterBase {
    abstract [sym]: string;
    constructor() { const { [Keys.key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &computed_binding,
            r#"export const { ["key"]: key } = { key: "x" } as const;
"#,
        )
        .unwrap();
        fs::write(
            &computed_binding_use,
            r#"import { key } from "./computed-binding";
abstract class ComputedBindingBase {
    abstract x: string;
    constructor() { const { [key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(&mutable_const_key, "export let key = \"x\" as const;\n").unwrap();
        fs::write(
            &mutable_const_key_use,
            r#"import { key } from "./mutable-const-key";
abstract class MutableConstKeyBase {
    abstract x: string;
    constructor() { const { [key]: value } = this; }
}
"#,
        )
        .unwrap();
        fs::write(
            &computed_local_a,
            r#"const choose = (): "wrong" => "wrong";
const names = { property: choose() } as const;
const property = names.property;
export const { [property]: key } = { wrong: "x" } as const;
"#,
        )
        .unwrap();
        fs::write(
            &computed_local_b,
            r#"const choose = (): "right" => "right";
const names = { property: choose() } as const;
const property = names.property;
export const { [property]: key } = { right: "y" } as const;
"#,
        )
        .unwrap();
        fs::write(
            &computed_local_use,
            r#"import { key } from "./computed-local-b";
abstract class ComputedLocalBase {
    x = "";
    abstract y: string;
    constructor() { const { [key]: value } = this; }
}
"#,
        )
        .unwrap();

        let mut options = CompilerOptions::default();
        options.no_emit = Some(true);
        let paths = [
            &keys,
            &class_keys,
            &named,
            &namespace,
            &class_static,
            &distinct_a,
            &distinct_b,
            &distinct_use,
            &class_collision,
            &exported_namespace,
            &namespace_use,
            &class_a,
            &class_b,
            &class_distinct_use,
            &inherited_keys,
            &inherited_use,
            &alias_a,
            &alias_b,
            &alias_use,
            &enum_a,
            &enum_b,
            &enum_use,
            &default_key,
            &default_use,
            &default_class,
            &default_class_use,
            &imported_root,
            &imported_derived,
            &imported_derived_use,
            &qualified_alias_a,
            &qualified_alias_b,
            &qualified_alias_use,
            &export_list_a,
            &export_list_b,
            &export_list_use,
            &imported_alias,
            &imported_alias_use,
            &inferred_key,
            &inferred_key_use,
            &inferred_class,
            &inferred_class_use,
            &default_alias_a,
            &default_alias_b,
            &default_alias_use,
            &namespace_qualified_a,
            &namespace_qualified_b,
            &namespace_qualified_use,
            &static_alias_a,
            &static_alias_b,
            &static_alias_use,
            &literal_class_a,
            &literal_class_b,
            &literal_class_use,
            &literal_object_a,
            &literal_object_b,
            &literal_object_use,
            &destructured_direct_a,
            &destructured_direct_b,
            &destructured_direct_use,
            &destructured_list_a,
            &destructured_list_b,
            &destructured_list_use,
            &qualified_static_a,
            &qualified_static_b,
            &qualified_static_distinct_use,
            &qualified_static_positive_use,
            &static_getter,
            &static_getter_use,
            &destructured_default,
            &destructured_default_use,
            &destructured_object_rest,
            &destructured_object_rest_use,
            &destructured_array_rest,
            &destructured_array_rest_use,
            &reopened_static_a,
            &reopened_static_b,
            &reopened_static_distinct_use,
            &reopened_static_positive_use,
            &imported_getter_a,
            &imported_getter_b,
            &imported_getter_keys,
            &imported_getter_use,
            &computed_binding,
            &computed_binding_use,
            &mutable_const_key,
            &mutable_const_key_use,
            &computed_local_a,
            &computed_local_b,
            &computed_local_use,
        ]
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
        let reversed_paths = paths.iter().rev().cloned().collect::<Vec<_>>();
        let project = TsProject::new(paths, options);
        let result = project.compile();
        let ts2715 = result
            .files
            .iter()
            .flat_map(|file| &file.diagnostics)
            .filter(|diagnostic| diagnostic.code == 2715)
            .collect::<Vec<_>>();
        assert_eq!(
            ts2715.len(),
            24,
            "TS2715 counts: {:?}",
            result
                .files
                .iter()
                .filter_map(|file| {
                    let count = file
                        .diagnostics
                        .iter()
                        .filter(|diagnostic| diagnostic.code == 2715)
                        .count();
                    (count != 0).then_some((&file.file_name, count))
                })
                .collect::<Vec<_>>()
        );
        assert!(
            result
                .files
                .iter()
                .find(|file| file.file_name == distinct_use.to_string_lossy())
                .is_some_and(|file| file
                    .diagnostics
                    .iter()
                    .all(|diagnostic| { diagnostic.code != 2715 })),
            "distinct module exports must keep separate unique-symbol identities: {:?}",
            result.files
        );
        for clean_file in [
            &class_distinct_use,
            &alias_use,
            &enum_use,
            &qualified_alias_use,
            &export_list_use,
            &default_alias_use,
            &namespace_qualified_use,
            &static_alias_use,
            &literal_class_use,
            &literal_object_use,
            &qualified_static_distinct_use,
            &reopened_static_distinct_use,
            &destructured_object_rest_use,
        ] {
            assert!(
                result
                    .files
                    .iter()
                    .find(|file| file.file_name == clean_file.to_string_lossy())
                    .is_some_and(|file| file
                        .diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.code != 2715)),
                "{} must not report TS2715: {:?}",
                clean_file.display(),
                result.files
            );
        }
        for (diagnostic_file, expected_count) in [
            (&destructured_direct_use, 1usize),
            (&destructured_list_use, 2usize),
            (&qualified_static_positive_use, 1usize),
            (&static_getter_use, 1usize),
            (&destructured_default_use, 1usize),
            (&destructured_array_rest_use, 1usize),
            (&reopened_static_positive_use, 1usize),
            (&imported_getter_use, 1usize),
            (&computed_binding_use, 1usize),
            (&mutable_const_key_use, 1usize),
            (&computed_local_use, 1usize),
        ] {
            let file = result
                .files
                .iter()
                .find(|file| file.file_name == diagnostic_file.to_string_lossy())
                .unwrap_or_else(|| panic!("missing result for {}", diagnostic_file.display()));
            let diagnostics = file
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 2715)
                .collect::<Vec<_>>();
            assert_eq!(
                diagnostics.len(),
                expected_count,
                "{} diagnostics: {:?}",
                diagnostic_file.display(),
                file.diagnostics
            );
            if diagnostic_file == &destructured_list_use {
                for property in ["y", "z"] {
                    assert!(
                        diagnostics.iter().any(|diagnostic| diagnostic
                            .message
                            .contains(&format!("property '{property}'"))),
                        "{} must resolve the nested and tuple keys: {:?}",
                        diagnostic_file.display(),
                        file.diagnostics
                    );
                }
            } else if diagnostic_file == &qualified_static_positive_use
                || diagnostic_file == &reopened_static_positive_use
            {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property '[N.sym]'")),
                    "{} must preserve the qualified unique-symbol identity: {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            } else if diagnostic_file == &destructured_default_use
                || diagnostic_file == &computed_binding_use
                || diagnostic_file == &mutable_const_key_use
            {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property 'x'")),
                    "{} must preserve the default literal key: {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            } else if diagnostic_file == &imported_getter_use {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property '[sym]'")),
                    "{} must preserve the imported getter identity: {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            } else {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property 'y'")),
                    "{} must resolve the file-local key to 'y': {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            }
        }
        assert!(
            result
                .files
                .iter()
                .find(|file| file.file_name == destructured_object_rest_use.to_string_lossy())
                .is_some_and(|file| file
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == 2339
                        && diagnostic.message.contains("'key'"))),
            "object rest must omit consumed keys: {:?}",
            result.files
        );

        let mut reversed_options = CompilerOptions::default();
        reversed_options.no_emit = Some(true);
        let reversed_result = TsProject::new(reversed_paths, reversed_options).compile();
        let reversed_ts2715 = reversed_result
            .files
            .iter()
            .flat_map(|file| &file.diagnostics)
            .filter(|diagnostic| diagnostic.code == 2715)
            .count();
        assert_eq!(
            reversed_ts2715, 24,
            "reversed donor order diagnostics: {:?}",
            reversed_result.files
        );
        for clean_file in [
            &distinct_use,
            &class_distinct_use,
            &alias_use,
            &enum_use,
            &qualified_alias_use,
            &export_list_use,
            &default_alias_use,
            &namespace_qualified_use,
            &static_alias_use,
            &literal_class_use,
            &literal_object_use,
            &qualified_static_distinct_use,
            &reopened_static_distinct_use,
            &destructured_object_rest_use,
        ] {
            assert!(
                reversed_result
                    .files
                    .iter()
                    .find(|file| file.file_name == clean_file.to_string_lossy())
                    .is_some_and(|file| file
                        .diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.code != 2715)),
                "{} must remain clean with reversed donor order: {:?}",
                clean_file.display(),
                reversed_result.files
            );
        }
        for (diagnostic_file, expected_count) in [
            (&destructured_direct_use, 1usize),
            (&destructured_list_use, 2usize),
            (&qualified_static_positive_use, 1usize),
            (&static_getter_use, 1usize),
            (&destructured_default_use, 1usize),
            (&destructured_array_rest_use, 1usize),
            (&reopened_static_positive_use, 1usize),
            (&imported_getter_use, 1usize),
            (&computed_binding_use, 1usize),
            (&mutable_const_key_use, 1usize),
            (&computed_local_use, 1usize),
        ] {
            let file = reversed_result
                .files
                .iter()
                .find(|file| file.file_name == diagnostic_file.to_string_lossy())
                .unwrap_or_else(|| panic!("missing result for {}", diagnostic_file.display()));
            let diagnostics = file
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 2715)
                .collect::<Vec<_>>();
            assert_eq!(
                diagnostics.len(),
                expected_count,
                "{} reversed diagnostics: {:?}",
                diagnostic_file.display(),
                file.diagnostics
            );
            if diagnostic_file == &destructured_list_use {
                for property in ["y", "z"] {
                    assert!(
                        diagnostics.iter().any(|diagnostic| diagnostic
                            .message
                            .contains(&format!("property '{property}'"))),
                        "{} must resolve the nested and tuple keys in reversed donor order: {:?}",
                        diagnostic_file.display(),
                        file.diagnostics
                    );
                }
            } else if diagnostic_file == &qualified_static_positive_use
                || diagnostic_file == &reopened_static_positive_use
            {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property '[N.sym]'")),
                    "{} must preserve the qualified unique-symbol identity in reversed donor order: {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            } else if diagnostic_file == &destructured_default_use
                || diagnostic_file == &computed_binding_use
                || diagnostic_file == &mutable_const_key_use
            {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property 'x'")),
                    "{} must preserve the default literal key in reversed donor order: {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            } else if diagnostic_file == &imported_getter_use {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property '[sym]'")),
                    "{} must preserve the imported getter identity in reversed donor order: {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            } else {
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.message.contains("property 'y'")),
                    "{} must resolve the file-local key to 'y' in reversed donor order: {:?}",
                    diagnostic_file.display(),
                    file.diagnostics
                );
            }
        }
        assert!(
            reversed_result
                .files
                .iter()
                .find(|file| file.file_name == destructured_object_rest_use.to_string_lossy())
                .is_some_and(|file| file
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == 2339
                        && diagnostic.message.contains("'key'"))),
            "object rest must omit consumed keys in reversed donor order: {:?}",
            reversed_result.files
        );
    }
}
