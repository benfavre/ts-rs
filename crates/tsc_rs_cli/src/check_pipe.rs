//! `tsc-rs --check-pipe` — a long-running type-check daemon.
//!
//! Built for editor and agent workflows where the cost model is:
//!
//! - Project init (parse + bind + inject_external_types) happens once
//!   per process: a few seconds on a 5 000-file monorepo.
//! - Each request is a single-file recheck against the cached donor
//!   TypeChecker: typically tens of milliseconds.
//!
//! The protocol is line-delimited JSON on stdin/stdout, mirroring the
//! `--pipe` transpile mode. One JSON request per line, one response per
//! line.
//!
//! ## Protocol
//!
//! Startup banner (written once, after init):
//!
//! ```jsonc
//! {"event":"ready","files":5732,"elapsed_ms":4321}
//! ```
//!
//! Followed by an optional one-time diagnostics line if any project
//! file failed to read:
//!
//! ```jsonc
//! {"event":"init_diagnostics","diagnostics":[…]}
//! ```
//!
//! Requests:
//!
//! ```jsonc
//! {"id":"1","cmd":"check","file":"src/foo.ts"}              // re-read from disk
//! {"id":"1","cmd":"check","file":"src/foo.ts","source":"…"}  // use provided source
//! {"id":"2","cmd":"check_files","files":[
//!   {"file":"a.ts"},
//!   {"file":"b.ts","source":"…"}
//! ]}
//! {"id":"3","cmd":"shutdown"}
//! ```
//!
//! Responses:
//!
//! ```jsonc
//! {"id":"1","ok":true,"elapsed_ms":48,"file":"src/foo.ts","diagnostics":[
//!   {"file":"src/foo.ts","line":117,"col":38,"code":2345,"severity":"error",
//!    "message":"Argument of type 'number' is not assignable to parameter of type 'number[]'."}
//! ]}
//! ```
//!
//! `line` and `col` are 1-based to match the existing CLI output.
//!
//! ## Staleness
//!
//! The donor is built once at startup. Edits to files OTHER than the
//! one being checked are not visible. Restart the daemon to pick up
//! cross-file changes. For most "did I break this file?" agent
//! workflows this is the right trade-off; for "did changing A break
//! B?", request both files in one `check_files` batch.
//!
//! ## Lifetime cap
//!
//! Mirrors the existing `--pipe` cap: `TSC_RS_CHECK_PIPE_MAX_REQUESTS`
//! and `TSC_RS_CHECK_PIPE_MAX_MEMORY_MB` env vars cause the daemon to
//! exit cleanly after the next request, so a supervisor can respawn it.

use std::io::{BufRead, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tsc_rs_ast::{Diagnostic, DiagnosticCategory};
use tsc_rs_project::{CheckSession, TsProject};

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CheckFileSpec {
    file: String,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize, Default)]
struct Request {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    files: Option<Vec<CheckFileSpec>>,
}

#[derive(Serialize)]
struct DiagOut<'a> {
    file: &'a str,
    line: u32,
    col: u32,
    code: u32,
    severity: &'static str,
    message: &'a str,
}

#[derive(Serialize)]
struct FileResult<'a> {
    file: &'a str,
    diagnostics: Vec<DiagOut<'a>>,
}

// ---------------------------------------------------------------------------
// Lifetime guardrails (mirror `pipe.rs`)
// ---------------------------------------------------------------------------

