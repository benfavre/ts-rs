//! Per-entry transitive closure over an `ImportGraph`.
//!
//! Given a built graph and a set of entry files, walks the resolved-edge
//! tree from each entry to enumerate every file reachable from it. The
//! union over all entries is the deterministic file set bext can pre-
//! compile at boot, populating its module registry before the V8 startup
//! snapshot is baked.
//!
//! The graph is expected to have been built with `skip_node_modules =
//! false` for prewarm use — otherwise the closure stops at the package
//! boundary and the framework / vendor closure isn't represented.

use std::collections::{BTreeSet, HashSet, VecDeque};

use crate::import_graph::ImportGraph;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Closure of one entry: the deterministic transitive file set, including
/// the entry itself.
#[derive(Debug, Clone)]
pub struct EntryClosure {
    /// The entry file (absolute path).
    pub entry: String,
    /// Optional URL-style label (e.g., `/examples/mdx`) when the closure
    /// was produced from a route discovery.
    pub label: Option<String>,
    /// All resolved files reachable from the entry (entry included), sorted.
    pub files: Vec<String>,
}

#[derive(Debug)]
pub struct ClosureReport {
    pub entries: Vec<EntryClosure>,
    /// Sorted union of every entry's closure.
    pub union: Vec<String>,
    /// Files in the graph that no entry reaches (defensive — usually empty).
    pub orphan_in_graph: Vec<String>,
    pub stats: ClosureStats,
}

#[derive(Debug)]
pub struct ClosureStats {
    pub entry_count: usize,
    pub union_files: usize,
    pub orphan_count: usize,
    pub min_per_entry: usize,
    pub max_per_entry: usize,
    pub avg_per_entry: f64,
}

// ---------------------------------------------------------------------------
// Closure walker
// ---------------------------------------------------------------------------

/// Compute per-entry closures plus the union over all of them.
///
/// `entries` is a list of `(entry_file, optional_label)`. Use the file path
/// alone when there's no convenient URL — the closure walk doesn't depend
/// on the label.
pub fn compute_closures(
    graph: &ImportGraph,
    entries: &[(String, Option<String>)],
) -> ClosureReport {
    let mut per_entry: Vec<EntryClosure> = Vec::with_capacity(entries.len());
    let mut union: BTreeSet<String> = BTreeSet::new();

    for (entry, label) in entries {
        let files = walk_reachable(graph, entry);
        for f in &files {
            union.insert(f.clone());
        }
        per_entry.push(EntryClosure {
            entry: entry.clone(),
            label: label.clone(),
            files,
        });
    }

    // Files seen by the graph builder but not reached from any entry.
    // Possible when the caller passes entries that don't cover every
    // visited file — surfaces unused entry-set inputs without blocking
    // the report.
    let mut orphan_in_graph: Vec<String> = graph
        .visited_files
        .iter()
        .filter(|f| !union.contains(*f))
        .cloned()
        .collect();
    orphan_in_graph.sort();

    let union_vec: Vec<String> = union.into_iter().collect();

    let stats = {
        let counts: Vec<usize> = per_entry.iter().map(|e| e.files.len()).collect();
        let min_per_entry = *counts.iter().min().unwrap_or(&0);
        let max_per_entry = *counts.iter().max().unwrap_or(&0);
        let avg_per_entry = if counts.is_empty() {
            0.0
        } else {
            counts.iter().sum::<usize>() as f64 / counts.len() as f64
        };
        ClosureStats {
            entry_count: per_entry.len(),
            union_files: union_vec.len(),
            orphan_count: orphan_in_graph.len(),
            min_per_entry,
            max_per_entry,
            avg_per_entry,
        }
    };

    ClosureReport {
        entries: per_entry,
        union: union_vec,
        orphan_in_graph,
        stats,
    }
}

/// BFS the graph from a single entry, collecting every file reachable via
/// `resolved_path` edges. Self-included.
fn walk_reachable(graph: &ImportGraph, entry: &str) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut seen: HashSet<String> = HashSet::new();

    let start = entry.to_string();
    queue.push_back(start.clone());
    seen.insert(start.clone());

    while let Some(file) = queue.pop_front() {
        out.insert(file.clone());
        let Some(edges) = graph.edges.get(&file) else {
            continue;
        };
        for edge in edges {
            if let Some(rp) = &edge.resolved_path {
                if seen.insert(rp.clone()) {
                    queue.push_back(rp.clone());
                }
            }
        }
    }

    out.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

