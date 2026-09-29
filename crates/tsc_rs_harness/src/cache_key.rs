//! Shared cache key helper for baseline harness tools.
//!
//! Computes a hash of the compiler source tree so that cached harness results
//! stay valid across no-op or irrelevant rebuilds (Cargo.lock bumps, edits to
//! the harness itself, etc.), and invalidate deterministically when any
//! compiler crate's source changes.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Crates whose source files influence compiler output. Harness/LSP-only
/// crates are excluded so that edits to them don't bust the cache.
const COMPILER_CRATES: &[&str] = &[
    "tsc_rs_ast",
    "tsc_rs_scanner",
    "tsc_rs_parser",
    "tsc_rs_emitter",
    "tsc_rs_symbols",
    "tsc_rs_types",
    "tsc_rs_resolver",
    "tsc_rs_project",
];

/// Hash of every `.rs` file under each compiler crate's `src/` directory.
///
/// Returns `None` if the workspace layout is unexpected. Callers treat `None`
/// as "unable to compute key" and skip caching.
pub fn compiler_source_hash(workspace_root: &Path) -> Option<u64> {
    let mut files: Vec<PathBuf> = Vec::new();
    for crate_name in COMPILER_CRATES {
        let src = workspace_root.join("crates").join(crate_name).join("src");
        if !src.is_dir() {
            continue;
        }
        collect_rs_files(&src, &mut files);
    }
    files.sort();

    let mut hasher = DefaultHasher::new();
    for path in &files {
        // Path relative to workspace root for stability across machines.
        let rel = path.strip_prefix(workspace_root).unwrap_or(path);
        rel.to_string_lossy().hash(&mut hasher);
        let data = std::fs::read(path).ok()?;
        data.hash(&mut hasher);
    }
    Some(hasher.finish())
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
