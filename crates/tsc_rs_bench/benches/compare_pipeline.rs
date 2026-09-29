//! Head-to-head pipeline comparison: tsc-rs (parse+emit) vs oxc (parse+codegen).
//!
//! Compares the full source-in → JS-out pipeline. This is the most
//! meaningful comparison for end users.

use std::path::Path;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc::{allocator::Allocator, codegen::Codegen, parser::Parser, span::SourceType};
use tsc_rs_ast::CompilerOptions;
use tsc_rs_bench::standard_fixtures;
use tsc_rs_emitter::emit;
use tsc_rs_parser::parse;

fn bench_compare_pipeline(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let tsc_options = CompilerOptions::default();
    let fast_options = CompilerOptions {
        fast_emit: Some(true),
        ..CompilerOptions::default()
    };

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        let mut group = c.benchmark_group(format!("pipeline/{}", fixture.name));
        group.throughput(Throughput::Bytes(bytes));

        // tsc-rs: parse → emit
        group.bench_with_input(
            BenchmarkId::new("tsc-rs", "parse+emit"),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    let output = emit(&sf, &tsc_options);
                    std::hint::black_box(output.javascript.len());
                });
            },
        );

        // tsc-rs FAST: parse → emit with fast_emit (skip cosmetic normalization)
        group.bench_with_input(
            BenchmarkId::new("tsc-rs-fast", "parse+emit"),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    let output = emit(&sf, &fast_options);
                    std::hint::black_box(output.javascript.len());
                });
            },
        );

        // Emit-only groups: parse is identical for both option sets and usually
        // dominates parse+emit, so measure emit in isolation too — this is the
        // number the fast_emit changes actually move.
        let parsed = parse(fixture.file_name, &fixture.source);
        group.bench_function(BenchmarkId::new("tsc-rs", "emit-only"), |b| {
            b.iter(|| {
                let output = emit(&parsed, &tsc_options);
                std::hint::black_box(output.javascript.len());
            });
        });
        group.bench_function(BenchmarkId::new("tsc-rs-fast", "emit-only"), |b| {
            b.iter(|| {
                let output = emit(&parsed, &fast_options);
                std::hint::black_box(output.javascript.len());
            });
        });

        // oxc: parse → codegen (type-strip is implicit — codegen skips TS nodes)
        let path = Path::new(fixture.file_name);
        let source_type = SourceType::from_path(path).unwrap_or_default();
        group.bench_with_input(
            BenchmarkId::new("oxc", "parse+codegen"),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let ret = Parser::new(&allocator, source, source_type).parse();
                    let js = Codegen::new().build(&ret.program).code;
                    std::hint::black_box(js.len());
                });
            },
        );

        group.finish();
    }
}

criterion_group!(benches, bench_compare_pipeline);
criterion_main!(benches);
