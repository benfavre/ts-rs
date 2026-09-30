//! Load and compare fourslash baselines for LSP operations.
//!
//! Baselines are stored as two-part files: a human-readable display section
//! followed by a JSON array with structured results per marker.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

/// A single quickInfo baseline entry (parsed from JSON).
#[derive(Debug, Clone)]
pub struct QuickInfoEntry {
    pub marker_name: String,
    pub file_name: String,
    pub position: u32,
    /// Concatenated displayParts text (the display string).
    pub display_text: String,
    /// Documentation text (concatenated).
    pub documentation: String,
    /// The symbol kind (e.g. "class", "var", "function").
    pub kind: String,
}

/// Load a quickInfo baseline file and extract entries.
pub fn load_quick_info_baseline(path: &Path) -> Result<Vec<QuickInfoEntry>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read baseline: {e}"))?;

    // Find the JSON section: first line starting with '['
    let json_start = content
        .lines()
        .enumerate()
        .find(|(_, line)| line.starts_with('['))
        .map(|(i, _)| i);

    let Some(start_line) = json_start else {
        return Err("no JSON section found in baseline".into());
    };

    let json_text: String = content
        .lines()
        .skip(start_line)
        .collect::<Vec<_>>()
        .join("\n");
    let arr: Vec<Value> =
        serde_json::from_str(&json_text).map_err(|e| format!("JSON parse error: {e}"))?;

    let mut entries = Vec::new();
    for val in &arr {
        let marker = &val["marker"];
        let marker_name = marker["name"].as_str().unwrap_or("").to_string();
        let file_name = marker["fileName"].as_str().unwrap_or("").to_string();
        let position = marker["position"].as_u64().unwrap_or(0) as u32;

        let item = &val["item"];
        let kind = item["kind"].as_str().unwrap_or("").to_string();

        // Concatenate displayParts
        let display_text = concat_display_parts(&item["displayParts"]);
        let documentation = concat_display_parts(&item["documentation"]);

        entries.push(QuickInfoEntry {
            marker_name,
            file_name,
            position,
            display_text,
            documentation,
            kind,
        });
    }

    Ok(entries)
}

/// Concatenate the `text` fields from a `displayParts` or `documentation` array.
fn concat_display_parts(parts: &Value) -> String {
    match parts.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        None => String::new(),
    }
}

/// Compare our hover result against a baseline entry.
/// Returns `true` if they match semantically.
pub fn compare_quick_info(actual: Option<&str>, expected: &QuickInfoEntry) -> bool {
    let Some(actual) = actual else {
        return expected.display_text.is_empty();
    };
    // Normalize both sides
    let actual_norm = normalize_display(actual);
    let expected_norm = normalize_display(&expected.display_text);
    actual_norm == expected_norm
}

