//! Persistent pipe mode for tsc-rs: newline-delimited JSON in/out.
//!
//! Supports both single and batch requests, nested compile options,
//! structured import/export extraction, and optional source maps.
//!
//! # Protocol
//!
//! ## Single request
//! ```json
//! {
//!   "id": "1",
//!   "file": "page.tsx",
//!   "source": "...",
//!   "options": {
//!     "target": "es2022",
//!     "module": "commonjs",
//!     "jsx": "react",
//!     "sourceMap": true,
//!     "importHelpers": true,
//!     "removeComments": false,
//!     "useDefineForClassFields": true
//!   },
//!   "pkg_json_type": "module"
//! }
//! ```
//! Flat `jsx` / `module` fields are accepted when `options` is absent.
//!
//! ## Single response
//! ```json
//! {
//!   "id": "1",
//!   "ok": true,
//!   "output": "...",
//!   "elapsed_ms": 5,
//!   "imports": [{"specifier": "react", "kind": "default"}, ...],
//!   "dynamic_imports": ["./lazy"],
//!   "exports": ["Button", "default"],
//!   "sourceMap": "{...}"
//! }
//! ```
//!
//! ## Batch request
//! ```json
//! {"id":"batch1","batch":[{"file":"a.ts","source":"..."}, ...]}
//! ```
//!
//! ## Batch response
//! ```json
//! {"id":"batch1","ok":true,"results":[{"file":"a.ts","ok":true,...}, ...]}
//! ```

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

use tsc_rs_ast::{
    ArrayPatElem, ArrowBody, ClassMemberKind, CompilerOptions, ExportDeclKind, Expr, ExprKind,
    ImportClause, JsxEmit, ModuleKind, ModuleName, ObjLitProp, ObjPatProp, Pat, PatKind,
    ScriptTarget, SourceFile, Stmt, StmtKind,
};

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct PipeOptions {
    target: Option<String>,
    module: Option<String>,
    jsx: Option<String>,
    #[serde(rename = "jsxImportSource")]
    jsx_import_source: Option<String>,
    #[serde(rename = "jsxFactory")]
    jsx_factory: Option<String>,
    #[serde(rename = "sourceMap")]
    source_map: Option<bool>,
    #[serde(rename = "importHelpers")]
    import_helpers: Option<bool>,
    #[serde(rename = "removeComments")]
    remove_comments: Option<bool>,
    #[serde(rename = "useDefineForClassFields")]
    use_define_for_class_fields: Option<bool>,
    /// Skip cosmetic formatting normalization (verbatim copy of no-transform
    /// statements). Output is semantically-identical valid JS, just formatted as
    /// the source was. For consumers like PRISM that feed the JS straight to V8.
    #[serde(rename = "fastEmit")]
    fast_emit: Option<bool>,
}

#[derive(Deserialize, Default)]
struct CompileUnit {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    options: Option<PipeOptions>,
    #[serde(default)]
    jsx: Option<String>,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    pkg_json_type: Option<String>,
}

#[derive(Deserialize, Default)]
struct PipeRequest {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    options: Option<PipeOptions>,
    #[serde(default)]
    jsx: Option<String>,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    pkg_json_type: Option<String>,
    #[serde(default)]
    batch: Option<Vec<CompileUnit>>,
}

#[derive(Serialize)]
struct ImportEntry {
    specifier: String,
    kind: &'static str,
}

#[derive(Serialize, Default)]
struct CompileResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    elapsed_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    imports: Vec<ImportEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dynamic_imports: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exports: Vec<String>,
    #[serde(rename = "sourceMap", skip_serializing_if = "Option::is_none")]
    source_map: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Read VmRSS (resident set size) of this process in kilobytes from
/// `/proc/self/status`. Linux-only; returns `None` on other platforms or
/// when the file is unreadable.
///
/// We deliberately avoid the rusage crate / sysinfo to keep tsc-rs free
/// of process-introspection deps — this is one small read of one well-known
/// kernel pseudo-file, called every `RSS_CHECK_INTERVAL` requests.
fn read_rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            for tok in rest.split_whitespace() {
                if let Ok(n) = tok.parse::<u64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Parse a `usize` env var with a default. Empty string is treated as unset.
fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| if v.is_empty() { None } else { Some(v) })
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

// Polling interval for the RSS check (in requests handled). Reading
// /proc/self/status every request would add ~20 µs of overhead each turn
// — fine for a build-time compiler, but unnecessary. 32 keeps the bound
// tight enough that we won't overshoot the limit by more than a couple of
// large compiles.
const RSS_CHECK_INTERVAL: usize = 32;

