//! Emitter throughput benchmarks.
//!
//! Measures JS code generation speed (emitter only, excluding parse time).
//! Compare with: oxc_codegen, swc_ecma_codegen.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tsc_rs_ast::CompilerOptions;
use tsc_rs_bench::standard_fixtures;
use tsc_rs_emitter::emit;
use tsc_rs_parser::parse;

fn bench_emit_default(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("emitter/emit_default");
    let options = CompilerOptions::default();

    for fixture in &fixtures {
        // Pre-parse so we only measure emit time
        let source_file = parse(fixture.file_name, &fixture.source);
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("emit", fixture.name),
            &source_file,
            |b, sf| {
                b.iter(|| {
                    let output = emit(sf, &options);
                    std::hint::black_box(output.javascript.len());
                });
            },
        );
    }

    group.finish();
}

fn bench_emit_with_source_map(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("emitter/emit_sourcemap");
    let options = CompilerOptions {
        source_map: Some(true),
        ..CompilerOptions::default()
    };

    for fixture in &fixtures {
        let source_file = parse(fixture.file_name, &fixture.source);
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("emit+sourcemap", fixture.name),
            &source_file,
            |b, sf| {
                b.iter(|| {
                    let output = emit(sf, &options);
                    std::hint::black_box((
                        output.javascript.len(),
                        output.source_map.as_ref().map(|s| s.len()),
                    ));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_emit_default, bench_emit_with_source_map);
criterion_main!(benches);
