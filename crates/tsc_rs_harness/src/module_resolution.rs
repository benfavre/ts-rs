use super::*;

pub(crate) fn resolve_module_specifier(
    importer_name: &str,
    spec: &str,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
) -> Option<usize> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    let importer_dir = dirname(importer_name);
    if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') {
        if spec == "." || spec == "./" {
            let index_base = if importer_dir.is_empty() {
                "index".to_string()
            } else {
                join_path(&importer_dir, "index")
            };
            if let Some(idx) =
                resolve_module_path_base(&index_base, path_to_idx, &ctx.module_suffixes)
            {
                return Some(idx);
            }
            if let Some(idx) = resolve_module_path_via_root_dirs(&index_base, path_to_idx, ctx) {
                return Some(idx);
            }
            return None;
        }
        let base = if spec.starts_with('/') {
            normalize_header_path(spec)
        } else {
            join_path(&importer_dir, spec)
        };
        if let Some(idx) = resolve_module_path_base(&base, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
        if let Some(idx) = resolve_module_path_via_root_dirs(&base, path_to_idx, ctx) {
            return Some(idx);
        }
        return None;
    }

    if let Some(idx) = resolve_non_relative_via_base_url_and_paths(spec, path_to_idx, ctx) {
        return Some(idx);
    }

    let mut dir = importer_dir.clone();
    loop {
        let node_base = join_path(&dir, &format!("node_modules/{spec}"));
        if let Some(idx) = resolve_module_path_base(&node_base, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
        let parent = dirname(&dir);
        if parent == dir || parent.is_empty() {
            break;
        }
        dir = parent;
    }

    if let Some(common) = ctx.common_source_dir.as_deref() {
        let project_base = join_path(common, spec);
        if let Some(idx) =
            resolve_module_path_base(&project_base, path_to_idx, &ctx.module_suffixes)
        {
            return Some(idx);
        }
    }

    for file in &test_case.files {
        let candidate = normalize_header_path(&file.name);
        for ext in [
            ".ts", ".tsx", ".mts", ".cts", ".d.ts", ".d.mts", ".d.cts", ".js", ".jsx", ".json",
        ] {
            let file_name = format!("{spec}{ext}");
            let index_name = format!("{spec}/index{ext}");
            if candidate == file_name
                || candidate == index_name
                || candidate.ends_with(&format!("/{spec}{ext}"))
                || candidate.ends_with(&format!("/{spec}/index{ext}"))
            {
                if let Some(idx) = path_to_idx.get(&candidate).copied() {
                    return Some(idx);
                }
            }
        }
    }

    None
}

/// Resolution used for TS2307 diagnostics. Unlike `resolve_module_specifier`
/// it never falls back to "some file with that basename exists": only the
/// relative/absolute forms, tsconfig `paths` (with or without `baseUrl`),
/// `baseUrl`, `rootDirs`, node_modules lookup and (under node16+/bundler)
/// package self-name references count as resolved.
pub(crate) fn resolve_module_specifier_for_diagnostics(
    importer_name: &str,
    spec: &str,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
    self_name_resolution: bool,
) -> Option<usize> {
    resolve_module_specifier_with_conditions(
        importer_name,
        spec,
        test_case,
        path_to_idx,
        ctx,
        self_name_resolution,
        None,
    )
}

