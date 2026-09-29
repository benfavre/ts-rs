//! Profiling harness: parse once, then fast-emit a fixture in a tight loop.
//! Usage: prof_fast <fixture_path> <iters>
use std::env;
use std::fs;
use tsc_rs_ast::{CompilerOptions, JsxEmit, ScriptTarget};
use tsc_rs_emitter::emit;
use tsc_rs_parser::parse;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
    let source = fs::read_to_string(path).unwrap();
    let file_name = path.rsplit('/').next().unwrap();
    // Match the real fast-emit setup: .tsx transpiles via the automatic runtime.
    let jsx = if file_name.ends_with(".tsx") {
        Some(JsxEmit::ReactJSX)
    } else {
        None
    };
    let opts = CompilerOptions {
        fast_emit: Some(true),
        target: Some(ScriptTarget::ES2022),
        jsx,
        jsx_import_source: Some("react".to_string()),
        ..CompilerOptions::default()
    };
    let sf = parse(file_name, &source);
    // A non-empty diagnostics list flips file_has_recovery_errors on, which
    // disables the emitter's recovery fast-outs — so surface it while profiling.
    if !sf.diagnostics.is_empty() {
        eprintln!(
            "WARNING: {} parse diagnostic(s) — recovery fast-outs disabled",
            sf.diagnostics.len()
        );
    }
    let mut total = 0usize;
    let start = std::time::Instant::now();
    for _ in 0..iters {
        let out = emit(&sf, &opts);
        total = total.wrapping_add(out.javascript.len());
    }
    let dur = start.elapsed();
    eprintln!(
        "iters={} bytes_in={} total_out={} elapsed={:?} per_iter={:?} thrpt={:.1} MiB/s",
        iters,
        source.len(),
        total,
        dur,
        dur / iters as u32,
        (source.len() as f64 * iters as f64) / dur.as_secs_f64() / 1_048_576.0
    );
}
