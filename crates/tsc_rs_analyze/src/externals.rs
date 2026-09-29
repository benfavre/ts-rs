//! Externals audit — determine which `serverExternalPackages` are actually
//! reachable from the application's import graph, distinguishing runtime
//! from type-only usage.

use std::collections::{BTreeMap, BTreeSet};

use crate::import_graph::{ImportGraph, PackageUsage};
use crate::opaque_imports::OpaqueScanResult;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of auditing externals against an import graph.
#[derive(Debug)]
pub struct ExternalsAudit {
    /// Externals that ARE reachable from the import graph at runtime.
    pub used: BTreeMap<String, PackageUsage>,
    /// Externals that are reachable but ONLY via type-only imports.
    /// These don't affect runtime bundles and can be removed from serverExternalPackages.
    pub type_only: BTreeMap<String, PackageUsage>,
    /// Externals not in the import graph but found via opaqueImport/service-registry scan.
    /// These MUST be kept — they're loaded dynamically at runtime.
    pub opaque_only: BTreeSet<String>,
    /// Externals that are NOT reachable by any means — safe to remove.
    pub unused: BTreeSet<String>,
    /// All bare packages found in the graph (including non-externals).
    pub all_packages: BTreeSet<String>,
    /// Summary statistics.
    pub stats: AuditStats,
}

#[derive(Debug)]
pub struct AuditStats {
    pub total_externals: usize,
    pub used_runtime: usize,
    pub used_type_only: usize,
    pub used_opaque: usize,
    pub unused_count: usize,
    pub removable_count: usize,
    pub total_files_scanned: usize,
    pub total_packages_found: usize,
}

// ---------------------------------------------------------------------------
// Audit logic
// ---------------------------------------------------------------------------