impl ClosureReport {
    pub fn format_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Closure: {entries} entries, union {union} files ({min}..{max} per entry, avg {avg:.1})\n",
            entries = self.stats.entry_count,
            union = self.stats.union_files,
            min = self.stats.min_per_entry,
            max = self.stats.max_per_entry,
            avg = self.stats.avg_per_entry,
        ));
        if self.stats.orphan_count > 0 {
            out.push_str(&format!(
                "  warning: {} files visited by the graph builder are unreachable from any entry — entry set may be incomplete\n",
                self.stats.orphan_count,
            ));
        }
        out.push('\n');

        out.push_str("=== PER ENTRY ===\n");
        for ec in &self.entries {
            let label = ec.label.as_deref().unwrap_or("");
            let entry_short = crate::utils::shorten_path(&ec.entry);
            out.push_str(&format!(
                "  {label:<32}  {n:>4} files  {entry_short}\n",
                n = ec.files.len(),
            ));
        }
        out.push('\n');

        out
    }

    /// Structured JSON for downstream tooling (bext-turbopack consumes this
    /// to drive boot-time pre-compile, populating the V8 snapshot's module
    /// registry deterministically).
    pub fn format_json(&self) -> String {
        let mut buf = String::new();
        buf.push_str("{\n");

        // stats
        buf.push_str("  \"stats\": {\n");
        buf.push_str(&format!(
            "    \"entryCount\": {},\n",
            self.stats.entry_count
        ));
        buf.push_str(&format!(
            "    \"unionFiles\": {},\n",
            self.stats.union_files
        ));
        buf.push_str(&format!(
            "    \"orphanCount\": {},\n",
            self.stats.orphan_count
        ));
        buf.push_str(&format!(
            "    \"minPerEntry\": {},\n",
            self.stats.min_per_entry
        ));
        buf.push_str(&format!(
            "    \"maxPerEntry\": {},\n",
            self.stats.max_per_entry
        ));
        buf.push_str(&format!(
            "    \"avgPerEntry\": {:.2}\n",
            self.stats.avg_per_entry
        ));
        buf.push_str("  },\n");

        // entries
        buf.push_str("  \"entries\": [\n");
        for (i, ec) in self.entries.iter().enumerate() {
            buf.push_str("    {\n");
            buf.push_str(&format!("      \"entry\": {},\n", json_string(&ec.entry)));
            buf.push_str(&format!(
                "      \"label\": {},\n",
                match &ec.label {
                    Some(s) => json_string(s),
                    None => "null".to_string(),
                }
            ));
            buf.push_str("      \"files\": [\n");
            for (j, f) in ec.files.iter().enumerate() {
                let comma = if j + 1 < ec.files.len() { "," } else { "" };
                buf.push_str(&format!("        {}{}\n", json_string(f), comma));
            }
            buf.push_str("      ]\n");
            let comma = if i + 1 < self.entries.len() { "," } else { "" };
            buf.push_str(&format!("    }}{comma}\n"));
        }
        buf.push_str("  ],\n");

        // union
        buf.push_str("  \"union\": [\n");
        for (i, f) in self.union.iter().enumerate() {
            let comma = if i + 1 < self.union.len() { "," } else { "" };
            buf.push_str(&format!("    {}{}\n", json_string(f), comma));
        }
        buf.push_str("  ],\n");

        // orphan_in_graph
        buf.push_str("  \"orphanInGraph\": [\n");
        for (i, f) in self.orphan_in_graph.iter().enumerate() {
            let comma = if i + 1 < self.orphan_in_graph.len() {
                ","
            } else {
                ""
            };
            buf.push_str(&format!("    {}{}\n", json_string(f), comma));
        }
        buf.push_str("  ]\n");

        buf.push_str("}\n");
        buf
    }
}

/// Quote a string for JSON. Handles the small subset of characters that
/// matter for absolute file paths — backslashes (Windows would have them
/// but we run on Linux), control chars, and double-quotes.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Bext / Next.js route entry discovery
// ---------------------------------------------------------------------------

