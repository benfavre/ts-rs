//! Module and global augmentation merging.

use super::*;

/// Apply module augmentations from `augmenting_table` into `target_table`.
///
/// For each module augmentation in `augmenting_table` whose target path matches
/// a module in `target_table`, merge the augmentation's exported symbols into
/// the target module's members/exports.
pub fn apply_module_augmentations(
    target_table: &mut SymbolTable,
    augmenting_table: &SymbolTable,
) -> usize {
    let mut merged = 0usize;

    for (path, aug_sym_ids) in &augmenting_table.module_augmentations {
        // Find the target module symbol by name in target_table
        let target_mod_id = match find_symbol(target_table, path) {
            Some(id) => id,
            None => continue,
        };

        for &aug_sym_id in aug_sym_ids {
            let aug_sym = match augmenting_table.get_symbol(aug_sym_id) {
                Some(s) => s.clone(),
                None => continue,
            };

            // Merge exports from the augmentation into the target module
            for (name, &exp_id) in &aug_sym.exports {
                if let Some(exp_sym) = augmenting_table.get_symbol(exp_id) {
                    let new_id = target_table.add_symbol(exp_sym.name.clone(), exp_sym.flags);
                    if let Some(sym) = target_table.get_symbol_mut(new_id) {
                        sym.declarations = exp_sym.declarations.clone();
                        sym.members = exp_sym.members.clone();
                    }
                    if let Some(target_mod) = target_table.get_symbol_mut(target_mod_id) {
                        target_mod.exports.insert(name.clone(), new_id);
                        target_mod.members.insert(name.clone(), new_id);
                    }
                    merged += 1;
                }
            }
        }
    }

    merged
}

/// Apply global augmentations from `augmenting_table` into `target_table`.
///
/// Copies symbols that were declared inside `declare global { ... }` blocks
/// into the target table.
pub fn apply_global_augmentations(
    target_table: &mut SymbolTable,
    augmenting_table: &SymbolTable,
) -> usize {
    let mut merged = 0usize;

    for &sym_id in &augmenting_table.global_augmentation_symbols {
        if let Some(src_sym) = augmenting_table.get_symbol(sym_id) {
            let name = src_sym.name.clone();
            // Check if the target already has this symbol
            if let Some(existing_id) = find_symbol(target_table, &name) {
                // Merge into existing symbol
                if let Some(existing) = target_table.get_symbol_mut(existing_id) {
                    existing.flags |= src_sym.flags;
                    for decl in &src_sym.declarations {
                        existing.declarations.push(decl.clone());
                    }
                    for (k, &v) in &src_sym.members {
                        existing.members.insert(k.clone(), v);
                    }
                }
            } else {
                // Create a new symbol in the target table
                let new_id = target_table.add_symbol(name, src_sym.flags);
                if let Some(sym) = target_table.get_symbol_mut(new_id) {
                    sym.declarations = src_sym.declarations.clone();
                    sym.members = src_sym.members.clone();
                }
            }
            merged += 1;
        }
    }

    merged
}

/// Resolve a module specifier against ambient module declarations, including
/// wildcard patterns. Returns the symbol ID if found.
pub fn resolve_ambient_module(table: &SymbolTable, specifier: &str) -> Option<SymbolId> {
    // Try exact match first
    if let Some(&sym_id) = table.ambient_modules.get(specifier) {
        return Some(sym_id);
    }

    // Try wildcard patterns
    for (pattern, sym_id) in &table.wildcard_modules {
        let sym_id = *sym_id;
        if wildcard_matches(pattern, specifier) {
            return Some(sym_id);
        }
    }

    None
}

/// Check if a wildcard pattern matches a module specifier.
/// The pattern uses `*` as a wildcard that matches any string.
pub(crate) fn wildcard_matches(pattern: &str, specifier: &str) -> bool {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        specifier.starts_with(prefix)
            && specifier.ends_with(suffix)
            && specifier.len() >= prefix.len() + suffix.len()
    } else {
        pattern == specifier
    }
}