/// Audit a list of externals against a built import graph and optional opaque scan.
pub fn audit_externals(
    graph: &ImportGraph,
    externals: &[String],
    opaque: Option<&OpaqueScanResult>,
) -> ExternalsAudit {
    let externals_set: BTreeSet<String> = externals.iter().cloned().collect();
    let opaque_packages: BTreeSet<String> = opaque
        .map(|o| o.packages.keys().cloned().collect())
        .unwrap_or_default();

    let mut used = BTreeMap::new();
    let mut type_only_map = BTreeMap::new();
    let mut opaque_only = BTreeSet::new();
    let mut unused = BTreeSet::new();

    for ext in &externals_set {
        if let Some(usage) = graph.packages.get(ext) {
            if usage.type_only && !opaque_packages.contains(ext) {
                // Type-only in import graph AND not in opaqueImport → removable
                type_only_map.insert(ext.clone(), usage.clone());
            } else {
                // Runtime import, or type-only but also opaqueImport-loaded
                used.insert(ext.clone(), usage.clone());
            }
        } else if opaque_packages.contains(ext) {
            // Not in import graph but loaded via opaqueImport → must keep
            opaque_only.insert(ext.clone());
        } else {
            // Not found anywhere → safe to remove
            unused.insert(ext.clone());
        }
    }

    let all_packages: BTreeSet<String> = graph.packages.keys().cloned().collect();
    // Removable = unused + type-only (not rescued by opaqueImport)
    let removable = unused.len() + type_only_map.len();

    let stats = AuditStats {
        total_externals: externals_set.len(),
        used_runtime: used.len(),
        used_type_only: type_only_map.len(),
        used_opaque: opaque_only.len(),
        unused_count: unused.len(),
        removable_count: removable,
        total_files_scanned: graph.visited_files.len(),
        total_packages_found: all_packages.len(),
    };

    ExternalsAudit {
        used,
        type_only: type_only_map,
        opaque_only,
        unused,
        all_packages,
        stats,
    }
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

impl ExternalsAudit {
    /// Render a human-readable report.
    pub fn format_report(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "Externals Audit: {runtime} runtime, {opaque} opaque, {type_only} type-only, {unused} unreachable — {removable} removable out of {total}\n",
            runtime = self.stats.used_runtime,
            opaque = self.stats.used_opaque,
            type_only = self.stats.used_type_only,
            unused = self.stats.unused_count,
            removable = self.stats.removable_count,
            total = self.stats.total_externals,
        ));
        out.push_str(&format!(
            "Scanned {} files, found {} unique packages\n\n",
            self.stats.total_files_scanned, self.stats.total_packages_found,
        ));

        // Removable: unused
        if !self.unused.is_empty() {
            out.push_str(&format!(
                "=== UNREACHABLE ({}) — safe to remove ===\n",
                self.unused.len()
            ));
            for pkg in &self.unused {
                out.push_str(&format!("  {pkg}\n"));
            }
            out.push('\n');
        }

        // Removable: type-only
        if !self.type_only.is_empty() {
            out.push_str(&format!(
                "=== TYPE-ONLY ({}) — safe to remove (no runtime effect) ===\n",
                self.type_only.len()
            ));
            for (pkg, usage) in &self.type_only {
                out.push_str(&format!("  {pkg}  ({} imports)\n", usage.import_count));
                let display_chain = shorten_chain(&usage.chain.files, 3);
                out.push_str(&format!(
                    "    via: {display_chain} → import type \"{}\"\n",
                    usage.chain.specifier
                ));
            }
            out.push('\n');
        }

        // Opaque-only (dynamic service-registry loads)
        if !self.opaque_only.is_empty() {
            out.push_str(&format!(
                "=== OPAQUE-ONLY ({}) — keep (loaded via service-registry) ===\n",
                self.opaque_only.len()
            ));
            for pkg in &self.opaque_only {
                out.push_str(&format!("  {pkg}\n"));
            }
            out.push('\n');
        }

        // Needed: runtime imports
        if !self.used.is_empty() {
            out.push_str(&format!("=== RUNTIME ({}) — keep ===\n", self.used.len()));
            for (pkg, usage) in &self.used {
                out.push_str(&format!("  {pkg}  ({} imports)\n", usage.import_count));
                let display_chain = shorten_chain(&usage.chain.files, 3);
                out.push_str(&format!(
                    "    via: {display_chain} → import \"{}\"\n",
                    usage.chain.specifier
                ));
            }
            out.push('\n');
        }

        out
    }

    /// Render as JSON.
    pub fn format_json(&self) -> String {
        let mut out = String::from("{\n");

        // Stats
        out.push_str("  \"stats\": {\n");
        out.push_str(&format!(
            "    \"totalExternals\": {},\n",
            self.stats.total_externals
        ));
        out.push_str(&format!(
            "    \"usedRuntime\": {},\n",
            self.stats.used_runtime
        ));
        out.push_str(&format!(
            "    \"usedTypeOnly\": {},\n",
            self.stats.used_type_only
        ));
        out.push_str(&format!("    \"unused\": {},\n", self.stats.unused_count));
        out.push_str(&format!(
            "    \"usedOpaque\": {},\n",
            self.stats.used_opaque
        ));
        out.push_str(&format!(
            "    \"removable\": {},\n",
            self.stats.removable_count
        ));
        out.push_str(&format!(
            "    \"filesScanned\": {},\n",
            self.stats.total_files_scanned
        ));
        out.push_str(&format!(
            "    \"packagesFound\": {}\n",
            self.stats.total_packages_found
        ));
        out.push_str("  },\n");

        // Unreachable
        out.push_str("  \"unreachable\": [\n");
        let unused_items: Vec<&String> = self.unused.iter().collect();
        for (i, pkg) in unused_items.iter().enumerate() {
            let comma = if i + 1 < unused_items.len() { "," } else { "" };
            out.push_str(&format!("    \"{pkg}\"{comma}\n"));
        }
        out.push_str("  ],\n");

        // Type-only
        out.push_str("  \"typeOnly\": [\n");
        let type_only_items: Vec<&String> = self.type_only.keys().collect();
        for (i, pkg) in type_only_items.iter().enumerate() {
            let comma = if i + 1 < type_only_items.len() {
                ","
            } else {
                ""
            };
            out.push_str(&format!("    \"{pkg}\"{comma}\n"));
        }
        out.push_str("  ],\n");

        // Opaque
        out.push_str("  \"opaque\": [\n");
        let opaque_items: Vec<&String> = self.opaque_only.iter().collect();
        for (i, pkg) in opaque_items.iter().enumerate() {
            let comma = if i + 1 < opaque_items.len() { "," } else { "" };
            out.push_str(&format!("    \"{pkg}\"{comma}\n"));
        }
        out.push_str("  ],\n");

        // Runtime used
        out.push_str("  \"runtime\": {\n");
        let used_items: Vec<(&String, &PackageUsage)> = self.used.iter().collect();
        for (i, (pkg, usage)) in used_items.iter().enumerate() {
            let comma = if i + 1 < used_items.len() { "," } else { "" };
            let escaped_spec = usage.chain.specifier.replace('"', "\\\"");
            out.push_str(&format!("    \"{pkg}\": {{\n"));
            out.push_str(&format!("      \"specifier\": \"{escaped_spec}\",\n"));
            out.push_str(&format!("      \"importCount\": {},\n", usage.import_count));
            out.push_str("      \"chain\": [");
            let chain_strs: Vec<String> = usage
                .chain
                .files
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect();
            out.push_str(&chain_strs.join(", "));
            out.push_str("]\n");
            out.push_str(&format!("    }}{comma}\n"));
        }
        out.push_str("  }\n");

        out.push_str("}\n");
        out
    }

    /// Return sorted list of all removable package names (unreachable + type-only).
    pub fn removable_packages(&self) -> Vec<String> {
        let mut pkgs: Vec<String> = self.unused.iter().cloned().collect();
        pkgs.extend(self.type_only.keys().cloned());
        pkgs.sort();
        pkgs
    }
}

