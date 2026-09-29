//! Symbol baseline output generation for test harness.

use super::*;

/// Compute (line, col) from a byte offset in the source text.
fn offset_to_line_col(text: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
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
    let mut line_symbols: HashMap<u32, Vec<(u32, SymbolId)>> = HashMap::new();
    for (&pos, &sym_id) in &table.position_to_symbol {
        let (line, col) = offset_to_line_col(&file.text, pos);
        line_symbols.entry(line).or_default().push((col, sym_id));
    }

    // Sort symbols on each line by column
    for syms in line_symbols.values_mut() {
        syms.sort_by_key(|&(col, _)| col);
    }

    for (line_idx, line) in file.text.lines().enumerate() {
        out.push_str(line);
        out.push('\n');

        if let Some(syms) = line_symbols.get(&(line_idx as u32)) {
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
                    out.push_str(&format!(
                        ">{} : Symbol({}, {})\n",
                        sym.name,
                        sym.name,
                        decl_strs.join(", ")
                    ));
                }
            }
        }
    }

    // TypeScript symbol baselines separate source-file sections with a blank line.
    out.push('\n');
    out
}
