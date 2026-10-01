//! Tooling query facade.
//!
//! `QueryEngine` is the single entry point for tooling queries. In this
//! crate it wraps the existing parser, binder, resolver, and checker.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Component, Path};

use rayon::prelude::*;
use tsc_rs_ast::{
    CompilerOptions, Diagnostic, DiagnosticCategory, ImportDecl, SourceFile, Span, StmtKind,
};
use tsc_rs_symbols::{
    SymbolTable, SYM_CLASS, SYM_CONSTRUCTOR, SYM_ENUM, SYM_ENUM_MEMBER, SYM_EXPORT, SYM_FUNCTION,
    SYM_IMPORT, SYM_INTERFACE, SYM_METHOD, SYM_MODULE, SYM_PROPERTY, SYM_STRING_MODULE,
    SYM_TYPE_ALIAS, SYM_VARIABLE,
};

// Autodiscovery feature gate. See `autodiscover_dts` below. Mirrors the
// implementation in `tsc_rs_project` so the QueryEngine code path (used
// for `tsc-rs -p tsconfig.json --noEmit` AND the LSP) gets the same
// behavior without adding a `tsc_rs_project` dependency (which would
// create a cycle).
//
// Defaults match `tsc_rs_project`: autodiscovery ON, opt out via
// `TSC_RS_AUTODISCOVER=0`. Bounds match too.
fn autodiscover_enabled() -> bool {
    match std::env::var("TSC_RS_AUTODISCOVER").as_deref() {
        Ok("0") | Ok("false") | Ok("FALSE") | Ok("no") => false,
        _ => true,
    }
}

fn autodiscover_max_bytes() -> u64 {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_FILE_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500_000)
}

fn autodiscover_max_files() -> usize {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_FILES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1500)
}

fn autodiscover_max_rounds() -> usize {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6)
}

