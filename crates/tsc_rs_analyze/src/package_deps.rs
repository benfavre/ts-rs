//! package.json dependency audit.
//!
//! Compares declared dependencies in `package.json` against the actual
//! bare-package imports observed in an `ImportGraph`. Reports:
//!
//! - **declared but unused**: deps that ship in `node_modules` but no source
//!   file imports them — overinstall, candidates for removal.
//! - **used but undeclared**: bare imports that resolve at build time only
//!   because of monorepo / workspace hoisting. A fresh install (or strict
//!   pnpm setup) would break them. This is the failure mode that caught
//!   `marked` leaking into bext's `sites/demo`.
//!
//! Workspace-protocol entries (`workspace:*`, `link:..`, `file:..`) and
//! Node built-ins (`node:fs`, `fs`, `path`, …) are excluded — neither set
//! has anything to do with package-manifest correctness.

use std::collections::{BTreeMap, BTreeSet};

use crate::import_graph::{ImportGraph, PackageUsage};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of comparing a `package.json` against the import graph.
#[derive(Debug)]
pub struct PackageDepsAudit {
    /// Declared in `dependencies` / `devDependencies` but never imported AND
    /// not required by a peer of a workspace dep. These are the truly safe-
    /// to-remove entries.
    pub unused: BTreeSet<String>,
    /// Declared and not imported by source, but listed in the
    /// peerDependencies of a workspace-protocol dep — i.e. the workspace
    /// package's compiled output `require`s these at runtime, so removing
    /// them would break SSR. Kept separate from `unused` so the
    /// "safe-to-remove" set doesn't include them.
    pub peer_required: BTreeMap<String, BTreeSet<String>>,
    /// Imported at runtime but absent from the manifest.
    pub undeclared: BTreeMap<String, PackageUsage>,
    /// Imported only via `import type` and absent from the manifest.
    /// Less urgent than `undeclared` (no runtime risk) but still drift.
    pub undeclared_type_only: BTreeMap<String, PackageUsage>,
    /// Declared and imported — the healthy set.
    pub matched: BTreeSet<String>,
    pub stats: PackageDepsStats,
}

#[derive(Debug)]
pub struct PackageDepsStats {
    pub declared_count: usize,
    pub imported_count: usize,
    pub matched_count: usize,
    pub unused_count: usize,
    pub peer_required_count: usize,
    pub undeclared_runtime_count: usize,
    pub undeclared_type_only_count: usize,
}

/// Parsed `package.json` minimal subset — only the dep maps + name.
#[derive(Debug, Default, Clone)]
pub struct PackageManifest {
    pub name: Option<String>,
    /// `dependencies` keys, with the version spec preserved so callers can
    /// filter `workspace:*` / `link:*` if they care (we filter by default).
    pub dependencies: BTreeMap<String, String>,
    pub dev_dependencies: BTreeMap<String, String>,
    pub peer_dependencies: BTreeMap<String, String>,
    pub optional_dependencies: BTreeMap<String, String>,
}

impl PackageManifest {
    /// All declared package names across `dependencies` +
    /// `devDependencies` + `peerDependencies` + `optionalDependencies`,
    /// regardless of version-spec shape. Used for the "undeclared at all"
    /// check — a workspace-protocol entry is still declared (it's resolved
    /// by the monorepo, not npm), so we must not flag it as undeclared.
    pub fn declared_all(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for map in [
            &self.dependencies,
            &self.dev_dependencies,
            &self.peer_dependencies,
            &self.optional_dependencies,
        ] {
            out.extend(map.keys().cloned());
        }
        out
    }