/// Shorten a file chain for display, showing first file, ..., last N files.
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

/// Shorten a path for display by keeping only the last 3 components.
use crate::utils::shorten_path;

// ---------------------------------------------------------------------------
// Externals list parsing
// ---------------------------------------------------------------------------

/// Parse an externals list from a JSON array file or a next.config.mjs-style
/// JavaScript array.
pub fn parse_externals_file(content: &str) -> Vec<String> {
    let trimmed = content.trim();

    // Try JSON array first
    if trimmed.starts_with('[') {
        return parse_json_array(trimmed);
    }

    // Try extracting from a JS/TS file (look for serverExternalPackages array)
    if let Some(arr_content) = extract_js_array(trimmed, "serverExternalPackages") {
        return parse_json_array(&format!("[{arr_content}]"));
    }

    // Fall back: one package per line
    trimmed
        .lines()
        .map(|l| l.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .collect()
}

/// Parse a JSON array of strings.
fn parse_json_array(json: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut in_string = false;
    let mut current = String::new();
    let mut escaped = false;

    for ch in json.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            if in_string {
                results.push(current.clone());
                current.clear();
            }
            in_string = !in_string;
            continue;
        }
        if in_string {
            current.push(ch);
        }
    }

    results
}

/// Extract the content of a named array assignment from a JS file.
fn extract_js_array(content: &str, name: &str) -> Option<String> {
    let search_patterns = [
        format!("{name}:"),
        format!("{name} :"),
        format!("{name}="),
        format!("{name} ="),
    ];

    for pattern in &search_patterns {
        if let Some(pos) = content.find(pattern.as_str()) {
            let after = &content[pos + pattern.len()..];
            if let Some(bracket_pos) = after.find('[') {
                let from_bracket = &after[bracket_pos..];
                let mut depth = 0;
                for (i, ch) in from_bracket.char_indices() {
                    match ch {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(from_bracket[1..i].to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_array() {
        let input = r#"["lodash", "@scope/pkg", "express"]"#;
        let result = parse_json_array(input);
        assert_eq!(result, vec!["lodash", "@scope/pkg", "express"]);
    }

    #[test]
    fn test_parse_externals_file_json() {
        let input = r#"[
            "lodash",
            "express"
        ]"#;
        let result = parse_externals_file(input);
        assert_eq!(result, vec!["lodash", "express"]);
    }

    #[test]
    fn test_extract_js_array() {
        let input = r#"
            const config = {
                serverExternalPackages: [
                    "pino",
                    "sharp",
                ],
                other: true,
            };
        "#;
        let result = parse_externals_file(input);
        assert_eq!(result, vec!["pino", "sharp"]);
    }

    #[test]
    fn test_shorten_path() {
        assert_eq!(
            shorten_path("/home/user/project/src/deep/file.ts"),
            ".../src/deep/file.ts"
        );
        assert_eq!(shorten_path("src/file.ts"), "src/file.ts");
    }
}
