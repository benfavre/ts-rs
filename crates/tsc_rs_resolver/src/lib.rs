//! Module resolution compatibility layer.
//!
//! Implements TypeScript's Node and Classic module resolution algorithms.
//! Supports relative/non-relative imports, extension trials, index files,
//! node_modules walking, and path mapping from CompilerOptions.

use std::path::{Path, PathBuf};

use tsc_rs_ast::CompilerOptions;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModule {
    pub resolved_file_name: String,
    pub is_external_library_import: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionResult {
    pub module_name: String,
    pub resolved_path: Option<String>,
    pub trace: Vec<String>,
}

pub trait ModuleResolver {
    fn resolve(&self, containing_file: &str, module_name: &str) -> ResolutionResult;
}

#[derive(Default)]
pub struct BootstrapResolver;

impl ModuleResolver for BootstrapResolver {
    fn resolve(&self, containing_file: &str, module_name: &str) -> ResolutionResult {
        ResolutionResult {
            module_name: module_name.to_string(),
            resolved_path: None,
            trace: vec![format!("bootstrap resolve from {containing_file}")],
        }
    }
}

// ---------------------------------------------------------------------------
// Resolution strategy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleResolutionKind {
    Node,
    Classic,
}

impl ModuleResolutionKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "node" | "node10" | "node16" | "nodenext" | "bundler" => Some(Self::Node),
            "classic" => Some(Self::Classic),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Extension trial order
// ---------------------------------------------------------------------------

const ALL_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx"];
const INDEX_FILES: &[&str] = &[
    "/index.ts",
    "/index.tsx",
    "/index.d.ts",
    "/index.js",
    "/index.jsx",
];

// Classic resolution only tries .ts and .d.ts
const CLASSIC_EXTENSIONS: &[&str] = &[".ts", ".d.ts"];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve a module name to a file path using TypeScript's resolution algorithms.
///
/// Returns `None` if the module cannot be resolved.
pub fn resolve_module_name(
    module_name: &str,
    containing_file: &str,
    options: &CompilerOptions,
) -> Option<ResolvedModule> {
    let kind = options
        .module_resolution
        .as_deref()
        .and_then(ModuleResolutionKind::parse)
        .unwrap_or(ModuleResolutionKind::Node);

    // Try path mapping first
    let resolved = if let Some(resolved) = try_path_mapping(module_name, options) {
        Some(resolved)
    } else {
        match kind {
            ModuleResolutionKind::Node => resolve_node(module_name, containing_file, options),
            ModuleResolutionKind::Classic => resolve_classic(module_name, containing_file),
        }
    };
    resolved.map(|mut resolved| {
        resolved.resolved_file_name = dedupe_package_file(resolved.resolved_file_name);
        resolved
    })
}

/// `name@version` of a `node_modules` package root, remembered for the
/// process (see [`dedupe_package_file`]).
static PACKAGE_IDS: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<String, Option<String>>>,
> = std::sync::OnceLock::new();
/// First path seen for each `name@version` + subpath.
static PACKAGE_FILE_REDIRECTS: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<String, String>>,
> = std::sync::OnceLock::new();

/// A file of a `node_modules` package that is a byte-for-byte install of a
/// package already seen elsewhere (same `name@version`, same path inside
/// it) is the file seen first, as tsc redirects duplicate packages to one
/// source file. Package managers nest copies of the same version under
/// each dependent (`@types/cors/node_modules/@types/node`, …): without
/// this an apps/app program held 16 copies of `@types/node`, 37 MB of
/// declarations whose globals all merged.
///
/// The table is never cleared: a long-lived session must redirect a
/// request's imports exactly as its init did.
pub fn dedupe_package_file(path: String) -> String {
    const MARKER: &str = "/node_modules/";
    let Some(last) = path.rfind(MARKER) else {
        return path;
    };
    let after = &path[last + MARKER.len()..];
    let mut segments = after.splitn(3, '/');
    let Some(first) = segments.next() else {
        return path;
    };
    let name_len = if first.starts_with('@') {
        match segments.next() {
            Some(second) => first.len() + 1 + second.len(),
            None => return path,
        }
    } else {
        first.len()
    };
    if after.len() <= name_len + 1 {
        return path;
    }
    let root = &path[..last + MARKER.len() + name_len];
    let subpath = &after[name_len + 1..];

    let ids = PACKAGE_IDS.get_or_init(Default::default);
    let cached = ids
        .read()
        .expect("package id cache poisoned")
        .get(root)
        .cloned();
    let id = match cached {
        Some(id) => id,
        None => {
            let id = read_package_json(&Path::new(root).join("package.json")).and_then(|pkg| {
                let value = pkg.value()?;
                let name = value.get("name")?.as_str()?;
                let version = value.get("version")?.as_str()?;
                Some(format!("{name}@{version}"))
            });
            ids.write()
                .expect("package id cache poisoned")
                .insert(root.to_string(), id.clone());
            id
        }
    };
    let Some(id) = id else {
        return path;
    };
    let key = format!("{id}/{subpath}");
    let redirects = PACKAGE_FILE_REDIRECTS.get_or_init(Default::default);
    if let Some(first) = redirects
        .read()
        .expect("package redirect table poisoned")
        .get(&key)
    {
        return first.clone();
    }
    redirects
        .write()
        .expect("package redirect table poisoned")
        .entry(key)
        .or_insert(path)
        .clone()
}

// ---------------------------------------------------------------------------
// Node resolution
// ---------------------------------------------------------------------------

fn resolve_node(
    module_name: &str,
    containing_file: &str,
    _options: &CompilerOptions,
) -> Option<ResolvedModule> {
    let containing_dir = Path::new(containing_file)
        .parent()
        .unwrap_or_else(|| Path::new("."));

    if is_relative_import(module_name) {
        // Relative import: resolve relative to containing file's directory
        resolve_relative(module_name, containing_dir, false)
    } else {
        // TS strips the `node:` prefix for Node.js built-in modules so they
        // resolve via @types/node. `node:fs` → `fs` → `node_modules/@types/node/fs.d.ts`.
        let bare = module_name.strip_prefix("node:").unwrap_or(module_name);
        // Non-relative import: walk up node_modules
        resolve_node_modules(bare, containing_dir)
    }
}

/// If `path` ends with a JS runtime extension that TypeScript pairs with a
/// declaration extension, return the declaration path. Otherwise `None`.
///
/// Mapping per the TypeScript 4.7+ resolver:
/// - `.cjs` → `.d.cts` (CJS-only typings)
/// - `.mjs` → `.d.mts` (ESM-only typings)
/// - `.js`  → `.d.ts`  (classic / dual-package typings)
///
/// Used so an `import "./foo.cjs"` inside a CJS-typed `.d.cts` file (zod/v4
/// does this) resolves to the matching `.d.cts` instead of the runtime
/// `.cjs` file, which is what TS does and what the autodiscovery filter
/// expects.
fn jsx_to_dts_pair(path: &Path) -> Option<PathBuf> {
    let s = path.to_string_lossy();
    let stem = if let Some(rest) = s.strip_suffix(".cjs") {
        format!("{rest}.d.cts")
    } else if let Some(rest) = s.strip_suffix(".mjs") {
        format!("{rest}.d.mts")
    } else if let Some(rest) = s.strip_suffix(".js") {
        format!("{rest}.d.ts")
    } else {
        return None;
    };
    Some(PathBuf::from(stem))
}

