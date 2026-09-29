#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use tsc_rs_ast::{
    CompilerOptions, Diagnostic, DiagnosticCategory, JsxEmit, ModuleKind, ScriptTarget, SourceFile,
};
use tsc_rs_harness::{BootstrapHarness, CommandTemplate, Suite, SuiteRunConfig};
use tsc_rs_project::{CompilationResult, TsProject};

mod check_pipe;
mod pipe;

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Memory guardrail
// ---------------------------------------------------------------------------

/// Cap the process's virtual address space at `TSC_RS_MAX_VMSIZE_MB` (default
/// 12 GB). Set to `0` to disable. Bext-server occasionally hands us
/// pathological inputs — most recently an 88 MB single-file concat bundle
/// that drove a single one-shot tsc-rs invocation to 40 GB RSS on the dev box
/// (2026-05-11) and triggered an hour-long deploy-pipeline meltdown. The
/// pipe-mode lifetime cap in pipe.rs handles long-running workers; this
/// covers everything else (one-shot CLI, harness, server). Allocation
/// failures past the limit cause Rust to abort with a clear OOM message
/// rather than swap-thrashing the host into a stuck state.
///
/// Unix-only — on other targets the function is a no-op.
#[cfg(unix)]
fn install_vmsize_rlimit() {
    let mb: u64 = std::env::var("TSC_RS_MAX_VMSIZE_MB")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .unwrap_or(12_288);
    if mb == 0 {
        return;
    }
    let bytes: libc::rlim_t = (mb.saturating_mul(1024).saturating_mul(1024)) as libc::rlim_t;
    let rlim = libc::rlimit {
        rlim_cur: bytes,
        rlim_max: bytes,
    };
    // SAFETY: setrlimit on a stack-owned rlimit pair is a documented libc
    // entry point — the kernel copies in, no aliasing concerns.
    let rc = unsafe { libc::setrlimit(libc::RLIMIT_AS, &rlim) };
    if rc != 0 {
        eprintln!(
            "tsc-rs: install_vmsize_rlimit({} MB) failed: {}",
            mb,
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(not(unix))]
fn install_vmsize_rlimit() {}

// ---------------------------------------------------------------------------
// Parallelism configuration
// ---------------------------------------------------------------------------

/// Configure the global rayon thread pool from `--jobs N` / `TSC_RS_JOBS=N`.
///
/// `--jobs` lives outside `CliArgs::parse` because we read it from raw argv
/// before constructing the project (the parse phase already uses rayon).
/// Precedence: CLI flag > env var > rayon default (one worker per core).
///
/// Setting `1` forces sequential execution — useful for reproducing
/// type-check bugs, gauging the parallel speedup, or pinning behaviour on
/// single-core CI runners. `0` and unset both mean "rayon default".
fn configure_rayon() {
    let mut jobs: Option<usize> = std::env::var("TSC_RS_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n: &usize| n > 0);

    // Scan argv (without consuming it) for --jobs N / --jobs=N. Be tolerant
    // of placement so the flag can sit anywhere after the binary name.
    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--jobs" || arg == "-j" {
            if let Some(n) = argv.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
                if n > 0 {
                    jobs = Some(n);
                }
            }
            i += 2;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--jobs=") {
            if let Ok(n) = rest.parse::<usize>() {
                if n > 0 {
                    jobs = Some(n);
                }
            }
        }
        i += 1;
    }

    // Rayon's default worker stack is 2 MB — too small for the type
    // checker on real codebases (Prisma's nested generic types recurse
    // through `resolve_type_reference_to_object` / `simplify_type` /
    // `is_assignable_to` and overflow the worker stack during parallel
    // parse + bind). Bump to 32 MB per worker by default. The main
    // thread's larger stack (set in `--check-pipe` mode + the rest via
    // `TSC_RS_STACK_MB`) doesn't help Rayon-dispatched work.
    let worker_stack_mb = std::env::var("TSC_RS_WORKER_STACK_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(32);
    let mut builder = rayon::ThreadPoolBuilder::new().stack_size(worker_stack_mb * 1024 * 1024);
    if let Some(n) = jobs {
        builder = builder.num_threads(n);
    }
    // `build_global` only succeeds once per process; ignore the
    // already-initialised error so re-entrant tests don't panic.
    let _ = builder.build_global();
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    // FIRST thing — before any allocation-heavy work happens. setrlimit is
    // inherited by forked threads, so this also bounds the rayon worker
    // threads spawned by pipe-mode batch handling.
    install_vmsize_rlimit();

    // Configure the global rayon pool BEFORE any par_iter runs. Honors
    // `--jobs N` / `TSC_RS_JOBS=N`; defaults to the rayon-detected core
    // count when unset or set to 0. Set to 1 to force fully sequential
    // execution (helpful for reproducing tsc-rs bugs in isolation or for
    // CI runners that allocate a single physical core).
    configure_rayon();

    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        print_help();
        return;
    }

    // Single-flag commands
    match args[0].as_str() {
        "--lsp" => {
            run_lsp();
            return;
        }
        "--version" | "-v" => {
            println!("tsc-rs {VERSION}");
            return;
        }
        "--help" | "-h" => {
            print_help();
            return;
        }
        "--init" => {
            run_init();
            return;
        }
        "--build" | "-b" => {
            run_build(&args[1..]);
            return;
        }
        "analyze" => {
            // Parser's recursive descent needs a larger stack for big files.
            let args_owned = args[1..].to_vec();
            let handle = std::thread::Builder::new()
                .name("analyze".into())
                .stack_size(64 * 1024 * 1024) // 64 MB
                .spawn(move || run_analyze(&args_owned))
                .expect("failed to spawn analyze thread");
            handle.join().expect("analyze thread panicked");
            return;
        }
        "discover" | "run-suite" => {
            run_harness(&args);
            return;
        }
        "--pipe" => {
            pipe::run_pipe();
            return;
        }
        "--check-pipe" => {
            // Accepted invocations:
            //   tsc-rs --check-pipe -p tsconfig.json
            //   tsc-rs --check-pipe tsconfig.json
            //   tsc-rs --check-pipe                  (defaults to ./tsconfig.json)
            let rest = &args[1..];
            let mut config_path = "tsconfig.json".to_string();
            let mut i = 0;
            while i < rest.len() {
                let a = &rest[i];
                if a == "-p" || a == "--project" {
                    if let Some(next) = rest.get(i + 1) {
                        config_path = next.clone();
                        i += 2;
                        continue;
                    }
                    eprintln!("--check-pipe: -p / --project needs a path");
                    std::process::exit(1);
                }
                if !a.starts_with('-') {
                    config_path = a.clone();
                }
                i += 1;
            }
            // Run on a dedicated thread with a large stack. The default
            // main-thread stack (8 MB on Linux) is enough for normal
            // type-checking but bottoms out on initialization for projects
            // that pull in deeply-nested generated typings — Prisma's
            // multi-megabyte `internal/class.d.ts` + `commonInputTypes.d.ts`
            // chain has produced "fatal runtime error: stack overflow"
            // crashes in real daemon respawns. A 128 MB stack accommodates
            // them with headroom. Tunable via `TSC_RS_STACK_MB`.
            let stack_mb = std::env::var("TSC_RS_STACK_MB")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(128);
            let cfg = config_path.clone();
            let handle = std::thread::Builder::new()
                .name("check-pipe".into())
                .stack_size(stack_mb * 1024 * 1024)
                .spawn(move || check_pipe::run_check_pipe(&cfg))
                .expect("failed to spawn check-pipe thread");
            handle.join().expect("check-pipe thread panicked");
            return;
        }
        _ => {}
    }

    // Parse CLI flags and file names
    let cli = match CliArgs::parse(&args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("error: {msg}");
            std::process::exit(1);
        }
    };

    // Determine the config path (if any) and initial project
    let config_path: Option<String> = if cli.project.is_some() {
        cli.project.clone()
    } else if cli.files.is_empty() && Path::new("tsconfig.json").is_file() {
        Some("tsconfig.json".to_string())
    } else {
        None
    };

    let project = if let Some(ref cp) = config_path {
        match TsProject::from_config(cp) {
            Ok(mut p) => {
                apply_overrides(&mut p.options, &cli);
                p
            }
            Err(e) => {
                eprintln!("error TS6053: {e}");
                std::process::exit(1);
            }
        }
    } else if !cli.files.is_empty() {
        let mut options = CompilerOptions::default();
        apply_overrides(&mut options, &cli);
        TsProject::new(cli.files.clone(), options)
    } else {
        eprintln!("error: No input files or tsconfig.json found.");
        print_help();
        std::process::exit(1);
    };

    if maybe_print_file_list(&project, &cli) {
        return;
    }

    if cli.watch {
        run_watch(project, &cli, config_path.as_deref());
    } else {
        // Compile on a dedicated thread with a larger stack. Same rationale
        // as the `--check-pipe` branch above: nested generic typings
        // (especially generated Prisma `.d.ts` files) overflow the default
        // 8 MB main-thread stack during initial parse+bind+inject. Tunable
        // via `TSC_RS_STACK_MB` (default 128 MB).
        let stack_mb = std::env::var("TSC_RS_STACK_MB")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(128);
        let project = project;
        let cli = cli;
        let cfg = config_path;
        let handle = std::thread::Builder::new()
            .name("compile".into())
            .stack_size(stack_mb * 1024 * 1024)
            .spawn(move || compile_and_emit(&project, &cli, cfg.as_deref()))
            .expect("failed to spawn compile thread");
        let has_errors = handle.join().expect("compile thread panicked");
        let exit_code = if has_errors { 1 } else { 0 };
        std::process::exit(exit_code);
    }
}