/// [`resolve_module_specifier_for_diagnostics`] with the package-map
/// conditions fixed by the caller (e.g. `require` for a CommonJS request).
pub(crate) fn resolve_module_specifier_with_conditions(
    importer_name: &str,
    spec: &str,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
    self_name_resolution: bool,
    forced_conditions: Option<&[&str]>,
) -> Option<usize> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    let importer_dir = dirname(importer_name);
    // Package map conditions follow the importer's Node format.
    let node_format = ctx.base_module.is_some_and(|m| {
        matches!(
            m,
            ModuleKind::Node16 | ModuleKind::Node18 | ModuleKind::Node20 | ModuleKind::NodeNext
        )
    });
    let conditions: &[&str] = match crate::transforms::effective_module_kind_for_source(
        importer_name,
        ctx.base_module,
        &ctx.package_type_by_dir,
    ) {
        Some(ModuleKind::ESNext) if node_format => &["types", "import", "node", "default"],
        Some(ModuleKind::CommonJS) if node_format => &["types", "require", "node", "default"],
        _ => ANY_CONDITIONS,
    };
    let conditions = forced_conditions.unwrap_or(conditions);
    let dot_relative =
        spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../");
    if dot_relative {
        let base = normalize_path_segments(&join_path(&importer_dir, spec));
        // `./x.mjs` / `./x.cjs` name emitted files; their sources or
        // declarations sit next to them (`x.mts` / `x.d.mts`, `x.cts` /
        // `x.d.cts`). Diagnostics-only: the emit resolver keeps program
        // file order.
        let lower = base.to_ascii_lowercase();
        for (js_ext, ts_exts) in [(".mjs", [".mts", ".d.mts"]), (".cjs", [".cts", ".d.cts"])] {
            if lower.ends_with(js_ext) {
                let stem = &base[..base.len() - js_ext.len()];
                for ts_ext in ts_exts {
                    if let Some(idx) = path_to_idx.get(&format!("{stem}{ts_ext}")).copied() {
                        return Some(idx);
                    }
                }
            }
        }
        if let Some(idx) = resolve_module_path_base(&base, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
        if let Some(idx) = resolve_module_path_via_root_dirs(&base, path_to_idx, ctx) {
            return Some(idx);
        }
        // allowArbitraryExtensions: `./foo.html` is declared by `foo.d.html.ts`
        // (never for extensions TypeScript resolves itself).
        let file_name = basename(&base);
        let arbitrary_extensions = option_flag_true(&test_case.options, "allowarbitraryextensions")
            || extract_json_bool(
                &find_tsconfig_file(test_case)
                    .and_then(|f| extract_json_object(&f.content, "compilerOptions"))
                    .unwrap_or_default(),
                "allowArbitraryExtensions",
            ) == Some(true);
        if let Some((stem, extension)) = file_name.rsplit_once('.') {
            let native = matches!(
                extension,
                "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "json"
            ) || stem.ends_with(".d");
            if arbitrary_extensions && !stem.is_empty() && !native {
                let dir = dirname(&base);
                for declaration_extension in [".ts", ".mts", ".cts"] {
                    let candidate = format!("{stem}.d.{extension}{declaration_extension}");
                    let path = if dir.is_empty() {
                        candidate
                    } else {
                        join_path(&dir, &candidate)
                    };
                    if let Some(idx) = path_to_idx.get(&path).copied() {
                        return Some(idx);
                    }
                }
            }
        }
        return None;
    }

    // `#internal` specifiers resolve through the nearest package.json `imports`.
    if spec.starts_with('#') {
        let mut dir = importer_dir.clone();
        loop {
            let package_json = if dir.is_empty() {
                "package.json".to_string()
            } else {
                join_path(&dir, "package.json")
            };
            if let Some(idx) = path_to_idx.get(&package_json).copied() {
                // Node reserves `#` and `#/`; a target must be `./`-relative
                // and name an existing file.
                if spec == "#" || spec.starts_with("#/") {
                    return None;
                }
                let targets = package_map_targets(&test_case.files[idx].content, "imports", spec)?;
                return resolve_package_map_targets(&dir, &targets, path_to_idx, ctx);
            }
            if dir.is_empty() {
                return None;
            }
            let parent = dirname(&dir);
            if parent == dir {
                return None;
            }
            dir = parent;
        }
    }

    // tsconfig `paths` apply to every non-dot-relative name, rooted ones
    // (`/foo`, `c:/foo`, `//server/foo`) included; without `baseUrl` the
    // targets are relative to the tsconfig directory.
    let paths_base = ctx
        .base_url
        .as_deref()
        .or(ctx.tsconfig_dir.as_deref())
        .map(|s| s.to_string());
    if let (Some(base), Some(paths_str)) = (paths_base.as_deref(), ctx.paths.as_deref()) {
        for (pattern, targets) in parse_paths_entries(paths_str) {
            let Some(matched) = match_path_pattern(spec, &pattern) else {
                continue;
            };
            for target in targets {
                let mut mapped = target.replace('*', &matched);
                while mapped.contains("//") {
                    mapped = mapped.replace("//", "/");
                }
                let candidate = if is_absolute_path(&mapped) {
                    normalize_path_segments(&normalize_header_path(&mapped))
                } else {
                    normalize_path_segments(&join_path(base, &normalize_header_path(&mapped)))
                };
                if let Some(idx) =
                    resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes)
                {
                    return Some(idx);
                }
            }
        }
    }
    if spec.starts_with('/') {
        let base = normalize_header_path(spec);
        if let Some(idx) = resolve_module_path_base(&base, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
        return resolve_module_path_via_root_dirs(&base, path_to_idx, ctx);
    }
    if let Some(base_url) = ctx.base_url.as_deref() {
        let candidate = normalize_path_segments(&join_path(base_url, spec));
        if let Some(idx) = resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
    }

    let (package_name, package_entry) = split_package_specifier(spec);
    let mut dir = importer_dir.clone();
    loop {
        let node_base = join_path(&dir, &format!("node_modules/{spec}"));
        let package_dir = join_path(&dir, &format!("node_modules/{package_name}"));
        let package_json = join_path(&package_dir, "package.json");
        let package_json_content = path_to_idx
            .get(&package_json)
            .map(|idx| (*idx, test_case.files[*idx].content.as_str()));
        // Under node16+/bundler resolution a package.json `exports` map is
        // authoritative: unlisted or null-mapped subpaths never resolve to
        // files that happen to exist on disk.
        if self_name_resolution {
            if let Some((_, content)) = package_json_content {
                match lookup_package_exports(content, &package_entry) {
                    ExportsLookup::Target => {
                        let targets = package_map_targets_with_conditions(
                            content,
                            "exports",
                            &package_entry,
                            conditions,
                        )
                        .unwrap_or_default();
                        return resolve_package_map_targets(
                            &package_dir,
                            &targets,
                            path_to_idx,
                            ctx,
                        );
                    }
                    ExportsLookup::Blocked => return None,
                    ExportsLookup::NoExportsMap => {}
                }
            }
        }
        // `typesVersions` redirects the package's type paths for the
        // TypeScript versions it lists (the root maps its `types` entry).
        if let Some((_, content)) = package_json_content {
            let subpath = if package_entry == "." {
                extract_json_string(content, "types")
                    .or_else(|| extract_json_string(content, "typings"))
                    .unwrap_or_else(|| "index".to_string())
            } else {
                package_entry.trim_start_matches("./").to_string()
            };
            let subpath = subpath.trim_start_matches("./");
            for target in types_versions_targets(content, subpath) {
                let candidate = normalize_path_segments(&join_path(
                    &package_dir,
                    &normalize_header_path(&target),
                ));
                if let Some(idx) =
                    resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes)
                {
                    return Some(idx);
                }
            }
        }
        if let Some(idx) = resolve_module_path_base(&node_base, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
        // Package root: `types` / `typings` / `main` name the entry file.
        if package_entry == "." {
            if let Some((_, content)) = package_json_content {
                for field in ["types", "typings", "main"] {
                    let Some(target) = extract_json_string(content, field) else {
                        continue;
                    };
                    let candidate = normalize_path_segments(&join_path(
                        &package_dir,
                        &normalize_header_path(&target),
                    ));
                    if let Some(idx) =
                        resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes)
                    {
                        return Some(idx);
                    }
                }
            }
        }
        if dir.is_empty() {
            break;
        }
        let parent = dirname(&dir);
        if parent == dir {
            break;
        }
        dir = parent;
    }
    if self_name_resolution {
        if let Some((idx, entry)) =
            self_name_package_reference(&importer_dir, spec, test_case, path_to_idx)
        {
            // Resolve the matched `exports` entry to its target FILE (its
            // own Node format decides TS1479); the package.json index is
            // only a fallback marker that the specifier resolves.
            let package_json_path = normalize_header_path(&test_case.files[idx].name);
            let package_dir = dirname(&package_json_path);
            let targets = package_map_targets_with_conditions(
                &test_case.files[idx].content,
                "exports",
                &entry,
                conditions,
            )
            .unwrap_or_default();
            if let Some(target_idx) =
                resolve_package_map_targets(&package_dir, &targets, path_to_idx, ctx)
            {
                return Some(target_idx);
            }
            return Some(idx);
        }
    }
    None
}

