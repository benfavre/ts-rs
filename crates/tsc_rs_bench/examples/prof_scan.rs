//! Scanner-only stats + timing: token count, comment count, regex count.
//! Usage: prof_scan <fixture_path> [iters]
use std::env;
use std::fs;
use std::hint::black_box;
use std::time::Instant;
use tsc_rs_scanner::{Scanner, TokenKind};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let source = fs::read_to_string(path).unwrap();

    let mut best = f64::MAX;
    let mut ntok = 0;
    let mut ncom = 0;
    let mut final_tokens = None;
    for iteration in 0..iters {
        let t = Instant::now();
        let (tokens, comments) = Scanner::new(black_box(&source)).scan_all_with_comments();
        let dt = t.elapsed().as_secs_f64();
        best = best.min(dt);
        ntok = tokens.len();
        ncom = comments.len();
        black_box((&tokens, &comments));
        if iteration + 1 == iters {
            // `perf` observes the whole process, not just the `Instant` region.
            // Retain one stream so the reporting pass runs once, not once per scan.
            final_tokens = Some(tokens);
        }
    }
    let final_tokens = final_tokens.unwrap_or_default();
    let nregex = final_tokens
        .iter()
        .filter(|t| t.kind == TokenKind::RegExpLiteral)
        .count();
    println!(
        "tokens={ntok} comments={ncom} regex_literals={nregex}\n\
         scan min = {:.2} ms  ({:.0} MiB/s, {:.1} ns/token)\n\
         regex x comments = {} retain-ops (quadratic risk)",
        best * 1000.0,
        source.len() as f64 / best / 1_048_576.0,
        best * 1e9 / ntok as f64,
        nregex * ncom
    );
}
