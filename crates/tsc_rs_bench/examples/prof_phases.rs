//! Phase/allocation breakdown for parse: how much is lexing vs AST building,
//! and how many heap allocations does each phase make?
//! Usage: prof_phases <fixture_path> [iters]
use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::fs;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tsc_rs_parser::parse;
use tsc_rs_scanner::Scanner;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
static FREED_BYTES: AtomicUsize = AtomicUsize::new(0);
/// Histogram by power-of-two size class (index = bucket, 0 => <=8 bytes).
static SIZES: [AtomicUsize; 16] = [const { AtomicUsize::new(0) }; 16];

fn bucket(n: usize) -> usize {
    (usize::BITS - n.max(1).leading_zeros()) as usize / 1
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size(), Ordering::Relaxed);
        SIZES[bucket(l.size()).min(15)].fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        FREED.fetch_add(1, Ordering::Relaxed);
        FREED_BYTES.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new.saturating_sub(l.size()), Ordering::Relaxed);
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn snap() -> (usize, usize) {
    (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
    )
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    let source = fs::read_to_string(path).unwrap();
    let file_name = path.rsplit('/').next().unwrap();

    // --- Phase 1: scan only ---
    let mut scan_best = f64::MAX;
    let mut ntokens = 0;
    let (a0, b0) = snap();
    for _ in 0..iters {
        let t = Instant::now();
        let (tokens, comments) = Scanner::new(black_box(&source)).scan_all_with_comments();
        let dt = t.elapsed().as_secs_f64();
        scan_best = scan_best.min(dt);
        ntokens = tokens.len();
        black_box((&tokens, &comments));
    }
    let (a1, b1) = snap();

    // --- Phase 2: full parse (scan + AST build) ---
    let mut parse_best = f64::MAX;
    for _ in 0..iters {
        let t = Instant::now();
        let sf = parse(black_box(file_name), black_box(&source));
        let dt = t.elapsed().as_secs_f64();
        parse_best = parse_best.min(dt);
        black_box(&sf);
    }
    let (a2, b2) = snap();

    let scan_allocs = (a1 - a0) / iters;
    let scan_bytes = (b1 - b0) / iters;
    let parse_allocs = (a2 - a1) / iters;
    let parse_bytes = (b2 - b1) / iters;

    println!("source            {:>12} bytes", source.len());
    println!("tokens            {:>12}", ntokens);
    println!();
    println!("scan  (min)       {:>9.2} ms", scan_best * 1000.0);
    println!("parse (min, incl. scan) {:>3.2} ms", parse_best * 1000.0);
    println!(
        "  => AST build     {:>9.2} ms  ({:.0}% of parse)",
        (parse_best - scan_best) * 1000.0,
        (parse_best - scan_best) / parse_best * 100.0
    );
    println!();
    println!(
        "scan  allocations {:>12}  ({:.1} MB)",
        scan_allocs,
        scan_bytes as f64 / 1e6
    );
    println!(
        "parse allocations {:>12}  ({:.1} MB)",
        parse_allocs,
        parse_bytes as f64 / 1e6
    );
    println!(
        "  => AST allocs    {:>12}  ({:.1} MB)  = {:.1} allocs per token",
        parse_allocs.saturating_sub(scan_allocs),
        (parse_bytes.saturating_sub(scan_bytes)) as f64 / 1e6,
        parse_allocs.saturating_sub(scan_allocs) as f64 / ntokens as f64
    );

    // Transient vs retained: how much of what we allocate during ONE parse is
    // freed again before the parse even finishes? That is pure churn.
    let (fa, fb) = (
        FREED.load(Ordering::Relaxed),
        FREED_BYTES.load(Ordering::Relaxed),
    );
    println!();
    println!("--- churn during parse (per iteration) ---");
    println!(
        "  total allocated  {:>10} allocs  {:>8.1} MB",
        parse_allocs,
        parse_bytes as f64 / 1e6
    );
    println!(
        "  freed DURING run {:>10} allocs  {:>8.1} MB",
        fa / (iters * 2),
        fb as f64 / (iters * 2) as f64 / 1e6
    );
    println!("  (freed/allocated = transient churn ratio)");

    println!("\nallocation size histogram (all phases, per iteration):");
    let total: usize = SIZES.iter().map(|s| s.load(Ordering::Relaxed)).sum();
    for (i, s) in SIZES.iter().enumerate() {
        let n = s.load(Ordering::Relaxed);
        if n == 0 {
            continue;
        }
        let lo = if i == 0 { 0 } else { 1usize << (i - 1) };
        let hi = 1usize << i;
        println!(
            "  {:>6}..{:<7} {:>10}  {:>5.1}%",
            lo,
            hi,
            n / (iters * 2),
            n as f64 / total as f64 * 100.0
        );
    }
}
