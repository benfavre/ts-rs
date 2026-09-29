use std::collections::HashMap;
use std::path::PathBuf;

use tsc_rs_harness::lsp_parser::LspOperation;
use tsc_rs_harness::lsp_runner::LspRunner;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p tsc_rs_harness --bin lsp-report -- [options]\n\
         \n\
         Options:\n\
           --op <quickinfo|gotodefinition|findallrefs|completions|all>  (default: quickinfo)\n\
           --root <workspace-root>                   (default: auto-detected)\n\
           --limit <n>                               run only first N discovered cases\n\
           --top <n>                                  show top N failure buckets (default: 20)\n\
           --kind <bucket>                           list cases for one bucket\n\
           --name-contains <text>                    filter cases by name substring\n\
           --show-samples <n>                        sample count per bucket (default: 3)\n\
           --json                                    structured JSON output\n\
           --save-case-list <path>                   write selected case names to file\n\
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

fn parse_op(s: &str) -> Option<LspOperation> {
    match s {
        "quickinfo" => Some(LspOperation::QuickInfo),
        "gotodefinition" => Some(LspOperation::GoToDefinition),
        "findallrefs" => Some(LspOperation::FindAllReferences),
        "completions" => Some(LspOperation::Completions),
        "signaturehelp" => Some(LspOperation::SignatureHelp),
        "rename" => Some(LspOperation::Rename),
        "all" => None, // None means no filter
        _ => {
            eprintln!("unknown operation: {s}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut root: Option<PathBuf> = None;
    let mut op_str = "quickinfo".to_string();
    let mut limit: Option<usize> = None;
    let mut top: usize = 20;
    let mut kind_filter: Option<String> = None;
    let mut name_contains: Option<String> = None;
    let mut show_samples: usize = 3;
    let mut json_output = false;
    let mut save_case_list: Option<PathBuf> = None;

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
            "--op" => {
                i += 1;
                op_str = args[i].clone();
            }
            "--limit" => {
                i += 1;
                limit = Some(args[i].parse().expect("--limit requires a number"));
            }
            "--top" => {
                i += 1;
                top = args[i].parse().expect("--top requires a number");
            }
            "--kind" => {
                i += 1;
                kind_filter = Some(args[i].clone());
            }
            "--name-contains" => {
                i += 1;
                name_contains = Some(args[i].clone());
            }
            "--show-samples" => {
                i += 1;
                show_samples = args[i].parse().expect("--show-samples requires a number");
            }
            "--json" => {
                json_output = true;
            }
            "--save-case-list" => {
                i += 1;
                save_case_list = Some(PathBuf::from(&args[i]));
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let root = root.unwrap_or_else(workspace_root);
    let op_filter = parse_op(&op_str);

    let runner = LspRunner::new(&root);
    let suite = match runner.run_suite(op_filter, limit) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // Collect failures into buckets
    let mut buckets: HashMap<String, Vec<String>> = HashMap::new();
    for result in &suite.results {
        if result.skipped || result.passed {
            continue;
        }
        let bucket_name = result
            .bucket
            .map(|b| b.to_string())
            .unwrap_or_else(|| "UNKNOWN".to_string());
        buckets
            .entry(bucket_name)
            .or_default()
            .push(result.name.clone());
    }

    // Apply name filter
    if let Some(ref substr) = name_contains {
        for cases in buckets.values_mut() {
            cases.retain(|name| name.contains(substr.as_str()));
        }
        buckets.retain(|_, cases| !cases.is_empty());
    }

    // Apply kind filter
    if let Some(ref kind) = kind_filter {
        let kind_upper = kind.to_ascii_uppercase();
        buckets.retain(|k, _| k == &kind_upper);
    }

    if json_output {
        let pass_rate = if suite.total > 0 {
            (suite.passed as f64 / (suite.total - suite.skipped) as f64) * 100.0
        } else {
            0.0
        };
        let mut sorted_buckets: Vec<_> = buckets.iter().collect();
        sorted_buckets.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let top_buckets: Vec<serde_json::Value> = sorted_buckets
            .iter()
            .take(top)
            .map(|(bucket, cases)| {
                serde_json::json!({
                    "bucket": bucket,
                    "count": cases.len(),
                    "samples": cases.iter().take(show_samples).collect::<Vec<_>>(),
                })
            })
            .collect();

        let output = serde_json::json!({
            "op": op_str,
            "total": suite.total,
            "passed": suite.passed,
            "failed": suite.failed,
            "skipped": suite.skipped,
            "pass_rate": format!("{:.1}%", pass_rate),
            "top_buckets": top_buckets,
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        let tested = suite.total - suite.skipped;
        let pass_rate = if tested > 0 {
            (suite.passed as f64 / tested as f64) * 100.0
        } else {
            0.0
        };

        println!(
            "op={} total={} passed={} failed={} skipped={} pass_rate={:.1}%",
            op_str, suite.total, suite.passed, suite.failed, suite.skipped, pass_rate
        );
        println!();

        // Sort buckets by count descending
        let mut sorted: Vec<_> = buckets.into_iter().collect();
        sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        if sorted.is_empty() {
            println!("No failures!");
        } else {
            println!("Top failure buckets:");
            for (bucket, cases) in sorted.iter().take(top) {
                println!("  {:>4}  {}", cases.len(), bucket);
                for case in cases.iter().take(show_samples) {
                    println!("          - {}", case);
                }
                if cases.len() > show_samples {
                    println!("          ... and {} more", cases.len() - show_samples);
                }
            }
        }
    }

    // Save case list if requested
    if let Some(ref path) = save_case_list {
        let mut names: Vec<&str> = Vec::new();
        for result in &suite.results {
            if !result.skipped && !result.passed {
                if let Some(ref substr) = name_contains {
                    if !result.name.contains(substr.as_str()) {
                        continue;
                    }
                }
                if let Some(ref kind) = kind_filter {
                    let bucket_name = result.bucket.map(|b| b.to_string()).unwrap_or_default();
                    if bucket_name != kind.to_ascii_uppercase() {
                        continue;
                    }
                }
                names.push(&result.name);
            }
        }
        names.sort();
        std::fs::write(path, names.join("\n") + "\n").unwrap_or_else(|e| {
            eprintln!("cannot write case list: {e}");
        });
        eprintln!("wrote {} case names to {}", names.len(), path.display());
    }
}
