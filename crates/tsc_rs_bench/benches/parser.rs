//! Parser throughput benchmarks.
//!
//! Measures full parse (scan + parse) speed across different file sizes.
//! Compare with: oxc_parser, swc_ecma_parser, biome_js_parser.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tsc_rs_bench::standard_fixtures;
use tsc_rs_parser::parse;

fn bench_parse(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("parser/parse");

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("full_parse", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    std::hint::black_box(sf.statements.len());
                });
            },
        );
    }

    group.finish();
}

/// Parse without measuring AST drop time.
/// This isolates parse cost from deallocation cost,
/// matching oxc's "no-drop" benchmark methodology.
fn bench_parse_no_drop(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("parser/parse_no_drop");

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("parse_no_drop", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter_with_large_drop(|| parse(fixture.file_name, source));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_parse, bench_parse_no_drop);
criterion_main!(benches);