pub fn run_pipe() {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let ready = format!("{{\"ready\":true,\"pid\":{}}}\n", std::process::id());
    let _ = out.write_all(ready.as_bytes());
    let _ = out.flush();

    // Self-imposed ceilings — bext-server's pipe-worker supervisor
    // (`TscRsPipeWorker::spawn`) respawns transparently on close, so we
    // can exit voluntarily before mimalloc + parser allocations balloon
    // the RSS into territory that triggers the systemd nginx
    // MemoryHigh/MemoryMax limits or, worse, the host OOM killer that
    // cascades into bext's node_pool workers.
    //
    // The 2026-05-11 incident on the dev box had a single `tsc-rs --pipe`
    // process at 39 GB RSS — enough to swap-thrash the host, knock out
    // the masquerade :443 listener, and tank every downstream deploy via
    // verify-step rollbacks. Bounding the worker lifetime (and dropping
    // back to a fresh process every few thousand requests) eliminates
    // that failure mode without any behavior change on the bext side.
    //
    // Overrideable for ops:
    //   TSC_RS_PIPE_MAX_MEMORY_MB  default 2048   (0 disables RSS check)
    //   TSC_RS_PIPE_MAX_REQUESTS   default 5000   (0 disables count cap)
    let max_memory_mb = env_usize("TSC_RS_PIPE_MAX_MEMORY_MB", 2048);
    let max_requests = env_usize("TSC_RS_PIPE_MAX_REQUESTS", 5000);
    let rss_limit_kb = max_memory_mb.saturating_mul(1024) as u64;
    let mut request_count: usize = 0;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: PipeRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let msg = format!(
                    "{{\"ok\":false,\"error\":{}}}\n",
                    serde_json::Value::String(format!("invalid JSON: {e}"))
                );
                let _ = out.write_all(msg.as_bytes());
                let _ = out.flush();
                continue;
            }
        };

        let id = req.id.clone().unwrap_or_default();

        if req.cmd.as_deref() == Some("shutdown") {
            let resp = format!(
                "{{\"id\":{},\"ok\":true,\"shutdown\":true}}\n",
                serde_json::Value::String(id)
            );
            let _ = out.write_all(resp.as_bytes());
            let _ = out.flush();
            break;
        }

        let response_json = if let Some(batch) = req.batch {
            handle_batch(&id, batch)
        } else {
            let unit = CompileUnit {
                file: req.file,
                source: req.source,
                options: req.options,
                jsx: req.jsx,
                module: req.module,
                pkg_json_type: req.pkg_json_type,
            };
            let result = compile_one(&unit);
            serialize_single(&id, result)
        };

        let _ = out.write_all(response_json.as_bytes());
        let _ = out.write_all(b"\n");
        let _ = out.flush();

        // Lifetime check: bail before the next request when we've either
        // crossed the configured RSS ceiling or hit the request cap. We
        // log the reason on stderr (bext routes that to its own journald
        // sink, then re-spawns silently).
        request_count = request_count.saturating_add(1);
        let mut should_exit: Option<&'static str> = None;
        if max_requests != 0 && request_count >= max_requests {
            should_exit = Some("max-requests");
        } else if rss_limit_kb != 0 && request_count % RSS_CHECK_INTERVAL == 0 {
            if let Some(rss_kb) = read_rss_kb() {
                if rss_kb >= rss_limit_kb {
                    eprintln!(
                        "tsc-rs --pipe: RSS {} kB exceeded ceiling {} kB after {} requests; exiting for respawn",
                        rss_kb, rss_limit_kb, request_count
                    );
                    should_exit = Some("max-memory");
                }
            }
        }
        if let Some(reason) = should_exit {
            // Best-effort notice on stdout so bext-side logs note the
            // reason. The supervisor reads JSON line-delimited, so keep
            // the shape valid. Then break: dropping `out` flushes; the
            // process exits cleanly with status 0.
            let msg = format!(
                "{{\"ok\":true,\"shutdown\":true,\"reason\":{},\"requests\":{}}}\n",
                serde_json::Value::String(reason.into()),
                request_count
            );
            let _ = out.write_all(msg.as_bytes());
            let _ = out.flush();
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn handle_batch(id: &str, batch: Vec<CompileUnit>) -> String {
    let results: Vec<CompileResult> = batch
        .par_iter()
        .map(|unit| {
            let mut r = compile_one(unit);
            if r.file.is_none() {
                r.file = unit.file.clone();
            }
            r
        })
        .collect();

    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), serde_json::Value::String(id.to_string()));
    obj.insert("ok".into(), serde_json::Value::Bool(true));
    obj.insert(
        "results".into(),
        serde_json::to_value(&results).unwrap_or(serde_json::Value::Null),
    );
    serde_json::Value::Object(obj).to_string()
}