/// First `./`-relative package map target that names a program file.
fn resolve_package_map_targets(
    package_dir: &str,
    targets: &[String],
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
) -> Option<usize> {
    targets
        .iter()
        .filter(|target| target.starts_with("./"))
        .find_map(|target| {
            let candidate = if package_dir.is_empty() {
                normalize_path_segments(&normalize_header_path(target))
            } else {
                normalize_path_segments(&join_path(package_dir, &normalize_header_path(target)))
            };
            // An `exports` target names the EMITTED file (`./index.cjs`);
            // its declaration lives next to it (`index.d.cts` / `.cts`).
            let lower = candidate.to_ascii_lowercase();
            for (js_ext, ts_exts) in [(".mjs", [".mts", ".d.mts"]), (".cjs", [".cts", ".d.cts"])] {
                if lower.ends_with(js_ext) {
                    let stem = &candidate[..candidate.len() - js_ext.len()];
                    for ts_ext in ts_exts {
                        if let Some(idx) = path_to_idx.get(&format!("{stem}{ts_ext}")).copied() {
                            return Some(idx);
                        }
                    }
                }
            }
            resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes)
        })
}

/// Split a bare specifier into (package name, export map entry): `inner` →
/// (`inner`, `.`), `@s/p/sub` → (`@s/p`, `./sub`).
/// Mapped targets of `subpath` under the first `typesVersions` entry whose
/// version range admits the current TypeScript (any `*`, `>=`, or `>` range;
/// ranges with an upper bound are treated as excluding it).
fn types_versions_targets(package_json: &str, subpath: &str) -> Vec<String> {
    let Some(json) = parse_json_lenient(package_json) else {
        return Vec::new();
    };
    let Some(JsonValue::Object(ranges)) = json.get("typesVersions") else {
        return Vec::new();
    };
    let Some((_, JsonValue::Object(mappings))) = ranges.iter().find(|(range, _)| {
        let range = range.trim();
        range == "*"
            || ((range.starts_with(">=") || range.starts_with('>')) && !range.contains('<'))
    }) else {
        return Vec::new();
    };
    for (pattern, targets) in mappings {
        let Some(matched) = match_path_pattern(subpath, pattern) else {
            continue;
        };
        let JsonValue::Array(targets) = targets else {
            continue;
        };
        return targets
            .iter()
            .filter_map(JsonValue::as_str)
            .map(|target| target.replace('*', &matched))
            .collect();
    }
    Vec::new()
}

