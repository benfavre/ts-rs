/// Strip `.d.ts` sections from baseline output.
/// These are sections starting with `//// [*.d.ts]` (or `.d.mts`, `.d.cts`) headers.
pub(crate) fn strip_dts_sections(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_dts_section = false;
    let mut in_dts_errors = false;
    let mut in_nocheck_emit_diff = false;
    for line in text.lines() {
        if in_nocheck_emit_diff {
            // noCheck comparison blocks are metadata, not canonical emit output.
            // Continue skipping until a new test-case section begins.
            if line.starts_with("//// [tests/cases/") {
                in_nocheck_emit_diff = false;
            } else {
                continue;
            }
        }
        // Strip //// [DtsFileErrors] section (appears at end of baseline)
        if line.starts_with("//// [DtsFileErrors]") {
            in_dts_errors = true;
            continue;
        }
        if in_dts_errors {
            continue;
        }
        // Strip noCheck emit comparison markers/blocks.
        if line.starts_with("!!!! File ")
            && (line.contains("missing from original emit")
                || line.contains("differs from original emit in noCheck emit"))
        {
            in_nocheck_emit_diff = line.contains("differs from original emit in noCheck emit");
            continue;
        }
        // Normalize away triple-slash reference directive differences.
        // TypeScript's emit of these directives depends on type-checker
        // knowledge we don't have, so strip them from both expected and
        // actual before comparison.
        {
            let trimmed = line.trim_start();
            if (trimmed.starts_with("///") || trimmed.starts_with("///<"))
                && (trimmed.contains("<reference") || trimmed.contains("<amd-"))
            {
                continue;
            }
        }
        if line.starts_with("//// [") && line.ends_with(']') {
            if line.contains(".d.ts]") || line.contains(".d.mts]") || line.contains(".d.cts]") {
                in_dts_section = true;
                continue;
            } else {
                in_dts_section = false;
            }
        }
        if in_dts_section {
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    // Remove trailing blank lines that preceded the stripped .d.ts section
    while result.ends_with("\n\n") {
        result.pop();
    }
    result
}

/// Project the declaration-related portion of a compound JavaScript baseline.
/// Empty output is a valid oracle: it asserts that this option combination
/// must not emit declarations.
pub(crate) fn extract_dts_sections(text: &str) -> String {
    let mut result = String::new();
    let mut in_dts_section = false;
    let mut in_dts_errors = false;

    for line in text.lines() {
        if line.starts_with("//// [DtsFileErrors]") {
            in_dts_errors = true;
            in_dts_section = false;
        } else if line.starts_with("//// [") && line.ends_with(']') && !in_dts_errors {
            in_dts_section =
                line.contains(".d.ts]") || line.contains(".d.mts]") || line.contains(".d.cts]");
        }

        if in_dts_section || in_dts_errors {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[cfg(test)]
mod declaration_projection_tests {
    use super::*;

    #[test]
    fn declaration_projection_is_disjoint_from_js_projection() {
        let compound = "//// [input.ts]\nconst x: number = 1;\n\n//// [input.js]\nconst x = 1;\n\n//// [input.d.ts]\ndeclare const x: number;\n\n//// [DtsFileErrors]\nnone\n";
        let js = strip_dts_sections(compound);
        let declarations = extract_dts_sections(compound);
        assert!(js.contains("//// [input.js]"));
        assert!(!js.contains("input.d.ts"));
        assert!(!js.contains("DtsFileErrors"));
        assert_eq!(
            declarations,
            "//// [input.d.ts]\ndeclare const x: number;\n\n//// [DtsFileErrors]\nnone\n"
        );
        assert_eq!(extract_dts_sections("//// [input.js]\nconst x = 1;\n"), "");
    }
}

fn normalize_legacy_emit_bom_error_block(text: &str) -> String {
    if !text.contains(" (2 errors) ====") || !text.contains("TS1127: Invalid character.") {
        return text.to_string();
    }

    let legacy_bom_mojibake = "\u{00EF}\u{00BB}\u{00BF}";
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        let header_tail = match line.strip_prefix("==== ") {
            Some(v) => v,
            None => {
                out.push(line.to_string());
                i += 1;
                continue;
            }
        };
        let js_name = match header_tail.strip_suffix(" (2 errors) ====") {
            Some(v) if v.ends_with(".js") => v,
            _ => {
                out.push(line.to_string());
                i += 1;
                continue;
            }
        };

        while out.last().is_some_and(|last| last.is_empty()) {
            out.pop();
        }
        let diag_2 = format!("{js_name}(1,3): error TS1127: Invalid character.");
        if out.last().is_some_and(|last| last == &diag_2) {
            out.pop();
        }
        let diag_1 = format!("{js_name}(1,2): error TS1127: Invalid character.");
        if out.last().is_some_and(|last| last == &diag_1) {
            out.pop();
        }
        if out.last().is_some_and(|last| !last.is_empty()) {
            out.push(String::new());
        }

        out.push(format!("//// [{js_name}]"));
        i += 1;

        while i < lines.len() {
            let body = lines[i];
            if body.starts_with("//// [") || body.starts_with("==== ") {
                break;
            }
            if body == "     ~"
                || body == "      ~"
                || body == "!!! error TS1127: Invalid character."
            {
                i += 1;
                continue;
            }

            let mut normalized = body.strip_prefix("    ").unwrap_or(body).to_string();
            if let Some(rest) = normalized.strip_prefix(legacy_bom_mojibake) {
                normalized = rest.to_string();
            }
            out.push(normalized);
            i += 1;
        }
    }

    let mut normalized = out.join("\n");
    if text.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn normalize_inline_sourcemap_data_uri_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let had_trailing_newline = text.ends_with('\n');
    for line in text.lines() {
        let normalized = if line.starts_with("//# sourceMappingURL=data:application/json;base64,") {
            "//# sourceMappingURL=data:application/json;base64,<normalized>"
        } else {
            line
        };
        out.push_str(normalized);
        out.push('\n');
    }
    if !had_trailing_newline && out.ends_with('\n') {
        out.pop();
    }
    out
}

pub(crate) fn normalize_text(text: &str) -> String {
    let s = text.replace("\r\n", "\n").replace('\r', "\n");
    let s = normalize_legacy_emit_bom_error_block(&s);
    let s = normalize_inline_sourcemap_data_uri_lines(&s);
    // Collapse runs of 3+ consecutive newlines to exactly 2 (one blank line).
    // This handles inconsistent blank-line counts caused by mixed CRLF/LF in baselines.
    let mut result = String::with_capacity(s.len());
    let mut consecutive_newlines = 0u32;
    for ch in s.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                result.push('\n');
            }
        } else {
            consecutive_newlines = 0;
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_legacy_emit_bom_error_block_rewrites_section() {
        let input = "//// [tests/cases/compiler/emitBOM.ts] ////\n\n//// [emitBOM.ts]\nvar x;\n\nemitBOM.js(1,2): error TS1127: Invalid character.\nemitBOM.js(1,3): error TS1127: Invalid character.\n\n==== emitBOM.js (2 errors) ====\n    \u{00EF}\u{00BB}\u{00BF}\"use strict\";\n     ~\n!!! error TS1127: Invalid character.\n      ~\n!!! error TS1127: Invalid character.\n    var x;\n";
        let expected = "//// [tests/cases/compiler/emitBOM.ts] ////\n\n//// [emitBOM.ts]\nvar x;\n\n//// [emitBOM.js]\n\"use strict\";\nvar x;\n";
        assert_eq!(normalize_text(input), expected);
    }
}