/// Resolve a relative import (starts with `./ or ../`).
fn resolve_relative(
    module_name: &str,
    containing_dir: &Path,
    is_external: bool,
) -> Option<ResolvedModule> {
    let candidate = containing_dir.join(module_name);

    // 0. JS-extension → matching declaration file. TypeScript prefers
    //    the `.d.cts`/`.d.mts`/`.d.ts` paired with `.cjs`/`.mjs`/`.js`
    //    over the JS file itself for type resolution. Without this, an
    //    `import "./classic/index.cjs"` inside zod/v4's typing file
    //    resolves to the runtime `.cjs` file (no types) instead of the
    //    sibling `.d.cts` (the real type surface).
    if let Some(dts_pair) = jsx_to_dts_pair(&candidate) {
        if dts_pair.probe_is_file() {
            return Some(ResolvedModule {
                resolved_file_name: normalize_path(&dts_pair),
                is_external_library_import: is_external,
            });
        }
    }

    // 1. Try the exact path (if it already has an extension)
    if candidate.probe_is_file() {
        return Some(ResolvedModule {
            resolved_file_name: normalize_path(&candidate),
            is_external_library_import: is_external,
        });
    }

    // 2. Try adding extensions
    for ext in ALL_EXTENSIONS {
        let with_ext = append_extension(&candidate, ext);
        if with_ext.probe_is_file() {
            return Some(ResolvedModule {
                resolved_file_name: normalize_path(&with_ext),
                is_external_library_import: is_external,
            });
        }
    }

    // 3. Try index files (if candidate is a directory or could be)
    for index in INDEX_FILES {
        let index_path = PathBuf::from(format!("{}{index}", candidate.display()));
        if index_path.probe_is_file() {
            return Some(ResolvedModule {
                resolved_file_name: normalize_path(&index_path),
                is_external_library_import: is_external,
            });
        }
    }

    None
}

/// Walk up directories looking for node_modules/<module_name>.
///
/// Also checks `node_modules/@types/<module_name>` as a fallback, matching
/// TypeScript's standard resolution for DefinitelyTyped packages (e.g.
/// `import { resolve } from "path"` → `@types/node`).
type NodeModulesCache = std::sync::RwLock<
    std::collections::HashMap<PathBuf, std::collections::HashMap<String, Option<ResolvedModule>>>,
>;
static NODE_MODULES_CACHE: std::sync::OnceLock<NodeModulesCache> = std::sync::OnceLock::new();

/// `resolve_node_modules_uncached`, remembered per (starting directory,
/// module name) in one-shot runs: every file of a directory importing the
/// same package repeated the same walk, `package.json` reads and parses.
fn resolve_node_modules(module_name: &str, starting_dir: &Path) -> Option<ResolvedModule> {
    if !CANONICAL_PATH_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return resolve_node_modules_uncached(module_name, starting_dir);
    }
    let cache = NODE_MODULES_CACHE.get_or_init(Default::default);
    if let Some(found) = cache
        .read()
        .expect("node_modules cache poisoned")
        .get(starting_dir)
        .and_then(|modules| modules.get(module_name))
    {
        return found.clone();
    }
    let resolved = resolve_node_modules_uncached(module_name, starting_dir);
    cache
        .write()
        .expect("node_modules cache poisoned")
        .entry(starting_dir.to_path_buf())
        .or_default()
        .insert(module_name.to_string(), resolved.clone());
    resolved
}

static PACKAGE_JSON_CACHE: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<PathBuf, Option<std::sync::Arc<PackageJson>>>>,
> = std::sync::OnceLock::new();

/// A `package.json`: its text and, parsed on first use, its JSON value.
struct PackageJson {
    text: String,
    value: std::sync::OnceLock<Option<serde_json::Value>>,
}

impl PackageJson {
    fn value(&self) -> Option<&serde_json::Value> {
        self.value
            .get_or_init(|| serde_json::from_str(&self.text).ok())
            .as_ref()
    }
}

/// Reads `path`; one-shot runs read and parse each `package.json` once.
fn read_package_json(path: &Path) -> Option<std::sync::Arc<PackageJson>> {
    let read = |path: &Path| {
        std::fs::read_to_string(path).ok().map(|text| {
            std::sync::Arc::new(PackageJson {
                text,
                value: std::sync::OnceLock::new(),
            })
        })
    };
    if !CANONICAL_PATH_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return read(path);
    }
    let cache = PACKAGE_JSON_CACHE.get_or_init(Default::default);
    if let Some(found) = cache.read().expect("package.json cache poisoned").get(path) {
        return found.clone();
    }
    let package = read(path);
    cache
        .write()
        .expect("package.json cache poisoned")
        .insert(path.to_path_buf(), package.clone());
    package
}

fn resolve_node_modules_uncached(module_name: &str, starting_dir: &Path) -> Option<ResolvedModule> {
    let mut dir = starting_dir.to_path_buf();

    loop {
        let node_modules = dir.join("node_modules");
        if node_modules.probe_is_dir() {
            // Split module name into package name and subpath
            let (package_name, subpath) = split_package_subpath(module_name);
            let package_dir = node_modules.join(package_name);

            if package_dir.probe_is_dir() {
                // First: try the package.json `exports` map. Modern packages
                // (including bext's workspace deps like @bext-stack/framework)
                // expose a flat-or-conditioned subpath table that maps
                // specifier → file. Without this, an import of
                // `@scope/pkg/streaming` would just look for
                // `<pkg>/streaming.{ts,tsx,js,…}` as a sibling of the
                // package root, miss the actual `<pkg>/src/streaming.ts`,
                // and fall through to a partial resolution.
                if let Some(resolved) = try_exports_map(&package_dir, subpath.as_deref()) {
                    return Some(resolved);
                }

                // Fallback: subpath against the package root (legacy resolution
                // for packages that ship without an `exports` field).
                if let Some(sub) = subpath {
                    if let Some(resolved) =
                        resolve_relative(&format!("./{sub}"), &package_dir, true)
                    {
                        return Some(resolved);
                    }
                }

                // Try package.json main/types/typings fields
                if let Some(resolved) = try_package_json(&package_dir) {
                    return Some(resolved);
                }

                // Try index files in the package root
                for index in INDEX_FILES {
                    let index_path = PathBuf::from(format!("{}{index}", package_dir.display()));
                    if index_path.probe_is_file() {
                        return Some(ResolvedModule {
                            resolved_file_name: normalize_path(&index_path),
                            is_external_library_import: true,
                        });
                    }
                }
            }

            // Also try the module directly as a file in node_modules
            let candidate = node_modules.join(module_name);
            for ext in ALL_EXTENSIONS {
                let with_ext = append_extension(&candidate, ext);
                if with_ext.probe_is_file() {
                    return Some(ResolvedModule {
                        resolved_file_name: normalize_path(&with_ext),
                        is_external_library_import: true,
                    });
                }
            }

            // @types fallback: check node_modules/@types/<package_name>
            // This is how TypeScript resolves types for packages that ship
            // their types via DefinitelyTyped (e.g. "path" → @types/node,
            // "express" → @types/express)
            if let Some(resolved) = try_at_types(&node_modules, package_name, subpath.as_deref()) {
                return Some(resolved);
            }
        }

        if !dir.pop() {
            break;
        }
        // Skip if the current directory is already node_modules
        if dir
            .file_name()
            .map(|n| n == "node_modules")
            .unwrap_or(false)
            && !dir.pop()
        {
            break;
        }
    }

    None
}