/// Normalize a display string for comparison.
/// Removes markdown formatting, extra whitespace, etc.
fn normalize_display(s: &str) -> String {
    let s = s.trim();
    // Strip markdown code fences
    let s = s.strip_prefix("```typescript\n").unwrap_or(s);
    let s = s.strip_prefix("```ts\n").unwrap_or(s);
    let s = s.strip_suffix("\n```").unwrap_or(s);
    // Strip leading keyword prefix like "(method) " or "(property) " — these are
    // formatting differences, but we actually want to preserve them since TSC
    // uses the same convention.  Only strip markdown/JSDoc separators.
    let s = if let Some(pos) = s.find("\n\n---\n\n") {
        &s[..pos]
    } else {
        s
    };
    // Collapse whitespace
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Resolve the baseline file path for a quickInfo test.
pub fn quick_info_baseline_path(baselines_dir: &Path, test_name: &str) -> std::path::PathBuf {
    baselines_dir.join(format!("{}.baseline", test_name))
}

/// Check if a baseline file exists.
pub fn baseline_exists(baselines_dir: &Path, test_name: &str) -> bool {
    quick_info_baseline_path(baselines_dir, test_name).exists()
}

// ---------------------------------------------------------------------------
// SignatureHelp baselines
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SignatureHelpBaselineEntry {
    pub marker_name: String,
    pub file_name: String,
    pub position: u32,
    pub signature_labels: Vec<String>,
    pub selected_label: Option<String>,
    pub selected_parameter: Option<usize>,
    pub selected_parameter_label: Option<String>,
    pub parameter_labels: Vec<String>,
    pub is_variadic: Option<bool>,
}

pub fn signature_help_baseline_path(baselines_dir: &Path, test_name: &str) -> std::path::PathBuf {
    baselines_dir.join(format!("{}.baseline", test_name))
}

pub fn load_signature_help_baseline(
    path: &Path,
) -> Result<Vec<SignatureHelpBaselineEntry>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read baseline: {e}"))?;

    let json_start = content
        .lines()
        .enumerate()
        .find(|(_, line)| line.starts_with('['))
        .map(|(i, _)| i);

    let Some(start_line) = json_start else {
        return Err("no JSON section found in baseline".into());
    };

    let json_text: String = content
        .lines()
        .skip(start_line)
        .collect::<Vec<_>>()
        .join("\n");
    let arr: Vec<Value> =
        serde_json::from_str(&json_text).map_err(|e| format!("JSON parse error: {e}"))?;

    let mut entries = Vec::new();
    for val in &arr {
        let marker = &val["marker"];
        let marker_name = marker["name"].as_str().unwrap_or("").to_string();
        let file_name = marker["fileName"].as_str().unwrap_or("").to_string();
        let position = marker["position"].as_u64().unwrap_or(0) as u32;

        let item = &val["item"];
        let signatures = item["items"].as_array().cloned().unwrap_or_default();
        let signature_labels: Vec<String> =
            signatures.iter().map(signature_help_item_label).collect();
        let selected_index = item["selectedItemIndex"].as_u64().unwrap_or(0) as usize;
        let selected = signatures
            .get(selected_index)
            .or_else(|| signatures.first());
        let parameter_labels = selected
            .and_then(|sig| sig["parameters"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|param| concat_display_parts(&param["displayParts"]))
            .collect::<Vec<_>>();
        let selected_parameter = item["argumentIndex"].as_u64().map(|n| n as usize);
        let selected_parameter_label =
            selected_parameter.and_then(|idx| parameter_labels.get(idx).cloned());
        let is_variadic = selected.and_then(|sig| sig["isVariadic"].as_bool());

        entries.push(SignatureHelpBaselineEntry {
            marker_name,
            file_name,
            position,
            selected_label: selected.map(signature_help_item_label),
            signature_labels,
            selected_parameter,
            selected_parameter_label,
            parameter_labels,
            is_variadic,
        });
    }

    Ok(entries)
}

fn signature_help_item_label(item: &Value) -> String {
    let prefix = concat_display_parts(&item["prefixDisplayParts"]);
    let suffix = concat_display_parts(&item["suffixDisplayParts"]);
    let separator = concat_display_parts(&item["separatorDisplayParts"]);
    let params = item["parameters"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|param| concat_display_parts(&param["displayParts"]))
        .collect::<Vec<_>>();
    format!("{}{}{}", prefix, params.join(&separator), suffix)
}

// ---------------------------------------------------------------------------
// Completions baselines
// ---------------------------------------------------------------------------

/// A single completions baseline entry.
#[derive(Debug, Clone)]
pub struct CompletionsBaselineEntry {
    pub marker_name: String,
    pub file_name: String,
    pub position: u32,
    /// Expected completion entry names.
    pub entry_names: Vec<String>,
}

/// Resolve the baseline file path for a completions test.
pub fn completions_baseline_path(baselines_dir: &Path, test_name: &str) -> std::path::PathBuf {
    baselines_dir.join(format!("{}.baseline", test_name))
}

/// Load a completions baseline file and extract entries.
pub fn load_completions_baseline(path: &Path) -> Result<Vec<CompletionsBaselineEntry>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read baseline: {e}"))?;

    let json_start = content
        .lines()
        .enumerate()
        .find(|(_, line)| line.starts_with('['))
        .map(|(i, _)| i);

    let Some(start_line) = json_start else {
        return Err("no JSON section found in baseline".into());
    };

    let json_text: String = content
        .lines()
        .skip(start_line)
        .collect::<Vec<_>>()
        .join("\n");
    let arr: Vec<Value> =
        serde_json::from_str(&json_text).map_err(|e| format!("JSON parse error: {e}"))?;

    let mut entries = Vec::new();
    for val in &arr {
        let marker = &val["marker"];
        let marker_name = marker["name"].as_str().unwrap_or("").to_string();
        let file_name = marker["fileName"].as_str().unwrap_or("").to_string();
        let position = marker["position"].as_u64().unwrap_or(0) as u32;

        let item = &val["item"];
        let entry_names: Vec<String> = item["entries"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        entries.push(CompletionsBaselineEntry {
            marker_name,
            file_name,
            position,
            entry_names,
        });
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// findAllReferences baselines
// ---------------------------------------------------------------------------

/// A single findAllReferences baseline entry.
#[derive(Debug, Clone)]
pub struct FindAllRefsEntry {
    /// Number of reference locations ([|...|] ranges) in the baseline.
    pub expected_ref_count: usize,
    /// The symbol name from the Details section.
    pub name: String,
    /// The symbol kind from the Details section.
    pub kind: String,
}

/// Resolve the baseline file path for a findAllReferences test.
pub fn find_all_refs_baseline_path(baselines_dir: &Path, test_name: &str) -> std::path::PathBuf {
    baselines_dir.join(format!("{}.baseline.jsonc", test_name))
}

/// Load a findAllReferences baseline and extract the expected reference count.
/// A top-level `// === <operation> ===` line (not a file-path header).
fn is_operation_header(line: &str) -> bool {
    let Some(inner) = line
        .strip_prefix("// === ")
        .and_then(|rest| rest.strip_suffix(" ==="))
    else {
        return false;
    };
    !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_alphabetic())
}

pub fn load_find_all_refs_baseline(path: &Path) -> Result<Vec<FindAllRefsEntry>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read baseline: {e}"))?;

    let mut entries = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == "// === findAllReferences ===" {
            i += 1;
            // Count [|...|] ranges in the source annotation section
            let mut ref_count = 0;
            let mut name = String::new();
            let mut kind = String::new();

            while i < lines.len() {
                let t = lines[i].trim();
                if t == "// === findAllReferences ===" {
                    break; // next section
                }
                // Another operation's section (`// === findRenameLocations ===`,
                // `// === documentHighlights ===`, ...) ends this entry; file
                // headers (`// === /a.ts ===`) and indented sub-sections don't.
                if is_operation_header(lines[i]) {
                    break;
                }
                // Count [| occurrences in source lines
                if t.starts_with("// ") {
                    let line = &t[3..];
                    ref_count += line.matches("[|").count();
                }
                // Parse Details JSON
                if t == "// === Details ===" {
                    i += 1;
                    let mut json_lines = Vec::new();
                    while i < lines.len() {
                        let jt = lines[i].trim();
                        if jt.starts_with("// === ") {
                            break;
                        }
                        if jt.is_empty() {
                            let peek = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
                            if peek.starts_with("// === ") || peek.is_empty() {
                                i += 1;
                                break;
                            }
                        }
                        json_lines.push(jt);
                        i += 1;
                    }
                    let json_text = json_lines.join("\n");
                    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&json_text) {
                        if let Some(first) = arr.first() {
                            name = first["name"].as_str().unwrap_or("").to_string();
                            kind = first["kind"].as_str().unwrap_or("").to_string();
                        }
                    }
                    continue;
                }
                i += 1;
            }

            entries.push(FindAllRefsEntry {
                expected_ref_count: ref_count,
                name,
                kind,
            });
        } else {
            i += 1;
        }
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// goToDefinition baselines
// ---------------------------------------------------------------------------

/// A single goToDefinition baseline entry (one section per marker).
#[derive(Debug, Clone)]
pub struct GoToDefinitionEntry {
    /// The target file that the definition points to.
    pub target_file: Option<String>,
    /// The character offset of the definition target (`[|` range start) within
    /// the target file's annotated source (with annotations stripped).
    pub target_position: Option<u32>,
    /// The symbol name from the JSON Details section.
    pub name: String,
    /// The symbol kind from the JSON Details section.
    pub kind: String,
    /// Whether the baseline expects no definition (empty section).
    pub expects_no_definition: bool,
}

/// Resolve the baseline file path for a goToDefinition test.
pub fn goto_definition_baseline_path(baselines_dir: &Path, test_name: &str) -> std::path::PathBuf {
    baselines_dir.join(format!("{}.baseline.jsonc", test_name))
}

/// Load a goToDefinition baseline file and extract entries.
///
/// The format has repeated sections, each starting with:
///   `// === goToDefinition ===` or `// === getDefinitionAtPosition ===`
/// followed by file annotations and an optional `// === Details ===` JSON block.
///
/// `virtual_sources` maps file paths (as used in the baseline) to the actual
/// virtual source text. When a baseline section has `--- (line: N) skipped ---`,
/// the skipped lines are restored from the virtual source so that character
/// offsets are computed correctly.
pub fn load_goto_definition_baseline(
    path: &Path,
    virtual_sources: &HashMap<String, String>,
) -> Result<Vec<GoToDefinitionEntry>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read baseline: {e}"))?;

    let lines: Vec<&str> = content.lines().collect();
    let mut entries = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        // Look for section header
        if trimmed == "// === goToDefinition ===" || trimmed == "// === getDefinitionAtPosition ==="
        {
            i += 1;
            let (entry, next_i) = parse_goto_definition_section(&lines, i, virtual_sources);
            entries.push(entry);
            i = next_i;
        } else {
            i += 1;
        }
    }

    Ok(entries)
}

