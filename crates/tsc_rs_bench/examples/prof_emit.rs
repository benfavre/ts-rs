//! Profile emission or parse+emit with CLI-equivalent allocation and precise timing.
//! Usage: prof_emit <fixture> [iterations] [normal|fast|es5|maps] [emit|pipeline] [automatic|classic]
//! Parsing, binding, and type checking are distinct; this harness does not type-check.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::time::Instant;
use tsc_rs_ast::{CompilerOptions, JsxEmit, ScriptTarget};
use tsc_rs_emitter::emit;
use tsc_rs_parser::parse;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: prof_emit <fixture> [iterations] [normal|fast|es5|maps] [emit|pipeline] [automatic|classic]");
    let iterations: u32 = args
        .get(2)
        .map(|s| s.parse().expect("invalid iteration count"))
        .unwrap_or(20);
    assert!(iterations > 0, "iterations must be positive");
    let preset = args.get(3).map(String::as_str).unwrap_or("normal");
    assert!(
        matches!(preset, "normal" | "fast" | "es5" | "maps"),
        "invalid preset"
    );
    let mode = args.get(4).map(String::as_str).unwrap_or("emit");
    assert!(matches!(mode, "emit" | "pipeline"), "invalid mode");
    let jsx_mode = args.get(5).map(String::as_str).unwrap_or("automatic");
    assert!(
        matches!(jsx_mode, "automatic" | "classic"),
        "invalid JSX mode"
    );
    let source = std::fs::read_to_string(path).expect("cannot read fixture");
    let file_name = std::path::Path::new(path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let options = CompilerOptions {
        target: Some(if preset == "es5" {
            ScriptTarget::ES5
        } else {
            ScriptTarget::ES2022
        }),
        fast_emit: Some(preset == "fast"),
        source_map: Some(preset == "maps"),
        jsx: file_name
            .ends_with(".tsx")
            .then_some(if jsx_mode == "classic" {
                JsxEmit::React
            } else {
                JsxEmit::ReactJSX
            }),
        jsx_import_source: Some("react".to_string()),
        ..Default::default()
    };
    let parsed = parse(file_name, &source);
    let diagnostics = parsed.diagnostics.len();
    let initial = emit(&parsed, &options);
    let output_bytes = initial.javascript.len();
    let mut fingerprint = DefaultHasher::new();
    initial.javascript.hash(&mut fingerprint);
    initial.source_map.hash(&mut fingerprint);
    initial.declaration_file.hash(&mut fingerprint);
    let output_hash = fingerprint.finish();
    drop(initial);
    let parsed = if mode == "pipeline" {
        drop(parsed);
        None
    } else {
        Some(parsed)
    };
    let operation = || match &parsed {
        Some(parsed) => emit(black_box(parsed), black_box(&options)),
        None => {
            let parsed = parse(black_box(file_name), black_box(&source));
            emit(&parsed, black_box(&options))
        }
    };
    for _ in 0..3 {
        black_box(operation());
    }
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(operation());
    }
    let elapsed_ns = start.elapsed().as_nanos();
    println!("{{\"iterations\":{iterations},\"jsx_mode\":\"{jsx_mode}\",\"source_bytes\":{},\"parse_diagnostics\":{diagnostics},\"elapsed_ns\":{elapsed_ns},\"per_iter_ns\":{},\"output_bytes\":{output_bytes},\"output_hash\":{output_hash}}}", source.len(), elapsed_ns / u128::from(iterations));
}