fn serialize_single(id: &str, mut result: CompileResult) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), serde_json::Value::String(id.to_string()));
    obj.insert("ok".into(), serde_json::Value::Bool(result.ok));
    if let Some(err) = result.error.take() {
        obj.insert("error".into(), serde_json::Value::String(err));
        obj.insert(
            "elapsed_ms".into(),
            serde_json::Value::Number(result.elapsed_ms.into()),
        );
        return serde_json::Value::Object(obj).to_string();
    }
    if let Some(out) = result.output.take() {
        obj.insert("output".into(), serde_json::Value::String(out));
    }
    obj.insert(
        "elapsed_ms".into(),
        serde_json::Value::Number(result.elapsed_ms.into()),
    );
    if !result.imports.is_empty() {
        obj.insert(
            "imports".into(),
            serde_json::to_value(&result.imports).unwrap_or(serde_json::Value::Null),
        );
    }
    if !result.dynamic_imports.is_empty() {
        obj.insert(
            "dynamic_imports".into(),
            serde_json::to_value(&result.dynamic_imports).unwrap_or(serde_json::Value::Null),
        );
    }
    if !result.exports.is_empty() {
        obj.insert(
            "exports".into(),
            serde_json::to_value(&result.exports).unwrap_or(serde_json::Value::Null),
        );
    }
    if let Some(sm) = result.source_map.take() {
        obj.insert("sourceMap".into(), serde_json::Value::String(sm));
    }
    serde_json::Value::Object(obj).to_string()
}

/// First parser (syntax) error of a source file, rendered as `file:line:col: TSxxxx: message`.
///
/// Only `Error` diagnostics count: recovery notes and suggestions must not
/// fail a build. The parser is the only stage that ran here, so every
/// diagnostic on the file is a syntax diagnostic.
fn first_syntax_error(sf: &tsc_rs_ast::SourceFile, file: &str, source: &str) -> Option<String> {
    let diag = sf
        .diagnostics
        .iter()
        .find(|d| d.category == tsc_rs_ast::DiagnosticCategory::Error)?;
    let (line, col) = diag
        .span
        .map(|span| line_and_column(source, span.start))
        .unwrap_or((1, 1));
    Some(format!(
        "{file}:{line}:{col}: TS{}: {}",
        diag.code, diag.message
    ))
}

/// 1-based line and column for a byte offset.
fn line_and_column(source: &str, offset: u32) -> (usize, usize) {
    let offset = (offset as usize).min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before.rfind('\n').map_or(offset, |nl| offset - nl - 1) + 1;
    (line, col)
}

fn compile_one(unit: &CompileUnit) -> CompileResult {
    let file = unit.file.clone().unwrap_or_else(|| "entry.tsx".to_string());
    let source = unit.source.clone().unwrap_or_default();

    let want_source_map = unit
        .options
        .as_ref()
        .and_then(|o| o.source_map)
        .unwrap_or(false);

    let options = build_compiler_options(unit);

    let start = std::time::Instant::now();
    let sf = tsc_rs_parser::parse(&file, &source);

    // A syntax error means the emitted JS is NOT the program the author wrote.
    // The parser recovers so the emitter can keep going, but recovery invents:
    // a stray backtick inside a template literal closes it early, truncating a
    // client script to a few characters and turning the identifier that
    // followed into a phantom export. Answering `ok: true` there hands the
    // caller corrupt output with no way to know — the page then renders 200
    // and only the browser fails. Refuse the unit instead; the caller decides
    // how loudly to fail.
    if let Some(err) = first_syntax_error(&sf, &file, &source) {
        return CompileResult {
            file: Some(file),
            ok: false,
            output: None,
            elapsed_ms: start.elapsed().as_millis() as u64,
            imports: Vec::new(),
            dynamic_imports: Vec::new(),
            exports: Vec::new(),
            source_map: None,
            error: Some(err),
        };
    }

    let (imports, dynamic_imports, exports) = extract_module_graph(&sf);

    let emit = tsc_rs_emitter::emit(&sf, &options);
    let elapsed_ms = start.elapsed().as_millis() as u64;

    CompileResult {
        file: Some(file),
        ok: true,
        output: Some(emit.javascript),
        elapsed_ms,
        imports,
        dynamic_imports,
        exports,
        source_map: if want_source_map {
            emit.source_map
        } else {
            None
        },
        error: None,
    }
}

// ---------------------------------------------------------------------------
// Option building
// ---------------------------------------------------------------------------