// ---------------------------------------------------------------------------
// CLI argument parser
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct CliArgs {
    files: Vec<String>,
    project: Option<String>,
    no_emit: bool,
    list_files: bool,
    list_files_only: bool,
    watch: bool,
    preserve_watch_output: bool,
    out_dir: Option<String>,
    target: Option<String>,
    module: Option<String>,
    jsx: Option<String>,
    jsx_factory: Option<String>,
    jsx_import_source: Option<String>,
    declaration: bool,
    source_map: bool,
    strict: bool,
    incremental: bool,
    composite: bool,
    transpile_only: bool,
    preserve_type_annotations: bool,
    preserve_comments: bool,
    preserve_whitespace: bool,
    fast_emit: bool,
    /// Cap how many diagnostics are printed to stderr. `None` prints all.
    /// The summary still reflects the full count. Useful when a fresh
    /// type-check run on a large project would dump tens of thousands of
    /// lines (apps/app reports 14 983 errors today).
    max_errors: Option<usize>,
}

impl CliArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut cli = CliArgs::default();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            match arg.as_str() {
                "--project" | "-p" => {
                    i += 1;
                    cli.project = Some(
                        args.get(i)
                            .ok_or("--project requires a path argument")?
                            .clone(),
                    );
                }
                "--noEmit" => cli.no_emit = true,
                "--listFiles" => cli.list_files = true,
                "--listFilesOnly" => cli.list_files_only = true,
                "--watch" | "-w" => cli.watch = true,
                "--preserveWatchOutput" => cli.preserve_watch_output = true,
                "--outDir" => {
                    i += 1;
                    cli.out_dir = Some(
                        args.get(i)
                            .ok_or("--outDir requires a path argument")?
                            .clone(),
                    );
                }
                "--target" => {
                    i += 1;
                    cli.target = Some(args.get(i).ok_or("--target requires a value")?.clone());
                }
                "--module" => {
                    i += 1;
                    cli.module = Some(args.get(i).ok_or("--module requires a value")?.clone());
                }
                "--jsx" => {
                    i += 1;
                    cli.jsx = Some(
                        args.get(i)
                            .ok_or("--jsx requires a value (react, react-jsx, preserve)")?
                            .clone(),
                    );
                }
                "--jsxFactory" => {
                    i += 1;
                    cli.jsx_factory =
                        Some(args.get(i).ok_or("--jsxFactory requires a value")?.clone());
                }
                "--jsxImportSource" => {
                    i += 1;
                    cli.jsx_import_source = Some(
                        args.get(i)
                            .ok_or("--jsxImportSource requires a value")?
                            .clone(),
                    );
                }
                "--transpileOnly" => cli.transpile_only = true,
                "--fast-emit" | "--fastEmit" => cli.fast_emit = true,
                "--declaration" | "-d" => cli.declaration = true,
                "--sourceMap" => cli.source_map = true,
                "--strict" => cli.strict = true,
                "--incremental" => cli.incremental = true,
                "--composite" => cli.composite = true,
                "--preserveTypeAnnotations" => cli.preserve_type_annotations = true,
                "--preserveComments" => cli.preserve_comments = true,
                "--preserveWhitespace" => cli.preserve_whitespace = true,
                "--maxErrors" => {
                    i += 1;
                    let raw = args.get(i).ok_or("--maxErrors requires a number")?;
                    cli.max_errors = Some(
                        raw.parse::<usize>()
                            .map_err(|_| format!("--maxErrors: not a number: {raw}"))?,
                    );
                }
                // `--jobs N` / `-j N` are consumed pre-parse by
                // `configure_rayon()` (before the rayon pool is built); we
                // still need to accept them here so the value argument is
                // skipped and CliArgs::parse doesn't surface "Unknown flag".
                "--jobs" | "-j" => {
                    i += 1;
                    if args.get(i).is_none() {
                        return Err("--jobs requires a number".to_string());
                    }
                }
                other if other.starts_with("--jobs=") => {
                    // Already consumed by configure_rayon; nothing to do.
                    let _ = other;
                }
                other if other.starts_with('-') => {
                    return Err(format!("Unknown flag: {other}"));
                }
                _ => {
                    cli.files.push(arg.clone());
                }
            }
            i += 1;
        }
        Ok(cli)
    }
}

/// Apply CLI overrides onto a CompilerOptions struct.
fn apply_overrides(options: &mut CompilerOptions, cli: &CliArgs) {
    if cli.no_emit {
        options.no_emit = Some(true);
    }
    if let Some(ref dir) = cli.out_dir {
        options.out_dir = Some(dir.clone());
    }
    if let Some(ref t) = cli.target {
        options.target = ScriptTarget::parse(t);
    }
    if let Some(ref m) = cli.module {
        options.module = ModuleKind::parse(m);
    }
    if let Some(ref j) = cli.jsx {
        options.jsx = JsxEmit::parse(j);
    }
    if let Some(ref f) = cli.jsx_factory {
        options.jsx_factory = Some(f.clone());
    }
    if let Some(ref s) = cli.jsx_import_source {
        options.jsx_import_source = Some(s.clone());
    }
    if cli.declaration {
        options.declaration = Some(true);
    }
    if cli.source_map {
        options.source_map = Some(true);
    }
    if cli.strict {
        options.strict = Some(true);
    }
    if cli.incremental {
        options.incremental = Some(true);
    }
    if cli.composite {
        options.composite = Some(true);
        // composite implies incremental and declaration
        options.incremental = Some(true);
        options.declaration = Some(true);
    }
    if cli.preserve_type_annotations {
        options.preserve_type_annotations = Some(true);
    }
    if cli.preserve_comments {
        options.preserve_comments = Some(true);
    }
    if cli.preserve_whitespace {
        options.preserve_whitespace = Some(true);
    }
    if cli.fast_emit {
        options.fast_emit = Some(true);
    }
}

fn maybe_print_file_list(project: &TsProject, cli: &CliArgs) -> bool {
    if cli.list_files || cli.list_files_only {
        for file_name in &project.file_names {
            println!("{file_name}");
        }
    }
    cli.list_files_only
}

// ---------------------------------------------------------------------------
// Compile and emit
// ---------------------------------------------------------------------------