    /// Subset of `declared_all` that excludes `workspace:` / `link:` / `file:`
    /// / `portal:` entries. These specs are satisfied by the monorepo
    /// regardless of usage and may legitimately ship without an importer
    /// (e.g. a workspace meta-package that aggregates type re-exports), so
    /// the "unused" report only considers the registry-backed subset where
    /// "no importer found" is actually actionable.
    pub fn declared_registry(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for map in [
            &self.dependencies,
            &self.dev_dependencies,
            &self.peer_dependencies,
            &self.optional_dependencies,
        ] {
            for (k, v) in map {
                if is_workspace_spec(v) {
                    continue;
                }
                out.insert(k.clone());
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Audit
// ---------------------------------------------------------------------------

pub fn audit_package_deps(graph: &ImportGraph, manifest: &PackageManifest) -> PackageDepsAudit {
    audit_package_deps_with_peers(graph, manifest, &BTreeMap::new())
}

/// Like `audit_package_deps`, but takes a map `peer_required[pkg] →
/// {workspace_deps_that_require_it}`. Used to suppress false positives
/// where a workspace dep's compiled output `require`s a package at
/// runtime (the workspace's `peerDependencies`), so the site MUST keep
/// the entry even though no source file imports it directly.
///
/// The CLI populates this map by reading each declared `workspace:*` /
/// `link:*` dep's `package.json` and collecting its `peerDependencies` +
/// non-workspace `dependencies`.
pub fn audit_package_deps_with_peers(
    graph: &ImportGraph,
    manifest: &PackageManifest,
    peer_required_by: &BTreeMap<String, BTreeSet<String>>,
) -> PackageDepsAudit {
    let declared_all = manifest.declared_all();
    let declared_registry = manifest.declared_registry();

    let mut imported_runtime: BTreeMap<String, PackageUsage> = BTreeMap::new();
    let mut imported_type_only: BTreeMap<String, PackageUsage> = BTreeMap::new();
    for (pkg, usage) in &graph.packages {
        if is_node_builtin(pkg) {
            continue;
        }
        if usage.type_only {
            imported_type_only.insert(pkg.clone(), usage.clone());
        } else {
            imported_runtime.insert(pkg.clone(), usage.clone());
        }
    }

    let imported_keys: BTreeSet<String> = imported_runtime
        .keys()
        .chain(imported_type_only.keys())
        .cloned()
        .collect();

    // Walk declared registry-backed deps. Split into "unused" (not imported
    // and not required by a workspace peer) vs "peer_required" (not imported
    // but a workspace dep needs it at runtime).
    let mut unused = BTreeSet::new();
    let mut peer_required: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for d in &declared_registry {
        if imported_keys.contains(d) {
            continue;
        }
        if let Some(requirers) = peer_required_by.get(d) {
            peer_required.insert(d.clone(), requirers.clone());
        } else {
            unused.insert(d.clone());
        }
    }

    // "Undeclared" runs against `declared_all` — workspace specs are still
    // declared (just resolved locally), so an import of `@scope/pkg` whose
    // entry is `"workspace:*"` should NOT show up as undeclared.
    let mut undeclared = BTreeMap::new();
    for (pkg, usage) in &imported_runtime {
        if !declared_all.contains(pkg) {
            undeclared.insert(pkg.clone(), usage.clone());
        }
    }

    let mut undeclared_type_only = BTreeMap::new();
    for (pkg, usage) in &imported_type_only {
        if !declared_all.contains(pkg) && !undeclared.contains_key(pkg) {
            undeclared_type_only.insert(pkg.clone(), usage.clone());
        }
    }

    let matched: BTreeSet<String> = declared_all
        .iter()
        .filter(|d| imported_keys.contains(*d))
        .cloned()
        .collect();

    let stats = PackageDepsStats {
        declared_count: declared_all.len(),
        imported_count: imported_keys.len(),
        matched_count: matched.len(),
        unused_count: unused.len(),
        peer_required_count: peer_required.len(),
        undeclared_runtime_count: undeclared.len(),
        undeclared_type_only_count: undeclared_type_only.len(),
    };

    PackageDepsAudit {
        unused,
        peer_required,
        undeclared,
        undeclared_type_only,
        matched,
        stats,
    }
}

/// Resolve a manifest's `workspace:*` / `link:*` / `file:*` deps and
/// return a map `package_name → {workspace_deps_requiring_it}`. The
/// inverse direction is handy for the audit's `peer_required` reporting
/// (operators see "react-dom is needed by @bext-stack/framework").
///
/// Reads each workspace dep's `package.json` from the location it
/// resolves to under `<site_dir>/node_modules/<name>/package.json` —
/// pnpm/bun/yarn-workspace symlink it, so the canonical path is the real
/// workspace package. If the file isn't there (cold install, partial
/// state), the entry is silently skipped — operators still get the
/// regular "unused" report, just without the peer-required carve-out.
pub fn collect_workspace_peer_requirements(
    manifest: &PackageManifest,
    site_dir: &std::path::Path,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut requirers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for map in [
        &manifest.dependencies,
        &manifest.dev_dependencies,
        &manifest.peer_dependencies,
        &manifest.optional_dependencies,
    ] {
        for (dep_name, spec) in map {
            if !is_workspace_spec(spec) {
                continue;
            }
            let dep_pkg_json = site_dir
                .join("node_modules")
                .join(dep_name)
                .join("package.json");
            let Ok(content) = std::fs::read_to_string(&dep_pkg_json) else {
                continue;
            };
            let Ok(parsed) = parse_manifest(&content) else {
                continue;
            };
            // peerDependencies AND non-workspace runtime dependencies of
            // the workspace dep are both runtime requirements on the
            // consuming site — when the workspace package's compiled
            // output runs, it `require`s these from the site's
            // node_modules. peerDependenciesMeta.optional handling is
            // out of scope (operators rarely set this).
            for (peer, _) in &parsed.peer_dependencies {
                requirers
                    .entry(peer.clone())
                    .or_default()
                    .insert(dep_name.clone());
            }
            for (sub, sub_spec) in &parsed.dependencies {
                if is_workspace_spec(sub_spec) {
                    continue;
                }
                requirers
                    .entry(sub.clone())
                    .or_default()
                    .insert(dep_name.clone());
            }
        }
    }
    requirers
}

// ---------------------------------------------------------------------------
// Manifest parsing
// ---------------------------------------------------------------------------

/// Parse a `package.json` file. Uses a deliberately minimal parser — we only
/// care about dep maps, and pulling in serde just for this would balloon the
/// crate's deps. Returns `Err(String)` with a human message on failure.
pub fn parse_manifest(content: &str) -> Result<PackageManifest, String> {
    let mut manifest = PackageManifest::default();
    manifest.name = extract_string_field(content, "name");
    manifest.dependencies = extract_string_map(content, "dependencies");
    manifest.dev_dependencies = extract_string_map(content, "devDependencies");
    manifest.peer_dependencies = extract_string_map(content, "peerDependencies");
    manifest.optional_dependencies = extract_string_map(content, "optionalDependencies");
    if manifest.dependencies.is_empty()
        && manifest.dev_dependencies.is_empty()
        && manifest.peer_dependencies.is_empty()
        && manifest.optional_dependencies.is_empty()
    {
        // Empty is plausible (a leaf workspace with no deps), but if the file
        // looks like JSON-with-content yet we found nothing, surface it.
        if !content.trim().is_empty() && content.contains('{') && content.contains('}') {
            // No-op — empty deps are legal. The caller decides whether that's
            // interesting. Keeping the manifest with empty maps is fine.
        }
    }
    Ok(manifest)
}

fn extract_string_field(content: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\"");
    let pos = content.find(&needle)?;
    let after = &content[pos + needle.len()..];
    let colon = after.find(':')?;
    let rest = &after[colon + 1..];
    let q = rest.find('"')?;
    let after_q = &rest[q + 1..];
    let end_q = after_q.find('"')?;
    Some(after_q[..end_q].to_string())
}

/// Extract `"<name>": { "key": "val", ... }` as a BTreeMap. Tolerates
/// arbitrary whitespace, inner braces in values (rare), and trailing commas.
fn extract_string_map(content: &str, name: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let needle = format!("\"{name}\"");
    let Some(pos) = content.find(&needle) else {
        return out;
    };
    let after = &content[pos + needle.len()..];
    let Some(brace_rel) = after.find('{') else {
        return out;
    };
    let from_brace = &after[brace_rel..];
    // Find the matching close brace.
    let mut depth: i32 = 0;
    let mut end_idx: Option<usize> = None;
    for (i, ch) in from_brace.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(end) = end_idx else {
        return out;
    };
    let body = &from_brace[1..end];

    // Walk pairs.
    let mut chars = body.chars().peekable();
    while let Some(_) = chars.peek() {
        // Skip whitespace + commas
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else {
                break;
            }
        }
        // Read key string
        if chars.peek() != Some(&'"') {
            break;
        }
        chars.next(); // consume opening quote
        let mut key = String::new();
        while let Some(c) = chars.next() {
            if c == '"' {
                break;
            }
            if c == '\\' {
                if let Some(esc) = chars.next() {
                    key.push(esc);
                }
                continue;
            }
            key.push(c);
        }
        // Skip whitespace + colon
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ':' {
                chars.next();
            } else {
                break;
            }
        }
        // Read value string
        if chars.peek() != Some(&'"') {
            // Skip non-string values defensively (e.g. a nested object).
            while let Some(c) = chars.next() {
                if c == ',' {
                    break;
                }
            }
            continue;
        }
        chars.next(); // consume opening quote
        let mut val = String::new();
        while let Some(c) = chars.next() {
            if c == '"' {
                break;
            }
            if c == '\\' {
                if let Some(esc) = chars.next() {
                    val.push(esc);
                }
                continue;
            }
            val.push(c);
        }
        if !key.is_empty() {
            out.insert(key, val);
        }
    }
    out
}

fn is_workspace_spec(spec: &str) -> bool {
    let s = spec.trim();
    s.starts_with("workspace:")
        || s.starts_with("link:")
        || s.starts_with("file:")
        || s.starts_with("portal:")
}

/// Built-in module names that don't belong in `dependencies`. We compare the
/// bare package head only (so `node:fs/promises` matches via the `node:`
/// prefix or `fs` head). The list isn't exhaustive — it's the common set
/// that shows up in bext + Next.js codebases.
///
/// Also covers Bun (`bun:sqlite`, `bun:test`) and Deno (`deno:*`) runtime
/// builtins — these resolve at runtime, not from `node_modules`, so flagging
/// them as "undeclared" creates a false positive on Bun-targeted bext sites.
fn is_node_builtin(pkg: &str) -> bool {
    if pkg.starts_with("node:") || pkg.starts_with("bun:") || pkg.starts_with("deno:") {
        return true;
    }
    matches!(
        pkg,
        "assert"
            | "async_hooks"
            | "buffer"
            | "child_process"
            | "cluster"
            | "console"
            | "constants"
            | "crypto"
            | "dgram"
            | "diagnostics_channel"
            | "dns"
            | "domain"
            | "events"
            | "fs"
            | "fs/promises"
            | "http"
            | "http2"
            | "https"
            | "inspector"
            | "module"
            | "net"
            | "os"
            | "path"
            | "path/posix"
            | "path/win32"
            | "perf_hooks"
            | "process"
            | "punycode"
            | "querystring"
            | "readline"
            | "readline/promises"
            | "repl"
            | "stream"
            | "stream/consumers"
            | "stream/promises"
            | "stream/web"
            | "string_decoder"
            | "sys"
            | "timers"
            | "timers/promises"
            | "tls"
            | "trace_events"
            | "tty"
            | "url"
            | "util"
            | "util/types"
            | "v8"
            | "vm"
            | "wasi"
            | "worker_threads"
            | "zlib"
    )
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

impl PackageDepsAudit {
    pub fn format_report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Package Deps Audit: {matched} matched, {undeclared} undeclared (runtime), {type_only} undeclared (type-only), {unused} unused, {peer} peer-required — out of {declared} declared / {imported} imported\n\n",
            matched = self.stats.matched_count,
            undeclared = self.stats.undeclared_runtime_count,
            type_only = self.stats.undeclared_type_only_count,
            unused = self.stats.unused_count,
            peer = self.stats.peer_required_count,
            declared = self.stats.declared_count,
            imported = self.stats.imported_count,
        ));

