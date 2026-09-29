//! Cross-file symbol linking: import-to-export resolution.

use super::*;

/// Result of linking imports from one file to exports of another file.
#[derive(Debug, Clone, Default)]
pub struct LinkResult {
    /// Number of import specifiers successfully linked.
    pub linked: usize,
    /// Import names that could not be resolved in the target.
    pub unresolved: Vec<String>,
}

/// Link import symbols in `source_table` to export symbols in `target_table`.
///
/// For each import specifier in `import_decl`, this function looks up the
/// corresponding exported symbol in `target_table` and, if found, copies
/// its declaration info into the source symbol table so downstream passes
/// (type checking, emit) can see the cross-file type information.
pub fn link_imports(
    source_table: &mut SymbolTable,
    target_table: &SymbolTable,
    import_decl: &ImportDecl,
) -> LinkResult {
    let mut result = LinkResult::default();

    match &import_decl.specifiers {
        ImportClause::Named {
            default,
            named,
            namespace,
        } => {
            // Link default import
            if let Some(local_name) = default {
                if let Some(target_id) = find_exported(target_table, "default") {
                    link_symbol(source_table, local_name, target_table, target_id);
                    result.linked += 1;
                } else {
                    result.unresolved.push(local_name.clone());
                }
            }

            // Link named imports: `import { Foo, Bar as Baz } from './other'`
            for spec in named {
                if spec.is_type {
                    continue;
                }
                let remote_name = spec.imported.as_deref().unwrap_or(&spec.local);
                if let Some(target_id) = find_exported(target_table, remote_name) {
                    link_symbol(source_table, &spec.local, target_table, target_id);
                    result.linked += 1;
                } else {
                    result.unresolved.push(spec.local.clone());
                }
            }

            // Link namespace import: `import * as NS from './other'`
            if let Some(ns_name) = namespace {
                let ns_id = if let Some(existing) = find_symbol(source_table, ns_name) {
                    existing
                } else {
                    source_table.add_symbol(ns_name.clone(), SYM_MODULE)
                };
                for target_sym in &target_table.symbols {
                    if target_sym.flags & SYM_EXPORT != 0 {
                        if let Some(sym) = source_table.get_symbol_mut(ns_id) {
                            sym.exports.insert(target_sym.name.clone(), target_sym.id);
                        }
                    }
                }
                result.linked += 1;
            }
        }
        ImportClause::Require(local_name) => {
            if let Some(target_id) = find_exported(target_table, "default") {
                link_symbol(source_table, local_name, target_table, target_id);
                result.linked += 1;
            } else {
                result.unresolved.push(local_name.clone());
            }
        }
    }

    result
}

/// Find an exported symbol by name in the target table.
fn find_exported(table: &SymbolTable, name: &str) -> Option<SymbolId> {
    table
        .symbols
        .iter()
        .find(|s| s.flags & SYM_EXPORT != 0 && s.name == name)
        .map(|s| s.id)
}

/// Copy declaration information from a target symbol into the source table.
fn link_symbol(
    source_table: &mut SymbolTable,
    local_name: &str,
    target_table: &SymbolTable,
    target_id: SymbolId,
) {
    let target_sym = match target_table.get_symbol(target_id) {
        Some(s) => s.clone(),
        None => return,
    };

    if let Some(existing_id) = find_symbol(source_table, local_name) {
        if let Some(sym) = source_table.get_symbol_mut(existing_id) {
            sym.flags |= target_sym.flags;
            for decl in &target_sym.declarations {
                sym.declarations.push(decl.clone());
            }
            for (k, &v) in &target_sym.members {
                sym.members.insert(k.clone(), v);
            }
        }
    } else {
        let new_id = source_table.add_symbol(local_name.to_string(), target_sym.flags);
        if let Some(sym) = source_table.get_symbol_mut(new_id) {
            for decl in &target_sym.declarations {
                sym.declarations.push(decl.clone());
            }
            for (k, &v) in &target_sym.members {
                sym.members.insert(k.clone(), v);
            }
        }
    }
}
