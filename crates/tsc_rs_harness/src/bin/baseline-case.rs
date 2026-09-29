use std::{collections::BTreeMap, path::PathBuf};

use tsc_rs_harness::{BaselineKind, BaselineRunner, Suite};

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p tsc_rs_harness --bin baseline-case -- <test-name> [options]\n\
         \n\
         Options:\n\
           --suite <compiler|conformance|fourslash|project>   (default: compiler)\n\
           --baseline <js|errors|symbols|types>               (default: js)\n\
           --root <workspace-root>                            (default: auto-detected)\n\
           --variant <key=value,...>                          select exact expanded oracle attributes\n\
           --show-outputs                                     print expected/actual sections\n\
           --show-first-mismatch                              print mismatch line + context\n\
           --show-js-headers                                  print expected/actual .js header order\n\
           --context-lines <n>                                context size for mismatch view (default: 2)\n\
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

fn first_mismatch_idx(expected: &str, actual: &str) -> Option<usize> {
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();
    let min_len = exp_lines.len().min(act_lines.len());
    for i in 0..min_len {
        if exp_lines[i] != act_lines[i] {
            return Some(i);
        }
    }
    (exp_lines.len() != act_lines.len()).then_some(min_len)
}

fn print_line_window(label: &str, lines: &[&str], center_idx: usize, context_lines: usize) {
    println!("{label}:");
    if lines.is_empty() {
        println!("  <empty>");
        return;
    }
    let start = center_idx.saturating_sub(context_lines);
    let end = (center_idx + context_lines + 1).min(lines.len());
    for i in start..end {
        let marker = if i == center_idx { ">" } else { " " };
        println!("  {marker} {:>5}: {}", i + 1, lines[i]);
    }
}

fn js_headers(output: &str) -> Vec<(usize, String)> {
    output
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            if !line.starts_with("//// [") {
                return None;
            }
            let lower = line.to_ascii_lowercase();
            if lower.ends_with(".js]") || lower.ends_with(".mjs]") || lower.ends_with(".cjs]") {
                return Some((idx + 1, line.to_string()));
            }
            None
        })
        .collect()
}

