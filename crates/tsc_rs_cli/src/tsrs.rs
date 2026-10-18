//! External tsrs is used for checking and configuration parsing only.
//! Transpilation stays in our native parser/emitter, with no upstream AST IPC.
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub fn package_version() -> &'static str {
    include_str!("../../../TSRS_VERSION").trim()
}

pub fn binary() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("TSRS_BINARY") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "TSRS_BINARY does not name a file: {}",
            path.display()
        ));
    }
    // Deployment sandboxes use a fresh HOME. A system install stays discoverable
    // there without inheriting an operator's user-specific environment.
    let system = PathBuf::from("/usr/local/share/tsc-rs/tsrs")
        .join(package_version())
        .join("tsrs");
    if system.is_file() {
        return Ok(system);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let installed = PathBuf::from(home)
            .join(".local/share/tsc-rs/tsrs")
            .join(package_version())
            .join("tsrs");
        if installed.is_file() {
            return Ok(installed);
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            let candidate = directory.join("tsrs");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join("tsrs");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(format!(
        "tsrs {} is not installed; run scripts/install-tsrs.py or set TSRS_BINARY to the native executable",
        package_version()
    ))
}

pub fn checker_arguments(
    args: &[String],
    environment: Option<&str>,
) -> Result<Option<Vec<String>>, String> {
    let mut selected = environment.map(str::to_owned);
    let mut forwarded = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--checker" {
            index += 1;
            selected = Some(args.get(index).ok_or("--checker needs a value")?.clone());
        } else if let Some(value) = args[index].strip_prefix("--checker=") {
            selected = Some(value.to_string());
        } else {
            forwarded.push(args[index].clone());
        }
        index += 1;
    }
    match selected.as_deref() {
        None | Some("") => Ok(None),
        Some("tsrs") => Ok(Some(forwarded)),
        Some("goport") => {
            Err("the vendored ts-rust backend was removed; use --checker tsrs for checking".into())
        }
        Some(other) => Err(format!(
            "unknown checker {other:?}; use tsrs or leave --checker unset for the native compiler"
        )),
    }
}

pub fn run_checker(mut args: Vec<String>) -> Result<i32, String> {
    // Selecting a checker must never silently select upstream transpilation.
    if let Some(index) = args.iter().position(|arg| arg == "--lsp") {
        // Upstream chooses the LSP entry point from its first argument.
        args.remove(index);
        args.insert(0, "--lsp".into());
    } else {
        args.extend(["--noEmit".into(), "true".into()]);
    }
    let mut command = Command::new(binary()?);
    command.args(args);
    // Guarded development checks monitor this PID's RSS and send it signals.
    // Replace the launcher so those controls observe the actual checker.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(format!("start tsrs checker: {}", command.exec()))
    }
    #[cfg(not(unix))]
    command
        .status()
        .map(|status| status.code().unwrap_or(1))
        .map_err(|error| format!("start tsrs checker: {error}"))
}

// TypeScript 7's synchronous API uses MessagePack [type, binary method,
// binary JSON payload]. Only initialize and parseConfigFile are needed here.
// The peer is our pinned native executable, not a network endpoint.
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

fn write_binary(writer: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(&[0xc6])?;
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(bytes)
}

fn read_byte(reader: &mut impl Read) -> Result<u8, String> {
    let mut value = [0];
    reader
        .read_exact(&mut value)
        .map_err(|error| format!("read tsrs API: {error}"))?;
    Ok(value[0])
}

