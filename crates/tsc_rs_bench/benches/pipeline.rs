//! Full pipeline benchmarks (scan → parse → emit).
//!
//! Measures end-to-end TypeScript-to-JavaScript compilation speed.
//! This is the metric most comparable to oxc's transformer benchmarks
//! and esbuild/swc transform benchmarks.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tsc_rs_ast::{CompilerOptions, ModuleKind, ScriptTarget};
use tsc_rs_bench::standard_fixtures;
use tsc_rs_emitter::emit;
use tsc_rs_parser::parse;

/// Full pipeline: parse + emit with default options.
fn bench_pipeline_default(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("pipeline/default");
    let options = CompilerOptions::default();

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("parse+emit", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    let output = emit(&sf, &options);
                    std::hint::black_box(output.javascript.len());
                });
            },
        );
    }

    group.finish();
}

/// Full pipeline targeting ES5 (requires more downleveling).
fn bench_pipeline_es5(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("pipeline/es5");
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5),
        ..CompilerOptions::default()
    };

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("parse+emit_es5", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    let output = emit(&sf, &options);
                    std::hint::black_box(output.javascript.len());
                });
            },
        );
    }

    group.finish();
}

/// Full pipeline with CommonJS module output.
fn bench_pipeline_commonjs(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("pipeline/commonjs");
    let options = CompilerOptions {
        module: Some(ModuleKind::CommonJS),
        ..CompilerOptions::default()
    };

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("parse+emit_cjs", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    let output = emit(&sf, &options);
                    std::hint::black_box(output.javascript.len());
                });
            },
        );
    }

    group.finish();
}

/// Full pipeline with source maps enabled.
fn bench_pipeline_sourcemap(c: &mut Criterion) {
    let fixtures = standard_fixtures();
    let mut group = c.benchmark_group("pipeline/sourcemap");
    let options = CompilerOptions {
        source_map: Some(true),
        ..CompilerOptions::default()
    };

    for fixture in &fixtures {
        let bytes = fixture.source.len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("parse+emit+map", fixture.name),
            &fixture.source,
            |b, source| {
                b.iter(|| {
                    let sf = parse(fixture.file_name, source);
                    let output = emit(&sf, &options);
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

criterion_group!(
    benches,
    bench_pipeline_default,
    bench_pipeline_es5,
    bench_pipeline_commonjs,
    bench_pipeline_sourcemap,
);
criterion_main!(benches);
