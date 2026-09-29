//! Scanner/lexer throughput benchmarks.
//!
//! Measures raw tokenization speed across different file sizes.
//! Compare with: oxc lexer, swc lexer, biome lexer.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tsc_rs_bench::standard_fixtures;
use tsc_rs_scanner::Scanner;

fn bench_scan_all(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("scanner/scan_all");

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("tokens", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let scanner = Scanner::new(source);
                    let tokens = scanner.scan_all();
                    std::hint::black_box(tokens.len());
                });
            },
        );
    }

    group.finish();
}

fn bench_scan_with_comments(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("scanner/scan_with_comments");

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("tokens+comments", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let scanner = Scanner::new(source);
                    let (tokens, comments) = scanner.scan_all_with_comments();
                    std::hint::black_box((tokens.len(), comments.len()));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_scan_all, bench_scan_with_comments);
criterion_main!(benches);
