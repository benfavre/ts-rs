//! Symbol baseline output generation for test harness.

use super::*;

/// Compute (line, col) from a byte offset in the source text.
fn offset_to_line_col(text: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let mut line = 0u32;
    let mut col = 0u32;
    let mut after_cr = false;
    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if matches!(ch, '\r' | '\n' | '\u{2028}' | '\u{2029}') {
            if ch != '\n' || !after_cr {
                line += 1;
            }
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
        after_cr = ch == '\r';
    }
    (line, col)
}

/// Interleave source lines and ordered annotations using the upstream
/// type/symbol baseline layout. An annotation group gets a separating blank
/// line unless the next source line already separates it (blank or a brace).
pub fn render_annotated_source(
    source: &str,
    annotations: impl IntoIterator<Item = (usize, String)>,
) -> String {
    let normalized = source.replace("\r\n", "\n");
    let lines: Vec<_> = normalized
        .split(['\n', '\r', '\u{2028}', '\u{2029}'])
        .collect();
    let mut output = String::new();
    let mut last_line = None;
    let separates = |line: &str| matches!(line.trim(), "" | "{" | "}" | "|");
    for (line, annotation) in annotations {
        assert!(line < lines.len(), "annotation line is outside its source");
        assert!(
            last_line.is_none_or(|previous| line >= previous),
            "annotations must follow source order"
        );
        if last_line != Some(line) {
            let start = last_line.map_or(0, |previous| previous + 1);
            if last_line.is_some() && lines.get(start).is_some_and(|line| !separates(line)) {
                output.push('\n');
            }
            if start <= line {
                output.push_str(&lines[start..=line].join("\n"));
            }
            output.push('\n');
        }
        output.push_str(&annotation);
        last_line = Some(line);
    }
    let start = last_line.map_or(0, |previous| previous + 1);
    if start < lines.len() {
        if !separates(lines[start]) {
            output.push('\n');
        }
        output.push_str(&lines[start..].join("\n"));
    }
    output.push('\n');
    output
}

/// Generate the .symbols baseline text for a source file.
pub fn generate_symbols_baseline(file: &SourceFile, table: &SymbolTable) -> String {
    let mut out = String::new();
    out.push_str(&format!("//// [{}] ////\n", file.file_name));
    out.push_str(&format!(
        "\n=== {} ===\n",
        file.file_name.rsplit('/').next().unwrap_or(&file.file_name)
    ));

    // Build a map from line number to list of (col, symbol_id) for symbols on that line
    let mut line_symbols: std::collections::BTreeMap<u32, Vec<(u32, SymbolId)>> =
        std::collections::BTreeMap::new();
    for (&pos, &sym_id) in &table.position_to_symbol {
        let (line, col) = offset_to_line_col(&file.text, pos);
        line_symbols.entry(line).or_default().push((col, sym_id));
    }

    // Sort symbols on each line by column
    for syms in line_symbols.values_mut() {
        syms.sort_by_key(|&(col, _)| col);
    }

    let mut annotations = Vec::new();
    for (line_idx, syms) in &line_symbols {
        for &(_col, sym_id) in syms {
            if let Some(sym) = table.get_symbol(sym_id) {
                // Format each declaration reference
                let decl_strs: Vec<String> = sym
                    .declarations
                    .iter()
                    .map(|d| {
                        let (d_line, d_col) = offset_to_line_col(&file.text, d.full_start);
                        let short_name = d.file_name.rsplit('/').next().unwrap_or(&d.file_name);
                        format!("Decl({}, {}, {})", short_name, d_line, d_col)
                    })
                    .collect();
                annotations.push((
                    *line_idx as usize,
                    format!(
                        ">{} : Symbol({}, {})\n",
                        sym.name,
                        sym.name,
                        decl_strs.join(", ")
                    ),
                ));
            }
        }
    }

    out.push_str(&render_annotated_source(&file.text, annotations));
    out
}
