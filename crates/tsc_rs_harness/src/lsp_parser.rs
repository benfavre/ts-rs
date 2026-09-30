//! Fourslash test file parser for the LSP harness.
//!
//! Parses `tests/cases/fourslash/*.ts` files to extract virtual source files,
//! marker positions, and verify commands.

use std::collections::HashMap;
use tsc_rs_ast::{CompilerOptions, JsxEmit, ModuleKind, ScriptTarget};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LspTest {
    pub name: String,
    pub files: Vec<LspTestFile>,
    pub markers: Vec<Marker>,
    pub verify_commands: Vec<VerifyCommand>,
    pub verify_text: String,
    pub options: CompilerOptions,
    pub has_edits: bool,
    pub include_module_exports: bool,
}

#[derive(Debug, Clone)]
pub struct LspTestFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct Marker {
    pub name: String,
    pub file_name: String,
    pub position: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignatureHelpTagExpectation {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignatureHelpTriggerReasonExpectation {
    pub kind: String,
    pub trigger_character: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SignatureHelpExpectation {
    pub text: Option<String>,
    pub overloads_count: Option<usize>,
    pub parameter_count: Option<usize>,
    pub argument_count: Option<usize>,
    pub parameter_name: Option<String>,
    pub parameter_span: Option<String>,
    pub doc_comment: Option<String>,
    pub parameter_doc_comment: Option<String>,
    pub is_variadic: Option<bool>,
    pub tags: Option<Vec<SignatureHelpTagExpectation>>,
    pub trigger_reason: Option<SignatureHelpTriggerReasonExpectation>,
    pub unsupported_fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum VerifyCommand {
    BaselineQuickInfo,
    QuickInfoAt {
        marker: String,
        expected_text: String,
    },
    QuickInfos {
        entries: HashMap<String, String>,
    },
    BaselineGoToDefinition {
        markers: Vec<String>,
    },
    BaselineFindAllReferences {
        markers: Vec<String>,
    },
    Completions {
        marker: String,
        includes: Vec<String>,
        excludes: Vec<String>,
        exact: Option<Vec<String>>,
    },
    BaselineCompletions,
    BaselineRename {
        markers: Vec<String>,
    },
    SignatureHelp {
        markers: Vec<String>,
        expected: SignatureHelpExpectation,
    },
    BaselineSignatureHelp,
    NoSignatureHelp {
        markers: Vec<String>,
    },
    QuickInfoExists {
        marker: String,
    },
    NoQuickInfo {
        markers: Vec<String>,
    },
    NoErrors,
    Unsupported {
        raw: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LspOperation {
    QuickInfo,
    GoToDefinition,
    FindAllReferences,
    Completions,
    SignatureHelp,
    Rename,
    Other,
}

impl LspOperation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::QuickInfo => "quickinfo",
            Self::GoToDefinition => "gotodefinition",
            Self::FindAllReferences => "findallrefs",
            Self::Completions => "completions",
            Self::SignatureHelp => "signaturehelp",
            Self::Rename => "rename",
            Self::Other => "other",
        }
    }
}

impl VerifyCommand {
    pub fn operation(&self) -> LspOperation {
        match self {
            Self::BaselineQuickInfo | Self::QuickInfoAt { .. } | Self::QuickInfos { .. } => {
                LspOperation::QuickInfo
            }
            Self::BaselineGoToDefinition { .. } => LspOperation::GoToDefinition,
            Self::BaselineFindAllReferences { .. } => LspOperation::FindAllReferences,
            Self::Completions { .. } | Self::BaselineCompletions => LspOperation::Completions,
            Self::BaselineRename { .. } => LspOperation::Rename,
            Self::SignatureHelp { .. }
            | Self::BaselineSignatureHelp
            | Self::NoSignatureHelp { .. } => LspOperation::SignatureHelp,
            Self::QuickInfoExists { .. } | Self::NoQuickInfo { .. } => LspOperation::QuickInfo,
            Self::NoErrors | Self::Unsupported { .. } => LspOperation::Other,
        }
    }
}

/// Determine the primary operation a test exercises by scanning for verify commands.
pub fn classify_test_operation(source: &str) -> LspOperation {
    if source.contains("verify.baselineQuickInfo")
        || source.contains("verify.quickInfoAt")
        || source.contains("verify.quickInfos")
        || source.contains("verify.quickInfoIs")
        || source.contains("verify.quickInfoExists")
        || source.contains("verify.not.quickInfoExists")
    {
        return LspOperation::QuickInfo;
    }
    if source.contains("verify.baselineGoToDefinition")
        || source.contains("verify.baselineGetDefinitionAtPosition")
    {
        return LspOperation::GoToDefinition;
    }
    if source.contains("verify.baselineFindAllReferences") {
        return LspOperation::FindAllReferences;
    }
    if source.contains("verify.completions") || source.contains("verify.baselineCompletions") {
        return LspOperation::Completions;
    }
    if source.contains("verify.baselineRename") {
        return LspOperation::Rename;
    }
    if source.contains("verify.signatureHelp")
        || source.contains("verify.baselineSignatureHelp")
        || source.contains("verify.noSignatureHelp")
        || source.contains("verify.signatureHelpPresentForTriggerReason")
    {
        return LspOperation::SignatureHelp;
    }
    LspOperation::Other
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a fourslash test file into an `LspTest`.
pub fn parse_fourslash(test_name: &str, raw: &str) -> Result<LspTest, String> {
    let mut options = CompilerOptions::default();
    let mut file_sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut current_file = format!("/tests/cases/fourslash/{}.ts", test_name);
    let mut current_lines: Vec<String> = Vec::new();
    let mut verify_lines: Vec<String> = Vec::new();
    let mut in_verify = false;

    for line in raw.lines() {
        // Skip the fourslash reference
        if line.starts_with("/// <reference") {
            continue;
        }

        // Option directives (before or between files)
        if let Some(rest) = line
            .strip_prefix("// @")
            .or_else(|| line.strip_prefix("//@"))
        {
            if let Some((key, val)) = rest.split_once(':') {
                let key_lower = key.trim().to_ascii_lowercase();
                let val = val.trim();

                if key_lower == "filename" {
                    // Flush current file
                    if !current_lines.is_empty() || file_sections.is_empty() {
                        file_sections
                            .push((current_file.clone(), std::mem::take(&mut current_lines)));
                    }
                    // Relative names anchor at the fourslash root, like
                    // tsc's test harness.
                    let cleaned = val.strip_prefix("./").unwrap_or(val);
                    current_file = if cleaned.starts_with('/') {
                        cleaned.to_string()
                    } else {
                        format!("/tests/cases/fourslash/{}", cleaned)
                    };
                    continue;
                }

                parse_option(&key_lower, val, &mut options);
            }
            continue;
        }

        // Source lines (////...)
        if let Some(rest) = line.strip_prefix("////") {
            in_verify = false;
            current_lines.push(rest.to_string());
            continue;
        }

        // Everything else after the source sections is the verify/imperative section
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with("///") && !trimmed.starts_with("// @") {
            in_verify = true;
        }
        if in_verify && !trimmed.is_empty() {
            verify_lines.push(line.to_string());
        }
    }

    // Flush last file
    file_sections.push((current_file, current_lines));

    // Fourslash tests run with tsc's language-service defaults, which are NOT
    // strict (the checker otherwise follows TS 6's strict-by-default CLI
    // semantics). Expectations like `optionalParam?: string` (no
    // `| undefined`) depend on it; tests opt in with `// @strict: true`.
    if options.strict.is_none() {
        options.strict = Some(false);
    }

    // Process files: strip markers from content, record marker positions
    let mut files = Vec::new();
    let mut markers = Vec::new();

    for (file_name, lines) in &file_sections {
        let raw_content = lines.join("\n");
        let (content, file_markers) = extract_markers(&raw_content, file_name);
        markers.extend(file_markers);
        if !content.is_empty() || lines.len() > 0 {
            files.push(LspTestFile {
                name: file_name.clone(),
                content,
            });
        }
    }

    // Parse verify commands
    // Drop `//`-commented SCRIPT lines: fourslash treats them as comments, but
    // our substring-scanning command extractors would otherwise execute
    // commented-out verifications (regexErrorRecovery has its whole script
    // commented except one edit).
    let verify_text = verify_lines
        .iter()
        .filter(|l| !l.trim_start().starts_with("//"))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let verify_commands = parse_verify_commands(&verify_text);

    Ok(LspTest {
        name: test_name.to_string(),
        files,
        markers,
        verify_commands,
        verify_text: verify_text.clone(),
        options,
        has_edits: verify_text.contains("edit."),
        include_module_exports: verify_text.contains("includeCompletionsForModuleExports: true"),
    })
}

/// Extract `/*name*/` markers from content, returning cleaned content and markers.
fn extract_markers(content: &str, file_name: &str) -> (String, Vec<Marker>) {
    let mut result = String::with_capacity(content.len());
    let mut markers = Vec::new();
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut pos: u32 = 0;

    while i < len {
        // Strip definition range markers [| and |]
        if i + 1 < len && bytes[i] == b'[' && bytes[i + 1] == b'|' {
            i += 2;
            continue;
        }
        if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b']' {
            i += 2;
            continue;
        }
        // Strip inline JSON markers {| ... |}
        // These have the form {| "name": "markerName", ... |}
        if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'|' {
            if let Some(close) = content[i + 2..].find("|}") {
                let json_text = &content[i + 2..i + 2 + close].trim();
                // Extract marker name from JSON-like content: "name" : "value"
                let marker_name = extract_json_name_value(json_text);
                if let Some(name) = marker_name {
                    markers.push(Marker {
                        name,
                        file_name: file_name.to_string(),
                        position: pos,
                    });
                }
                i += 2 + close + 2; // skip past |}
                continue;
            }
        }
        // Check for /*...*/ marker pattern
        if i + 2 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Find the closing */
            if let Some(close) = content[i + 2..].find("*/") {
                let marker_name = &content[i + 2..i + 2 + close];
                // Only treat as a marker if it's alphanumeric (or empty for unnamed)
                if marker_name.is_empty()
                    || marker_name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    markers.push(Marker {
                        name: marker_name.to_string(),
                        file_name: file_name.to_string(),
                        position: pos,
                    });
                    i += 2 + close + 2; // skip past */
                    continue;
                }
            }
        }
        result.push(bytes[i] as char);
        pos += 1;
        i += 1;
    }

    (result, markers)
}

/// Extract the "name" value from a JSON-like inline marker.
/// Input: `"name" : "markerName"` or `"name": "markerName", "contextRangeIndex": 0`
/// Returns: Some("markerName") or None
fn extract_json_name_value(json_text: &str) -> Option<String> {
    // Look for "name" : "value" pattern
    let name_key = json_text.find("\"name\"")?;
    let after_key = &json_text[name_key + 6..].trim_start();
    let after_colon = after_key.strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    let after_quote = after_colon.strip_prefix('"')?;
    let end_quote = after_quote.find('"')?;
    Some(after_quote[..end_quote].to_string())
}

/// Parse compiler options from `// @key: value` directives.
fn parse_option(key: &str, val: &str, options: &mut CompilerOptions) {
    match key {
        "target" => {
            options.target = ScriptTarget::parse(val);
        }
        "module" => {
            options.module = ModuleKind::parse(val);
        }
        "jsx" => {
            options.jsx = JsxEmit::parse(val);
        }
        "strict" => {
            options.strict = Some(val.eq_ignore_ascii_case("true"));
        }
        "noimplicitany" => {
            options.no_implicit_any = Some(val.eq_ignore_ascii_case("true"));
        }
        "strictnullchecks" => {
            options.strict_null_checks = Some(val.eq_ignore_ascii_case("true"));
        }
        "declaration" => {
            options.declaration = Some(val.eq_ignore_ascii_case("true"));
        }
        "noemit" => {
            options.no_emit = Some(val.eq_ignore_ascii_case("true"));
        }
        "allowjs" => {
            options.allow_js = Some(val.eq_ignore_ascii_case("true"));
        }
        "esmoduleinterop" => {
            options.es_module_interop = Some(val.eq_ignore_ascii_case("true"));
        }
        "allowimportingtsextensions" => {
            options.allow_importing_ts_extensions = Some(val.eq_ignore_ascii_case("true"));
        }
        "moduleresolution" => {
            options.module_resolution = Some(val.to_string());
        }
        _ => {} // ignore unknown options
    }
}

/// Parse verify.* commands from the imperative section.
fn parse_verify_commands(text: &str) -> Vec<VerifyCommand> {
    let mut commands = parse_verify_commands_inner(text);
    // `[0, 1, 2].forEach(marker => ... marker: `${marker}` ...)` — expand
    // template-interpolated markers over the loop's array elements.
    if commands
        .iter()
        .any(|c| matches!(c, VerifyCommand::Completions { marker, .. } if marker.contains("${")))
    {
        if let Some(fe) = text.find("].forEach(") {
            if let Some(open) = text[..fe].rfind('[') {
                let elems: Vec<String> = text[open + 1..fe]
                    .split(',')
                    .map(|e| {
                        e.trim()
                            .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                            .to_string()
                    })
                    .filter(|e| !e.is_empty())
                    .collect();
                if !elems.is_empty() {
                    let mut expanded = Vec::new();
                    for c in commands {
                        match c {
                            VerifyCommand::Completions {
                                marker,
                                includes,
                                excludes,
                                exact,
                            } if marker.contains("${") => {
                                for e in &elems {
                                    expanded.push(VerifyCommand::Completions {
                                        marker: e.clone(),
                                        includes: includes.clone(),
                                        excludes: excludes.clone(),
                                        exact: exact.clone(),
                                    });
                                }
                            }
                            other => expanded.push(other),
                        }
                    }
                    return expanded;
                }
            }
        }
    }
    commands
}

fn parse_verify_commands_inner(text: &str) -> Vec<VerifyCommand> {
    let mut commands = Vec::new();

    // Match verify.baselineQuickInfo()
    if text.contains("verify.baselineQuickInfo") {
        commands.push(VerifyCommand::BaselineQuickInfo);
    }

    // Match verify.quickInfoAt("marker", "expected")
    for cap in find_quick_info_at_calls(text) {
        commands.push(cap);
    }

    // Match verify.quickInfos({ ... })
    if let Some(cmd) = parse_quick_infos(text) {
        commands.push(cmd);
    }

    // Match goTo.marker("name"); verify.quickInfoIs("text") pairs
    for cmd in find_quick_info_is_calls(text) {
        commands.push(cmd);
    }

    // Match verify.quickInfoExists() — hover should return something
    if text.contains("verify.quickInfoExists()") && !text.contains("verify.not.quickInfoExists()") {
        // Find the preceding goTo.marker() to get the marker name
        let pattern = "verify.quickInfoExists()";
        let goto_pattern = "goTo.marker(";
        let mut search_from = 0;
        while let Some(pos) = text[search_from..].find(pattern) {
            let abs_pos = search_from + pos;
            let before = &text[..abs_pos];
            let marker = if let Some(goto_pos) = before.rfind(goto_pattern) {
                let goto_arg_start = goto_pos + goto_pattern.len();
                extract_string_at(text, goto_arg_start)
                    .map(|(s, _)| s)
                    .unwrap_or_default()
            } else {
                String::new()
            };
            commands.push(VerifyCommand::QuickInfoExists { marker });
            search_from = abs_pos + pattern.len();
        }
    }

    // Match verify.not.quickInfoExists() — hover should return nothing
    if text.contains("verify.not.quickInfoExists()") {
        // Check for goTo.eachMarker pattern
        if text.contains("goTo.eachMarker") {
            // Apply to all markers
            commands.push(VerifyCommand::NoQuickInfo {
                markers: vec![String::new()], // empty = all markers
            });
        } else {
            let pattern = "verify.not.quickInfoExists()";
            let goto_pattern = "goTo.marker(";
            let mut search_from = 0;
            while let Some(pos) = text[search_from..].find(pattern) {
                let abs_pos = search_from + pos;
                let before = &text[..abs_pos];
                let marker = if let Some(goto_pos) = before.rfind(goto_pattern) {
                    let goto_arg_start = goto_pos + goto_pattern.len();
                    extract_string_at(text, goto_arg_start)
                        .map(|(s, _)| s)
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                commands.push(VerifyCommand::NoQuickInfo {
                    markers: vec![marker],
                });
                search_from = abs_pos + pattern.len();
            }
        }
    }

    // Match verify.baselineGoToDefinition(...) or verify.baselineGetDefinitionAtPosition(...)
    if text.contains("verify.baselineGoToDefinition") {
        let markers = extract_string_args(text, "verify.baselineGoToDefinition");
        commands.push(VerifyCommand::BaselineGoToDefinition { markers });
    } else if text.contains("verify.baselineGetDefinitionAtPosition") {
        let markers = extract_string_args(text, "verify.baselineGetDefinitionAtPosition");
        commands.push(VerifyCommand::BaselineGoToDefinition { markers });
    }

    // Match verify.baselineFindAllReferences(...)
    if text.contains("verify.baselineFindAllReferences") {
        let markers = extract_string_args(text, "verify.baselineFindAllReferences");
        commands.push(VerifyCommand::BaselineFindAllReferences { markers });
    }

    // Match verify.completions(...)
    if text.contains("verify.completions") && !text.contains("verify.baselineCompletions") {
        commands.extend(parse_completions_calls(text));
    }

    if text.contains("verify.baselineCompletions") {
        commands.push(VerifyCommand::BaselineCompletions);
    }

    if text.contains("verify.baselineRename") {
        let markers = extract_string_args(text, "verify.baselineRename");
        commands.push(VerifyCommand::BaselineRename { markers });
    }

    if text.contains("verify.baselineSignatureHelp") {
        commands.push(VerifyCommand::BaselineSignatureHelp);
    }

    commands.extend(parse_signature_help_calls(text));
    commands.extend(parse_no_signature_help_calls(text));

    if text.contains("verify.signatureHelpPresentForTriggerReason") {
        commands.push(VerifyCommand::Unsupported {
            raw: "verify.signatureHelpPresentForTriggerReason".to_string(),
        });
    }

    if text.contains("verify.noErrors") {
        commands.push(VerifyCommand::NoErrors);
    }

    // If we found nothing, check for any verify.* and mark as unsupported
    if commands.is_empty() {
        if let Some(pos) = text.find("verify.") {
            let end = text[pos..]
                .find('(')
                .map(|e| pos + e)
                .unwrap_or(text.len().min(pos + 40));
            commands.push(VerifyCommand::Unsupported {
                raw: text[pos..end].to_string(),
            });
        }
    }

    commands
}

fn parse_signature_help_calls(text: &str) -> Vec<VerifyCommand> {
    let mut commands = Vec::new();
    let pattern = "verify.signatureHelp(";
    let mut search_from = 0;

    while let Some(pos) = text[search_from..].find(pattern) {
        let abs_pos = search_from + pos;
        let args_start = abs_pos + pattern.len();
        let Some(args_end) = find_matching_close(text, args_start - 1, b'(', b')') else {
            search_from = args_start;
            continue;
        };

        let args_text = text[args_start..args_end].trim();
        if args_text.starts_with('{') {
            if let Some(obj_end) = find_matching_close(args_text, 0, b'{', b'}') {
                commands.push(parse_single_signature_help_obj(&args_text[..=obj_end]));
            } else {
                commands.push(VerifyCommand::Unsupported {
                    raw: "verify.signatureHelp".to_string(),
                });
            }
        } else {
            commands.push(VerifyCommand::Unsupported {
                raw: "verify.signatureHelp".to_string(),
            });
        }

        search_from = args_end + 1;
    }

    commands
}

fn parse_single_signature_help_obj(obj_text: &str) -> VerifyCommand {
    let mut unsupported_fields = Vec::new();
    let mut markers = extract_marker_list(obj_text);
    if markers.is_empty() {
        if obj_text.contains("marker:") || obj_text.contains("marker :") {
            unsupported_fields.push("marker".to_string());
        } else {
            markers.push(String::new());
        }
    }

    let supported_fields = [
        "marker",
        "text",
        "overloadsCount",
        "parameterCount",
        "argumentCount",
        "parameterName",
        "parameterSpan",
        "docComment",
        "parameterDocComment",
        "isVariadic",
        "tags",
        "triggerReason",
    ];
    for field in extract_top_level_object_keys(obj_text) {
        if !supported_fields.contains(&field.as_str()) {
            unsupported_fields.push(field);
        }
    }
    unsupported_fields.sort();
    unsupported_fields.dedup();

    VerifyCommand::SignatureHelp {
        markers,
        expected: SignatureHelpExpectation {
            text: extract_obj_string_field(obj_text, "text"),
            overloads_count: extract_obj_usize_field(obj_text, "overloadsCount"),
            parameter_count: extract_obj_usize_field(obj_text, "parameterCount"),
            argument_count: extract_obj_usize_field(obj_text, "argumentCount"),
            parameter_name: extract_obj_string_field(obj_text, "parameterName"),
            parameter_span: extract_obj_string_field(obj_text, "parameterSpan"),
            doc_comment: extract_obj_string_field(obj_text, "docComment"),
            parameter_doc_comment: extract_obj_string_field(obj_text, "parameterDocComment"),
            is_variadic: extract_obj_bool_field(obj_text, "isVariadic"),
            tags: None,
            trigger_reason: None,
            unsupported_fields,
        },
    }
}

fn parse_no_signature_help_calls(text: &str) -> Vec<VerifyCommand> {
    let mut commands = Vec::new();
    let pattern = "verify.noSignatureHelp(";
    let mut search_from = 0;

    while let Some(pos) = text[search_from..].find(pattern) {
        let abs_pos = search_from + pos;
        let args_start = abs_pos + pattern.len();
        let Some(args_end) = find_matching_close(text, args_start - 1, b'(', b')') else {
            search_from = args_start;
            continue;
        };

        let args_text = text[args_start..args_end].trim();
        let markers = if args_text.is_empty() {
            vec![String::new()]
        } else if args_text.starts_with('[') {
            let values = extract_names_from_array(args_text);
            if values.is_empty() {
                vec![String::new()]
            } else {
                values
            }
        } else if args_text.starts_with('"')
            || args_text.starts_with('\'')
            || args_text.starts_with('`')
        {
            extract_string_at(args_text, 0)
                .map(|(s, _)| vec![s])
                .unwrap_or_else(|| vec![String::new()])
        } else {
            vec![String::new()]
        };

        commands.push(VerifyCommand::NoSignatureHelp { markers });
        search_from = args_end + 1;
    }

    commands
}

/// Parse `goTo.marker("name"); verify.quickInfoIs("text")` pairs.
fn find_quick_info_is_calls(text: &str) -> Vec<VerifyCommand> {
    let mut results = Vec::new();
    let pattern = "verify.quickInfoIs(";
    let goto_pattern = "goTo.marker(";
    let mut search_from = 0;

    while let Some(pos) = text[search_from..].find(pattern) {
        let abs_pos = search_from + pos;
        let start = abs_pos + pattern.len();

        // Extract the expected text (first string arg)
        if let Some((expected_text, _)) = extract_string_at(text, start) {
            // Find the most recent goTo.marker() call before this quickInfoIs
            let before = &text[..abs_pos];
            if let Some(goto_pos) = before.rfind(goto_pattern) {
                let goto_arg_start = goto_pos + goto_pattern.len();
                // Handle goTo.marker() with no argument (empty parens)
                let marker_name = if let Some((name, _)) = extract_string_at(text, goto_arg_start) {
                    name
                } else {
                    // No string arg — use empty string to mean "first/unnamed marker"
                    String::new()
                };
                results.push(VerifyCommand::QuickInfoAt {
                    marker: marker_name,
                    expected_text,
                });
            }
        }
        search_from = start;
    }
    results
}

/// Parse `verify.quickInfoAt("marker", "text")` calls.
/// Also handles `verify.quickInfoAt("marker", ["line1", "line2"].join("\n"))`.
fn find_quick_info_at_calls(text: &str) -> Vec<VerifyCommand> {
    let mut results = Vec::new();
    let pattern = "verify.quickInfoAt(";
    let mut search_from = 0;
    while let Some(pos) = text[search_from..].find(pattern) {
        let start = search_from + pos + pattern.len();
        // Try standard two-string-arg pattern first
        if let Some((marker, expected)) = parse_two_string_args(&text[start..]) {
            results.push(VerifyCommand::QuickInfoAt {
                marker,
                expected_text: expected,
            });
        } else if let Some((marker, expected)) = parse_marker_and_array_join(&text[start..]) {
            // Handle ["line1", "line2"].join("\n") pattern
            results.push(VerifyCommand::QuickInfoAt {
                marker,
                expected_text: expected,
            });
        }
        search_from = start;
    }
    results
}

/// Parse `"marker", ["line1", "line2"].join("\n")` pattern.
pub(crate) fn parse_marker_and_array_join(text: &str) -> Option<(String, String)> {
    let first = extract_string_at(text, 0)?;
    // After first string + comma, look for `[`
    let after_first = &text[first.1..];
    let comma_pos = after_first.find(',')?;
    let after_comma = after_first[comma_pos + 1..].trim_start();
    if !after_comma.starts_with('[') {
        return None;
    }
    // Find matching `]`
    let bracket_content_start = 1;
    let mut depth = 1;
    let mut end = bracket_content_start;
    let bytes = after_comma.as_bytes();
    while end < bytes.len() && depth > 0 {
        match bytes[end] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            end += 1;
        }
    }
    if depth != 0 {
        return None;
    }
    let bracket_content = &after_comma[bracket_content_start..end];
    // Parse strings inside the array
    let mut lines = Vec::new();
    let mut pos = 0;
    loop {
        match extract_string_at(bracket_content, pos) {
            Some((s, next_pos)) => {
                lines.push(s);
                // Skip to next comma or end
                pos = next_pos;
                if let Some(cp) = bracket_content[pos..].find(',') {
                    pos += cp + 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some((first.0, lines.join("\n")))
}

/// Parse `verify.quickInfos({ key: "value", ... })`.
fn parse_quick_infos(text: &str) -> Option<VerifyCommand> {
    // Build a lookup table for const/var declarations (for variable references in values)
    let const_vars = resolve_const_vars(text);

    let pattern = "verify.quickInfos(";
    let pos = text.find(pattern)?;
    let start = pos + pattern.len();

    // Find the matching brace
    let rest = &text[start..];
    let brace_start = rest.find('{')?;
    let brace_content = &rest[brace_start + 1..];

    let mut entries = HashMap::new();
    // Parse key: "value" or key: `value` pairs
    let mut i = 0;
    let bytes = brace_content.as_bytes();
    while i < bytes.len() {
        // Skip whitespace
        while i < bytes.len()
            && (bytes[i] == b' '
                || bytes[i] == b'\n'
                || bytes[i] == b'\r'
                || bytes[i] == b'\t'
                || bytes[i] == b',')
        {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'}' {
            break;
        }

        // Skip line comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Parse key (could be number or string)
        let key_start = i;
        let key = if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let s = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let k = String::from_utf8_lossy(&bytes[s..i]).to_string();
            if i < bytes.len() {
                i += 1;
            } // skip closing quote
            k
        } else {
            while i < bytes.len() && bytes[i] != b':' && bytes[i] != b' ' {
                i += 1;
            }
            String::from_utf8_lossy(&bytes[key_start..i]).to_string()
        };

        // Skip : and whitespace
        while i < bytes.len() && (bytes[i] == b':' || bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }

        // Parse value (string, template literal, or array)
        if i >= bytes.len() {
            break;
        }
        let value = if bytes[i] == b'[' {
            // Array value: extract first string element as display text
            i += 1; // skip [
                    // Skip whitespace
            while i < bytes.len()
                && (bytes[i] == b' ' || bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t')
            {
                i += 1;
            }
            let val =
                if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'' || bytes[i] == b'`') {
                    let quote = bytes[i];
                    i += 1;
                    let mut s = String::new();
                    while i < bytes.len() && bytes[i] != quote {
                        if bytes[i] == b'\\' && i + 1 < bytes.len() {
                            match bytes[i + 1] {
                                b'n' => {
                                    s.push('\n');
                                    i += 2;
                                    continue;
                                }
                                b'\\' => {
                                    s.push('\\');
                                    i += 2;
                                    continue;
                                }
                                c if c == quote => {
                                    s.push(c as char);
                                    i += 2;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        s.push(bytes[i] as char);
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    } // skip closing quote
                    s
                } else {
                    String::new()
                };
            // Skip to end of array
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            val
        } else if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let mut val = String::new();
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    match bytes[i + 1] {
                        b'n' => {
                            val.push('\n');
                            i += 2;
                            continue;
                        }
                        b't' => {
                            val.push('\t');
                            i += 2;
                            continue;
                        }
                        b'\\' => {
                            val.push('\\');
                            i += 2;
                            continue;
                        }
                        c if c == quote => {
                            val.push(c as char);
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                }
                val.push(bytes[i] as char);
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            } // skip closing quote
            val
        } else if bytes[i] == b'`' {
            i += 1;
            let s = i;
            while i < bytes.len() && bytes[i] != b'`' {
                i += 1;
            }
            let val = String::from_utf8_lossy(&bytes[s..i]).to_string();
            if i < bytes.len() {
                i += 1;
            }
            val
        } else {
            // Bare identifier or expression — skip until comma or closing brace
            let s = i;
            while i < bytes.len() && bytes[i] != b',' && bytes[i] != b'}' {
                i += 1;
            }
            let raw = String::from_utf8_lossy(&bytes[s..i]).trim().to_string();
            // Try to resolve variable references
            const_vars.get(&raw).cloned().unwrap_or(raw)
        };

        if !key.is_empty() {
            entries.insert(key, value);
        }
    }

    if entries.is_empty() {
        None
    } else {
        Some(VerifyCommand::QuickInfos { entries })
    }
}

/// Build a lookup table of `const name = "value"` and `var name = "value"` declarations.
/// Used to resolve variable references in verify.quickInfos objects.
fn resolve_const_vars(text: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    // Match patterns like: const name = "value"; or var name = "value";
    for line in text.lines() {
        let trimmed = line.trim();
        let rest = if let Some(r) = trimmed.strip_prefix("const ") {
            r
        } else if let Some(r) = trimmed.strip_prefix("var ") {
            r
        } else if let Some(r) = trimmed.strip_prefix("let ") {
            r
        } else {
            continue;
        };
        // Find `name = "value"` or `name = 'value'` or `name = `value``
        if let Some(eq_pos) = rest.find('=') {
            let name_part = rest[..eq_pos].trim();
            // Skip declarations with type annotations (e.g., `const a: SomeType = ...`)
            let name = name_part.split(':').next().unwrap_or("").trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let val_part = rest[eq_pos + 1..].trim();
            let val_part = val_part.strip_suffix(';').unwrap_or(val_part).trim();
            // Extract string value
            if let Some(s) = extract_simple_string(val_part) {
                vars.insert(name.to_string(), s);
            }
        }
    }
    vars
}

/// Extract a simple string literal value (handles "...", '...', `...`).
pub(crate) fn extract_simple_string(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let q = bytes[0];
    if q != b'"' && q != b'\'' && q != b'`' {
        return None;
    }
    // Find matching close quote
    let end = s[1..].find(|c: char| c as u8 == q)?;
    let content = &s[1..1 + end];
    Some(unescape_js_string(content))
}

/// Parse two string arguments from a call like `("a", "b")`.
fn parse_two_string_args(text: &str) -> Option<(String, String)> {
    let first = extract_string_at(text, 0)?;
    let after_first = text[first.1..].find(',')?;
    let second_start = first.1 + after_first + 1;
    let second = extract_string_at(text, second_start)?;
    Some((first.0, second.0))
}

/// Extract a string literal starting at or after `start`.
pub(crate) fn extract_string_at(text: &str, start: usize) -> Option<(String, usize)> {
    let rest = &text[start..];
    let trimmed_offset = rest.len() - rest.trim_start().len();
    let trimmed = rest.trim_start();
    let quote = trimmed.as_bytes().first()?;
    if *quote != b'"' && *quote != b'\'' && *quote != b'`' {
        return None;
    }
    let q = *quote;
    let content_start = 1;
    let end = trimmed[content_start..].find(|c: char| c as u8 == q)?;
    let raw = &trimmed[content_start..content_start + end];
    let s = unescape_js_string(raw);
    Some((s, start + trimmed_offset + content_start + end + 1))
}

/// Interpret JS escape sequences in a string literal body.
pub(crate) fn unescape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('`') => out.push('`'),
                Some('0') => out.push('\0'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Extract string arguments from a function call like `func("a", "b")`.
fn extract_string_args(text: &str, func_name: &str) -> Vec<String> {
    let Some(pos) = text.find(func_name) else {
        return Vec::new();
    };
    let start = pos + func_name.len();
    let Some(paren) = text[start..].find('(') else {
        return Vec::new();
    };
    let args_start = start + paren + 1;
    let mut args = Vec::new();
    let mut offset = args_start;
    while offset < text.len() {
        if let Some((s, end)) = extract_string_at(text, offset) {
            args.push(s);
            offset = end;
        } else {
            break;
        }
        // Skip past comma
        let rest = &text[offset..];
        if let Some(comma) = rest.find(',') {
            offset += comma + 1;
        } else {
            break;
        }
    }
    args
}

/// Parse all `verify.completions(...)` calls from the verify section.
///
/// A single call may contain multiple object arguments separated by commas:
///   verify.completions({ marker: "1", includes: [...] }, { marker: "2", exact: [...] });
fn parse_completions_calls(text: &str) -> Vec<VerifyCommand> {
    let mut commands = Vec::new();
    let pattern = "verify.completions(";
    let mut search_from = 0;

    while let Some(pos) = text[search_from..].find(pattern) {
        // Make sure it's not verify.baselineCompletions
        let abs_pos = search_from + pos;
        if abs_pos > 0 && text.as_bytes().get(abs_pos - 1) == Some(&b'.') {
            // Already matched "verify.completions" so the char before "verify" would be something else
        }

        let args_start = abs_pos + pattern.len();
        // Find the matching closing paren, handling nested parens/brackets/braces
        let Some(args_end) = find_matching_close(text, args_start - 1, b'(', b')') else {
            search_from = args_start;
            continue;
        };

        let args_text = &text[args_start..args_end];

        // Split into individual object arguments at the top level
        let objects = split_top_level_objects(args_text);
        for obj in objects {
            commands.extend(parse_single_completions_obj(&obj));
        }

        search_from = args_end + 1;
    }

    commands
}

/// Find the position of the matching close delimiter starting from `open_pos`.
pub(crate) fn find_matching_close(
    text: &str,
    open_pos: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open_pos;
    let mut in_string: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];

        // Handle string literals
        if let Some(q) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }

        if b == b'"' || b == b'\'' || b == b'`' {
            in_string = Some(b);
            i += 1;
            continue;
        }

        // Handle line comments
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Split the arguments of `verify.completions(...)` into individual top-level
/// object literals (`{ ... }`). Handles nesting.
pub(crate) fn split_top_level_objects(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip whitespace and commas between objects
        while i < bytes.len()
            && (bytes[i] == b' '
                || bytes[i] == b'\n'
                || bytes[i] == b'\r'
                || bytes[i] == b'\t'
                || bytes[i] == b',')
        {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        if bytes[i] == b'{' {
            if let Some(end) = find_matching_close(text, i, b'{', b'}') {
                results.push(text[i..=end].to_string());
                i = end + 1;
            } else {
                break;
            }
        } else {
            // Skip non-object content (e.g. trailing paren, comments)
            i += 1;
        }
    }

    results
}

/// Parse a single completions object like `{ marker: "1", includes: ["foo"] }`.
/// Returns multiple commands when marker is an array (e.g. `marker: ["1", "2"]`).
pub(crate) fn parse_single_completions_obj(obj_text: &str) -> Vec<VerifyCommand> {
    // Extract marker(s). If no marker field, use unnamed marker ""
    let mut markers = extract_marker_list(obj_text);
    if markers.is_empty() {
        markers.push(String::new()); // Use unnamed marker
    }

    // Extract includes: can be a string, array of strings, or array of objects with `name`
    let mut includes = extract_name_list(obj_text, "includes");
    let excludes = extract_name_list(obj_text, "excludes");

    // Note: "unsorted" field contains keyword + member completions that we
    // don't generate keyword completions for. Skip parsing it since it would
    // cause more failures than passes.
    let exact = if obj_text.contains("exact:") || obj_text.contains("exact :") {
        // Check for `exact: undefined` which means "no completions expected"
        let Some(exact_pos) = obj_text.find("exact") else {
            return Vec::new();
        };
        let after_colon = &obj_text[exact_pos..];
        let Some(colon_pos) = after_colon.find(':') else {
            return Vec::new();
        };
        let value_start = after_colon[colon_pos + 1..].trim_start();
        if value_start.starts_with("undefined") {
            Some(Vec::new()) // exact: undefined means empty set
        } else if value_start.starts_with('[') {
            // Static array — parse the names. Arrays containing spreads
            // (`...completion.globals`) can't be evaluated statically:
            // downgrade the parseable names to `includes` semantics.
            let bracket_body = value_start
                .find(']')
                .map(|e| &value_start[..e])
                .unwrap_or(value_start);
            if bracket_body.contains("...") {
                for n in extract_name_list(obj_text, "exact") {
                    if !includes.contains(&n) {
                        includes.push(n);
                    }
                }
                None
            } else {
                let names = extract_name_list(obj_text, "exact");
                Some(names)
            }
        } else {
            // Dynamic expression (e.g. completion.globals, completion.globalsPlus(...))
            // We can't evaluate these statically — don't set exact
            None
        }
    } else {
        None
    };

    markers
        .into_iter()
        .map(|marker| VerifyCommand::Completions {
            marker,
            includes: includes.clone(),
            excludes: excludes.clone(),
            exact: exact.clone(),
        })
        .collect()
}

/// Extract marker value(s) from a completions object.
/// Handles both `marker: "1"` and `marker: ["1", "2"]`.
pub(crate) fn extract_marker_list(obj: &str) -> Vec<String> {
    let field_pattern = "marker:";
    let Some(pos) = obj.find(field_pattern) else {
        return Vec::new();
    };
    let after = &obj[pos + field_pattern.len()..];
    let trimmed = after.trim_start();

    // Handle `test.markers()` — means "use all markers in the test".
    // Signal this by returning a single empty string.
    if trimmed.starts_with("test.markers()") {
        return vec![String::new()];
    }

    if trimmed.starts_with('[') {
        // Array of markers
        extract_names_from_array(trimmed)
    } else if let Some((s, _)) = extract_string_at(after, 0) {
        vec![s]
    } else {
        Vec::new()
    }
}

/// Extract a string field value from an object literal: `field: "value"`.
pub(crate) fn extract_obj_string_field(obj: &str, field: &str) -> Option<String> {
    // Match field: "value" or field: 'value' or field: ["value1", "value2"] (take first)
    let field_pattern = format!("{}:", field);
    let pos = obj.find(&field_pattern)?;
    let after = &obj[pos + field_pattern.len()..];
    let trimmed = after.trim_start();

    // Handle array of strings for marker: ["1", "2"] — take first
    if trimmed.starts_with('[') {
        let (s, _) = extract_string_at(trimmed, 1)?;
        return Some(s);
    }

    let (s, _) = extract_string_at(after, 0)?;
    Some(s)
}

pub(crate) fn extract_obj_usize_field(obj: &str, field: &str) -> Option<usize> {
    let field_pattern = format!("{}:", field);
    let after = if let Some(pos) = obj.find(&field_pattern) {
        &obj[pos + field_pattern.len()..]
    } else {
        let alt = format!("{} :", field);
        let pos = obj.find(&alt)?;
        &obj[pos + alt.len()..]
    };
    let trimmed = after.trim_start();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

pub(crate) fn extract_obj_bool_field(obj: &str, field: &str) -> Option<bool> {
    let field_pattern = format!("{}:", field);
    let after = if let Some(pos) = obj.find(&field_pattern) {
        &obj[pos + field_pattern.len()..]
    } else {
        let alt = format!("{} :", field);
        let pos = obj.find(&alt)?;
        &obj[pos + alt.len()..]
    };
    let trimmed = after.trim_start();
    if trimmed.starts_with("true") {
        Some(true)
    } else if trimmed.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

pub(crate) fn extract_top_level_object_keys(obj: &str) -> Vec<String> {
    let bytes = obj.as_bytes();
    if bytes.first() != Some(&b'{') {
        return Vec::new();
    }

    let mut keys = Vec::new();
    let mut i = 1usize;
    let mut depth = 0i32;
    let mut in_string: Option<u8> = None;

    while i + 1 < bytes.len() {
        let b = bytes[i];

        if let Some(q) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }

        if b == b'"' || b == b'\'' || b == b'`' {
            in_string = Some(b);
            i += 1;
            continue;
        }

        match b {
            b'{' | b'[' | b'(' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' | b']' | b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                i += 1;
                continue;
            }
            _ => {}
        }

        if depth == 0 && (b.is_ascii_alphabetic() || b == b'_') {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let key = &obj[start..i];
            let mut lookahead = i;
            while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                lookahead += 1;
            }
            if lookahead < bytes.len() && bytes[lookahead] == b':' {
                keys.push(key.to_string());
            }
            continue;
        }

        i += 1;
    }

    keys
}

/// Extract a list of completion entry names from a field like:
///   includes: ["foo", "bar"]
///   includes: [{ name: "foo" }, { name: "bar" }]
///   includes: "singleName"
///   includes: { name: "foo" }
fn extract_name_list(obj: &str, field: &str) -> Vec<String> {
    let field_pattern = format!("{}:", field);
    let Some(pos) = obj.find(&field_pattern) else {
        // Try with space before colon
        let alt = format!("{} :", field);
        let Some(pos) = obj.find(&alt) else {
            return Vec::new();
        };
        return extract_name_list_from_value(&obj[pos + alt.len()..]);
    };
    extract_name_list_from_value(&obj[pos + field_pattern.len()..])
}

fn extract_name_list_from_value(after: &str) -> Vec<String> {
    let trimmed = after.trim_start();

    // Case 1: single string — includes: "foo"
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        if let Some((s, _)) = extract_string_at(trimmed, 0) {
            return vec![s];
        }
        return Vec::new();
    }

    // Case 2: array
    if trimmed.starts_with('[') {
        return extract_names_from_array(trimmed);
    }

    // Case 3: single object — includes: { name: "foo" }
    if trimmed.starts_with('{') {
        if let Some(name) = extract_obj_string_field(trimmed, "name") {
            return vec![name];
        }
    }

    Vec::new()
}

/// Extract names from an array that may contain strings or objects with `name` fields.
///   ["foo", "bar"]  or  [{ name: "foo" }, { name: "bar" }]
fn extract_names_from_array(arr_text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = arr_text.as_bytes();
    if bytes.is_empty() || bytes[0] != b'[' {
        return names;
    }

    let mut i = 1; // skip opening [
    let len = bytes.len();

    while i < len {
        // Skip whitespace and commas
        while i < len
            && (bytes[i] == b' '
                || bytes[i] == b'\n'
                || bytes[i] == b'\r'
                || bytes[i] == b'\t'
                || bytes[i] == b',')
        {
            i += 1;
        }
        if i >= len || bytes[i] == b']' {
            break;
        }

        // Skip line comments
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // String element
        if bytes[i] == b'"' || bytes[i] == b'\'' || bytes[i] == b'`' {
            if let Some((s, end)) = extract_string_at(arr_text, i) {
                names.push(s);
                i = end;
                continue;
            }
        }

        // Object element: { name: "foo", ... }
        if bytes[i] == b'{' {
            if let Some(close) = find_matching_close(arr_text, i, b'{', b'}') {
                let obj_text = &arr_text[i..=close];
                if let Some(name) = extract_obj_string_field(obj_text, "name") {
                    names.push(name);
                }
                i = close + 1;
                continue;
            }
        }

        // Skip anything else (e.g., `completion.globals`, spread operators, function calls)
        // These are dynamic references we can't evaluate statically — just skip them
        let mut depth = 0i32;
        let mut in_str: Option<u8> = None;
        while i < len {
            let b = bytes[i];
            if let Some(q) = in_str {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    in_str = None;
                }
                i += 1;
                continue;
            }
            if b == b'"' || b == b'\'' || b == b'`' {
                in_str = Some(b);
                i += 1;
                continue;
            }
            if b == b'(' || b == b'[' || b == b'{' {
                depth += 1;
            } else if b == b')' || b == b']' || b == b'}' {
                if depth == 0 {
                    break; // hit the outer ] or }
                }
                depth -= 1;
            } else if b == b',' && depth == 0 {
                break;
            }
            i += 1;
        }
    }

    names
}

/// Quick-check: does this test have a baseline file?
pub fn baseline_name_for_test(test_name: &str, op: LspOperation) -> Option<String> {
    match op {
        LspOperation::QuickInfo => Some(format!("{}.baseline", test_name)),
        LspOperation::GoToDefinition => Some(format!("{}.baseline.jsonc", test_name)),
        LspOperation::FindAllReferences => Some(format!("{}.baseline.jsonc", test_name)),
        LspOperation::Completions => Some(format!("{}.baseline", test_name)),
        LspOperation::Rename => Some(format!("{}.baseline.jsonc", test_name)),
        LspOperation::SignatureHelp => Some(format!("{}.baseline", test_name)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_markers() {
        let (content, markers) = extract_markers("var x/*1*/ = 5;", "/test.ts");
        assert_eq!(content, "var x = 5;");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].name, "1");
        assert_eq!(markers[0].position, 5);
    }

    #[test]
    fn test_extract_unnamed_marker() {
        let (content, markers) = extract_markers("var x/**/ = 5;", "/test.ts");
        assert_eq!(content, "var x = 5;");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].name, "");
        assert_eq!(markers[0].position, 5);
    }

    #[test]
    fn test_parse_simple_test() {
        let input = r#"/// <reference path='fourslash.ts' />

////var x/*1*/ = 5;

verify.quickInfoAt("1", "var x: number");
"#;
        let test = parse_fourslash("test", input).unwrap();
        assert_eq!(test.files.len(), 1);
        assert_eq!(test.markers.len(), 1);
        assert_eq!(test.markers[0].name, "1");
        assert!(matches!(
            &test.verify_commands[0],
            VerifyCommand::QuickInfoAt { marker, expected_text }
            if marker == "1" && expected_text == "var x: number"
        ));
    }

    #[test]
    fn test_parse_multi_file() {
        let input = r#"/// <reference path='fourslash.ts' />

// @Filename: /a.ts
////export const x = 0;

// @Filename: /b.ts
////import { x } from "./a";
////x/*b*/;

verify.baselineQuickInfo()
"#;
        let test = parse_fourslash("test", input).unwrap();
        assert_eq!(test.files.len(), 2);
        assert_eq!(test.files[0].name, "/a.ts");
        assert_eq!(test.files[1].name, "/b.ts");
        assert_eq!(test.markers.len(), 1);
        assert_eq!(test.markers[0].name, "b");
        assert_eq!(test.markers[0].file_name, "/b.ts");
    }

    #[test]
    fn test_parse_signature_help() {
        let input = r#"/// <reference path='fourslash.ts' />

////declare function f(a: number, b: string): void;
////f(/*1*/1, "x");

verify.signatureHelp({ marker: "1", text: "f(a: number, b: string): void", argumentCount: 0, parameterName: "a" });
"#;
        let test = parse_fourslash("sig", input).unwrap();
        assert!(matches!(
            &test.verify_commands[0],
            VerifyCommand::SignatureHelp { markers, expected }
            if markers == &vec!["1".to_string()]
                && expected.text.as_deref() == Some("f(a: number, b: string): void")
                && expected.argument_count == Some(0)
                && expected.parameter_name.as_deref() == Some("a")
        ));
    }

    #[test]
    fn test_parse_signature_help_with_docs() {
        let input = r#"/// <reference path='fourslash.ts' />

////declare function f(a: number, b: string): void;
////f(/*1*/1, "x");

verify.signatureHelp({ marker: "1", docComment: "docs", parameterDocComment: "param docs" });
"#;
        let test = parse_fourslash("sigdocs", input).unwrap();
        assert!(matches!(
            &test.verify_commands[0],
            VerifyCommand::SignatureHelp { markers, expected }
            if markers == &vec!["1".to_string()]
                && expected.doc_comment.as_deref() == Some("docs")
                && expected.parameter_doc_comment.as_deref() == Some("param docs")
        ));
    }

    #[test]
    fn test_parse_no_signature_help() {
        let input = r#"/// <reference path='fourslash.ts' />

////const x = 1/*1*/;

verify.noSignatureHelp("1");
"#;
        let test = parse_fourslash("nosig", input).unwrap();
        assert!(matches!(
            &test.verify_commands[0],
            VerifyCommand::NoSignatureHelp { markers }
            if markers == &vec!["1".to_string()]
        ));
    }
}
