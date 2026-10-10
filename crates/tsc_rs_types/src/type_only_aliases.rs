//! tsc's type-only alias tracking (`getTypeOnlyAliasDeclaration`): an
//! import that reaches its target through `import type`, `export type` or
//! `export type *` has no value meaning. A value use of it is TS1361
//! (imported using 'import type') or TS1362 (exported using 'export type'),
//! with a note at the first type-only declaration on the way from the use.

use tsc_rs_ast::*;

use crate::TypeChecker;

/// Where a type-only alias chain turns type-only.
#[derive(Debug, Clone)]
pub(crate) struct TypeOnlyOrigin {
    /// 1361 (`import type`) or 1362 (`export type`).
    pub code: u32,
    pub file: String,
    pub span: Span,
}

/// What an import or export resolves to next.
#[derive(Debug, Clone)]
enum Target {
    /// A declaration of this file; whether it has a value meaning.
    Local(bool),
    /// Module `source`'s export `imported` (`*`: the module itself).
    Module { source: String, imported: String },
}

#[derive(Debug, Clone)]
enum ExportEntry {
    /// Resolves on without turning type-only here.
    Plain(Target),
    /// Type-only right here.
    TypeOnly(TypeOnlyOrigin, Target),
}

/// One file's exports, as far as type-only-ness goes.
#[derive(Debug, Clone, Default)]
pub(crate) struct TypeOnlyExports {
    named: rustc_hash::FxHashMap<String, ExportEntry>,
    /// `export * from "m"`: the specifier, and the statement when it is
    /// `export type *`.
    stars: Vec<(String, Option<TypeOnlyOrigin>)>,
}

/// The position of `name` as a whole word in `text[from..]`.
fn find_word(text: &str, from: usize, name: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80;
    let mut search = from;
    while let Some(found) = text.get(search..)?.find(name) {
        let at = search + found;
        let end = at + name.len();
        let before_ok = at == 0 || !is_word(bytes[at - 1]);
        let after_ok = end >= bytes.len() || !is_word(bytes[end]);
        if before_ok && after_ok {
            return Some(at);
        }
        search = end;
    }
    None
}

/// The span of a binding name written somewhere in a statement.
fn name_span_in(text: &str, statement: Span, skip: usize, name: &str) -> Span {
    let start = statement.start as usize;
    find_word(text, start + skip, name)
        .filter(|at| *at < statement.end as usize)
        .map_or(statement, |at| {
            Span::new(at as u32, (at + name.len()) as u32)
        })
}

/// An import binding: where it resolves, and whether it is `import type`.
struct ImportBinding {
    type_only: Option<TypeOnlyOrigin>,
    source: String,
    imported: String,
}

fn collect_import_bindings(file: &SourceFile) -> rustc_hash::FxHashMap<String, ImportBinding> {
    let mut bindings = rustc_hash::FxHashMap::default();
    for statement in &file.statements {
        let StmtKind::Import(import) = &statement.kind else {
            continue;
        };
        let ImportClause::Named {
            default,
            named,
            namespace,
        } = &import.specifiers
        else {
            continue;
        };
        let origin = |span: Span| TypeOnlyOrigin {
            code: 1361,
            file: file.file_name.clone(),
            span,
        };
        let binding = |type_only: Option<TypeOnlyOrigin>, imported: &str| ImportBinding {
            type_only,
            source: import.source.clone(),
            imported: imported.to_string(),
        };
        // "import type " — the bindings follow the keywords.
        let skip = if import.type_only { 12 } else { 7 };
        if let Some(name) = default {
            // The import clause starts at `type`.
            let type_only = import
                .type_only
                .then(|| origin(name_span_in(&file.text, statement.span, 6, "type")));
            bindings.insert(name.clone(), binding(type_only, "default"));
        }
        if let Some(name) = namespace {
            let type_only = import
                .type_only
                .then(|| origin(name_span_in(&file.text, statement.span, skip, name)));
            bindings.insert(name.clone(), binding(type_only, "*"));
        }
        for specifier in named {
            let type_only = (import.type_only || specifier.is_type).then(|| origin(specifier.span));
            let imported = specifier.imported.as_deref().unwrap_or(&specifier.local);
            bindings.insert(specifier.local.clone(), binding(type_only, imported));
        }
    }
    bindings
}

