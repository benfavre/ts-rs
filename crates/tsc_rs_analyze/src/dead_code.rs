//! Dead code detection.
//!
//! Finds source files that are never imported by any other file in the
//! import graph. These are orphans — dead weight that wastes build time,
//! confuses developers, and inflates the codebase.
//!
//! The analysis distinguishes:
//! - **Entry points**: files that are expected to have no importers (pages,
//!   routes, scripts, tests, configs). These are NOT dead code.
//! - **Orphans**: non-entry files with zero importers. Likely dead code.

use std::collections::HashSet;
use std::path::Path;

use crate::import_graph::ImportGraph;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A file identified as potentially dead (never imported).
#[derive(Debug, Clone)]
pub struct OrphanFile {
    pub path: String,
    pub size_bytes: u64,
    pub category: OrphanCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrphanCategory {
    /// Component/utility/lib — likely dead code
    DeadCode,
    /// Script file — expected to have no importers
    Script,
    /// Test file — expected to have no importers
    Test,
    /// Page/route entry point — expected to have no importers
    PageEntry,
    /// Config file — expected to have no importers
    Config,
    /// Bext server action (`"use server"` under `src/actions/`) — discovered
    /// by bext's directive scanner, not via static imports.
    Action,
    /// Bext client/signal island (`"use client"` or `"use signals"` under
    /// `src/components/` or `src/islands/`) — discovered by bext's island
    /// scanner, not via static imports.
    Island,
}

/// Result of dead code analysis.
#[derive(Debug)]
pub struct DeadCodeReport {
    /// Files with zero importers, categorized.
    pub orphans: Vec<OrphanFile>,
    /// Total source files analyzed.
    pub total_files: usize,
    /// Files that ARE imported by at least one other file.
    pub imported_files: usize,
    /// Summary stats.
    pub stats: DeadCodeStats,
}

#[derive(Debug)]
pub struct DeadCodeStats {
    pub dead_code_count: usize,
    pub dead_code_bytes: u64,
    pub entry_point_count: usize,
    pub test_count: usize,
    pub script_count: usize,
    pub action_count: usize,
    pub island_count: usize,
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Find orphan files: files that exist in the source tree but are never
/// imported by any other file.
///
/// `all_source_files` should be the complete list of .ts/.tsx files discovered
/// in the source tree (from `discover_ts_files`).
pub fn find_dead_code(graph: &ImportGraph, all_source_files: &[String]) -> DeadCodeReport {
    // Build set of all files that are imported by at least one other file
    let mut imported: HashSet<String> = HashSet::new();
    for edges in graph.edges.values() {
        for edge in edges {
            if let Some(ref rp) = edge.resolved_path {
                imported.insert(rp.clone());
            }
        }
    }

    let total_files = all_source_files.len();
    let imported_files = imported.len();

    // Find orphans: files in all_source_files that are NOT in imported set
    // and NOT in the visited set as entry points
    let mut orphans: Vec<OrphanFile> = Vec::new();

    for file in all_source_files {
        // Check if this file is imported by anything
        if imported.contains(file) {
            continue;
        }
        // Also check canonical form
        let canonical = std::fs::canonicalize(file)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        if !canonical.is_empty() && imported.contains(&canonical) {
            continue;
        }

        let size_bytes = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        let category = classify_file(file);

        orphans.push(OrphanFile {
            path: file.clone(),
            size_bytes,
            category,
        });
    }

    // Sort: dead code first (by size desc), then entry points
    orphans.sort_by(|a, b| {
        let cat_order = |c: &OrphanCategory| match c {
            OrphanCategory::DeadCode => 0,
            OrphanCategory::Script => 1,
            OrphanCategory::Test => 2,
            OrphanCategory::PageEntry => 3,
            OrphanCategory::Action => 4,
            OrphanCategory::Island => 5,
            OrphanCategory::Config => 6,
        };
        cat_order(&a.category)
            .cmp(&cat_order(&b.category))
            .then(b.size_bytes.cmp(&a.size_bytes))
    });

    let dead_code_count = orphans
        .iter()
        .filter(|o| o.category == OrphanCategory::DeadCode)
        .count();
    let dead_code_bytes: u64 = orphans
        .iter()
        .filter(|o| o.category == OrphanCategory::DeadCode)
        .map(|o| o.size_bytes)
        .sum();
    let entry_point_count = orphans
        .iter()
        .filter(|o| o.category == OrphanCategory::PageEntry)
        .count();
    let test_count = orphans
        .iter()
        .filter(|o| o.category == OrphanCategory::Test)
        .count();
    let script_count = orphans
        .iter()
        .filter(|o| o.category == OrphanCategory::Script)
        .count();
    let action_count = orphans
        .iter()
        .filter(|o| o.category == OrphanCategory::Action)
        .count();
    let island_count = orphans
        .iter()
        .filter(|o| o.category == OrphanCategory::Island)
        .count();

    DeadCodeReport {
        orphans,
        total_files,
        imported_files,
        stats: DeadCodeStats {
            dead_code_count,
            dead_code_bytes,
            entry_point_count,
            test_count,
            script_count,
            action_count,
            island_count,
        },
    }
}

// ---------------------------------------------------------------------------
// File classification
// ---------------------------------------------------------------------------

/// Classify a file as an entry point, test, script, or potentially dead code.
fn classify_file(path: &str) -> OrphanCategory {
    let p = Path::new(path);
    let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let path_lower = path.to_lowercase();

    // Test files
    if file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.contains("__tests__")
        || path_lower.contains("/__tests__/")
        || path_lower.contains("/tests/")
        || path_lower.contains("/test/")
    {
        return OrphanCategory::Test;
    }

    let is_ts_or_js = file_name.ends_with(".ts")
        || file_name.ends_with(".tsx")
        || file_name.ends_with(".js")
        || file_name.ends_with(".jsx");

    // Bext "use server" actions live under src/actions/. They're discovered
    // by bext's action scanner via the leading directive, not via static
    // imports — no other file in the graph references them, so without
    // directive-aware classification they look like dead code.
    if is_ts_or_js
        && path_contains_segment(&path_lower, "src/actions/")
        && file_starts_with_directive(path, "use server")
    {
        return OrphanCategory::Action;
    }

    // Bext client/signal islands live under src/components/ or src/islands/.
    // bext's island scanner finds them by leading directive — same deal as
    // actions: no static importer → false-positive dead code without this.
    if is_ts_or_js
        && (path_contains_segment(&path_lower, "src/components/")
            || path_contains_segment(&path_lower, "src/islands/"))
        && (file_starts_with_directive(path, "use client")
            || file_starts_with_directive(path, "use signals"))
    {
        return OrphanCategory::Island;
    }

    // Script/CLI files
    if path_lower.contains("/scripts/")
        || path_lower.contains("/cli")
        || file_name == "cli.ts"
        || file_name.starts_with("seed-")
        || file_name.starts_with("migrate-")
    {
        return OrphanCategory::Script;
    }

    // Next.js page/route entry points (these are loaded by the framework, not by imports)
    if file_name == "page.tsx"
        || file_name == "page.ts"
        || file_name == "page.client.tsx"
        || file_name == "page.server.tsx"
        || file_name == "layout.tsx"
        || file_name == "layout.ts"
        || file_name == "loading.tsx"
        || file_name == "error.tsx"
        || file_name == "not-found.tsx"
        || file_name == "route.ts"
        || file_name == "route.tsx"
        || file_name == "middleware.ts"
        || file_name == "template.tsx"
        || file_name == "default.tsx"
        || file_name == "global-error.tsx"
        || file_name == "sitemap.ts"
        || file_name == "sitemap.tsx"
        || file_name == "opengraph-image.tsx"
        || file_name == "robots.ts"
        || file_name.starts_with("manifest")
    {
        return OrphanCategory::PageEntry;
    }

    // Config files
    if file_name.contains("config")
        || file_name == "env.mjs"
        || file_name == "env.ts"
        || file_name.ends_with(".config.ts")
        || file_name.ends_with(".config.mjs")
    {
        return OrphanCategory::Config;
    }

    // Everything else is potentially dead code
    OrphanCategory::DeadCode
}

/// True when `path_lower` contains `segment` either preceded by `/` or as
/// a leading prefix. Lets the same check work against both relative paths
/// (`src/actions/foo.ts`) and absolute paths (`/repo/site/src/actions/foo.ts`).
fn path_contains_segment(path_lower: &str, segment: &str) -> bool {
    if path_lower.starts_with(segment) {
        return true;
    }
    let with_slash = format!("/{segment}");
    path_lower.contains(&with_slash)
}

/// Read the first ~50 bytes of a file and return true when the first
/// non-whitespace token is `"<directive>"` or `'<directive>'`. Mirrors
/// `bext-turbopack/src/prism.rs::file_starts_with_directive`.
fn file_starts_with_directive(path: &str, directive: &str) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 64];
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let head = match std::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let trimmed = head.trim_start();
    let dq = format!("\"{directive}\"");
    let sq = format!("'{directive}'");
    trimmed.starts_with(&dq) || trimmed.starts_with(&sq)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

impl DeadCodeReport {
    /// Format human-readable report, showing only dead code (not entry points/tests/scripts).
    pub fn format_report(&self, show_all: bool) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "Dead Code Report: {} orphan files out of {} total ({} imported)\n",
            self.orphans.len(),
            self.total_files,
            self.imported_files,
        ));
        out.push_str(&format!(
            "  Dead code:    {} files ({} KB)\n",
            self.stats.dead_code_count,
            self.stats.dead_code_bytes / 1024,
        ));
        out.push_str(&format!(
            "  Entry points: {} (pages/routes/layouts — expected)\n",
            self.stats.entry_point_count,
        ));
        out.push_str(&format!(
            "  Tests:        {} (expected)\n",
            self.stats.test_count,
        ));
        out.push_str(&format!(
            "  Scripts:      {} (expected)\n",
            self.stats.script_count,
        ));
        if self.stats.action_count > 0 {
            out.push_str(&format!(
                "  Actions:      {} (bext \"use server\" — expected)\n",
                self.stats.action_count,
            ));
        }
        if self.stats.island_count > 0 {
            out.push_str(&format!(
                "  Islands:      {} (bext \"use client\"/\"use signals\" — expected)\n",
                self.stats.island_count,
            ));
        }
        out.push('\n');

        // Dead code section
        let dead: Vec<&OrphanFile> = self
            .orphans
            .iter()
            .filter(|o| o.category == OrphanCategory::DeadCode)
            .collect();

        if !dead.is_empty() {
            out.push_str(&format!(
                "=== DEAD CODE ({} files, {} KB) ===\n",
                dead.len(),
                self.stats.dead_code_bytes / 1024,
            ));
            for orphan in &dead {
                let kb = orphan.size_bytes / 1024;
                let short = shorten_path(&orphan.path);
                if kb > 0 {
                    out.push_str(&format!("  {kb:>4} KB  {short}\n"));
                } else {
                    out.push_str(&format!(
                        "  {size:>4} B   {short}\n",
                        size = orphan.size_bytes
                    ));
                }
            }
            out.push('\n');
        }

        if show_all {
            // Also show entry points, tests, scripts for completeness
            for (category, label) in &[
                (OrphanCategory::Script, "SCRIPTS"),
                (OrphanCategory::Test, "TESTS"),
                (OrphanCategory::PageEntry, "ENTRY POINTS"),
                (OrphanCategory::Action, "BEXT ACTIONS"),
                (OrphanCategory::Island, "BEXT ISLANDS"),
                (OrphanCategory::Config, "CONFIGS"),
            ] {
                let items: Vec<&OrphanFile> = self
                    .orphans
                    .iter()
                    .filter(|o| &o.category == category)
                    .collect();
                if !items.is_empty() {
                    out.push_str(&format!("=== {} ({}) ===\n", label, items.len()));
                    for orphan in items.iter().take(20) {
                        out.push_str(&format!("  {}\n", shorten_path(&orphan.path)));
                    }
                    if items.len() > 20 {
                        out.push_str(&format!("  ... +{} more\n", items.len() - 20));
                    }
                    out.push('\n');
                }
            }
        }

        out
    }
}

use crate::utils::shorten_path;