fn build_compiler_options(unit: &CompileUnit) -> CompilerOptions {
    let opts = unit.options.as_ref();

    let target_str = opts
        .and_then(|o| o.target.clone())
        .unwrap_or_else(|| "es2022".to_string());
    let module_str = opts
        .and_then(|o| o.module.clone())
        .or_else(|| unit.module.clone())
        .unwrap_or_else(|| "commonjs".to_string());
    let jsx_str = opts
        .and_then(|o| o.jsx.clone())
        .or_else(|| unit.jsx.clone())
        .unwrap_or_else(|| "react".to_string());

    let mut effective_module = module_str.clone();
    // T6: package.json "type" field detection.
    // For .js files in a "type": "module" context, default to ES modules.
    if let Some(pkg_type) = &unit.pkg_json_type {
        let file_lower = unit.file.as_deref().unwrap_or("").to_ascii_lowercase();
        let is_ambiguous_js = file_lower.ends_with(".js") || file_lower.ends_with(".jsx");
        let caller_unspecified =
            opts.and_then(|o| o.module.as_ref()).is_none() && unit.module.is_none();
        if is_ambiguous_js && caller_unspecified {
            match pkg_type.as_str() {
                "module" => effective_module = "esnext".to_string(),
                "commonjs" => effective_module = "commonjs".to_string(),
                _ => {}
            }
        }
    }

    let mut options = CompilerOptions::default();
    options.target = ScriptTarget::parse(&target_str);
    options.module = ModuleKind::parse(&effective_module);
    options.jsx = JsxEmit::parse(&jsx_str);

    if let Some(o) = opts {
        options.source_map = o.source_map;
        options.import_helpers = o.import_helpers;
        options.remove_comments = o.remove_comments;
        options.use_define_for_class_fields = o.use_define_for_class_fields;
        options.fast_emit = o.fast_emit;
        if let Some(s) = &o.jsx_import_source {
            options.jsx_import_source = Some(s.clone());
        }
        if let Some(f) = &o.jsx_factory {
            options.jsx_factory = Some(f.clone());
        }
    }

    options
}

// ---------------------------------------------------------------------------
// Module graph extraction
// ---------------------------------------------------------------------------

fn extract_module_graph(sf: &SourceFile) -> (Vec<ImportEntry>, Vec<String>, Vec<String>) {
    let mut imports = Vec::new();
    let mut dynamic_imports = Vec::new();
    let mut exports = Vec::new();

    for stmt in &sf.statements {
        collect_from_stmt(stmt, &mut imports, &mut dynamic_imports, &mut exports);
    }

    (imports, dynamic_imports, exports)
}

fn collect_from_stmt(
    stmt: &Stmt,
    imports: &mut Vec<ImportEntry>,
    dynamic: &mut Vec<String>,
    exports: &mut Vec<String>,
) {
    match &stmt.kind {
        StmtKind::Import(imp) => {
            if imp.type_only {
                return;
            }
            let kind = match &imp.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    if namespace.is_some() {
                        "namespace"
                    } else if default.is_some() && named.is_empty() {
                        "default"
                    } else if default.is_some() {
                        "mixed"
                    } else if !named.is_empty() {
                        "named"
                    } else {
                        "side-effect"
                    }
                }
                ImportClause::Require(_) => "require",
            };
            imports.push(ImportEntry {
                specifier: imp.source.clone(),
                kind,
            });
        }
        StmtKind::Export(exp) => match &exp.kind {
            ExportDeclKind::Named {
                specifiers,
                source,
                type_only,
            } => {
                if *type_only {
                    return;
                }
                if let Some(src) = source {
                    imports.push(ImportEntry {
                        specifier: src.clone(),
                        kind: "re-export",
                    });
                }
                for spec in specifiers {
                    if spec.is_type {
                        continue;
                    }
                    let name = spec.exported.clone().unwrap_or_else(|| spec.local.clone());
                    exports.push(name);
                }
            }
            ExportDeclKind::All {
                source,
                alias,
                type_only,
                ..
            } => {
                if *type_only {
                    return;
                }
                imports.push(ImportEntry {
                    specifier: source.clone(),
                    kind: "re-export-all",
                });
                if let Some(a) = alias {
                    exports.push(a.clone());
                }
            }
            ExportDeclKind::Decl(inner) => {
                collect_declared_exports(inner, exports);
                // Nested import/export statements shouldn't happen here,
                // but recurse just in case.
                collect_from_stmt(inner, imports, dynamic, exports);
            }
            ExportDeclKind::Default(expr) => {
                exports.push("default".to_string());
                visit_expr(expr, dynamic);
            }
            ExportDeclKind::DefaultDecl(inner) => {
                exports.push("default".to_string());
                collect_from_stmt(inner, imports, dynamic, exports);
            }
        },
        StmtKind::Expr(e) => visit_expr(e, dynamic),
        StmtKind::Var(v) => {
            for decl in &v.declarations {
                if let Some(init) = &decl.init {
                    visit_expr(init, dynamic);
                }
            }
        }
        StmtKind::Return(Some(e)) | StmtKind::Throw(e) => visit_expr(e, dynamic),
        StmtKind::If(i) => {
            visit_expr(&i.test, dynamic);
            collect_from_stmt(&i.consequent, imports, dynamic, exports);
            if let Some(alt) = &i.alternate {
                collect_from_stmt(alt, imports, dynamic, exports);
            }
        }
        StmtKind::Block(stmts) => {
            for s in stmts {
                collect_from_stmt(s, imports, dynamic, exports);
            }
        }
        StmtKind::While(w) => {
            visit_expr(&w.test, dynamic);
            collect_from_stmt(&w.body, imports, dynamic, exports);
        }
        StmtKind::DoWhile(d) => {
            visit_expr(&d.test, dynamic);
            collect_from_stmt(&d.body, imports, dynamic, exports);
        }
        StmtKind::For(f) => {
            if let Some(test) = &f.test {
                visit_expr(test, dynamic);
            }
            if let Some(update) = &f.update {
                visit_expr(update, dynamic);
            }
            collect_from_stmt(&f.body, imports, dynamic, exports);
        }
        StmtKind::ForIn(f) => {
            visit_expr(&f.right, dynamic);
            collect_from_stmt(&f.body, imports, dynamic, exports);
        }
        StmtKind::ForOf(f) => {
            visit_expr(&f.right, dynamic);
            collect_from_stmt(&f.body, imports, dynamic, exports);
        }
        StmtKind::Try(t) => {
            for s in &t.block {
                collect_from_stmt(s, imports, dynamic, exports);
            }
            if let Some(c) = &t.handler {
                for s in &c.body {
                    collect_from_stmt(s, imports, dynamic, exports);
                }
            }
            if let Some(fin) = &t.finalizer {
                for s in fin {
                    collect_from_stmt(s, imports, dynamic, exports);
                }
            }
        }
        StmtKind::Switch(sw) => {
            visit_expr(&sw.discriminant, dynamic);
            for case in &sw.cases {
                for s in &case.consequent {
                    collect_from_stmt(s, imports, dynamic, exports);
                }
            }
        }
        StmtKind::ExportAssign(e) => {
            exports.push("default".to_string());
            visit_expr(e, dynamic);
        }
        StmtKind::FnDecl(f) => {
            for p in &f.params {
                if let Some(init) = &p.initializer {
                    visit_expr(init, dynamic);
                }
            }
            if let Some(body) = &f.body {
                for s in body {
                    collect_from_stmt(s, imports, dynamic, exports);
                }
            }
        }
        StmtKind::ClassDecl(c) => {
            for m in &c.members {
                walk_class_member(&m.kind, imports, dynamic, exports);
            }
        }
        _ => {}
    }
}

