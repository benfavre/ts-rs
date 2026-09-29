//! Profiling harness: parse a fixture in a tight loop.
//! Usage: prof_parse <fixture_path> [iters]
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use tsc_rs_parser::parse;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
    assert!(iters > 0, "iterations must be positive");
    let source = fs::read_to_string(path).unwrap();
    let file_name = path.rsplit('/').next().unwrap();

    // Warmup
    for _ in 0..5 {
        black_box(parse(file_name, &source));
    }

    let mut best = f64::MAX;
    let mut total_stmts = 0usize;
    let start = std::time::Instant::now();
    for _ in 0..iters {
        let t = std::time::Instant::now();
        let sf = parse(black_box(file_name), black_box(&source));
        let dt = t.elapsed().as_secs_f64();
        if dt < best {
            best = dt;
        }
        total_stmts = total_stmts.wrapping_add(sf.statements.len());
        black_box(&sf);
    }
    let dur = start.elapsed();
    let avg = dur.as_secs_f64() / iters as f64;

    // Fingerprint the complete parsed result outside the timed loop so paired
    // measurements can detect changes to the AST or diagnostics. Do this after
    // timing so fingerprint allocations cannot perturb the measured parses.
    let parsed = parse(file_name, &source);
    let parse_diagnostics = parsed.diagnostics.len();
    let mut fingerprint = DefaultHasher::new();
    format!("{parsed:?}").hash(&mut fingerprint);
    let ast_hash = fingerprint.finish();
    drop(parsed);

    println!(
        "{{\"iterations\":{iters},\"source_bytes\":{},\"parse_diagnostics\":{parse_diagnostics},\"ast_hash\":{ast_hash},\"elapsed_ns\":{},\"per_iter_ns\":{},\"statements\":{total_stmts}}}",
        source.len(), dur.as_nanos(), dur.as_nanos() / iters as u128,
    );
    eprintln!(
        "iters={} bytes={} stmts={} avg={:.2} ms  min={:.2} ms  thrpt={:.1} MiB/s (avg)",
        iters,
        source.len(),
        total_stmts,
        avg * 1000.0,
        best * 1000.0,
        (source.len() as f64) / avg / 1_048_576.0
    );
}