/// Collected file block within a goToDefinition section.
struct FileBlock {
    path: String,
    annotated: String,
    has_angle_brackets: bool,   // contains <|...|>
    has_square_brackets: bool,  // contains [|...|]
    has_goto_def_comment: bool, // contains /*GOTO DEF*/
}

/// Parse a single goToDefinition section starting after the `=== goToDefinition ===` header.
/// Returns the entry and the line index after the section.
fn parse_goto_definition_section(
    lines: &[&str],
    start: usize,
    virtual_sources: &HashMap<String, String>,
) -> (GoToDefinitionEntry, usize) {
    let mut i = start;
    let mut name = String::new();
    let mut kind = String::new();
    let mut file_blocks: Vec<FileBlock> = Vec::new();

    // Collect annotated source lines per file until we hit Details or next section
    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Next section or end
        if trimmed == "// === goToDefinition ===" || trimmed == "// === getDefinitionAtPosition ==="
        {
            break;
        }

        // File header: // === /path/to/file.ts ===
        if trimmed.starts_with("// === /") && trimmed.ends_with(" ===") {
            let file_path = &trimmed[7..trimmed.len() - 4]; // strip "// === " and " ==="

            // Collect annotated source lines for this file
            i += 1;
            let mut annotated_lines = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim();
                if t.starts_with("// === ") {
                    break;
                }
                // Source lines are prefixed with "// "
                if let Some(rest) = lines[i].strip_prefix("// ") {
                    // Handle `--- (line: N) skipped ---` placeholder lines.
                    // These indicate that lines 1 through N-1 of the source were
                    // omitted in the baseline. Restore the actual source lines from
                    // the virtual file to keep offset calculations correct.
                    if rest.contains("skipped") {
                        if let Some(start) = rest.find("(line: ") {
                            let num_start = start + 7;
                            if let Some(end) = rest[num_start..].find(')') {
                                if let Ok(line_num) =
                                    rest[num_start..num_start + end].parse::<usize>()
                                {
                                    // Skipped lines 1..line_num-1. Look up the virtual
                                    // source for this file and insert the actual lines.
                                    let file_path_str = file_path;
                                    let source = virtual_sources.get(file_path_str).or_else(|| {
                                        // Try matching by filename only
                                        let fname = file_path_str
                                            .rsplit('/')
                                            .next()
                                            .unwrap_or(file_path_str);
                                        virtual_sources
                                            .iter()
                                            .find(|(k, _)| {
                                                k.rsplit('/').next().unwrap_or(k) == fname
                                            })
                                            .map(|(_, v)| v)
                                    });
                                    if let Some(src) = source {
                                        let src_lines: Vec<&str> = src.lines().collect();
                                        // Insert the skipped lines. `(line: N)` means
                                        // lines 1..N (1-based) are omitted; we restore
                                        // them so that character offsets align.
                                        for li in 0..line_num.saturating_sub(1) {
                                            if li < src_lines.len() {
                                                annotated_lines.push(src_lines[li].to_string());
                                            } else {
                                                annotated_lines.push(String::new());
                                            }
                                        }
                                    } else {
                                        // Fallback: insert empty lines
                                        for _ in 0..line_num.saturating_sub(1) {
                                            annotated_lines.push(String::new());
                                        }
                                    }
                                    i += 1;
                                    continue;
                                }
                            }
                        }
                    }
                    annotated_lines.push(rest.to_string());
                } else if lines[i].trim().is_empty() {
                    // Blank line between sections — peek ahead
                    let mut peek = i + 1;
                    while peek < lines.len() && lines[peek].trim().is_empty() {
                        peek += 1;
                    }
                    if peek >= lines.len() {
                        break;
                    }
                    let next_trimmed = lines[peek].trim();
                    if next_trimmed.starts_with("// === ")
                        || next_trimmed.starts_with('[')
                        || next_trimmed.starts_with('{')
                    {
                        break;
                    }
                    annotated_lines.push(String::new());
                    i += 1;
                    continue;
                } else {
                    break;
                }
                i += 1;
            }

            let joined = annotated_lines.join("\n");
            file_blocks.push(FileBlock {
                path: file_path.to_string(),
                has_angle_brackets: joined.contains("<|"),
                has_square_brackets: joined.contains("[|"),
                has_goto_def_comment: joined.contains("GOTO DEF"),
                annotated: joined,
            });

            continue;
        }

        // Details JSON section
        if trimmed == "// === Details ===" {
            i += 1;
            // Collect JSON lines
            let mut json_lines = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim();
                if t.starts_with("// === ") {
                    break;
                }
                if t.is_empty() {
                    // Could be end of JSON block — peek ahead
                    let mut peek = i + 1;
                    while peek < lines.len() && lines[peek].trim().is_empty() {
                        peek += 1;
                    }
                    if peek >= lines.len() {
                        i = peek;
                        break;
                    }
                    let next = lines[peek].trim();
                    if next.starts_with("// === ") {
                        i = peek;
                        break;
                    }
                    // Blank in middle of JSON
                    json_lines.push("");
                    i += 1;
                    continue;
                }
                json_lines.push(t);
                i += 1;
            }

            let json_text = json_lines.join("\n");
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&json_text) {
                if let Some(first) = arr.first() {
                    name = first["name"].as_str().unwrap_or("").to_string();
                    kind = first["kind"].as_str().unwrap_or("").to_string();
                }
            }
            continue;
        }

        i += 1;
    }

    // Determine the target file and position:
    // 1. If a file has <|...|>, it's the definition target.
    // 2. Otherwise, the first file with [|...|] that does NOT contain /*GOTO DEF*/ is the target.
    // 3. If only one file exists and it has [|...|], it's both cursor and target.
    let mut target_file: Option<String> = None;
    let mut target_position: Option<u32> = None;

    // Strategy 1: file with <|
    for block in &file_blocks {
        if block.has_angle_brackets {
            if let Some(pos) = find_target_position_in_annotated(&block.annotated) {
                target_file = Some(block.path.clone());
                target_position = Some(pos);
            }
            break;
        }
    }

    // Strategy 2: first file with [| but no GOTO DEF
    if target_file.is_none() {
        for block in &file_blocks {
            if block.has_square_brackets && !block.has_goto_def_comment {
                if let Some(pos) = find_target_position_in_annotated(&block.annotated) {
                    target_file = Some(block.path.clone());
                    target_position = Some(pos);
                }
                break;
            }
        }
    }

    // Strategy 3: single file with [| (it's both cursor and target)
    if target_file.is_none() && file_blocks.len() == 1 && file_blocks[0].has_square_brackets {
        if let Some(pos) = find_target_position_in_annotated(&file_blocks[0].annotated) {
            target_file = Some(file_blocks[0].path.clone());
            target_position = Some(pos);
        }
    }

    let found_any_content = !file_blocks.is_empty();
    let expects_no_definition = !found_any_content || (target_file.is_none() && name.is_empty());

    (
        GoToDefinitionEntry {
            target_file,
            target_position,
            name,
            kind,
            expects_no_definition,
        },
        i,
    )
}