fn split_package_specifier(spec: &str) -> (String, String) {
    let mut segments = spec.splitn(3, '/');
    let first = segments.next().unwrap_or_default();
    let name = if first.starts_with('@') {
        match segments.next() {
            Some(second) => format!("{first}/{second}"),
            None => first.to_string(),
        }
    } else {
        first.to_string()
    };
    let rest = spec.strip_prefix(name.as_str()).unwrap_or_default();
    let entry = if rest.is_empty() {
        ".".to_string()
    } else {
        format!(".{rest}")
    };
    (name, entry)
}

/// Package self-name reference: the nearest `package.json` above
/// `importer_dir` whose `name` equals `spec` (export map entry `.`) or
/// prefixes it (`./<subpath>`), provided it declares an `exports` map.
pub(crate) fn self_name_package_reference(
    importer_dir: &str,
    spec: &str,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
) -> Option<(usize, String)> {
    let mut dir = importer_dir.to_string();
    loop {
        let package_json = if dir.is_empty() {
            "package.json".to_string()
        } else {
            join_path(&dir, "package.json")
        };
        if let Some(idx) = path_to_idx.get(&package_json).copied() {
            let content = &test_case.files[idx].content;
            let name = extract_json_string(content, "name").filter(|name| !name.is_empty())?;
            let entry = if spec == name {
                ".".to_string()
            } else {
                let rest = spec.strip_prefix(name.as_str())?;
                if !rest.starts_with('/') {
                    return None;
                }
                format!(".{rest}")
            };
            return (lookup_package_exports(content, &entry) == ExportsLookup::Target)
                .then_some((idx, entry));
        }
        if dir.is_empty() {
            return None;
        }
        let parent = dirname(&dir);
        if parent == dir {
            return None;
        }
        dir = parent;
    }
}