fn print_js_headers(label: &str, output: &str) {
    println!("{label}:");
    let headers = js_headers(output);
    if headers.is_empty() {
        println!("  <none>");
        return;
    }
    for (line_no, header) in headers {
        println!("  {:>5}: {}", line_no, header);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);

    let mut suite = Suite::Compiler;
    let mut baseline_kind = BaselineKind::Js;
    let mut root = workspace_root();
    let mut show_outputs = false;
    let mut show_first_mismatch = false;
    let mut show_js_headers = false;
    let mut context_lines = 2usize;
    let mut test_name: Option<String> = None;
    let mut variant_attributes = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--variant" => {
                let Some(value) = args.next() else {
                    eprintln!("error: --variant requires key=value,...");
                    std::process::exit(2);
                };
                let mut attributes = BTreeMap::new();
                for item in value.split(',') {
                    let Some((key, value)) = item.split_once('=') else {
                        eprintln!("error: invalid variant attribute '{item}'");
                        std::process::exit(2);
                    };
                    let key = key.trim().to_ascii_lowercase();
                    let value = value.trim().to_ascii_lowercase();
                    if key.is_empty() || value.is_empty() || attributes.insert(key, value).is_some()
                    {
                        eprintln!("error: empty or duplicate variant attribute '{item}'");
                        std::process::exit(2);
                    }
                }
                variant_attributes = Some(attributes);
            }
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "--suite" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --suite requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                suite = match val.parse::<Suite>() {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: invalid suite '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--baseline" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --baseline requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                baseline_kind = match val.parse::<BaselineKind>() {
                    Ok(k) => k,
                    Err(_) => {
                        eprintln!("error: invalid baseline kind '{val}' (expected: js, errors, symbols, types)");
                        std::process::exit(2);
                    }
                };
            }
            "--root" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --root requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                root = PathBuf::from(val);
            }
            "--show-outputs" => {
                show_outputs = true;
            }
            "--show-first-mismatch" => {
                show_first_mismatch = true;
            }
            "--show-js-headers" => {
                show_js_headers = true;
            }
            "--context-lines" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --context-lines requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                context_lines = match val.parse::<usize>() {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("error: invalid --context-lines value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option '{other}'");
                print_usage();
                std::process::exit(2);
            }
            other => {
                if test_name.is_some() {
                    eprintln!("error: unexpected extra argument '{other}'");
                    print_usage();
                    std::process::exit(2);
                }
                test_name = Some(other.to_string());
            }
        }
    }

    let Some(test_name) = test_name else {
        eprintln!("error: missing <test-name>");
        print_usage();
        std::process::exit(2);
    };

    let runner = BaselineRunner::new(root);
    let run_results = if let Some(attributes) = variant_attributes {
        runner
            .discover_baseline_variants(suite, baseline_kind)
            .and_then(|variants| {
                let selected: Vec<_> = variants
                    .iter()
                    .filter(|variant| {
                        variant
                            .source_path
                            .file_stem()
                            .and_then(|name| name.to_str())
                            == Some(test_name.as_str())
                            && variant.attributes == attributes
                    })
                    .collect();
                match selected.as_slice() {
                    [variant] => Ok(vec![
                        runner
                            .run_expanded_variant(variant, suite, baseline_kind)
                            .result,
                    ]),
                    _ => Err(format!(
                        "expected one oracle for '{test_name}' with {attributes:?}, found {}",
                        selected.len()
                    )),
                }
            })
    } else {
        runner
            .run_named_cases_with_kind(suite, &[test_name.as_str()], baseline_kind)
            .map_err(|error| error.to_string())
    };
    let results = match run_results {
        Ok(r) => r,
        Err(e) => {
            eprintln!("failed to run baseline case: {e}");
            std::process::exit(2);
        }
    };
    let result = &results[0];

    println!(
        "case={} suite={} baseline={} passed={} baseline_exists={}",
        result.name,
        suite.as_str(),
        baseline_kind.as_str(),
        result.passed,
        result.baseline_exists
    );

    if show_js_headers {
        println!("\n--- JS HEADERS ---");
        print_js_headers("expected", &result.expected_output);
        print_js_headers("actual", &result.actual_output);
    }

    if !result.baseline_exists {
        println!(
            "baseline missing for '{}'. Expected file: tests/baselines/reference/{}.js",
            result.name, result.name
        );
        std::process::exit(2);
    }

    if result.passed {
        println!("status: PASS");
        return;
    }

    println!("status: FAIL");
    if let Some(ref diff) = result.diff {
        println!("\n--- DIFF ---\n{diff}");
    }
    if show_first_mismatch {
        if let Some(idx) = first_mismatch_idx(&result.expected_output, &result.actual_output) {
            let exp_lines: Vec<&str> = result.expected_output.lines().collect();
            let act_lines: Vec<&str> = result.actual_output.lines().collect();
            println!("\n--- FIRST MISMATCH ---\nline: {}", idx + 1);
            match exp_lines.get(idx) {
                Some(line) => println!("expected: {line}"),
                None => println!("expected: <EOF>"),
            }
            match act_lines.get(idx) {
                Some(line) => println!("actual:   {line}"),
                None => println!("actual:   <EOF>"),
            }
            println!();
            print_line_window(
                "expected context",
                &exp_lines,
                idx.min(exp_lines.len().saturating_sub(1)),
                context_lines,
            );
            println!();
            print_line_window(
                "actual context",
                &act_lines,
                idx.min(act_lines.len().saturating_sub(1)),
                context_lines,
            );
        } else {
            println!("\n--- FIRST MISMATCH ---\nnone");
        }
    }
    if show_outputs {
        println!("\n--- EXPECTED ---\n{}", result.expected_output);
        println!("\n--- ACTUAL ---\n{}", result.actual_output);
    }
    std::process::exit(1);
}
