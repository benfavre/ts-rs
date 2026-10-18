//! `tsc-rs check-site <dir>`: type-check a bext site with external tsrs.
//!
//! bext sites are not checked by their own tsconfig alone: their JSX
//! types live in the framework's `jsx.d.ts` (a global `JSX` namespace that
//! nothing imports), and older sites have no `node_modules` link to
//! `@bext-stack/framework` (the bext runtime embeds it). So this writes a
//! temporary tsconfig that extends the site's and adds:
//! - the site's files (as its own config resolves them), plus the
//!   framework's `jsx.d.ts` when the site's JSX import source is the
//!   framework or its `types` name `@bext-stack/framework/jsx` (which tsc
//!   cannot resolve: type libraries are not looked up through `exports`);
//! - when the site has no `node_modules/@bext-stack/framework`, `paths`
//!   for each subpath of the framework's `package.json` exports, next to
//!   the site's own `paths` (made absolute, since `paths` resolve against
//!   the config that sets them).
//!
//! Configuration parsing and checking both use the external native tsrs binary.
//! Our transpiler is not involved. This is report-only: errors and failed
//! checks still exit 0. Inspect `errors` and `skipped`; invalid CLI usage exits 2.
//!
//! Options: `--framework <dir>` (else `BEXT_FRAMEWORK_DIR`, else a
//! `shared/framework` next to an ancestor of the site), `--json`,
//! `--max <n>` diagnostics listed (default 20).
//!
//! Several site directories check in parallel, each in its own child
//! process exactly as alone (`--jobs <n>` at once, default a quarter of
//! the cores: a check runs 4 checker threads), largest site first; the
//! reports print in the order of the arguments (with `--json`, one line
//! per site).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

const FRAMEWORK: &str = "@bext-stack/framework";

#[derive(Serialize)]
struct Diag {
    file: String,
    line: u32,
    column: u32,
    code: u32,
    message: String,
}

#[derive(Serialize)]
struct Report {
    site: String,
    tsconfig: Option<String>,
    framework: Option<String>,
    files: usize,
    /// Errors in the site's own files (and errors with no file).
    errors: usize,
    /// Errors in files outside the site (shared code it imports), which
    /// every site importing them reports again.
    external_errors: usize,
    by_code: BTreeMap<String, usize>,
    diagnostics: Vec<Diag>,
    elapsed_ms: u128,
    /// Why nothing was checked (no tsconfig), or why the check failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<String>,
}

pub fn run(args: &[String]) -> i32 {
    let mut sites: Vec<PathBuf> = Vec::new();
    let mut framework: Option<PathBuf> = None;
    let mut json = false;
    let mut max = 20usize;
    let mut jobs: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--framework" => {
                i += 1;
                framework = args.get(i).map(PathBuf::from);
            }
            "--json" => json = true,
            "--max" => {
                i += 1;
                max = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(max);
            }
            "--jobs" => {
                i += 1;
                jobs = args.get(i).and_then(|v| v.parse().ok()).filter(|&n| n > 0);
            }
            other if !other.starts_with('-') => sites.push(PathBuf::from(other)),
            other => {
                eprintln!("check-site: unknown argument {other}");
                return 2;
            }
        }
        i += 1;
    }
    let sites: Option<Vec<PathBuf>> = sites
        .iter()
        .map(|s| std::fs::canonicalize(s).ok())
        .collect();
    let Some(sites) = sites.filter(|sites| !sites.is_empty()) else {
        eprintln!(
            "usage: tsc-rs check-site <site-dir>... [--framework <dir>] [--json] [--max <n>] [--jobs <n>]"
        );
        return 2;
    };
    if let [site] = &sites[..] {
        let report = check_site(site, framework, max);
        print(&report, json);
        return 0;
    }
    let jobs = jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get() / 4)
            .max(1)
    });
    for report in check_sites(&sites, framework, max, jobs) {
        if !json {
            println!("{}:", report.site);
        }
        print(&report, json);
    }
    0
}

