//! Exact-size allocation histogram for one parse().
//!
//! Distinguishes single boxed nodes from Vec buffers: a `Box<Expr>` is exactly 40
//! bytes, while a `Vec<Expr>`'s first buffer is 4 * 40 = 160 (Rust's minimum
//! non-zero capacity for elements this size), then 320, 640...
//!
//! Usage: prof_sizes <fixture_path>
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use tsc_rs_parser::parse;

const MAX: usize = 4096;
static COUNTS: [AtomicUsize; MAX] = [const { AtomicUsize::new(0) }; MAX];
static BIG: AtomicUsize = AtomicUsize::new(0);
static ON: AtomicUsize = AtomicUsize::new(0);

struct C;
unsafe impl GlobalAlloc for C {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ON.load(Ordering::Relaxed) == 1 {
            if l.size() < MAX {
                COUNTS[l.size()].fetch_add(1, Ordering::Relaxed);
            } else {
                BIG.fetch_add(1, Ordering::Relaxed);
            }
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: C = C;

fn main() {
    let path = env::args().nth(1).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    let file_name = path.rsplit('/').next().unwrap();

    ON.store(1, Ordering::Relaxed);
    let sf = parse(file_name, &source);
    ON.store(0, Ordering::Relaxed);
    black_box(&sf);

    let mut by_size: BTreeMap<usize, usize> = BTreeMap::new();
    let mut total = 0usize;
    let mut bytes = 0usize;
    for (size, c) in COUNTS.iter().enumerate() {
        let n = c.load(Ordering::Relaxed);
        if n > 0 {
            by_size.insert(size, n);
            total += n;
            bytes += n * size;
        }
    }
    let big = BIG.load(Ordering::Relaxed);

    // These were hardcoded to 40, so they kept reporting 40 no matter what the AST
    // actually did — which quietly contradicted the allocation histogram below.
    println!(
        "Expr={} Stmt={} bytes",
        std::mem::size_of::<tsc_rs_ast::Expr>(),
        std::mem::size_of::<tsc_rs_ast::Stmt>()
    );
    println!(
        "total allocations: {total} (+{big} over {MAX}B)   {:.1} MB\n",
        bytes as f64 / 1e6
    );

    let mut top: Vec<_> = by_size.iter().map(|(s, n)| (*n, *s)).collect();
    top.sort_unstable_by(|a, b| b.0.cmp(&a.0));

    println!(
        "{:>8}  {:>10}  {:>7}  {:>8}",
        "size", "count", "% allocs", "MB"
    );
    for (n, size) in top.iter().take(16) {
        println!(
            "{:>8}  {:>10}  {:>7.1}%  {:>8.1}",
            size,
            n,
            *n as f64 / total as f64 * 100.0,
            (n * size) as f64 / 1e6
        );
    }

    // Roll up: single nodes (<=64B, i.e. one Box) vs Vec buffers (>=128B, multi-element)
    let singles: usize = by_size
        .iter()
        .filter(|(s, _)| **s <= 64)
        .map(|(_, n)| *n)
        .sum();
    let buffers: usize = by_size
        .iter()
        .filter(|(s, _)| **s > 64)
        .map(|(_, n)| *n)
        .sum();
    println!(
        "\nrollup:  <=64B (single boxed nodes) {:>8} ({:.0}%)   >64B (Vec buffers) {:>8} ({:.0}%)",
        singles,
        singles as f64 / total as f64 * 100.0,
        buffers,
        buffers as f64 / total as f64 * 100.0
    );
    drop(sf);
}