/// tsc guesses the project root from the common directory of the importing
/// file and the package.json; every ancestor of that directory is an equally
/// valid guess, so the root is ambiguous unless the common directory is the
/// file-system root. Relative unit names live under the harness's `/.src`.
pub(crate) fn project_root_is_ambiguous(importer: &str, package_path: &str) -> bool {
    if !is_absolute_path(importer) || !is_absolute_path(package_path) {
        return true;
    }
    let importer_dir = dirname(importer);
    let package_dir = dirname(package_path);
    let a: Vec<&str> = importer_dir.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = package_dir.split('/').filter(|s| !s.is_empty()).collect();
    let common = a
        .iter()
        .zip(b.iter())
        .take_while(|(x, y)| x.eq_ignore_ascii_case(y))
        .count();
    let root_segments = usize::from(is_windows_absolute(importer));
    common > root_segments
}

fn resolve_non_relative_via_base_url_and_paths(
    spec: &str,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
) -> Option<usize> {
    let base_url = ctx.base_url.as_deref()?;

    if let Some(paths_str) = ctx.paths.as_deref() {
        for (pattern, targets) in parse_paths_entries(paths_str) {
            let Some(matched) = match_path_pattern(spec, &pattern) else {
                continue;
            };
            for target in targets {
                let mapped = target.replace('*', &matched);
                let candidate = if is_absolute_path(&mapped) {
                    normalize_path_segments(&normalize_header_path(&mapped))
                } else {
                    normalize_path_segments(&join_path(base_url, &normalize_header_path(&mapped)))
                };
                if let Some(idx) =
                    resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes)
                {
                    return Some(idx);
                }
            }
        }
    }

    let candidate = join_path(base_url, spec);
    resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes)
}

fn match_path_pattern(module_name: &str, pattern: &str) -> Option<String> {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        if module_name.starts_with(prefix)
            && module_name.ends_with(suffix)
            && module_name.len() >= prefix.len() + suffix.len()
        {
            Some(module_name[prefix.len()..module_name.len() - suffix.len()].to_string())
        } else {
            None
        }
    } else if module_name == pattern {
        Some(String::new())
    } else {
        None
    }
}

fn parse_paths_entries(paths_str: &str) -> Vec<(String, Vec<String>)> {
    let mut entries = Vec::new();
    let trimmed = paths_str.trim();
    let inner = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let mut remaining = inner.trim();
    while !remaining.is_empty() {
        let Some(key_start) = remaining.find('"') else {
            break;
        };
        remaining = &remaining[key_start + 1..];
        let Some(key_end) = remaining.find('"') else {
            break;
        };
        let key = remaining[..key_end].to_string();
        remaining = &remaining[key_end + 1..];

        let Some(colon) = remaining.find(':') else {
            break;
        };
        remaining = remaining[colon + 1..].trim_start();

        if !remaining.starts_with('[') {
            break;
        }
        remaining = &remaining[1..];

        let mut targets = Vec::new();
        loop {
            remaining = remaining.trim_start();
            if remaining.starts_with(']') {
                remaining = &remaining[1..];
                break;
            }

            let Some(value_start) = remaining.find('"') else {
                break;
            };
            remaining = &remaining[value_start + 1..];
            let Some(value_end) = remaining.find('"') else {
                break;
            };
            targets.push(remaining[..value_end].to_string());
            remaining = &remaining[value_end + 1..];

            remaining = remaining.trim_start();
            if remaining.starts_with(',') {
                remaining = &remaining[1..];
            }
        }

        entries.push((key, targets));
        remaining = remaining.trim_start();
        if remaining.starts_with(',') {
            remaining = &remaining[1..];
        }
    }

    entries
}

pub(crate) fn resolve_reference_specifier(
    importer_name: &str,
    spec: &str,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
) -> Option<usize> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    if spec.starts_with('/') {
        let base = normalize_header_path(spec);
        if let Some(idx) = resolve_module_path_base(&base, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
        if let Some(idx) = resolve_module_path_via_root_dirs(&base, path_to_idx, ctx) {
            return Some(idx);
        }
    } else {
        let importer_dir = dirname(importer_name);
        let base = join_path(&importer_dir, spec);
        if let Some(idx) = resolve_module_path_base(&base, path_to_idx, &ctx.module_suffixes) {
            return Some(idx);
        }
        if let Some(idx) = resolve_module_path_via_root_dirs(&base, path_to_idx, ctx) {
            return Some(idx);
        }
    }

    resolve_module_specifier(importer_name, spec, test_case, path_to_idx, ctx)
}

