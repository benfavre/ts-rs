//! Circular dependency detection.
//!
//! Finds import cycles in the file graph. Cycles hurt tree-shaking, cause
//! initialization order issues, and make Turbopack trace more modules than
//! necessary.
//!
//! Uses iterative DFS with a color map (white/grey/black) for cycle detection.

use std::collections::{HashMap, HashSet};

use crate::import_graph::ImportGraph;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single import cycle.
#[derive(Debug, Clone)]
pub struct Cycle {
    /// Ordered list of files forming the cycle.
    /// The last file imports the first (closing the loop).
    pub files: Vec<String>,
}

/// Result of cycle detection.
#[derive(Debug)]
pub struct CycleReport {
    /// All cycles found, deduplicated.
    pub cycles: Vec<Cycle>,
    /// Number of files involved in at least one cycle.
    pub files_in_cycles: usize,
    /// Total files analyzed.
    pub total_files: usize,
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Find all import cycles in the graph.
/// Uses iterative DFS to avoid stack overflow on large graphs.
/// Find all import cycles in the graph.
/// If `static_only` is true, dynamic `import()` edges are excluded — these
/// don't cause build-time or initialization-order issues.
pub fn find_cycles(graph: &ImportGraph) -> CycleReport {
    find_cycles_impl(graph, false)
}

pub fn find_cycles_static_only(graph: &ImportGraph) -> CycleReport {
    find_cycles_impl(graph, true)
}

fn find_cycles_impl(graph: &ImportGraph, static_only: bool) -> CycleReport {
    // Build adjacency list: file → list of resolved files it imports
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (file, edges) in &graph.edges {
        let targets: Vec<&str> = edges
            .iter()
            .filter(|e| !static_only || !e.dynamic)
            .filter_map(|e| e.resolved_path.as_deref())
            .collect();
        adj.insert(file.as_str(), targets);
    }

    let total_files = adj.len();
    let mut all_cycles: Vec<Vec<String>> = Vec::new();
    let mut seen_cycle_keys: HashSet<String> = HashSet::new();

    // Color map: 0=white(unvisited), 1=grey(in-stack), 2=black(done)
    let mut color: HashMap<&str, u8> = HashMap::new();
    for &file in adj.keys() {
        color.insert(file, 0);
    }

    // Ancestor stack for cycle reconstruction
    let mut parent: HashMap<&str, &str> = HashMap::new();

    for &start in adj.keys() {
        if color[start] != 0 {
            continue;
        }

        // Iterative DFS
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        color.insert(start, 1);

        while let Some(&mut (node, ref mut idx)) = stack.last_mut() {
            let neighbors = adj.get(node).map(|v| v.as_slice()).unwrap_or(&[]);

            if *idx < neighbors.len() {
                let next = neighbors[*idx];
                *idx += 1;

                match color.get(next).copied().unwrap_or(0) {
                    0 => {
                        // Unvisited — push to stack
                        color.insert(next, 1);
                        parent.insert(next, node);
                        stack.push((next, 0));
                    }
                    1 => {
                        // Back edge — cycle found! Reconstruct from stack.
                        let mut cycle = vec![next.to_string()];
                        // Walk back through the stack to find `next`
                        for &(ancestor, _) in stack.iter().rev() {
                            cycle.push(ancestor.to_string());
                            if ancestor == next {
                                break;
                            }
                        }
                        cycle.reverse();

                        // Deduplicate: normalize cycle by rotating to smallest element
                        let key = normalize_cycle_key(&cycle);
                        if !seen_cycle_keys.contains(&key) && cycle.len() > 1 {
                            seen_cycle_keys.insert(key);
                            all_cycles.push(cycle);
                        }
                    }
                    _ => {
                        // Already fully processed — skip
                    }
                }
            } else {
                // Done with this node
                color.insert(node, 2);
                stack.pop();
            }
        }
    }

    // Sort cycles: shortest first, then alphabetically
    all_cycles.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));

    // Count unique files involved in cycles
    let mut files_in_cycles: HashSet<String> = HashSet::new();
    for cycle in &all_cycles {
        for file in cycle {
            files_in_cycles.insert(file.clone());
        }
    }

    CycleReport {
        cycles: all_cycles
            .into_iter()
            .map(|files| Cycle { files })
            .collect(),
        files_in_cycles: files_in_cycles.len(),
        total_files,
    }
}

/// Normalize a cycle for dedup: rotate so the lexicographically smallest
/// element is first, then create a joined key.
fn normalize_cycle_key(cycle: &[String]) -> String {
    if cycle.is_empty() {
        return String::new();
    }
    let min_idx = cycle
        .iter()
        .enumerate()
        .min_by_key(|(_, s)| s.as_str())
        .map(|(i, _)| i)
        .unwrap_or(0);

    let rotated: Vec<&str> = cycle[min_idx..]
        .iter()
        .chain(cycle[..min_idx].iter())
        .map(|s| s.as_str())
        .collect();
    rotated.join(" → ")
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

impl CycleReport {
    pub fn format_report(&self, max_show: usize) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "Cycle Report: {} cycles found across {} files (of {} total)\n\n",
            self.cycles.len(),
            self.files_in_cycles,
            self.total_files,
        ));

        if self.cycles.is_empty() {
            out.push_str("No circular dependencies found.\n");
            return out;
        }

        // Group by cycle length
        let mut by_length: HashMap<usize, usize> = HashMap::new();
        for cycle in &self.cycles {
            *by_length.entry(cycle.files.len()).or_default() += 1;
        }
        let mut lengths: Vec<(usize, usize)> = by_length.into_iter().collect();
        lengths.sort();

        out.push_str("Cycle lengths:\n");
        for (len, count) in &lengths {
            out.push_str(&format!("  {len}-file cycles: {count}\n"));
        }
        out.push('\n');

        let show_count = max_show.min(self.cycles.len());
        out.push_str(&format!(
            "Showing {show_count} of {} cycles:\n\n",
            self.cycles.len()
        ));

        for (i, cycle) in self.cycles.iter().take(show_count).enumerate() {
            out.push_str(&format!(
                "Cycle #{} ({} files):\n",
                i + 1,
                cycle.files.len()
            ));
            for (j, file) in cycle.files.iter().enumerate() {
                let arrow = if j + 1 < cycle.files.len() {
                    " →"
                } else {
                    " ↩" // loops back to first
                };
                out.push_str(&format!("  {}{}\n", shorten_path(file), arrow));
            }
            out.push('\n');
        }

        if self.cycles.len() > show_count {
            out.push_str(&format!(
                "... +{} more cycles\n",
                self.cycles.len() - show_count
            ));
        }

        out
    }
}

use crate::utils::shorten_path;
