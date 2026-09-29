//! Head-to-head instruction/cycle accounting: oxc's parser vs ts-rs's, same file,
//! same process shape, so `perf stat` numbers are directly comparable.
//!
//! Usage: prof_oxc <fixture_path> [iters] [oxc|tsrs]
//!
//! Run each mode under `perf stat -e instructions,cycles` and compare. This tells
//! us WHY oxc wins: fewer instructions (does less work) or higher IPC (better data
//! layout / fewer stalls). The answer picks the refactor.
use std::env;
use std::fs;
use std::hint::black_box;
use std::time::Instant;

use oxc::allocator::Allocator;
use oxc::parser::Parser as OxcParser;
use oxc::span::SourceType;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    let mode = args.get(3).map(|s| s.as_str()).unwrap_or("oxc");
    let source = fs::read_to_string(path).unwrap();
    let file_name = path.rsplit('/').next().unwrap();
    let st = SourceType::from_path(path).unwrap();

    let mut best = f64::MAX;
    for _ in 0..iters {
        match mode {
            "oxc" => {
                let alloc = Allocator::default();
                let t = Instant::now();
                let ret = OxcParser::new(&alloc, black_box(&source), st).parse();
                best = best.min(t.elapsed().as_secs_f64());
                // How many bytes of arena did oxc's AST actually need? This is the
                // apples-to-apples counterpart to our 64 MB of Box'd nodes.
                eprintln!(
                    "  oxc arena used = {:.1} MB",
                    alloc.used_bytes() as f64 / 1e6
                );
                black_box(&ret);
            }
            // Drop is OUTSIDE the benchmark's timed region (it captures dt before the
            // AST falls out of scope), but perf still samples it — and dropping 875k
            // nodes is a huge share of our misses. `nodrop` leaks the AST instead, so
            // the counters reflect only the work the benchmark actually times.
            "tsrs-nodrop" => {
                let t = Instant::now();
                let sf = tsc_rs_parser::parse(black_box(file_name), black_box(&source));
                best = best.min(t.elapsed().as_secs_f64());
                black_box(&sf);
                std::mem::forget(sf);
            }
            _ => {
                let t = Instant::now();
                let sf = tsc_rs_parser::parse(black_box(file_name), black_box(&source));
                best = best.min(t.elapsed().as_secs_f64());
                black_box(&sf);
            }
        }
    }
    eprintln!(
        "{mode:>5}  min = {:.2} ms   ({:.0} MiB/s)",
        best * 1000.0,
        source.len() as f64 / best / 1_048_576.0
    );
}