/// Try resolving a module from `node_modules/@types/<name>`.
///
/// For Node.js built-in modules (fs, path, crypto, etc.), the types live in
/// `@types/node` under a specific directory structure. We handle both:
/// 1. Direct `@types/<name>` packages (e.g. `@types/express`)
/// 2. `@types/node` for Node.js built-ins (e.g. `"path"` → `@types/node/path.d.ts`)
fn try_at_types(
    node_modules: &Path,
    package_name: &str,
    subpath: Option<&str>,
) -> Option<ResolvedModule> {
    let at_types = node_modules.join("@types");
    if !at_types.probe_is_dir() {
        return None;
    }

    // 1. Try @types/<package_name> directly (e.g. @types/express, @types/lodash)
    let types_pkg = at_types.join(package_name);
    if types_pkg.probe_is_dir() {
        if let Some(sub) = subpath {
            if let Some(resolved) = resolve_relative(&format!("./{sub}"), &types_pkg, true) {
                return Some(resolved);
            }
        }
        if let Some(resolved) = try_package_json(&types_pkg) {
            return Some(resolved);
        }
        for index in INDEX_FILES {
            let index_path = PathBuf::from(format!("{}{index}", types_pkg.display()));
            if index_path.probe_is_file() {
                return Some(ResolvedModule {
                    resolved_file_name: normalize_path(&index_path),
                    is_external_library_import: true,
                });
            }
        }
    }

    // 2. Try @types/node/<module_name> for Node.js built-in modules
    //    e.g. "fs" → @types/node/fs.d.ts, "path" → @types/node/path.d.ts
    let at_types_node = at_types.join("node");
    if at_types_node.probe_is_dir() {
        // Try @types/node/<module>.d.ts directly
        let builtin_dts = at_types_node.join(format!("{package_name}.d.ts"));
        if builtin_dts.probe_is_file() {
            return Some(ResolvedModule {
                resolved_file_name: normalize_path(&builtin_dts),
                is_external_library_import: true,
            });
        }
        // Try @types/node/<module>/index.d.ts
        let builtin_dir = at_types_node.join(package_name);
        if builtin_dir.probe_is_dir() {
            let index_dts = builtin_dir.join("index.d.ts");
            if index_dts.probe_is_file() {
                return Some(ResolvedModule {
                    resolved_file_name: normalize_path(&index_dts),
                    is_external_library_import: true,
                });
            }
        }
    }

    // 3. Try @types/bun for Bun built-in modules (e.g. "bun" → @types/bun)
    let at_types_bun = at_types.join("bun");
    if at_types_bun.probe_is_dir() && package_name == "bun" {
        if let Some(resolved) = try_package_json(&at_types_bun) {
            return Some(resolved);
        }
        for index in INDEX_FILES {
            let index_path = PathBuf::from(format!("{}{index}", at_types_bun.display()));
            if index_path.probe_is_file() {
                return Some(ResolvedModule {
                    resolved_file_name: normalize_path(&index_path),
                    is_external_library_import: true,
                });
            }
        }
    }

    None
}

/// Resolve a bare-package import via the `exports` map in the package's
/// `package.json`. Honors:
///
/// - Single-string root export: `"exports": "./dist/index.js"`
/// - Object with subpath keys: `{ ".": "./src/index.ts", "./streaming": "./src/streaming.ts" }`
/// - Conditional values: `{ "import": "./esm.js", "default": "./cjs.js" }`
///
/// Condition lookup priority (TypeScript-friendly first): `types`, `typings`,
/// `import`, `default`, `node`, `require`. The first matching condition that
/// resolves to a real file wins.
///
/// `subpath` is what comes after the package name in the import specifier:
/// `"@scope/pkg"` → `None`, `"@scope/pkg/streaming"` → `Some("streaming")`.
/// Wildcard patterns (`"./*"` → `"./src/*.ts"`) are not yet supported — bext
/// workspace deps use explicit subpath maps.
fn try_exports_map(package_dir: &Path, subpath: Option<&str>) -> Option<ResolvedModule> {
    let pkg_path = package_dir.join("package.json");
    let package = read_package_json(&pkg_path)?;
    let pkg = package.value()?;
    let exports = pkg.get("exports")?;

    let key = match subpath {
        None => ".".to_string(),
        Some(s) => format!("./{}", s.trim_start_matches("./")),
    };

    // Resolve `key` against the exports value. The exports field can be a
    // bare string (root export only) or an object with subpath keys —
    // exact-match first, wildcard patterns as fallback.
    let (target_value, capture) = if let Some(s) = exports.as_str() {
        // Bare string: only `.` is meaningful.
        if key != "." {
            return None;
        }
        (serde_json::Value::String(s.to_string()), None)
    } else if let Some(obj) = exports.as_object() {
        if let Some(exact) = obj.get(&key).cloned() {
            (exact, None)
        } else {
            let (val, cap) = match_wildcard_export(obj, &key)?;
            (val, Some(cap))
        }
    } else {
        return None;
    };

    let target_str_raw = pick_condition_target(&target_value)?;
    // Substitute `*` in the target with whatever the lookup `*` captured.
    // Per Node.js spec: target must contain exactly one `*` if the pattern
    // did. We replace_all defensively — multiple `*`s all get the same
    // capture, which matches what other resolvers do.
    let target_str = match capture {
        Some(c) => target_str_raw.replace('*', &c),
        None => target_str_raw,
    };

    // Resolve target file relative to package_dir.
    let resolved_path = package_dir.join(&target_str);
    if resolved_path.probe_is_file() {
        return Some(ResolvedModule {
            resolved_file_name: normalize_path(&resolved_path),
            is_external_library_import: true,
        });
    }
    // Try appending extensions (some manifests omit `.ts` if a build step
    // emits `.js`). For bext, framework's exports include the `.ts` already
    // — this branch is just defensive.
    for ext in ALL_EXTENSIONS {
        let with_ext = append_extension(&resolved_path, ext);
        if with_ext.probe_is_file() {
            return Some(ResolvedModule {
                resolved_file_name: normalize_path(&with_ext),
                is_external_library_import: true,
            });
        }
    }
    None
}