fn compile_and_emit(project: &TsProject, cli: &CliArgs, config_path: Option<&str>) -> bool {
    // --transpileOnly: skip type checking entirely, just parse + emit
    if cli.transpile_only {
        let result = project.transpile_only();
        let no_emit = project.options.no_emit == Some(true) || cli.no_emit;
        // Transpile-only skips TYPE checking; it does not make syntax errors
        // disappear. The parser recovers so the emitter can still run, but the
        // emitted JS is then corrupt (truncated template literals, phantom
        // exports). Report those diagnostics and refuse to write the output,
        // exactly as the checked path does — a compiler must not answer
        // "success" while emitting garbage.
        let syntax_diags = collect_all_diagnostics(&result);
        print_diagnostics(&syntax_diags, project, cli.max_errors);
        let has_errors = result.has_errors();
        if !no_emit && !has_errors {
            write_outputs(&result, &project.options);
        }
        if has_errors {
            print_error_summary(&syntax_diags);
        }
        return has_errors;
    }

    if should_use_query_pipeline(project, cli) {
        let query_result = project.compile_query();
        print_diagnostics(&query_result.diagnostics, project, cli.max_errors);
        print_error_summary(&query_result.diagnostics);
        return query_result.has_errors();
    }

    let is_incremental = is_incremental_mode(project, cli);
    let no_emit = project.options.no_emit == Some(true) || cli.no_emit;

    let result = if is_incremental {
        let build_info_path =
            tsc_rs_project::resolve_build_info_path(&project.options, config_path);
        let (compilation_result, build_info) = project.compile_incremental(&build_info_path);

        // Skip writing .tsbuildinfo in --noEmit mode: the cache stores file
        // hashes + deps but NOT diagnostics, so the next type-check-only run
        // sees "0 files to recompile" and silently exits 0 even when the
        // previous run had errors. Emit-mode runs still benefit from the
        // cache (they additionally cache compiled JS output).
        if !no_emit {
            if let Err(e) = build_info.save(&build_info_path) {
                eprintln!("warning: Cannot write .tsbuildinfo: {e}");
            }
        }

        compilation_result
    } else {
        project.compile()
    };

    // Print diagnostics
    let all_diags = collect_all_diagnostics(&result);
    print_diagnostics(&all_diags, project, cli.max_errors);

    // Write output files unless --noEmit
    if !no_emit && !result.has_errors() {
        write_outputs(&result, &project.options);
    }

    print_error_summary(&all_diags);
    result.has_errors()
}

fn should_use_query_pipeline(project: &TsProject, cli: &CliArgs) -> bool {
    let no_emit = project.options.no_emit == Some(true) || cli.no_emit;
    no_emit && !is_incremental_mode(project, cli)
}

fn is_incremental_mode(project: &TsProject, cli: &CliArgs) -> bool {
    project.options.incremental == Some(true)
        || project.options.composite == Some(true)
        || cli.incremental
        || cli.composite
}

fn print_error_summary(all_diags: &[Diagnostic]) {
    let error_count = all_diags
        .iter()
        .filter(|d| d.category == DiagnosticCategory::Error)
        .count();
    if error_count == 0 {
        return;
    }

    let error_files: HashSet<&str> = all_diags
        .iter()
        .filter(|d| d.category == DiagnosticCategory::Error)
        .filter_map(|d| d.file_name.as_deref())
        .collect();
    let file_count = error_files.len();
    let es = if error_count == 1 { "" } else { "s" };
    if file_count > 0 {
        let fs = if file_count == 1 { "" } else { "s" };
        eprintln!("\nFound {error_count} error{es} in {file_count} file{fs}.");
    } else {
        eprintln!("\nFound {error_count} error{es}.");
    }
}

fn collect_all_diagnostics(result: &CompilationResult) -> Vec<Diagnostic> {
    let mut diags: Vec<Diagnostic> = result.diagnostics.clone();
    for file_output in &result.files {
        // The checker doesn't always tag emitted diagnostics with the file
        // they came from (e.g. unknown-module / cross-file resolution
        // failures synthesised during inject_external_types). Without a
        // file_name, print_diagnostics drops the `path(line,col):` prefix
        // and the error-summary file count collapses to 0 — which made
        // 14,987 errors look like they came from "2 files" on real
        // projects. Use the owning FileOutput as the fallback.
        for diag in &file_output.diagnostics {
            if diag.file_name.is_some() {
                diags.push(diag.clone());
            } else {
                let mut tagged = diag.clone();
                tagged.file_name = Some(file_output.file_name.clone());
                diags.push(tagged);
            }
        }
    }
    diags
}