fn new_report(site: &Path) -> Report {
    Report {
        site: site.display().to_string(),
        tsconfig: None,
        framework: None,
        files: 0,
        errors: 0,
        external_errors: 0,
        by_code: BTreeMap::new(),
        diagnostics: Vec::new(),
        elapsed_ms: 0,
        skipped: None,
    }
}

/// The report of one site.
fn check_site(site: &Path, framework: Option<PathBuf>, max: usize) -> Report {
    let start = std::time::Instant::now();
    let mut report = new_report(site);
    let result =
        prepare(site, framework, &mut report).and_then(|check| check.run(max, &mut report));
    if let Err(reason) = result {
        report.skipped = Some(reason);
    }
    report.elapsed_ms = start.elapsed().as_millis();
    report
}

/// The reports of `sites`, in their order, `jobs` checks at a time. Each
/// check is the child process that a lone site runs, so each report is the
/// one `check_site` gives. The largest checks (by file count) start first,
/// so a large site does not start last and end alone.
fn check_sites(
    sites: &[PathBuf],
    framework: Option<PathBuf>,
    max: usize,
    jobs: usize,
) -> Vec<Report> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    let jobs = jobs.min(sites.len()).max(1);
    // Each site's report, its prepared check and the time it took.
    type Slot = (Report, Option<Result<Check, String>>, std::time::Duration);
    let slots: Vec<Mutex<Slot>> = sites
        .iter()
        .map(|site| Mutex::new((new_report(site), None, std::time::Duration::ZERO)))
        .collect();
    let lock = |i: usize| slots[i].lock().unwrap_or_else(|e| e.into_inner());
    let next = AtomicUsize::new(0);
    let pool = |work: &(dyn Fn(usize) + Sync), order: &[usize]| {
        next.store(0, Ordering::Relaxed);
        std::thread::scope(|scope| {
            for _ in 0..jobs {
                scope.spawn(|| {
                    while let Some(&i) = order.get(next.fetch_add(1, Ordering::Relaxed)) {
                        work(i);
                    }
                });
            }
        });
    };
    let in_order: Vec<usize> = (0..sites.len()).collect();
    pool(
        &|i| {
            let start = std::time::Instant::now();
            let mut slot = lock(i);
            let (report, check, elapsed) = &mut *slot;
            *check = Some(prepare(&sites[i], framework.clone(), report));
            *elapsed = start.elapsed();
        },
        &in_order,
    );
    let mut largest_first = in_order;
    largest_first.sort_by_key(|&i| std::cmp::Reverse(lock(i).0.files));
    pool(
        &|i| {
            let start = std::time::Instant::now();
            let (mut report, check, prepared) = {
                let mut slot = lock(i);
                let check = slot.1.take().expect("prepared");
                (
                    std::mem::replace(&mut slot.0, new_report(&sites[i])),
                    check,
                    slot.2,
                )
            };
            if let Err(reason) = check.and_then(|check| check.run(max, &mut report)) {
                report.skipped = Some(reason);
            }
            report.elapsed_ms = (prepared + start.elapsed()).as_millis();
            lock(i).0 = report;
        },
        &largest_first,
    );
    slots
        .into_iter()
        .map(|slot| slot.into_inner().unwrap_or_else(|e| e.into_inner()).0)
        .collect()
}