/// Find an exports-map subpath pattern (containing exactly one `*`) that
/// matches `key`. Returns the matched value plus the captured substring
/// (what `*` matched in the lookup) so the caller can substitute back
/// into the target.
///
/// Per the Node.js exports spec: when multiple patterns match, the one
/// with the longest *literal* base before the `*` wins; ties are broken
/// by the longer expansion. Patterns without a `*` are skipped (exact
/// matches are handled by the caller before reaching here).
fn match_wildcard_export(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<(serde_json::Value, String)> {
    let mut best: Option<(usize, &str, String, &serde_json::Value)> = None;
    for (pattern, value) in obj {
        let star_pos = match pattern.find('*') {
            Some(p) => p,
            None => continue,
        };
        // Reject patterns with more than one `*` — Node.js doesn't support
        // them in exports patterns.
        if pattern[star_pos + 1..].contains('*') {
            continue;
        }
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        if !key.starts_with(prefix) || !key.ends_with(suffix) {
            continue;
        }
        if key.len() < prefix.len() + suffix.len() {
            continue;
        }
        let cap_start = prefix.len();
        let cap_end = key.len() - suffix.len();
        if cap_end < cap_start {
            continue;
        }
        let capture = key[cap_start..cap_end].to_string();
        let prefix_len = prefix.len();
        match &best {
            Some((best_prefix_len, _, best_capture, _))
                if prefix_len < *best_prefix_len
                    || (prefix_len == *best_prefix_len && capture.len() <= best_capture.len()) =>
            {
                // Existing match wins (longer prefix, or same prefix +
                // longer capture).
            }
            _ => {
                best = Some((prefix_len, pattern.as_str(), capture, value));
            }
        }
    }
    best.map(|(_, _, cap, val)| (val.clone(), cap))
}

/// Walk an exports-map value and return the first concrete file-target
/// string. Strings return themselves; objects are walked condition-by-
/// condition until one resolves to a string. `null` skips. Unknown shapes
/// return `None`.
fn pick_condition_target(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => None,
        serde_json::Value::Object(map) => {
            // TypeScript-aware order: prefer .d.ts → .ts (via `types`/`typings`),
            // then ESM (`import`), then `default`, then `node`/`require`. Falls
            // back to the first string-valued entry if none of the named
            // conditions match (covers user-defined conditions).
            const PRIORITY: &[&str] = &["types", "typings", "import", "default", "node", "require"];
            for cond in PRIORITY {
                if let Some(inner) = map.get(*cond) {
                    if let Some(t) = pick_condition_target(inner) {
                        return Some(t);
                    }
                }
            }
            for (_, inner) in map {
                if let Some(t) = pick_condition_target(inner) {
                    return Some(t);
                }
            }
            None
        }
        // Arrays in exports are alternatives — return the first that
        // resolves. Rare in practice.
        serde_json::Value::Array(arr) => {
            for inner in arr {
                if let Some(t) = pick_condition_target(inner) {
                    return Some(t);
                }
            }
            None
        }
        _ => None,
    }
}