        if !self.undeclared.is_empty() {
            out.push_str(&format!(
                "=== UNDECLARED (RUNTIME) ({}) — add to package.json or risk install breakage ===\n",
                self.undeclared.len()
            ));
            for (pkg, usage) in &self.undeclared {
                out.push_str(&format!("  {pkg}  ({} imports)\n", usage.import_count));
                let chain = shorten_chain(&usage.chain.files, 3);
                out.push_str(&format!(
                    "    via: {chain} → import \"{}\"\n",
                    usage.chain.specifier
                ));
            }
            out.push('\n');
        }

        if !self.undeclared_type_only.is_empty() {
            out.push_str(&format!(
                "=== UNDECLARED (TYPE-ONLY) ({}) — drift, no runtime risk ===\n",
                self.undeclared_type_only.len()
            ));
            for (pkg, usage) in &self.undeclared_type_only {
                out.push_str(&format!("  {pkg}  ({} imports)\n", usage.import_count));
            }
            out.push('\n');
        }

        if !self.peer_required.is_empty() {
            out.push_str(&format!(
                "=== PEER-REQUIRED ({}) — keep (workspace dep needs at runtime) ===\n",
                self.peer_required.len()
            ));
            for (pkg, requirers) in &self.peer_required {
                let req_list: Vec<&str> = requirers.iter().map(|s| s.as_str()).collect();
                out.push_str(&format!(
                    "  {pkg}  (required by: {})\n",
                    req_list.join(", ")
                ));
            }
            out.push('\n');
        }