fn read_binary(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let tag = read_byte(reader)?;
    let size = match tag {
        0xc4 => read_byte(reader)? as usize,
        0xc5 => ((read_byte(reader)? as usize) << 8) | read_byte(reader)? as usize,
        0xc6 => {
            let mut bytes = [0; 4];
            reader
                .read_exact(&mut bytes)
                .map_err(|error| error.to_string())?;
            u32::from_be_bytes(bytes) as usize
        }
        _ => return Err(format!("unexpected tsrs API binary tag {tag:#x}")),
    };
    if size > MAX_FRAME_BYTES {
        return Err("tsrs API response exceeded 64 MiB".into());
    }
    let mut bytes = vec![0; size];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn request(
    writer: &mut impl Write,
    reader: &mut impl Read,
    method: &str,
    payload: &[u8],
) -> Result<serde_json::Value, String> {
    writer
        .write_all(&[0x93, 1])
        .and_then(|_| write_binary(writer, method.as_bytes()))
        .and_then(|_| write_binary(writer, payload))
        .and_then(|_| writer.flush())
        .map_err(|error| format!("write tsrs API: {error}"))?;
    if read_byte(reader)? != 0x93 {
        return Err("unexpected tsrs API tuple header".into());
    }
    let tag = read_byte(reader)?;
    let tag = if tag == 0xcc { read_byte(reader)? } else { tag };
    let name = read_binary(reader)?;
    let response = read_binary(reader)?;
    if name != method.as_bytes() {
        return Err("tsrs API response method mismatch".into());
    }
    match tag {
        4 => serde_json::from_slice(&response).map_err(|error| format!("decode tsrs API: {error}")),
        5 => Err(String::from_utf8_lossy(&response).into_owned()),
        _ => Err(format!("unexpected tsrs API response type {tag}")),
    }
}

struct ApiChild(Child);

impl Drop for ApiChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) struct ConfigApi {
    _child: ApiChild,
    input: std::process::ChildStdin,
    output: BufReader<std::process::ChildStdout>,
}

impl ConfigApi {
    fn new() -> Result<Self, String> {
        // Config paths are absolute. A neutral working directory lets one
        // worker parse several projects without carrying a site's cwd over.
        let mut child = ApiChild(
            Command::new(binary()?)
                .args(["--api", "--cwd"])
                .arg(std::env::temp_dir())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|error| format!("start tsrs configuration API: {error}"))?,
        );
        let mut input = child
            .0
            .stdin
            .take()
            .ok_or("tsrs API stdin is unavailable")?;
        let mut output = BufReader::new(
            child
                .0
                .stdout
                .take()
                .ok_or("tsrs API stdout is unavailable")?,
        );
        request(&mut input, &mut output, "initialize", b"null")?;
        Ok(Self {
            _child: child,
            input,
            output,
        })
    }

    fn parse(&mut self, config: &Path) -> Result<serde_json::Value, String> {
        let payload = serde_json::to_vec(&serde_json::json!({ "file": config }))
            .map_err(|error| error.to_string())?;
        request(
            &mut self.input,
            &mut self.output,
            "parseConfigFile",
            &payload,
        )
    }
}

pub fn parse_config(
    config: &Path,
    api: &mut Option<ConfigApi>,
) -> Result<serde_json::Value, String> {
    let config =
        std::fs::canonicalize(config).map_err(|error| format!("tsconfig path: {error}"))?;
    if api.is_none() {
        *api = Some(ConfigApi::new()?);
    }
    let result = api
        .as_mut()
        .expect("initialized configuration API")
        .parse(&config);
    if result.is_err() {
        // A failed transport cannot poison the worker's next project.
        api.take();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_only_supported_checker_names() {
        let args = vec!["--checker".into(), "tsrs".into(), "-p".into(), "app".into()];
        assert_eq!(
            checker_arguments(&args, None).unwrap().unwrap(),
            vec!["-p", "app"]
        );
        assert!(checker_arguments(&[], Some("goport"))
            .unwrap_err()
            .contains("removed"));
        assert!(checker_arguments(&["--checker".into()], None).is_err());
        assert!(checker_arguments(&[], None).unwrap().is_none());
    }

    #[test]
    fn api_frames_validate_method_type_and_length() {
        let mut response = vec![0x93, 4];
        write_binary(&mut response, b"initialize").unwrap();
        write_binary(&mut response, br#"{"ok":true}"#).unwrap();
        let mut written = Vec::new();
        assert_eq!(
            request(
                &mut written,
                &mut response.as_slice(),
                "initialize",
                b"null"
            )
            .unwrap()["ok"],
            true
        );
        assert_eq!(&written[..2], &[0x93, 1]);
        assert!(request(&mut Vec::new(), &mut response.as_slice(), "wrong", b"null").is_err());
        let mut oversized = vec![0xc6];
        oversized.extend(((MAX_FRAME_BYTES + 1) as u32).to_be_bytes());
        assert!(read_binary(&mut oversized.as_slice())
            .unwrap_err()
            .contains("64 MiB"));
    }
}
