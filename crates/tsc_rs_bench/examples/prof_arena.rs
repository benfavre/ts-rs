//! MEASUREMENT ONLY — not a production allocator.
//!
//! Bump allocator over ONE fixed pre-reserved region (so reset-to-mark is sound).
//! alloc = pointer bump, dealloc = no-op, reset = O(1).
//!
//! This simulates the ceiling of an arena-allocated AST *without* touching any
//! AST type, and answers: how much of parse time is malloc overhead?
//!
//! Region is pre-faulted before timing so we measure allocator cost, not
//! first-touch page faults. Aborts if the region is exhausted, so a bad
//! measurement can never silently masquerade as a good one.
//!
//! Usage: prof_arena <fixture_path> [iters]
use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::fs;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tsc_rs_parser::parse;

const REGION: usize = 3 << 30; // 3 GB, one shot, never grown

struct Bump {
    base: AtomicUsize,
    cur: AtomicUsize,
}

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let mut base = self.base.load(Ordering::Relaxed);
        if base == 0 {
            // First use: reserve the single region.
            base = unsafe { System.alloc(Layout::from_size_align(REGION, 4096).unwrap()) } as usize;
            assert!(base != 0, "region reserve failed");
            self.base.store(base, Ordering::Relaxed);
            self.cur.store(base, Ordering::Relaxed);
        }
        let align = l.align().max(16);
        let cur = self.cur.load(Ordering::Relaxed);
        let aligned = (cur + align - 1) & !(align - 1);
        let next = aligned + l.size();
        if next > base + REGION {
            // Never silently fall back — that would corrupt the measurement.
            eprintln!("BUMP REGION EXHAUSTED — measurement invalid");
            std::process::abort();
        }
        self.cur.store(next, Ordering::Relaxed);
        aligned as *mut u8
    }
    unsafe fn dealloc(&self, _p: *mut u8, _l: Layout) {}
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let np = unsafe { self.alloc(Layout::from_size_align_unchecked(new, l.align())) };
        unsafe { std::ptr::copy_nonoverlapping(p, np, l.size().min(new)) };
        np
    }
}

#[global_allocator]
static GLOBAL: Bump = Bump {
    base: AtomicUsize::new(0),
    cur: AtomicUsize::new(0),
};

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    let source = fs::read_to_string(path).unwrap();
    let file_name = path.rsplit('/').next().unwrap();

    // Pre-fault the whole region so page-fault cost is not attributed to parsing.
    // (mimalloc's baseline reuses already-warm pages, so this keeps it apples-to-apples.)
    let base = GLOBAL.base.load(Ordering::Relaxed);
    assert!(base != 0);
    unsafe {
        let p = base as *mut u8;
        let mut i = 0;
        while i < REGION {
            std::ptr::write_volatile(p.add(i), 0);
            i += 4096;
        }
    }

    // Mark AFTER source/argv are allocated: reset reclaims only parse memory.
    let mark = GLOBAL.cur.load(Ordering::Relaxed);

    for _ in 0..2 {
        GLOBAL.cur.store(mark, Ordering::Relaxed);
        std::mem::forget(parse(file_name, &source));
    }

    let mut best = f64::MAX;
    let mut used = 0usize;
    for _ in 0..iters {
        GLOBAL.cur.store(mark, Ordering::Relaxed); // O(1) arena reset, outside timing
        let t = Instant::now();
        let sf = parse(black_box(file_name), black_box(&source));
        best = best.min(t.elapsed().as_secs_f64());
        black_box(&sf);
        used = GLOBAL.cur.load(Ordering::Relaxed) - mark;
        std::mem::forget(sf); // never drop against reset memory
    }
    println!(
        "BUMP/ARENA parse min = {:.2} ms   ({:.0} MiB/s)   arena bytes used = {:.1} MB",
        best * 1000.0,
        source.len() as f64 / best / 1_048_576.0,
        used as f64 / 1e6
    );
}
