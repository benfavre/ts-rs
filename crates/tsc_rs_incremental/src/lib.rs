//! Incremental build metadata and change detection helpers.
//!
//! The actual incremental compilation pipeline lives in `tsc_rs_project`.
//! This crate exposes the persisted build-info type plus deterministic
//! change-detection helpers so there is a single incremental story in the
//! workspace instead of a disconnected prototype.

use std::collections::HashMap;

pub use tsc_rs_project::{
    hash_compiler_options, resolve_build_info_path, simple_hash, TsBuildInfo,
};

/// Why a file needs to be rebuilt on an incremental pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeReason {
    NewFile,
    ContentChanged,
    DependencyChanged,
    OptionsChanged,
}

/// A single incremental rebuild decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub file_path: String,
    pub reason: ChangeReason,
}

/// Hash the current contents of a set of files using the same hash function as
/// `tsc_rs_project`.
pub fn hash_files(file_names: &[String]) -> HashMap<String, String> {
    let mut hashes = HashMap::new();
    for file_name in file_names {
        if let Ok(source) = std::fs::read_to_string(file_name) {
            hashes.insert(file_name.clone(), simple_hash(&source));
        }
    }
    hashes
}

/// Build a new `.tsbuildinfo` payload from the current file hashes.
pub fn build_info(
    version: &str,
    file_hashes: HashMap<String, String>,
    options_hash: String,
    dependencies: HashMap<String, Vec<String>>,
) -> TsBuildInfo {
    TsBuildInfo {
        version: version.to_string(),
        file_hashes,
        options_hash,
        dependencies,
    }
}

/// Detect which files need rebuilding for the current project state.
///
/// This mirrors the semantics used by `tsc_rs_project::TsProject::compile_incremental`:
/// content changes rebuild the file, dependency changes invalidate importers,
/// and compiler option changes force a full rebuild.
pub fn detect_changes(
    current_hashes: &HashMap<String, String>,
    previous: Option<&TsBuildInfo>,
    options_hash: &str,
) -> Vec<FileChange> {
    let Some(previous) = previous else {
        let mut changes: Vec<_> = current_hashes
            .keys()
            .cloned()
            .map(|file_path| FileChange {
                file_path,
                reason: ChangeReason::NewFile,
            })
            .collect();
        changes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        return changes;
    };

    if previous.options_hash != options_hash {
        let mut changes: Vec<_> = current_hashes
            .keys()
            .cloned()
            .map(|file_path| FileChange {
                file_path,
                reason: ChangeReason::OptionsChanged,
            })
            .collect();
        changes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        return changes;
    }

    let mut changes = Vec::new();
    for file_path in current_hashes.keys() {
        if let Some(reason) = detect_change_for_file(file_path, current_hashes, previous) {
            changes.push(FileChange {
                file_path: file_path.clone(),
                reason,
            });
        }
    }
    changes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    changes
}

fn detect_change_for_file(
    file_path: &str,
    current_hashes: &HashMap<String, String>,
    previous: &TsBuildInfo,
) -> Option<ChangeReason> {
    let current = current_hashes.get(file_path)?;
    let previous_hash = previous.file_hashes.get(file_path);

    match previous_hash {
        None => return Some(ChangeReason::NewFile),
        Some(previous_hash) if current != previous_hash => {
            return Some(ChangeReason::ContentChanged)
        }
        Some(_) => {}
    }

    if let Some(deps) = previous.dependencies.get(file_path) {
        for dep in deps {
            let dep_current = current_hashes.get(dep);
            let dep_previous = previous.file_hashes.get(dep);
            if dep_current != dep_previous {
                return Some(ChangeReason::DependencyChanged);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_changes_marks_new_files_without_previous_state() {
        let current = HashMap::from([
            ("a.ts".to_string(), "1".to_string()),
            ("b.ts".to_string(), "2".to_string()),
        ]);

        let changes = detect_changes(&current, None, "opts");
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| c.reason == ChangeReason::NewFile));
    }

    #[test]
    fn detect_changes_marks_all_files_when_options_change() {
        let current = HashMap::from([("a.ts".to_string(), "1".to_string())]);
        let previous = TsBuildInfo {
            version: "0.1.1".to_string(),
            file_hashes: current.clone(),
            options_hash: "old".to_string(),
            dependencies: HashMap::new(),
        };

        let changes = detect_changes(&current, Some(&previous), "new");
        assert_eq!(
            changes,
            vec![FileChange {
                file_path: "a.ts".to_string(),
                reason: ChangeReason::OptionsChanged,
            }]
        );
    }

    #[test]
    fn detect_changes_marks_content_changes() {
        let current = HashMap::from([("a.ts".to_string(), "new".to_string())]);
        let previous = TsBuildInfo {
            version: "0.1.1".to_string(),
            file_hashes: HashMap::from([("a.ts".to_string(), "old".to_string())]),
            options_hash: "opts".to_string(),
            dependencies: HashMap::new(),
        };

        let changes = detect_changes(&current, Some(&previous), "opts");
        assert_eq!(
            changes,
            vec![FileChange {
                file_path: "a.ts".to_string(),
                reason: ChangeReason::ContentChanged,
            }]
        );
    }

    #[test]
    fn detect_changes_marks_dependency_changes() {
        let current = HashMap::from([
            ("a.ts".to_string(), "same".to_string()),
            ("dep.ts".to_string(), "new".to_string()),
        ]);
        let previous = TsBuildInfo {
            version: "0.1.1".to_string(),
            file_hashes: HashMap::from([
                ("a.ts".to_string(), "same".to_string()),
                ("dep.ts".to_string(), "old".to_string()),
            ]),
            options_hash: "opts".to_string(),
            dependencies: HashMap::from([("a.ts".to_string(), vec!["dep.ts".to_string()])]),
        };

        let changes = detect_changes(&current, Some(&previous), "opts");
        assert_eq!(
            changes,
            vec![
                FileChange {
                    file_path: "a.ts".to_string(),
                    reason: ChangeReason::DependencyChanged,
                },
                FileChange {
                    file_path: "dep.ts".to_string(),
                    reason: ChangeReason::ContentChanged,
                },
            ]
        );
    }

    #[test]
    fn hash_files_uses_project_hash_function() {
        let dir = std::env::temp_dir().join(format!("tsc_rs_incremental_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("a.ts");
        std::fs::write(&file, "export const a = 1;").expect("write should succeed");

        let hashes = hash_files(&[file.to_string_lossy().into_owned()]);
        let value = hashes
            .values()
            .next()
            .expect("expected a file hash")
            .clone();
        assert_eq!(value, simple_hash("export const a = 1;"));

        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_dir(dir);
    }
}