fn print(report: &Report, json: bool) {
    if json {
        println!("{}", serde_json::to_string(report).unwrap_or_default());
        return;
    }
    if let Some(reason) = &report.skipped {
        println!("typecheck: skipped ({reason})");
        return;
    }
    let codes: Vec<String> = {
        let mut by: Vec<(&String, &usize)> = report.by_code.iter().collect();
        by.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        by.iter()
            .take(6)
            .map(|(code, n)| format!("{code} x{n}"))
            .collect()
    };
    println!(
        "typecheck: {} error(s) in {} site file(s){}, {} ms{}",
        report.errors,
        report.files,
        if report.external_errors > 0 {
            format!(" (+{} in shared code it imports)", report.external_errors)
        } else {
            String::new()
        },
        report.elapsed_ms,
        if codes.is_empty() {
            String::new()
        } else {
            format!(" ({})", codes.join(", "))
        }
    );
    for d in &report.diagnostics {
        if d.file.is_empty() {
            println!("  TS{}: {}", d.code, d.message);
        } else {
            println!(
                "  {}({},{}): TS{}: {}",
                d.file, d.line, d.column, d.code, d.message
            );
        }
    }
    if report.errors > report.diagnostics.len() {
        println!("  ... {} more", report.errors - report.diagnostics.len());
    }
}

/// A site check ready to run: its temporary tsconfig.
struct Check {
    site: PathBuf,
    temp: PathBuf,
    temp_config_path: PathBuf,
}

/// Fills the site part of `report` and writes the temporary tsconfig of the
/// check.
fn prepare(site: &Path, framework: Option<PathBuf>, report: &mut Report) -> Result<Check, String> {
    let config = site.join("tsconfig.json");
    if !config.is_file() {
        return Err("no tsconfig.json".into());
    }
    report.tsconfig = Some(config.display().to_string());
    let parsed = crate::tsrs::parse_config(&config)?;
    if let Some(errors) = parsed["errors"]
        .as_array()
        .filter(|errors| !errors.is_empty())
    {
        return Err(format!(
            "tsconfig.json: {} error(s): {}",
            errors.len(),
            errors[0]
        ));
    }
    let options = &parsed["options"];

    let linked = site.join("node_modules").join(FRAMEWORK);
    let framework = if linked.is_dir() {
        std::fs::canonicalize(&linked).ok()
    } else {
        framework
            .or_else(|| std::env::var_os("BEXT_FRAMEWORK_DIR").map(PathBuf::from))
            .or_else(|| {
                site.ancestors()
                    .map(|dir| dir.join("shared/framework"))
                    .find(|dir| dir.join("package.json").is_file())
            })
            .and_then(|dir| std::fs::canonicalize(dir).ok())
    };
    report.framework = framework.as_ref().map(|dir| dir.display().to_string());

    let mut files: Vec<String> = parsed["fileNames"]
        .as_array()
        .ok_or("tsrs returned no fileNames")?
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    report.files = files.len();
    let mut uses_framework_jsx = options["jsxImportSource"]
        .as_str()
        .is_some_and(|value| value.starts_with(FRAMEWORK));
    let mut compiler_options = serde_json::Map::new();
    // `"types": ["@bext-stack/framework/jsx"]` does not resolve in tsc: a
    // type library is not looked up through a package's `exports`, and not
    // through `paths` either. Drop the entry and add the file it names.
    if let Some(values) = options["types"].as_array() {
        let types: Vec<&str> = values.iter().filter_map(|value| value.as_str()).collect();
        if types.iter().any(|t| t.starts_with(FRAMEWORK)) {
            uses_framework_jsx = true;
            let kept: Vec<&&str> = types.iter().filter(|t| !t.starts_with(FRAMEWORK)).collect();
            compiler_options.insert(
                "types".into(),
                serde_json::to_value(kept).unwrap_or_default(),
            );
        }
    }
    if let Some(dir) = &framework {
        let jsx_types = dir.join("jsx.d.ts");
        if uses_framework_jsx && jsx_types.is_file() {
            files.push(jsx_types.display().to_string());
        }
    }

    let mut paths: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    if let Some(site_paths) = options["paths"].as_object() {
        let base = options["baseUrl"]
            .as_str()
            .or_else(|| options["pathsBasePath"].as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| site.to_path_buf());
        for (pattern, targets) in site_paths {
            let targets: Vec<String> = targets
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str())
                .map(|t| base.join(t).display().to_string())
                .collect();
            paths.insert(pattern.clone(), targets.into());
        }
    }
    if let (false, Some(dir)) = (linked.is_dir(), &framework) {
        for (pattern, target) in framework_export_paths(dir) {
            paths.entry(pattern).or_insert_with(|| vec![target].into());
        }
    }

    static TEMPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let temp = std::env::temp_dir().join(format!(
        "tsc-rs-check-site-{}-{}-{}",
        std::process::id(),
        start_nanos(),
        TEMPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&temp).map_err(|e| format!("temp dir: {e}"))?;
    if !paths.is_empty() {
        compiler_options.insert("paths".into(), paths.into());
    }
    let temp_config = serde_json::json!({
        "extends": config.display().to_string(),
        "compilerOptions": compiler_options,
        "files": files,
        "include": [],
    });
    let temp_config_path = temp.join("tsconfig.json");
    if let Err(e) = std::fs::write(&temp_config_path, temp_config.to_string()) {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(format!("temp tsconfig: {e}"));
    }
    Ok(Check {
        site: site.to_path_buf(),
        temp,
        temp_config_path,
    })
}

