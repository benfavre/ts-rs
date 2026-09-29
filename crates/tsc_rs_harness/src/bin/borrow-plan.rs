use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tsc_rs_harness::{BaselineResult, BaselineRunner, Suite};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailureRecord {
    name: String,
    bucket: String,
    line: Option<usize>,
    expected: Option<String>,
    actual: Option<String>,
    diff: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct TopicDef {
    id: &'static str,
    title: &'static str,
    why: &'static str,
    crate_targets: &'static [&'static str],
    keywords: &'static [&'static str],
    references: &'static [&'static str],
}

#[derive(Debug, Default, Clone)]
struct TopicSummary {
    score: usize,
    count: usize,
    buckets: BTreeMap<String, usize>,
    samples: Vec<String>,
}

const TOPIC_DEFS: &[TopicDef] = &[
    TopicDef {
        id: "module-envelope-and-prologue",
        title: "Module envelope and prologue parity",
        why: "Targets AMD/UMD/System/CJS wrappers, `exports` wiring, and `__esModule` policy.",
        crate_targets: &["crates/tsc_rs_emitter", "crates/tsc_rs_harness"],
        keywords: &[
            "system.register",
            "define(",
            "__esmodule",
            "__modulename",
            "module.exports",
            "exports.",
            "require(",
            "amd",
            "umd",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/transformers/module/module.ts",
            "reference/repos/TypeScript/src/compiler/emitter.ts",
            "reference/repos/TypeScript/src/compiler/transformers/declarations.ts",
            "reference/repos/oxc/crates/oxc_transformer/src/typescript/module.rs",
        ],
    },
    TopicDef {
        id: "comments-trivia-and-layout",
        title: "Comments, trivia, headers, and whitespace",
        why: "Targets first-mismatch classes caused by directive/comment movement or line-layout drift.",
        crate_targets: &["crates/tsc_rs_emitter", "crates/tsc_rs_harness"],
        keywords: &[
            "//// [",
            "sourcemappingurl",
            "/// <reference",
            "comment",
            "whitespace",
            "/*",
            "//",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/emitter.ts",
            "reference/repos/TypeScript/src/compiler/sourcemap.ts",
            "reference/repos/TypeScript/src/harness/compilerImpl.ts",
            "reference/repos/swc/crates/swc_ecma_codegen/src/lib.rs",
            "reference/repos/swc/crates/swc_ecma_codegen/src/text_writer/basic_impl.rs",
            "reference/repos/oxc/crates/oxc_codegen/src/lib.rs",
        ],
    },
    TopicDef {
        id: "async-and-generator-transform",
        title: "Async/await and generator transform parity",
        why: "Targets helper emission and lowering differences around `__awaiter`, `yield`, and `for await`.",
        crate_targets: &["crates/tsc_rs_emitter"],
        keywords: &[
            "__awaiter",
            "__generator",
            "for await",
            "await ",
            "yield ",
            "async",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/transformers/es2017.ts",
            "reference/repos/TypeScript/src/compiler/transformers/generators.ts",
            "reference/repos/swc/crates/swc_ecma_compat_es2017/src/async_to_generator.rs",
            "reference/repos/oxc/crates/oxc_transformer/src/es2017/async_to_generator.rs",
        ],
    },
    TopicDef {
        id: "class-fields-and-decorators",
        title: "Class fields, static blocks, and decorators",
        why: "Targets class transform ordering, helper wiring, and static initializer behavior.",
        crate_targets: &["crates/tsc_rs_emitter", "crates/tsc_rs_parser"],
        keywords: &[
            "__esdecorate",
            "__runinitializers",
            "class",
            "constructor",
            "static",
            "super(",
            "#",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/transformers/classFields.ts",
            "reference/repos/TypeScript/src/compiler/transformers/esDecorators.ts",
            "reference/repos/swc/crates/swc_ecma_compat_es2022/src/class_properties/mod.rs",
            "reference/repos/oxc/crates/oxc_transformer/src/es2022/class_properties/mod.rs",
        ],
    },
    TopicDef {
        id: "destructuring-and-object-spread",
        title: "Destructuring and object spread lowering",
        why: "Targets binding pattern lowering and `Object.assign` expansion/parens ordering.",
        crate_targets: &["crates/tsc_rs_emitter", "crates/tsc_rs_parser"],
        keywords: &[
            "destructuring",
            "spread",
            "object.assign",
            "__rest",
            "for of",
            "for_in",
            "...",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/transformers/destructuring.ts",
            "reference/repos/TypeScript/src/compiler/transformers/es2018.ts",
            "reference/repos/swc/crates/swc_ecma_compat_es2015/src/destructuring.rs",
            "reference/repos/oxc/crates/oxc_transformer/src/es2018/object_rest_spread.rs",
        ],
    },
    TopicDef {
        id: "jsx-and-react-emit",
        title: "JSX and React emit mode parity",
        why: "Targets JSX factory/classic-vs-preserve behavior and prop spread call shape.",
        crate_targets: &["crates/tsc_rs_emitter", "crates/tsc_rs_parser"],
        keywords: &[
            "jsx",
            "tsx",
            "react.createelement",
            "fragment",
            "<div",
            "</",
            "<>",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/transformers/jsx.ts",
            "reference/repos/swc/crates/swc_ecma_transforms_react/src/jsx/mod.rs",
            "reference/repos/oxc/crates/oxc_transformer/src/jsx/mod.rs",
        ],
    },
    TopicDef {
        id: "const-enum-and-namespace",
        title: "Const-enum and namespace emit decisions",
        why: "Targets inlining/preservation boundary and namespace merge emit ordering.",
        crate_targets: &["crates/tsc_rs_emitter"],
        keywords: &[
            "constenum",
            "const enum",
            "enum",
            "namespace",
            "/* ",
            " */",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/transformers/ts.ts",
            "reference/repos/TypeScript/src/compiler/checker.ts",
            "reference/repos/oxc/crates/oxc_transformer/src/typescript/enum.rs",
            "reference/repos/oxc/crates/oxc_transformer/src/typescript/namespace.rs",
        ],
    },
    TopicDef {
        id: "resolver-and-output-paths",
        title: "Resolver and output path/source-map semantics",
        why: "Targets baseline drift caused by module specifier resolution and output file naming.",
        crate_targets: &["crates/tsc_rs_resolver", "crates/tsc_rs_project", "crates/tsc_rs_harness"],
        keywords: &[
            "moduleresolution",
            "roots",
            "rootdir",
            "paths",
            "tsconfig",
            "sourcemappingurl",
            "outdir",
            "commonsource",
            "nodenext",
        ],
        references: &[
            "reference/repos/TypeScript/src/compiler/moduleNameResolver.ts",
            "reference/repos/TypeScript/src/compiler/program.ts",
            "reference/repos/oxc-resolver/src/resolution.rs",
            "reference/repos/oxc-resolver/src/tsconfig_resolver.rs",
        ],
    },
    TopicDef {
        id: "general-emit-polish",
        title: "General emit polish and parser recovery",
        why: "Catch-all for syntax-recovery and codegen edge cases not strongly matched elsewhere.",
        crate_targets: &["crates/tsc_rs_emitter", "crates/tsc_rs_parser"],
        keywords: &["<error>", "panic", "crash", "syntax", "token", "unexpected"],
        references: &[
            "reference/repos/TypeScript/src/compiler/parser.ts",
            "reference/repos/TypeScript/src/compiler/scanner.ts",
            "reference/repos/TypeScript/src/compiler/emitter.ts",
            "reference/repos/oxc/crates/oxc_parser/src/lib.rs",
        ],
    },
];

