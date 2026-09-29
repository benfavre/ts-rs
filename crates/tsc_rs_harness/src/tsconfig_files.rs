//! Root-file selection for a test case that carries a `tsconfig.json`.
//!
//! TypeScript's compiler test runner parses the config's `files`, `include`
//! and `exclude` against the virtual file system and treats the matched
//! units as the compilation roots; every other unit is merely present on
//! disk. The error baseline lists the config first, then the roots (in
//! unit order), then the remaining units (in unit order).

use super::*;

const COMMON_PACKAGE_FOLDERS: [&str; 3] = ["node_modules", "bower_components", "jspm_packages"];

/// Remove `//` and `/* */` comments outside string literals.
pub(crate) fn strip_json_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn resolve_spec_path(config_dir: &str, spec: &str) -> String {
    let spec = normalize_header_path(spec);
    if is_absolute_path(&spec) || config_dir.is_empty() {
        normalize_path_segments(&spec)
    } else {
        normalize_path_segments(&join_path(config_dir, &spec))
    }
}

fn relative_to_config_dir(path: &str, config_dir: &str) -> Option<String> {
    if config_dir.is_empty() {
        return (!is_absolute_path(path)).then(|| path.to_string());
    }
    strip_prefix_path(path, config_dir)
}

/// The spec as a config-relative path: absolute specs are stripped of the
/// config directory, `./` prefixes and trailing slashes are dropped.
fn spec_relative(config_dir: &str, spec: &str) -> Option<String> {
    let spec = normalize_header_path(spec);
    let spec = spec.trim_end_matches('/');
    if is_absolute_path(spec) {
        if config_dir.is_empty() {
            return None;
        }
        return strip_prefix_path(&normalize_path_segments(spec), config_dir);
    }
    Some(normalize_path_segments(spec))
}

fn has_wildcard(spec: &str) -> bool {
    spec.contains('*') || spec.contains('?')
}

fn is_common_package_folder(segment: &str) -> bool {
    COMMON_PACKAGE_FOLDERS
        .iter()
        .any(|folder| folder.eq_ignore_ascii_case(segment))
}

fn segment_matches(pattern: &str, segment: &str) -> bool {
    fn rec(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|k| rec(&p[1..], &s[k..])),
            Some('?') => !s.is_empty() && rec(&p[1..], &s[1..]),
            Some(c) => s.first() == Some(c) && rec(&p[1..], &s[1..]),
        }
    }
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let s: Vec<char> = segment.to_ascii_lowercase().chars().collect();
    rec(&p, &s)
}

/// Segment-wise glob match. `**` spans any number of directories, but never
/// a common package folder or a dot-directory; a wildcard segment never
/// matches a common package folder. With `prefix_ok` the pattern may stop
/// at any directory boundary (exclude semantics).
fn glob_segments(pattern: &[&str], path: &[&str], prefix_ok: bool) -> bool {
    if pattern.is_empty() {
        return path.is_empty() || prefix_ok;
    }
    if pattern[0] == "**" {
        for skip in 0..=path.len() {
            if glob_segments(&pattern[1..], &path[skip..], prefix_ok) {
                return true;
            }
            if skip < path.len()
                && (is_common_package_folder(path[skip]) || path[skip].starts_with('.'))
            {
                break;
            }
        }
        return false;
    }
    let Some(first) = path.first() else {
        return false;
    };
    if !segment_matches(pattern[0], first) {
        return false;
    }
    if has_wildcard(pattern[0]) && is_common_package_folder(first) {
        return false;
    }
    glob_segments(&pattern[1..], &path[1..], prefix_ok)
}

fn spec_excludes(config_dir: &str, spec: &str, rel: &str) -> bool {
    let Some(spec) = spec_relative(config_dir, spec) else {
        return false;
    };
    if spec.is_empty() {
        return false;
    }
    if has_wildcard(&spec) {
        let pattern: Vec<&str> = spec.split('/').collect();
        let path: Vec<&str> = rel.split('/').collect();
        return glob_segments(&pattern, &path, true);
    }
    let lower_rel = rel.to_ascii_lowercase();
    let lower_spec = spec.to_ascii_lowercase();
    lower_rel == lower_spec || lower_rel.starts_with(&format!("{lower_spec}/"))
}

/// Whether `rel` is picked up by an include spec. Returns `Some(true)` for a
/// literal (non-wildcard) file entry, `Some(false)` for a wildcard match.
fn spec_includes(config_dir: &str, spec: &str, rel: &str) -> Option<bool> {
    let spec = spec_relative(config_dir, spec)?;
    let path: Vec<&str> = rel.split('/').collect();
    if has_wildcard(&spec) {
        let pattern: Vec<&str> = spec.split('/').collect();
        return glob_segments(&pattern, &path, false).then_some(false);
    }
    if spec.is_empty() {
        let pattern = ["**", "*"];
        return glob_segments(&pattern, &path, false).then_some(false);
    }
    if rel.eq_ignore_ascii_case(&spec) {
        return Some(true);
    }
    let mut pattern: Vec<&str> = spec.split('/').collect();
    pattern.push("**");
    pattern.push("*");
    glob_segments(&pattern, &path, false).then_some(false)
}

/// Extensions in TypeScript's priority order; a wildcard-matched file loses
/// to a sibling with the same stem and an earlier extension.
const EXTENSION_PRIORITY: [&str; 11] = [
    ".ts", ".tsx", ".d.ts", ".cts", ".d.cts", ".mts", ".d.mts", ".js", ".jsx", ".cjs", ".mjs",
];