impl Check {
    /// Runs the check and adds its diagnostics to `report`.
    fn run(self, max: usize, report: &mut Report) -> Result<(), String> {
        let output = self.output();
        let _ = std::fs::remove_dir_all(&self.temp);
        read_output(
            output.map_err(|e| format!("spawn check: {e}"))?,
            max,
            report,
        )
    }

    fn output(&self) -> std::io::Result<std::process::Output> {
        let exe = crate::tsrs::binary().map_err(std::io::Error::other)?;
        let mut command = Command::new(exe);
        command
            .current_dir(&self.site)
            .arg("-p")
            .arg(&self.temp_config_path)
            .args([
                "--noEmit",
                "--incremental",
                "false",
                "--pretty",
                "false",
                "--checkers",
                "4",
                "--maxMemory",
                "1G",
                "--checkerAssignment",
                "go",
                "--noLazyMembers",
            ])
            .env("RAYON_NUM_THREADS", "4");
        command.output()
    }
}

/// Adds the diagnostics of a check's output to `report`.
fn read_output(
    output: std::process::Output,
    max: usize,
    report: &mut Report,
) -> Result<(), String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let Some(diag) = parse_diagnostic(line) else {
            continue;
        };
        // Paths are relative to the site (the check's directory).
        if diag.file.starts_with("../") || diag.file.starts_with('/') {
            report.external_errors += 1;
            continue;
        }
        report.errors += 1;
        *report
            .by_code
            .entry(format!("TS{}", diag.code))
            .or_default() += 1;
        if report.diagnostics.len() < max {
            report.diagnostics.push(diag);
        }
    }
    // The check exits 1 or 2 with errors; anything else without
    // diagnostics (a crash) is a failure to check.
    let code = output.status.code();
    if !matches!(code, Some(0..=2))
        || (report.errors + report.external_errors == 0 && !matches!(code, Some(0)))
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stdout
            .lines()
            .chain(stderr.lines())
            .find(|l| !l.trim().is_empty());
        return Err(format!(
            "check failed (exit {code:?}){}",
            first.map(|l| format!(": {l}")).unwrap_or_default()
        ));
    }
    Ok(())
}

fn start_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// `paths` entries for the framework's `package.json` exports:
/// `"./jsx-runtime": "./src/jsx.ts"` -> `@bext-stack/framework/jsx-runtime`.
fn framework_export_paths(dir: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(package) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(exports) = package.get("exports").and_then(|e| e.as_object()) else {
        return Vec::new();
    };
    exports
        .iter()
        .filter_map(|(subpath, target)| {
            let target = match target {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(conditions) => conditions
                    .get("types")
                    .or_else(|| conditions.get("import"))
                    .or_else(|| conditions.get("default"))
                    .and_then(|v| v.as_str())?
                    .to_string(),
                _ => return None,
            };
            let pattern = match subpath.strip_prefix('.') {
                Some("") => FRAMEWORK.to_string(),
                Some(rest) => format!("{FRAMEWORK}{rest}"),
                None => return None,
            };
            Some((
                pattern,
                dir.join(target.trim_start_matches("./"))
                    .display()
                    .to_string(),
            ))
        })
        .collect()
}