fn print_usage() {
    eprintln!(
        "Usage: cargo run -p tsc_rs_harness --bin borrow-plan -- [options]\n\
         \n\
         Options:\n\
           --suite <compiler|conformance|fourslash|project>   (default: compiler)\n\
           --root <workspace-root>                            (default: auto-detected)\n\
           --limit <n>                                        run only first N discovered cases\n\
           --kind <bucket>                                    restrict to one mismatch bucket\n\
           --name-contains <text>                             filter selected cases by name substring\n\
           --top-topics <n>                                   show top N prioritized topics (default: 8)\n\
           --show-cases <n>                                   show sample cases per topic (default: 5)\n\
           --out <path>                                       write report to path instead of stdout\n\
           --copy-pack <dir>                                  copy top-topic reference files into dir\n\
           --copy-topics <n>                                  number of top topics to include in pack (default: 3)\n\
           --no-cache                                         skip result cache\n\
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

fn first_mismatch(expected: &str, actual: &str) -> (Option<usize>, Option<String>, Option<String>) {
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();
    let min_len = exp_lines.len().min(act_lines.len());

    for i in 0..min_len {
        if exp_lines[i] != act_lines[i] {
            return (
                Some(i + 1),
                Some(exp_lines[i].to_string()),
                Some(act_lines[i].to_string()),
            );
        }
    }

    if exp_lines.len() != act_lines.len() {
        return (
            Some(min_len + 1),
            exp_lines.get(min_len).map(|s| s.to_string()),
            act_lines.get(min_len).map(|s| s.to_string()),
        );
    }

    (None, None, None)
}

fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with("*")
}

fn classify_failure(result: &BaselineResult) -> FailureRecord {
    if !result.baseline_exists {
        return FailureRecord {
            name: result.name.clone(),
            bucket: "NO-BASELINE".to_string(),
            line: None,
            expected: None,
            actual: None,
            diff: result.diff.clone(),
        };
    }

    let diff = result.diff.as_deref().unwrap_or_default();
    if diff.starts_with("PANIC") {
        return FailureRecord {
            name: result.name.clone(),
            bucket: "PANIC".to_string(),
            line: None,
            expected: None,
            actual: None,
            diff: result.diff.clone(),
        };
    }
    if diff.starts_with("CRASH") {
        return FailureRecord {
            name: result.name.clone(),
            bucket: "CRASH".to_string(),
            line: None,
            expected: None,
            actual: None,
            diff: result.diff.clone(),
        };
    }

    let (line, expected, actual) = first_mismatch(&result.expected_output, &result.actual_output);
    let bucket = match (expected.as_deref(), actual.as_deref()) {
        (None, Some(_)) => "EXTRA-LINES-AT-END",
        (Some(_), None) => "MISSING-LINES-AT-END",
        (Some(exp), Some(act)) if exp.starts_with("//// [") && act.starts_with("//// [") => {
            "WRONG-FILE-HEADER"
        }
        (Some(exp), Some(act)) if exp.trim() == act.trim() => "WHITESPACE-ONLY",
        (Some(exp), Some(act)) if is_comment_line(exp) && !is_comment_line(act) => {
            "MISSING-COMMENT"
        }
        (Some(exp), Some(act)) if !is_comment_line(exp) && is_comment_line(act) => "EXTRA-COMMENT",
        _ => "CODE-DIFF",
    };

    FailureRecord {
        name: result.name.clone(),
        bucket: bucket.to_string(),
        line,
        expected,
        actual,
        diff: result.diff.clone(),
    }
}

fn bucket_weight(bucket: &str) -> usize {
    match bucket {
        "PANIC" | "CRASH" => 8,
        "CODE-DIFF" => 5,
        "WRONG-FILE-HEADER" => 4,
        "MISSING-COMMENT" | "EXTRA-COMMENT" => 4,
        "WHITESPACE-ONLY" => 2,
        "EXTRA-LINES-AT-END" | "MISSING-LINES-AT-END" => 2,
        "NO-BASELINE" => 0,
        _ => 3,
    }
}

fn keyword_score(text: &str, topic: &TopicDef) -> usize {
    topic
        .keywords
        .iter()
        .filter(|kw| text.contains(**kw))
        .count()
}

fn topic_for_failure(failure: &FailureRecord) -> &'static TopicDef {
    let mut text = String::new();
    text.push_str(&failure.name.to_ascii_lowercase());
    text.push('\n');
    if let Some(expected) = failure.expected.as_deref() {
        text.push_str(&expected.to_ascii_lowercase());
        text.push('\n');
    }
    if let Some(actual) = failure.actual.as_deref() {
        text.push_str(&actual.to_ascii_lowercase());
        text.push('\n');
    }
    if let Some(diff) = failure.diff.as_deref() {
        text.push_str(&diff.to_ascii_lowercase());
    }

    let mut best: Option<(&TopicDef, usize)> = None;
    for topic in TOPIC_DEFS {
        let mut score = keyword_score(&text, topic);

        if failure.bucket == "WRONG-FILE-HEADER" && topic.id == "module-envelope-and-prologue" {
            score += 2;
        }
        if (failure.bucket == "MISSING-COMMENT"
            || failure.bucket == "EXTRA-COMMENT"
            || failure.bucket == "WHITESPACE-ONLY")
            && topic.id == "comments-trivia-and-layout"
        {
            score += 2;
        }
        if failure.name.to_ascii_lowercase().contains("jsx") && topic.id == "jsx-and-react-emit" {
            score += 3;
        }
        if failure.name.to_ascii_lowercase().contains("constenum")
            && topic.id == "const-enum-and-namespace"
        {
            score += 3;
        }
        if failure.name.to_ascii_lowercase().contains("module")
            && topic.id == "module-envelope-and-prologue"
        {
            score += 1;
        }

        match best {
            Some((_, best_score)) if score <= best_score => {}
            _ => {
                best = Some((topic, score));
            }
        }
    }

    let fallback = TOPIC_DEFS
        .iter()
        .find(|t| t.id == "general-emit-polish")
        .expect("missing general-emit-polish topic");

    match best {
        Some((topic, score)) if score > 0 => topic,
        _ => fallback,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_report(
    suite: Suite,
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    selected: &[FailureRecord],
    ranked_topics: &[(&'static TopicDef, TopicSummary)],
    top_topics: usize,
) -> String {
    let pass_rate = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64 * 100.0
    };

    let mut out = String::new();
    out.push_str("# Borrowing Priority Report\n\n");
    out.push_str(&format!(
        "suite={} total={} passed={} failed={} skipped={} pass_rate={:.1}%\n\n",
        suite.as_str(),
        total,
        passed,
        failed,
        skipped,
        pass_rate
    ));
    out.push_str(&format!(
        "selected_failures={} (after filters)\n\n",
        selected.len()
    ));
    out.push_str("## Prioritized Topics\n\n");

    for (idx, (topic, summary)) in ranked_topics.iter().take(top_topics).enumerate() {
        out.push_str(&format!(
            "{}. `{}` - {} (score={}, cases={})\n",
            idx + 1,
            topic.id,
            topic.title,
            summary.score,
            summary.count
        ));
        out.push_str(&format!("   why: {}\n", topic.why));
        out.push_str("   target crates: ");
        out.push_str(&topic.crate_targets.join(", "));
        out.push('\n');
        out.push_str("   bucket mix: ");
        if summary.buckets.is_empty() {
            out.push_str("<none>\n");
        } else {
            let parts: Vec<String> = summary
                .buckets
                .iter()
                .map(|(bucket, count)| format!("{bucket}={count}"))
                .collect();
            out.push_str(&parts.join(", "));
            out.push('\n');
        }
        if !summary.samples.is_empty() {
            out.push_str("   sample cases:\n");
            for sample in &summary.samples {
                out.push_str(&format!("   - {}\n", sample));
            }
        }
        out.push_str("   references:\n");
        for r in topic.references {
            out.push_str(&format!("   - {}\n", r));
        }
        out.push('\n');
    }

    out
}

fn copy_reference_pack(
    workspace_root: &Path,
    pack_dir: &Path,
    ranked_topics: &[(&'static TopicDef, TopicSummary)],
    copy_topics: usize,
) -> std::io::Result<String> {
    std::fs::create_dir_all(pack_dir)?;

    let mut refs: BTreeSet<String> = BTreeSet::new();
    let mut topic_rows: Vec<(&str, Vec<&str>)> = Vec::new();
    for (topic, _) in ranked_topics.iter().take(copy_topics) {
        let mut topic_refs = Vec::new();
        for r in topic.references {
            refs.insert((*r).to_string());
            topic_refs.push(*r);
        }
        topic_rows.push((topic.id, topic_refs));
    }

    let mut copied = 0usize;
    let mut missing: Vec<String> = Vec::new();
    for rel in &refs {
        let src = workspace_root.join(rel);
        if !src.is_file() {
            missing.push(rel.clone());
            continue;
        }
        let dst = pack_dir.join("files").join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
        copied += 1;
    }

    let mut index = String::new();
    index.push_str("# Borrowed Reference Pack\n\n");
    index.push_str(&format!(
        "source workspace: `{}`\n\n",
        workspace_root.display()
    ));
    index.push_str("## Included Topics\n\n");
    for (topic_id, refs_for_topic) in topic_rows {
        index.push_str(&format!("- `{}`\n", topic_id));
        for r in refs_for_topic {
            index.push_str(&format!("  - `{}`\n", r));
        }
    }
    index.push('\n');
    index.push_str(&format!("copied_files={}\n", copied));
    if !missing.is_empty() {
        index.push_str("missing_files:\n");
        for m in &missing {
            index.push_str(&format!("- `{}`\n", m));
        }
    }
    std::fs::write(pack_dir.join("INDEX.md"), index)?;

    Ok(format!(
        "copied_files={} missing_files={} pack_dir={}",
        copied,
        missing.len(),
        pack_dir.display()
    ))
}

// --- Result cache ---

#[derive(Serialize, Deserialize)]
struct BorrowCache {
    source_hash: u64,
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<FailureRecord>,
}

fn cache_path(root: &Path, suite: &str, bin: &str) -> PathBuf {
    root.join(".harness-cache")
        .join(format!("{bin}-{suite}.json"))
}

fn load_cache(path: &Path, current_hash: u64) -> Option<BorrowCache> {
    let data = std::fs::read_to_string(path).ok()?;
    let cached: BorrowCache = serde_json::from_str(&data).ok()?;
    if cached.source_hash == current_hash {
        Some(cached)
    } else {
        None
    }
}

fn save_cache(path: &Path, cache: &BorrowCache) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, data);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);

    let mut suite = Suite::Compiler;
    let mut root = workspace_root();
    let mut limit: Option<usize> = None;
    let mut filter_kind: Option<String> = None;
    let mut name_contains: Option<String> = None;
    let mut top_topics = 8usize;
    let mut show_cases = 5usize;
    let mut out_path: Option<PathBuf> = None;
    let mut copy_pack: Option<PathBuf> = None;
    let mut copy_topics = 3usize;
    let mut no_cache = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
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
            "--root" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --root requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                root = PathBuf::from(val);
            }
            "--limit" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --limit requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                limit = match val.parse::<usize>() {
                    Ok(v) => Some(v),
                    Err(e) => {
                        eprintln!("error: invalid --limit value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--kind" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --kind requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                filter_kind = Some(val);
            }
            "--name-contains" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --name-contains requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                name_contains = Some(val);
            }
            "--top-topics" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --top-topics requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                top_topics = match val.parse::<usize>() {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("error: invalid --top-topics value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--show-cases" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --show-cases requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                show_cases = match val.parse::<usize>() {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("error: invalid --show-cases value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--out" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --out requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                out_path = Some(PathBuf::from(val));
            }
            "--copy-pack" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --copy-pack requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                copy_pack = Some(PathBuf::from(val));
            }
            "--copy-topics" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --copy-topics requires a value");
                    print_usage();
                    std::process::exit(2);
                };
                copy_topics = match val.parse::<usize>() {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("error: invalid --copy-topics value '{val}': {e}");
                        std::process::exit(2);
                    }
                };
            }
            "--no-cache" => {
                no_cache = true;
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option '{other}'");
                print_usage();
                std::process::exit(2);
            }
            other => {
                eprintln!("error: unexpected argument '{other}'");
                print_usage();
                std::process::exit(2);
            }
        }
    }

    // Try loading cached results for full suite runs
    let source_hash = tsc_rs_harness::cache_key::compiler_source_hash(&root);
    let use_cache = limit.is_none() && !no_cache;
    let cf = if use_cache {
        Some(cache_path(&root, suite.as_str(), "borrow"))
    } else {
        None
    };

    let cached = cf
        .as_ref()
        .zip(source_hash)
        .and_then(|(path, hash)| load_cache(path, hash));

    let (total, passed, failed, skipped, mut failures) = if let Some(c) = cached {
        eprintln!("(using cached results)");
        (c.total, c.passed, c.failed, c.skipped, c.failures)
    } else {
        let runner = BaselineRunner::new(&root);
        let suite_result = match runner.run_suite_with_limit(suite, limit) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("failed to run suite: {e}");
                std::process::exit(2);
            }
        };

        let failures: Vec<FailureRecord> = suite_result
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(classify_failure)
            .collect();

        if let (Some(path), Some(hash)) = (&cf, source_hash) {
            save_cache(
                path,
                &BorrowCache {
                    source_hash: hash,
                    total: suite_result.total,
                    passed: suite_result.passed,
                    failed: suite_result.failed,
                    skipped: suite_result.skipped,
                    failures: failures.clone(),
                },
            );
        }

        (
            suite_result.total,
            suite_result.passed,
            suite_result.failed,
            suite_result.skipped,
            failures,
        )
    };

    if let Some(kind) = filter_kind.as_deref() {
        failures.retain(|f| f.bucket.eq_ignore_ascii_case(kind));
    }
    if let Some(needle) = name_contains.as_deref() {
        let needle = needle.to_ascii_lowercase();
        failures.retain(|f| f.name.to_ascii_lowercase().contains(&needle));
    }

    failures.sort_by(|a, b| a.name.cmp(&b.name));

    let mut topic_summaries: HashMap<&'static str, TopicSummary> = HashMap::new();
    for failure in &failures {
        let topic = topic_for_failure(failure);
        let entry = topic_summaries.entry(topic.id).or_default();
        entry.score += bucket_weight(&failure.bucket);
        entry.count += 1;
        *entry.buckets.entry(failure.bucket.clone()).or_insert(0) += 1;
        if entry.samples.len() < show_cases && !entry.samples.contains(&failure.name) {
            let label = if let Some(line) = failure.line {
                format!("{} (line {})", failure.name, line)
            } else {
                failure.name.clone()
            };
            entry.samples.push(label);
        }
    }

    let mut ranked_topics: Vec<(&'static TopicDef, TopicSummary)> = TOPIC_DEFS
        .iter()
        .filter_map(|topic| {
            topic_summaries
                .remove(topic.id)
                .map(|summary| (topic, summary))
        })
        .collect();
    ranked_topics.sort_by(|a, b| {
        b.1.score
            .cmp(&a.1.score)
            .then_with(|| b.1.count.cmp(&a.1.count))
            .then_with(|| a.0.id.cmp(b.0.id))
    });

    let report = render_report(
        suite,
        total,
        passed,
        failed,
        skipped,
        &failures,
        &ranked_topics,
        top_topics,
    );

    if let Some(path) = out_path {
        if let Err(e) = std::fs::write(&path, report) {
            eprintln!("failed to write report to {}: {e}", path.display());
            std::process::exit(2);
        }
        println!("wrote report: {}", path.display());
    } else {
        println!("{report}");
    }

    if let Some(pack_dir) = copy_pack {
        match copy_reference_pack(&root, &pack_dir, &ranked_topics, copy_topics) {
            Ok(summary) => println!("{summary}"),
            Err(e) => {
                eprintln!(
                    "failed to copy reference pack to {}: {e}",
                    pack_dir.display()
                );
                std::process::exit(2);
            }
        }
    }
}
