use std::path::PathBuf;

use tsc_rs_harness::lsp_runner::LspRunner;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p tsc_rs_harness --bin lsp-case -- <test-name> [options]\n\
         \n\
         Options:\n\
           --root <workspace-root>     (default: auto-detected)\n\
           --show-outputs              print expected/actual for all markers\n\
           --show-markers              print all discovered markers\n\
           --show-source               print extracted virtual file system\n\
           -h, --help"
    );
}

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to find workspace root from crate path")
        .to_path_buf()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut root: Option<PathBuf> = None;
    let mut test_name: Option<String> = None;
    let mut show_outputs = false;
    let mut show_markers = false;
    let mut show_source = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "--root" => {
                i += 1;
                root = Some(PathBuf::from(&args[i]));
            }
            "--show-outputs" => show_outputs = true,
            "--show-markers" => show_markers = true,
            "--show-source" => show_source = true,
            arg if !arg.starts_with('-') => {
                test_name = Some(arg.to_string());
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let Some(test_name) = test_name else {
        eprintln!("error: test name required");
        print_usage();
        std::process::exit(1);
    };

    let root = root.unwrap_or_else(workspace_root);
    let runner = LspRunner::new(&root);

    // Optionally show parsed test info
    if show_source || show_markers {
        let test_path = root
            .join("tests/cases/fourslash")
            .join(format!("{}.ts", test_name));
        if let Ok(raw) = std::fs::read_to_string(&test_path) {
            if let Ok(test) = tsc_rs_harness::lsp_parser::parse_fourslash(&test_name, &raw) {
                if show_source {
                    println!("=== Virtual File System ===");
                    for file in &test.files {
                        println!("--- {} ({} bytes) ---", file.name, file.content.len());
                        for (i, line) in file.content.lines().enumerate() {
                            println!("{:>4} | {}", i + 1, line);
                        }
                        println!();
                    }
                }
                if show_markers {
                    println!("=== Markers ({}) ===", test.markers.len());
                    for m in &test.markers {
                        println!(
                            "  {:>8} @ {}:{} (offset {})",
                            if m.name.is_empty() {
                                "(unnamed)"
                            } else {
                                &m.name
                            },
                            m.file_name,
                            m.position,
                            m.position
                        );
                    }
                    println!();
                    println!("=== Verify Commands ({}) ===", test.verify_commands.len());
                    for cmd in &test.verify_commands {
                        println!("  {:?}", cmd);
                    }
                    println!();
                }
            }
        }
    }

    // Run the test
    let result = match runner.run_named(&test_name) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // Print summary
    let status = if result.passed {
        "PASSED"
    } else if result.skipped {
        "SKIPPED"
    } else {
        "FAILED"
    };
    println!(
        "test={} op={} status={}",
        result.name,
        result.operation.label(),
        status
    );

    if let Some(ref reason) = result.skip_reason {
        println!("skip_reason: {}", reason);
    }
    if let Some(ref bucket) = result.bucket {
        println!("bucket: {}", bucket);
    }
    if let Some(ref msg) = result.panic_message {
        println!("panic: {}", msg);
    }

    // Print marker results
    if !result.marker_results.is_empty() {
        println!();
        let total = result.marker_results.len();
        let passed = result.marker_results.iter().filter(|m| m.passed).count();
        let failed = total - passed;
        println!(
            "markers: {} total, {} passed, {} failed",
            total, passed, failed
        );

        for mr in &result.marker_results {
            let status = if mr.passed { "ok" } else { "FAIL" };
            println!(
                "  [{}] marker={:>8} @ {}:{}",
                status,
                if mr.marker_name.is_empty() {
                    "(unnamed)"
                } else {
                    &mr.marker_name
                },
                mr.file_name,
                mr.position
            );

            if show_outputs || !mr.passed {
                if let Some(ref expected) = mr.expected {
                    println!("        expected: {}", expected);
                }
                if let Some(ref actual) = mr.actual {
                    println!("        actual:   {}", actual);
                } else {
                    println!("        actual:   (none)");
                }
            }
        }
    }

    // Exit with error code on failure
    if !result.passed && !result.skipped {
        std::process::exit(1);
    }
}