fn walk_class_member(
    kind: &ClassMemberKind,
    imports: &mut Vec<ImportEntry>,
    dynamic: &mut Vec<String>,
    exports: &mut Vec<String>,
) {
    match kind {
        ClassMemberKind::Property(p) => {
            if let Some(init) = &p.initializer {
                visit_expr(init, dynamic);
            }
        }
        ClassMemberKind::Method(m) => {
            if let Some(body) = &m.body {
                for s in body {
                    collect_from_stmt(s, imports, dynamic, exports);
                }
            }
        }
        ClassMemberKind::Constructor(c) => {
            if let Some(body) = &c.body {
                for s in body {
                    collect_from_stmt(s, imports, dynamic, exports);
                }
            }
        }
        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
            if let Some(body) = &a.body {
                for s in body {
                    collect_from_stmt(s, imports, dynamic, exports);
                }
            }
        }
        ClassMemberKind::StaticBlock(stmts) => {
            for s in stmts {
                collect_from_stmt(s, imports, dynamic, exports);
            }
        }
        ClassMemberKind::IndexSignature(_) | ClassMemberKind::SemicolonClassElement => {}
    }
}

fn collect_declared_exports(stmt: &Stmt, exports: &mut Vec<String>) {
    match &stmt.kind {
        StmtKind::FnDecl(f) => {
            if let Some(n) = &f.name {
                if !n.is_empty() {
                    exports.push(n.clone());
                }
            }
        }
        StmtKind::ClassDecl(c) => {
            if let Some(n) = &c.name {
                if !n.is_empty() {
                    exports.push(n.clone());
                }
            }
        }
        StmtKind::Var(v) => {
            for decl in &v.declarations {
                push_pat_names(&decl.name, exports);
            }
        }
        StmtKind::EnumDecl(e) => {
            if !e.name.is_empty() {
                exports.push(e.name.to_string());
            }
        }
        StmtKind::ModuleDecl(m) => {
            // `export namespace Foo {}` — emits the namespace name.
            let name = match &m.name {
                ModuleName::Ident(s) | ModuleName::String(s) => s.clone(),
            };
            if !name.is_empty() {
                exports.push(name);
            }
        }
        StmtKind::TypeAlias(_) | StmtKind::InterfaceDecl(_) => {
            // type-only, not value exports
        }
        _ => {}
    }
}

