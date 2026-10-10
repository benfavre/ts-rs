//! tsc's `checkImportAttributes` for import and re-export declarations:
//! attributes (`with { … }` / `assert { … }`) need a module kind that
//! preserves them (TS2823), and cannot sit on a type-only declaration
//! (TS2857).

use tsc_rs_ast::*;

use crate::TypeChecker;

impl TypeChecker {
    pub(crate) fn check_import_attributes(&mut self, stmt: &Stmt) {
        let (specifier_end, type_only) = match &stmt.kind {
            StmtKind::Import(import) => (Some(import.source_span.end), import.type_only),
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Named {
                    source: Some(_),
                    type_only,
                    ..
                } => (None, *type_only),
                ExportDeclKind::All { type_only, .. } => (None, *type_only),
                _ => return,
            },
            _ => return,
        };
        let Some(module) = self.compiler_options.module else {
            return;
        };
        let Some(source) = self.current_source.as_deref() else {
            return;
        };
        let Some(span) = attributes_span(source, stmt.span, specifier_end) else {
            return;
        };
        // A type-only declaration may pick its `resolution-mode`.
        if type_only && source[span.start as usize..span.end as usize].contains("resolution-mode") {
            return;
        }
        let supported = matches!(
            module,
            ModuleKind::ESNext
                | ModuleKind::Node18
                | ModuleKind::Node20
                | ModuleKind::NodeNext
                | ModuleKind::Preserve
        );
        let (code, message) = if !supported {
            (
                2823,
                "Import attributes are only supported when the '--module' option is set to 'esnext', 'node18', 'node20', 'nodenext', or 'preserve'.",
            )
        } else if type_only {
            (
                2857,
                "Import attributes cannot be used with type-only imports or exports.",
            )
        } else {
            return;
        };
        self.diagnostics.push(Diagnostic {
            code,
            message: message.to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }
}

/// The `with { … }` / `assert { … }` clause after a declaration's module
/// specifier. Re-exports carry no specifier span, so theirs is found after
/// the `from` keyword.
fn attributes_span(source: &str, stmt: Span, specifier_end: Option<u32>) -> Option<Span> {
    let text = source.get(stmt.start as usize..stmt.end as usize)?;
    let after_specifier = match specifier_end {
        Some(end) => end.checked_sub(stmt.start)? as usize,
        None => {
            let bytes = text.as_bytes();
            let mut from = None;
            let mut search = 0;
            while let Some(found) = text[search..].find("from") {
                let at = search + found;
                let rest = text[at + 4..].trim_start();
                if rest.starts_with('"') || rest.starts_with('\'') {
                    from = Some(text.len() - rest.len());
                    break;
                }
                search = at + 4;
            }
            let open = from?;
            let quote = bytes[open];
            let close = text[open + 1..].find(quote as char)? + open + 1;
            close + 1
        }
    };
    let rest = text.get(after_specifier..)?;
    let keyword_at = after_specifier + (rest.len() - rest.trim_start().len());
    let rest = &text[keyword_at..];
    let keyword_len = if rest.starts_with("with") {
        4
    } else if rest.starts_with("assert") {
        6
    } else {
        return None;
    };
    let after_keyword = &rest[keyword_len..];
    if after_keyword
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let open = keyword_at + keyword_len + after_keyword.find('{')?;
    let mut depth = 0usize;
    for (offset, byte) in text.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let end = open + offset + 1;
                    return Some(Span::new(
                        stmt.start + keyword_at as u32,
                        stmt.start + end as u32,
                    ));
                }
            }
            _ => {}
        }
    }
    None
}

impl TypeChecker {
    /// TS18043: a JavaScript `export { T }` naming only a type (an
    /// interface or type alias, with no value of that name).
    pub(crate) fn check_js_type_only_export(&mut self, stmt: &Stmt) {
        if !self.current_file_is_js() {
            return;
        }
        let StmtKind::Export(export) = &stmt.kind else {
            return;
        };
        let ExportDeclKind::Named {
            specifiers,
            source: None,
            type_only: false,
        } = &export.kind
        else {
            return;
        };
        for specifier in specifiers {
            let name = specifier.local.as_str();
            let names_type =
                self.interface_info.contains_key(name) || self.type_aliases.contains_key(name);
            if names_type && self.lookup_var(name).is_none() && !specifier.is_type {
                self.diagnostics.push(Diagnostic {
                    code: 18043,
                    message: "Types cannot appear in export declarations in JavaScript files."
                        .to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    // tsc marks the local name, not `local as exported`.
                    span: Some(Span::new(
                        specifier.span.start,
                        specifier.span.start + name.len() as u32,
                    )),
                    related: None,
                });
            }
        }
    }
}

impl TypeChecker {
    /// TS2206/TS2207: a `type` modifier on a specifier of a declaration that
    /// is already `import type` / `export type`.
    pub(crate) fn check_redundant_type_modifiers(&mut self, stmt: &Stmt) {
        // grammarErrorOnNode: silent in a file with parse errors.
        if !self.check_index_grammar {
            return;
        }
        let spans: Vec<Span> = match &stmt.kind {
            StmtKind::Import(import) if import.type_only => match &import.specifiers {
                ImportClause::Named { named, .. } => named
                    .iter()
                    .filter(|specifier| specifier.is_type)
                    .map(|specifier| specifier.span)
                    .collect(),
                _ => Vec::new(),
            },
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Named {
                    specifiers,
                    type_only: true,
                    ..
                } => specifiers
                    .iter()
                    .filter(|specifier| specifier.is_type)
                    .map(|specifier| specifier.span)
                    .collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        let (code, kind) = if matches!(stmt.kind, StmtKind::Import(_)) {
            (2206, "import")
        } else {
            (2207, "export")
        };
        for span in spans {
            // tsc marks the `type` keyword.
            let span = Span::new(span.start, span.start + 4);
            self.diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "The 'type' modifier cannot be used on a named {kind} when '{kind} type' is used on its {kind} statement."
                ),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }
    }
}
