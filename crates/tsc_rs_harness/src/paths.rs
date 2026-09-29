/// Extract the basename (last path component) from a file path.
///
/// TypeScript baselines use only the file name (not the full path from
/// `// @filename:` directives) as the section header.  For example,
/// `node_modules/typescript/package.json` becomes `package.json` and
/// `c:/root/folder1/file1.ts` becomes `file1.ts`.
pub(crate) fn basename(path: &str) -> String {
    // Handle both forward and backward slashes.
    path.rsplit(&['/', '\\'][..])
        .next()
        .unwrap_or(path)
        .to_string()
}

pub(crate) fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

pub(crate) fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\')
}

pub(crate) fn normalize_header_path(path: &str) -> String {
    let mut p = normalize_slashes(path.trim());
    while p.starts_with("./") {
        p = p[2..].to_string();
    }
    p
}

pub(crate) fn is_unix_absolute(path: &str) -> bool {
    path.starts_with('/')
}

pub(crate) fn is_absolute_path(path: &str) -> bool {
    is_unix_absolute(path) || is_windows_absolute(path)
}

pub(crate) fn trim_trailing_slash_preserve_root(path: &str) -> String {
    let mut p = normalize_header_path(path);
    if p == "/" || (is_windows_absolute(&p) && p.len() == 3 && p.ends_with('/')) {
        return p;
    }
    while p.ends_with('/') {
        p.pop();
    }
    if p.is_empty() {
        "/".to_string()
    } else {
        p
    }
}

pub(crate) fn dirname(path: &str) -> String {
    let p = trim_trailing_slash_preserve_root(path);
    if p == "/" || (is_windows_absolute(&p) && p.len() == 3 && p.ends_with('/')) {
        return p;
    }
    if let Some(pos) = p.rfind('/') {
        if pos == 0 {
            "/".to_string()
        } else if pos == 2 && p.as_bytes()[1] == b':' {
            format!("{}/", &p[..2])
        } else {
            p[..pos].to_string()
        }
    } else {
        String::new()
    }
}

pub(crate) fn join_path(base: &str, rel: &str) -> String {
    let rel = normalize_header_path(rel);
    if is_absolute_path(&rel) {
        return normalize_path_segments(&rel);
    }
    let rel = rel.trim_start_matches('/').to_string();
    if rel.is_empty() {
        return trim_trailing_slash_preserve_root(base);
    }
    if base.is_empty() {
        return normalize_path_segments(&rel);
    }
    let mut base = trim_trailing_slash_preserve_root(base);
    if !base.ends_with('/') {
        base.push('/');
    }
    base.push_str(&rel);
    normalize_path_segments(&base)
}

pub(crate) fn normalize_path_segments(path: &str) -> String {
    let p = normalize_header_path(path);
    let (prefix, rest) = if is_windows_absolute(&p) {
        (format!("{}/", &p[..2]), &p[3..])
    } else if is_unix_absolute(&p) {
        ("/".to_string(), &p[1..])
    } else {
        (String::new(), p.as_str())
    };
    let mut stack: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if let Some(last) = stack.last() {
                if *last != ".." {
                    stack.pop();
                    continue;
                }
            }
            if prefix.is_empty() {
                stack.push("..");
            }
            continue;
        }
        stack.push(part);
    }
    if prefix.is_empty() {
        stack.join("/")
    } else if stack.is_empty() {
        prefix
    } else {
        format!("{prefix}{}", stack.join("/"))
    }
}

pub(crate) fn strip_prefix_path(path: &str, prefix: &str) -> Option<String> {
    let p = normalize_header_path(path);
    let mut pre = normalize_header_path(prefix);
    if pre.is_empty() {
        return Some(p);
    }
    if pre == "/" {
        return Some(p.trim_start_matches('/').to_string());
    }
    if pre != "/" {
        pre = pre.trim_end_matches('/').to_string();
    }

    let (p_cmp, pre_cmp) = if is_windows_absolute(&p) && is_windows_absolute(&pre) {
        (p.to_ascii_lowercase(), pre.to_ascii_lowercase())
    } else {
        (p.clone(), pre.clone())
    };
    if p_cmp == pre_cmp {
        return Some(String::new());
    }
    let prefix_slash = format!("{pre_cmp}/");
    if p_cmp.starts_with(&prefix_slash) {
        let cut = pre.len() + 1;
        return Some(p[cut..].to_string());
    }
    None
}

pub(crate) fn split_components(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|c| !c.is_empty())
        .map(|c| c.to_string())
        .collect()
}

pub(crate) fn common_directory(paths: &[String]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let first = &paths[0];
    let first_is_win = is_windows_absolute(first);
    let first_drive = if first_is_win {
        Some(first[..2].to_ascii_lowercase())
    } else {
        None
    };
    for path in &paths[1..] {
        if is_windows_absolute(path) != first_is_win {
            return None;
        }
        if first_is_win {
            let drive = path[..2].to_ascii_lowercase();
            if Some(drive) != first_drive {
                return None;
            }
        } else if is_unix_absolute(path) != is_unix_absolute(first) {
            return None;
        }
    }

    let first_dir = dirname(first);
    let mut common = split_components(&first_dir);
    for path in &paths[1..] {
        let path_dir = dirname(path);
        let comps = split_components(&path_dir);
        let mut i = 0usize;
        while i < common.len() && i < comps.len() && common[i] == comps[i] {
            i += 1;
        }
        common.truncate(i);
    }

    if first_is_win {
        let drive = first_drive.unwrap_or_else(|| "c:".to_string());
        if common.is_empty() {
            Some(format!("{drive}/"))
        } else {
            Some(format!("{drive}/{}", common.join("/")))
        }
    } else if is_unix_absolute(first) {
        if common.is_empty() {
            Some("/".to_string())
        } else {
            Some(format!("/{}", common.join("/")))
        }
    } else if common.is_empty() {
        None
    } else {
        Some(common.join("/"))
    }
}

pub(crate) fn contributes_to_common_source_dir_with_options(
    name: &str,
    include_node_modules: bool,
) -> bool {
    let lower = normalize_header_path(name).to_ascii_lowercase();
    if lower.ends_with("tsconfig.json") {
        return false;
    }
    if !include_node_modules && lower.contains("/node_modules/") {
        return false;
    }
    lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".mts")
        || lower.ends_with(".cts")
        || lower.ends_with(".d.ts")
        || lower.ends_with(".d.tsx")
        || lower.ends_with(".d.mts")
        || lower.ends_with(".d.cts")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
}

pub(crate) fn contributes_to_common_source_dir(name: &str) -> bool {
    contributes_to_common_source_dir_with_options(name, false)
}

/// Compute the relative path from directory `from_dir` to file `to_path`.
/// Both paths should be absolute (starting with `/`).
/// Example: from_dir="/app/bin/src", to_path="/app/myMapRoot/src/index.js.map"
///       → "../../myMapRoot/src/index.js.map"
pub(crate) fn make_relative_path(from_dir: &str, to_path: &str) -> String {
    let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_dir = dirname(to_path);
    let to_file = basename(to_path);
    let to_parts: Vec<&str> = to_dir.split('/').filter(|s| !s.is_empty()).collect();

    // Find common prefix length.
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = from_parts.len() - common;
    let mut result = String::new();
    for _ in 0..ups {
        result.push_str("../");
    }
    for part in &to_parts[common..] {
        result.push_str(part);
        result.push('/');
    }
    result.push_str(&to_file);
    result
}
