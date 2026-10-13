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
    /// Reference locations in the top-level source annotation. The separate
    /// Definitions section describes groups; it does not add reference uses.
    pub references: Vec<(String, tsc_rs_ast::Span)>,
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

pub fn load_find_all_refs_baseline(
    path: &Path,
    states: &[HashMap<String, String>],
) -> Result<Vec<FindAllRefsEntry>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read baseline: {e}"))?;
    parse_reference_sections(&content, states)
}

fn parse_reference_sections(
    content: &str,
    states: &[HashMap<String, String>],
) -> Result<Vec<FindAllRefsEntry>, String> {
    let mut entries = Vec::new();
    let mut references = Vec::new();
    let mut active = false;
    let mut file: Option<&str> = None;
    let mut source_line = 0usize;
    let mut stack = Vec::new();
    for line in content.lines() {
        if is_operation_header(line) {
            if active {
                if !stack.is_empty() {
                    return Err("unclosed reference range".into());
                }
                entries.push(FindAllRefsEntry {
                    references: std::mem::take(&mut references),
                });
            }
            active = line == "// === findAllReferences ===";
            file = None;
            continue;
        }
        if !active {
            continue;
        }
        let sources = states
            .get(entries.len())
            .ok_or("more reference baselines than requests")?;
        // Indented Definitions/Details are explanatory copies, not references.
        if line.starts_with("  // === ") {
            file = None;
            continue;
        }
        if let Some(path) = line
            .strip_prefix("// === ")
            .and_then(|s| s.strip_suffix(" ==="))
        {
            if !stack.is_empty() {
                return Err("reference range crosses files".into());
            }
            if !sources
                .keys()
                .any(|f| normalize_reference_path(f) == normalize_reference_path(path))
            {
                return Err(format!("missing virtual source: {path}"));
            }
            file = Some(path);
            source_line = 0;
            continue;
        }
        let Some(file) = file else {
            continue;
        };
        let Some(annotated) = line.strip_prefix("// ") else {
            continue;
        };
        if let Some(rest) = annotated.strip_prefix("--- (line: ") {
            source_line = rest
                .split(')')
                .next()
                .unwrap_or("")
                .parse()
                .map_err(|_| "invalid omitted reference line")?;
            continue;
        }
        let source = sources
            .iter()
            .find(|(f, _)| normalize_reference_path(f) == normalize_reference_path(file))
            .map(|(_, s)| s)
            .ok_or("missing reference source")?;
        let source_lines: Vec<_> = source.split('\n').collect();
        let actual = source_lines
            .get(source_line)
            .ok_or_else(|| format!("reference line out of range: {file}:{source_line}"))?;
        let line_start: usize = source_lines[..source_line]
            .iter()
            .map(|l| l.len() + 1)
            .sum();
        let mut plain = String::new();
        let mut events = Vec::new();
        let mut rest = annotated;
        while !rest.is_empty() {
            if let Some(s) = rest.strip_prefix("[|") {
                events.push((plain.len(), true));
                rest = s;
            } else if let Some(s) = rest.strip_prefix("|]") {
                events.push((plain.len(), false));
                rest = s;
            } else if rest.starts_with("<|") || rest.starts_with("|>") {
                rest = &rest[2..];
            } else if rest.starts_with("{|") {
                let end = rest.find("|}").ok_or("unterminated reference metadata")?;
                rest = &rest[end + 2..];
            } else if let Some(s) = rest.strip_prefix("/*FIND ALL REFS*/") {
                rest = s;
            } else {
                let c = rest.chars().next().unwrap();
                plain.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        if plain.trim_start() != actual.trim_start() {
            return Err(format!(
                "reference annotation differs from source: {file}:{}: {plain:?} != {actual:?}",
                source_line + 1
            ));
        }
        let old_indent = plain.len() - plain.trim_start().len();
        let new_indent = actual.len() - actual.trim_start().len();
        for (column, open) in events {
            let position = (line_start + column.saturating_sub(old_indent) + new_indent) as u32;
            if open {
                stack.push(position);
            } else {
                let start = stack.pop().ok_or("unmatched reference range end")?;
                references.push((
                    normalize_reference_path(file),
                    tsc_rs_ast::Span::new(start, position),
                ));
            }
        }
        source_line += 1;
    }
    if active {
        if !stack.is_empty() {
            return Err("unclosed reference range".into());
        }
        entries.push(FindAllRefsEntry { references });
    }
    Ok(entries)
}

pub(crate) fn normalize_reference_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------------------
// goToDefinition baselines
// ---------------------------------------------------------------------------

/// A single goToDefinition baseline entry (one section per marker).
#[derive(Debug, Clone)]
pub struct GoToDefinitionEntry {
    /// Every declaration target, in baseline order. Cursor ranges are excluded.
    pub targets: Vec<(String, u32)>,
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
        if trimmed.starts_with("// === ")
            && !trimmed.starts_with("// === /")
            && trimmed != "// === Details ==="
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
                    // N is the zero-based line where the excerpt resumes.
                    // Restore the actual source lines from
                    // the virtual file to keep offset calculations correct.
                    if rest.contains("skipped") {
                        if let Some(start) = rest.find("(line: ") {
                            let num_start = start + 7;
                            if let Some(end) = rest[num_start..].find(')') {
                                if let Ok(line_num) =
                                    rest[num_start..num_start + end].parse::<usize>()
                                {
                                    // Look up the virtual source for this file and
                                    // restore the omitted lines before this excerpt.
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
                                        // N is the zero-based line of the next
                                        // displayed source line. Restore only the
                                        // gap, including gaps after displayed lines.
                                        for li in annotated_lines.len()..line_num {
                                            if li < src_lines.len() {
                                                annotated_lines.push(src_lines[li].to_string());
                                            } else {
                                                annotated_lines.push(String::new());
                                            }
                                        }
                                    } else {
                                        // Fallback: insert empty lines
                                        for _ in annotated_lines.len()..line_num {
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

    let targets = definition_targets(&file_blocks, virtual_sources);
    let target_file = targets.first().map(|(file, _)| file.clone());
    let target_position = targets.first().map(|(_, position)| *position);
    let expects_no_definition = targets.is_empty() && name.is_empty();

    (
        GoToDefinitionEntry {
            targets,
            target_file,
            target_position,
            name,
            kind,
            expects_no_definition,
        },
        i,
    )
}

/// Read all definition ranges, respecting defId metadata and excluding the
/// cursor's textSpan. Align to the virtual source so indentation stripped by
/// fourslash and omitted source lines cannot shift a target to another token.
fn definition_targets(
    blocks: &[FileBlock],
    sources: &HashMap<String, String>,
) -> Vec<(String, u32)> {
    struct Range {
        start: usize,
        end: usize,
        in_declaration: bool,
        def_id: Option<usize>,
        cursor: bool,
    }
    let mut ranges = vec![];
    for block in blocks {
        let text = &block.annotated;
        let mut plain = String::new();
        let mut stack: Vec<Range> = vec![];
        let mut completed = vec![];
        let mut declaration_depth = 0usize;
        let mut cursors = vec![];
        let mut i = 0;
        while i < text.len() {
            let rest = &text[i..];
            if rest.starts_with("<|") {
                declaration_depth += 1;
                i += 2;
            } else if rest.starts_with("|>") {
                declaration_depth = declaration_depth.saturating_sub(1);
                i += 2;
            } else if rest.starts_with("[|") {
                stack.push(Range {
                    start: plain.len(),
                    end: plain.len(),
                    in_declaration: declaration_depth > 0,
                    def_id: None,
                    cursor: false,
                });
                i += 2;
            } else if rest.starts_with("|]") {
                if let Some(mut range) = stack.pop() {
                    range.end = plain.len();
                    completed.push(range);
                }
                i += 2;
            } else if rest.starts_with("{|") {
                if let Some(end) = rest.find("|}") {
                    if let Some(range) = stack.last_mut() {
                        if let Some((_, value)) = rest[..end].split_once("defId:") {
                            range.def_id = value
                                .trim_start()
                                .chars()
                                .take_while(char::is_ascii_digit)
                                .collect::<String>()
                                .parse()
                                .ok();
                        }
                    }
                    i += end + 2;
                } else {
                    i += 2;
                }
            } else if rest.starts_with("/*GOTO DEF") {
                cursors.push(plain.len());
                if let Some(end) = rest.find("*/") {
                    i += end + 2;
                } else {
                    break;
                }
            } else {
                let c = rest.chars().next().unwrap();
                plain.push(c);
                i += c.len_utf8();
            }
        }
        let source = sources.get(&block.path);
        for mut range in completed {
            range.cursor = cursors.iter().any(|p| range.start <= *p && *p <= range.end);
            let position = if range.start == 0 && (range.end == 0 || range.end == plain.len()) {
                0
            } else if let Some(source) = source {
                let line = plain[..range.start].bytes().filter(|b| *b == b'\n').count();
                let column = range.start - plain[..range.start].rfind('\n').map_or(0, |p| p + 1);
                let source_lines: Vec<_> = source.split('\n').collect();
                let annotated_line = plain.split('\n').nth(line).unwrap_or("");
                if let Some(source_line) = source_lines.get(line) {
                    let indent_source = source_line.len() - source_line.trim_start().len();
                    let indent_annotated = annotated_line.len() - annotated_line.trim_start().len();
                    source_lines[..line]
                        .iter()
                        .map(|s| s.len() + 1)
                        .sum::<usize>()
                        + column.saturating_sub(indent_annotated)
                        + indent_source
                } else {
                    range.start
                }
            } else {
                range.start
            };
            ranges.push((block.path.clone(), position as u32, range));
        }
    }
    let has_ids = ranges.iter().any(|(_, _, r)| r.def_id.is_some());
    let has_declarations = ranges.iter().any(|(_, _, r)| r.in_declaration);
    let single_range = ranges.len() == 1;
    ranges.retain(|(_, _, r)| {
        if has_ids {
            r.def_id.is_some()
        } else if has_declarations {
            r.in_declaration
        } else {
            !r.cursor || single_range
        }
    });
    if has_ids {
        ranges.sort_by_key(|(_, _, r)| r.def_id);
    }
    ranges
        .into_iter()
        .map(|(file, pos, _)| (file, pos))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_ranges_include_every_id_and_exclude_the_cursor() {
        let sources = HashMap::from([("/a.ts".into(), "class C { static {} static {} }".into())]);
        let blocks = vec![FileBlock { path: "/a.ts".into(), annotated: "class C { <|[|{| defId: 0 |}static|] {}|> <|/*GOTO DEF*/[|{| textSpan: true, defId: 1 |}static|] {}|> }".into() }];
        assert_eq!(
            definition_targets(&blocks, &sources),
            vec![("/a.ts".into(), 10), ("/a.ts".into(), 20)]
        );
    }

    #[test]
    fn definition_module_start_is_zero_despite_fourslash_indentation() {
        let sources = HashMap::from([("/a.ts".into(), " export const x = 1;".into())]);
        let blocks = vec![
            FileBlock {
                path: "/a.ts".into(),
                annotated: "[|export const x = 1;|]".into(),
            },
            FileBlock {
                path: "/b.ts".into(),
                annotated: "import /*GOTO DEF*/[|alias|] = require('./a');".into(),
            },
        ];
        assert_eq!(
            definition_targets(&blocks, &sources),
            vec![("/a.ts".into(), 0)]
        );
    }

    #[test]
    fn definition_parser_restores_middle_gaps_and_stops_at_other_operations() {
        let source = "const before = 1;\n// omitted\nconst target = 2;\ntarget;";
        let sources = HashMap::from([("/a.ts".into(), source.into())]);
        let lines = [
            "// === /a.ts ===",
            "// const before = 1;",
            "// --- (line: 2) skipped ---",
            "// <|const [|target|] = 2;|>",
            "// /*GOTO DEF*/target;",
            "// === Details ===",
            "[{\"name\":\"target\",\"kind\":\"const\"}]",
            "// === findReferences ===",
            "// === /a.ts ===",
            "// [|unrelated|]",
        ];
        let (entry, next) = parse_goto_definition_section(&lines, 0, &sources);
        assert_eq!(entry.targets, vec![("/a.ts".into(), 35)]);
        assert_eq!(next, 7);
        assert!(!entry.expects_no_definition);
    }

    #[test]
    fn reference_locations_ignore_definition_copies_and_metadata() {
        let states = vec![HashMap::from([("/a.ts".into(), "const x = 1;\nx;".into())])];
        let entries = parse_reference_sections("// === findAllReferences ===\n// === /a.ts ===\n// const [|{| isDefinition: true |}x|] = 1;\n// /*FIND ALL REFS*/[|x|];\n  // === Definitions ===\n  // === /a.ts ===\n  // const [|x|] = 1;", &states).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].references,
            vec![
                ("/a.ts".into(), tsc_rs_ast::Span::new(6, 7)),
                ("/a.ts".into(), tsc_rs_ast::Span::new(13, 14))
            ]
        );
    }

    #[test]
    fn reference_locations_restore_skipped_lines_and_indentation() {
        let states = vec![HashMap::from([(
            "/src/./a.ts".into(),
            "// one\n// two\n    name;".into(),
        )])];
        let entries = parse_reference_sections("// === findAllReferences ===\n// === /src/a.ts ===\n// --- (line: 2) skipped ---\n// [|name|];", &states).unwrap();
        assert_eq!(
            entries[0].references,
            vec![("/src/a.ts".into(), tsc_rs_ast::Span::new(18, 22))]
        );
    }

    #[test]
    fn reference_locations_use_each_request_snapshot() {
        let states = vec![
            HashMap::from([("/a.ts".into(), "x;".into())]),
            HashMap::from([("/a.ts".into(), "\nx;".into())]),
        ];
        let entries = parse_reference_sections("// === findAllReferences ===\n// === /a.ts ===\n// [|x|];\n// === findAllReferences ===\n// === /a.ts ===\n// \n// [|x|];", &states).unwrap();
        assert_eq!(entries[0].references[0].1, tsc_rs_ast::Span::new(0, 1));
        assert_eq!(entries[1].references[0].1, tsc_rs_ast::Span::new(1, 2));
    }

    #[test]
    fn reference_locations_reject_incomplete_or_inconsistent_oracles() {
        let states = vec![HashMap::from([("/a.ts".into(), "x;".into())])];
        for body in ["[|x;", "y;", "x|];"] {
            assert!(parse_reference_sections(
                &format!("// === findAllReferences ===\n// === /a.ts ===\n// {body}"),
                &states
            )
            .is_err());
        }
        assert!(parse_reference_sections(
            "// === findAllReferences ===\n// === /missing.ts ===",
            &states
        )
        .is_err());
    }

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