/// Walk an `app/` tree (Next.js conventions) and emit `(file, label)` for
/// each entry kind that maps to a request handler. Mirrors the shape of
/// `bext-turbopack::prism::discover_routes` but runs over the file system
/// and doesn't try to canonicalise URL params — `[id]` segments are kept
/// verbatim in the label.
pub fn discover_app_route_entries(app_dir: &std::path::Path) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    walk_app(app_dir, "", &mut out);
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn walk_app(dir: &std::path::Path, label_prefix: &str, out: &mut Vec<(String, Option<String>)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Files in this dir.
    let mut subdirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if matches!(name, "node_modules" | ".bext" | ".next" | ".turbo") {
                continue;
            }
            subdirs.push(path);
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let stem_kind = match name {
            "page.tsx" | "page.ts" | "page.jsx" | "page.js" => Some("page"),
            "layout.tsx" | "layout.ts" => Some("layout"),
            "route.ts" | "route.tsx" => Some("route"),
            "middleware.ts" => Some("middleware"),
            "not-found.tsx" => Some("not-found"),
            "loading.tsx" => Some("loading"),
            "error.tsx" => Some("error"),
            "default.tsx" => Some("default"),
            "template.tsx" => Some("template"),
            _ => None,
        };
        if let Some(kind) = stem_kind {
            let abs = match path.canonicalize() {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => path.to_string_lossy().to_string(),
            };
            let label = if label_prefix.is_empty() {
                format!("/  ({kind})")
            } else {
                format!("{label_prefix}  ({kind})")
            };
            out.push((abs, Some(label)));
        }
    }

    // Recurse into subdirs.
    for sub in subdirs {
        let name = sub
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        // Route groups `(group)` and parallel slots `@slot` don't contribute
        // a URL segment.
        let next_label = if name.starts_with('(') && name.ends_with(')') {
            label_prefix.to_string()
        } else if name.starts_with('@') {
            label_prefix.to_string()
        } else {
            format!("{label_prefix}/{name}")
        };

        walk_app(&sub, &next_label, out);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import_graph::{ImportEdge, ImportGraph};
    use std::collections::{BTreeMap, HashMap, HashSet};

    fn empty_graph() -> ImportGraph {
        ImportGraph {
            edges: HashMap::new(),
            packages: BTreeMap::new(),
            visited_files: HashSet::new(),
            unresolved: Vec::new(),
            read_errors: 0,
        }
    }

    fn add_edge(graph: &mut ImportGraph, from: &str, to: &str) {
        graph
            .edges
            .entry(from.to_string())
            .or_default()
            .push(ImportEdge {
                specifier: to.to_string(),
                resolved_path: Some(to.to_string()),
                package_name: None,
                type_only: false,
                dynamic: false,
            });
        graph.visited_files.insert(from.to_string());
        graph.visited_files.insert(to.to_string());
    }

    #[test]
    fn closure_includes_entry() {
        let g = empty_graph();
        let r = compute_closures(&g, &[("/a.ts".into(), None)]);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].files, vec!["/a.ts".to_string()]);
        assert_eq!(r.union, vec!["/a.ts".to_string()]);
    }

    #[test]
    fn closure_walks_resolved_edges() {
        let mut g = empty_graph();
        add_edge(&mut g, "/a.ts", "/b.ts");
        add_edge(&mut g, "/b.ts", "/c.ts");
        let r = compute_closures(&g, &[("/a.ts".into(), Some("/foo".into()))]);
        assert_eq!(r.entries[0].files, vec!["/a.ts", "/b.ts", "/c.ts"]);
    }

    #[test]
    fn closure_handles_cycles() {
        let mut g = empty_graph();
        add_edge(&mut g, "/a.ts", "/b.ts");
        add_edge(&mut g, "/b.ts", "/a.ts");
        let r = compute_closures(&g, &[("/a.ts".into(), None)]);
        assert_eq!(r.entries[0].files, vec!["/a.ts", "/b.ts"]);
    }

    #[test]
    fn closure_union_dedupes() {
        let mut g = empty_graph();
        add_edge(&mut g, "/a.ts", "/shared.ts");
        add_edge(&mut g, "/b.ts", "/shared.ts");
        let r = compute_closures(&g, &[("/a.ts".into(), None), ("/b.ts".into(), None)]);
        assert_eq!(r.union, vec!["/a.ts", "/b.ts", "/shared.ts"]);
    }

    #[test]
    fn closure_reports_orphan_when_entry_set_is_partial() {
        let mut g = empty_graph();
        add_edge(&mut g, "/a.ts", "/b.ts");
        // /c.ts visited but not reachable from /a.ts
        g.visited_files.insert("/c.ts".to_string());
        let r = compute_closures(&g, &[("/a.ts".into(), None)]);
        assert_eq!(r.orphan_in_graph, vec!["/c.ts".to_string()]);
    }
}
