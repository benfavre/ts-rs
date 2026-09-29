//! Time checking a pre-parsed and bound file, without standard-library loading.
//! Usage: cargo run --profile perf -p tsc_rs_harness --example profile_check -- file.ts [iterations]
use std::{env, fs, hint::black_box, time::Instant};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let mut args = env::args().skip(1);
    let path = args.next().expect("expected a TypeScript source path");
    let iterations: usize = args
        .next()
        .map(|value| value.parse().expect("iterations must be an integer"))
        .unwrap_or(100);
    assert!(iterations > 0, "iterations must be positive");
    let source = fs::read_to_string(&path).expect("failed to read source");
    let file = tsc_rs_parser::parse(&path, &source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = tsc_rs_ast::CompilerOptions::default();
    let check = || {
        tsc_rs_types::TypeChecker::new().check_with_options(
            black_box(&file),
            black_box(&symbols),
            black_box(&options),
        )
    };
    let expected = check().diagnostics.len();
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        let output = check();
        assert_eq!(black_box(&output).diagnostics.len(), expected);
        drop(output);
        samples.push(start.elapsed().as_secs_f64());
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "bytes={} iterations={iterations} diagnostics={expected} median_ms={:.6} min_ms={:.6}",
        source.len(),
        samples[iterations / 2] * 1000.0,
        samples[0] * 1000.0,
    );
}