        if !self.unused.is_empty() {
            out.push_str(&format!(
                "=== UNUSED ({}) — declared, no source imports, no workspace peer needs ===\n",
                self.unused.len()
            ));
            for pkg in &self.unused {
                out.push_str(&format!("  {pkg}\n"));
            }
            out.push('\n');
        }

        out
    }
}

use crate::utils::shorten_path;

fn shorten_chain(files: &[String], max_tail: usize) -> String {
    if files.len() <= max_tail + 1 {
        return files
            .iter()
            .map(|f| shorten_path(f))
            .collect::<Vec<_>>()
            .join(" → ");
    }
    let first = shorten_path(&files[0]);
    let tail: Vec<String> = files[files.len() - max_tail..]
        .iter()
        .map(|f| shorten_path(f))
        .collect();
    format!("{first} → ... → {}", tail.join(" → "))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_manifest() {
        let pkg = r#"{
            "name": "demo",
            "version": "0.1.0",
            "dependencies": {
                "marked": "^12.0.0",
                "@scope/lib": "workspace:*"
            },
            "devDependencies": {
                "typescript": "^5.0.0"
            }
        }"#;
        let m = parse_manifest(pkg).unwrap();
        assert_eq!(m.name.as_deref(), Some("demo"));
        assert_eq!(
            m.dependencies.get("marked").map(String::as_str),
            Some("^12.0.0")
        );
        assert_eq!(
            m.dependencies.get("@scope/lib").map(String::as_str),
            Some("workspace:*")
        );
        assert_eq!(
            m.dev_dependencies.get("typescript").map(String::as_str),
            Some("^5.0.0")
        );
    }

    #[test]
    fn declared_all_includes_workspace_specs() {
        let pkg = r#"{
            "dependencies": {
                "marked": "^12.0.0",
                "@bext-stack/framework": "workspace:*"
            }
        }"#;
        let m = parse_manifest(pkg).unwrap();
        let all = m.declared_all();
        assert!(all.contains("marked"));
        assert!(all.contains("@bext-stack/framework"));
    }

    #[test]
    fn declared_registry_skips_workspace_specs() {
        let pkg = r#"{
            "dependencies": {
                "marked": "^12.0.0",
                "@bext-stack/framework": "workspace:*"
            }
        }"#;
        let m = parse_manifest(pkg).unwrap();
        let registry = m.declared_registry();
        assert!(registry.contains("marked"));
        assert!(!registry.contains("@bext-stack/framework"));
    }

    #[test]
    fn workspace_spec_detection() {
        assert!(is_workspace_spec("workspace:*"));
        assert!(is_workspace_spec("workspace:^"));
        assert!(is_workspace_spec("link:../foo"));
        assert!(is_workspace_spec("file:./pkg"));
        assert!(!is_workspace_spec("^1.2.3"));
        assert!(!is_workspace_spec("1.2.3"));
    }

    #[test]
    fn node_builtin_detection() {
        assert!(is_node_builtin("fs"));
        assert!(is_node_builtin("node:crypto"));
        assert!(is_node_builtin("path"));
        assert!(!is_node_builtin("marked"));
        assert!(!is_node_builtin("@bext-stack/framework"));
    }

    #[test]
    fn bun_and_deno_builtins_are_recognized() {
        assert!(is_node_builtin("bun:sqlite"));
        assert!(is_node_builtin("bun:test"));
        assert!(is_node_builtin("deno:foo"));
        assert!(!is_node_builtin("bun-types"));
    }

    #[test]
    fn peer_required_lifts_dep_out_of_unused() {
        use crate::import_graph::{ImportEdge, ImportGraph};
        use std::collections::{BTreeMap, HashMap, HashSet};

        let manifest_str = r#"{
            "dependencies": {
                "@scope/framework": "workspace:*",
                "react-dom": "^19.0.0",
                "stale-pkg": "^1.0.0"
            }
        }"#;
        let manifest = parse_manifest(manifest_str).unwrap();

        let graph = ImportGraph {
            edges: HashMap::new(),
            packages: BTreeMap::new(),
            visited_files: HashSet::new(),
            unresolved: Vec::new(),
            read_errors: 0,
        };

        let mut peer_required_by: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        peer_required_by
            .entry("react-dom".into())
            .or_default()
            .insert("@scope/framework".into());

        let audit = audit_package_deps_with_peers(&graph, &manifest, &peer_required_by);

        assert!(
            audit.peer_required.contains_key("react-dom"),
            "react-dom should be in peer_required, not unused"
        );
        assert!(
            !audit.unused.contains("react-dom"),
            "react-dom must not be flagged unused — workspace peer needs it"
        );
        assert!(
            audit.unused.contains("stale-pkg"),
            "stale-pkg has no peer requirement, should be unused"
        );
        let _ = ImportEdge {
            specifier: "_".into(),
            resolved_path: None,
            package_name: None,
            type_only: false,
            dynamic: false,
        };
    }
}