/// Write .js (and optionally .d.ts / .map) files.
fn write_outputs(result: &CompilationResult, options: &CompilerOptions) {
    for file_output in &result.files {
        let source_path = Path::new(&file_output.file_name);
        let base = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");

        let out_dir = if let Some(ref dir) = options.out_dir {
            PathBuf::from(dir)
        } else {
            source_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        };

        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!(
                "error: Cannot create output directory '{}': {e}",
                out_dir.display()
            );
            continue;
        }

        // Write .js
        let js_path = out_dir.join(format!("{base}.js"));
        if let Err(e) = std::fs::write(&js_path, &file_output.emit.javascript) {
            eprintln!("error: Cannot write '{}': {e}", js_path.display());
        }

        // Write .js.map if present
        if let Some(ref map) = file_output.emit.source_map {
            let map_path = out_dir.join(format!("{base}.js.map"));
            if let Err(e) = std::fs::write(&map_path, map) {
                eprintln!("error: Cannot write '{}': {e}", map_path.display());
            }
        }

        // Write .d.ts if present
        if let Some(ref dts) = file_output.emit.declaration_file {
            let dts_path = out_dir.join(format!("{base}.d.ts"));
            if let Err(e) = std::fs::write(&dts_path, dts) {
                eprintln!("error: Cannot write '{}': {e}", dts_path.display());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// --build mode: build project references
// ---------------------------------------------------------------------------

fn run_build(args: &[String]) {
    let config_path = if args.is_empty() {
        "tsconfig.json".to_string()
    } else {
        args[0].clone()
    };

    if !Path::new(&config_path).is_file() {
        eprintln!("error: Cannot find project configuration: {config_path}");
        std::process::exit(1);
    }

    match tsc_rs_project::build_project_references(&config_path) {
        Ok(build_result) => {
            let mut has_errors = false;
            for (cfg, result) in &build_result.results {
                if result.has_errors() {
                    has_errors = true;
                    eprintln!("Project '{}' has errors:", cfg);
                    let diags = collect_all_diagnostics(result);
                    for diag in &diags {
                        eprintln!("  {}: {}", diag.code, diag.message);
                    }
                }
            }
            if has_errors {
                std::process::exit(1);
            }
            eprintln!(
                "Built {} project(s) in order: {}",
                build_result.build_order.len(),
                build_result.build_order.join(" -> ")
            );
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostic formatting
// ---------------------------------------------------------------------------

fn print_diagnostics(diagnostics: &[Diagnostic], project: &TsProject, max_errors: Option<usize>) {
    let use_color = is_tty();
    let mut errors_printed: usize = 0;
    let total_errors = diagnostics
        .iter()
        .filter(|d| d.category == DiagnosticCategory::Error)
        .count();

    // Build a lookup from file_name -> source text for line/col resolution
    let mut source_cache: HashMap<String, String> = HashMap::new();
    for fname in &project.file_names {
        if let Ok(src) = std::fs::read_to_string(fname) {
            source_cache.insert(fname.clone(), src);
        }
    }

    for diag in diagnostics {
        if let Some(cap) = max_errors {
            if diag.category == DiagnosticCategory::Error && errors_printed >= cap {
                continue;
            }
        }
        let category_str = match diag.category {
            DiagnosticCategory::Error => "error",
            DiagnosticCategory::Warning => "warning",
            DiagnosticCategory::Suggestion => "suggestion",
            DiagnosticCategory::Message => "message",
        };

        let location = if let (Some(ref fname), Some(span)) = (&diag.file_name, diag.span) {
            if let Some(source) = source_cache.get(fname.as_str()) {
                let (line, col) = offset_to_line_col(source, span.start);
                format!("{}({},{})", fname, line + 1, col + 1)
            } else {
                fname.clone()
            }
        } else if let Some(ref fname) = diag.file_name {
            fname.clone()
        } else {
            String::new()
        };

        if use_color {
            let color_code = match diag.category {
                DiagnosticCategory::Error => "\x1b[91m",      // bright red
                DiagnosticCategory::Warning => "\x1b[93m",    // bright yellow
                DiagnosticCategory::Suggestion => "\x1b[96m", // bright cyan
                DiagnosticCategory::Message => "\x1b[97m",    // bright white
            };
            let reset = "\x1b[0m";

            if location.is_empty() {
                eprintln!(
                    "{color_code}{category_str} TS{code}{reset}: {msg}",
                    code = diag.code,
                    msg = diag.message,
                );
            } else {
                eprintln!(
                    "{location}: {color_code}{category_str} TS{code}{reset}: {msg}",
                    code = diag.code,
                    msg = diag.message,
                );
            }
        } else if location.is_empty() {
            eprintln!(
                "{category_str} TS{code}: {msg}",
                code = diag.code,
                msg = diag.message,
            );
        } else {
            eprintln!(
                "{location}: {category_str} TS{code}: {msg}",
                code = diag.code,
                msg = diag.message,
            );
        }
        if diag.category == DiagnosticCategory::Error {
            errors_printed += 1;
        }
    }

    if let Some(cap) = max_errors {
        if total_errors > cap {
            eprintln!(
                "... {} more errors suppressed by --maxErrors {} (use --maxErrors 0 or omit for full output).",
                total_errors - cap,
                cap,
            );
        }
    }
}

/// Compute line and column (both 0-based) from a byte offset.
fn offset_to_line_col(source: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let bytes = source.as_bytes();
    let mut line = 0u32;
    let mut col = 0u32;
    for &b in &bytes[..offset.min(bytes.len())] {
        if b == b'\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Check whether color output should be used.
/// Respects NO_COLOR convention and falls back to checking TERM.
fn is_tty() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    // On CI or dumb terminals, disable color
    match std::env::var("TERM") {
        Ok(term) if term == "dumb" => false,
        Ok(_) => true,
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Content hashing (simple, non-crypto)
// ---------------------------------------------------------------------------

/// Compute a fast hash of a byte slice using the standard library DefaultHasher.
fn hash_content(content: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Read content hashes for all files. Returns a map from file name to hash.
fn collect_content_hashes(file_names: &[String]) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    for name in file_names {
        if let Ok(content) = std::fs::read(name) {
            map.insert(name.clone(), hash_content(&content));
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Watch mode (polling) with new file detection and incremental state
// ---------------------------------------------------------------------------

fn run_watch(mut project: TsProject, cli: &CliArgs, config_path: Option<&str>) {
    eprintln!("Starting compilation in watch mode...\n");

    // Initial compile (no cache for first run)
    let _ = compile_and_emit(&project, cli, config_path);

    // Track content hashes for incremental parse skipping
    let mut content_hashes: HashMap<String, u64> = collect_content_hashes(&project.file_names);

    // Cache of parsed SourceFiles, keyed by file name
    let mut parse_cache: HashMap<String, SourceFile> = HashMap::new();

    // Populate parse cache from initial compile
    for file_name in &project.file_names {
        if let Ok(source) = std::fs::read_to_string(file_name) {
            let sf = tsc_rs_parser::parse(file_name, &source);
            parse_cache.insert(file_name.clone(), sf);
        }
    }

    // Track tsconfig.json modification time (if config-based project)
    let mut config_mtime =
        config_path.and_then(|cp| std::fs::metadata(cp).ok().and_then(|m| m.modified().ok()));

    eprintln!("\x1b[96mWatching for file changes...\x1b[0m\n");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut needs_recompile = false;
        let mut config_changed = false;

        // 1. Check if tsconfig.json has been modified
        if let Some(cp) = config_path {
            if let Ok(meta) = std::fs::metadata(cp) {
                if let Ok(mtime) = meta.modified() {
                    if config_mtime.as_ref() != Some(&mtime) {
                        config_mtime = Some(mtime);
                        config_changed = true;
                        needs_recompile = true;
                    }
                }
            }
        }

        // 2. Re-scan include globs to detect new/deleted files
        if config_changed {
            if let Some(cp) = config_path {
                match TsProject::from_config(cp) {
                    Ok(mut new_project) => {
                        apply_overrides(&mut new_project.options, cli);
                        project = new_project;
                    }
                    Err(e) => {
                        eprintln!("error TS6053: {e}");
                        // Keep using old project on config error
                    }
                }
            }
        } else if let Some(cp) = config_path {
            // Even without config change, re-scan globs for new/deleted files
            match TsProject::from_config(cp) {
                Ok(mut new_project) => {
                    apply_overrides(&mut new_project.options, cli);

                    let old_set: HashSet<&String> = project.file_names.iter().collect();
                    let new_set: HashSet<&String> = new_project.file_names.iter().collect();

                    // Detect new files
                    for f in &new_set {
                        if !old_set.contains(f) {
                            eprintln!("  New file detected: {f}");
                            needs_recompile = true;
                        }
                    }

                    // Detect deleted files
                    for f in &old_set {
                        if !new_set.contains(f) {
                            eprintln!("  Deleted file detected: {f}");
                            needs_recompile = true;
                            // Remove from caches
                            parse_cache.remove(*f);
                            content_hashes.remove(*f);
                        }
                    }

                    if new_project.file_names != project.file_names {
                        project = new_project;
                    }
                }
                Err(_) => {
                    // Config became invalid; keep using old project
                }
            }
        }

        // 3. Check for deleted files (files in list that no longer exist on disk)
        {
            let mut deleted = Vec::new();
            for f in &project.file_names {
                if !Path::new(f).exists() {
                    deleted.push(f.clone());
                }
            }
            for f in &deleted {
                eprintln!("  File no longer exists: {f}");
                parse_cache.remove(f);
                content_hashes.remove(f);
                needs_recompile = true;
            }
            if !deleted.is_empty() {
                let deleted_set: HashSet<&String> = deleted.iter().collect();
                project.file_names.retain(|f| !deleted_set.contains(f));
            }
        }

        // 4. Check content hashes to detect modified files
        let new_hashes = collect_content_hashes(&project.file_names);
        for (name, new_hash) in &new_hashes {
            let old_hash = content_hashes.get(name);
            if old_hash != Some(new_hash) {
                needs_recompile = true;
                // Invalidate parse cache for changed files
                parse_cache.remove(name);
            }
        }

        if !needs_recompile {
            continue;
        }

        // Clear screen unless --preserveWatchOutput
        if !cli.preserve_watch_output {
            eprint!("\x1b[2J\x1b[H");
        }

        eprintln!("\x1b[96mFile change detected. Starting incremental compilation...\x1b[0m\n");

        // Compile with cache
        let _ = compile_and_emit(&project, cli, config_path);

        // Update content hashes
        content_hashes = new_hashes;

        // Re-populate parse cache for all files (including newly parsed ones)
        for file_name in &project.file_names {
            if !parse_cache.contains_key(file_name) {
                if let Ok(source) = std::fs::read_to_string(file_name) {
                    let sf = tsc_rs_parser::parse(file_name, &source);
                    parse_cache.insert(file_name.clone(), sf);
                }
            }
        }

        eprintln!("\x1b[96mWatching for file changes...\x1b[0m\n");
    }
}

// ---------------------------------------------------------------------------
// analyze: static analysis subcommands
// ---------------------------------------------------------------------------

fn run_analyze(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: tsc-rs analyze <subcommand> [options]");
        eprintln!();
        eprintln!("Subcommands:");
        eprintln!("  externals     Audit serverExternalPackages against actual import graph");
        eprintln!("  dep-graph     Show dependency fan-out from entry files");
        eprintln!("  dead-code     Find orphan files never imported by anything");
        eprintln!("  cycles        Find circular import dependencies");
        eprintln!("  package-deps  Compare package.json against actual bare-package imports");
        eprintln!("  closure       Per-route transitive closure for boot-time pre-compile");
        eprintln!();
        eprintln!("Run 'tsc-rs analyze <subcommand> --help' for details.");
        std::process::exit(1);
    }

    match args[0].as_str() {
        "externals" => run_analyze_externals(&args[1..]),
        "dep-graph" => run_analyze_dep_graph(&args[1..]),
        "dead-code" => run_analyze_dead_code(&args[1..]),
        "cycles" => run_analyze_cycles(&args[1..]),
        "package-deps" => run_analyze_package_deps(&args[1..]),
        "closure" => run_analyze_closure(&args[1..]),
        other => {
            eprintln!("Unknown analyze subcommand: {other}");
            std::process::exit(1);
        }
    }
}

fn run_analyze_externals(args: &[String]) {
    let mut entry_dir: Option<String> = None;
    let mut project_path: Option<String> = None;
    let mut externals_file: Option<String> = None;
    let mut definitions_dir: Option<String> = None;
    let mut json_output = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: tsc-rs analyze externals [options]");
                println!();
                println!("Audit which serverExternalPackages are actually imported.");
                println!();
                println!("Options:");
                println!("  --entry <dir>        Source directory to scan (default: src/)");
                println!("  -p <tsconfig.json>   Load compiler options from tsconfig");
                println!("  --externals <file>   File with externals list (JSON array, next.config.mjs, or one-per-line)");
                println!("  --definitions <dir>  Service-registry definitions dir (scans opaqueImport patterns)");
                println!("  --json               Output as JSON");
                return;
            }
            "--entry" => {
                i += 1;
                entry_dir = args.get(i).cloned();
            }
            "-p" | "--project" => {
                i += 1;
                project_path = args.get(i).cloned();
            }
            "--externals" => {
                i += 1;
                externals_file = args.get(i).cloned();
            }
            "--definitions" => {
                i += 1;
                definitions_dir = args.get(i).cloned();
            }
            "--json" => json_output = true,
            other => {
                eprintln!("Unknown option: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Load externals list
    let externals = if let Some(ref path) = externals_file {
        let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("Cannot read externals file '{path}': {e}");
            std::process::exit(1);
        });
        tsc_rs_analyze::externals::parse_externals_file(&content)
    } else {
        eprintln!("--externals <file> is required");
        std::process::exit(1);
    };

    if externals.is_empty() {
        eprintln!("No externals found in the provided file.");
        std::process::exit(1);
    }
    eprintln!("Loaded {} externals", externals.len());

    // Load compiler options — use a safe loader that detects self-referencing extends
    let options = if let Some(ref cp) = project_path {
        load_compiler_options_safe(cp)
    } else {
        CompilerOptions::default()
    };

    // Discover entry files
    let entry_files = if let Some(ref dir) = entry_dir {
        let path = Path::new(dir);
        if !path.is_dir() {
            eprintln!("Entry directory '{dir}' does not exist");
            std::process::exit(1);
        }
        eprintln!("Discovering .ts/.tsx files in {dir}...");
        tsc_rs_analyze::import_graph::discover_ts_files(path)
    } else if let Some(ref cp) = project_path {
        match TsProject::from_config(cp) {
            Ok(p) => p.file_names,
            Err(_) => {
                let default_dir = Path::new("src");
                if default_dir.is_dir() {
                    tsc_rs_analyze::import_graph::discover_ts_files(default_dir)
                } else {
                    eprintln!("No --entry dir and no tsconfig files. Specify --entry <dir>.");
                    std::process::exit(1);
                }
            }
        }
    } else {
        let default_dir = Path::new("src");
        if default_dir.is_dir() {
            tsc_rs_analyze::import_graph::discover_ts_files(default_dir)
        } else {
            eprintln!("No --entry dir specified and no src/ directory found.");
            std::process::exit(1);
        }
    };

    eprintln!("Found {} entry files", entry_files.len());

    // Build import graph
    eprintln!("Building import graph...");
    let builder = tsc_rs_analyze::import_graph::ImportGraphBuilder::new(options);
    let graph = builder.build(&entry_files);
    eprintln!(
        "Walked {} files ({} read errors), found {} unique packages",
        graph.visited_files.len(),
        graph.read_errors,
        graph.packages.len()
    );

    // Scan opaqueImport definitions if provided
    let opaque_scan = if let Some(ref dir) = definitions_dir {
        let defs_path = Path::new(dir);
        if !defs_path.is_dir() {
            eprintln!("Definitions directory '{dir}' does not exist");
            std::process::exit(1);
        }
        eprintln!("Scanning opaqueImport definitions in {dir}...");
        let scan = tsc_rs_analyze::opaque_imports::scan_definitions_dir(defs_path);
        eprintln!(
            "Found {} opaque packages from {} definition files",
            scan.packages.len(),
            scan.files_scanned
        );
        Some(scan)
    } else {
        None
    };

    // Audit
    let audit =
        tsc_rs_analyze::externals::audit_externals(&graph, &externals, opaque_scan.as_ref());

    if json_output {
        println!("{}", audit.format_json());
    } else {
        println!("{}", audit.format_report());
    }
}

fn run_analyze_dep_graph(args: &[String]) {
    let mut entry_dir: Option<String> = None;
    let mut project_path: Option<String> = None;
    let mut top_n: usize = 20;
    let mut reverse_target: Option<String> = None;
    let mut show_packages = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: tsc-rs analyze dep-graph [options]");
                println!();
                println!("Show dependency fan-out from entry files.");
                println!();
                println!("Options:");
                println!("  --entry <dir>        Source directory to scan");
                println!("  -p <tsconfig.json>   Load compiler options from tsconfig");
                println!("  --top <N>            Show top N files by fan-out (default: 20)");
                println!("  --reverse <target>   Show files that import <target> (file path or package name)");
                println!("  --packages           Show per-package import counts");
                return;
            }
            "--entry" => {
                i += 1;
                entry_dir = args.get(i).cloned();
            }
            "-p" | "--project" => {
                i += 1;
                project_path = args.get(i).cloned();
            }
            "--top" => {
                i += 1;
                top_n = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(20);
            }
            "--reverse" => {
                i += 1;
                reverse_target = args.get(i).cloned();
            }
            "--packages" => show_packages = true,
            other => {
                eprintln!("Unknown option: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let options = if let Some(ref cp) = project_path {
        load_compiler_options_safe(cp)
    } else {
        CompilerOptions::default()
    };

    let entry_files = if let Some(ref dir) = entry_dir {
        let path = Path::new(dir);
        if !path.is_dir() {
            eprintln!("Entry directory '{dir}' does not exist");
            std::process::exit(1);
        }
        eprintln!("Discovering files in {dir}...");
        tsc_rs_analyze::import_graph::discover_ts_files(path)
    } else {
        eprintln!("--entry <dir> is required");
        std::process::exit(1);
    };

    eprintln!("Found {} entry files", entry_files.len());
    eprintln!("Building import graph...");

    let builder = tsc_rs_analyze::import_graph::ImportGraphBuilder::new(options);
    let graph = builder.build(&entry_files);

    // Reverse dependency mode
    if let Some(ref target) = reverse_target {
        println!("Files that import \"{}\":", target);
        println!("{}", "-".repeat(80));

        let mut importers: Vec<(&String, &str)> = Vec::new();
        for (file, edges) in &graph.edges {
            for edge in edges {
                let matches = if target.contains('/') && !target.starts_with('@') {
                    // Target looks like a file path — match resolved paths
                    edge.resolved_path
                        .as_ref()
                        .map_or(false, |rp| rp.ends_with(target))
                } else {
                    // Target looks like a package name — match specifiers
                    edge.package_name.as_deref() == Some(target)
                        || edge.specifier == *target
                        || edge.specifier.starts_with(&format!("{target}/"))
                };
                if matches {
                    let kind = if edge.type_only {
                        "type"
                    } else if edge.dynamic {
                        "dynamic"
                    } else {
                        "runtime"
                    };
                    importers.push((file, kind));
                }
            }
        }
        importers.sort_by_key(|(f, _)| (*f).clone());
        importers.dedup();

        if importers.is_empty() {
            println!("  (no imports found)");
        } else {
            for (file, kind) in &importers {
                println!("  [{kind:>7}]  {file}");
            }
            println!();
            println!("Total: {} files import \"{}\"", importers.len(), target);
        }
        return;
    }

    // Package breakdown mode
    if show_packages {
        let mut pkg_list: Vec<(&String, &tsc_rs_analyze::import_graph::PackageUsage)> =
            graph.packages.iter().collect();
        pkg_list.sort_by(|a, b| b.1.import_count.cmp(&a.1.import_count));

        println!("Packages by import count:");
        println!("{:<8} {:<10} {}", "Count", "Kind", "Package");
        println!("{}", "-".repeat(80));
        for (pkg, usage) in &pkg_list {
            let kind = if usage.type_only {
                "type-only"
            } else {
                "runtime"
            };
            println!("{:<8} {:<10} {pkg}", usage.import_count, kind);
        }
        println!();
        println!("Total: {} unique packages", pkg_list.len());
        return;
    }

    // Default: fan-out mode
    let mut fan_out: Vec<(&String, usize)> = graph
        .edges
        .iter()
        .map(|(file, edges)| (file, edges.len()))
        .collect();
    fan_out.sort_by(|a, b| b.1.cmp(&a.1));

    println!("Top {} files by import fan-out:", top_n);
    println!("{:<6} {}", "Count", "File");
    println!("{}", "-".repeat(80));
    for (file, count) in fan_out.iter().take(top_n) {
        println!("{:<6} {file}", count);
    }

    println!();
    println!(
        "Total: {} files, {} edges, {} unique packages",
        graph.visited_files.len(),
        graph.edges.values().map(|v| v.len()).sum::<usize>(),
        graph.packages.len(),
    );

    // Show unresolved imports
    if !graph.unresolved.is_empty() {
        println!();
        println!("Unresolved relative imports ({}):", graph.unresolved.len());
        for u in graph.unresolved.iter().take(20) {
            println!("  {} → \"{}\"", u.from_file, u.specifier);
        }
        if graph.unresolved.len() > 20 {
            println!("  ... and {} more", graph.unresolved.len() - 20);
        }
    }
}

fn run_analyze_dead_code(args: &[String]) {
    let mut entry_dir: Option<String> = None;
    let mut project_path: Option<String> = None;
    let mut show_all = false;
    let mut min_size: u64 = 0;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: tsc-rs analyze dead-code [options]");
                println!();
                println!("Find orphan files never imported by anything.");
                println!();
                println!("Options:");
                println!("  --entry <dir>        Source directory to scan");
                println!("  -p <tsconfig.json>   Load compiler options from tsconfig");
                println!("  --all                Show entry points, tests, scripts too (not just dead code)");
                println!("  --min-size <bytes>   Only show dead files larger than N bytes");
                return;
            }
            "--entry" => {
                i += 1;
                entry_dir = args.get(i).cloned();
            }
            "-p" | "--project" => {
                i += 1;
                project_path = args.get(i).cloned();
            }
            "--all" => show_all = true,
            "--min-size" => {
                i += 1;
                min_size = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            other => {
                eprintln!("Unknown option: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let options = if let Some(ref cp) = project_path {
        load_compiler_options_safe(cp)
    } else {
        CompilerOptions::default()
    };

    let entry_files = if let Some(ref dir) = entry_dir {
        let path = Path::new(dir);
        if !path.is_dir() {
            eprintln!("Entry directory '{dir}' does not exist");
            std::process::exit(1);
        }
        eprintln!("Discovering files in {dir}...");
        tsc_rs_analyze::import_graph::discover_ts_files(path)
    } else {
        eprintln!("--entry <dir> is required");
        std::process::exit(1);
    };

    let all_source_files = entry_files.clone();
    eprintln!("Found {} source files", all_source_files.len());
    eprintln!("Building import graph...");

    let builder = tsc_rs_analyze::import_graph::ImportGraphBuilder::new(options);
    let graph = builder.build(&entry_files);
    eprintln!(
        "Walked {} files ({} read errors)",
        graph.visited_files.len(),
        graph.read_errors
    );

    let mut report = tsc_rs_analyze::dead_code::find_dead_code(&graph, &all_source_files);

    // Apply min-size filter
    if min_size > 0 {
        report.orphans.retain(|o| {
            o.category != tsc_rs_analyze::dead_code::OrphanCategory::DeadCode
                || o.size_bytes >= min_size
        });
        // Recalculate stats
        report.stats.dead_code_count = report
            .orphans
            .iter()
            .filter(|o| o.category == tsc_rs_analyze::dead_code::OrphanCategory::DeadCode)
            .count();
        report.stats.dead_code_bytes = report
            .orphans
            .iter()
            .filter(|o| o.category == tsc_rs_analyze::dead_code::OrphanCategory::DeadCode)
            .map(|o| o.size_bytes)
            .sum();
    }

    println!("{}", report.format_report(show_all));
}

fn run_analyze_package_deps(args: &[String]) {
    let mut entry_dir: Option<String> = None;
    let mut project_path: Option<String> = None;
    let mut manifest_path: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: tsc-rs analyze package-deps [options]");
                println!();
                println!("Compare a site's package.json against the bare-package imports");
                println!("observed in its source tree. Surfaces:");
                println!("  - undeclared (runtime): imported but not in package.json — relies");
                println!("    on workspace/hoist resolution and breaks on a fresh install");
                println!("  - undeclared (type-only): drift, no runtime risk");
                println!("  - unused: declared in package.json but no source imports");
                println!();
                println!("Options:");
                println!("  --entry <dir>        Source directory to scan (default: src/)");
                println!("  -p <tsconfig.json>   Load compiler options from tsconfig");
                println!("  --package <file>     Path to package.json (default: ./package.json)");
                return;
            }
            "--entry" => {
                i += 1;
                entry_dir = args.get(i).cloned();
            }
            "-p" | "--project" => {
                i += 1;
                project_path = args.get(i).cloned();
            }
            "--package" => {
                i += 1;
                manifest_path = args.get(i).cloned();
            }
            other => {
                eprintln!("Unknown option: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let manifest_path = manifest_path.unwrap_or_else(|| "package.json".to_string());
    let manifest_content = std::fs::read_to_string(&manifest_path).unwrap_or_else(|e| {
        eprintln!("Cannot read package.json '{manifest_path}': {e}");
        std::process::exit(1);
    });
    let manifest = match tsc_rs_analyze::package_deps::parse_manifest(&manifest_content) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Cannot parse '{manifest_path}': {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "Manifest: {} declared total ({} registry-backed)",
        manifest.declared_all().len(),
        manifest.declared_registry().len()
    );

    let options = if let Some(ref cp) = project_path {
        load_compiler_options_safe(cp)
    } else {
        CompilerOptions::default()
    };

    let entry_files = if let Some(ref dir) = entry_dir {
        let path = Path::new(dir);
        if !path.is_dir() {
            eprintln!("Entry directory '{dir}' does not exist");
            std::process::exit(1);
        }
        eprintln!("Discovering files in {dir}...");
        tsc_rs_analyze::import_graph::discover_ts_files(path)
    } else {
        let default_dir = Path::new("src");
        if default_dir.is_dir() {
            eprintln!("Discovering files in src/ (default)...");
            tsc_rs_analyze::import_graph::discover_ts_files(default_dir)
        } else {
            eprintln!("--entry <dir> is required (no src/ found)");
            std::process::exit(1);
        }
    };

    eprintln!("Found {} entry files", entry_files.len());
    eprintln!("Building import graph...");

    let builder = tsc_rs_analyze::import_graph::ImportGraphBuilder::new(options);
    let graph = builder.build(&entry_files);
    eprintln!(
        "Walked {} files ({} read errors)",
        graph.visited_files.len(),
        graph.read_errors
    );

    // Resolve workspace peer requirements so deps the framework needs at
    // runtime don't show up as "unused" (e.g. react-dom is in every bext
    // site's deps because @bext-stack/framework's compiled output requires
    // it; no source file in the site itself imports it).
    let site_dir = std::path::Path::new(&manifest_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let peer_required_by =
        tsc_rs_analyze::package_deps::collect_workspace_peer_requirements(&manifest, &site_dir);
    let audit = tsc_rs_analyze::package_deps::audit_package_deps_with_peers(
        &graph,
        &manifest,
        &peer_required_by,
    );
    println!("{}", audit.format_report());

    // Non-zero exit when there's a runtime drift the user almost always wants
    // to fix — easier to wire into CI without a separate parser.
    if audit.stats.undeclared_runtime_count > 0 {
        std::process::exit(2);
    }
}

fn run_analyze_closure(args: &[String]) {
    let mut app_dir: Option<String> = None;
    let mut project_path: Option<String> = None;
    let mut json_output = false;
    let mut skip_node_modules = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: tsc-rs analyze closure [options]");
                println!();
                println!("Compute per-route transitive closures from a bext / Next.js app");
                println!("directory. Discovers route entry files (page.tsx, layout.tsx,");
                println!("route.ts, middleware.ts, etc.), builds the import graph FOLLOWING");
                println!("into node_modules by default, and emits each route's reachable");
                println!("file set plus the union over all routes.");
                println!();
                println!("Use the JSON output to drive boot-time pre-compile so a server's");
                println!("module registry is populated deterministically before its V8");
                println!("startup snapshot is baked.");
                println!();
                println!("Options:");
                println!("  --app <dir>          App directory containing page.tsx etc. (default: src/app)");
                println!("  -p <tsconfig.json>   Load compiler options from tsconfig");
                println!("  --skip-node-modules  Stop the walk at the package boundary");
                println!("  --json               Emit structured JSON for downstream tooling");
                return;
            }
            "--app" => {
                i += 1;
                app_dir = args.get(i).cloned();
            }
            "-p" | "--project" => {
                i += 1;
                project_path = args.get(i).cloned();
            }
            "--skip-node-modules" => skip_node_modules = true,
            "--json" => json_output = true,
            other => {
                eprintln!("Unknown option: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let app_dir = app_dir.unwrap_or_else(|| "src/app".to_string());
    let app_path = Path::new(&app_dir);
    if !app_path.is_dir() {
        eprintln!("App directory '{app_dir}' does not exist");
        std::process::exit(1);
    }

    let entries = tsc_rs_analyze::closure::discover_app_route_entries(app_path);
    if entries.is_empty() {
        eprintln!("No route entries found under '{app_dir}' (expected page.tsx / layout.tsx / route.ts / middleware.ts)");
        std::process::exit(1);
    }
    eprintln!("Discovered {} route entries under {app_dir}", entries.len());

    let options = if let Some(ref cp) = project_path {
        load_compiler_options_safe(cp)
    } else {
        CompilerOptions::default()
    };

    let entry_files: Vec<String> = entries.iter().map(|(f, _)| f.clone()).collect();
    let mut builder = tsc_rs_analyze::import_graph::ImportGraphBuilder::new(options);
    // Following into node_modules is the whole point — bext's prewarm wants
    // the framework + vendor closure baked into the snapshot, not just src/.
    builder.skip_node_modules = skip_node_modules;
    eprintln!(
        "Building import graph (skip_node_modules = {})...",
        skip_node_modules
    );

    let graph = builder.build(&entry_files);
    eprintln!(
        "Walked {} files ({} read errors, {} unresolved)",
        graph.visited_files.len(),
        graph.read_errors,
        graph.unresolved.len()
    );

    let report = tsc_rs_analyze::closure::compute_closures(&graph, &entries);

    if json_output {
        print!("{}", report.format_json());
    } else {
        print!("{}", report.format_text());
    }
}

fn run_analyze_cycles(args: &[String]) {
    let mut entry_dir: Option<String> = None;
    let mut project_path: Option<String> = None;
    let mut max_show: usize = 30;
    let mut static_only = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: tsc-rs analyze cycles [options]");
                println!();
                println!("Find circular import dependencies.");
                println!();
                println!("Options:");
                println!("  --entry <dir>        Source directory to scan");
                println!("  -p <tsconfig.json>   Load compiler options from tsconfig");
                println!("  --max <N>            Max cycles to display (default: 30)");
                println!("  --static-only        Exclude dynamic import() edges (only show build-time cycles)");
                return;
            }
            "--entry" => {
                i += 1;
                entry_dir = args.get(i).cloned();
            }
            "-p" | "--project" => {
                i += 1;
                project_path = args.get(i).cloned();
            }
            "--max" => {
                i += 1;
                max_show = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(30);
            }
            "--static-only" => static_only = true,
            other => {
                eprintln!("Unknown option: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let options = if let Some(ref cp) = project_path {
        load_compiler_options_safe(cp)
    } else {
        CompilerOptions::default()
    };

    let entry_files = if let Some(ref dir) = entry_dir {
        let path = Path::new(dir);
        if !path.is_dir() {
            eprintln!("Entry directory '{dir}' does not exist");
            std::process::exit(1);
        }
        eprintln!("Discovering files in {dir}...");
        tsc_rs_analyze::import_graph::discover_ts_files(path)
    } else {
        eprintln!("--entry <dir> is required");
        std::process::exit(1);
    };

    eprintln!("Found {} source files", entry_files.len());
    eprintln!("Building import graph...");

    let builder = tsc_rs_analyze::import_graph::ImportGraphBuilder::new(options);
    let graph = builder.build(&entry_files);
    eprintln!(
        "Walked {} files ({} read errors)",
        graph.visited_files.len(),
        graph.read_errors
    );
    eprintln!("Detecting cycles...");

    let report = if static_only {
        tsc_rs_analyze::cycles::find_cycles_static_only(&graph)
    } else {
        tsc_rs_analyze::cycles::find_cycles(&graph)
    };
    println!("{}", report.format_report(max_show));
}

/// Load CompilerOptions from a tsconfig, guarding against self-referencing extends.
fn load_compiler_options_safe(config_path: &str) -> CompilerOptions {
    let abs_path =
        std::fs::canonicalize(config_path).unwrap_or_else(|_| PathBuf::from(config_path));

    // Check for self-referencing extends before delegating to TsProject
    if let Ok(content) = std::fs::read_to_string(&abs_path) {
        // Quick check: does "extends" point to itself?
        if let Some(extends_val) = extract_simple_json_field(&content, "extends") {
            let extends_path = if Path::new(&extends_val).is_absolute() {
                PathBuf::from(&extends_val)
            } else {
                abs_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(&extends_val)
            };
            if let Ok(extends_canonical) = std::fs::canonicalize(&extends_path) {
                if extends_canonical == abs_path {
                    eprintln!(
                        "warning: tsconfig '{config_path}' extends itself — loading options directly"
                    );
                    // Parse options directly without extends resolution
                    return parse_options_from_json(&content, &abs_path);
                }
            }
        }
    }

    // Safe to use TsProject::from_config
    match TsProject::from_config(config_path) {
        Ok(p) => p.options,
        Err(e) => {
            eprintln!("warning: cannot fully load tsconfig '{config_path}': {e}");
            // Fall back to direct parsing
            if let Ok(content) = std::fs::read_to_string(config_path) {
                let abs = std::fs::canonicalize(config_path)
                    .unwrap_or_else(|_| PathBuf::from(config_path));
                parse_options_from_json(&content, &abs)
            } else {
                CompilerOptions::default()
            }
        }
    }
}

/// Extract a string field value from JSON (minimal, no serde).
fn extract_simple_json_field(json: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{field}\"");
    let pos = json.find(&pattern)?;
    let after_key = &json[pos + pattern.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let trimmed = after_colon.trim_start();
    if trimmed.starts_with('"') {
        // Find closing quote, skipping escaped quotes
        let bytes = trimmed.as_bytes();
        let mut i = 1; // skip opening "
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2; // skip escaped char
            } else if bytes[i] == b'"' {
                return Some(trimmed[1..i].to_string());
            } else {
                i += 1;
            }
        }
        None
    } else {
        None
    }
}

/// Parse CompilerOptions directly from tsconfig JSON content.
/// Only extracts what the analyze commands need: baseUrl and paths.
fn parse_options_from_json(content: &str, config_path: &Path) -> CompilerOptions {
    let mut options = CompilerOptions::default();

    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    // Extract baseUrl
    if let Some(base_url) = extract_simple_json_field(content, "baseUrl") {
        let abs_base = if Path::new(&base_url).is_absolute() {
            PathBuf::from(&base_url)
        } else {
            config_dir.join(&base_url)
        };
        options.base_url = Some(abs_base.to_string_lossy().to_string());
    } else {
        // Default baseUrl to config directory for path resolution
        options.base_url = Some(config_dir.to_string_lossy().to_string());
    }

    // Extract paths block (the whole JSON object as a string)
    if let Some(paths_start) = content.find("\"paths\"") {
        let after = &content[paths_start + 7..];
        if let Some(brace_start) = after.find('{') {
            let from_brace = &after[brace_start..];
            let mut depth = 0;
            for (i, ch) in from_brace.char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            options.paths = Some(from_brace[..=i].to_string());
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    options
}

// ---------------------------------------------------------------------------
// --init: generate default tsconfig.json
// ---------------------------------------------------------------------------

fn run_init() {
    let path = Path::new("tsconfig.json");
    if path.exists() {
        eprintln!("tsconfig.json already exists.");
        std::process::exit(1);
    }

    let content = r#"{
  "compilerOptions": {
    "target": "es2016",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  }
}
"#;

    match std::fs::write(path, content) {
        Ok(()) => {
            println!("Created tsconfig.json");
        }
        Err(e) => {
            eprintln!("error: Cannot write tsconfig.json: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// LSP server
// ---------------------------------------------------------------------------

fn run_lsp() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    if let Err(e) = tsc_rs_server::run_server(&mut reader, &mut writer) {
        eprintln!("LSP server error: {e}");
        std::process::exit(2);
    }
}

// ---------------------------------------------------------------------------
// Harness commands (discover / run-suite)
// ---------------------------------------------------------------------------

fn run_harness(args: &[String]) {
    let harness = BootstrapHarness;
    let mut iter = args.iter();
    let command = iter.next().unwrap();

    match command.as_str() {
        "discover" => {
            let repo = iter
                .next()
                .unwrap_or_else(|| exit_with("missing TypeScript repo path"));
            let suite_raw = iter
                .next()
                .unwrap_or_else(|| exit_with("missing suite name"));
            let suite = parse_suite(suite_raw);

            match harness.discover_cases(&PathBuf::from(repo), suite) {
                Ok(cases) => {
                    println!("suite={} cases={}", suite.as_str(), cases.len());
                    for case in cases {
                        println!("{}", case.display());
                    }
                }
                Err(err) => exit_with(&format!("failed to discover cases: {err}")),
            }
        }
        "run-suite" => {
            let repo = iter
                .next()
                .unwrap_or_else(|| exit_with("missing TypeScript repo path"));
            let suite_raw = iter
                .next()
                .unwrap_or_else(|| exit_with("missing suite name"));
            let oracle_raw = iter
                .next()
                .unwrap_or_else(|| exit_with("missing oracle command template"));
            let candidate_raw = iter
                .next()
                .unwrap_or_else(|| exit_with("missing candidate command template"));
            let max_cases = iter.next().and_then(|raw| raw.parse::<usize>().ok());

            let suite = parse_suite(suite_raw);
            let oracle_command = parse_template(oracle_raw, "oracle");
            let candidate_command = parse_template(candidate_raw, "candidate");

            let config = SuiteRunConfig {
                ts_repo: PathBuf::from(repo),
                suite,
                oracle_command,
                candidate_command,
                max_cases,
            };

            match harness.run_suite(&config) {
                Ok(result) => {
                    println!(
                        "suite={} total={} passed={} failed={}",
                        result.suite.as_str(),
                        result.total,
                        result.passed,
                        result.failed.len()
                    );
                    for failed in result.failed {
                        println!("FAIL {} => {:?}", failed.case.display(), failed.mismatches);
                    }
                }
                Err(err) => exit_with(&format!("suite run failed: {err}")),
            }
        }
        _ => unreachable!(),
    }
}

fn parse_suite(raw: &str) -> Suite {
    match raw.parse::<Suite>() {
        Ok(suite) => suite,
        Err(err) => exit_with(err),
    }
}

fn parse_template(raw: &str, name: &str) -> CommandTemplate {
    match CommandTemplate::parse(raw) {
        Ok(template) => template,
        Err(err) => exit_with(&format!("invalid {name} command template: {err}")),
    }
}

fn exit_with(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

fn print_help() {
    println!("tsc-rs {VERSION} - TypeScript compiler (Rust)");
    println!();
    println!("Usage:");
    println!("  tsc-rs [options] [file ...]");
    println!();
    println!("Compilation:");
    println!("  tsc-rs file.ts                Compile TypeScript file(s)");
    println!("  tsc-rs -p tsconfig.json       Compile project from tsconfig.json");
    println!("  tsc-rs --noEmit               Type-check only, no output files");
    println!("  tsc-rs -w                     Watch mode (recompile on changes)");
    println!("  tsc-rs --init                 Generate a default tsconfig.json");
    println!("  tsc-rs -b [tsconfig.json]     Build project references");
    println!();
    println!("Compiler flags:");
    println!("  --outDir <dir>                Redirect output to directory");
    println!("  --target <target>             Set ECMAScript target (es5, es2015, esnext, ...)");
    println!("  --module <module>             Set module system (commonjs, es2015, esnext, ...)");
    println!("  --declaration, -d             Generate .d.ts declaration files");
    println!("  --sourceMap                   Generate source map files");
    println!("  --listFiles                   Print all source files in the compilation");
    println!("  --listFilesOnly               Print source files and exit");
    println!("  --strict                      Enable all strict type-checking options");
    println!("  --incremental                 Enable incremental compilation");
    println!("  --composite                   Enable composite project (implies --incremental -d)");
    println!("  --preserveTypeAnnotations     Preserve type annotations (as, satisfies, <Type>)");
    println!("  --preserveComments            Preserve comments even when removeComments is set");
    println!("  --preserveWhitespace           Preserve whitespace in output");
    println!("  --jobs, -j <N>                 Parallel worker count (default: physical cores; env: TSC_RS_JOBS)");
    println!("  --maxErrors <N>                Cap diagnostics printed (summary still reflects full count)");
    println!();
    println!("JSX:");
    println!(
        "  --jsx <mode>                  Set JSX emit mode (react, react-jsx, preserve, none)"
    );
    println!(
        "  --jsxFactory <factory>        Set JSX factory function (default: React.createElement)"
    );
    println!("  --jsxImportSource <source>    Set JSX import source (default: react)");
    println!();
    println!("Transform:");
    println!("  --transpileOnly               Skip type checking, just parse + emit (fast JSX/TS transform)");
    println!("  --fast-emit                   Skip cosmetic formatting normalization (verbatim copy of no-transform stmts). Valid JS, not tsc-formatted. For consumers like PRISM that feed V8.");
    println!(
        "  --pipe                        Persistent transpile-only pipe (JSON requests on stdin)"
    );
    println!(
        "  --check-pipe -p tsconfig.json  Persistent type-check daemon (JSON requests on stdin)"
    );
    println!();
    println!("Watch:");
    println!("  --watch, -w                   Watch mode (recompile on changes)");
    println!("  --preserveWatchOutput         Do not clear screen on recompile");
    println!();
    println!("Analysis:");
    println!("  tsc-rs analyze externals      Audit serverExternalPackages against import graph");
    println!("  tsc-rs analyze dep-graph      Show dependency fan-out from entry files");
    println!();
    println!("Other:");
    println!("  --version, -v                 Print version");
    println!("  --help, -h                    Print this help");
    println!("  --lsp                         Start LSP server on stdin/stdout");
    println!();
    println!("Harness:");
    println!("  tsc-rs discover <ts-repo> <suite>");
    println!("  tsc-rs run-suite <ts-repo> <suite> '<oracle>' '<candidate>' [max]");
    println!("  suite: compiler | fourslash | project");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_project(options: CompilerOptions) -> TsProject {
        TsProject::new(Vec::new(), options)
    }

    #[test]
    fn query_pipeline_enabled_for_no_emit_non_incremental() {
        let project = test_project(CompilerOptions::default());
        let cli = CliArgs {
            no_emit: true,
            ..CliArgs::default()
        };
        assert!(should_use_query_pipeline(&project, &cli));
    }

    #[test]
    fn query_pipeline_disabled_for_incremental_no_emit() {
        let options = CompilerOptions {
            incremental: Some(true),
            ..CompilerOptions::default()
        };
        let project = test_project(options);
        let cli = CliArgs {
            no_emit: true,
            ..CliArgs::default()
        };
        assert!(!should_use_query_pipeline(&project, &cli));
    }

    #[test]
    fn query_pipeline_enabled_when_project_sets_no_emit() {
        let options = CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        };
        let project = test_project(options);
        let cli = CliArgs::default();
        assert!(should_use_query_pipeline(&project, &cli));
    }

    #[test]
    fn query_pipeline_disabled_when_cli_forces_composite() {
        let project = test_project(CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        });
        let cli = CliArgs {
            composite: true,
            ..CliArgs::default()
        };
        assert!(!should_use_query_pipeline(&project, &cli));
    }

    #[test]
    fn parse_list_files_flag() {
        let args = vec!["--listFiles".to_string(), "src/main.ts".to_string()];
        let cli = CliArgs::parse(&args).expect("args should parse");
        assert!(cli.list_files);
        assert!(!cli.list_files_only);
        assert_eq!(cli.files, vec!["src/main.ts"]);
    }

    #[test]
    fn list_files_only_short_circuits_compilation_path() {
        let project = test_project(CompilerOptions::default());
        let cli = CliArgs {
            list_files_only: true,
            ..CliArgs::default()
        };
        assert!(maybe_print_file_list(&project, &cli));
    }
}