/// `file(line,col): error TS1234: message`, or `error TS1234: message`
/// for an error with no file (an option or a missing type library).
fn parse_diagnostic(line: &str) -> Option<Diag> {
    if let Some(rest) = line.strip_prefix("error TS") {
        let (code, message) = rest.split_once(": ")?;
        return Some(Diag {
            file: String::new(),
            line: 0,
            column: 0,
            code: code.parse().ok()?,
            message: message.to_string(),
        });
    }
    let (location, rest) = line.split_once("): error TS")?;
    let (file, position) = location.rsplit_once('(')?;
    let (line_no, column) = position.split_once(',')?;
    let (code, message) = rest.split_once(": ")?;
    Some(Diag {
        file: file.to_string(),
        line: line_no.parse().ok()?,
        column: column.parse().ok()?,
        code: code.parse().ok()?,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn crashed_checker_with_partial_diagnostics_is_not_a_completed_check() {
        use std::os::unix::process::ExitStatusExt;
        let mut report = new_report(Path::new("/tmp/site"));
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(9),
            stdout: b"src/page.ts(1,1): error TS2322: partial result\n".to_vec(),
            stderr: Vec::new(),
        };
        assert!(read_output(output, 20, &mut report).is_err());
        assert_eq!(report.errors, 1);
    }

    #[test]
    fn parses_tsc_diagnostic_lines() {
        let d = parse_diagnostic(
            "src/a (1).tsx(12,5): error TS2322: Type 'string' is not assignable to type 'number'.",
        )
        .unwrap();
        assert_eq!(
            (d.file.as_str(), d.line, d.column, d.code),
            ("src/a (1).tsx", 12, 5, 2322)
        );
        assert_eq!(
            d.message,
            "Type 'string' is not assignable to type 'number'."
        );
        assert!(parse_diagnostic("Found 3 errors in 2 files.").is_none());
        let g =
            parse_diagnostic("error TS2688: Cannot find type definition file for 'x'.").unwrap();
        assert_eq!((g.file.as_str(), g.code), ("", 2688));
    }

    #[test]
    fn several_sites_report_in_argument_order() {
        let base = std::env::temp_dir().join(format!("check-sites-test-{}", std::process::id()));
        let sites: Vec<PathBuf> = (0..5).map(|i| base.join(format!("site{i}"))).collect();
        for site in &sites {
            std::fs::create_dir_all(site).unwrap();
        }
        let reports = check_sites(&sites, None, 20, 3);
        let _ = std::fs::remove_dir_all(&base);
        let names: Vec<&str> = reports.iter().map(|r| r.site.as_str()).collect();
        let expected: Vec<String> = sites.iter().map(|s| s.display().to_string()).collect();
        assert_eq!(names, expected);
        assert!(reports
            .iter()
            .all(|r| r.skipped.as_deref() == Some("no tsconfig.json")));
    }

    #[test]
    fn maps_framework_exports() {
        let dir = std::env::temp_dir().join(format!("check-site-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"exports":{".":"./src/index.ts","./jsx-runtime":"./src/jsx.ts","./x":{"types":"./x.d.ts"}}}"#,
        )
        .unwrap();
        let paths = framework_export_paths(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let find = |p: &str| paths.iter().find(|(k, _)| k == p).map(|(_, v)| v.clone());
        assert!(find("@bext-stack/framework")
            .unwrap()
            .ends_with("src/index.ts"));
        assert!(find("@bext-stack/framework/jsx-runtime")
            .unwrap()
            .ends_with("src/jsx.ts"));
        assert!(find("@bext-stack/framework/x").unwrap().ends_with("x.d.ts"));
    }
}