fn push_pat_names(pat: &Pat, out: &mut Vec<String>) {
    match &pat.kind {
        PatKind::Ident(n) => {
            if !n.is_empty() {
                out.push(n.to_string());
            }
        }
        PatKind::Object(props) => {
            for p in props {
                match p {
                    ObjPatProp::KeyValue(_, inner_pat) => push_pat_names(inner_pat, out),
                    ObjPatProp::Rest(inner) => push_pat_names(inner, out),
                    ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) => {
                        if !name.is_empty() {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
        PatKind::Array(elems) => {
            for e in elems.iter().flatten() {
                match e {
                    ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => push_pat_names(p, out),
                }
            }
        }
        PatKind::Rest(inner) => push_pat_names(inner, out),
        PatKind::Assign(inner, _) => push_pat_names(inner, out),
    }
}

// ---------------------------------------------------------------------------
// Expression walker: finds dynamic `import("...")` calls
// ---------------------------------------------------------------------------

fn visit_expr(expr: &Expr, dynamic: &mut Vec<String>) {
    match &expr.kind {
        ExprKind::Call(call) => {
            if is_import_ident(&call.callee) {
                if let Some(first) = call.args.first() {
                    if let ExprKind::StrLit(s) = &first.kind {
                        dynamic.push(s.to_string());
                    } else if let ExprKind::NoSubstTemplate(s) = &first.kind {
                        dynamic.push(s.to_string());
                    }
                }
            } else {
                visit_expr(&call.callee, dynamic);
            }
            for a in &call.args {
                visit_expr(a, dynamic);
            }
        }
        ExprKind::New(n) => {
            visit_expr(&n.callee, dynamic);
            if let Some(args) = &n.args {
                for a in args {
                    visit_expr(a, dynamic);
                }
            }
        }
        ExprKind::Member(m) => visit_expr(&m.object, dynamic),
        ExprKind::ElemAccess(e) => {
            visit_expr(&e.object, dynamic);
            visit_expr(&e.index, dynamic);
        }
        ExprKind::Binary(b) => {
            visit_expr(&b.left, dynamic);
            visit_expr(&b.right, dynamic);
        }
        ExprKind::Cond(c) => {
            visit_expr(&c.test, dynamic);
            visit_expr(&c.consequent, dynamic);
            visit_expr(&c.alternate, dynamic);
        }
        ExprKind::Unary(u) => visit_expr(&u.argument, dynamic),
        ExprKind::Update(u) => visit_expr(&u.argument, dynamic),
        ExprKind::Paren(p) => visit_expr(p, dynamic),
        ExprKind::As(a) => visit_expr(&a.expr, dynamic),
        ExprKind::Satisfies(s) => visit_expr(&s.expr, dynamic),
        ExprKind::TypeAssertion(t) => visit_expr(&t.expr, dynamic),
        ExprKind::NonNull(e) => visit_expr(e, dynamic),
        ExprKind::Instantiation(i) => visit_expr(&i.expr, dynamic),
        ExprKind::Spread(e) => visit_expr(e, dynamic),
        ExprKind::Await(e) => visit_expr(e, dynamic),
        ExprKind::Delete(e) => visit_expr(e, dynamic),
        ExprKind::Typeof(e) => visit_expr(e, dynamic),
        ExprKind::Void(e) => visit_expr(e, dynamic),
        ExprKind::Yield(_, Some(e)) => visit_expr(e, dynamic),
        ExprKind::Assign(a) => {
            visit_expr(&a.left, dynamic);
            visit_expr(&a.right, dynamic);
        }
        ExprKind::Comma(exprs) => {
            for e in exprs {
                visit_expr(e, dynamic);
            }
        }
        ExprKind::ArrayLit(items) => {
            for item in items.iter().flatten() {
                visit_expr(item, dynamic);
            }
        }
        ExprKind::ObjectLit(props) => {
            for p in props {
                match p {
                    ObjLitProp::Property(op) => visit_expr(&op.value, dynamic),
                    ObjLitProp::Spread(e, _) | ObjLitProp::ShorthandDefault(_, e, _) => {
                        visit_expr(e, dynamic)
                    }
                    ObjLitProp::Method(m) => {
                        for p in &m.params {
                            if let Some(init) = &p.initializer {
                                visit_expr(init, dynamic);
                            }
                        }
                        for s in &m.body {
                            walk_stmt_for_dynamic(s, dynamic);
                        }
                    }
                    ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                        for s in &a.body {
                            walk_stmt_for_dynamic(s, dynamic);
                        }
                    }
                    ObjLitProp::Shorthand(_, _) => {}
                }
            }
        }
        ExprKind::Arrow(a) => {
            for p in &a.params {
                if let Some(init) = &p.initializer {
                    visit_expr(init, dynamic);
                }
            }
            match &a.body {
                ArrowBody::Expr(e) => visit_expr(e, dynamic),
                ArrowBody::Block(stmts) => {
                    for s in stmts {
                        walk_stmt_for_dynamic(s, dynamic);
                    }
                }
            }
        }
        ExprKind::FnExpr(f) => {
            for p in &f.params {
                if let Some(init) = &p.initializer {
                    visit_expr(init, dynamic);
                }
            }
            if let Some(body) = &f.body {
                for s in body {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
        }
        ExprKind::ClassExpr(c) => {
            for m in &c.members {
                walk_class_member_for_dynamic(&m.kind, dynamic);
            }
        }
        _ => {}
    }
}

/// Dynamic-imports-only walker for bodies reached from an expression. Cheaper
/// than the full `collect_from_stmt` since we know no new top-level imports
/// or exports can be introduced inside a function body.
fn walk_stmt_for_dynamic(stmt: &Stmt, dynamic: &mut Vec<String>) {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
            visit_expr(e, dynamic)
        }
        StmtKind::Var(v) => {
            for d in &v.declarations {
                if let Some(init) = &d.init {
                    visit_expr(init, dynamic);
                }
            }
        }
        StmtKind::Return(Some(e)) => visit_expr(e, dynamic),
        StmtKind::If(i) => {
            visit_expr(&i.test, dynamic);
            walk_stmt_for_dynamic(&i.consequent, dynamic);
            if let Some(alt) = &i.alternate {
                walk_stmt_for_dynamic(alt, dynamic);
            }
        }
        StmtKind::While(w) => {
            visit_expr(&w.test, dynamic);
            walk_stmt_for_dynamic(&w.body, dynamic);
        }
        StmtKind::DoWhile(d) => {
            visit_expr(&d.test, dynamic);
            walk_stmt_for_dynamic(&d.body, dynamic);
        }
        StmtKind::For(f) => {
            if let Some(test) = &f.test {
                visit_expr(test, dynamic);
            }
            if let Some(u) = &f.update {
                visit_expr(u, dynamic);
            }
            walk_stmt_for_dynamic(&f.body, dynamic);
        }
        StmtKind::ForIn(fi) => {
            visit_expr(&fi.right, dynamic);
            walk_stmt_for_dynamic(&fi.body, dynamic);
        }
        StmtKind::ForOf(fo) => {
            visit_expr(&fo.right, dynamic);
            walk_stmt_for_dynamic(&fo.body, dynamic);
        }
        StmtKind::Block(ss) => {
            for s in ss {
                walk_stmt_for_dynamic(s, dynamic);
            }
        }
        StmtKind::Try(t) => {
            for s in &t.block {
                walk_stmt_for_dynamic(s, dynamic);
            }
            if let Some(c) = &t.handler {
                for s in &c.body {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
            if let Some(f) = &t.finalizer {
                for s in f {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
        }
        StmtKind::Switch(sw) => {
            visit_expr(&sw.discriminant, dynamic);
            for c in &sw.cases {
                if let Some(t) = &c.test {
                    visit_expr(t, dynamic);
                }
                for s in &c.consequent {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
        }
        StmtKind::Labeled(l) => walk_stmt_for_dynamic(&l.body, dynamic),
        StmtKind::FnDecl(f) => {
            if let Some(body) = &f.body {
                for s in body {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
        }
        StmtKind::ClassDecl(c) => {
            for m in &c.members {
                walk_class_member_for_dynamic(&m.kind, dynamic);
            }
        }
        _ => {}
    }
}

fn walk_class_member_for_dynamic(kind: &ClassMemberKind, dynamic: &mut Vec<String>) {
    match kind {
        ClassMemberKind::Property(p) => {
            if let Some(init) = &p.initializer {
                visit_expr(init, dynamic);
            }
        }
        ClassMemberKind::Method(m) => {
            if let Some(body) = &m.body {
                for s in body {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
        }
        ClassMemberKind::Constructor(c) => {
            if let Some(body) = &c.body {
                for s in body {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
        }
        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
            if let Some(body) = &a.body {
                for s in body {
                    walk_stmt_for_dynamic(s, dynamic);
                }
            }
        }
        ClassMemberKind::StaticBlock(stmts) => {
            for s in stmts {
                walk_stmt_for_dynamic(s, dynamic);
            }
        }
        _ => {}
    }
}

fn is_import_ident(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Ident(name) if name.as_str() == "import")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(file: &str, source: &str) -> CompileUnit {
        CompileUnit {
            file: Some(file.into()),
            source: Some(source.into()),
            ..Default::default()
        }
    }

    #[test]
    fn pipe_extracts_static_imports_with_kinds() {
        let src = r#"
            import React from "react";
            import { a, b } from "./foo";
            import * as ns from "./bar";
            import "./side-effect";
            import Def, { x } from "./mixed";
        "#;
        let r = compile_one(&unit("page.tsx", src));
        assert!(r.ok);
        let kinds: Vec<(&str, &str)> = r
            .imports
            .iter()
            .map(|i| (i.specifier.as_str(), i.kind))
            .collect();
        assert!(kinds.contains(&("react", "default")));
        assert!(kinds.contains(&("./foo", "named")));
        assert!(kinds.contains(&("./bar", "namespace")));
        assert!(kinds.contains(&("./side-effect", "side-effect")));
        assert!(kinds.contains(&("./mixed", "mixed")));
    }

    #[test]
    fn pipe_strips_type_only_imports() {
        let src = r#"
            import type { T } from "./types";
            import { x } from "./value";
        "#;
        let r = compile_one(&unit("a.ts", src));
        let specs: Vec<&str> = r.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(!specs.contains(&"./types"));
        assert!(specs.contains(&"./value"));
    }

    #[test]
    fn pipe_extracts_dynamic_imports() {
        let src = r#"
            async function load() {
                const m = await import("./lazy");
                return m;
            }
            const p = import("./also");
            const arrow = async () => await import("./in-arrow");
            class C {
                async method() { await import("./in-method"); }
                static { import("./in-static-block"); }
            }
        "#;
        let r = compile_one(&unit("a.ts", src));
        let d = &r.dynamic_imports;
        assert!(
            d.contains(&"./also".to_string()),
            "top-level missing: {d:?}"
        );
        assert!(d.contains(&"./lazy".to_string()), "fn body missing: {d:?}");
        assert!(
            d.contains(&"./in-arrow".to_string()),
            "arrow body missing: {d:?}"
        );
        assert!(
            d.contains(&"./in-method".to_string()),
            "method body missing: {d:?}"
        );
        assert!(
            d.contains(&"./in-static-block".to_string()),
            "static block missing: {d:?}"
        );
    }

    #[test]
    fn pipe_extracts_exports() {
        let src = r#"
            export const x = 1;
            export function y() {}
            export class Z {}
            export default function def() {}
            export { a, b as renamed } from "./re";
            export * from "./all";
        "#;
        let r = compile_one(&unit("a.ts", src));
        assert!(r.exports.contains(&"x".to_string()));
        assert!(r.exports.contains(&"y".to_string()));
        assert!(r.exports.contains(&"Z".to_string()));
        assert!(r.exports.contains(&"default".to_string()));
        assert!(r.exports.contains(&"a".to_string()));
        assert!(r.exports.contains(&"renamed".to_string()));
        let specs: Vec<&str> = r.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(specs.contains(&"./re"));
        assert!(specs.contains(&"./all"));
    }

    #[test]
    fn pipe_options_control_source_map() {
        let mut u = unit("a.ts", "export const x = 1;");
        u.options = Some(PipeOptions {
            source_map: Some(true),
            ..Default::default()
        });
        let r = compile_one(&u);
        assert!(r.source_map.is_some(), "sourceMap should be returned");

        let u2 = unit("a.ts", "export const x = 1;");
        let r2 = compile_one(&u2);
        assert!(
            r2.source_map.is_none(),
            "sourceMap should be absent by default"
        );
    }

    #[test]
    fn pipe_options_nested_beats_flat() {
        let mut u = unit("a.tsx", "export const x = <div />;");
        u.jsx = Some("preserve".into());
        u.options = Some(PipeOptions {
            jsx: Some("react".into()),
            ..Default::default()
        });
        let r = compile_one(&u);
        // Nested option should take precedence: `react` emits createElement.
        assert!(r.output.as_ref().unwrap().contains("createElement"));
    }

    #[test]
    fn pipe_pkg_json_type_module_promotes_js_to_esm() {
        let mut u = unit("entry.js", "export const x = 1;");
        u.pkg_json_type = Some("module".into());
        let r = compile_one(&u);
        // ESM keeps `export` rather than rewriting to `exports.x = ...`
        let out = r.output.unwrap();
        assert!(
            out.contains("export const") || out.contains("export {"),
            "expected ESM output under pkg_json_type=module, got:\n{out}"
        );
    }

    #[test]
    fn pipe_import_meta_dirname_cjs() {
        let src = "const d = import.meta.dirname; const f = import.meta.filename; const u = import.meta.url;";
        let u = unit("a.ts", src);
        let r = compile_one(&u);
        let out = r.output.unwrap();
        assert!(out.contains("__dirname"), "expected __dirname, got:\n{out}");
        assert!(out.contains("__filename"));
        assert!(out.contains("pathToFileURL(__filename).href"));
    }

    #[test]
    fn pipe_batch_compile() {
        let batch = vec![
            unit("a.ts", "export const a = 1;"),
            unit("b.ts", "export const b = 2;"),
            unit("c.ts", "export const c = 3;"),
        ];
        let json = handle_batch("b1", batch);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["id"], "b1");
        assert_eq!(v["ok"], true);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        for (i, f) in ["a.ts", "b.ts", "c.ts"].iter().enumerate() {
            assert_eq!(results[i]["file"], *f);
            assert_eq!(results[i]["ok"], true);
        }
    }
}