/// Try reading package.json for `types`, `typings`, or `main` fields.
fn try_package_json(package_dir: &Path) -> Option<ResolvedModule> {
    let pkg_path = package_dir.join("package.json");
    let package = read_package_json(&pkg_path)?;
    let content = &package.text;

    // Simple JSON field extraction (avoids serde dependency)
    // Try types/typings first (TypeScript declaration), then main
    for field in &["types", "typings", "main"] {
        if let Some(value) = extract_json_string_field(&content, field) {
            let resolved_path = package_dir.join(&value);

            // If the path exists as-is
            if resolved_path.probe_is_file() {
                return Some(ResolvedModule {
                    resolved_file_name: normalize_path(&resolved_path),
                    is_external_library_import: true,
                });
            }

            // Try appending extensions
            for ext in ALL_EXTENSIONS {
                let with_ext = append_extension(&resolved_path, ext);
                if with_ext.probe_is_file() {
                    return Some(ResolvedModule {
                        resolved_file_name: normalize_path(&with_ext),
                        is_external_library_import: true,
                    });
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Classic resolution
// ---------------------------------------------------------------------------

fn resolve_classic(module_name: &str, containing_file: &str) -> Option<ResolvedModule> {
    let containing_dir = Path::new(containing_file)
        .parent()
        .unwrap_or_else(|| Path::new("."));

    if is_relative_import(module_name) {
        // For relative imports, resolve from the containing directory
        let candidate = containing_dir.join(module_name);
        for ext in CLASSIC_EXTENSIONS {
            let with_ext = append_extension(&candidate, ext);
            if with_ext.probe_is_file() {
                return Some(ResolvedModule {
                    resolved_file_name: normalize_path(&with_ext),
                    is_external_library_import: false,
                });
            }
        }
        None
    } else {
        // For non-relative imports, walk up from the containing directory
        let mut dir = containing_dir.to_path_buf();
        loop {
            let candidate = dir.join(module_name);
            for ext in CLASSIC_EXTENSIONS {
                let with_ext = append_extension(&candidate, ext);
                if with_ext.probe_is_file() {
                    return Some(ResolvedModule {
                        resolved_file_name: normalize_path(&with_ext),
                        is_external_library_import: false,
                    });
                }
            }
            if !dir.pop() {
                break;
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Path mapping
// ---------------------------------------------------------------------------

/// Try to resolve using `baseUrl` and `paths` from CompilerOptions.
fn try_path_mapping(module_name: &str, options: &CompilerOptions) -> Option<ResolvedModule> {
    let base_url = options.base_url.as_deref()?;
    let base_path = Path::new(base_url);

    // If there are explicit path mappings, try those first
    if let Some(paths_str) = &options.paths {
        if let Some(resolved) = try_paths_mapping(module_name, base_path, paths_str) {
            return Some(resolved);
        }
    }

    // With just baseUrl, try resolving relative to it
    let candidate = base_path.join(module_name);
    for ext in ALL_EXTENSIONS {
        let with_ext = append_extension(&candidate, ext);
        if with_ext.probe_is_file() {
            return Some(ResolvedModule {
                resolved_file_name: normalize_path(&with_ext),
                is_external_library_import: false,
            });
        }
    }

    for index in INDEX_FILES {
        let index_path = PathBuf::from(format!("{}{index}", candidate.display()));
        if index_path.probe_is_file() {
            return Some(ResolvedModule {
                resolved_file_name: normalize_path(&index_path),
                is_external_library_import: false,
            });
        }
    }

    None
}

/// Try matching against paths mapping entries.
///
/// Paths are specified as JSON in the CompilerOptions, e.g.:
/// `{ "@app/*": ["src/app/*"] }`
///
/// We do minimal parsing here since the paths field is stored as a raw string.
fn try_paths_mapping(
    module_name: &str,
    base_path: &Path,
    paths_str: &str,
) -> Option<ResolvedModule> {
    // Parse simple path mapping entries from the JSON-like string
    // Format: { "pattern": ["target1", "target2"], ... }
    let entries = parse_paths_entries(paths_str);

    for (pattern, targets) in &entries {
        if let Some(matched) = match_path_pattern(module_name, pattern) {
            for target in targets {
                let resolved_target = target.replace('*', &matched);
                let candidate = base_path.join(&resolved_target);

                // Try exact
                if candidate.probe_is_file() {
                    return Some(ResolvedModule {
                        resolved_file_name: normalize_path(&candidate),
                        is_external_library_import: false,
                    });
                }

                // Try extensions
                for ext in ALL_EXTENSIONS {
                    let with_ext = append_extension(&candidate, ext);
                    if with_ext.probe_is_file() {
                        return Some(ResolvedModule {
                            resolved_file_name: normalize_path(&with_ext),
                            is_external_library_import: false,
                        });
                    }
                }

                // Try index files
                for index in INDEX_FILES {
                    let index_path = PathBuf::from(format!("{}{index}", candidate.display()));
                    if index_path.probe_is_file() {
                        return Some(ResolvedModule {
                            resolved_file_name: normalize_path(&index_path),
                            is_external_library_import: false,
                        });
                    }
                }
            }
        }
    }

    None
}

/// Match a module name against a path pattern (which may contain a `*` wildcard).
/// Returns the text that matched the wildcard, or empty string for exact matches.
fn match_path_pattern(module_name: &str, pattern: &str) -> Option<String> {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        if module_name.starts_with(prefix)
            && module_name.ends_with(suffix)
            && module_name.len() >= prefix.len() + suffix.len()
        {
            let matched = &module_name[prefix.len()..module_name.len() - suffix.len()];
            Some(matched.to_string())
        } else {
            None
        }
    } else if module_name == pattern {
        Some(String::new())
    } else {
        None
    }
}

/// Minimal parser for paths entries from a JSON-like string.
fn parse_paths_entries(paths_str: &str) -> Vec<(String, Vec<String>)> {
    let mut entries = Vec::new();
    let trimmed = paths_str.trim();
    let inner = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    // Split by pattern entries - this is a simplified parser
    let mut remaining = inner.trim();
    while !remaining.is_empty() {
        // Find the key
        let Some(key_start) = remaining.find('"') else {
            break;
        };
        remaining = &remaining[key_start + 1..];
        let Some(key_end) = remaining.find('"') else {
            break;
        };
        let key = remaining[..key_end].to_string();
        remaining = &remaining[key_end + 1..];

        // Find the colon
        let Some(colon) = remaining.find(':') else {
            break;
        };
        remaining = &remaining[colon + 1..];

        // Find the array
        let Some(arr_start) = remaining.find('[') else {
            break;
        };
        remaining = &remaining[arr_start + 1..];
        let Some(arr_end) = remaining.find(']') else {
            break;
        };
        let arr_content = &remaining[..arr_end];
        remaining = &remaining[arr_end + 1..];

        // Parse array values
        let mut targets = Vec::new();
        let mut arr_remaining = arr_content;
        while let Some(str_start) = arr_remaining.find('"') {
            arr_remaining = &arr_remaining[str_start + 1..];
            if let Some(str_end) = arr_remaining.find('"') {
                targets.push(arr_remaining[..str_end].to_string());
                arr_remaining = &arr_remaining[str_end + 1..];
            } else {
                break;
            }
        }

        entries.push((key, targets));

        // Skip comma
        if let Some(comma) = remaining.find(',') {
            remaining = &remaining[comma + 1..];
        } else {
            break;
        }
    }

    entries
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_relative_import(module_name: &str) -> bool {
    module_name.starts_with("./") || module_name.starts_with("../")
}

/// Split a module specifier into package name and optional subpath.
/// E.g., `@scope/pkg/sub/path` -> (`@scope/pkg`, Some(`sub/path`))
///        `lodash/fp` -> (`lodash`, Some(`fp`))
///        `lodash` -> (`lodash`, None)
fn split_package_subpath(module_name: &str) -> (&str, Option<&str>) {
    if module_name.starts_with('@') {
        // Scoped package: @scope/name[/subpath]
        if let Some(first_slash) = module_name.find('/') {
            if let Some(second_slash) = module_name[first_slash + 1..]
                .find('/')
                .map(|i| i + first_slash + 1)
            {
                (
                    &module_name[..second_slash],
                    Some(&module_name[second_slash + 1..]),
                )
            } else {
                (module_name, None)
            }
        } else {
            (module_name, None)
        }
    } else {
        // Regular package: name[/subpath]
        if let Some(slash) = module_name.find('/') {
            (&module_name[..slash], Some(&module_name[slash + 1..]))
        } else {
            (module_name, None)
        }
    }
}

fn append_extension(path: &Path, ext: &str) -> PathBuf {
    let s = path.to_string_lossy();
    PathBuf::from(format!("{s}{ext}"))
}

fn normalize_path(path: &Path) -> String {
    // Canonicalize if possible, otherwise use the path as-is
    canonicalize(path)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

/// What a path is on disk, as remembered by the probe cache.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathKind {
    Missing,
    File,
    Dir,
    Other,
}

static PATH_KIND_CACHE: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<PathBuf, PathKind>>,
> = std::sync::OnceLock::new();

fn path_kind(path: &Path) -> PathKind {
    let stat = |path: &Path| match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => PathKind::File,
        Ok(meta) if meta.is_dir() => PathKind::Dir,
        Ok(_) => PathKind::Other,
        Err(_) => PathKind::Missing,
    };
    // Same switch as the canonical path cache: one-shot runs only. Misses
    // are remembered too, which is what makes candidate probing cheap
    // (most probes of a module resolution fail).
    if !CANONICAL_PATH_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return stat(path);
    }
    let cache = PATH_KIND_CACHE.get_or_init(Default::default);
    if let Some(kind) = cache.read().expect("path kind cache poisoned").get(path) {
        return *kind;
    }
    let kind = stat(path);
    cache
        .write()
        .expect("path kind cache poisoned")
        .insert(path.to_path_buf(), kind);
    kind
}

/// `Path::is_file` / `Path::is_dir` through the probe cache.
trait PathProbe {
    fn probe_is_file(&self) -> bool;
    fn probe_is_dir(&self) -> bool;
}

impl PathProbe for Path {
    fn probe_is_file(&self) -> bool {
        path_kind(self) == PathKind::File
    }

    fn probe_is_dir(&self) -> bool {
        path_kind(self) == PathKind::Dir
    }
}

static CANONICAL_PATH_CACHE_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static CANONICAL_PATH_CACHE: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<PathBuf, PathBuf>>,
> = std::sync::OnceLock::new();

/// Lets [`canonicalize`] remember its results for the rest of the process.
///
/// For one-shot runs (a CLI compile or check), where the filesystem is
/// taken as fixed: resolving a project canonicalized the same paths over a
/// million `readlink`s deep. Leave it off in long-lived processes (pipe
/// workers, the language server): a deploy that re-points a symlink must
/// be seen.
pub fn enable_canonical_path_cache() {
    CANONICAL_PATH_CACHE_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Turns the filesystem caches off again and empties them.
///
/// For long-lived processes that take one snapshot of a project (the
/// `--check-pipe` session): its init resolves the whole graph against a
/// filesystem it treats as fixed anyway, so it runs with the caches on and
/// calls this once the snapshot is built, leaving later requests to see
/// the disk as it is.
pub fn disable_and_clear_path_caches() {
    CANONICAL_PATH_CACHE_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
    fn clear<K, V>(
        cache: &std::sync::OnceLock<std::sync::RwLock<std::collections::HashMap<K, V>>>,
    ) {
        if let Some(cache) = cache.get() {
            if let Ok(mut map) = cache.write() {
                *map = std::collections::HashMap::new();
            }
        }
    }
    clear(&CANONICAL_PATH_CACHE);
    clear(&PATH_KIND_CACHE);
    clear(&PACKAGE_JSON_CACHE);
    clear(&NODE_MODULES_CACHE);
}

/// Whether [`enable_canonical_path_cache`] is in effect.
pub fn path_caches_enabled() -> bool {
    CANONICAL_PATH_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// `Path::canonicalize`, through the process-wide cache when
/// [`enable_canonical_path_cache`] was called. Only successes are cached
/// here; a missing path is answered by the probe cache ([`path_kind`]),
/// which remembers misses. Callers canonicalize candidate spellings
/// (`.ts`, `.d.ts`, `/index.ts`, …) that mostly do not exist, and each miss
/// was a `readlink` per path component: 1.7 M of them in an apps/app
/// `--check-pipe` init.
pub fn canonicalize(path: impl AsRef<Path>) -> Option<PathBuf> {
    let path = path.as_ref();
    if !CANONICAL_PATH_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return path.canonicalize().ok();
    }
    let cache = CANONICAL_PATH_CACHE.get_or_init(Default::default);
    if let Some(found) = cache
        .read()
        .expect("canonical path cache poisoned")
        .get(path)
    {
        return Some(found.clone());
    }
    if path_kind(path) == PathKind::Missing {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    cache
        .write()
        .expect("canonical path cache poisoned")
        .insert(path.to_path_buf(), canonical.clone());
    Some(canonical)
}

/// Extract a simple string value for a given key from JSON content.
/// This is a minimal parser that avoids the serde_json dependency.
fn extract_json_string_field(json: &str, field: &str) -> Option<String> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    // Skip whitespace and colon
    let after_colon = after_key.trim_start();
    let after_colon = after_colon.strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    // Extract string value
    let after_colon = after_colon.strip_prefix('"')?;
    let end = after_colon.find('"')?;
    Some(after_colon[..end].to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();

        // Create file structure:
        // root/
        //   src/
        //     main.ts
        //     utils.ts
        //     utils/
        //       index.ts
        //     helpers/
        //       math.ts
        //       math.d.ts
        //   node_modules/
        //     lodash/
        //       package.json
        //       index.js
        //     @types/
        //       lodash/
        //         index.d.ts

        let root = dir.path();
        fs::create_dir_all(root.join("src/utils")).unwrap();
        fs::create_dir_all(root.join("src/helpers")).unwrap();
        fs::create_dir_all(root.join("node_modules/lodash")).unwrap();
        fs::create_dir_all(root.join("node_modules/@types/lodash")).unwrap();
        fs::create_dir_all(root.join("node_modules/express")).unwrap();

        fs::write(root.join("src/main.ts"), "import './utils';").unwrap();
        fs::write(root.join("src/utils.ts"), "export const x = 1;").unwrap();
        fs::write(root.join("src/utils/index.ts"), "export const y = 2;").unwrap();
        fs::write(
            root.join("src/helpers/math.ts"),
            "export function add(a: number, b: number) { return a + b; }",
        )
        .unwrap();
        fs::write(
            root.join("src/helpers/math.d.ts"),
            "export declare function add(a: number, b: number): number;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/lodash/package.json"),
            r#"{ "name": "lodash", "main": "index.js" }"#,
        )
        .unwrap();
        fs::write(
            root.join("node_modules/lodash/index.js"),
            "module.exports = {};",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@types/lodash/index.d.ts"),
            "declare const _: any; export = _;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/express/index.js"),
            "module.exports = {};",
        )
        .unwrap();

        dir
    }

    #[test]
    fn relative_path_with_extension() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions::default();

        let result = resolve_module_name("./utils", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("utils.ts"));
        assert!(!resolved.is_external_library_import);
    }

    #[test]
    fn relative_path_ts_extension_first() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions::default();

        // helpers/math has both .ts and .d.ts, .ts should be tried first
        let result = resolve_module_name("./helpers/math", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("math.ts"));
    }

    #[test]
    fn index_file_resolution() {
        let _dir = setup_test_dir();
        let opts = CompilerOptions::default();

        // Create a directory that only has index.ts (no utils.ts at top level)
        let other_dir = tempfile::tempdir().unwrap();
        let root = other_dir.path();
        fs::create_dir_all(root.join("src/lib")).unwrap();
        fs::write(root.join("src/main.ts"), "").unwrap();
        fs::write(root.join("src/lib/index.ts"), "export const z = 3;").unwrap();

        let containing2 = root.join("src/main.ts");
        let result = resolve_module_name("./lib", containing2.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("index.ts"));
    }

    #[test]
    fn node_modules_resolution() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions::default();

        let result = resolve_module_name("lodash", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("lodash"));
        assert!(resolved.is_external_library_import);
    }

    #[test]
    fn node_modules_with_package_json_main() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions::default();

        let result = resolve_module_name("lodash", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("index.js"));
    }

    #[test]
    fn nonexistent_module_returns_none() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions::default();

        let result = resolve_module_name("nonexistent-module", containing.to_str().unwrap(), &opts);
        assert!(result.is_none());
    }

    #[test]
    fn relative_nonexistent_returns_none() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions::default();

        let result = resolve_module_name("./does_not_exist", containing.to_str().unwrap(), &opts);
        assert!(result.is_none());
    }

    #[test]
    fn classic_resolution_relative() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions {
            module_resolution: Some("classic".to_string()),
            ..Default::default()
        };

        let result = resolve_module_name("./helpers/math", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("math.ts"));
        assert!(!resolved.is_external_library_import);
    }

    #[test]
    fn classic_resolution_non_relative_walks_up() {
        let dir = setup_test_dir();
        // Create a file at root for classic resolution to find
        fs::write(dir.path().join("globals.ts"), "declare const G: any;").unwrap();

        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions {
            module_resolution: Some("classic".to_string()),
            ..Default::default()
        };

        let result = resolve_module_name("globals", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("globals.ts"));
    }

    #[test]
    fn path_mapping_base_url() {
        let dir = setup_test_dir();
        let opts = CompilerOptions {
            base_url: Some(dir.path().join("src").to_str().unwrap().to_string()),
            ..Default::default()
        };
        let containing = dir.path().join("src/helpers/math.ts");

        // With baseUrl = src, "utils" should resolve to src/utils.ts
        let result = resolve_module_name("utils", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("utils.ts"));
    }

    #[test]
    fn path_mapping_with_wildcard() {
        let dir = setup_test_dir();
        let src = dir.path().join("src");
        let opts = CompilerOptions {
            base_url: Some(src.to_str().unwrap().to_string()),
            paths: Some(r#"{ "@helpers/*": ["helpers/*"] }"#.to_string()),
            ..Default::default()
        };
        let containing = dir.path().join("src/main.ts");

        let result = resolve_module_name("@helpers/math", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.contains("math.ts"));
    }

    #[test]
    fn relative_path_with_explicit_extension() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions::default();

        let result = resolve_module_name("./utils.ts", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.ends_with("/src/utils.ts"));
    }

    #[test]
    fn package_json_types_preferred_over_main() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();

        fs::write(root.join("src/main.ts"), "import 'pkg';").unwrap();
        fs::write(
            root.join("node_modules/pkg/index.js"),
            "module.exports = {};",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/pkg/index.d.ts"),
            "export declare const x: number;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/pkg/package.json"),
            r#"{ "name": "pkg", "types": "index.d.ts", "main": "index.js" }"#,
        )
        .unwrap();

        let containing = root.join("src/main.ts");
        let opts = CompilerOptions::default();
        let result = resolve_module_name("pkg", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved
            .resolved_file_name
            .ends_with("/node_modules/pkg/index.d.ts"));
        assert!(resolved.is_external_library_import);
    }

    #[test]
    fn node_modules_package_subpath_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules/lodash/fp")).unwrap();

        fs::write(root.join("src/main.ts"), "import 'lodash/fp';").unwrap();
        fs::write(
            root.join("node_modules/lodash/fp/index.js"),
            "module.exports = {};",
        )
        .unwrap();

        let containing = root.join("src/main.ts");
        let opts = CompilerOptions::default();
        let result = resolve_module_name("lodash/fp", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved
            .resolved_file_name
            .contains("node_modules/lodash/fp/index.js"));
        assert!(resolved.is_external_library_import);
    }

    #[test]
    fn path_mapping_exact_key_without_wildcard() {
        let dir = setup_test_dir();
        let src = dir.path().join("src");
        let opts = CompilerOptions {
            base_url: Some(src.to_str().unwrap().to_string()),
            paths: Some(r#"{ "@utils": ["utils"] }"#.to_string()),
            ..Default::default()
        };
        let containing = dir.path().join("src/main.ts");

        let result = resolve_module_name("@utils", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.ends_with("/src/utils.ts"));
    }

    #[test]
    fn parse_paths_entries_multiple_patterns() {
        let entries =
            parse_paths_entries(r#"{ "@a/*": ["src/a/*", "gen/a/*"], "@b": ["src/b/index"] }"#);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "@a/*");
        assert_eq!(entries[0].1, vec!["src/a/*", "gen/a/*"]);
        assert_eq!(entries[1].0, "@b");
        assert_eq!(entries[1].1, vec!["src/b/index"]);
    }

    #[test]
    fn module_resolution_kind_parse_variants() {
        assert_eq!(
            ModuleResolutionKind::parse("bundler"),
            Some(ModuleResolutionKind::Node)
        );
        assert_eq!(
            ModuleResolutionKind::parse("node16"),
            Some(ModuleResolutionKind::Node)
        );
        assert_eq!(
            ModuleResolutionKind::parse("classic"),
            Some(ModuleResolutionKind::Classic)
        );
        assert_eq!(ModuleResolutionKind::parse("unknown-kind"), None);
    }

    #[test]
    fn unknown_module_resolution_falls_back_to_node() {
        let dir = setup_test_dir();
        let containing = dir.path().join("src/main.ts");
        let opts = CompilerOptions {
            module_resolution: Some("not-a-real-kind".to_string()),
            ..Default::default()
        };

        let result = resolve_module_name("lodash", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved
            .resolved_file_name
            .contains("node_modules/lodash/index.js"));
        assert!(resolved.is_external_library_import);
    }

    #[test]
    fn package_json_main_without_extension_is_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg/dist")).unwrap();

        fs::write(root.join("src/main.ts"), "import 'pkg';").unwrap();
        fs::write(
            root.join("node_modules/pkg/dist/index.js"),
            "module.exports = {};",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/pkg/package.json"),
            r#"{ "name": "pkg", "main": "dist/index" }"#,
        )
        .unwrap();

        let containing = root.join("src/main.ts");
        let opts = CompilerOptions::default();
        let result = resolve_module_name("pkg", containing.to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved
            .resolved_file_name
            .ends_with("/node_modules/pkg/dist/index.js"));
    }

    #[test]
    fn path_mapping_base_url_index_file_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/lib")).unwrap();
        fs::write(root.join("src/main.ts"), "import 'lib';").unwrap();
        fs::write(root.join("src/lib/index.ts"), "export const x = 1;").unwrap();

        let opts = CompilerOptions {
            base_url: Some(root.join("src").to_str().unwrap().to_string()),
            ..Default::default()
        };

        let result = resolve_module_name("lib", root.join("src/main.ts").to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.resolved_file_name.ends_with("/src/lib/index.ts"));
    }

    #[test]
    fn node_modules_direct_file_resolution_without_package_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::write(root.join("src/main.ts"), "import 'leftpad';").unwrap();
        fs::write(
            root.join("node_modules/leftpad.ts"),
            "export const leftpad = 1;",
        )
        .unwrap();

        let opts = CompilerOptions::default();
        let result =
            resolve_module_name("leftpad", root.join("src/main.ts").to_str().unwrap(), &opts);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved
            .resolved_file_name
            .ends_with("/node_modules/leftpad.ts"));
        assert!(resolved.is_external_library_import);
    }

    #[test]
    fn split_package_scoped_without_subpath() {
        let (pkg, sub) = split_package_subpath("@scope/pkg");
        assert_eq!(pkg, "@scope/pkg");
        assert_eq!(sub, None);
    }

    #[test]
    fn split_package_scoped() {
        let (pkg, sub) = split_package_subpath("@scope/pkg/sub/path");
        assert_eq!(pkg, "@scope/pkg");
        assert_eq!(sub, Some("sub/path"));
    }

    #[test]
    fn split_package_regular() {
        let (pkg, sub) = split_package_subpath("lodash/fp");
        assert_eq!(pkg, "lodash");
        assert_eq!(sub, Some("fp"));
    }

    #[test]
    fn split_package_no_subpath() {
        let (pkg, sub) = split_package_subpath("lodash");
        assert_eq!(pkg, "lodash");
        assert_eq!(sub, None);
    }

    #[test]
    fn is_relative_detection() {
        assert!(is_relative_import("./foo"));
        assert!(is_relative_import("../bar"));
        assert!(!is_relative_import("lodash"));
        assert!(!is_relative_import("@types/node"));
    }

    #[test]
    fn match_path_pattern_wildcard() {
        assert_eq!(
            match_path_pattern("@app/utils/math", "@app/*"),
            Some("utils/math".to_string())
        );
        assert_eq!(match_path_pattern("@app/", "@app/*"), Some(String::new()));
        assert_eq!(match_path_pattern("other/path", "@app/*"), None);
    }

    #[test]
    fn match_path_pattern_exact() {
        assert_eq!(match_path_pattern("jquery", "jquery"), Some(String::new()));
        assert_eq!(match_path_pattern("jqueryui", "jquery"), None);
    }

    #[test]
    fn extract_json_field_works() {
        let json = r#"{ "name": "lodash", "main": "index.js", "types": "index.d.ts" }"#;
        assert_eq!(
            extract_json_string_field(json, "main"),
            Some("index.js".to_string())
        );
        assert_eq!(
            extract_json_string_field(json, "types"),
            Some("index.d.ts".to_string())
        );
        assert_eq!(extract_json_string_field(json, "missing"), None);
    }

    #[test]
    fn bootstrap_resolver_returns_none() {
        let resolver = BootstrapResolver;
        let result = resolver.resolve("/src/main.ts", "./utils");
        assert!(result.resolved_path.is_none());
        assert_eq!(result.module_name, "./utils");
    }

    fn setup_exports_pkg() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Site that imports through subpath exports.
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules/@scope/framework/src")).unwrap();
        fs::create_dir_all(root.join("node_modules/@scope/framework/src/sub")).unwrap();

        fs::write(
            root.join("src/main.ts"),
            "import { x } from '@scope/framework';",
        )
        .unwrap();
        // Framework manifest with exports map (mirrors @bext-stack/framework shape).
        fs::write(
            root.join("node_modules/@scope/framework/package.json"),
            r#"{
                "name": "@scope/framework",
                "exports": {
                    ".": "./src/index.ts",
                    "./streaming": "./src/streaming.ts",
                    "./sub/inner": "./src/sub/inner.ts",
                    "./conditioned": {
                        "types": "./src/types.d.ts",
                        "import": "./src/esm.ts",
                        "default": "./src/cjs.ts"
                    }
                }
            }"#,
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/framework/src/index.ts"),
            "export const a = 1;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/framework/src/streaming.ts"),
            "export const stream = 1;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/framework/src/sub/inner.ts"),
            "export const inner = 1;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/framework/src/types.d.ts"),
            "export declare const t: number;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/framework/src/esm.ts"),
            "export const esm = 1;",
        )
        .unwrap();
        dir
    }

    #[test]
    fn exports_map_root_resolves() {
        let dir = setup_exports_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/framework",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("root export should resolve");
        assert!(
            r.resolved_file_name.ends_with("/src/index.ts"),
            "got {}",
            r.resolved_file_name
        );
    }

    #[test]
    fn exports_map_subpath_resolves() {
        let dir = setup_exports_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/framework/streaming",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("./streaming should resolve via exports map");
        assert!(
            r.resolved_file_name.ends_with("/src/streaming.ts"),
            "got {}",
            r.resolved_file_name
        );
    }

    #[test]
    fn exports_map_nested_subpath_resolves() {
        let dir = setup_exports_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/framework/sub/inner",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("nested subpath should resolve");
        assert!(
            r.resolved_file_name.ends_with("/src/sub/inner.ts"),
            "got {}",
            r.resolved_file_name
        );
    }

    #[test]
    fn exports_map_condition_picks_types_first() {
        let dir = setup_exports_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/framework/conditioned",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("conditioned export should resolve via priority chain");
        // `types` comes first in the priority chain.
        assert!(
            r.resolved_file_name.ends_with("/src/types.d.ts"),
            "got {}",
            r.resolved_file_name
        );
    }

    fn setup_exports_wildcard_pkg() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules/@scope/lib/dist/feature")).unwrap();
        fs::create_dir_all(root.join("node_modules/@scope/lib/dist/util")).unwrap();

        fs::write(root.join("src/main.ts"), "import './unused';").unwrap();
        // Wildcard pattern + an exact match that should be preferred when
        // both apply.
        fs::write(
            root.join("node_modules/@scope/lib/package.json"),
            r#"{
                "name": "@scope/lib",
                "exports": {
                    "./feature/*": "./dist/feature/*.ts",
                    "./feature/special": "./dist/feature/special-override.ts",
                    "./util/*.js": "./dist/util/*.ts"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/lib/dist/feature/foo.ts"),
            "export const foo = 1;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/lib/dist/feature/nested/bar.ts"),
            "export const bar = 1;",
        )
        .unwrap_or_else(|_| {
            fs::create_dir_all(root.join("node_modules/@scope/lib/dist/feature/nested")).unwrap();
            fs::write(
                root.join("node_modules/@scope/lib/dist/feature/nested/bar.ts"),
                "export const bar = 1;",
            )
            .unwrap();
        });
        fs::write(
            root.join("node_modules/@scope/lib/dist/feature/special-override.ts"),
            "export const special = 1;",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/@scope/lib/dist/util/helpers.ts"),
            "export const helpers = 1;",
        )
        .unwrap();
        dir
    }

    #[test]
    fn exports_map_wildcard_resolves_simple_capture() {
        let dir = setup_exports_wildcard_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/lib/feature/foo",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("wildcard should resolve");
        assert!(
            r.resolved_file_name.ends_with("/dist/feature/foo.ts"),
            "got {}",
            r.resolved_file_name
        );
    }

    #[test]
    fn exports_map_wildcard_captures_nested_path() {
        let dir = setup_exports_wildcard_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/lib/feature/nested/bar",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("wildcard capture should accept slashes");
        assert!(
            r.resolved_file_name
                .ends_with("/dist/feature/nested/bar.ts"),
            "got {}",
            r.resolved_file_name
        );
    }

    #[test]
    fn exports_map_exact_match_beats_wildcard() {
        let dir = setup_exports_wildcard_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/lib/feature/special",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("exact key should resolve");
        assert!(
            r.resolved_file_name
                .ends_with("/dist/feature/special-override.ts"),
            "got {} — exact match should outrank `./feature/*`",
            r.resolved_file_name
        );
    }

    #[test]
    fn exports_map_wildcard_with_suffix() {
        let dir = setup_exports_wildcard_pkg();
        let main = dir.path().join("src/main.ts");
        let resolved = resolve_module_name(
            "@scope/lib/util/helpers.js",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        let r = resolved.expect("`./util/*.js` pattern should match `./util/helpers.js`");
        assert!(
            r.resolved_file_name.ends_with("/dist/util/helpers.ts"),
            "got {}",
            r.resolved_file_name
        );
    }

    #[test]
    fn exports_map_unknown_subpath_falls_back_legacy() {
        let dir = setup_exports_pkg();
        let main = dir.path().join("src/main.ts");
        // For an unknown subpath, the new exports-map path returns None,
        // so resolution falls through to the legacy bare-subpath check.
        // The package has no `not-in-map.ts` at its root, so legacy also
        // returns None — but the loop continues to the parent `dir.pop()`
        // and walks node_modules upward looking for `@scope/framework/...`.
        // In a sandboxed tempdir there's no parent, so we get None.
        // What this test really asserts: the new exports-map code doesn't
        // *synthesise* a path for unknown subpaths.
        let resolved = resolve_module_name(
            "@scope/framework/not-in-map",
            main.to_str().unwrap(),
            &CompilerOptions::default(),
        );
        // If something resolves, it must NOT be the package's index.ts —
        // that would mean we incorrectly fell back to the root export.
        if let Some(r) = resolved {
            assert!(
                !r.resolved_file_name.ends_with("/src/index.ts"),
                "unknown subpath wrongly resolved to root export: {}",
                r.resolved_file_name
            );
        }
    }
}