fn split_source_extension(lower_path: &str) -> Option<(&str, &str)> {
    for ext in [".d.ts", ".d.cts", ".d.mts"] {
        if let Some(stem) = lower_path.strip_suffix(ext) {
            return Some((stem, ext));
        }
    }
    EXTENSION_PRIORITY
        .iter()
        .find_map(|ext| lower_path.strip_suffix(ext).map(|stem| (stem, *ext)))
}

fn has_supported_extension(lower_path: &str, allow_js: bool) -> bool {
    match split_source_extension(lower_path) {
        Some((_, ext)) => allow_js || !matches!(ext, ".js" | ".jsx" | ".cjs" | ".mjs"),
        None => false,
    }
}

/// Indices (in unit order) of the units that the test's `tsconfig.json`
/// selects as compilation roots. `None` without a config file.
pub(crate) fn tsconfig_root_file_indices(test_case: &TestCase) -> Option<Vec<usize>> {
    let tsconfig = find_tsconfig_file(test_case)?;
    let content = strip_json_comments(&tsconfig.content);
    let config_dir = dirname(&normalize_header_path(&tsconfig.name));
    let compiler = extract_json_object(&content, "compilerOptions").unwrap_or_default();
    let options = &test_case.options;
    let allow_js = options.allow_js == Some(true)
        || options.check_js == Some(true)
        || extract_json_bool(&compiler, "allowJs") == Some(true)
        || extract_json_bool(&compiler, "checkJs") == Some(true);
    let out_dir = options
        .out_dir
        .clone()
        .or_else(|| extract_json_string(&compiler, "outDir"));
    let declaration_dir = options
        .other
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("declarationdir"))
        .map(|(_, value)| value.clone())
        .or_else(|| extract_json_string(&compiler, "declarationDir"));
    let files = extract_json_string_array(&content, "files");
    let include = extract_json_string_array(&content, "include");
    let exclude = extract_json_string_array(&content, "exclude");
    let include_specs: Vec<String> = match (&files, &include) {
        (Some(_), None) => Vec::new(),
        (_, Some(specs)) => specs.clone(),
        (None, None) => vec!["**/*".to_string()],
    };
    let exclude_specs: Vec<String> = match &exclude {
        Some(specs) => specs.clone(),
        None => {
            let mut specs: Vec<String> = COMMON_PACKAGE_FOLDERS
                .iter()
                .map(|folder| folder.to_string())
                .collect();
            specs.extend(out_dir.iter().cloned());
            specs.extend(declaration_dir.iter().cloned());
            specs
        }
    };
    let literal_files: Vec<String> = files
        .unwrap_or_default()
        .iter()
        .map(|file| resolve_spec_path(&config_dir, file))
        .collect();

    let mut roots: Vec<usize> = Vec::new();
    let mut wildcard_matches: Vec<(usize, String)> = Vec::new();
    for (idx, file) in test_case.files.iter().enumerate() {
        let path = normalize_path_segments(&normalize_header_path(&file.name));
        if basename(&path).eq_ignore_ascii_case("tsconfig.json") {
            continue;
        }
        if literal_files
            .iter()
            .any(|literal| literal.eq_ignore_ascii_case(&path))
        {
            roots.push(idx);
            continue;
        }
        let Some(rel) = relative_to_config_dir(&path, &config_dir) else {
            continue;
        };
        if rel.is_empty() {
            continue;
        }
        if exclude_specs
            .iter()
            .any(|spec| spec_excludes(&config_dir, spec, &rel))
        {
            continue;
        }
        let lower = path.to_ascii_lowercase();
        let mut included = None;
        for spec in &include_specs {
            match spec_includes(&config_dir, spec, &rel) {
                Some(true) => {
                    included = Some(true);
                    break;
                }
                Some(false) if has_supported_extension(&lower, allow_js) => {
                    included = Some(false);
                }
                _ => {}
            }
        }
        match included {
            Some(true) => roots.push(idx),
            Some(false) => wildcard_matches.push((idx, lower)),
            None => {}
        }
    }
    let lower_paths: Vec<&str> = wildcard_matches.iter().map(|(_, p)| p.as_str()).collect();
    for (idx, lower) in &wildcard_matches {
        let shadowed = split_source_extension(lower).is_some_and(|(stem, ext)| {
            EXTENSION_PRIORITY
                .iter()
                .take_while(|candidate| **candidate != ext)
                .any(|candidate| lower_paths.contains(&format!("{stem}{candidate}").as_str()))
        });
        if !shadowed {
            roots.push(*idx);
        }
    }
    roots.sort_unstable();
    roots.dedup();
    Some(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_line_and_block_comments_outside_strings() {
        let src = "{ // c\n \"a\": \"x//y\", /* b */ \"files\": [\"f.ts\"] }";
        assert_eq!(
            strip_json_comments(src),
            "{ \n \"a\": \"x//y\",  \"files\": [\"f.ts\"] }"
        );
    }

    #[test]
    fn globs_skip_common_package_folders_and_dot_directories() {
        let p = ["**", "*"];
        assert!(glob_segments(&p, &["src", "a.ts"], false));
        assert!(!glob_segments(&p, &["node_modules", "a", "a.ts"], false));
        assert!(!glob_segments(&p, &[".hidden", "a.ts"], false));
        assert!(glob_segments(
            &["src", "**", "*.ts"],
            &["src", "x", "a.ts"],
            false
        ));
        assert!(glob_segments(&["bin"], &["bin", "a.ts"], true));
    }
}
