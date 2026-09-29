//! Opaque import scanner.
//!
//! Scans service-registry definition files for dynamic `await import("...")`
//! calls inside factory functions. These packages are loaded at runtime but
//! invisible to the normal import graph walker (which stops at the factory
//! boundary because the definitions use `opaqueImport` with `webpackIgnore`).

use std::collections::BTreeMap;
use std::path::Path;

use crate::import_graph;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A package discovered via dynamic import in service-registry definitions.
#[derive(Debug, Clone)]
pub struct OpaquePackage {
    /// The bare package name (e.g. `@acme/services-shipping`).
    pub package_name: String,
    /// Full specifier (e.g. `@acme/services-shipping/shipping/service`).
    pub specifier: String,
    /// Which definition file it was found in.
    pub definition_file: String,
}

/// Result of scanning definition directories.
#[derive(Debug)]
pub struct OpaqueScanResult {
    /// All packages found, keyed by bare package name.
    pub packages: BTreeMap<String, Vec<OpaquePackage>>,
    /// Number of definition files scanned.
    pub files_scanned: usize,
    /// Total dynamic import calls found.
    pub total_imports: usize,
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

/// Scan a directory of service-registry definition files for dynamic imports.
/// Returns all packages referenced via `await import("...")` patterns.
pub fn scan_definitions_dir(dir: &Path) -> OpaqueScanResult {
    let mut result = OpaqueScanResult {
        packages: BTreeMap::new(),
        files_scanned: 0,
        total_imports: 0,
    };

    let files = import_graph::discover_ts_files(dir);
    result.files_scanned = files.len();

    for file_path in &files {
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let source_file = tsc_rs_parser::parse(file_path, &source);

        // Extract all dynamic imports from the entire file (deep walk)
        let mut specifiers = Vec::new();
        for stmt in &source_file.statements {
            import_graph::extract_from_stmt_public(stmt, &mut specifiers);
        }

        for (specifier, _type_only, _is_dynamic) in &specifiers {
            // Skip relative and project-alias imports — only care about packages
            if specifier.starts_with("./")
                || specifier.starts_with("../")
                || specifier.starts_with("@/")
                || specifier.starts_with("~/")
            {
                continue;
            }

            let pkg_name = extract_package_name(specifier).to_string();
            if pkg_name.is_empty() {
                continue;
            }

            result.total_imports += 1;

            let entry = result.packages.entry(pkg_name.clone()).or_default();
            // Only add if we don't already have this exact specifier from this file
            if !entry
                .iter()
                .any(|e| e.specifier == *specifier && e.definition_file == *file_path)
            {
                entry.push(OpaquePackage {
                    package_name: pkg_name,
                    specifier: specifier.clone(),
                    definition_file: file_path.clone(),
                });
            }
        }
    }

    result
}

/// Extract the bare package name from a specifier.
fn extract_package_name(specifier: &str) -> &str {
    if specifier.starts_with('@') {
        if let Some(first_slash) = specifier.find('/') {
            if let Some(second_slash) = specifier[first_slash + 1..]
                .find('/')
                .map(|i| i + first_slash + 1)
            {
                &specifier[..second_slash]
            } else {
                specifier
            }
        } else {
            specifier
        }
    } else if let Some(slash) = specifier.find('/') {
        &specifier[..slash]
    } else {
        specifier
    }
}

impl OpaqueScanResult {
    /// Format a human-readable report.
    pub fn format_report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Opaque Import Scan: {} packages from {} definition files ({} total imports)\n\n",
            self.packages.len(),
            self.files_scanned,
            self.total_imports,
        ));

        for (pkg, entries) in &self.packages {
            out.push_str(&format!("  {pkg}"));
            if entries.len() == 1 {
                let short = shorten_path(&entries[0].definition_file);
                out.push_str(&format!("  ({})\n", short));
            } else {
                out.push_str(&format!("  ({} sub-imports)\n", entries.len()));
                for e in entries.iter().take(3) {
                    let short = shorten_path(&e.definition_file);
                    out.push_str(&format!("    {} → \"{}\"\n", short, e.specifier));
                }
                if entries.len() > 3 {
                    out.push_str(&format!("    ... +{} more\n", entries.len() - 3));
                }
            }
        }

        out
    }
}

use crate::utils::shorten_path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name() {
        assert_eq!(
            extract_package_name("@acme/services-shipping/shipping/service"),
            "@acme/services-shipping"
        );
        assert_eq!(extract_package_name("lodash/fp"), "lodash");
        assert_eq!(extract_package_name("express"), "express");
    }
}
