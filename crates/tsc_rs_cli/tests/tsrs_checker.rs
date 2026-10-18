use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tsrs-checker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, relative: &str, source: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tsc-rs"));
    command
        .env_remove("TSC_RS_CHECKER")
        .env_remove("TSC_RS_EMIT");
    command
}

#[test]
fn site_check_preserves_inherited_paths_jsx_types_and_owned_diagnostics() {
    let fixture = Fixture::new();
    fixture.write("base.json", r#"{"compilerOptions":{"strict":true,"target":"ES2022","module":"ESNext","moduleResolution":"bundler","jsx":"react-jsx","jsxImportSource":"@bext-stack/framework","types":["@bext-stack/framework/jsx"],"paths":{"@/*":["./site/src/*"]}}}"#);
    fixture.write(
        "site/tsconfig.json",
        r#"{"extends":"../base.json","files":["src/input.tsx"]}"#,
    );
    fixture.write("site/src/input.tsx", "import { shared } from '../../shared'; import { value } from '@/value'; export const invalid: number = 'wrong'; export const view = <main>{shared + value}</main>;\n");
    fixture.write("site/src/value.ts", "export const value = 1;\n");
    fixture.write("shared.ts", "export const shared: number = 'wrong';\n");
    fixture.write(
        "framework/package.json",
        r#"{"exports":{"./jsx-runtime":"./jsx.ts"}}"#,
    );
    fixture.write(
        "framework/jsx.d.ts",
        "declare namespace JSX { interface IntrinsicElements { main: { children?: unknown }; } }\n",
    );
    fixture.write("framework/jsx.ts", "export function jsx(tag: string, props: any): string { return ''; } export { jsx as jsxs };\n");
    let result = command()
        .arg("check-site")
        .arg(fixture.0.join("site"))
        .arg("--framework")
        .arg(fixture.0.join("framework"))
        .args(["--json", "--max", "1"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(report.get("skipped").is_none(), "{report}");
    assert_eq!(report["errors"], 1, "{report}");
    assert_eq!(report["external_errors"], 1, "{report}");
    assert_eq!(report["diagnostics"][0]["code"], 2322);
    assert!(!fixture.0.join("site/src/input.js").exists());
}

#[test]
fn external_checker_forces_no_emit_even_if_caller_disables_it() {
    let fixture = Fixture::new();
    fixture.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"target":"ES2022","outDir":"out"},"files":["input.ts"]}"#,
    );
    fixture.write("input.ts", "export const count: number = 'wrong';\n");
    let output = command()
        .current_dir(&fixture.0)
        .args([
            "--checker",
            "tsrs",
            "-p",
            "tsconfig.json",
            "--noEmit",
            "false",
            "--pretty",
            "false",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("error TS2322:"),
        "{output:?}"
    );
    assert!(!fixture.0.join("out").exists());
}

#[test]
fn missing_checker_is_reported_as_skipped_instead_of_a_clean_check() {
    let fixture = Fixture::new();
    fixture.write("tsconfig.json", r#"{"files":["input.ts"]}"#);
    fixture.write("input.ts", "export const x = 1;\n");
    let output = command()
        .env("TSRS_BINARY", fixture.0.join("absent"))
        .arg("check-site")
        .arg(&fixture.0)
        .arg("--json")
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["skipped"].as_str().unwrap().contains("TSRS_BINARY"),
        "{report}"
    );
}

#[test]
fn native_pipe_is_independent_of_the_external_checker() {
    let request = serde_json::json!({"id":"native","file":"/tmp/native.tsx",
        "source":"export const url = import.meta.url; export const view = <main>Hello</main>;",
        "options":{"target":"es2022","module":"commonjs","jsx":"react-jsx","jsxImportSource":"@bext-stack/framework","sourceMap":true,"fastEmit":true}});
    let mut child = command()
        .arg("--pipe")
        .env("TSRS_BINARY", "/definitely/missing/tsrs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines = String::from_utf8(output.stdout).unwrap();
    let response: serde_json::Value = serde_json::from_str(lines.lines().last().unwrap()).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    let javascript = response["output"].as_str().unwrap();
    assert!(javascript.contains("@bext-stack/framework/jsx-runtime"));
    assert!(javascript.contains("pathToFileURL(__filename)"));
    assert!(response["sourceMap"].is_string());
}

#[test]
fn removed_backend_requests_fail_loudly() {
    let output = command()
        .args(["--checker", "goport", "--version"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("removed"));
    let output = command()
        .env("TSC_RS_EMIT", "goport")
        .arg("--pipe")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("external emit backends were removed"));
}

#[cfg(unix)]
#[test]
fn external_checker_keeps_the_launchers_pid_and_signal_status() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    let fixture = Fixture::new();
    let checker = fixture.0.join("checker");
    fixture.write(
        "checker",
        "#!/bin/sh\nprintf '%s\\n' \"$$\"\nkill -TERM \"$$\"\n",
    );
    std::fs::set_permissions(&checker, std::fs::Permissions::from_mode(0o755)).unwrap();
    let child = command()
        .args(["--checker", "tsrs"])
        .env("TSRS_BINARY", &checker)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let launched_pid = child.id();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        launched_pid.to_string()
    );
    assert_eq!(output.status.signal(), Some(libc::SIGTERM));
}