const RSS_CHECK_INTERVAL: usize = 50;

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

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| if v.is_empty() { None } else { Some(v) })
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run_check_pipe(config_path: &str) {
    let max_requests = env_usize("TSC_RS_CHECK_PIPE_MAX_REQUESTS", 10_000);
    let rss_limit_kb: u64 =
        (env_usize("TSC_RS_CHECK_PIPE_MAX_MEMORY_MB", 2_048) as u64).saturating_mul(1024);

    let init_start = std::time::Instant::now();
    let init_timing = std::env::var("TSC_RS_INIT_TIMING").is_ok();

    // Resolve the project (loads tsconfig.json, expands includes, etc.).
    let project = match TsProject::from_config(config_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tsc-rs --check-pipe: cannot load '{}': {}", config_path, e);
            std::process::exit(1);
        }
    };

    if init_timing {
        eprintln!(
            "[init] from_config     {:>7.1}ms",
            init_start.elapsed().as_secs_f64() * 1000.0
        );
    }

    let session = project.open_check_session();

    let init_ms = init_start.elapsed().as_millis() as u64;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let ready = serde_json::json!({
        "event": "ready",
        "files": session.project_file_count(),
        "elapsed_ms": init_ms,
    });
    let _ = writeln!(out, "{}", ready);
    let _ = out.flush();

    let init_diags = session.init_diagnostics();
    if !init_diags.is_empty() {
        let payload = serde_json::json!({
            "event": "init_diagnostics",
            "diagnostics": init_diags
                .iter()
                .map(|d| serialize_diag(d))
                .collect::<Vec<_>>(),
        });
        let _ = writeln!(out, "{}", payload);
        let _ = out.flush();
    }

    let stdin = std::io::stdin();
    let mut request_count: usize = 0;
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let payload = serde_json::json!({
                    "ok": false,
                    "error": format!("invalid request: {}", e),
                });
                let _ = writeln!(out, "{}", payload);
                let _ = out.flush();
                continue;
            }
        };

        let id = req.id.clone().unwrap_or_default();

        if req.cmd.as_deref() == Some("shutdown") {
            let payload = serde_json::json!({
                "id": id,
                "ok": true,
                "shutdown": true,
            });
            let _ = writeln!(out, "{}", payload);
            let _ = out.flush();
            break;
        }

        let response = handle(&session, &id, req);
        let _ = writeln!(out, "{}", response);
        let _ = out.flush();

        request_count = request_count.saturating_add(1);
        let mut should_exit: Option<&'static str> = None;
        if max_requests != 0 && request_count >= max_requests {
            should_exit = Some("max-requests");
        } else if rss_limit_kb != 0 && request_count % RSS_CHECK_INTERVAL == 0 {
            if let Some(rss_kb) = read_rss_kb() {
                if rss_kb >= rss_limit_kb {
                    eprintln!(
                        "tsc-rs --check-pipe: RSS {} kB exceeded ceiling {} kB after {} requests; exiting for respawn",
                        rss_kb, rss_limit_kb, request_count
                    );
                    should_exit = Some("max-memory");
                }
            }
        }
        if let Some(reason) = should_exit {
            let msg = serde_json::json!({
                "ok": true,
                "shutdown": true,
                "reason": reason,
                "requests": request_count,
            });
            let _ = writeln!(out, "{}", msg);
            let _ = out.flush();
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn handle(session: &CheckSession, id: &str, req: Request) -> serde_json::Value {
    let cmd = req.cmd.as_deref().unwrap_or("check");
    match cmd {
        "check" => {
            let file = match req.file.as_deref() {
                Some(f) => f,
                None => {
                    return serde_json::json!({
                        "id": id,
                        "ok": false,
                        "error": "check: missing `file`",
                    });
                }
            };
            let start = std::time::Instant::now();
            let diags = session.check_file(file, req.source.as_deref());
            let elapsed_ms = start.elapsed().as_millis() as u64;
            serde_json::json!({
                "id": id,
                "ok": true,
                "elapsed_ms": elapsed_ms,
                "file": file,
                "diagnostics": diags
                    .iter()
                    .map(|d| serialize_diag(d))
                    .collect::<Vec<_>>(),
            })
        }
        "check_files" => {
            let specs = match req.files {
                Some(v) => v,
                None => {
                    return serde_json::json!({
                        "id": id,
                        "ok": false,
                        "error": "check_files: missing `files`",
                    });
                }
            };
            let inputs: Vec<(String, Option<String>)> =
                specs.into_iter().map(|s| (s.file, s.source)).collect();
            let start = std::time::Instant::now();
            let results = session.check_files(&inputs);
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let files_out: Vec<serde_json::Value> = results
                .iter()
                .map(|(file, diags)| {
                    serde_json::json!({
                        "file": file,
                        "diagnostics": diags
                            .iter()
                            .map(|d| serialize_diag(d))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();
            serde_json::json!({
                "id": id,
                "ok": true,
                "elapsed_ms": elapsed_ms,
                "files": files_out,
            })
        }
        other => serde_json::json!({
            "id": id,
            "ok": false,
            "error": format!("unknown cmd: {}", other),
        }),
    }
}

// ---------------------------------------------------------------------------
// Diagnostic serialization (1-based line/col, matches existing CLI output)
// ---------------------------------------------------------------------------

fn serialize_diag(d: &Diagnostic) -> serde_json::Value {
    let severity = match d.category {
        DiagnosticCategory::Error => "error",
        DiagnosticCategory::Warning => "warning",
        DiagnosticCategory::Suggestion => "suggestion",
        DiagnosticCategory::Message => "message",
    };
    let file = d.file_name.as_deref().unwrap_or("");

    // Compute 1-based line/col from the span start offset. Read the
    // file from disk on demand — daemon use is interactive so the read
    // is a few ms per diagnostic at most.
    let (line, col) = match d.span {
        Some(span) => {
            let mut line: u32 = 1;
            let mut col: u32 = 1;
            if !file.is_empty() {
                if let Ok(src) = std::fs::read_to_string(Path::new(file)) {
                    let bytes = src.as_bytes();
                    let end = (span.start as usize).min(bytes.len());
                    for &b in &bytes[..end] {
                        if b == b'\n' {
                            line += 1;
                            col = 1;
                        } else {
                            col += 1;
                        }
                    }
                }
            }
            (line, col)
        }
        None => (1, 1),
    };
    serde_json::json!({
        "file": file,
        "line": line,
        "col": col,
        "code": d.code,
        "severity": severity,
        "message": d.message,
    })
}