fn autodiscover_max_per_package() -> usize {
    std::env::var("TSC_RS_AUTODISCOVER_MAX_PER_PACKAGE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(250)
}

fn collect_module_sources_for_autodisc(file: &SourceFile) -> Vec<String> {
    let mut sources = Vec::new();
    for stmt in &file.statements {
        match &stmt.kind {
            StmtKind::Import(import_decl) => sources.push(import_decl.source.to_string()),
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

fn node_modules_package_query(path: &str) -> Option<&str> {
    let marker = "/node_modules/";
    let last = path.rfind(marker)?;
    let after = &path[last + marker.len()..];
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

/// Recognise the three TypeScript declaration extensions a typed package can
/// legitimately publish: `.d.ts`, `.d.cts` (CJS-only — zod/v4 ships these),
/// and `.d.mts` (ESM-only). Must mirror `tsc_rs_project::is_dts_path`.
fn is_dts_path_query(path: &str) -> bool {
    path.ends_with(".d.ts") || path.ends_with(".d.cts") || path.ends_with(".d.mts")
}

fn autodisc_should_follow_query(from: &str, to: &str) -> bool {
    let from_pkg = node_modules_package_query(from);
    let to_pkg = node_modules_package_query(to);
    match (from_pkg, to_pkg) {
        (None, _) => true,
        (Some(_), None) => true,
        (Some(a), Some(b)) => a == b,
    }
}

/// Fast content hash for the parse cache. Uses
/// `std::collections::hash_map::DefaultHasher` over the source bytes —
/// not cryptographically strong, but it doesn't need to be: a collision
/// on changed content just means a cache miss → re-parse. Picked over
/// `FxHash` because hashing is rare (once per check_all per file) and
/// DefaultHasher avoids a new crate dep.
fn quick_source_hash(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn autodiscover_dts(parsed_files: &mut Vec<SourceFile>, options: &CompilerOptions) {
    let debug = std::env::var("TSC_RS_AUTODISCOVER_DEBUG").is_ok();
    if debug {
        eprintln!("[autodisc:query] entry: enabled={}", autodiscover_enabled());
    }
    if !autodiscover_enabled() {
        return;
    }
    let max_bytes = autodiscover_max_bytes();
    let max_files = autodiscover_max_files();
    let max_rounds = autodiscover_max_rounds();
    let max_per_package = autodiscover_max_per_package();
    let initial_count = parsed_files.len();
    let mut discovered: HashSet<String> =
        parsed_files.iter().map(|sf| sf.file_name.clone()).collect();
    // Per-package quota — see `tsc_rs_project::autodiscover_dts` for
    // rationale. Without this, apps/app's 280 direct imports balloon to
    // 2000+ in round 1 via `@types/node` (200+ files) etc.
    let mut pkg_counts: HashMap<String, usize> = HashMap::new();
    for sf in parsed_files.iter() {
        if let Some(pkg) = node_modules_package_query(&sf.file_name) {
            *pkg_counts.entry(pkg.to_string()).or_insert(0) += 1;
        }
    }
    let mut frontier: Vec<usize> = (0..parsed_files.len()).collect();
    for round in 0..max_rounds {
        if frontier.is_empty() || parsed_files.len().saturating_sub(initial_count) >= max_files {
            break;
        }
        let work: Vec<String> = frontier
            .par_iter()
            .flat_map_iter(|&idx| {
                let sf = &parsed_files[idx];
                let from = sf.file_name.clone();
                collect_module_sources_for_autodisc(sf)
                    .into_iter()
                    .filter_map(move |source| {
                        let resolved =
                            tsc_rs_resolver::resolve_module_name(&source, &from, options)?;
                        let path = resolved.resolved_file_name;
                        // Accept .d.ts, .d.cts (zod/v4-style CJS-only), .d.mts.
                        if !is_dts_path_query(&path) {
                            return None;
                        }
                        if !autodisc_should_follow_query(&from, &path) {
                            return None;
                        }
                        Some(path)
                    })
            })
            .collect();
        let mut to_parse: Vec<String> = Vec::new();
        for path in work {
            if !discovered.insert(path.clone()) {
                continue;
            }
            if max_per_package > 0 {
                if let Some(pkg) = node_modules_package_query(&path) {
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
                if max_bytes > 0 {
                    if let Ok(meta) = std::fs::metadata(path) {
                        if meta.len() > max_bytes {
                            return None;
                        }
                    }
                }
                std::fs::read_to_string(path)
                    .ok()
                    .map(|src| tsc_rs_parser::parse(path, &src))
            })
            .collect();
        if debug {
            eprintln!(
                "[autodisc:query] round {}: candidates={} new={}",
                round,
                to_parse.len(),
                newly_parsed.len()
            );
        }
        let start = parsed_files.len();
        parsed_files.extend(newly_parsed);
        frontier = (start..parsed_files.len()).collect();
    }
    if debug {
        eprintln!(
            "[autodisc:query] done: added {} files",
            parsed_files.len() - initial_count
        );
    }
}

const EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx"];
const INDEX_FILES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.d.ts",
    "index.js",
    "index.jsx",
];
const FALLBACK_STDLIB_SOURCE: &str = r#"
interface ObjectConstructor {
    assign<T extends object, U>(target: T, source: U): T & U;
    assign<T extends object, U, V>(target: T, source1: U, source2: V): T & U & V;
    assign<T extends object, U, V, W>(target: T, source1: U, source2: V, source3: W): T & U & V & W;
    assign(target: object, ...sources: any[]): any;
}

declare var Object: ObjectConstructor;
"#;

#[derive(Debug, Clone)]
struct FileAnalysis {
    symbols: SymbolTable,
    expression_types: HashMap<u32, String>,
    selected_overload_indices: HashMap<u32, usize>,
    type_ids_by_display: HashMap<String, u64>,
    /// Per-file diagnostics from the last full check. Re-emitted into
    /// `QueryEngine::diagnostics` on cache-hit (incremental) check_all
    /// passes so unchanged files retain their squigglies without
    /// re-running the type checker.
    diagnostics: Vec<Diagnostic>,
    /// Per-file `stable_types` from the last full check. Re-merged
    /// into `QueryEngine::type_index` on cache-hit passes.
    stable_types: HashMap<u64, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable,
    Class,
    Interface,
    TypeAlias,
    Enum,
    EnumMember,
    Module,
    Property,
    Method,
    Constructor,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    pub id: u64,
    pub name: String,
    pub kind: SymbolKind,
    pub type_id: Option<u64>,
    pub declarations: Vec<(String, Span)>,
    pub is_exported: bool,
}

#[derive(Debug, Clone)]
pub struct TypeMemberInfo {
    pub owner_name: String,
    pub member_name: String,
    pub flags: u32,
    pub declarations: Vec<(String, Span)>,
    pub type_display: Option<String>,
    pub overloads: Vec<tsc_rs_symbols::OverloadSignature>,
}

/// The central query API for ts-rs tooling.
///
/// All IDE features (hover, go-to-definition, diagnostics) can use this
/// facade instead of wiring parser/binder/checker directly.
pub struct QueryEngine {
    options: CompilerOptions,
    sources: HashMap<String, String>,
    /// BTreeMap (not HashMap): cross-file lookups iterate this — sorted
    /// order keeps "first match wins" deterministic across runs
    /// (goToDefinitionOverriddenMember8 flaked on HashMap order).
    analyses: std::collections::BTreeMap<String, FileAnalysis>,
    library_symbols: HashMap<String, SymbolTable>,
    diagnostics: Vec<Diagnostic>,
    type_index: HashMap<u64, String>,
    dependencies: HashMap<String, Vec<String>>,
    /// Parsed `SourceFile` cache keyed by file name + content hash. Lets
    /// `check_all` skip the parser for files whose source hasn't changed
    /// since the last invocation. Critical for the LSP hot path: an edit
    /// in one user file used to re-parse the full 1500-file
    /// autodiscovered .d.ts universe on every keystroke.
    ///
    /// Capped at `PARSE_CACHE_MAX_FILES` to bound memory. Eviction is
    /// content-hash-aware: a hash collision (= source changed) replaces
    /// the cached entry; otherwise the entry stays put.
    parse_cache: HashMap<String, (u64, SourceFile)>,
    /// Source content hash that was used to produce each entry in
    /// `analyses`. Together with the source hash at check_all time,
    /// lets the type-check pass skip files whose content hasn't changed
    /// since the last invocation — the dominant LSP latency win for
    /// projects with deep generic libraries (zod, prisma, trpc) where
    /// re-checking the full module graph took 30s+ per keystroke even
    /// with the parse cache warm. Files missing from this map are
    /// (re)checked; files present with a matching hash reuse their
    /// existing analysis.
    analyzed_hash: HashMap<String, u64>,
}

const PARSE_CACHE_MAX_FILES: usize = 4000;

impl QueryEngine {
    pub fn new() -> Self {
        Self::with_options(CompilerOptions::default())
    }

    pub fn with_options(options: CompilerOptions) -> Self {
        Self {
            options,
            sources: HashMap::new(),
            analyses: std::collections::BTreeMap::new(),
            library_symbols: HashMap::new(),
            diagnostics: Vec::new(),
            type_index: HashMap::new(),
            dependencies: HashMap::new(),
            parse_cache: HashMap::new(),
            analyzed_hash: HashMap::new(),
        }
    }

    /// Borrow this QE's options. Used by the LSP to decide whether the
    /// cached QE for a URI can be reused across edits (caller compares
    /// via `hash_compiler_options` to avoid relying on
    /// `CompilerOptions: PartialEq`).
    pub fn options(&self) -> &CompilerOptions {
        &self.options
    }

    pub fn set_options(&mut self, options: CompilerOptions) {
        self.options = options;
        self.analyses.clear();
        self.library_symbols.clear();
        self.diagnostics.clear();
        self.type_index.clear();
        self.dependencies.clear();
        // Compiler options affect parse output (e.g. JSX mode); drop
        // the parse cache too. `analyzed_hash` must follow `analyses`.
        self.parse_cache.clear();
        self.analyzed_hash.clear();
    }

    /// Add (or replace) a source file in the query engine.
    ///
    /// Intentionally does NOT clear `analyses` / `analyzed_hash` —
    /// those are content-hash-keyed, so the stale entry for this file
    /// is naturally rejected in the next `check_all` (its hash will
    /// mismatch). Keeping the other files' cached state intact is
    /// what makes per-keystroke re-checks fast in the LSP path.
    pub fn add_source(&mut self, path: String, source: String) -> Result<(), String> {
        if path.trim().is_empty() {
            return Err("source path cannot be empty".to_string());
        }

        let normalized = normalize_path(&path);
        self.sources.insert(normalized, source);
        Ok(())
    }

    /// Run parser/binder/checker over all sources currently loaded.
    pub fn check_all(&mut self) -> Result<(), Vec<Diagnostic>> {
        // KEEP `self.analyses` and `self.analyzed_hash` across calls —
        // they are the incremental cache. Entries are invalidated below
        // when a file's source hash changes. Cross-file maps
        // (library_symbols, diagnostics, type_index, dependencies) are
        // rebuilt every call: cheaper to recompute than to maintain
        // incrementally, and `type_index` is restored from each
        // surviving FileAnalysis's `stable_types`.
        self.library_symbols.clear();
        self.diagnostics.clear();
        self.type_index.clear();
        self.dependencies.clear();
        // Drop cached analyses for files no longer in `sources` (the
        // LSP doesn't currently remove sources, but the API allows it).
        self.analyses.retain(|f, _| self.sources.contains_key(f));
        self.analyzed_hash
            .retain(|f, _| self.sources.contains_key(f));

        let mut file_names: Vec<String> = self.sources.keys().cloned().collect();
        file_names.sort();

        // Parse in parallel, consulting the parse cache. For files whose
        // (file_name, content_hash) matches a cached entry, we reuse the
        // parsed `SourceFile` instead of re-running the parser. This is
        // the critical LSP hot-path optimization: an edit in one user
        // file used to re-parse every autodiscovered .d.ts file on every
        // keystroke. With the cache, only the changed file is re-parsed.
        //
        // Diagnostics for cache-hit files are skipped here — they were
        // already merged into `self.diagnostics` on the original parse,
        // and `self.diagnostics` is cleared at the top of `check_all`,
        // so we re-emit by cloning from the cached SourceFile's own
        // diagnostics vector.
        let parse_pass: Vec<(SourceFile, Vec<Diagnostic>, bool)> = file_names
            .par_iter()
            .map(|file_name| {
                let source = self
                    .sources
                    .get(file_name)
                    .expect("file name derived from sources map");
                let hash = quick_source_hash(source);
                // Look up in cache. We can't mutate `self.parse_cache`
                // from a parallel iterator, so this branch returns a
                // clone of the cached entry on a hit and parses on miss.
                if let Some((cached_hash, cached_sf)) = self.parse_cache.get(file_name) {
                    if *cached_hash == hash {
                        let parse_diags: Vec<Diagnostic> = cached_sf
                            .diagnostics
                            .iter()
                            .cloned()
                            .map(|diag| with_file_name(diag, file_name))
                            .collect();
                        return (cached_sf.clone(), parse_diags, true);
                    }
                }
                let parsed = tsc_rs_parser::parse(file_name, source);
                let parse_diags: Vec<Diagnostic> = parsed
                    .diagnostics
                    .iter()
                    .cloned()
                    .map(|diag| with_file_name(diag, file_name))
                    .collect();
                (parsed, parse_diags, false)
            })
            .collect();

        let mut parsed_files = Vec::with_capacity(parse_pass.len());
        let mut parse_misses: Vec<(String, SourceFile)> = Vec::new();
        for (parsed, parse_diags, was_cache_hit) in parse_pass {
            self.diagnostics.extend(parse_diags);
            if !was_cache_hit {
                parse_misses.push((parsed.file_name.clone(), parsed.clone()));
            }
            parsed_files.push(parsed);
        }
        // Insert cache misses serially after the parallel pass. Bound
        // the cache: if we'd grow past the cap, drop the misses for
        // files we haven't seen before rather than evicting at random
        // (avoids evicting hot files like the user's open buffer).
        for (file_name, parsed) in parse_misses {
            if self.parse_cache.contains_key(&file_name)
                || self.parse_cache.len() < PARSE_CACHE_MAX_FILES
            {
                let hash = quick_source_hash(
                    self.sources
                        .get(&file_name)
                        .expect("file_name from sources map"),
                );
                self.parse_cache.insert(file_name, (hash, parsed));
            }
        }

        // Autodiscover transitively-reachable `.d.ts` files. See
        // `autodiscover_dts` for rationale + env var controls. Default OFF;
        // mirrors the same phase in `compile_selected` /
        // `open_check_session`. The QueryEngine path is what the `--noEmit`
        // CLI invocation (`tsc-rs -p tsconfig.json`) takes when
        // `should_use_query_pipeline` returns true — without it the env
        // var would have no effect for the most common invocation.
        //
        // `file_names` / `file_index` must be rebuilt to reflect the
        // expanded `parsed_files` before downstream loops index into them.
        let pre_autodisc_count = parsed_files.len();
        autodiscover_dts(&mut parsed_files, &self.options);
        // Persist autodiscovered files into `self.sources` + `parse_cache`
        // so subsequent `check_all` invocations start from the post-
        // autodiscovery set. Without this, every LSP keystroke would
        // re-walk all 1500 .d.ts files from scratch (parse, BFS, etc.).
        // The parse_cache also lets the parallel parse pass above skip
        // those files entirely on the next pass.
        for sf in parsed_files.iter().skip(pre_autodisc_count) {
            if !self.sources.contains_key(&sf.file_name) {
                let hash = quick_source_hash(&sf.text);
                self.sources.insert(sf.file_name.clone(), sf.text.clone());
                if self.parse_cache.len() < PARSE_CACHE_MAX_FILES {
                    self.parse_cache
                        .insert(sf.file_name.clone(), (hash, sf.clone()));
                }
            }
        }
        let mut file_names: Vec<String> =
            parsed_files.iter().map(|sf| sf.file_name.clone()).collect();

        // Bind in parallel — SymbolTables are independent per file.
        let mut symbol_tables: Vec<SymbolTable> =
            parsed_files.par_iter().map(tsc_rs_symbols::bind).collect();

        let file_index: HashMap<String, usize> = file_names
            .iter()
            .enumerate()
            .map(|(i, path)| (path.clone(), i))
            .collect();
        let mut available_file_names: HashSet<String> = file_names.iter().cloned().collect();

        let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
        for file_idx in 0..parsed_files.len() {
            let imports = collect_imports(&parsed_files[file_idx]);
            let current_file = parsed_files[file_idx].file_name.clone();
            let mut deps_for_file = Vec::new();
            for import in &imports {
                if let Some(resolved) = tsc_rs_resolver::resolve_module_name(
                    &import.source,
                    &parsed_files[file_idx].file_name,
                    &self.options,
                ) {
                    available_file_names.insert(normalize_path(&resolved.resolved_file_name));
                }
                if let Some(target_idx) = resolve_import_target(
                    &import.source,
                    &parsed_files[file_idx].file_name,
                    &self.options,
                    &file_index,
                ) {
                    deps_for_file.push(parsed_files[target_idx].file_name.clone());
                    if target_idx != file_idx {
                        let target_table = symbol_tables[target_idx].clone();
                        tsc_rs_symbols::link_imports(
                            &mut symbol_tables[file_idx],
                            &target_table,
                            import,
                        );
                    }
                }
            }
            deps_for_file.sort();
            deps_for_file.dedup();
            if !deps_for_file.is_empty() {
                dependencies.insert(current_file, deps_for_file);
            }
        }
        self.dependencies = dependencies;
        let mut available_file_names: Vec<String> = available_file_names.into_iter().collect();
        available_file_names.sort();

        let stdlib_files: Vec<tsc_rs_ast::SourceFile> =
            tsc_rs_types::load_stdlib_sources(&self.options)
                .into_iter()
                .map(|lib| tsc_rs_parser::parse(&lib.file_name, &lib.source))
                .collect();
        self.library_symbols = if stdlib_files.is_empty() {
            let fallback = tsc_rs_parser::parse("__fallback_lib__.d.ts", FALLBACK_STDLIB_SOURCE);
            HashMap::from([(
                normalize_path(&fallback.file_name),
                tsc_rs_symbols::bind(&fallback),
            )])
        } else {
            stdlib_files
                .iter()
                .map(|lib| (normalize_path(&lib.file_name), tsc_rs_symbols::bind(lib)))
                .collect()
        };

        // Collect references to all parsed files for cross-file interface injection.
        let mut all_parsed_refs: Vec<&tsc_rs_ast::SourceFile> = parsed_files.iter().collect();
        all_parsed_refs.extend(stdlib_files.iter());

        // Per-file check is independent: each spins up a fresh TypeChecker
        // and produces an isolated TypeCheckOutput. Run them in parallel and
        // collect the side-effect tuples; merge into QueryEngine state below
        // on the main thread so the mutation order stays deterministic.
        //
        // Pre-build a "donor" TypeChecker with the project-wide cross-file
        // type tables populated ONCE (interface_info, class_info, enum_info,
        // type_aliases, global scope decls). Each per-file checker clones
        // the donor instead of re-walking every other file's AST. Without
        // this, `inject_external_types(&other_files)` was called N times
        // per project, each pass O(N) → O(N^2) total. Cloning the donor's
        // HashMap state is much cheaper than the AST walk + decl
        // reconstruction. See `compile_selected` for the same pattern.
        struct CheckResult {
            file_name: String,
            symbols: SymbolTable,
            diagnostics: Vec<Diagnostic>,
            expression_types: HashMap<u32, String>,
            selected_overload_indices: HashMap<u32, usize>,
            type_ids_by_display: HashMap<String, u64>,
            stable_types: HashMap<u64, String>,
        }

        // Project-wide "donor" TypeChecker: see the same pattern in
        // `tsc_rs_project::TsProject::compile_selected`. TypeChecker is
        // `Sync` (atomic recursion counter + Mutex cycle-detection set), so
        // the donor can be safely shared across rayon workers via `&donor`
        // — each worker clones it for its own file pass.
        let donor: Option<tsc_rs_types::TypeChecker> = if all_parsed_refs.is_empty() {
            None
        } else {
            let mut d = tsc_rs_types::TypeChecker::new();
            d.enable_module_resolution_diagnostics();
            d.inject_external_types(&all_parsed_refs);
            d.register_available_files(&available_file_names);
            d.take_diagnostics();
            Some(d)
        };

        // Partition files into (needs_recheck, cache_hit). A file is
        // a cache hit when:
        //   1. It already has a FileAnalysis from a prior check_all
        //   2. Its source content hash hasn't changed
        // For cache hits we skip the type-check (the dominant cost
        // for projects with heavy generic libraries like zod/trpc:
        // ~30 s for 152 files on every keystroke in the LSP path).
        // The cached FileAnalysis already has symbols + types +
        // diagnostics; we re-emit those into the QE's roll-up maps.
        let per_file_inputs: Vec<(String, &SourceFile, SymbolTable, u64)> = file_names
            .into_iter()
            .zip(parsed_files.iter())
            .zip(symbol_tables.into_iter())
            .map(|((file_name, parsed), symbols)| {
                let hash = self
                    .sources
                    .get(&file_name)
                    .map(|s| quick_source_hash(s))
                    .unwrap_or(0);
                (file_name, parsed, symbols, hash)
            })
            .collect();

        let (cache_hits, needs_recheck): (Vec<_>, Vec<_>) =
            per_file_inputs
                .into_iter()
                .partition(|(file_name, _, _, hash)| {
                    self.analyzed_hash.get(file_name) == Some(hash)
                        && self.analyses.contains_key(file_name)
                });

        // Cache-hit fast path: re-emit cached state without invoking
        // the type checker. Sequential since it just touches HashMaps.
        for (file_name, _parsed, _symbols, _hash) in cache_hits {
            if let Some(analysis) = self.analyses.get(&file_name) {
                self.diagnostics
                    .extend(analysis.diagnostics.iter().cloned());
                for (id, ty) in &analysis.stable_types {
                    self.type_index.entry(*id).or_insert_with(|| ty.clone());
                }
                for ty in analysis.expression_types.values() {
                    if !analysis.type_ids_by_display.contains_key(ty) {
                        let fallback_id = stable_type_id(ty);
                        self.type_index
                            .entry(fallback_id)
                            .or_insert_with(|| ty.clone());
                    }
                }
            }
        }

        let check_results: Vec<CheckResult> = needs_recheck
            .into_par_iter()
            .map(|(file_name, parsed, symbols, _hash)| {
                let mut checker = match &donor {
                    Some(d) => d.clone(),
                    None => {
                        let mut c = tsc_rs_types::TypeChecker::new();
                        c.enable_module_resolution_diagnostics();
                        c.register_available_files(&available_file_names);
                        c
                    }
                };
                checker.set_current_file_name(&file_name);
                let check_output = checker.check_with_options(parsed, &symbols, &self.options);
                let type_ids_by_display = build_display_id_map(&check_output.stable_types);
                let diagnostics: Vec<Diagnostic> = check_output
                    .diagnostics
                    .into_iter()
                    .map(|diag| with_file_name(diag, &file_name))
                    .collect();
                CheckResult {
                    file_name,
                    symbols,
                    diagnostics,
                    expression_types: check_output.expression_types,
                    selected_overload_indices: check_output.selected_overload_indices,
                    type_ids_by_display,
                    stable_types: check_output.stable_types,
                }
            })
            .collect();

        for r in check_results {
            for (type_id, ty) in &r.stable_types {
                self.type_index
                    .entry(*type_id)
                    .or_insert_with(|| ty.clone());
            }
            for ty in r.expression_types.values() {
                if !r.type_ids_by_display.contains_key(ty) {
                    let fallback_id = stable_type_id(ty);
                    self.type_index
                        .entry(fallback_id)
                        .or_insert_with(|| ty.clone());
                }
            }
            self.diagnostics.extend(r.diagnostics.iter().cloned());
            let hash = self
                .sources
                .get(&r.file_name)
                .map(|s| quick_source_hash(s))
                .unwrap_or(0);
            self.analyzed_hash.insert(r.file_name.clone(), hash);
            self.analyses.insert(
                r.file_name,
                FileAnalysis {
                    symbols: r.symbols,
                    expression_types: r.expression_types,
                    selected_overload_indices: r.selected_overload_indices,
                    type_ids_by_display: r.type_ids_by_display,
                    diagnostics: r.diagnostics,
                    stable_types: r.stable_types,
                },
            );
        }

        if self
            .diagnostics
            .iter()
            .any(|d| d.category == DiagnosticCategory::Error)
        {
            Err(self.diagnostics.clone())
        } else {
            Ok(())
        }
    }

    /// Get the stable type id at (or near) a given file offset.
    pub fn get_type_at(&self, file: &str, offset: u32) -> Option<u64> {
        let file = normalize_path(file);
        let analysis = self.analyses.get(&file)?;
        let ty = lookup_expression_type(&analysis.expression_types, offset)?;
        Some(resolve_stable_type_id(analysis, ty))
    }

    /// Get the symbol id at a given file offset.
    pub fn get_symbol_at(&self, file: &str, offset: u32) -> Option<u64> {
        let file = normalize_path(file);
        let analysis = self.analyses.get(&file)?;
        find_symbol_at_offset(&analysis.symbols, offset)
    }

    /// Get all diagnostics for the current project, formatted as strings.
    pub fn diagnostics(&self) -> Vec<String> {
        self.diagnostics.iter().map(format_diagnostic).collect()
    }

    /// Get diagnostics for a single file.
    pub fn file_diagnostics(&self, file: &str) -> Vec<String> {
        let file = normalize_path(file);
        self.diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name
                    .as_deref()
                    .map(normalize_path)
                    .as_deref()
                    .is_some_and(|diag_file| diag_file == file)
            })
            .map(format_diagnostic)
            .collect()
    }

    /// Get diagnostics at a specific location.
    pub fn diagnostics_at(&self, file: &str, offset: u32) -> Vec<String> {
        let file = normalize_path(file);
        let exact: Vec<String> = self
            .diagnostics
            .iter()
            .filter(|diag| {
                let same_file = diag
                    .file_name
                    .as_deref()
                    .map(normalize_path)
                    .as_deref()
                    .is_some_and(|diag_file| diag_file == file);
                if !same_file {
                    return false;
                }
                diag.span
                    .is_none_or(|span| span.start <= offset && offset <= span.end)
            })
            .map(format_diagnostic)
            .collect();
        if !exact.is_empty() {
            return exact;
        }

        let mut nearest: Vec<(u32, &Diagnostic)> = self
            .diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name
                    .as_deref()
                    .map(normalize_path)
                    .as_deref()
                    .is_some_and(|diag_file| diag_file == file)
            })
            .filter_map(|diag| {
                let span = diag.span?;
                let distance = if offset < span.start {
                    span.start - offset
                } else if offset > span.end {
                    offset - span.end
                } else {
                    0
                };
                (distance <= 16).then_some((distance, diag))
            })
            .collect();
        nearest.sort_by_key(|(distance, _)| *distance);
        nearest
            .into_iter()
            .map(|(_, diag)| format_diagnostic(diag))
            .collect()
    }

    /// Number of error diagnostics currently recorded.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.category == DiagnosticCategory::Error)
            .count()
    }

    /// Find symbols by exact name across all analyzed files.
    pub fn find_symbol(&self, name: &str) -> Vec<u64> {
        let mut ids = Vec::new();
        for analysis in self.analyses.values() {
            for symbol in &analysis.symbols.symbols {
                if symbol.name == name {
                    ids.push(u64::from(symbol.id));
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// `SymbolInfo` of every symbol named `name`, each read from its own
    /// file's table (unlike `find_symbol` + `get_symbol_info`, which look a
    /// per-file id up in whichever file has that index first). Ordered by
    /// file path, then declaration position.
    pub fn find_symbol_infos(&self, name: &str) -> Vec<SymbolInfo> {
        let mut files: Vec<&String> = self.analyses.keys().collect();
        files.sort();
        let mut out = Vec::new();
        for file in files {
            let analysis = &self.analyses[file];
            for symbol in &analysis.symbols.symbols {
                if symbol.name != name {
                    continue;
                }
                let declarations = symbol
                    .declarations
                    .iter()
                    .map(|d| (normalize_path(&d.file_name), d.span))
                    .collect::<Vec<_>>();
                let type_id = symbol.declarations.first().and_then(|d| {
                    let ty = type_for_span(analysis, d.span)?;
                    Some(resolve_stable_type_id(analysis, ty))
                });
                out.push(SymbolInfo {
                    id: u64::from(symbol.id),
                    name: symbol.name.clone(),
                    kind: symbol_kind_from_flags(symbol.flags),
                    type_id,
                    declarations,
                    is_exported: symbol.flags & SYM_EXPORT != 0,
                });
            }
        }
        out
    }

    /// Declaration locations of every symbol named `name`, across files.
    ///
    /// Symbol ids are per-file, so `find_symbol` + `get_symbol_locations`
    /// mixes up same-numbered symbols of different files; this pairs each
    /// declaration with the table it came from. Grouped per symbol in
    /// (file, first declaration) order.
    pub fn find_symbol_declarations(&self, name: &str) -> Vec<Vec<(String, Span)>> {
        let mut groups: Vec<Vec<(String, Span)>> = Vec::new();
        for analysis in self.analyses.values() {
            for symbol in &analysis.symbols.symbols {
                if symbol.name != name || symbol.declarations.is_empty() {
                    continue;
                }
                let mut locs: Vec<(String, Span)> = symbol
                    .declarations
                    .iter()
                    .map(|decl| (normalize_path(&decl.file_name), decl.span))
                    .collect();
                locs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.start.cmp(&b.1.start)));
                locs.dedup();
                groups.push(locs);
            }
        }
        groups.sort_by(|a, b| {
            a[0].0
                .cmp(&b[0].0)
                .then_with(|| a[0].1.start.cmp(&b[0].1.start))
        });
        groups.dedup();
        groups
    }

    /// Get declaration locations for a symbol.
    pub fn get_symbol_locations(&self, symbol_id: u64) -> Vec<(String, Span)> {
        let Some(symbol_id) = as_u32(symbol_id) else {
            return Vec::new();
        };

        let mut locations = Vec::new();
        for analysis in self.analyses.values() {
            if let Some(symbol) = analysis.symbols.get_symbol(symbol_id) {
                for decl in &symbol.declarations {
                    locations.push((normalize_path(&decl.file_name), decl.span));
                }
            }
        }
        locations.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.start.cmp(&b.1.start)));
        locations.dedup();
        locations
    }

    /// Get known uses (identifier positions) of a symbol.
    pub fn get_symbol_uses(&self, symbol_id: u64) -> Vec<(String, Span)> {
        let Some(symbol_id) = as_u32(symbol_id) else {
            return Vec::new();
        };

        let mut uses = Vec::new();
        for (file_name, analysis) in &self.analyses {
            for (&pos, &mapped_id) in &analysis.symbols.position_to_symbol {
                if mapped_id == symbol_id {
                    let end = analysis
                        .symbols
                        .get_symbol(mapped_id)
                        .map(|s| pos.saturating_add(s.name.len() as u32))
                        .unwrap_or_else(|| pos.saturating_add(1));
                    uses.push((file_name.clone(), Span::new(pos, end)));
                }
            }
        }

        uses.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.start.cmp(&b.1.start)));
        uses.dedup();
        uses
    }

    /// Get an inferred type id for a symbol when available.
    pub fn get_symbol_type(&self, symbol_id: u64) -> Option<u64> {
        let symbol_id = as_u32(symbol_id)?;
        let (file_name, span) = self
            .get_symbol_locations(u64::from(symbol_id))
            .into_iter()
            .next()?;
        let analysis = self.analyses.get(&file_name)?;
        let ty = type_for_span(analysis, span)?;
        Some(resolve_stable_type_id(analysis, ty))
    }

    /// Like `get_symbol_type` but only consults `preferred_file`'s analysis.
    /// Avoids the cross-file symbol-id collision where two unrelated files
    /// happen to mint the same local `symbol_id` (the IDs are per-file and
    /// `get_symbol_locations` previously returned matches from every file).
    /// Hover/definition flows that originate from a known file should use
    /// this form so the LSP doesn't reach into `zod/...d.ts` for a local
    /// `const deepName` query.
    pub fn get_symbol_type_in_file(&self, symbol_id: u64, file_name: &str) -> Option<u64> {
        let symbol_id = as_u32(symbol_id)?;
        let file = normalize_path(file_name);
        let analysis = self.analyses.get(&file)?;
        let symbol = analysis.symbols.get_symbol(symbol_id)?;
        let span = symbol.declarations.first()?.span;
        let ty = type_for_span(analysis, span)?;
        Some(resolve_stable_type_id(analysis, ty))
    }

    /// Resolve a type reference to a stable id.
    pub fn resolve_type_ref(&self, type_ref: &str, from_file: &str) -> Option<u64> {
        let type_ref = type_ref.trim();
        if type_ref.is_empty() {
            return None;
        }

        let primitive = match type_ref {
            "string" => Some(tsc_rs_types::Type::String),
            "number" => Some(tsc_rs_types::Type::Number),
            "boolean" => Some(tsc_rs_types::Type::Boolean),
            "undefined" => Some(tsc_rs_types::Type::Undefined),
            "null" => Some(tsc_rs_types::Type::Null),
            "any" => Some(tsc_rs_types::Type::Any),
            "unknown" => Some(tsc_rs_types::Type::Unknown),
            "never" => Some(tsc_rs_types::Type::Never),
            "void" => Some(tsc_rs_types::Type::Void),
            _ => None,
        };
        if let Some(ty) = primitive {
            return Some(stable_type_id_for_type(&ty));
        }

        let symbols = self.find_symbol(type_ref);
        if let Some(sym_id) = symbols.first().copied() {
            if let Some(ty) = self.get_symbol_type(sym_id) {
                return Some(ty);
            }
        }

        self.get_type_at(from_file, 0)
    }

    /// Get detailed information about a symbol.
    pub fn get_symbol_info(&self, symbol_id: u64) -> Option<SymbolInfo> {
        let symbol_id_u32 = as_u32(symbol_id)?;
        for analysis in self.analyses.values() {
            if let Some(symbol) = analysis.symbols.get_symbol(symbol_id_u32) {
                let declarations = symbol
                    .declarations
                    .iter()
                    .map(|d| (normalize_path(&d.file_name), d.span))
                    .collect::<Vec<_>>();
                return Some(SymbolInfo {
                    id: symbol_id,
                    name: symbol.name.clone(),
                    kind: symbol_kind_from_flags(symbol.flags),
                    type_id: self.get_symbol_type(symbol_id),
                    declarations,
                    is_exported: symbol.flags & SYM_EXPORT != 0,
                });
            }
        }
        None
    }

    /// Retrieve a known type string by stable id.
    pub fn get_type(&self, type_id: u64) -> Option<String> {
        self.type_index.get(&type_id).cloned()
    }

    /// Convert a stable type id back to a display string.
    pub fn type_to_string(&self, type_id: u64) -> Option<String> {
        self.get_type(type_id)
    }

    /// Get source text for a file loaded in the query engine.
    pub fn get_source_file(&self, file: &str) -> Option<&str> {
        let file = normalize_path(file);
        self.sources.get(&file).map(String::as_str)
    }

    /// Get direct dependencies (resolved imports) for a file.
    pub fn get_dependencies(&self, file: &str) -> Vec<String> {
        let file = normalize_path(file);
        self.dependencies.get(&file).cloned().unwrap_or_default()
    }

    /// Get all expression types for a specific file.
    /// Returns a vec of (offset, type_string) pairs.
    pub fn get_expression_types(&self, file: &str) -> Vec<(u32, String)> {
        let file = normalize_path(file);
        self.analyses
            .get(&file)
            .map(|a| {
                a.expression_types
                    .iter()
                    .map(|(&k, v)| (k, v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find the declaration of a member (property/method) within a type (interface/class).
    ///
    /// Searches all analyzed files for a symbol named `type_name` that has a
    /// member named `member_name` in its `members` map, then returns the
    /// member's first declaration location.
    pub fn find_member_declaration(
        &self,
        type_name: &str,
        member_name: &str,
    ) -> Option<(String, Span)> {
        self.get_type_member_info(type_name, member_name)
            .and_then(|member| member.declarations.into_iter().next())
    }

    /// Get hover display info for a member of a type (interface/class).
    ///
    /// Searches all analyses for a type named `type_name` with a member named
    /// `member_name` and returns a formatted display string.
    pub fn get_member_hover(&self, type_name: &str, member_name: &str) -> Option<String> {
        let member = self.get_type_member_info(type_name, member_name)?;
        let is_method = member.flags & SYM_METHOD != 0 || member.flags & SYM_FUNCTION != 0;
        let prefix = if is_method { "(method)" } else { "(property)" };
        let qualified = format!("{}.{}", type_name, member_name);
        match member.type_display {
            Some(ts) => Some(format!("{} {}: {}", prefix, qualified, ts)),
            None => Some(format!("{} {}", prefix, qualified)),
        }
    }

    pub fn get_type_member_info(
        &self,
        type_name: &str,
        member_name: &str,
    ) -> Option<TypeMemberInfo> {
        for analysis in self.analyses.values() {
            for sym in &analysis.symbols.symbols {
                let is_type_like = sym.flags
                    & (SYM_INTERFACE
                        | SYM_CLASS
                        | SYM_TYPE_ALIAS
                        | SYM_VARIABLE
                        | SYM_MODULE
                        | SYM_ENUM)
                    != 0;
                if !is_type_like || sym.name != type_name {
                    continue;
                }
                let Some(member_id) = sym
                    .members
                    .get(member_name)
                    .copied()
                    .or_else(|| sym.exports.get(member_name).copied())
                else {
                    continue;
                };
                let Some(member_sym) = analysis.symbols.get_symbol(member_id) else {
                    continue;
                };
                return Some(TypeMemberInfo {
                    owner_name: sym.name.clone(),
                    member_name: member_name.to_string(),
                    flags: member_sym.flags,
                    declarations: member_sym
                        .declarations
                        .iter()
                        .map(|decl| (normalize_path(&decl.file_name), decl.span))
                        .collect(),
                    type_display: member_sym
                        .declarations
                        .first()
                        .and_then(|decl| analysis.expression_types.get(&decl.span.start))
                        .cloned(),
                    overloads: analysis.symbols.get_overload_signatures(member_id).to_vec(),
                });
            }
        }
        for symbols in self.library_symbols.values() {
            for sym in &symbols.symbols {
                let is_type_like = sym.flags
                    & (SYM_INTERFACE
                        | SYM_CLASS
                        | SYM_TYPE_ALIAS
                        | SYM_VARIABLE
                        | SYM_MODULE
                        | SYM_ENUM)
                    != 0;
                if !is_type_like || sym.name != type_name {
                    continue;
                }
                let Some(member_id) = sym
                    .members
                    .get(member_name)
                    .copied()
                    .or_else(|| sym.exports.get(member_name).copied())
                else {
                    continue;
                };
                let Some(member_sym) = symbols.get_symbol(member_id) else {
                    continue;
                };
                return Some(TypeMemberInfo {
                    owner_name: sym.name.clone(),
                    member_name: member_name.to_string(),
                    flags: member_sym.flags,
                    declarations: member_sym
                        .declarations
                        .iter()
                        .map(|decl| (normalize_path(&decl.file_name), decl.span))
                        .collect(),
                    type_display: None,
                    overloads: symbols.get_overload_signatures(member_id).to_vec(),
                });
            }
        }
        None
    }

    /// Get members of a type (interface/class) for completion.
    ///
    /// Returns (member_name, flags, optional_type_string) tuples.
    pub fn get_type_member_completions(
        &self,
        type_name: &str,
    ) -> Vec<(String, u32, Option<String>)> {
        let mut items = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        // Walk the extends chain (BFS): start from `type_name`, follow
        // `extends` clauses to base interfaces/classes, accumulate
        // members from each. Capped at 8 levels deep to avoid runaway
        // recursion through pathological chains. Without this walk,
        // `z.object().parse(…)` had no completion because `parse` is
        // defined on `_ZodType`/`$ZodType`, not on `ZodObject` itself.
        let mut visited: HashSet<String> = HashSet::new();
        let mut frontier: Vec<String> = vec![type_name.to_string()];
        let mut depth = 0usize;
        while !frontier.is_empty() && depth < 8 {
            let current = std::mem::take(&mut frontier);
            for name in &current {
                if !visited.insert(name.clone()) {
                    continue;
                }
                let mut parents: Vec<String> = Vec::new();
                for analysis in self.analyses.values() {
                    if let Some(ps) = analysis.symbols.extends_map.get(name) {
                        for p in ps {
                            parents.push(p.clone());
                        }
                    }
                    for sym in &analysis.symbols.symbols {
                        let is_type_like = sym.flags
                            & (SYM_INTERFACE | SYM_CLASS | SYM_TYPE_ALIAS | SYM_VARIABLE)
                            != 0;
                        if !is_type_like || &sym.name != name || sym.members.is_empty() {
                            continue;
                        }
                        for (member_name, &member_id) in &sym.members {
                            // Resolve the member symbol BEFORE marking
                            // `seen`. `link_imports` leaves `members[name]
                            // = id` pointing at a symbol whose owning
                            // analysis is a different file (the re-
                            // exporting one). `analysis.symbols
                            // .get_symbol(id)` then returns None even
                            // though the member exists upstream. If we
                            // marked `seen` first, the upstream analysis
                            // (iterated later) would be blocked from
                            // contributing the real member.
                            let Some(member_sym) = analysis.symbols.get_symbol(member_id) else {
                                continue;
                            };
                            if !seen.insert(member_name.clone()) {
                                continue;
                            }
                            let detail = member_sym
                                .declarations
                                .first()
                                .and_then(|d| analysis.expression_types.get(&d.span.start))
                                .cloned();
                            items.push((member_name.clone(), member_sym.flags, detail));
                        }
                    }
                }
                // Heritage targets often carry generic arguments
                // (`extends _ZodType<core.$ZodObjectInternals<…>>`) and
                // namespace prefixes (`extends core.$ZodObject<…>`).
                // Strip both before queueing — `extends_map` keys and
                // symbol names are stored as bare local names. We also
                // queue both the namespaced and bare forms so projects
                // that store the heritage verbatim still resolve.
                for p in parents {
                    let no_generics = p.split('<').next().unwrap_or(&p).trim().to_string();
                    if !no_generics.is_empty() {
                        frontier.push(no_generics.clone());
                    }
                    if let Some(last) = no_generics.rsplit('.').next() {
                        if last != no_generics {
                            frontier.push(last.to_string());
                        }
                    }
                }
            }
            depth += 1;
        }
        items
    }

    /// Get all top-level symbols across all analyzed files.
    /// Used for scope-level completions to include globals from lib stubs.
    /// Overload signatures for a global function declared in the loaded
    /// stdlib (`eval`, `parseInt`, ...). User files shadow the lib, so this
    /// is only consulted after per-file symbol lookup fails.
    pub fn lib_function_overloads(
        &self,
        name: &str,
    ) -> Option<Vec<tsc_rs_symbols::OverloadSignature>> {
        for table in self.library_symbols.values() {
            for sym in &table.symbols {
                if sym.name == name && sym.flags & SYM_FUNCTION != 0 {
                    if let Some(ovs) = table.overload_signatures.get(&sym.id) {
                        if !ovs.is_empty() {
                            return Some(ovs.clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// Collect overload signatures for a GLOBAL (script-scope) function
    /// declared in any analyzed file — cross-file globals are visible without
    /// imports, but per-file symbol tables are never merged.
    pub fn global_function_overloads(&self, name: &str) -> Vec<tsc_rs_symbols::OverloadSignature> {
        let mut out = Vec::new();
        for analysis in self.analyses.values() {
            for sym in &analysis.symbols.symbols {
                if sym.name == name && sym.flags & SYM_FUNCTION != 0 && sym.flags & SYM_IMPORT == 0
                {
                    if let Some(ovs) = analysis.symbols.overload_signatures.get(&sym.id) {
                        out.extend(ovs.iter().cloned());
                    }
                }
            }
        }
        out
    }

    /// Source text of a global function's declaration header (through the
    /// param list and any return annotation, excluding the body) from
    /// whichever analyzed file declares it. Used by signature help when the
    /// function has a body (so no overload signatures were recorded).
    pub fn global_function_declaration_header(&self, name: &str) -> Option<String> {
        for (file, analysis) in self.analyses.iter() {
            for sym in &analysis.symbols.symbols {
                if sym.name == name && sym.flags & SYM_FUNCTION != 0 && sym.flags & SYM_IMPORT == 0
                {
                    let decl = sym.declarations.first()?;
                    let src = self
                        .sources
                        .get(file)
                        .or_else(|| self.sources.get(&normalize_path(&decl.file_name)))?;
                    let start = decl.span.start as usize;
                    let hay = src.get(start..)?;
                    // Header ends at the body `{` or `;` at paren depth 0.
                    let mut depth = 0i32;
                    let mut end = hay.len();
                    let bytes = hay.as_bytes();
                    for (i, &b) in bytes.iter().enumerate() {
                        match b {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            b'{' | b';' if depth == 0 => {
                                end = i;
                                break;
                            }
                            _ => {}
                        }
                    }
                    return Some(hay[..end].trim().to_string());
                }
            }
        }
        None
    }

    /// All known source file names (project files, in-memory node_modules
    /// entries, ...) — module-specifier completions enumerate these.
    pub fn source_file_names(&self) -> Vec<std::string::String> {
        self.sources.keys().cloned().collect()
    }

    /// Names of string-named ambient modules (`declare module "@e/f"`),
    /// excluding relative and wildcard declarations.
    pub fn ambient_string_module_names(&self) -> Vec<std::string::String> {
        let mut names = Vec::new();
        for analysis in self.analyses.values() {
            for sym in &analysis.symbols.symbols {
                if sym.flags & tsc_rs_symbols::SYM_STRING_MODULE != 0
                    && !sym.name.starts_with('.')
                    && !sym.name.starts_with('/')
                    && !sym.name.contains('*')
                    && !names.contains(&sym.name)
                {
                    names.push(sym.name.clone());
                }
            }
        }
        names
    }

    pub fn all_global_symbols(&self) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for analysis in self.analyses.values() {
            for sym in &analysis.symbols.symbols {
                if sym.name.is_empty() || seen.contains(&sym.name) {
                    continue;
                }
                // Only include top-level symbols (exported or declared at file scope)
                let is_top_level = sym.flags & SYM_EXPORT != 0
                    || sym.flags
                        & (SYM_INTERFACE
                            | SYM_CLASS
                            | SYM_FUNCTION
                            | SYM_VARIABLE
                            | SYM_ENUM
                            | SYM_TYPE_ALIAS)
                        != 0;
                if !is_top_level || sym.declarations.is_empty() {
                    continue;
                }
                seen.insert(sym.name.clone());
                symbols.push(SymbolInfo {
                    id: u64::from(sym.id),
                    name: sym.name.clone(),
                    kind: symbol_kind_from_flags(sym.flags),
                    type_id: None,
                    declarations: sym
                        .declarations
                        .iter()
                        .map(|d| (normalize_path(&d.file_name), d.span))
                        .collect(),
                    is_exported: sym.flags & SYM_EXPORT != 0,
                });
            }
        }
        symbols
    }

    /// True when `file` is a module (ESM syntax, require() invocations, or
    /// CommonJS exports assignments) rather than a global script.
    pub fn file_is_module(&self, file: &str) -> bool {
        self.sources
            .get(file)
            .map(|src| source_text_is_module(src))
            .unwrap_or(false)
    }

    /// Get files that directly depend on the given file.
    pub fn get_dependent_files(&self, file: &str) -> Vec<String> {
        let file = normalize_path(file);
        let mut dependents = Vec::new();
        for (candidate, deps) in &self.dependencies {
            if deps.iter().any(|dep| dep == &file) {
                dependents.push(candidate.clone());
            }
        }
        dependents.sort();
        dependents.dedup();
        dependents
    }

    /// Get raw diagnostics for programmatic consumers.
    /// List every file currently in the analyses map. Useful for
    /// debugging autodiscovery completeness.
    pub fn list_analyzed_files(&self) -> Vec<String> {
        self.analyses.keys().cloned().collect()
    }

    pub fn raw_diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Get the linked symbol table for a file (after check_all).
    ///
    /// This returns the symbol table that has been through `link_imports`,
    /// so imported symbols carry declarations from their source files.
    pub fn get_file_symbols(&self, file: &str) -> Option<&SymbolTable> {
        let file = normalize_path(file);
        self.analyses.get(&file).map(|a| &a.symbols)
    }

    /// Build a TypeCheckOutput from the QE's stored expression_types for a file.
    ///
    /// This allows callers to get a TypeCheckOutput compatible with
    /// hover_at / definition_at without re-running the checker.
    pub fn get_file_check_output(&self, file: &str) -> Option<tsc_rs_types::TypeCheckOutput> {
        let file = normalize_path(file);
        let analysis = self.analyses.get(&file)?;
        Some(tsc_rs_types::TypeCheckOutput {
            diagnostics: Vec::new(),
            expression_types: analysis.expression_types.clone(),
            expression_type_spans: HashMap::new(),
            selected_overload_indices: analysis.selected_overload_indices.clone(),
            stable_types: analysis
                .type_ids_by_display
                .values()
                .filter_map(|id| self.type_index.get(id).map(|ty| (*id, ty.clone())))
                .collect(),
        })
    }

    /// Resolve an import specifier from one file to the target file path.
    ///
    /// Used by cross-file definition resolution to find where an imported
    /// symbol is actually defined.
    pub fn resolve_import_path(&self, module_specifier: &str, from_file: &str) -> Option<String> {
        let from_file = normalize_path(from_file);
        let mut sorted_keys: Vec<String> = self.sources.keys().cloned().collect();
        sorted_keys.sort();
        let file_index: HashMap<String, usize> = sorted_keys
            .iter()
            .enumerate()
            .map(|(i, path)| (path.clone(), i))
            .collect();
        resolve_import_target(module_specifier, &from_file, &self.options, &file_index)
            .map(|idx| sorted_keys[idx].clone())
    }
}

impl Default for QueryEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Textual module detection: ESM statements, `require(...)` invocations, or
/// CommonJS exports assignments make a file a module (its top-level names
/// are not global).
fn source_text_is_module(src: &str) -> bool {
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("import ")
            || t.starts_with("import{")
            || t.starts_with("import(")
            || t.starts_with("export ")
            || t.starts_with("export{")
            || t.starts_with("module.exports")
            || t.starts_with("exports.")
        {
            return true;
        }
    }
    for (k, _) in src.match_indices("require") {
        if k > 0 {
            let prev = src.as_bytes()[k - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev == b'.' {
                continue;
            }
        }
        let after = src[k + "require".len()..].trim_start();
        if after.starts_with('(') {
            return true;
        }
    }
    // exports assignments not at line starts (inside blocks/functions)
    for pat in ["exports.", "module.exports"] {
        for (k, _) in src.match_indices(pat) {
            if k > 0 {
                let prev = src.as_bytes()[k - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev == b'.' {
                    continue;
                }
            }
            let after = src[k + pat.len()..].trim_start();
            // an assignment or member assignment marks CJS output
            if after.starts_with('=') && !after.starts_with("==") {
                return true;
            }
            if pat == "exports." {
                let name_len = after
                    .bytes()
                    .take_while(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
                    .count();
                let rest = after[name_len..].trim_start();
                if name_len > 0 && rest.starts_with('=') && !rest.starts_with("==") {
                    return true;
                }
            }
        }
    }
    false
}

fn collect_imports(file: &SourceFile) -> Vec<ImportDecl> {
    let mut imports = Vec::new();
    for stmt in &file.statements {
        if let StmtKind::Import(import_decl) = &stmt.kind {
            imports.push((**import_decl).clone());
        }
    }
    imports
}

fn resolve_import_target(
    module_name: &str,
    containing_file: &str,
    options: &CompilerOptions,
    file_index: &HashMap<String, usize>,
) -> Option<usize> {
    if let Some(resolved) =
        tsc_rs_resolver::resolve_module_name(module_name, containing_file, options)
    {
        let normalized = normalize_path(&resolved.resolved_file_name);
        if let Some(idx) = file_index.get(&normalized) {
            return Some(*idx);
        }
    }

    let in_memory = resolve_in_memory_relative(module_name, containing_file, file_index)?;
    file_index.get(&in_memory).copied()
}

fn resolve_in_memory_relative(
    module_name: &str,
    containing_file: &str,
    file_index: &HashMap<String, usize>,
) -> Option<String> {
    if !is_relative_import(module_name) {
        return None;
    }

    let containing_dir = Path::new(containing_file)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let base = containing_dir.join(module_name);
    let base_norm = normalize_path(&base.to_string_lossy());

    if file_index.contains_key(&base_norm) {
        return Some(base_norm);
    }

    for ext in EXTENSIONS {
        let candidate = format!("{base_norm}{ext}");
        if file_index.contains_key(&candidate) {
            return Some(candidate);
        }
    }

    for index in INDEX_FILES {
        let candidate = format!("{base_norm}/{index}");
        if file_index.contains_key(&candidate) {
            return Some(candidate);
        }
    }

    None
}

fn is_relative_import(module_name: &str) -> bool {
    module_name.starts_with("./") || module_name.starts_with("../")
}

fn lookup_expression_type(expression_types: &HashMap<u32, String>, offset: u32) -> Option<&str> {
    if let Some(exact) = expression_types.get(&offset) {
        return Some(exact.as_str());
    }

    // Fallback for cursor positions inside an expression token.
    let mut best: Option<(u32, &str)> = None;
    for (&pos, ty) in expression_types {
        if pos <= offset && offset.saturating_sub(pos) <= 32 {
            match best {
                None => best = Some((pos, ty.as_str())),
                Some((best_pos, _)) if pos > best_pos => best = Some((pos, ty.as_str())),
                _ => {}
            }
        }
    }
    best.map(|(_, ty)| ty)
}

fn type_for_span(analysis: &FileAnalysis, span: Span) -> Option<&str> {
    if let Some(exact) = analysis.expression_types.get(&span.start) {
        return Some(exact.as_str());
    }

    let mut best: Option<(u32, &str)> = None;
    for (&pos, ty) in &analysis.expression_types {
        if span.start <= pos && pos <= span.end {
            match best {
                None => best = Some((pos, ty.as_str())),
                Some((best_pos, _)) if pos < best_pos => best = Some((pos, ty.as_str())),
                _ => {}
            }
        }
    }
    best.map(|(_, ty)| ty)
}

fn build_display_id_map(stable_types: &HashMap<u64, String>) -> HashMap<String, u64> {
    let mut by_display = HashMap::new();
    for (&type_id, display) in stable_types {
        by_display.entry(display.clone()).or_insert(type_id);
    }
    by_display
}

fn resolve_stable_type_id(analysis: &FileAnalysis, ty: &str) -> u64 {
    analysis
        .type_ids_by_display
        .get(ty)
        .copied()
        .unwrap_or_else(|| stable_type_id(ty))
}

fn find_symbol_at_offset(symbols: &SymbolTable, offset: u32) -> Option<u64> {
    // Fast path: exact match at this offset (works for both declaration and usage sites)
    if let Some(&sym_id) = symbols.position_to_symbol.get(&offset) {
        return Some(u64::from(sym_id));
    }

    let mut best: Option<(u32, u32)> = None;
    for (&pos, &sym_id) in &symbols.position_to_symbol {
        if pos > offset {
            continue;
        }
        if let Some(sym) = symbols.get_symbol(sym_id) {
            // Check if offset falls within a declaration span
            let overlaps = sym
                .declarations
                .iter()
                .any(|decl| decl.span.start <= offset && offset <= decl.span.end);
            // Also check if offset falls within the identifier's name width
            // (handles usage-site references)
            let in_name = offset < pos + sym.name.len() as u32;
            if overlaps || in_name {
                match best {
                    None => best = Some((pos, sym_id)),
                    Some((best_pos, _)) if pos > best_pos => best = Some((pos, sym_id)),
                    _ => {}
                }
            }
        }
    }

    best.map(|(_, sym_id)| u64::from(sym_id))
}

fn as_u32(value: u64) -> Option<u32> {
    u32::try_from(value).ok()
}

fn symbol_kind_from_flags(flags: u32) -> SymbolKind {
    if flags & SYM_FUNCTION != 0 {
        SymbolKind::Function
    } else if flags & SYM_VARIABLE != 0 {
        SymbolKind::Variable
    } else if flags & SYM_CLASS != 0 {
        SymbolKind::Class
    } else if flags & SYM_INTERFACE != 0 {
        SymbolKind::Interface
    } else if flags & SYM_TYPE_ALIAS != 0 {
        SymbolKind::TypeAlias
    } else if flags & SYM_ENUM != 0 {
        SymbolKind::Enum
    } else if flags & SYM_ENUM_MEMBER != 0 {
        SymbolKind::EnumMember
    } else if flags & SYM_MODULE != 0 {
        SymbolKind::Module
    } else if flags & SYM_PROPERTY != 0 {
        SymbolKind::Property
    } else if flags & SYM_METHOD != 0 {
        SymbolKind::Method
    } else if flags & SYM_CONSTRUCTOR != 0 {
        SymbolKind::Constructor
    } else {
        SymbolKind::Unknown
    }
}

fn stable_type_id(type_repr: &str) -> u64 {
    // FNV-1a 64-bit.
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for byte in type_repr.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn stable_type_id_for_type(ty: &tsc_rs_types::Type) -> u64 {
    let mut hasher = DefaultHasher::new();
    ty.hash(&mut hasher);
    hasher.finish()
}

fn with_file_name(mut diagnostic: Diagnostic, file_name: &str) -> Diagnostic {
    if diagnostic.file_name.is_none() {
        diagnostic.file_name = Some(file_name.to_string());
    }
    diagnostic
}

fn format_diagnostic(diag: &Diagnostic) -> String {
    let category = match diag.category {
        DiagnosticCategory::Error => "error",
        DiagnosticCategory::Warning => "warning",
        DiagnosticCategory::Suggestion => "suggestion",
        DiagnosticCategory::Message => "message",
    };

    match (&diag.file_name, diag.span) {
        (Some(file), Some(span)) => {
            format!(
                "{file}:{}-{} {category} TS{}: {}",
                span.start, span.end, diag.code, diag.message
            )
        }
        (Some(file), None) => format!("{file} {category} TS{}: {}", diag.code, diag.message),
        (None, _) => format!("{category} TS{}: {}", diag.code, diag.message),
    }
}

fn normalize_path(path: &str) -> String {
    let path = Path::new(path);
    let mut prefix: Option<String> = None;
    let mut is_absolute = false;
    let mut parts: Vec<String> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(p) => {
                prefix = Some(p.as_os_str().to_string_lossy().replace('\\', "/"));
            }
            Component::RootDir => {
                is_absolute = true;
                parts.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(last) = parts.last() {
                    if last != ".." {
                        parts.pop();
                    } else if !is_absolute {
                        parts.push("..".to_string());
                    }
                } else if !is_absolute {
                    parts.push("..".to_string());
                }
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
        }
    }

    let mut out = String::new();
    if let Some(prefix) = prefix {
        out.push_str(&prefix);
        if is_absolute {
            out.push('/');
        }
    } else if is_absolute {
        out.push('/');
    }
    out.push_str(&parts.join("/"));

    if out.is_empty() {
        ".".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_rs_ast::DiagnosticCategory;

    #[test]
    fn query_engine_resolves_symbol_and_type_queries() {
        let mut engine = QueryEngine::new();
        let source = "let value = 1;\nvalue + 2;\n";

        engine
            .add_source("src/main.ts".to_string(), source.to_string())
            .expect("source should be accepted");
        assert!(
            engine.check_all().is_ok(),
            "unexpected diagnostics: {:?}",
            engine.diagnostics()
        );

        let value_offset = source.find("value").expect("value identifier should exist") as u32 + 1;
        let one_offset = source.find('1').expect("literal should exist") as u32;

        let symbol_id = engine.get_symbol_at("src/main.ts", value_offset);
        assert!(symbol_id.is_some(), "expected symbol at identifier offset");

        let type_id = engine.get_type_at("src/main.ts", one_offset);
        assert!(type_id.is_some(), "expected type at literal offset");
        assert_eq!(
            type_id,
            engine.get_type_at("src/main.ts", one_offset),
            "type ids should be deterministic"
        );
    }

    #[test]
    fn query_engine_surfaces_type_diagnostics() {
        let mut engine = QueryEngine::new();
        engine
            .add_source(
                "src/error.ts".to_string(),
                "let value: number = \"hello\";\n".to_string(),
            )
            .expect("source should be accepted");

        let result = engine.check_all();
        assert!(result.is_err(), "expected a type error");
        assert!(
            engine
                .raw_diagnostics()
                .iter()
                .any(|d| d.category == DiagnosticCategory::Error),
            "expected at least one error diagnostic"
        );
        assert!(
            !engine.diagnostics().is_empty(),
            "formatted diagnostics should be present"
        );
    }

    #[test]
    fn query_engine_rechecks_after_source_replacement() {
        let mut engine = QueryEngine::new();
        let path = "src/update.ts".to_string();

        engine
            .add_source(path.clone(), "let x = 1;\n".to_string())
            .expect("source should be accepted");
        let _ = engine.check_all();
        assert!(engine.raw_diagnostics().is_empty());

        engine
            .add_source(path, "let x: number = \"oops\";\n".to_string())
            .expect("source should be accepted");
        assert!(engine.check_all().is_err());
        assert!(engine
            .raw_diagnostics()
            .iter()
            .any(|d| d.category == DiagnosticCategory::Error));
    }

    #[test]
    fn query_engine_symbol_and_diagnostic_filters_work() {
        let mut engine = QueryEngine::new();
        let source = "let foo = 1;\nfoo + 1;\nlet bar: number = \"bad\";\n";
        let file = "src/filter.ts";

        engine
            .add_source(file.to_string(), source.to_string())
            .expect("source should be accepted");
        assert!(engine.check_all().is_err());

        let foo_symbols = engine.find_symbol("foo");
        assert!(!foo_symbols.is_empty(), "expected foo symbol");

        let foo_locations = engine.get_symbol_locations(foo_symbols[0]);
        assert!(
            !foo_locations.is_empty(),
            "expected at least one foo declaration location"
        );

        let foo_uses = engine.get_symbol_uses(foo_symbols[0]);
        assert!(!foo_uses.is_empty(), "expected mapped uses for foo");

        let file_diags = engine.file_diagnostics(file);
        assert!(!file_diags.is_empty(), "expected file diagnostics");

        let bad_offset = source.find("bad").expect("bad string should exist") as u32;
        let at_diags = engine.diagnostics_at(file, bad_offset);
        assert!(
            !at_diags.is_empty(),
            "expected location-specific diagnostics"
        );

        assert!(engine.error_count() >= 1, "expected at least one error");
    }

    #[test]
    fn query_engine_resolves_type_refs() {
        let mut engine = QueryEngine::new();
        let source = "let n: number = 1;\n";
        engine
            .add_source("src/type_ref.ts".to_string(), source.to_string())
            .expect("source should be accepted");
        let _ = engine.check_all();

        let num_ty = engine.resolve_type_ref("number", "src/type_ref.ts");
        assert!(num_ty.is_some(), "primitive type ref should resolve");
    }

    #[test]
    fn query_engine_exposes_symbol_and_type_metadata() {
        let mut engine = QueryEngine::new();
        let source = "const foo = 123;\nfoo;\n";
        let file = "src/meta.ts";
        engine
            .add_source(file.to_string(), source.to_string())
            .expect("source should be accepted");
        assert!(engine.check_all().is_ok());

        let foo_symbols = engine.find_symbol("foo");
        assert!(!foo_symbols.is_empty(), "expected foo symbol");

        let info = engine
            .get_symbol_info(foo_symbols[0])
            .expect("symbol info should exist");
        assert_eq!(info.name, "foo");
        assert!(
            !info.declarations.is_empty(),
            "declarations should be present"
        );

        let literal_offset = source.find("123").expect("literal should exist") as u32;
        let type_id = engine
            .get_type_at(file, literal_offset)
            .expect("type id should resolve");
        let type_str = engine
            .type_to_string(type_id)
            .expect("type string should resolve");
        assert_eq!(type_str, "123");

        let source_text = engine
            .get_source_file(file)
            .expect("source text should be retrievable");
        assert!(source_text.contains("const foo"));
    }

    #[test]
    fn query_engine_tracks_dependencies() {
        let mut engine = QueryEngine::new();
        engine
            .add_source(
                "src/a.ts".to_string(),
                "import { b } from \"./b\";\nexport const a = b;\n".to_string(),
            )
            .expect("source should be accepted");
        engine
            .add_source("src/b.ts".to_string(), "export const b = 1;\n".to_string())
            .expect("source should be accepted");

        let _ = engine.check_all();

        let deps = engine.get_dependencies("src/a.ts");
        assert_eq!(deps, vec!["src/b.ts".to_string()]);

        let dependents = engine.get_dependent_files("src/b.ts");
        assert_eq!(dependents, vec!["src/a.ts".to_string()]);
    }
}