/// Whether a namespace body declares any value (an instantiated namespace).
fn namespace_has_value(body: Option<&ModuleBody>) -> bool {
    match body {
        Some(ModuleBody::Block(statements)) => statements.iter().any(|statement| {
            let inner = match &statement.kind {
                StmtKind::Export(export) => match &export.kind {
                    ExportDeclKind::Decl(inner) => inner.as_ref(),
                    _ => return true,
                },
                _ => statement,
            };
            match &inner.kind {
                StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => false,
                StmtKind::ModuleDecl(module) => namespace_has_value(module.body.as_ref()),
                _ => true,
            }
        }),
        Some(ModuleBody::Module(inner)) => namespace_has_value(inner.body.as_ref()),
        None => false,
    }
}

/// Top-level declared names of a statement, with whether each is a value.
fn declared_names(statement: &Stmt) -> Vec<(String, bool)> {
    match &statement.kind {
        StmtKind::ClassDecl(class) => class.name.iter().map(|n| (n.to_string(), true)).collect(),
        StmtKind::FnDecl(function) => function
            .name
            .iter()
            .map(|n| (n.to_string(), true))
            .collect(),
        StmtKind::EnumDecl(enum_decl) => vec![(enum_decl.name.to_string(), true)],
        StmtKind::Var(var) => var
            .declarations
            .iter()
            .filter_map(|d| match &d.name.kind {
                PatKind::Ident(name) => Some((name.to_string(), true)),
                _ => None,
            })
            .collect(),
        StmtKind::ModuleDecl(module) => match &module.name {
            ModuleName::Ident(name) => {
                vec![(name.to_string(), namespace_has_value(module.body.as_ref()))]
            }
            _ => Vec::new(),
        },
        StmtKind::ImportEquals(import) => vec![(import.name.to_string(), true)],
        StmtKind::InterfaceDecl(interface) => vec![(interface.name.to_string(), false)],
        StmtKind::TypeAlias(alias) => vec![(alias.name.to_string(), false)],
        StmtKind::Export(export) => match &export.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                declared_names(inner)
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn collect_exports(file: &SourceFile) -> TypeOnlyExports {
    let imports = collect_import_bindings(file);
    // Local declarations: any value declaration of a name gives it a value.
    let mut locals: rustc_hash::FxHashMap<String, bool> = rustc_hash::FxHashMap::default();
    for statement in &file.statements {
        for (name, value) in declared_names(statement) {
            *locals.entry(name).or_insert(false) |= value;
        }
    }
    let local_target = |name: &str| -> (Option<TypeOnlyOrigin>, Target) {
        match imports.get(name) {
            Some(binding) => (
                binding.type_only.clone(),
                Target::Module {
                    source: binding.source.clone(),
                    imported: binding.imported.clone(),
                },
            ),
            None => (
                None,
                Target::Local(locals.get(name).copied().unwrap_or(true)),
            ),
        }
    };
    let mut exports = TypeOnlyExports::default();
    let export_origin = |span: Span| TypeOnlyOrigin {
        code: 1362,
        file: file.file_name.clone(),
        span,
    };
    for statement in &file.statements {
        let StmtKind::Export(export) = &statement.kind else {
            continue;
        };
        match &export.kind {
            ExportDeclKind::Named {
                specifiers,
                source,
                type_only,
            } => {
                for specifier in specifiers {
                    let exported = specifier
                        .exported
                        .clone()
                        .unwrap_or_else(|| specifier.local.clone());
                    let (imported_type_only, target) = match source {
                        Some(source) => (
                            None,
                            Target::Module {
                                source: source.clone(),
                                imported: specifier.local.clone(),
                            },
                        ),
                        None => local_target(&specifier.local),
                    };
                    let entry = if *type_only || specifier.is_type {
                        ExportEntry::TypeOnly(export_origin(specifier.span), target)
                    } else if let Some(origin) = imported_type_only {
                        ExportEntry::TypeOnly(origin, target)
                    } else {
                        ExportEntry::Plain(target)
                    };
                    exports.named.insert(exported, entry);
                }
            }
            ExportDeclKind::All {
                source,
                alias,
                type_only,
                ..
            } => match alias {
                Some(alias) => {
                    let target = Target::Module {
                        source: source.clone(),
                        imported: "*".to_string(),
                    };
                    let entry = if *type_only {
                        // `export type * as ns` — the note marks the `*`.
                        let span = name_span_in(&file.text, statement.span, 6, "*");
                        ExportEntry::TypeOnly(export_origin(span), target)
                    } else {
                        ExportEntry::Plain(target)
                    };
                    exports.named.insert(alias.clone(), entry);
                }
                None => {
                    let origin = type_only.then(|| export_origin(statement.span));
                    exports.stars.push((source.clone(), origin));
                }
            },
            ExportDeclKind::Default(expression) => {
                let target = match &expression.kind {
                    ExprKind::Ident(name) => local_target(name).1,
                    _ => Target::Local(true),
                };
                exports
                    .named
                    .insert("default".to_string(), ExportEntry::Plain(target));
            }
            ExportDeclKind::DefaultDecl(declaration) => {
                let value = declared_names(declaration).iter().any(|(_, value)| *value)
                    || !matches!(
                        declaration.kind,
                        StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_)
                    );
                exports.named.insert(
                    "default".to_string(),
                    ExportEntry::Plain(Target::Local(value)),
                );
            }
            ExportDeclKind::Decl(declaration) => {
                for (name, _) in declared_names(declaration) {
                    let value = locals.get(&name).copied().unwrap_or(true);
                    exports
                        .named
                        .insert(name, ExportEntry::Plain(Target::Local(value)));
                }
            }
        }
    }
    exports
}

impl TypeChecker {
    /// See `TypeOnlyExports`: every program file's table, by normalized
    /// file name.
    pub(crate) fn collect_type_only_exports(
        files: &[&SourceFile],
    ) -> rustc_hash::FxHashMap<String, TypeOnlyExports> {
        files
            .iter()
            .map(|file| {
                (
                    Self::normalized_file_name(&file.file_name),
                    collect_exports(file),
                )
            })
            .collect()
    }

    /// A module specifier's file: through the resolver, or — for a relative
    /// specifier the host registered no resolution for (`export type *`) —
    /// the program file it names.
    fn resolve_type_only_module(&self, specifier: &str, containing_file: &str) -> Option<String> {
        if let Some(path) = self.resolve_module_to_path(specifier, containing_file) {
            return Some(path);
        }
        if !(specifier.starts_with("./") || specifier.starts_with("../")) {
            return None;
        }
        let joined = std::path::Path::new(containing_file)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .join(specifier);
        // Virtual files cannot be canonicalized: drop `.`/`..` lexically.
        let mut normal = std::path::PathBuf::new();
        for component in joined.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    normal.pop();
                }
                other => normal.push(other.as_os_str()),
            }
        }
        let base = normal.to_string_lossy();
        let base = base.trim_end_matches(".js");
        [
            "",
            ".ts",
            ".tsx",
            ".d.ts",
            ".js",
            ".jsx",
            "/index.ts",
            "/index.js",
        ]
        .iter()
        .map(|suffix| format!("{base}{suffix}"))
        .find(|candidate| {
            self.type_only_exports
                .contains_key(&Self::normalized_file_name(candidate))
        })
    }

    /// Module `path`'s export `name`: the first type-only declaration on
    /// the way to it, and whether what it finally names has a value meaning
    /// (`None`: not found).
    fn resolve_type_only_export(
        &self,
        path: &str,
        name: &str,
        hops: u8,
    ) -> Option<(Option<TypeOnlyOrigin>, bool)> {
        if hops == 0 {
            return None;
        }
        if name == "*" {
            return Some((None, true));
        }
        let exports = self
            .type_only_exports
            .get(&Self::normalized_file_name(path))?;
        if let Some(entry) = exports.named.get(name) {
            let (origin, target) = match entry {
                ExportEntry::Plain(target) => (None, target),
                ExportEntry::TypeOnly(origin, target) => (Some(origin.clone()), target),
            };
            let (inner_origin, value) = self.resolve_type_only_target(target, path, hops);
            return Some((origin.or(inner_origin), value));
        }
        if name == "default" {
            return None;
        }
        // Stars that provide the name: a value star wins over a type-only one.
        let mut type_only_star = None;
        for (source, origin) in &exports.stars {
            let Some(next) = self.resolve_type_only_module(source, path) else {
                continue;
            };
            let Some((inner_origin, value)) = self.resolve_type_only_export(&next, name, hops - 1)
            else {
                continue;
            };
            match origin {
                Some(origin) => {
                    type_only_star.get_or_insert((Some(origin.clone()), value));
                }
                None => return Some((inner_origin, value)),
            }
        }
        type_only_star
    }

    /// See `resolve_type_only_export`; an unresolvable target counts as a
    /// value.
    fn resolve_type_only_target(
        &self,
        target: &Target,
        path: &str,
        hops: u8,
    ) -> (Option<TypeOnlyOrigin>, bool) {
        match target {
            Target::Local(value) => (None, *value),
            Target::Module { source, imported } => self
                .resolve_type_only_module(source, path)
                .and_then(|next| self.resolve_type_only_export(&next, imported, hops - 1))
                .unwrap_or((None, true)),
        }
    }

    /// For the file being checked: each import binding whose value is
    /// type-only, with where it became so.
    pub(crate) fn collect_type_only_bindings(
        &self,
        file: &SourceFile,
        current_file: &str,
    ) -> rustc_hash::FxHashMap<String, TypeOnlyOrigin> {
        let mut result = rustc_hash::FxHashMap::default();
        for (local, binding) in collect_import_bindings(file) {
            if binding.type_only.is_none() && self.type_only_exports.is_empty() {
                continue;
            }
            let target = Target::Module {
                source: binding.source,
                imported: binding.imported,
            };
            let (inner_origin, value) = self.resolve_type_only_target(&target, current_file, 16);
            // A type-only alias of a declaration without a value meaning
            // does not shadow a value of the same name.
            if let Some(origin) = binding.type_only.or(inner_origin) {
                if value {
                    result.insert(local, origin);
                }
            }
        }
        result
    }

    /// TS1361/TS1362 at a value use of a type-only import binding.
    pub(crate) fn check_type_only_value_use(&mut self, name: &str, span: Span) {
        if self.type_only_bindings.is_empty()
            || self.ambient_depth > 0
            || self.type_only_use_allowed > 0
            || self.type_only_allowed_span == Some(span)
            || self.current_file_is_declaration()
        {
            return;
        }
        let Some(origin) = self.type_only_bindings.get(name).cloned() else {
            return;
        };
        if !self.binding_scope_is_root(name).unwrap_or(true)
            || !self
                .reported_duplicate_spans
                .insert((origin.code, span.start, span.end))
        {
            return;
        }
        let (how, note_code, note) = if origin.code == 1361 {
            ("imported using 'import type'", 1376, "imported")
        } else {
            ("exported using 'export type'", 1377, "exported")
        };
        self.diagnostics.push(Diagnostic {
            code: origin.code,
            message: format!("'{name}' cannot be used as a value because it was {how}."),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: Some(vec![RelatedDiagnostic {
                code: note_code,
                message: format!("'{name}' was {note} here."),
                file_name: Some(origin.file),
                span: Some(origin.span),
            }]),
        });
    }

    /// A type-only binding has no value meaning here, so our resolution may
    /// also report it as unknown (TS2304); tsc reports only TS1361/TS1362.
    pub(crate) fn drop_unresolved_type_only_uses(&mut self) {
        if self.type_only_bindings.is_empty() {
            return;
        }
        let reported: rustc_hash::FxHashSet<(u32, u32)> = self
            .diagnostics
            .iter()
            .filter(|d| matches!(d.code, 1361 | 1362))
            .filter_map(|d| d.span.map(|span| (span.start, span.end)))
            .collect();
        if reported.is_empty() {
            return;
        }
        self.diagnostics.retain(|d| {
            !(d.code == 2304
                && d.span
                    .is_some_and(|span| reported.contains(&(span.start, span.end))))
        });
    }
}
