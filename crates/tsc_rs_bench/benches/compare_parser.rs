//! Head-to-head parser comparison: tsc-rs vs oxc.
//!
//! Uses the same fixture files as oxc's own benchmarks for direct comparison.
//! Results are grouped per-fixture so you see tsc-rs vs oxc side by side.

use std::path::Path;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc::{allocator::Allocator, parser::Parser, span::SourceType};
use tsc_rs_bench::standard_fixtures;
use tsc_rs_parser::parse;

fn bench_compare_parse(c: &mut Criterion) {
    let fixtures = standard_fixtures();

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        let mut group = c.benchmark_group(format!("parse/{}", fixture.name));
        group.throughput(Throughput::Bytes(bytes));

        // tsc-rs parser
        group.bench_with_input(
            BenchmarkId::new("tsc-rs", "single-thread"),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    std::hint::black_box(sf.statements.len());
                });
            },
        );

        // tsc-rs parser (no-drop — exclude AST deallocation)
        group.bench_with_input(
            BenchmarkId::new("tsc-rs", "no-drop"),
            &fixture.source,
            |b, source| {
                b.iter_with_large_drop(|| parse(fixture.file_name, source));
            },
        );

        // oxc parser
        let path = Path::new(fixture.file_name);
        let source_type = SourceType::from_path(path).unwrap_or_default();
        group.bench_with_input(
            BenchmarkId::new("oxc", "single-thread"),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let ret = Parser::new(&allocator, source, source_type).parse();
                    std::hint::black_box(ret.program.body.len());
                    // allocator dropped here — included in measurement like tsc-rs
                });
            },
        );

        // oxc parser (no-drop — exclude arena deallocation)
        group.bench_with_input(
            BenchmarkId::new("oxc", "no-drop"),
            &fixture.source,
            |b, source| {
                b.iter_with_large_drop(|| {
                    let allocator = Allocator::default();
                    let _ = Parser::new(&allocator, source, source_type).parse();
                    allocator // return allocator so drop is excluded
                });
            },
        );

        group.finish();
    }
}

criterion_group!(benches, bench_compare_parse);
criterion_main!(benches);
