//! Shared utilities for the analyze crate.

/// Shorten a file path for display. Tries to find a recognizable prefix
/// (/apps/, /packages/, /src/) and trims everything before it.
pub fn shorten_path(path: &str) -> String {
    if let Some(pos) = path.find("/apps/") {
        return path[pos + 1..].to_string();
    }
    if let Some(pos) = path.find("/packages/") {
        return path[pos + 1..].to_string();
    }
    if let Some(pos) = path.find("/src/") {
        return format!("...{}", &path[pos..]);
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 4 {
        return path.to_string();
    }
    format!(".../{}", parts[parts.len() - 4..].join("/"))
}
