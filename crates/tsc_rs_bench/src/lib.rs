//! Benchmark utilities for tsc-rs.
//!
//! Provides fixture loading and shared helpers for criterion benchmarks.
//!
//! Set `TSC_BENCH_LARGE=1` to include the huge fixtures (typescript.js 8MB,
//! cal.com.tsx 1MB). These are excluded by default to avoid OOM on small VMs.

use std::path::{Path, PathBuf};

/// Root of the ts-rs repository (two levels up from this crate).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Load a fixture file from the fixtures/ directory.
pub fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to load fixture {}: {}\nRun: make bench-fixtures",
            name, e
        )
    })
}

/// Load a test case from the test suite.
pub fn load_test_case(suite: &str, name: &str) -> String {
    let root = repo_root();
    for ext in &["ts", "tsx"] {
        let path = root
            .join("tests/cases")
            .join(suite)
            .join(format!("{}.{}", name, ext));
        if path.exists() {
            return std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to load {}: {}", path.display(), e));
        }
    }
    for entry in walkdir(root.join("tests/cases").join(suite)) {
        let fname = entry.file_name().unwrap_or_default().to_string_lossy();
        if fname == format!("{}.ts", name) || fname == format!("{}.tsx", name) {
            return std::fs::read_to_string(&entry)
                .unwrap_or_else(|e| panic!("Failed to load {}: {}", entry.display(), e));
        }
    }
    panic!("Test case not found: {}/{}", suite, name);
}

fn walkdir(dir: PathBuf) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(walkdir(path));
            } else {
                results.push(path);
            }
        }
    }
    results
}

/// Fixture descriptor for benchmark parametrization.
pub struct Fixture {
    pub name: &'static str,
    pub file_name: &'static str,
    pub source: String,
}

/// Try to load a file, returning None if missing or too small (failed download).
fn try_load(path: &Path, min_bytes: u64) -> Option<String> {
    if !path.exists() {
        return None;
    }
    if let Ok(meta) = path.metadata() {
        if meta.len() < min_bytes {
            return None;
        }
    }
    std::fs::read_to_string(path).ok()
}

fn include_large() -> bool {
    std::env::var("TSC_BENCH_LARGE").map_or(false, |v| v == "1" || v == "true")
}

/// Load all standard benchmark fixtures.
///
/// By default loads small/medium fixtures only (up to ~537KB).
/// Set `TSC_BENCH_LARGE=1` to also include typescript.js (8MB) and
/// cal.com.tsx (1MB) for stress testing on machines with enough RAM.
pub fn standard_fixtures() -> Vec<Fixture> {
    let mut fixtures = Vec::new();
    let root = repo_root();
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let large = include_large();

    // --- Large fixtures (opt-in via TSC_BENCH_LARGE=1) ---

    if large {
        // TypeScript compiler bundle (~8MB, 172K lines) — THE canonical large benchmark
        // Source: oxc-project/bench-javascript-parser-written-in-rust
        if let Some(source) = try_load(&fixture_dir.join("typescript.js"), 1000) {
            fixtures.push(Fixture {
                name: "typescript_js",
                file_name: "typescript.js",
                source,
            });
        }

        // cal.com TSX bundle (~1MB, 30K lines) — large real-world TSX
        // Source: oxc-project/bench-javascript-parser-written-in-rust
        if let Some(source) = try_load(&fixture_dir.join("cal.com.tsx"), 1000) {
            fixtures.push(Fixture {
                name: "cal_com_tsx",
                file_name: "cal.com.tsx",
                source,
            });
        }
    }

    // --- Medium fixtures (always included when downloaded) ---

    // TypeScript parser source (~537K, 10.8K lines) — real-world TS from the compiler
    // Source: oxc-project/bench-transformer
    if let Some(source) = try_load(&fixture_dir.join("parser.ts"), 1000) {
        fixtures.push(Fixture {
            name: "ts_parser_source",
            file_name: "parser.ts",
            source,
        });
    }

    // Vue.js runtime-core renderer (~72K, 2.5K lines) — real-world TS library
    // Source: oxc-project/bench-transformer
    if let Some(source) = try_load(&fixture_dir.join("renderer.ts"), 1000) {
        fixtures.push(Fixture {
            name: "vue_renderer",
            file_name: "renderer.ts",
            source,
        });
    }

    // AFFiNE table component (~31K, 1.1K lines) — medium real-world TSX
    // Source: oxc-project/bench-transformer
    if let Some(source) = try_load(&fixture_dir.join("table.tsx"), 1000) {
        fixtures.push(Fixture {
            name: "affine_table_tsx",
            file_name: "table.tsx",
            source,
        });
    }

    // cal.com UserSettings (~4K, 124 lines) — small TSX component
    // Source: oxc-project/bench-transformer
    if let Some(source) = try_load(&fixture_dir.join("UserSettings.tsx"), 100) {
        fixtures.push(Fixture {
            name: "user_settings_tsx",
            file_name: "UserSettings.tsx",
            source,
        });
    }

    // --- Built-in fixtures from our test suite (always available) ---

    if let Some(source) = try_load(
        &root.join("tests/cases/conformance/parser/ecmascript5/parserRealSource11.ts"),
        1000,
    ) {
        fixtures.push(Fixture {
            name: "real_source_small",
            file_name: "parserRealSource11.ts",
            source,
        });
    }

    if let Some(source) = try_load(&root.join("tests/cases/compiler/manyConstExports.ts"), 1000) {
        fixtures.push(Fixture {
            name: "many_const_exports",
            file_name: "manyConstExports.ts",
            source,
        });
    }

    if let Some(source) = try_load(
        &root.join("tests/cases/compiler/largeControlFlowGraph.ts"),
        1000,
    ) {
        fixtures.push(Fixture {
            name: "large_control_flow",
            file_name: "largeControlFlowGraph.ts",
            source,
        });
    }

    if fixtures.is_empty() {
        eprintln!(
            "WARNING: No benchmark fixtures found. Run `make bench-fixtures` to download them."
        );
    }

    fixtures
}