/// Find the character position of the `[|` target range within annotated source text.
///
/// The annotated source may contain `<|...|>` (full declaration range) and
/// `[|...|]` (definition name range). We need to strip all annotations to
/// compute the true character offset of the `[|` target.
fn find_target_position_in_annotated(text: &str) -> Option<u32> {
    // We need to find the [| marker and compute its position after stripping
    // all annotation markers: <|, |>, [|, |], /*GOTO DEF*/, /*GOTO DEF POS*/
    let mut pos: u32 = 0;
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut found_target: Option<u32> = None;

    while i < len {
        // Check for annotation markers
        if i + 1 < len {
            // [| — definition name range start (this is what we want)
            if bytes[i] == b'[' && bytes[i + 1] == b'|' {
                if found_target.is_none() {
                    found_target = Some(pos);
                }
                i += 2;
                continue;
            }
            // |] — definition name range end
            if bytes[i] == b'|' && bytes[i + 1] == b']' {
                i += 2;
                continue;
            }
            // <| — full declaration range start
            if bytes[i] == b'<' && bytes[i + 1] == b'|' {
                i += 2;
                continue;
            }
            // |> — full declaration range end
            if bytes[i] == b'|' && bytes[i + 1] == b'>' {
                i += 2;
                continue;
            }
        }

        // Check for /*GOTO DEF*/ or /*GOTO DEF POS*/ comments
        if i + 3 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            if let Some(close) = text[i + 2..].find("*/") {
                let comment = &text[i + 2..i + 2 + close];
                if comment.starts_with("GOTO DEF") {
                    i += 2 + close + 2;
                    continue;
                }
            }
        }

        pos += 1;
        i += 1;
    }

    found_target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_display() {
        assert_eq!(normalize_display("class Foo"), "class Foo");
        assert_eq!(
            normalize_display("```typescript\nclass Foo\n```"),
            "class Foo"
        );
        assert_eq!(
            normalize_display("const x: number\n\n---\n\nSome doc"),
            "const x: number"
        );
    }

    #[test]
    fn test_compare_quick_info_match() {
        let entry = QuickInfoEntry {
            marker_name: "1".into(),
            file_name: "/test.ts".into(),
            position: 10,
            display_text: "class Foo".into(),
            documentation: String::new(),
            kind: "class".into(),
        };
        assert!(compare_quick_info(Some("class Foo"), &entry));
    }

    #[test]
    fn test_compare_quick_info_mismatch() {
        let entry = QuickInfoEntry {
            marker_name: "1".into(),
            file_name: "/test.ts".into(),
            position: 10,
            display_text: "class Foo".into(),
            documentation: String::new(),
            kind: "class".into(),
        };
        assert!(!compare_quick_info(Some("var Foo: any"), &entry));
    }
}