pub(crate) fn resolve_module_path_via_root_dirs(
    base: &str,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
) -> Option<usize> {
    if ctx.root_dirs.len() < 2 {
        return None;
    }
    let base = normalize_header_path(base);
    for root in &ctx.root_dirs {
        let Some(rel_base) = strip_prefix_path(&base, root) else {
            continue;
        };
        for candidate_root in &ctx.root_dirs {
            if candidate_root == root {
                continue;
            }
            let candidate = join_path(candidate_root, &rel_base);
            if let Some(idx) =
                resolve_module_path_base(&candidate, path_to_idx, &ctx.module_suffixes)
            {
                return Some(idx);
            }
        }
    }
    None
}

pub(crate) fn resolve_module_path_base(
    base: &str,
    path_to_idx: &HashMap<String, usize>,
    module_suffixes: &[String],
) -> Option<usize> {
    let base = normalize_header_path(base);
    let ext_candidates = [
        ".ts", ".tsx", ".mts", ".cts", ".d.ts", ".d.mts", ".d.cts", ".js", ".jsx", ".json",
    ];
    let suffixes: Vec<String> = if module_suffixes.is_empty() {
        vec![String::new()]
    } else {
        module_suffixes.to_vec()
    };

    let lower = base.to_ascii_lowercase();
    let has_ext = ext_candidates.iter().any(|ext| lower.ends_with(ext));
    if has_ext {
        if lower.ends_with(".json") && !module_suffixes.is_empty() {
            let stem = &base[..base.len() - ".json".len()];
            for suffix in module_suffixes {
                let candidate = format!("{stem}{suffix}.json");
                if let Some(idx) = path_to_idx.get(&candidate).copied() {
                    return Some(idx);
                }
            }
        }
        // When moduleSuffixes are configured and the import has a .js/.mjs/.cjs extension,
        // try suffix-inserted variants BEFORE the exact match.
        // e.g. for `./foo.js` with suffix `.ios`, try `./foo.ios.js` first.
        if !module_suffixes.is_empty() {
            let js_suffix_exts: &[&str] =
                &[".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"];
            for ext in js_suffix_exts {
                if lower.ends_with(ext) {
                    let stem = &base[..base.len() - ext.len()];
                    for suffix in module_suffixes {
                        if suffix.is_empty() {
                            continue;
                        }
                        let candidate = format!("{stem}{suffix}{ext}");
                        if let Some(idx) = path_to_idx.get(&candidate).copied() {
                            return Some(idx);
                        }
                    }
                    break;
                }
            }
        }
        if let Some(idx) = path_to_idx.get(&base).copied() {
            return Some(idx);
        }
        // Extension substitution: when importing `foo.js`, TypeScript (node16/nodenext)
        // may resolve to `foo.ts`, `foo.tsx`, `foo.mts`, `foo.cts`, or `foo.d.ts`.
        // Similarly `.mjs` → `.mts` and `.cjs` → `.cts`.
        let js_ext_subst: &[(&str, &[&str])] = &[
            (".js", &[".ts", ".tsx", ".d.ts"]),
            (".mjs", &[".mts", ".d.mts"]),
            (".cjs", &[".cts", ".d.cts"]),
        ];
        for (js_ext, ts_exts) in js_ext_subst {
            if lower.ends_with(js_ext) {
                let stem = &base[..base.len() - js_ext.len()];
                for ts_ext in *ts_exts {
                    let candidate = format!("{stem}{ts_ext}");
                    if let Some(idx) = path_to_idx.get(&candidate).copied() {
                        return Some(idx);
                    }
                }
            }
        }
        return None;
    }

    if let Some(idx) = path_to_idx.get(&base).copied() {
        return Some(idx);
    }

    for suffix in &suffixes {
        for ext in &ext_candidates {
            let file_candidate = format!("{base}{suffix}{ext}");
            if let Some(idx) = path_to_idx.get(&file_candidate).copied() {
                return Some(idx);
            }
        }
        for ext in &ext_candidates {
            let index_candidate = if base.is_empty() {
                format!("index{suffix}{ext}")
            } else {
                format!("{base}/index{suffix}{ext}")
            };
            if let Some(idx) = path_to_idx.get(&index_candidate).copied() {
                return Some(idx);
            }
        }
    }
    None
}
