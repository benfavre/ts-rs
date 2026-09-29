//! Clean transient-vs-retained measurement, windowed around ONE parse() call
//! with the resulting SourceFile kept alive.
//!
//!   retained = memory the final AST actually needs
//!   transient = allocated AND freed *during* the parse = speculative/backtracking waste
//!
//! Usage: prof_live <fixture_path>
use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::fs;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use tsc_rs_parser::parse;

static A_N: AtomicUsize = AtomicUsize::new(0);
static A_B: AtomicUsize = AtomicUsize::new(0);
static F_N: AtomicUsize = AtomicUsize::new(0);
static F_B: AtomicUsize = AtomicUsize::new(0);

struct C;
unsafe impl GlobalAlloc for C {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        A_N.fetch_add(1, Ordering::Relaxed);
        A_B.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        F_N.fetch_add(1, Ordering::Relaxed);
        F_B.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // A realloc is an alloc of `new` + a free of `l.size()`.
        A_N.fetch_add(1, Ordering::Relaxed);
        A_B.fetch_add(new, Ordering::Relaxed);
        F_N.fetch_add(1, Ordering::Relaxed);
        F_B.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static GLOBAL: C = C;

fn snap() -> (usize, usize, usize, usize) {
    (
        A_N.load(Ordering::Relaxed),
        A_B.load(Ordering::Relaxed),
        F_N.load(Ordering::Relaxed),
        F_B.load(Ordering::Relaxed),
    )
}

fn main() {
    let path = env::args().nth(1).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    let file_name = path.rsplit('/').next().unwrap();

    let before = snap();
    let sf = parse(file_name, &source); // kept alive across the measurement
    let after = snap();
    black_box(&sf);

    let alloc_n = after.0 - before.0;
    let alloc_b = after.1 - before.1;
    let free_n = after.2 - before.2;
    let free_b = after.3 - before.3;

    println!("source                {:>10} bytes", source.len());
    println!();
    println!("during one parse():");
    println!(
        "  allocated           {:>10} allocs   {:>7.1} MB",
        alloc_n,
        alloc_b as f64 / 1e6
    );
    println!(
        "  freed (transient)   {:>10} allocs   {:>7.1} MB",
        free_n,
        free_b as f64 / 1e6
    );
    println!(
        "  retained (live AST) {:>10} allocs   {:>7.1} MB",
        alloc_n - free_n,
        (alloc_b - free_b) as f64 / 1e6
    );
    println!();
    println!(
        "  => {:.0}% of allocations and {:.0}% of bytes are TRANSIENT",
        free_n as f64 / alloc_n as f64 * 100.0,
        free_b as f64 / alloc_b as f64 * 100.0
    );
    println!(
        "  => AST blowup: {:.1}x source size",
        (alloc_b - free_b) as f64 / source.len() as f64
    );
    drop(sf);
}