#[test]
fn batched_configs_keep_project_relative_paths_and_recover_after_invalid_config() {
    let fixture = Fixture::new();
    for (name, value) in [("first", "1"), ("second", "'wrong'")] {
        fixture.write(
            &format!("{name}/tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"target":"ES2022","module":"ESNext","moduleResolution":"bundler","paths":{"@/*":["./src/*"]}},"include":["src/*.ts"]}"#,
        );
        fixture.write(
            &format!("{name}/src/value.ts"),
            &format!("export const value = {value};\n"),
        );
        fixture.write(
            &format!("{name}/src/input.ts"),
            "import { value } from '@/value'; export const result: number = value;\n",
        );
    }
    fixture.write("invalid/tsconfig.json", "{ invalid");
    let output = command()
        .arg("check-site")
        .arg(fixture.0.join("invalid"))
        .arg(fixture.0.join("first"))
        .arg(fixture.0.join("second"))
        .args(["--json", "--jobs", "1"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let reports: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(reports[0]["skipped"].is_string(), "{reports:?}");
    assert!(reports[1].get("skipped").is_none(), "{reports:?}");
    assert!(reports[2].get("skipped").is_none(), "{reports:?}");
    assert_eq!(reports[1]["errors"], 0, "{reports:?}");
    assert_eq!(reports[2]["errors"], 1, "{reports:?}");
    assert_eq!(reports[2]["diagnostics"][0]["code"], 2322, "{reports:?}");
}

#[cfg(unix)]
#[test]
fn configuration_api_is_reaped_after_single_and_batched_checks() {
    use std::os::unix::fs::PermissionsExt;

    let version = include_str!("../../../TSRS_VERSION").trim();
    let native = std::env::var_os("TSRS_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let system = PathBuf::from("/usr/local/share/tsc-rs/tsrs")
                .join(version)
                .join("tsrs");
            if system.is_file() {
                system
            } else {
                PathBuf::from(std::env::var_os("HOME").expect("HOME"))
                    .join(".local/share/tsc-rs/tsrs")
                    .join(version)
                    .join("tsrs")
            }
        });
    assert!(native.is_file(), "install the pinned native tsrs checker");
    let fixture = Fixture::new();
    for name in ["first", "second"] {
        fixture.write(
            &format!("{name}/tsconfig.json"),
            r#"{"files":["input.ts"]}"#,
        );
        fixture.write(&format!("{name}/input.ts"), "export const value = 1;\n");
    }
    fixture.write("proxy", "#!/bin/sh\nif [ \"$1\" = --api ]; then printf '%s\\n' \"$$\" >> \"$API_PID_LOG\"; fi\nexec \"$NATIVE_TSRS\" \"$@\"\n");
    let proxy = fixture.0.join("proxy");
    fs::set_permissions(&proxy, fs::Permissions::from_mode(0o755)).unwrap();
    let log = fixture.0.join("api-pids");
    for batch in [false, true] {
        let mut cmd = command();
        cmd.env("TSRS_BINARY", &proxy)
            .env("NATIVE_TSRS", &native)
            .env("API_PID_LOG", &log)
            .arg("check-site")
            .arg(fixture.0.join("first"));
        if batch {
            cmd.arg(fixture.0.join("second"));
        }
        let output = cmd.args(["--json", "--jobs", "1"]).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        for line in String::from_utf8(output.stdout).unwrap().lines() {
            let report: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(report.get("skipped").is_none(), "{report}");
        }
        let pids = fs::read_to_string(&log).unwrap();
        assert_eq!(pids.lines().count(), if batch { 2 } else { 1 });
        for pid in pids.lines() {
            let pid: libc::pid_t = pid.parse().unwrap();
            // SAFETY: signal 0 checks existence without sending a signal.
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "API process {pid} survived CLI exit"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
    }
}
