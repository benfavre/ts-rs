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

/// Format a member with its containing symbol, stopping at a name visible in
/// the occurrence's lexical scope (for example `C.p` inside `N`, `N.C.p`
/// outside it). Members themselves always retain their owner qualification.
fn symbol_display_name(
    file: &SourceFile,
    table: &SymbolTable,
    owners: &HashMap<SymbolId, (SymbolId, bool)>,
    symbol: SymbolId,
    scope: ScopeId,
) -> String {
    let mut names = Vec::new();
    let mut current = symbol;
    let mut seen = std::collections::HashSet::new();
    while seen.insert(current) {
        let sym = &table.symbols[current as usize];
        let quoted_member = owners.get(&current).is_some_and(|(_, member)| *member)
            && sym.declarations.first().is_some_and(|decl| {
                file.text
                    .get(decl.span.start as usize..)
                    .is_some_and(|text| text.starts_with(['\'', '"']))
            });
        names.push(if quoted_member {
            format!("[{}]", quote_property_name(&sym.name))
        } else {
            sym.name.clone()
        });
        let Some(&(owner, is_member)) = owners.get(&current) else {
            break;
        };
        if !is_member {
            let mut lexical_scope = Some(scope);
            let mut visible = None;
            while let Some(scope_id) = lexical_scope {
                let Some(info) = table.scope_graph.scope(scope_id) else {
                    break;
                };
                if let Some(&binding) = info.bindings.get(&sym.name) {
                    if !owners.get(&binding).is_some_and(|(_, member)| *member) {
                        visible = Some(binding);
                        break;
                    }
                }
                lexical_scope = info.parent;
            }
            if visible == Some(current) {
                break;
            }
        }
        current = owner;
    }
    names.reverse();
    let mut result = String::new();
    for name in names {
        if !result.is_empty() && !name.starts_with('[') {
            result.push('.');
        }
        result.push_str(&name);
    }
    result
}

fn quote_property_name(name: &str) -> String {
    let mut result = String::from("\"");
    for ch in name.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{0008}' => result.push_str("\\b"),
            '\u{000c}' => result.push_str("\\f"),
            ch if ch < ' ' || matches!(ch, '\u{2028}' | '\u{2029}') => {
                result.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}

/// Generate the .symbols baseline text for a source file.
pub fn generate_symbols_baseline(file: &SourceFile, table: &SymbolTable) -> String {
    let mut out = String::new();
    out.push_str(&format!("//// [{}] ////\n", file.file_name));
    out.push_str(&format!(
        "\n=== {} ===\n",
        file.file_name.rsplit('/').next().unwrap_or(&file.file_name)
    ));

    let mut owners = HashMap::new();
    for symbol in &table.symbols {
        for &member in symbol.members.values() {
            owners.insert(member, (symbol.id, true));
        }
        for &export in symbol.exports.values() {
            owners.entry(export).or_insert((symbol.id, false));
        }
    }

    // Build a map from line number to list of (col, symbol_id) for symbols on that line
    let mut line_symbols: std::collections::BTreeMap<u32, Vec<(u32, u32, SymbolId)>> =
        std::collections::BTreeMap::new();
    for (&pos, &sym_id) in &table.position_to_symbol {
        let (line, col) = offset_to_line_col(&file.text, pos);
        line_symbols
            .entry(line)
            .or_default()
            .push((col, pos, sym_id));
    }

    // Sort symbols on each line by column
    for syms in line_symbols.values_mut() {
        syms.sort_by_key(|&(col, _, _)| col);
    }

    let mut annotations = Vec::new();
    for (line_idx, syms) in &line_symbols {
        for &(_col, pos, sym_id) in syms {
            if let Some(sym) = table.get_symbol(sym_id) {
                let scope = table
                    .scope_graph
                    .reference_at(pos)
                    .map(|reference| reference.scope_id)
                    .or_else(|| table.scope_graph.binding_scope(sym_id))
                    .unwrap_or(0);
                let display_name = symbol_display_name(file, table, &owners, sym_id, scope);
                let source_name = sym
                    .declarations
                    .iter()
                    .find_map(|decl| {
                        if decl.span.start != pos
                            || !owners.get(&sym_id).is_some_and(|(_, member)| *member)
                        {
                            return None;
                        }
                        file.text
                            .get(decl.span.start as usize..decl.span.end as usize)
                            .filter(|text| text.starts_with(['\'', '"']))
                    })
                    .unwrap_or(&sym.name);
                // Format each declaration reference
                let decl_strs: Vec<String> = sym
                    .declarations
                    .iter()
                    .take(5)
                    .map(|d| {
                        let (d_line, d_col) = offset_to_line_col(&file.text, d.full_start);
                        let short_name = d.file_name.rsplit('/').next().unwrap_or(&d.file_name);
                        format!("Decl({}, {}, {})", short_name, d_line, d_col)
                    })
                    .collect();
                let mut declarations = String::new();
                for declaration in decl_strs {
                    declarations.push_str(", ");
                    declarations.push_str(&declaration);
                }
                if sym.declarations.len() > 5 {
                    declarations.push_str(&format!(" ... and {} more", sym.declarations.len() - 5));
                }
                annotations.push((
                    *line_idx as usize,
                    format!(
                        ">{} : Symbol({}{})\n",
                        source_name, display_name, declarations
                    ),
                ));
            }
        }
    }

    out.push_str(&render_annotated_source(&file.text, annotations));
    out
}
