//! AST analysis: namespace/export collection, value ref scanning, class checks, type metadata.

use super::*;

/// Collect exported names from an ExportDeclKind for pre-declaration.
///
/// This is used in CommonJS mode to emit `exports.X = void 0;` for each
/// exported binding.
pub(crate) fn collect_export_names(
    kind: &ExportDeclKind,
    names: &mut Vec<String>,
    preserve_const_enums: bool,
) {
    match kind {
        ExportDeclKind::Decl(decl) => match &decl.kind {
            StmtKind::FnDecl(_fn_decl) => {
                // Functions are hoisted and get `exports.name = name;` directly,
                // not `exports.name = void 0;` pre-declarations.
            }
            StmtKind::ClassDecl(class_decl) => {
                if class_decl.modifiers & MOD_DECLARE == 0 {
                    if let Some(ref name) = class_decl.name {
                        names.push(name.clone());
                    }
                }
            }
            StmtKind::Var(var_stmt) => {
                if var_stmt.modifiers & MOD_DECLARE == 0 {
                    for decl in &var_stmt.declarations {
                        collect_binding_names(&decl.name, names);
                    }
                }
            }
            StmtKind::EnumDecl(enum_decl) => {
                if enum_decl.modifiers & MOD_DECLARE == 0 {
                    names.push(enum_decl.name.clone());
                }
            }
            StmtKind::ModuleDecl(module_decl) => {
                // Namespaces DO get pre-declarations (unlike enums).
                if module_decl.modifiers & MOD_DECLARE == 0
                    && !module_decl_is_type_only(module_decl, preserve_const_enums)
                {
                    let name = match &module_decl.name {
                        ModuleName::Ident(n) => n.clone(),
                        ModuleName::String(n) => n.clone(),
                    };
                    names.push(name);
                }
            }
            // Interface and type alias are type-only, no runtime export.
            // ImportEquals is handled in the caller with runtime value check.
            _ => {}
        },
        ExportDeclKind::Named {
            specifiers,
            type_only,
            ..
        } => {
            if !type_only {
                for spec in specifiers {
                    if !spec.is_type {
                        let name = spec.exported.as_ref().unwrap_or(&spec.local);
                        names.push(name.clone());
                    }
                }
            }
        }
        ExportDeclKind::Default(_) | ExportDeclKind::DefaultDecl(_) => {
            // Default exports never get void 0 pre-declaration.
            // They are handled via direct assignment by emit_export_decl_cjs.
        }
        ExportDeclKind::All {
            alias: Some(alias),
            type_only: false,
            ..
        } => {
            // `export * as ns from "..."` reserves `exports.ns`.
            names.push(alias.clone());
        }
        ExportDeclKind::All { .. } => {
            // Plain star re-exports don't need pre-declaration.
        }
    }
}

/// Check if any import statement in the file binds `name` as a local
/// (default import, named import — including `as` aliases — or namespace).
/// Type-only imports do NOT count as a runtime binding.
pub(crate) fn has_import_local_named(stmts: &[Stmt], name: &str) -> bool {
    for s in stmts {
        if let StmtKind::Import(imp) = &s.kind {
            if imp.type_only {
                continue;
            }
            match &imp.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    if default.as_deref() == Some(name) {
                        return true;
                    }
                    if namespace.as_deref() == Some(name) {
                        return true;
                    }
                    for spec in named {
                        if spec.is_type {
                            continue;
                        }
                        if spec.local == name {
                            return true;
                        }
                    }
                }
                ImportClause::Require(local) => {
                    if local == name {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check if a file has a local non-import value declaration for the given name.
/// Handles var/const/let, class, function, enum, and namespace declarations,
/// including those wrapped in `export { ... }`.
pub(crate) fn has_local_value_decl(stmts: &[Stmt], name: &str) -> bool {
    fn check_stmt(s: &Stmt, name: &str) -> bool {
        match &s.kind {
            StmtKind::Var(v) if v.modifiers & MOD_DECLARE == 0 => {
                let mut names = Vec::new();
                for d in &v.declarations {
                    collect_binding_names(&d.name, &mut names);
                }
                names.iter().any(|n| n == name)
            }
            StmtKind::ClassDecl(c) if c.modifiers & MOD_DECLARE == 0 => {
                c.name.as_deref() == Some(name)
            }
            StmtKind::FnDecl(f) if f.modifiers & MOD_DECLARE == 0 => {
                f.name.as_deref() == Some(name)
            }
            StmtKind::EnumDecl(e) if e.modifiers & MOD_DECLARE == 0 => e.name == name,
            StmtKind::ModuleDecl(m)
                if m.modifiers & MOD_DECLARE == 0 && !module_decl_is_type_only(m, false) =>
            {
                matches!(&m.name, ModuleName::Ident(n) if n == name)
            }
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(inner) => check_stmt(inner, name),
                _ => false,
            },
            _ => false,
        }
    }
    stmts.iter().any(|s| check_stmt(s, name))
}

/// Collect local named export assignments from `export { local as exported }`.
///
/// Re-exports (`from "..."`) and type-only exports are ignored.
pub(crate) fn collect_local_named_export_assignments(
    kind: &ExportDeclKind,
    assignments: &mut Vec<(String, String)>,
) {
    if let ExportDeclKind::Named {
        specifiers,
        source: None,
        type_only: false,
    } = kind
    {
        for spec in specifiers {
            if spec.is_type {
                continue;
            }
            let exported = spec.exported.as_ref().unwrap_or(&spec.local).clone();
            assignments.push((exported, spec.local.clone()));
        }
    }
}

/// Collect the set of names that are locally value-bound in a file.
///
/// A name is value-bound if it is declared as a non-ambient runtime value
/// (var/let/const, function, class, non-const enum, non-type-only namespace)
/// or imported as a value binding (non-type-only import).
///
/// Names that appear only in ambient declarations (`declare ...`) or are
/// referenced only as types have no runtime representation in the emitted
/// JavaScript and should not get `exports.X = X;` assignments.
pub(crate) fn collect_file_value_bound_names(
    stmts: &[Stmt],
    preserve_const_enums: bool,
) -> HashSet<AstString> {
    let mut names: HashSet<AstString> = HashSet::new();
    for stmt in stmts {
        let mut tmp: Vec<String> = Vec::new();
        match &stmt.kind {
            StmtKind::Var(v) if v.modifiers & MOD_DECLARE == 0 => {
                for d in &v.declarations {
                    collect_binding_names(&d.name, &mut tmp);
                }
            }
            StmtKind::FnDecl(f) if f.modifiers & MOD_DECLARE == 0 => {
                if let Some(ref n) = f.name {
                    tmp.push(n.clone());
                }
            }
            StmtKind::ClassDecl(c) if c.modifiers & MOD_DECLARE == 0 => {
                if let Some(ref n) = c.name {
                    tmp.push(n.clone());
                }
            }
            StmtKind::EnumDecl(e)
                if e.modifiers & MOD_DECLARE == 0 && (!e.is_const || preserve_const_enums) =>
            {
                tmp.push(e.name.clone());
            }
            StmtKind::ModuleDecl(m)
                if m.modifiers & MOD_DECLARE == 0
                    && !module_decl_is_type_only(m, preserve_const_enums) =>
            {
                if let ModuleName::Ident(ref n) = m.name {
                    tmp.push(n.clone());
                }
            }
            StmtKind::Import(imp) if !imp.type_only => match &imp.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    if let Some(d) = default {
                        tmp.push(d.clone());
                    }
                    if let Some(ns) = namespace {
                        tmp.push(ns.clone());
                    }
                    for spec in named {
                        if !spec.is_type {
                            tmp.push(spec.local.clone());
                        }
                    }
                }
                ImportClause::Require(name) => {
                    tmp.push(name.clone());
                }
            },
            StmtKind::ImportEquals(ie) => {
                tmp.push(ie.name.clone());
            }
            StmtKind::Export(e) => {
                if let ExportDeclKind::Decl(ref d) = e.kind {
                    match &d.kind {
                        StmtKind::Var(v) if v.modifiers & MOD_DECLARE == 0 => {
                            for decl in &v.declarations {
                                collect_binding_names(&decl.name, &mut tmp);
                            }
                        }
                        StmtKind::FnDecl(f) if f.modifiers & MOD_DECLARE == 0 => {
                            if let Some(ref n) = f.name {
                                tmp.push(n.clone());
                            }
                        }
                        StmtKind::ClassDecl(c) if c.modifiers & MOD_DECLARE == 0 => {
                            if let Some(ref n) = c.name {
                                tmp.push(n.clone());
                            }
                        }
                        StmtKind::EnumDecl(en)
                            if en.modifiers & MOD_DECLARE == 0
                                && (!en.is_const || preserve_const_enums) =>
                        {
                            tmp.push(en.name.clone());
                        }
                        StmtKind::ModuleDecl(m)
                            if m.modifiers & MOD_DECLARE == 0
                                && !module_decl_is_type_only(m, preserve_const_enums) =>
                        {
                            if let ModuleName::Ident(ref n) = m.name {
                                tmp.push(n.clone());
                            }
                        }
                        StmtKind::Import(imp) if !imp.type_only => match &imp.specifiers {
                            ImportClause::Require(name) => {
                                tmp.push(name.clone());
                            }
                            ImportClause::Named {
                                default,
                                named,
                                namespace,
                            } => {
                                if let Some(d) = default {
                                    tmp.push(d.clone());
                                }
                                if let Some(ns) = namespace {
                                    tmp.push(ns.clone());
                                }
                                for spec in named {
                                    if !spec.is_type {
                                        tmp.push(spec.local.clone());
                                    }
                                }
                            }
                        },
                        StmtKind::ImportEquals(ie) => {
                            tmp.push(ie.name.clone());
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        names.extend(tmp.into_iter().map(AstString::from));
    }
    names
}

/// Check whether any binding introduced inside the namespace body's nested
/// scopes (function parameters, local variables, catch params, etc.) collides
/// with `name`.  This tells the emitter it must rename the IIFE parameter
/// (e.g. `M` → `M_1`).
/// Returns `true` if a namespace body contains only type-level declarations
/// (interfaces, type aliases, other type-only namespaces, declare statements)
/// and no runtime code. Such namespaces are completely erased by TypeScript.
pub(crate) fn namespace_body_is_type_only(stmts: &[Stmt], preserve_const_enums: bool) -> bool {
    // Collect all value-producing names in this namespace body
    let value_names: HashSet<&str> = stmts
        .iter()
        .filter_map(|s| match &s.kind {
            StmtKind::FnDecl(f) if f.modifiers & MOD_DECLARE == 0 => f.name.as_deref(),
            StmtKind::Var(v) if v.modifiers & MOD_DECLARE == 0 => {
                v.declarations.first().and_then(|d| {
                    if let PatKind::Ident(ref n) = d.name.kind {
                        Some(n.as_str())
                    } else {
                        None
                    }
                })
            }
            StmtKind::ClassDecl(c) if c.modifiers & MOD_DECLARE == 0 => c.name.as_deref(),
            StmtKind::EnumDecl(e)
                if e.modifiers & MOD_DECLARE == 0 && (!e.is_const || preserve_const_enums) =>
            {
                Some(e.name.as_str())
            }
            StmtKind::ModuleDecl(m) if !module_decl_is_type_only(m, preserve_const_enums) => {
                if let ModuleName::Ident(ref n) = m.name {
                    Some(n.as_str())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    stmts.iter().all(|s| match &s.kind {
        StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => true,
        StmtKind::ModuleDecl(m) => module_decl_is_type_only(m, preserve_const_enums),
        StmtKind::FnDecl(f) => f.modifiers & MOD_DECLARE != 0,
        StmtKind::Var(v) => v.modifiers & MOD_DECLARE != 0,
        StmtKind::ClassDecl(c) => c.modifiers & MOD_DECLARE != 0,
        StmtKind::EnumDecl(e) => {
            e.modifiers & MOD_DECLARE != 0 || (e.is_const && !preserve_const_enums)
        }
        StmtKind::Export(ed) => match &ed.kind {
            ExportDeclKind::Decl(d) => match &d.kind {
                StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => true,
                StmtKind::FnDecl(f) => f.modifiers & MOD_DECLARE != 0,
                StmtKind::Var(v) => v.modifiers & MOD_DECLARE != 0,
                StmtKind::ClassDecl(c) => c.modifiers & MOD_DECLARE != 0,
                StmtKind::EnumDecl(e) => {
                    e.modifiers & MOD_DECLARE != 0 || (e.is_const && !preserve_const_enums)
                }
                StmtKind::ModuleDecl(m) => module_decl_is_type_only(m, preserve_const_enums),
                _ => false,
            },
            ExportDeclKind::Named { type_only, .. } => *type_only, // non-type re-exports instantiate the namespace
            _ => false,
        },
        // import X = Y: type-only when it doesn't reference a value-producing
        // declaration in this namespace body. Simple identifiers are checked
        // directly; dotted names (X.Y.Z) are type-only when no value name
        // in this body matches the root.
        StmtKind::ImportEquals(ie) => match &ie.module_ref.kind {
            ExprKind::Ident(name) => !value_names.contains(name.as_str()),
            ExprKind::Member(m) => {
                // Walk to the root of the member chain.
                let mut current = &*m.object;
                while let ExprKind::Member(inner) = &current.kind {
                    current = &*inner.object;
                }
                match &current.kind {
                    ExprKind::Ident(root) => !value_names.contains(root.as_str()),
                    _ => false,
                }
            }
            _ => false,
        },
        StmtKind::Import(_) => true,
        _ => false,
    })
}

/// Convenience: check if a module decl is type-only without needing preserve_const_enums context.
/// Used in collision detection where const enums are always considered value-producing.
pub(crate) fn module_decl_is_type_only_standalone(module_decl: &ModuleDecl) -> bool {
    module_decl_is_type_only(module_decl, false)
}

pub(crate) fn module_decl_is_type_only(
    module_decl: &ModuleDecl,
    preserve_const_enums: bool,
) -> bool {
    if module_decl.modifiers & MOD_DECLARE != 0 {
        return true;
    }
    match &module_decl.body {
        Some(ModuleBody::Block(stmts)) => {
            // Non-declare namespaces with only ambient (declare) members still
            // need the IIFE wrapper.  Only skip when ALL statements are pure
            // type declarations (interfaces, type aliases) that produce no
            // runtime binding at all.
            if stmts.is_empty() {
                return true;
            }
            // Check if any member is an ambient value declaration (declare class,
            // declare enum, declare function, etc.) — these still need the IIFE.
            let has_ambient_value = stmts.iter().any(|s| match &s.kind {
                StmtKind::ClassDecl(c) => c.modifiers & MOD_DECLARE != 0,
                StmtKind::EnumDecl(e) => e.modifiers & MOD_DECLARE != 0,
                StmtKind::FnDecl(f) => f.modifiers & MOD_DECLARE != 0 && f.body.is_none(),
                StmtKind::Var(v) => v.modifiers & MOD_DECLARE != 0,
                StmtKind::Export(ed) => match &ed.kind {
                    ExportDeclKind::Decl(d) => match &d.kind {
                        StmtKind::ClassDecl(c) => c.modifiers & MOD_DECLARE != 0,
                        StmtKind::EnumDecl(e) => e.modifiers & MOD_DECLARE != 0,
                        StmtKind::FnDecl(f) => f.modifiers & MOD_DECLARE != 0 && f.body.is_none(),
                        StmtKind::Var(v) => v.modifiers & MOD_DECLARE != 0,
                        _ => false,
                    },
                    _ => false,
                },
                _ => false,
            });
            if has_ambient_value {
                return false;
            }
            namespace_body_is_type_only(stmts, preserve_const_enums)
        }
        Some(ModuleBody::Module(inner)) => module_decl_is_type_only(inner, preserve_const_enums),
        None => true,
    }
}

/// Returns true when a `declare namespace` has value-level declarations
/// (classes, enums, functions, vars) in its body.  Used by import-equals
/// alias retention: `import ab = A.B` should emit `var ab = A.B` when
/// `declare namespace A.B` has value-level exports, even though the
/// namespace itself produces no IIFE.
pub(crate) fn declare_namespace_has_value_exports(module_decl: &ModuleDecl) -> bool {
    match &module_decl.body {
        Some(ModuleBody::Block(stmts)) => stmts.iter().any(|s| match &s.kind {
            StmtKind::ClassDecl(_)
            | StmtKind::EnumDecl(_)
            | StmtKind::FnDecl(_)
            | StmtKind::Var(_) => true,
            StmtKind::ModuleDecl(m) => declare_namespace_has_value_exports(m),
            StmtKind::Export(ed) => match &ed.kind {
                ExportDeclKind::Decl(d) => match &d.kind {
                    StmtKind::ClassDecl(_)
                    | StmtKind::EnumDecl(_)
                    | StmtKind::FnDecl(_)
                    | StmtKind::Var(_) => true,
                    StmtKind::ModuleDecl(m) => declare_namespace_has_value_exports(m),
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        }),
        Some(ModuleBody::Module(inner)) => declare_namespace_has_value_exports(inner),
        None => false,
    }
}

/// Stricter variant of `module_decl_is_type_only` for cross-file type-only tracking.
///
/// Unlike `module_decl_is_type_only` which treats ALL `declare namespace` as
/// type-only (correct for the emitter's own handling where `declare namespace`
/// produces no runtime code), this function checks whether a namespace is
/// PURELY type-only — i.e., it contains only interfaces, type aliases, and
/// other type-only nested namespaces, with NO value members (var, function,
/// class, enum) even if those are also `declare`.
///
/// Used by `collect_type_only_names` for cross-file type-only name tracking
/// where a `declare namespace` with value members (like `export var foo`)
/// should still allow re-exports to get CJS pre-declarations.
pub(crate) fn declare_namespace_is_purely_type_only(
    module_decl: &ModuleDecl,
    preserve_const_enums: bool,
) -> bool {
    match &module_decl.body {
        Some(ModuleBody::Block(stmts)) => {
            stmts.iter().all(|s| match &s.kind {
                StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => true,
                StmtKind::ModuleDecl(m) => {
                    declare_namespace_is_purely_type_only(m, preserve_const_enums)
                }
                StmtKind::Export(ed) => match &ed.kind {
                    ExportDeclKind::Decl(d) => match &d.kind {
                        StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => true,
                        StmtKind::ModuleDecl(m) => {
                            declare_namespace_is_purely_type_only(m, preserve_const_enums)
                        }
                        // Any value declaration (even `declare var`) means it's not purely type-only.
                        _ => false,
                    },
                    ExportDeclKind::Named {
                        type_only: true, ..
                    } => true,
                    ExportDeclKind::Named { specifiers, .. } => {
                        specifiers.iter().all(|s| s.is_type)
                    }
                    _ => false,
                },
                // Import-equals in type-only position
                StmtKind::ImportEquals(..) => true,
                // Any other statement (var, function, class, enum) is a value.
                _ => false,
            })
        }
        Some(ModuleBody::Module(inner)) => {
            declare_namespace_is_purely_type_only(inner, preserve_const_enums)
        }
        None => true,
    }
}

///
/// We only look *inside* nested scopes — method/function bodies, arrow bodies,
/// catch clauses, class property initializers, etc. — not at the top-level
/// declarations of the namespace body themselves (those live at the IIFE
/// parameter scope level and don't collide).
pub(crate) fn has_ns_name_collision(stmts: &[Stmt], name: &str) -> bool {
    /// Returns `true` if `pat` introduces a binding whose name equals `name`.
    fn pat_has_name(pat: &Pat, name: &str) -> bool {
        match &pat.kind {
            PatKind::Ident(n) => n == name,
            PatKind::Array(elems) => elems.iter().flatten().any(|elem| match elem {
                ArrayPatElem::Pat(p) => pat_has_name(p, name),
                ArrayPatElem::Rest(p) => pat_has_name(p, name),
            }),
            PatKind::Object(props) => props.iter().any(|prop| match prop {
                ObjPatProp::KeyValue(_, val) => pat_has_name(val, name),
                ObjPatProp::Shorthand(n, _) => n == name,
                ObjPatProp::ShorthandAssign(n, _, _) => n == name,
                ObjPatProp::Rest(p) => pat_has_name(p, name),
            }),
            PatKind::Assign(p, _) => pat_has_name(p, name),
            PatKind::Rest(p) => pat_has_name(p, name),
        }
    }

    /// Check if any of the given parameters introduce a binding named `name`.
    fn params_have_name(params: &[Param], name: &str) -> bool {
        params.iter().any(|p| pat_has_name(&p.name, name))
    }

    /// Check if any binding in the given var declarators matches `name`.
    fn var_decls_have_name(decls: &[VarDeclarator], name: &str) -> bool {
        decls.iter().any(|d| pat_has_name(&d.name, name))
    }

    /// Recursively scan statements for a binding named `name`.
    fn scan_stmts(stmts: &[Stmt], name: &str) -> bool {
        for s in stmts {
            if scan_stmt(s, name) {
                return true;
            }
        }
        false
    }

    /// Scan a single statement for a binding named `name`.
    fn scan_stmt(stmt: &Stmt, name: &str) -> bool {
        match &stmt.kind {
            StmtKind::Var(v) => {
                if var_decls_have_name(&v.declarations, name) {
                    return true;
                }
                // Also scan initializers for nested functions/arrows/classes
                for d in &v.declarations {
                    if let Some(init) = &d.init {
                        if scan_expr(init, name) {
                            return true;
                        }
                    }
                }
                false
            }
            StmtKind::Expr(e) => scan_expr(e, name),
            StmtKind::Return(Some(e)) => scan_expr(e, name),
            StmtKind::Return(None) => false,
            StmtKind::If(if_s) => {
                scan_expr(&if_s.test, name)
                    || scan_stmt(&if_s.consequent, name)
                    || if_s.alternate.as_ref().is_some_and(|a| scan_stmt(a, name))
            }
            StmtKind::While(w) => scan_expr(&w.test, name) || scan_stmt(&w.body, name),
            StmtKind::DoWhile(d) => scan_stmt(&d.body, name) || scan_expr(&d.test, name),
            StmtKind::For(f) => {
                if let Some(init) = &f.init {
                    match init {
                        ForInit::Var(v) => {
                            if var_decls_have_name(&v.declarations, name) {
                                return true;
                            }
                            for d in &v.declarations {
                                if let Some(e) = &d.init {
                                    if scan_expr(e, name) {
                                        return true;
                                    }
                                }
                            }
                        }
                        ForInit::Expr(e) => {
                            if scan_expr(e, name) {
                                return true;
                            }
                        }
                    }
                }
                if f.test.as_ref().is_some_and(|e| scan_expr(e, name)) {
                    return true;
                }
                if f.update.as_ref().is_some_and(|e| scan_expr(e, name)) {
                    return true;
                }
                scan_stmt(&f.body, name)
            }
            StmtKind::ForIn(fi) => {
                if match &fi.left {
                    ForInOfLeft::Var(v) => var_decls_have_name(&v.declarations, name),
                    ForInOfLeft::Pat(p) => pat_has_name(p, name),
                    ForInOfLeft::Expr(e) => scan_expr(e, name),
                } {
                    return true;
                }
                scan_expr(&fi.right, name) || scan_stmt(&fi.body, name)
            }
            StmtKind::ForOf(fo) => {
                if match &fo.left {
                    ForInOfLeft::Var(v) => var_decls_have_name(&v.declarations, name),
                    ForInOfLeft::Pat(p) => pat_has_name(p, name),
                    ForInOfLeft::Expr(e) => scan_expr(e, name),
                } {
                    return true;
                }
                scan_expr(&fo.right, name) || scan_stmt(&fo.body, name)
            }
            StmtKind::Switch(sw) => {
                if scan_expr(&sw.discriminant, name) {
                    return true;
                }
                for case in &sw.cases {
                    if case.test.as_ref().is_some_and(|e| scan_expr(e, name)) {
                        return true;
                    }
                    if scan_stmts(&case.consequent, name) {
                        return true;
                    }
                }
                false
            }
            StmtKind::Try(t) => {
                if scan_stmts(&t.block, name) {
                    return true;
                }
                if let Some(catch) = &t.handler {
                    if catch.param.as_ref().is_some_and(|p| pat_has_name(p, name)) {
                        return true;
                    }
                    if scan_stmts(&catch.body, name) {
                        return true;
                    }
                }
                if let Some(fin) = &t.finalizer {
                    if scan_stmts(fin, name) {
                        return true;
                    }
                }
                false
            }
            StmtKind::Throw(e) => scan_expr(e, name),
            StmtKind::Block(stmts) => scan_stmts(stmts, name),
            StmtKind::Labeled(l) => scan_stmt(&l.body, name),
            StmtKind::With(w) => scan_expr(&w.object, name) || scan_stmt(&w.body, name),
            StmtKind::FnDecl(f) => {
                // Check the declaration name itself (introduces a binding in the enclosing scope)
                if f.name.as_deref() == Some(name) {
                    return true;
                }
                scan_fn_decl(f, name)
            }
            StmtKind::ClassDecl(c) => {
                if c.name.as_deref() == Some(name) {
                    return true;
                }
                scan_class_decl(c, name)
            }
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) => {
                    // Exported import-equals inside a namespace emits as
                    // `param.name = rhs;` (not `var name = rhs;`), so it
                    // does NOT introduce a local binding that shadows the
                    // IIFE parameter.
                    if matches!(&inner.kind, StmtKind::ImportEquals(..)) {
                        false
                    } else {
                        scan_stmt(inner, name)
                    }
                }
                ExportDeclKind::Default(e) => scan_expr(e, name),
                ExportDeclKind::DefaultDecl(inner) => scan_stmt(inner, name),
                _ => false,
            },
            StmtKind::ModuleDecl(m) => {
                // Check the namespace's own name (it introduces a `let` binding).
                // Only count as collision if the namespace is NOT type-only
                // (type-only namespaces are erased and don't produce a `let`).
                let mod_name = match &m.name {
                    ModuleName::Ident(n) => Some(n.as_str()),
                    ModuleName::String(_) => None,
                };
                if mod_name == Some(name) && !module_decl_is_type_only_standalone(m) {
                    return true;
                }
                if let Some(body) = &m.body {
                    match body {
                        ModuleBody::Block(stmts) => scan_stmts(stmts, name),
                        ModuleBody::Module(inner) => {
                            let inner_name = match &inner.name {
                                ModuleName::Ident(n) => Some(n.as_str()),
                                ModuleName::String(_) => None,
                            };
                            if inner_name == Some(name)
                                && !module_decl_is_type_only_standalone(inner)
                            {
                                return true;
                            }
                            if let Some(inner_body) = &inner.body {
                                match inner_body {
                                    ModuleBody::Block(stmts) => scan_stmts(stmts, name),
                                    _ => false,
                                }
                            } else {
                                false
                            }
                        }
                    }
                } else {
                    false
                }
            }
            StmtKind::EnumDecl(e) => e.name == name,
            StmtKind::ImportEquals(ie) => ie.name == name,
            StmtKind::Import(imp) => {
                // `import m2 = require(...)` introduces a local binding.
                match &imp.specifiers {
                    ImportClause::Require(local_name) => local_name == name,
                    ImportClause::Named {
                        default,
                        named,
                        namespace,
                    } => {
                        default.as_deref() == Some(name)
                            || namespace.as_deref() == Some(name)
                            || named.iter().any(|s| s.local == name)
                    }
                }
            }
            _ => false,
        }
    }

    /// Scan a function declaration's params and body for collisions.
    fn scan_fn_decl(f: &FnDecl, name: &str) -> bool {
        // Check if the function's own name collides
        if let Some(ref fn_name) = f.name {
            if fn_name == name {
                return true;
            }
        }
        if params_have_name(&f.params, name) {
            return true;
        }
        if let Some(body) = &f.body {
            if scan_stmts(body, name) {
                return true;
            }
        }
        false
    }

    /// Scan class declaration name and members for collisions.
    fn scan_class_decl(c: &ClassDecl, name: &str) -> bool {
        // Check if the class's own name collides
        if let Some(ref class_name) = c.name {
            if class_name == name {
                return true;
            }
        }
        for member in &c.members {
            match &member.kind {
                ClassMemberKind::Method(m) => {
                    // Only check params and body, NOT the method name
                    if params_have_name(&m.params, name) {
                        return true;
                    }
                    if let Some(body) = &m.body {
                        if scan_stmts(body, name) {
                            return true;
                        }
                    }
                }
                ClassMemberKind::Constructor(ctor) => {
                    if params_have_name(&ctor.params, name) {
                        return true;
                    }
                    if let Some(body) = &ctor.body {
                        if scan_stmts(body, name) {
                            return true;
                        }
                    }
                }
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    // Only check params and body, NOT the accessor name
                    if params_have_name(&acc.params, name) {
                        return true;
                    }
                    if let Some(body) = &acc.body {
                        if scan_stmts(body, name) {
                            return true;
                        }
                    }
                }
                ClassMemberKind::Property(prop) => {
                    if let Some(init) = &prop.initializer {
                        if scan_expr(init, name) {
                            return true;
                        }
                    }
                }
                ClassMemberKind::StaticBlock(stmts) => {
                    if scan_stmts(stmts, name) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Scan an expression for nested functions/arrows/classes that introduce
    /// bindings matching `name`.
    fn scan_expr(expr: &Expr, name: &str) -> bool {
        match &expr.kind {
            ExprKind::FnExpr(f) => scan_fn_decl(f, name),
            ExprKind::Arrow(arrow) => {
                if params_have_name(&arrow.params, name) {
                    return true;
                }
                match &arrow.body {
                    ArrowBody::Expr(e) => scan_expr(e, name),
                    ArrowBody::Block(stmts) => scan_stmts(stmts, name),
                }
            }
            ExprKind::ClassExpr(c) => scan_class_decl(c, name),
            ExprKind::ObjectLit(props) => {
                for prop in props {
                    match prop {
                        ObjLitProp::Method(m) => {
                            // Only check params and body, NOT the method name
                            if params_have_name(&m.params, name) {
                                return true;
                            }
                            if scan_stmts(&m.body, name) {
                                return true;
                            }
                        }
                        ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                            // Only check params and body, NOT the accessor name
                            if params_have_name(&acc.params, name) {
                                return true;
                            }
                            if scan_stmts(&acc.body, name) {
                                return true;
                            }
                        }
                        ObjLitProp::Property(p) => {
                            if scan_expr(&p.value, name) {
                                return true;
                            }
                        }
                        ObjLitProp::Spread(e, _) => {
                            if scan_expr(e, name) {
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
                false
            }
            ExprKind::Call(call) => {
                if scan_expr(&call.callee, name) {
                    return true;
                }
                for arg in &call.args {
                    if scan_expr(arg, name) {
                        return true;
                    }
                }
                false
            }
            ExprKind::New(new_e) => {
                if scan_expr(&new_e.callee, name) {
                    return true;
                }
                if let Some(args) = &new_e.args {
                    for arg in args {
                        if scan_expr(arg, name) {
                            return true;
                        }
                    }
                }
                false
            }
            ExprKind::ArrayLit(elems) => {
                for elem in elems.iter().flatten() {
                    if scan_expr(elem, name) {
                        return true;
                    }
                }
                false
            }
            ExprKind::Paren(e) => scan_expr(e, name),
            ExprKind::Spread(e) => scan_expr(e, name),
            ExprKind::Assign(a) => scan_expr(&a.left, name) || scan_expr(&a.right, name),
            ExprKind::Binary(b) => scan_expr(&b.left, name) || scan_expr(&b.right, name),
            ExprKind::Cond(c) => {
                scan_expr(&c.test, name)
                    || scan_expr(&c.consequent, name)
                    || scan_expr(&c.alternate, name)
            }
            ExprKind::Member(m) => scan_expr(&m.object, name),
            ExprKind::ElemAccess(ea) => scan_expr(&ea.object, name) || scan_expr(&ea.index, name),
            ExprKind::Unary(u) => scan_expr(&u.argument, name),
            ExprKind::Update(u) => scan_expr(&u.argument, name),
            ExprKind::Comma(exprs) => exprs.iter().any(|e| scan_expr(e, name)),
            ExprKind::Template(t) => t.exprs.iter().any(|e| scan_expr(e, name)),
            ExprKind::TaggedTemplate(t) => {
                scan_expr(&t.tag, name) || t.quasi.exprs.iter().any(|e| scan_expr(e, name))
            }
            ExprKind::Yield(_, Some(e)) => scan_expr(e, name),
            ExprKind::Await(e) => scan_expr(e, name),
            ExprKind::Delete(e) => scan_expr(e, name),
            ExprKind::Typeof(e) => scan_expr(e, name),
            ExprKind::Void(e) => scan_expr(e, name),
            ExprKind::TypeAssertion(e) => scan_expr(&e.expr, name),
            ExprKind::As(e) => scan_expr(&e.expr, name),
            ExprKind::Satisfies(e) => scan_expr(&e.expr, name),
            ExprKind::NonNull(e) => scan_expr(e, name),
            ExprKind::Instantiation(e) => scan_expr(&e.expr, name),
            _ => false,
        }
    }

    // Entry point: scan each top-level statement for collisions.
    // Check both declaration names and inner scopes.
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::ClassDecl(c) => {
                if c.name.as_deref() == Some(name) {
                    return true;
                }
                if scan_class_decl(c, name) {
                    return true;
                }
            }
            StmtKind::FnDecl(f) => {
                if f.name.as_deref() == Some(name) {
                    return true;
                }
                if scan_fn_decl(f, name) {
                    return true;
                }
            }
            StmtKind::EnumDecl(e) => {
                if e.name == name {
                    return true;
                }
            }
            StmtKind::Var(v) => {
                if var_decls_have_name(&v.declarations, name) {
                    return true;
                }
                for d in &v.declarations {
                    if let Some(init) = &d.init {
                        if scan_expr(init, name) {
                            return true;
                        }
                    }
                }
            }
            StmtKind::Export(export) => {
                match &export.kind {
                    ExportDeclKind::Decl(inner) => {
                        // Recurse into the inner declaration
                        match &inner.kind {
                            StmtKind::ClassDecl(c) => {
                                if c.name.as_deref() == Some(name) {
                                    return true;
                                }
                                if scan_class_decl(c, name) {
                                    return true;
                                }
                            }
                            StmtKind::FnDecl(f) => {
                                if f.name.as_deref() == Some(name) {
                                    return true;
                                }
                                if scan_fn_decl(f, name) {
                                    return true;
                                }
                            }
                            StmtKind::EnumDecl(e) => {
                                if e.name == name {
                                    return true;
                                }
                            }
                            StmtKind::Var(v) => {
                                if var_decls_have_name(&v.declarations, name) {
                                    return true;
                                }
                                for d in &v.declarations {
                                    if let Some(init) = &d.init {
                                        if scan_expr(init, name) {
                                            return true;
                                        }
                                    }
                                }
                            }
                            // Exported import-equals inside a namespace
                            // emits as `param.name = rhs;` (an assignment),
                            // not `var name = rhs;`, so skip collision check.
                            StmtKind::ImportEquals(..) => {}
                            _ => {
                                if scan_stmt(inner, name) {
                                    return true;
                                }
                            }
                        }
                    }
                    ExportDeclKind::Default(e) => {
                        if scan_expr(e, name) {
                            return true;
                        }
                    }
                    ExportDeclKind::DefaultDecl(inner) => match &inner.kind {
                        StmtKind::ClassDecl(c) => {
                            if scan_class_decl(c, name) {
                                return true;
                            }
                        }
                        StmtKind::FnDecl(f) => {
                            if scan_fn_decl(f, name) {
                                return true;
                            }
                        }
                        _ => {
                            if scan_stmt(inner, name) {
                                return true;
                            }
                        }
                    },
                    _ => {}
                }
            }
            StmtKind::Expr(e) => {
                if scan_expr(e, name) {
                    return true;
                }
            }
            _ => {
                if scan_stmt(stmt, name) {
                    return true;
                }
            }
        }
    }
    false
}

/// Collect binding names from a pattern (for var declarations).
pub(crate) fn collect_binding_names(pat: &Pat, names: &mut Vec<String>) {
    match &pat.kind {
        PatKind::Ident(name) if name != "<error>" => names.push(name.to_string()),
        PatKind::Ident(_) => {} // skip error placeholders
        PatKind::Array(elems) => {
            for elem in elems.iter().flatten() {
                match elem {
                    ArrayPatElem::Pat(p) => collect_binding_names(p, names),
                    ArrayPatElem::Rest(p) => collect_binding_names(p, names),
                }
            }
        }
        PatKind::Object(props) => {
            for prop in props {
                match prop {
                    ObjPatProp::KeyValue(_, val) => collect_binding_names(val, names),
                    ObjPatProp::Shorthand(name, _) => names.push(name.to_string()),
                    ObjPatProp::ShorthandAssign(name, _, _) => names.push(name.to_string()),
                    ObjPatProp::Rest(p) => collect_binding_names(p, names),
                }
            }
        }
        PatKind::Assign(p, _) => collect_binding_names(p, names),
        PatKind::Rest(p) => collect_binding_names(p, names),
    }
}

/// Return the first bound identifier span in a pattern (source order).
///
/// Useful when preserving comments attached to inner destructuring bindings.
pub(crate) fn first_binding_span(pat: &Pat) -> Option<Span> {
    match &pat.kind {
        PatKind::Ident(_) => Some(pat.span),
        PatKind::Array(elems) => {
            for elem in elems.iter().flatten() {
                let span = match elem {
                    ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => first_binding_span(p),
                };
                if span.is_some() {
                    return span;
                }
            }
            None
        }
        PatKind::Object(props) => {
            for prop in props {
                let span = match prop {
                    ObjPatProp::KeyValue(_, val) => first_binding_span(val),
                    ObjPatProp::Shorthand(_, span) | ObjPatProp::ShorthandAssign(_, _, span) => {
                        Some(*span)
                    }
                    ObjPatProp::Rest(p) => first_binding_span(p),
                };
                if span.is_some() {
                    return span;
                }
            }
            None
        }
        PatKind::Assign(p, _) | PatKind::Rest(p) => first_binding_span(p),
    }
}

/// Collect (binding_name, access_expression) pairs for namespace destructuring.
///
/// Given a pattern and a temp variable name, produces pairs like:
/// - Array `[a, b]` with tmp `_a` → `[("a", "_a[0]"), ("b", "_a[1]")]`
/// - Object `{x, y}` with tmp `_a` → `[("x", "_a.x"), ("y", "_a.y")]`
/// - Object `{x: a}` with tmp `_a` → `[("a", "_a.x")]`
pub(crate) fn collect_destructure_accesses(pat: &Pat, tmp: &str, out: &mut Vec<(String, String)>) {
    match &pat.kind {
        PatKind::Array(elems) => {
            let mut idx = 0usize;
            for elem in elems {
                match elem {
                    Some(ArrayPatElem::Pat(p)) => {
                        match &p.kind {
                            PatKind::Ident(name) => {
                                out.push((name.to_string(), format!("{}[{}]", tmp, idx)));
                            }
                            PatKind::Assign(inner, _) => {
                                // Default value: `[a = 1]` → still access by index
                                let mut names = Vec::new();
                                collect_binding_names(inner, &mut names);
                                for name in names {
                                    out.push((name, format!("{}[{}]", tmp, idx)));
                                }
                            }
                            _ => {
                                // Nested pattern — just collect names with index access
                                let mut names = Vec::new();
                                collect_binding_names(p, &mut names);
                                for name in names {
                                    out.push((name, format!("{}[{}]", tmp, idx)));
                                }
                            }
                        }
                        idx += 1;
                    }
                    Some(ArrayPatElem::Rest(p)) => {
                        let mut names = Vec::new();
                        collect_binding_names(p, &mut names);
                        for name in names {
                            out.push((name, format!("{}.slice({})", tmp, idx)));
                        }
                    }
                    None => {
                        // Elision (hole): skip index
                        idx += 1;
                    }
                }
            }
        }
        PatKind::Object(props) => {
            // Collect excluded keys for __rest: all non-rest property keys.
            let mut excluded_keys: Vec<String> = Vec::new();
            for prop in props {
                match prop {
                    ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) => {
                        excluded_keys.push(format!("\"{}\"", name));
                    }
                    ObjPatProp::KeyValue(key, _) => {
                        match key {
                            PropName::Ident(s, _) | PropName::String(s, _) => {
                                excluded_keys.push(format!("\"{}\"", s));
                            }
                            PropName::Number(s, _) => {
                                excluded_keys.push(format!("\"{}\"", s));
                            }
                            PropName::Computed(expr, _) => {
                                // For computed keys, use the expression text
                                match &expr.kind {
                                    ExprKind::StrLit(s) => {
                                        excluded_keys.push(format!("\"{}\"", s));
                                    }
                                    _ => {
                                        // Can't statically determine the key
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ObjPatProp::Rest(_) => {} // Skip rest
                }
            }
            for prop in props {
                match prop {
                    ObjPatProp::Shorthand(name, _) => {
                        out.push((name.to_string(), format!("{}.{}", tmp, name)));
                    }
                    ObjPatProp::ShorthandAssign(name, _, _) => {
                        out.push((name.to_string(), format!("{}.{}", tmp, name)));
                    }
                    ObjPatProp::KeyValue(key, val) => {
                        let key_str = match key {
                            PropName::Ident(s, _) | PropName::String(s, _) => s.clone(),
                            PropName::Number(s, _) => s.clone(),
                            _ => continue,
                        };
                        let mut names = Vec::new();
                        collect_binding_names(val, &mut names);
                        for name in names {
                            out.push((name, format!("{}.{}", tmp, key_str)));
                        }
                    }
                    ObjPatProp::Rest(p) => {
                        let mut names = Vec::new();
                        collect_binding_names(p, &mut names);
                        let keys_str = excluded_keys.join(", ");
                        for name in names {
                            out.push((name, format!("__rest({}, [{}])", tmp, keys_str)));
                        }
                    }
                }
            }
        }
        PatKind::Ident(name) => {
            out.push((name.to_string(), tmp.to_string()));
        }
        PatKind::Assign(inner, _) => {
            collect_destructure_accesses(inner, tmp, out);
        }
        PatKind::Rest(inner) => {
            collect_destructure_accesses(inner, tmp, out);
        }
    }
}

/// For a single-property object destructuring pattern like `{ toString }` or
/// `{ x: y }`, return the property key that should be used for property access
/// on the initializer when lowering to `exports.name = init.key;`.
/// Returns `None` for non-object patterns, multi-property patterns, or patterns
/// with rest elements.
pub(crate) fn single_obj_destructure_prop_key(pat: &Pat) -> Option<String> {
    if let PatKind::Object(props) = &pat.kind {
        if props.len() == 1 {
            match &props[0] {
                ObjPatProp::Shorthand(name, _) => return Some(name.to_string()),
                ObjPatProp::ShorthandAssign(name, _, _) => return Some(name.to_string()),
                ObjPatProp::KeyValue(key, _) => {
                    if let Some(name) = key.ident_name() {
                        return Some(name.to_string());
                    }
                }
                ObjPatProp::Rest(_) => {}
            }
        }
    }
    None
}

/// For single-binding destructuring patterns, determine the access suffix needed
/// to extract the value from the initializer expression.
/// - `{ a: name }` → `DestructureAccess::Member("a")`
/// - `[name]` → `DestructureAccess::Index(0)`
/// - simple ident or multi-binding → `DestructureAccess::None`
#[derive(Debug, Clone)]
pub(crate) enum DestructureAccess {
    None,
    Member(String),
    Index(usize),
}

pub(crate) fn single_destructure_access(pat: &Pat) -> DestructureAccess {
    if let Some(prop) = single_obj_destructure_prop_key(pat) {
        return DestructureAccess::Member(prop);
    }
    if let PatKind::Array(elems) = &pat.kind {
        if elems.len() == 1 {
            if let Some(ArrayPatElem::Pat(p)) = &elems[0] {
                if matches!(&p.kind, PatKind::Ident(_)) {
                    return DestructureAccess::Index(0);
                }
            }
        }
    }
    DestructureAccess::None
}

/// Collect names from `export var/let/const` declarations in a namespace body.
/// Only variable declarations need qualification because class/function/enum
/// declarations retain their local binding in the IIFE (e.g. `class A {} NS.A = A;`),
/// while `export var x = init;` becomes `NS.x = init;` with no local `x`.
pub(crate) fn collect_namespace_exports(stmts: &[Stmt]) -> HashSet<String> {
    let mut exports = HashSet::new();
    let type_only_import_equals = collect_ns_type_only_import_equals(stmts);
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Var(var_stmt) if var_stmt.modifiers & MOD_EXPORT != 0 => {
                for decl in &var_stmt.declarations {
                    let mut names = Vec::new();
                    collect_binding_names(&decl.name, &mut names);
                    for n in names {
                        exports.insert(n);
                    }
                }
            }
            StmtKind::Export(export_decl) => {
                if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                    match &inner.kind {
                        StmtKind::Var(var_stmt) => {
                            for decl in &var_stmt.declarations {
                                let mut names = Vec::new();
                                collect_binding_names(&decl.name, &mut names);
                                for n in names {
                                    exports.insert(n);
                                }
                            }
                        }
                        StmtKind::ImportEquals(ie) => {
                            if !type_only_import_equals.contains(&ie.name) {
                                exports.insert(ie.name.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    exports
}

/// Collect ALL exported names (vars, classes, functions, enums) from a namespace body.
/// Used for cumulative tracking across namespace reopenings: when a namespace is
/// reopened, exports from previous openings need qualification because they have
/// no local binding in the new IIFE scope.
pub(crate) fn collect_all_namespace_exports(stmts: &[Stmt]) -> HashSet<String> {
    let mut exports = HashSet::new();
    let type_only_import_equals = collect_ns_type_only_import_equals(stmts);
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Var(v) if v.modifiers & MOD_EXPORT != 0 => {
                for decl in &v.declarations {
                    let mut names = Vec::new();
                    collect_binding_names(&decl.name, &mut names);
                    exports.extend(names);
                }
            }
            StmtKind::ModuleDecl(m)
                if m.modifiers & MOD_EXPORT != 0 && !module_decl_is_type_only_standalone(m) =>
            {
                if let ModuleName::Ident(name) = &m.name {
                    exports.insert(name.clone());
                }
            }
            StmtKind::ClassDecl(c) if c.modifiers & MOD_EXPORT != 0 => {
                if let Some(name) = &c.name {
                    exports.insert(name.clone());
                }
            }
            StmtKind::FnDecl(f) if f.modifiers & MOD_EXPORT != 0 => {
                if let Some(name) = &f.name {
                    exports.insert(name.clone());
                }
            }
            StmtKind::EnumDecl(e) if e.modifiers & MOD_EXPORT != 0 => {
                exports.insert(e.name.clone());
            }
            StmtKind::Export(export_decl) => {
                if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                    match &inner.kind {
                        StmtKind::Var(v) => {
                            for decl in &v.declarations {
                                let mut names = Vec::new();
                                collect_binding_names(&decl.name, &mut names);
                                exports.extend(names);
                            }
                        }
                        StmtKind::ClassDecl(c) => {
                            if let Some(name) = &c.name {
                                exports.insert(name.clone());
                            }
                        }
                        StmtKind::FnDecl(f) => {
                            if let Some(name) = &f.name {
                                exports.insert(name.clone());
                            }
                        }
                        StmtKind::EnumDecl(e) => {
                            exports.insert(e.name.clone());
                        }
                        StmtKind::ModuleDecl(m) if !module_decl_is_type_only_standalone(m) => {
                            if let ModuleName::Ident(name) = &m.name {
                                exports.insert(name.clone());
                            }
                        }
                        StmtKind::ImportEquals(ie) => {
                            if !type_only_import_equals.contains(&ie.name) {
                                exports.insert(ie.name.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    exports
}

/// Collect all type-only exported names from a namespace body.
/// Returns names that are exported interfaces, type aliases, or type-only-standalone
/// namespaces. Used to distinguish "member is a declared type-only export" from
/// "member is an undeclared property" in import-equals elision.
pub(crate) fn collect_namespace_type_only_exports(stmts: &[Stmt]) -> HashSet<String> {
    let mut type_exports = HashSet::new();
    let type_only_import_equals = collect_ns_type_only_import_equals(stmts);
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::InterfaceDecl(i) if i.modifiers & MOD_EXPORT != 0 => {
                type_exports.insert(i.name.clone());
            }
            StmtKind::TypeAlias(t) if t.modifiers & MOD_EXPORT != 0 => {
                type_exports.insert(t.name.clone());
            }
            StmtKind::ModuleDecl(m)
                if m.modifiers & MOD_EXPORT != 0 && module_decl_is_type_only_standalone(m) =>
            {
                if let ModuleName::Ident(name) = &m.name {
                    type_exports.insert(name.clone());
                }
            }
            StmtKind::Export(export_decl) => {
                if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                    match &inner.kind {
                        StmtKind::InterfaceDecl(i) => {
                            type_exports.insert(i.name.clone());
                        }
                        StmtKind::TypeAlias(t) => {
                            type_exports.insert(t.name.clone());
                        }
                        StmtKind::ModuleDecl(m) if module_decl_is_type_only_standalone(m) => {
                            if let ModuleName::Ident(name) = &m.name {
                                type_exports.insert(name.clone());
                            }
                        }
                        // `export import X = N;` where X is type-only (N is an
                        // empty namespace or only referenced in type position).
                        StmtKind::ImportEquals(ie) => {
                            if type_only_import_equals.contains(&ie.name) {
                                type_exports.insert(ie.name.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    type_exports
}

/// Pre-scan all namespace declarations in the file to build cumulative export maps.
/// This ensures that when the first opening of a merged namespace is emitted,
/// it already knows about exports from all subsequent reopenings.
pub(crate) fn prescan_namespace_exports(
    stmts: &[Stmt],
    parent_key: Option<&str>,
    cumulative: &mut HashMap<AstString, HashSet<AstString>>,
) {
    prescan_namespace_exports_full(stmts, parent_key, cumulative, None);
}

/// Pre-scan namespace exports, optionally also collecting type-only exports.
pub(crate) fn prescan_namespace_exports_full(
    stmts: &[Stmt],
    parent_key: Option<&str>,
    cumulative: &mut HashMap<AstString, HashSet<AstString>>,
    type_cumulative: Option<&mut HashMap<AstString, HashSet<AstString>>>,
) {
    // Collect module decls first (to avoid borrow issues with the mutable ref)
    let module_decls: Vec<&ModuleDecl> = stmts
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::ModuleDecl(m) => Some(m.as_ref()),
            StmtKind::Export(e) => {
                if let ExportDeclKind::Decl(inner) = &e.kind {
                    if let StmtKind::ModuleDecl(m) = &inner.kind {
                        Some(m.as_ref())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        })
        .filter(|m| m.modifiers & MOD_DECLARE == 0)
        .collect();

    // Process each module decl
    if let Some(type_cum) = type_cumulative {
        for module_decl in &module_decls {
            prescan_module_decl_full(module_decl, parent_key, cumulative, Some(type_cum));
        }
    } else {
        for module_decl in &module_decls {
            prescan_module_decl(module_decl, parent_key, cumulative);
        }
    }
}

/// Add runtime exports that flow through aliases in ambient namespaces.
/// Ambient namespaces do not emit IIFEs, so the regular namespace pre-scan
/// intentionally skips them. Their value exports still determine whether an
/// `export import X = Namespace.member` alias has runtime meaning.
pub(crate) fn prescan_declare_namespace_value_exports(
    stmts: &[Stmt],
    cumulative: &mut HashMap<AstString, HashSet<AstString>>,
) {
    #[derive(Default)]
    struct NamespaceInfo {
        direct_values: HashSet<String>,
        aliases: HashMap<String, String>,
        exported_locals: Vec<(String, String)>,
    }

    fn member_path(expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::Member(member) => {
                let mut path = member_path(&member.object)?;
                path.push('.');
                path.push_str(&member.property);
                Some(path)
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => member_path(inner),
            ExprKind::TypeAssertion(assertion) => member_path(&assertion.expr),
            ExprKind::As(assertion) => member_path(&assertion.expr),
            ExprKind::Satisfies(assertion) => member_path(&assertion.expr),
            ExprKind::Instantiation(instantiation) => member_path(&instantiation.expr),
            _ => None,
        }
    }

    fn record_stmt(stmt: &Stmt, exported: bool, info: &mut NamespaceInfo) {
        let mut declared = Vec::new();
        match &stmt.kind {
            StmtKind::Var(var_stmt) => {
                for decl in &var_stmt.declarations {
                    collect_binding_names(&decl.name, &mut declared);
                }
            }
            StmtKind::FnDecl(function) => {
                if let Some(name) = &function.name {
                    declared.push(name.clone());
                }
            }
            StmtKind::ClassDecl(class) => {
                if let Some(name) = &class.name {
                    declared.push(name.clone());
                }
            }
            StmtKind::EnumDecl(enum_decl) => declared.push(enum_decl.name.clone()),
            StmtKind::ModuleDecl(module_decl)
                if declare_namespace_has_value_exports(module_decl) =>
            {
                if let ModuleName::Ident(name) = &module_decl.name {
                    declared.push(name.clone());
                }
            }
            StmtKind::ImportEquals(import_equals) => {
                if let Some(path) = member_path(&import_equals.module_ref) {
                    info.aliases.insert(import_equals.name.clone(), path);
                    if exported {
                        info.exported_locals
                            .push((import_equals.name.clone(), import_equals.name.clone()));
                    }
                }
                return;
            }
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(inner) => {
                    record_stmt(inner, true, info);
                    return;
                }
                ExportDeclKind::Named {
                    specifiers,
                    source: None,
                    type_only: false,
                } => {
                    for specifier in specifiers {
                        if !specifier.is_type {
                            info.exported_locals.push((
                                specifier
                                    .exported
                                    .clone()
                                    .unwrap_or_else(|| specifier.local.clone()),
                                specifier.local.clone(),
                            ));
                        }
                    }
                    return;
                }
                _ => return,
            },
            _ => return,
        }

        for name in declared {
            info.direct_values.insert(name.clone());
            if exported {
                info.exported_locals.push((name.clone(), name));
            }
        }
    }

    let mut namespaces: HashMap<String, NamespaceInfo> = HashMap::new();
    for stmt in stmts {
        let module_decl = match &stmt.kind {
            StmtKind::ModuleDecl(module_decl) => Some(module_decl.as_ref()),
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(inner) => match &inner.kind {
                    StmtKind::ModuleDecl(module_decl) => Some(module_decl.as_ref()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let Some(module_decl) = module_decl else {
            continue;
        };
        if module_decl.modifiers & MOD_DECLARE == 0 {
            continue;
        }
        let ModuleName::Ident(name) = &module_decl.name else {
            continue;
        };
        let Some(ModuleBody::Block(body)) = &module_decl.body else {
            continue;
        };
        let info = namespaces.entry(name.clone()).or_default();
        for body_stmt in body {
            record_stmt(body_stmt, false, info);
        }
    }

    let mut resolved_locals: HashMap<String, HashSet<String>> = namespaces
        .iter()
        .map(|(name, info)| (name.clone(), info.direct_values.clone()))
        .collect();
    let mut runtime_exports: HashMap<String, HashSet<String>> = HashMap::new();
    loop {
        let mut changed = false;
        for (namespace, info) in &namespaces {
            for (local, path) in &info.aliases {
                let mut parts = path.split('.');
                let root = parts.next().unwrap_or_default();
                let first_member = parts.next();
                let resolves_to_value = match first_member {
                    Some(member) => runtime_exports
                        .get(root)
                        .is_some_and(|exports| exports.contains(member)),
                    None => runtime_exports
                        .get(root)
                        .is_some_and(|exports| !exports.is_empty()),
                };
                if resolves_to_value
                    && resolved_locals
                        .entry(namespace.clone())
                        .or_default()
                        .insert(local.clone())
                {
                    changed = true;
                }
            }
            for (exported, local) in &info.exported_locals {
                if resolved_locals
                    .get(namespace)
                    .is_some_and(|locals| locals.contains(local))
                    && runtime_exports
                        .entry(namespace.clone())
                        .or_default()
                        .insert(exported.clone())
                {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    for (namespace, exports) in runtime_exports {
        cumulative
            .entry(AstString::from(namespace))
            .or_default()
            .extend(exports.into_iter().map(AstString::from));
    }
}

/// Recursively pre-scan a single module declaration (handles dotted namespaces).
/// `parent_key` is the key prefix for building the cumulative map key (matches
/// the `prev_target` in the emission code, e.g. "exports" at top level CJS).
pub(crate) fn prescan_module_decl(
    module_decl: &ModuleDecl,
    parent_key: Option<&str>,
    cumulative: &mut HashMap<AstString, HashSet<AstString>>,
) {
    prescan_module_decl_full(module_decl, parent_key, cumulative, None);
}

/// Recursively pre-scan a single module declaration, optionally collecting type-only exports.
pub(crate) fn prescan_module_decl_full(
    module_decl: &ModuleDecl,
    parent_key: Option<&str>,
    cumulative: &mut HashMap<AstString, HashSet<AstString>>,
    type_cumulative: Option<&mut HashMap<AstString, HashSet<AstString>>>,
) {
    let name = match &module_decl.name {
        ModuleName::Ident(n) => n.clone(),
        ModuleName::String(n) => n.clone(),
    };
    let cumulative_key: AstString = match parent_key {
        Some(parent) => format!("{}::{}", parent, name).into(),
        None => name.clone().into(),
    };
    match module_decl.body.as_ref() {
        Some(ModuleBody::Block(body_stmts)) => {
            let exports = collect_all_namespace_exports(body_stmts);
            cumulative
                .entry(cumulative_key.clone())
                .or_default()
                .extend(exports.into_iter().map(AstString::from));
            if let Some(type_cum) = type_cumulative {
                let type_exports = collect_namespace_type_only_exports(body_stmts);
                type_cum
                    .entry(cumulative_key.clone())
                    .or_default()
                    .extend(type_exports.into_iter().map(AstString::from));
                // Recurse into nested namespaces.
                prescan_namespace_exports_full(body_stmts, Some(&name), cumulative, Some(type_cum));
            } else {
                prescan_namespace_exports(body_stmts, Some(&name), cumulative);
            }
        }
        Some(ModuleBody::Module(inner)) => {
            // Dotted namespace: `namespace A.B.C` → Module("A", Module("B", Block(...)))
            // The intermediate "A" level exports "B" as a sub-namespace.
            cumulative
                .entry(cumulative_key.clone())
                .or_default()
                .insert(AstString::from(match &inner.name {
                    ModuleName::Ident(n) => n.clone(),
                    ModuleName::String(n) => n.clone(),
                }));
            // For dotted namespaces, use the namespace name as parent key.
            if let Some(type_cum) = type_cumulative {
                prescan_module_decl_full(inner, Some(&name), cumulative, Some(type_cum));
            } else {
                prescan_module_decl(inner, Some(&name), cumulative);
            }
        }
        None => {}
    }
}

/// Returns true if a statement has the `export` modifier flag.
/// These are declarations like `export class C {}` where the parser consumes
/// `export` as a modifier rather than wrapping in `StmtKind::Export`.
pub(crate) fn stmt_has_export_modifier(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ClassDecl(c) => c.modifiers & MOD_EXPORT != 0,
        StmtKind::FnDecl(f) => f.modifiers & MOD_EXPORT != 0,
        StmtKind::Var(v) => v.modifiers & MOD_EXPORT != 0,
        StmtKind::EnumDecl(e) => e.modifiers & MOD_EXPORT != 0,
        StmtKind::InterfaceDecl(i) => i.modifiers & MOD_EXPORT != 0,
        StmtKind::TypeAlias(t) => t.modifiers & MOD_EXPORT != 0,
        StmtKind::ModuleDecl(m) => m.modifiers & MOD_EXPORT != 0,
        _ => false,
    }
}

/// Collect exported names from a statement with the `MOD_EXPORT` modifier.
pub(crate) fn collect_modifier_export_names(stmt: &Stmt, names: &mut Vec<String>) {
    match &stmt.kind {
        StmtKind::ClassDecl(c)
            if c.modifiers & MOD_EXPORT != 0 && c.modifiers & MOD_DECLARE == 0 =>
        {
            if let Some(ref name) = c.name {
                names.push(name.clone());
            }
        }
        StmtKind::FnDecl(_) => {
            // Functions are hoisted — handled by collect_modifier_fn_export_names.
        }
        StmtKind::Var(v) if v.modifiers & MOD_EXPORT != 0 && v.modifiers & MOD_DECLARE == 0 => {
            for decl in &v.declarations {
                collect_binding_names(&decl.name, names);
            }
        }
        StmtKind::EnumDecl(e)
            if e.modifiers & MOD_EXPORT != 0 && e.modifiers & MOD_DECLARE == 0 =>
        {
            names.push(e.name.clone());
        }
        StmtKind::ModuleDecl(m)
            if m.modifiers & MOD_EXPORT != 0 && m.modifiers & MOD_DECLARE == 0 =>
        {
            let name = match &m.name {
                ModuleName::Ident(n) => n.clone(),
                ModuleName::String(n) => n.clone(),
            };
            names.push(name);
        }
        StmtKind::ImportEquals(ie)
            if ie.modifiers & MOD_EXPORT != 0 && ie.modifiers & MOD_DECLARE == 0 =>
        {
            names.push(ie.name.clone());
        }
        _ => {}
    }
}

/// Collect exported function names for direct `exports.name = name;` assignment
/// (functions are hoisted and don't need `void 0` pre-declarations).
pub(crate) fn collect_fn_export_names(
    kind: &ExportDeclKind,
    names: &mut Vec<(String, String)>,
    default_counter: &mut usize,
) {
    match kind {
        ExportDeclKind::Decl(decl) => {
            if let StmtKind::FnDecl(fn_decl) = &decl.kind {
                if fn_decl.body.is_some() && fn_decl.modifiers & MOD_DECLARE == 0 {
                    if let Some(ref name) = fn_decl.name {
                        names.push((name.clone(), name.clone()));
                    }
                }
            }
            // Anonymous exported class (e.g. `export class { }`) also consumes
            // a counter slot, matching the emission phase in emit_export_decl_cjs.
            if let StmtKind::ClassDecl(class_decl) = &decl.kind {
                if class_decl.modifiers & MOD_DECLARE == 0 && class_decl.name.is_none() {
                    *default_counter += 1;
                }
            }
        }
        ExportDeclKind::DefaultDecl(decl) => match &decl.kind {
            StmtKind::FnDecl(fn_decl) => {
                if fn_decl.body.is_some() && fn_decl.modifiers & MOD_DECLARE == 0 {
                    let local = fn_decl.name.clone().unwrap_or_else(|| {
                        let name = format!("default_{}", *default_counter);
                        *default_counter += 1;
                        name
                    });
                    names.push(("default".to_string(), local));
                }
            }
            StmtKind::ClassDecl(class_decl) => {
                // Anonymous default class exports also consume a counter slot.
                if class_decl.modifiers & MOD_DECLARE == 0 && class_decl.name.is_none() {
                    *default_counter += 1;
                }
            }
            _ => {}
        },
        _ => {}
    }
}

pub(crate) fn collect_modifier_fn_export_names(stmt: &Stmt, names: &mut Vec<(String, String)>) {
    if let StmtKind::FnDecl(f) = &stmt.kind {
        if f.modifiers & MOD_EXPORT != 0 && f.body.is_some() && f.modifiers & MOD_DECLARE == 0 {
            if let Some(ref name) = f.name {
                names.push((name.clone(), name.clone()));
            }
        }
    }
}

/// Returns true if an import declaration binds the given local name.
pub(crate) fn import_decl_binds_local(import_decl: &ImportDecl, local: &str) -> bool {
    if import_decl.type_only {
        return false;
    }
    match &import_decl.specifiers {
        ImportClause::Named {
            default,
            named,
            namespace,
        } => {
            if default.as_deref() == Some(local) {
                return true;
            }
            if namespace.as_deref() == Some(local) {
                return true;
            }
            named.iter().any(|s| !s.is_type && s.local == local)
        }
        ImportClause::Require(name) => name == local,
    }
}

/// Collect decorator and param-decorator strings for a class method member.
/// Parameter decorators (wrapped in __param) come first, then member decorators.
/// Extract a trailing `// ...` comment on the same line after a span in the source.
/// Returns the comment text (including `//`) or empty string if none found.
pub(crate) fn trailing_line_comment(source: &str, span_end: u32) -> &str {
    let end = span_end as usize;
    if end >= source.len() {
        return "";
    }
    let rest = &source[end..];
    // Find end of current line
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let on_line = &rest[..line_end];
    // Look for `//` comment start (skip whitespace)
    let trimmed = on_line.trim_start();
    if trimmed.starts_with("//") {
        trimmed
    } else {
        ""
    }
}

pub(crate) fn collect_member_decorator_strings(
    source: &str,
    options: &CompilerOptions,
    decorators: &[Expr],
    params: &[Param],
    helper_prefix: &str,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    import_shadows: &crate::import_shadow::ImportShadows,
) -> Vec<String> {
    collect_member_decorator_strings_inner(
        source,
        options,
        decorators,
        params,
        helper_prefix,
        cjs_import_map,
        cjs_string_import_locals,
        import_shadows,
        false,
    )
}

pub(crate) fn collect_member_decorator_strings_await_to_yield(
    source: &str,
    options: &CompilerOptions,
    decorators: &[Expr],
    params: &[Param],
    helper_prefix: &str,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    import_shadows: &crate::import_shadow::ImportShadows,
) -> Vec<String> {
    collect_member_decorator_strings_inner(
        source,
        options,
        decorators,
        params,
        helper_prefix,
        cjs_import_map,
        cjs_string_import_locals,
        import_shadows,
        true,
    )
}

fn collect_member_decorator_strings_inner(
    source: &str,
    options: &CompilerOptions,
    decorators: &[Expr],
    params: &[Param],
    helper_prefix: &str,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    import_shadows: &crate::import_shadow::ImportShadows,
    await_to_yield: bool,
) -> Vec<String> {
    let mut result = Vec::new();
    let remove_comments = options.remove_comments == Some(true);
    let emit_fn = if await_to_yield {
        emit_expr_to_string_await_to_yield
    } else {
        emit_expr_to_string
    };
    // Member decorators first
    for dec in decorators {
        let mut s = emit_fn(
            source,
            options,
            dec,
            cjs_import_map,
            cjs_string_import_locals,
            import_shadows,
        );
        if !remove_comments {
            let comment = trailing_line_comment(source, dec.span.end);
            if !comment.is_empty() {
                s.push(' ');
                s.push_str(comment);
            }
        }
        result.push(s);
    }
    // Then parameter decorators (wrapped in __param)
    // Skip `this` parameters when computing the index — they are erased at runtime.
    // The parser may represent `this` as either "this" or "<error>".
    let mut param_idx = 0usize;
    for param in params.iter() {
        let is_this =
            matches!(&param.name.kind, PatKind::Ident(name) if name == "this" || name == "<error>");
        if is_this {
            continue;
        }
        for dec in &param.decorators {
            let dec_str = emit_fn(
                source,
                options,
                dec,
                cjs_import_map,
                cjs_string_import_locals,
                import_shadows,
            );
            result.push(format!(
                "{}__param({}, {})",
                helper_prefix, param_idx, dec_str
            ));
        }
        param_idx += 1;
    }
    result
}

/// Check if a class declaration has any decorators (class-level, member, or parameter).
/// Check if any member decorator in a class references a private field name
/// (e.g. `@decorator((x) => x.#x)`). When this is true, `__decorate` calls
/// must be emitted inside a `static {}` block to access the private name.
pub(crate) fn class_decorators_reference_private_names(class: &ClassDecl, source: &str) -> bool {
    for member in &class.members {
        let decorators: &[Expr] = match &member.kind {
            ClassMemberKind::Method(m) => &m.decorators,
            ClassMemberKind::Property(p) => &p.decorators,
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => &a.decorators,
            _ => continue,
        };
        for dec in decorators {
            let s = dec.span.start as usize;
            let e = dec.span.end as usize;
            if s < e && e <= source.len() && source[s..e].contains('#') {
                return true;
            }
        }
        // Also check parameter decorators
        if let ClassMemberKind::Method(m) = &member.kind {
            for param in &m.params {
                for dec in &param.decorators {
                    let s = dec.span.start as usize;
                    let e = dec.span.end as usize;
                    if s < e && e <= source.len() && source[s..e].contains('#') {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub(crate) fn class_has_decorators(class: &ClassDecl) -> bool {
    if !class.decorators.is_empty() {
        return true;
    }
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Method(m) => {
                // Private names cannot be decorated with legacy decorators
                if matches!(&m.name, PropName::Private(..)) {
                    continue;
                }
                if !m.decorators.is_empty() {
                    return true;
                }
                for p in &m.params {
                    if !p.decorators.is_empty() {
                        return true;
                    }
                }
            }
            ClassMemberKind::Property(p) => {
                if matches!(&p.name, PropName::Private(..)) {
                    continue;
                }
                if !p.decorators.is_empty() {
                    return true;
                }
            }
            ClassMemberKind::Constructor(ctor) => {
                if !ctor.decorators.is_empty() {
                    return true;
                }
                for p in &ctor.params {
                    if !p.decorators.is_empty() {
                        return true;
                    }
                }
            }
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                if matches!(&a.name, PropName::Private(..)) {
                    continue;
                }
                if !a.decorators.is_empty() {
                    return true;
                }
                for p in &a.params {
                    if !p.decorators.is_empty() {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Narrow standard-decorator support used for `importHelpers` class wrappers.
///
/// Class-level standard decorator IIFE wrapper: `let X = (() => { ... })();`
/// Targets named classes with class-level decorators, no extends, no type params.
/// Supports classes with public members (methods, properties, static blocks).
/// Excludes classes with private members or per-member decorators (need more complex handling).
pub(crate) fn class_can_emit_simple_standard_decorator_wrapper(class: &ClassDecl) -> bool {
    if class.name.is_none()
        || class.decorators.is_empty()
        || class.type_params.is_some()
        || !class.implements.is_empty()
    {
        return false;
    }
    // Exclude classes with private members or per-member decorators
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                if matches!(prop.name, PropName::Private(_, _)) || !prop.decorators.is_empty() {
                    return false;
                }
            }
            ClassMemberKind::Method(method) => {
                if matches!(method.name, PropName::Private(_, _)) {
                    return false;
                }
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if matches!(acc.name, PropName::Private(_, _)) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

/// Check if a class expression can use the simple IIFE wrapper for standard
/// (TC39) decorators. Supports anonymous classes; requires no extends,
/// no private members, and no per-member decorators.
pub(crate) fn class_expr_can_emit_simple_standard_decorator_wrapper(class: &ClassDecl) -> bool {
    if class.decorators.is_empty() || class.type_params.is_some() || !class.implements.is_empty() {
        return false;
    }
    // Exclude classes with private members or per-member decorators
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                if matches!(prop.name, PropName::Private(_, _)) || !prop.decorators.is_empty() {
                    return false;
                }
            }
            ClassMemberKind::Method(method) => {
                if matches!(method.name, PropName::Private(_, _)) {
                    return false;
                }
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if matches!(acc.name, PropName::Private(_, _)) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeStandardDecoratorClassShape {
    DecoratedPrivateField {
        is_static: bool,
        is_auto_accessor: bool,
    },
    DecoratedPrivateMethod {
        is_static: bool,
    },
    DecoratedPrivateGetter {
        is_static: bool,
    },
    DecoratedPrivateSetter {
        is_static: bool,
    },
    DecoratedStaticComputedField,
    DecoratedStaticComputedAutoAccessor,
    DecoratedStaticComputedMethod,
    DecoratedStaticComputedGetter,
    DecoratedStaticComputedSetter,
    ClassDecoratorWithPrivateStaticMethod,
}

impl NativeStandardDecoratorClassShape {
    pub(crate) fn needs_prop_key_helper(self) -> bool {
        matches!(
            self,
            NativeStandardDecoratorClassShape::DecoratedStaticComputedField
                | NativeStandardDecoratorClassShape::DecoratedStaticComputedAutoAccessor
                | NativeStandardDecoratorClassShape::DecoratedStaticComputedMethod
                | NativeStandardDecoratorClassShape::DecoratedStaticComputedGetter
                | NativeStandardDecoratorClassShape::DecoratedStaticComputedSetter
        )
    }

    pub(crate) fn needs_set_function_name_helper(self) -> bool {
        matches!(
            self,
            NativeStandardDecoratorClassShape::DecoratedPrivateMethod { .. }
                | NativeStandardDecoratorClassShape::DecoratedPrivateGetter { .. }
                | NativeStandardDecoratorClassShape::DecoratedPrivateSetter { .. }
                | NativeStandardDecoratorClassShape::DecoratedPrivateField {
                    is_auto_accessor: true,
                    ..
                }
                | NativeStandardDecoratorClassShape::ClassDecoratorWithPrivateStaticMethod
        )
    }
}

fn stmts_are_empty(body: Option<&Vec<Stmt>>) -> bool {
    body.is_some_and(|stmts| stmts.is_empty())
}

fn accessor_body_return_expr(body: Option<&Vec<Stmt>>) -> Option<&Expr> {
    let stmts = body?;
    if stmts.len() != 1 {
        return None;
    }
    let StmtKind::Return(Some(expr)) = &stmts[0].kind else {
        return None;
    };
    Some(expr)
}

fn class_non_semicolon_members<'a>(class: &'a ClassDecl) -> Vec<&'a ClassMember> {
    class
        .members
        .iter()
        .filter(|member| !matches!(member.kind, ClassMemberKind::SemicolonClassElement))
        .collect()
}

pub(crate) fn class_native_standard_decorator_shape(
    class: &ClassDecl,
) -> Option<NativeStandardDecoratorClassShape> {
    if class.name.is_none()
        || class.extends.is_some()
        || class.type_params.is_some()
        || !class.implements.is_empty()
    {
        return None;
    }

    let members = class_non_semicolon_members(class);
    if members.len() != 1 {
        return None;
    }
    let member = members[0];

    if !class.decorators.is_empty() {
        let ClassMemberKind::Method(method) = &member.kind else {
            return None;
        };
        if !method.decorators.is_empty()
            || !matches!(method.name, PropName::Private(_, _))
            || method.modifiers & MOD_STATIC == 0
            || !method.params.is_empty()
            || method.is_async
            || method.is_generator
            || !stmts_are_empty(method.body.as_ref())
        {
            return None;
        }
        return Some(NativeStandardDecoratorClassShape::ClassDecoratorWithPrivateStaticMethod);
    }

    match &member.kind {
        ClassMemberKind::Property(prop) => {
            if prop.decorators.is_empty()
                || prop.modifiers & (MOD_DECLARE | MOD_ABSTRACT) != 0
                || prop.initializer.is_some()
            {
                return None;
            }
            match &prop.name {
                PropName::Private(_, _) => {
                    Some(NativeStandardDecoratorClassShape::DecoratedPrivateField {
                        is_static: prop.modifiers & MOD_STATIC != 0,
                        is_auto_accessor: prop.modifiers & MOD_ACCESSOR != 0,
                    })
                }
                PropName::Computed(_, _) => {
                    if prop.modifiers & MOD_STATIC == 0 {
                        return None;
                    }
                    if prop.modifiers & MOD_ACCESSOR != 0 {
                        Some(NativeStandardDecoratorClassShape::DecoratedStaticComputedAutoAccessor)
                    } else {
                        Some(NativeStandardDecoratorClassShape::DecoratedStaticComputedField)
                    }
                }
                _ => None,
            }
        }
        ClassMemberKind::Method(method) => {
            if method.decorators.is_empty()
                || method
                    .params
                    .iter()
                    .any(|param| !param.decorators.is_empty())
                || method.is_async
                || method.is_generator
                || !stmts_are_empty(method.body.as_ref())
            {
                return None;
            }
            match &method.name {
                PropName::Private(_, _) => {
                    Some(NativeStandardDecoratorClassShape::DecoratedPrivateMethod {
                        is_static: method.modifiers & MOD_STATIC != 0,
                    })
                }
                PropName::Computed(_, _) if method.modifiers & MOD_STATIC != 0 => {
                    Some(NativeStandardDecoratorClassShape::DecoratedStaticComputedMethod)
                }
                _ => None,
            }
        }
        ClassMemberKind::GetAccessor(acc) => {
            if acc.decorators.is_empty()
                || acc.params.iter().any(|param| !param.decorators.is_empty())
                || accessor_body_return_expr(acc.body.as_ref()).is_none()
            {
                return None;
            }
            match &acc.name {
                PropName::Private(_, _) => {
                    Some(NativeStandardDecoratorClassShape::DecoratedPrivateGetter {
                        is_static: acc.modifiers & MOD_STATIC != 0,
                    })
                }
                PropName::Computed(_, _) if acc.modifiers & MOD_STATIC != 0 => {
                    Some(NativeStandardDecoratorClassShape::DecoratedStaticComputedGetter)
                }
                _ => None,
            }
        }
        ClassMemberKind::SetAccessor(acc) => {
            if acc.decorators.is_empty()
                || acc.params.iter().any(|param| !param.decorators.is_empty())
                || !stmts_are_empty(acc.body.as_ref())
            {
                return None;
            }
            match &acc.name {
                PropName::Private(_, _) => {
                    Some(NativeStandardDecoratorClassShape::DecoratedPrivateSetter {
                        is_static: acc.modifiers & MOD_STATIC != 0,
                    })
                }
                PropName::Computed(_, _) if acc.modifiers & MOD_STATIC != 0 => {
                    Some(NativeStandardDecoratorClassShape::DecoratedStaticComputedSetter)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Narrow standard-decorator class-expression support for public decorated
/// fields/accessors. This intentionally targets the crash-regression shape
/// rather than claiming full Stage 3 decorator support.
pub(crate) fn class_can_emit_narrow_standard_decorator_member_expr(class: &ClassDecl) -> bool {
    if !class.decorators.is_empty()
        || class.extends.is_some()
        || class.type_params.is_some()
        || !class.implements.is_empty()
    {
        return false;
    }

    let mut seen_instance_field = false;
    let mut seen_static_field = false;
    let mut seen_instance_accessor = false;
    let mut seen_static_accessor = false;
    let mut saw_supported_member = false;

    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                if prop.decorators.is_empty()
                    || prop.modifiers & (MOD_DECLARE | MOD_ABSTRACT) != 0
                    || prop.name.ident_name().is_none()
                    || matches!(
                        prop.name,
                        PropName::Private(_, _) | PropName::Computed(_, _)
                    )
                {
                    return false;
                }

                let is_static = prop.modifiers & MOD_STATIC != 0;
                let is_accessor = prop.modifiers & MOD_ACCESSOR != 0;
                let slot = match (is_static, is_accessor) {
                    (false, false) => &mut seen_instance_field,
                    (true, false) => &mut seen_static_field,
                    (false, true) => &mut seen_instance_accessor,
                    (true, true) => &mut seen_static_accessor,
                };
                if *slot {
                    return false;
                }
                *slot = true;
                saw_supported_member = true;
            }
            ClassMemberKind::SemicolonClassElement => {}
            _ => return false,
        }
    }

    saw_supported_member
}

/// Narrow standard-decorator class-declaration support for public decorated
/// instance fields. This keeps the supported surface area intentionally small
/// while allowing ordinary constructors and methods to stay intact.
pub(crate) fn class_can_emit_narrow_standard_decorator_member_decl(class: &ClassDecl) -> bool {
    if class.name.is_none()
        || !class.decorators.is_empty()
        || class.extends.is_some()
        || class.type_params.is_some()
        || !class.implements.is_empty()
    {
        return false;
    }

    let mut seen_supported_names = std::collections::HashSet::new();
    let mut saw_supported_member = false;

    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                if prop.decorators.is_empty() {
                    if matches!(prop.name, PropName::Private(_, _)) {
                        return false;
                    }
                    continue;
                }

                if prop.modifiers & (MOD_DECLARE | MOD_ABSTRACT | MOD_STATIC | MOD_ACCESSOR) != 0
                    || prop.name.ident_name().is_none()
                    || matches!(
                        prop.name,
                        PropName::Private(_, _) | PropName::Computed(_, _)
                    )
                {
                    return false;
                }

                let prop_name = prop.name.ident_name().unwrap_or_default().to_string();
                if !seen_supported_names.insert(prop_name) {
                    return false;
                }
                saw_supported_member = true;
            }
            ClassMemberKind::Method(method) => {
                if !method.decorators.is_empty()
                    || method
                        .params
                        .iter()
                        .any(|param| !param.decorators.is_empty())
                    || matches!(method.name, PropName::Private(_, _))
                {
                    return false;
                }
            }
            ClassMemberKind::Constructor(_) | ClassMemberKind::SemicolonClassElement => {}
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if !acc.decorators.is_empty()
                    || acc.params.iter().any(|param| !param.decorators.is_empty())
                    || matches!(acc.name, PropName::Private(_, _))
                {
                    return false;
                }
            }
            ClassMemberKind::IndexSignature(_) => {}
            ClassMemberKind::StaticBlock(_) => return false,
        }
    }

    saw_supported_member
}

/// Check if a class has any decorated methods (method, getter, or setter with decorators).
/// Used to determine helper emission order: decorated methods want __runInitializers first.
pub(crate) fn class_has_decorated_methods(class: &ClassDecl) -> bool {
    class.members.iter().any(|m| match &m.kind {
        ClassMemberKind::Method(method) => !method.decorators.is_empty(),
        ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
            !acc.decorators.is_empty()
        }
        _ => false,
    })
}

/// Check if a class declaration with method-only decorators (no class decorator)
/// can use the method-only IIFE wrapper. Requires:
/// - No class-level decorators
/// - At least one method with decorators
/// - No private/computed names on decorated methods
/// - No field/property decorators (those use the narrow member wrapper)
pub(crate) fn class_can_emit_public_multi_method_decorator_wrapper(class: &ClassDecl) -> bool {
    if class.name.is_none()
        || !class.decorators.is_empty()
        || class.extends.is_some()
        || class.type_params.is_some()
        || !class.implements.is_empty()
    {
        return false;
    }
    let mut saw_decorated_method = false;
    for member in &class.members {
        match &member.kind {
            // Field and auto-accessor initialization has a different initializer
            // protocol. Keep it on the dedicated field paths until the two plans
            // can be composed without changing evaluation order.
            ClassMemberKind::Property(_) => return false,
            ClassMemberKind::Method(method) => {
                if method.body.is_none()
                    || method.modifiers & (MOD_PRIVATE | MOD_ABSTRACT | MOD_DECLARE) != 0
                    || method
                        .params
                        .iter()
                        .any(|param| !param.decorators.is_empty())
                    || !standard_decorator_public_member_name_supported(&method.name)
                    || method.decorators.is_empty()
                    || method
                        .decorators
                        .iter()
                        .any(standard_decorator_expr_needs_receiver_binding)
                {
                    return false;
                }
                saw_decorated_method = true;
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if acc.body.is_none()
                    || acc.modifiers & (MOD_PRIVATE | MOD_ABSTRACT | MOD_DECLARE) != 0
                    || acc.params.iter().any(|param| !param.decorators.is_empty())
                    || !standard_decorator_public_member_name_supported(&acc.name)
                    || acc.decorators.is_empty()
                    || acc
                        .decorators
                        .iter()
                        .any(standard_decorator_expr_needs_receiver_binding)
                {
                    return false;
                }
                saw_decorated_method = true;
            }
            ClassMemberKind::Constructor(ctor) => {
                if ctor.body.is_none()
                    || !ctor.decorators.is_empty()
                    || ctor.params.iter().any(|param| !param.decorators.is_empty())
                    || ctor.params.iter().any(|param| {
                        param.modifiers
                            & (MOD_PUBLIC
                                | MOD_PRIVATE
                                | MOD_PROTECTED
                                | MOD_READONLY
                                | MOD_OVERRIDE)
                            != 0
                    })
                {
                    return false;
                }
            }
            ClassMemberKind::SemicolonClassElement => {}
            ClassMemberKind::IndexSignature(_) | ClassMemberKind::StaticBlock(_) => return false,
        }
    }
    saw_decorated_method
}

/// Check whether the legacy method-only wrapper can handle this class.
///
/// Keep this predicate intentionally unchanged from the original path: the
/// structural public-member wrapper has a narrower safety gate and must not
/// turn previously-supported extends/receiver-binding cases into raw output.
pub(crate) fn class_can_emit_method_only_decorator_wrapper(class: &ClassDecl) -> bool {
    if !class.decorators.is_empty() || class.type_params.is_some() || !class.implements.is_empty() {
        return false;
    }
    let mut saw_decorated_method = false;
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                if !prop.decorators.is_empty() || matches!(prop.name, PropName::Private(_, _)) {
                    return false;
                }
            }
            ClassMemberKind::Method(method) => {
                if matches!(method.name, PropName::Private(_, _)) {
                    return false;
                }
                if !method.decorators.is_empty() {
                    if method.name.ident_name().is_none()
                        || matches!(method.name, PropName::Computed(_, _))
                    {
                        return false;
                    }
                    saw_decorated_method = true;
                }
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if matches!(acc.name, PropName::Private(_, _)) {
                    return false;
                }
                if !acc.decorators.is_empty() {
                    if acc.name.ident_name().is_none()
                        || matches!(acc.name, PropName::Computed(_, _))
                    {
                        return false;
                    }
                    saw_decorated_method = true;
                }
            }
            ClassMemberKind::Constructor(_) | ClassMemberKind::SemicolonClassElement => {}
            _ => {}
        }
    }
    saw_decorated_method
}

fn standard_decorator_expr_needs_receiver_binding(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Paren(inner) => standard_decorator_expr_needs_receiver_binding(inner),
        ExprKind::Member(_) | ExprKind::ElemAccess(_) => true,
        _ => false,
    }
}

fn standard_decorator_public_member_name_supported(name: &PropName) -> bool {
    match name {
        PropName::Ident(..) | PropName::String(..) => true,
        PropName::Computed(expr, _) => standard_decorator_computed_key_expr_supported(expr),
        PropName::Number(..) | PropName::Private(..) => false,
    }
}

fn standard_decorator_computed_key_expr_supported(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident(_)
        | ExprKind::StrLit(_)
        | ExprKind::NoSubstTemplate(_)
        | ExprKind::NumLit(_) => true,
        ExprKind::Paren(inner) => standard_decorator_computed_key_expr_supported(inner),
        ExprKind::Call(call) => {
            matches!(call.callee.kind, ExprKind::Ident(_))
                && call
                    .args
                    .iter()
                    .all(|arg| standard_decorator_computed_key_expr_supported(arg))
        }
        _ => false,
    }
}

pub(crate) fn class_method_decorator_wrapper_needs_prop_key(class: &ClassDecl) -> bool {
    class.members.iter().any(|member| {
        let name = match &member.kind {
            ClassMemberKind::Method(method) if !method.decorators.is_empty() => &method.name,
            ClassMemberKind::GetAccessor(accessor) if !accessor.decorators.is_empty() => {
                &accessor.name
            }
            ClassMemberKind::SetAccessor(accessor) if !accessor.decorators.is_empty() => {
                &accessor.name
            }
            _ => return false,
        };
        matches!(name, PropName::Computed(expr, _) if !matches!(
            &expr.kind,
            ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_)
        ))
    })
}

/// Serialize a type annotation to a runtime metadata value for `__metadata`.
/// Maps TypeScript type annotations to their runtime equivalents.
pub(crate) fn serialize_type_for_metadata(
    type_ann: &TypeNode,
    source: &str,
    type_only_names: &HashSet<AstString>,
    type_only_import_names: &HashSet<AstString>,
    global_type_only_names: &HashSet<AstString>,
    enum_names: &HashSet<AstString>,
    strict_null_checks: bool,
) -> String {
    serialize_type_for_metadata_cjs(
        type_ann,
        source,
        type_only_names,
        type_only_import_names,
        global_type_only_names,
        &HashMap::new(),
        &HashSet::new(),
        enum_names,
        strict_null_checks,
    )
}

enum MetadataUnionDecision<'a> {
    Void,
    RuntimeType(&'a TypeNode),
    Object,
}

fn metadata_union_constituent_is_effective(type_ann: &TypeNode, strict_null_checks: bool) -> bool {
    if strict_null_checks {
        !matches!(type_ann.kind, TypeNodeKind::Keyword(KeywordTypeKind::Never))
    } else {
        !matches!(
            type_ann.kind,
            TypeNodeKind::Keyword(KeywordTypeKind::Null)
                | TypeNodeKind::Keyword(KeywordTypeKind::Undefined)
                | TypeNodeKind::Keyword(KeywordTypeKind::Never)
                | TypeNodeKind::Literal(LiteralTypeKind::Null)
        )
    }
}

/// Select the runtime metadata behavior for a union once, using the same
/// constituent serialization that will be emitted. The import-retention
/// prepass calls this helper too, so it cannot retain a type reference for a
/// union that ultimately serializes to `Object`.
fn metadata_union_decision<'a>(
    types: &'a [TypeNode],
    strict_null_checks: bool,
    mut serialize: impl FnMut(&TypeNode) -> String,
) -> MetadataUnionDecision<'a> {
    let mut effective = types
        .iter()
        .filter(|type_ann| metadata_union_constituent_is_effective(type_ann, strict_null_checks));
    let Some(first) = effective.next() else {
        return MetadataUnionDecision::Void;
    };
    let Some(second) = effective.next() else {
        return MetadataUnionDecision::RuntimeType(first);
    };

    let first_serialized = serialize(first);
    if serialize(second) == first_serialized
        && effective.all(|type_ann| serialize(type_ann) == first_serialized)
    {
        MetadataUnionDecision::RuntimeType(first)
    } else {
        MetadataUnionDecision::Object
    }
}

/// Same as `serialize_type_for_metadata` but qualifies imported names with
/// their CJS binding (e.g. `observable_1.Observable` instead of `Observable`).
pub(crate) fn serialize_type_for_metadata_cjs(
    type_ann: &TypeNode,
    source: &str,
    type_only_names: &HashSet<AstString>,
    type_only_import_names: &HashSet<AstString>,
    global_type_only_names: &HashSet<AstString>,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    enum_names: &HashSet<AstString>,
    strict_null_checks: bool,
) -> String {
    match &type_ann.kind {
        TypeNodeKind::Keyword(kw) => match kw {
            KeywordTypeKind::String => "String".to_string(),
            KeywordTypeKind::Number => "Number".to_string(),
            KeywordTypeKind::Boolean => "Boolean".to_string(),
            KeywordTypeKind::Symbol => "Symbol".to_string(),
            KeywordTypeKind::BigInt => {
                "typeof BigInt === \"function\" ? BigInt : Object".to_string()
            }
            KeywordTypeKind::Void
            | KeywordTypeKind::Undefined
            | KeywordTypeKind::Never
            | KeywordTypeKind::Null => "void 0".to_string(),
            KeywordTypeKind::Any
            | KeywordTypeKind::Unknown
            | KeywordTypeKind::Object
            | KeywordTypeKind::Intrinsic => "Object".to_string(),
        },
        TypeNodeKind::Reference(type_ref) => {
            // Extract the type name from source
            let start = type_ref.name.span.start as usize;
            let end = type_ref.name.span.end as usize;
            if start < end && end <= source.len() {
                let name = &source[start..end];
                let root = name.split('.').next().unwrap_or(name);
                let is_type_only = type_only_names.contains(name)
                    || (root != name && type_only_names.contains(root));
                // Type-only imports/references have no runtime value
                if is_type_only {
                    let is_type_only_import = type_only_import_names.contains(name)
                        || (root != name && type_only_import_names.contains(root));
                    if is_type_only_import {
                        // For type-only IMPORTS, TypeScript emits:
                        // - Object for interface/type-only targets
                        // - Function for class-like targets with no runtime binding.
                        if global_type_only_names.contains(name)
                            || global_type_only_names.contains(root)
                        {
                            return "Object".to_string();
                        }
                        return "Function".to_string();
                    }
                    return "Object".to_string();
                }
                // For qualified names from namespace imports (e.g. `ns.T`), if
                // the member is globally known type-only, metadata falls back
                // to Object.
                if let Some((_, member)) = name.rsplit_once('.') {
                    if global_type_only_names.contains(member) {
                        return "Object".to_string();
                    }
                }
                // Enum types serialize to Number (e.g. `E` → Number,
                // `E.A` → Number). Check both the full name and the
                // root of dotted names.
                if enum_names.contains(name) {
                    return "Number".to_string();
                }
                if let Some(root) = name.split('.').next() {
                    if root != name && enum_names.contains(root) {
                        return "Number".to_string();
                    }
                }
                // Check for well-known types
                match name {
                    "Function" => "Function".to_string(),
                    "Array" | "ReadonlyArray" => "Array".to_string(),
                    "Promise" => "Promise".to_string(),
                    "Map" => "Map".to_string(),
                    "Set" => "Set".to_string(),
                    "RegExp" => "RegExp".to_string(),
                    "Date" => "Date".to_string(),
                    "Error" => "Error".to_string(),
                    _ => {
                        // In CJS mode, qualify imported names with their
                        // require binding (e.g. `observable_1.Observable`).
                        if let Some((req_var, imported_name)) = cjs_import_map.get(name) {
                            if imported_name.is_empty() {
                                req_var.to_string()
                            } else if cjs_string_import_locals.contains(name) {
                                format!(
                                    "{req_var}[\"{}\"]",
                                    emit_expr::cook_string_literal_raw_for_double_quote(
                                        imported_name
                                    )
                                )
                            } else {
                                format!("{req_var}.{imported_name}")
                            }
                        } else {
                            name.to_string()
                        }
                    }
                }
            } else {
                "Object".to_string()
            }
        }
        TypeNodeKind::Array(_) => "Array".to_string(),
        TypeNodeKind::Tuple(_) => "Array".to_string(),
        TypeNodeKind::Function(_) | TypeNodeKind::Constructor(_) => "Function".to_string(),
        TypeNodeKind::This => "Object".to_string(),
        TypeNodeKind::Paren(inner) => serialize_type_for_metadata_cjs(
            inner,
            source,
            type_only_names,
            type_only_import_names,
            global_type_only_names,
            cjs_import_map,
            cjs_string_import_locals,
            enum_names,
            strict_null_checks,
        ),
        TypeNodeKind::Literal(lit) => match lit {
            LiteralTypeKind::String(_) => "String".to_string(),
            LiteralTypeKind::Number(_) => "Number".to_string(),
            LiteralTypeKind::Boolean(_) => "Boolean".to_string(),
            LiteralTypeKind::BigInt(_) => {
                "typeof BigInt === \"function\" ? BigInt : Object".to_string()
            }
            LiteralTypeKind::Null => "void 0".to_string(),
            LiteralTypeKind::Minus(_) => "Number".to_string(),
        },
        TypeNodeKind::Union(types) => {
            let decision = metadata_union_decision(types, strict_null_checks, |type_ann| {
                serialize_type_for_metadata_cjs(
                    type_ann,
                    source,
                    type_only_names,
                    type_only_import_names,
                    global_type_only_names,
                    cjs_import_map,
                    cjs_string_import_locals,
                    enum_names,
                    strict_null_checks,
                )
            });
            match decision {
                MetadataUnionDecision::Void => "void 0".to_string(),
                MetadataUnionDecision::RuntimeType(type_ann) => serialize_type_for_metadata_cjs(
                    type_ann,
                    source,
                    type_only_names,
                    type_only_import_names,
                    global_type_only_names,
                    cjs_import_map,
                    cjs_string_import_locals,
                    enum_names,
                    strict_null_checks,
                ),
                MetadataUnionDecision::Object => "Object".to_string(),
            }
        }
        TypeNodeKind::Intersection(_) => "Object".to_string(),
        TypeNodeKind::TypeLit(_) => "Object".to_string(),
        TypeNodeKind::TemplateLit(_) => "String".to_string(),
        TypeNodeKind::Conditional(cond) => {
            let t = serialize_type_for_metadata_cjs(
                &cond.true_type,
                source,
                type_only_names,
                type_only_import_names,
                global_type_only_names,
                cjs_import_map,
                cjs_string_import_locals,
                enum_names,
                strict_null_checks,
            );
            let f = serialize_type_for_metadata_cjs(
                &cond.false_type,
                source,
                type_only_names,
                type_only_import_names,
                global_type_only_names,
                cjs_import_map,
                cjs_string_import_locals,
                enum_names,
                strict_null_checks,
            );
            if t == f {
                t
            } else {
                "Object".to_string()
            }
        }
        TypeNodeKind::Mapped(_) => "Object".to_string(),
        TypeNodeKind::IndexedAccess(_, _) => "Object".to_string(),
        TypeNodeKind::TypeQuery(_) => "Object".to_string(),
        TypeNodeKind::Keyof(_) => "Object".to_string(),
        // `readonly T[]` / `readonly [T, U]` — unwrap to the inner type
        TypeNodeKind::Readonly(inner) | TypeNodeKind::TypeOperator(_, inner) => {
            serialize_type_for_metadata_cjs(
                inner,
                source,
                type_only_names,
                type_only_import_names,
                global_type_only_names,
                cjs_import_map,
                cjs_string_import_locals,
                enum_names,
                strict_null_checks,
            )
        }
        // JSDoc nullable `string?` / `?string` → unwrap to inner type
        TypeNodeKind::JSDocNullable(Some(inner)) | TypeNodeKind::Optional(inner) => {
            serialize_type_for_metadata_cjs(
                inner,
                source,
                type_only_names,
                type_only_import_names,
                global_type_only_names,
                cjs_import_map,
                cjs_string_import_locals,
                enum_names,
                strict_null_checks,
            )
        }
        _ => "Object".to_string(),
    }
}

/// Serialize a parameter's type annotation for metadata, or "Object" if none.
/// For rest parameters (`...args: T[]`), uses the element type (`T`).
pub(crate) fn serialize_param_type_for_metadata(
    param: &Param,
    source: &str,
    type_only_names: &HashSet<AstString>,
    type_only_import_names: &HashSet<AstString>,
    global_type_only_names: &HashSet<AstString>,
    enum_names: &HashSet<AstString>,
    strict_null_checks: bool,
) -> String {
    serialize_param_type_for_metadata_cjs(
        param,
        source,
        type_only_names,
        type_only_import_names,
        global_type_only_names,
        &HashMap::new(),
        &HashSet::new(),
        enum_names,
        strict_null_checks,
    )
}

/// Same as `serialize_param_type_for_metadata` but with CJS import map support.
pub(crate) fn serialize_param_type_for_metadata_cjs(
    param: &Param,
    source: &str,
    type_only_names: &HashSet<AstString>,
    type_only_import_names: &HashSet<AstString>,
    global_type_only_names: &HashSet<AstString>,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    enum_names: &HashSet<AstString>,
    strict_null_checks: bool,
) -> String {
    match &param.type_ann {
        Some(type_ann) => {
            // For rest parameters, unwrap array type to get element type
            if param.dotdotdot {
                if let TypeNodeKind::Array(inner) = &type_ann.kind {
                    return serialize_type_for_metadata_cjs(
                        inner,
                        source,
                        type_only_names,
                        type_only_import_names,
                        global_type_only_names,
                        cjs_import_map,
                        cjs_string_import_locals,
                        enum_names,
                        strict_null_checks,
                    );
                }
            }
            serialize_type_for_metadata_cjs(
                type_ann,
                source,
                type_only_names,
                type_only_import_names,
                global_type_only_names,
                cjs_import_map,
                cjs_string_import_locals,
                enum_names,
                strict_null_checks,
            )
        }
        None => "Object".to_string(),
    }
}

/// Collect type reference names from a type annotation that would be used
/// in decorator metadata.  Only collects names from `TypeNodeKind::Reference`
/// positions (keyword types like `string` -> `String` don't need imports).
#[allow(dead_code)] // needed for decorator metadata implementation
pub(crate) fn collect_metadata_type_refs(
    type_ann: &TypeNode,
    source: &str,
    type_only: &HashSet<AstString>,
    type_only_import_names: &HashSet<AstString>,
    global_type_only_names: &HashSet<AstString>,
    enum_names: &HashSet<AstString>,
    out: &mut HashSet<String>,
    strict_null_checks: bool,
) {
    match &type_ann.kind {
        TypeNodeKind::Reference(type_ref) => {
            let start = type_ref.name.span.start as usize;
            let end = type_ref.name.span.end as usize;
            if start < end && end <= source.len() {
                let name = &source[start..end];
                if !type_only.contains(name) {
                    out.insert(name.to_string());
                }
            }
        }
        TypeNodeKind::Union(types) => {
            let cjs_import_map = HashMap::new();
            let decision = metadata_union_decision(types, strict_null_checks, |type_ann| {
                serialize_type_for_metadata_cjs(
                    type_ann,
                    source,
                    type_only,
                    type_only_import_names,
                    global_type_only_names,
                    &cjs_import_map,
                    &HashSet::new(),
                    enum_names,
                    strict_null_checks,
                )
            });
            if let MetadataUnionDecision::RuntimeType(type_ann) = decision {
                collect_metadata_type_refs(
                    type_ann,
                    source,
                    type_only,
                    type_only_import_names,
                    global_type_only_names,
                    enum_names,
                    out,
                    strict_null_checks,
                );
            }
        }
        TypeNodeKind::Intersection(_) => {
            // Intersections always serialize to Object — no import refs needed
        }
        TypeNodeKind::Array(inner) | TypeNodeKind::Paren(inner) => {
            collect_metadata_type_refs(
                inner,
                source,
                type_only,
                type_only_import_names,
                global_type_only_names,
                enum_names,
                out,
                strict_null_checks,
            );
        }
        TypeNodeKind::Conditional(cond) => {
            collect_metadata_type_refs(
                &cond.true_type,
                source,
                type_only,
                type_only_import_names,
                global_type_only_names,
                enum_names,
                out,
                strict_null_checks,
            );
            collect_metadata_type_refs(
                &cond.false_type,
                source,
                type_only,
                type_only_import_names,
                global_type_only_names,
                enum_names,
                out,
                strict_null_checks,
            );
        }
        _ => {}
    }
}

/// Check if a decorated class needs `let X = class X {};` because the
/// `__decorate` call reassigns the class name (class-level decorators or
/// constructor parameter decorators).
pub(crate) fn class_needs_let_wrapper(class: &ClassDecl) -> bool {
    if !class.decorators.is_empty() {
        return true;
    }
    for member in &class.members {
        if let ClassMemberKind::Constructor(ctor) = &member.kind {
            for param in &ctor.params {
                if !param.decorators.is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if a decorated class body references its own class name, requiring
/// a `var C_1` alias so internal references survive decoration.
pub(crate) fn decorated_class_has_self_reference(class: &ClassDecl) -> bool {
    let Some(ref name) = class.name else {
        return false;
    };
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Method(m) => {
                if let Some(ref body) = m.body {
                    if stmts_reference_name(body, name) {
                        return true;
                    }
                }
                if let PropName::Computed(ref key, _) = m.name {
                    if expr_references_name(key, name) {
                        return true;
                    }
                }
            }
            ClassMemberKind::Constructor(ctor) => {
                if let Some(ref body) = ctor.body {
                    if stmts_reference_name(body, name) {
                        return true;
                    }
                }
                for p in &ctor.params {
                    if let Some(ref init) = p.initializer {
                        if expr_references_name(init, name) {
                            return true;
                        }
                    }
                }
            }
            ClassMemberKind::Property(prop) => {
                if let Some(ref init) = prop.initializer {
                    if expr_references_name(init, name) {
                        return true;
                    }
                }
                if let PropName::Computed(ref key, _) = prop.name {
                    if expr_references_name(key, name) {
                        return true;
                    }
                }
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if let Some(ref body) = acc.body {
                    if stmts_reference_name(body, name) {
                        return true;
                    }
                }
                if let PropName::Computed(ref key, _) = acc.name {
                    if expr_references_name(key, name) {
                        return true;
                    }
                }
            }
            ClassMemberKind::StaticBlock(stmts) => {
                if stmts_reference_name(stmts, name) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn expr_references_name(expr: &Expr, name: &str) -> bool {
    match &expr.kind {
        ExprKind::Ident(n) => n == name,
        ExprKind::Paren(inner) => expr_references_name(inner, name),
        ExprKind::Binary(b) => {
            expr_references_name(&b.left, name) || expr_references_name(&b.right, name)
        }
        ExprKind::Cond(c) => {
            expr_references_name(&c.test, name)
                || expr_references_name(&c.consequent, name)
                || expr_references_name(&c.alternate, name)
        }
        ExprKind::Unary(u) => expr_references_name(&u.argument, name),
        ExprKind::Update(u) => expr_references_name(&u.argument, name),
        ExprKind::Assign(a) => {
            expr_references_name(&a.left, name) || expr_references_name(&a.right, name)
        }
        ExprKind::Member(m) => expr_references_name(&m.object, name),
        ExprKind::ElemAccess(ea) => {
            expr_references_name(&ea.object, name) || expr_references_name(&ea.index, name)
        }
        ExprKind::Call(c) => {
            expr_references_name(&c.callee, name)
                || c.args.iter().any(|a| expr_references_name(a, name))
        }
        ExprKind::New(n) => {
            expr_references_name(&n.callee, name)
                || n.args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|a| expr_references_name(a, name)))
        }
        ExprKind::Arrow(arrow) => {
            arrow.params.iter().any(|p| {
                p.initializer
                    .as_ref()
                    .is_some_and(|init| expr_references_name(init, name))
            }) || match &arrow.body {
                tsc_rs_ast::ArrowBody::Expr(e) => expr_references_name(e, name),
                tsc_rs_ast::ArrowBody::Block(stmts) => stmts_reference_name(stmts, name),
            }
        }
        // Regular functions and class exprs create new scopes — stop recursing
        ExprKind::FnExpr(_) | ExprKind::ClassExpr(_) => false,
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_references_name(e, name)),
        ExprKind::Spread(inner) => expr_references_name(inner, name),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(p) => expr_references_name(&p.value, name),
            ObjLitProp::Spread(e, _) => expr_references_name(e, name),
            _ => false,
        }),
        ExprKind::ArrayLit(elems) => elems
            .iter()
            .any(|e| e.as_ref().is_some_and(|e| expr_references_name(e, name))),
        ExprKind::Template(t) => t.exprs.iter().any(|e| expr_references_name(e, name)),
        ExprKind::TaggedTemplate(tt) => {
            expr_references_name(&tt.tag, name)
                || tt.quasi.exprs.iter().any(|e| expr_references_name(e, name))
        }
        ExprKind::Typeof(inner) => expr_references_name(inner, name),
        ExprKind::Yield(_, y) => y.as_ref().is_some_and(|e| expr_references_name(e, name)),
        ExprKind::Await(inner) => expr_references_name(inner, name),
        _ => false,
    }
}

/// Return whether moving a class computed-name expression into the legacy
/// ES5 class IIFE would change its lexical environment or leave unsupported
/// ES2015 function syntax behind.
///
/// The sole arrow form admitted by the transform is a root, parameterless,
/// empty arrow. It has a dedicated `function () { }` emitter. Nested arrows,
/// ordinary functions/classes, and object methods fail closed because their
/// closure/name-conversion behavior needs a separate ownership transform.
pub(crate) fn legacy_es5_computed_key_has_environment_hazard(
    expr: &Expr,
    class_name: &str,
) -> bool {
    fn prop_name_has_hazard(name: &PropName, class_name: &str) -> bool {
        matches!(name, PropName::Computed(expr, _) if visit(expr, class_name, false))
    }

    fn jsx_child_has_hazard(child: &JsxChild, class_name: &str) -> bool {
        match child {
            JsxChild::Element(expr) => visit(expr, class_name, false),
            JsxChild::Expression(expr, _) => expr
                .as_ref()
                .is_some_and(|expr| visit(expr, class_name, false)),
            JsxChild::Fragment(fragment) => fragment
                .children
                .iter()
                .any(|child| jsx_child_has_hazard(child, class_name)),
            JsxChild::Text(_, _) => false,
        }
    }

    fn visit(expr: &Expr, class_name: &str, root: bool) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => name == class_name || name == "arguments",
            ExprKind::This | ExprKind::Super | ExprKind::MetaProp(_) => true,
            ExprKind::Arrow(arrow) => {
                !(root
                    && !arrow.is_async
                    && arrow.type_params.is_none()
                    && arrow.return_type.is_none()
                    && arrow.params.is_empty()
                    && matches!(&arrow.body, ArrowBody::Block(body) if body.is_empty()))
            }
            ExprKind::FnExpr(_) | ExprKind::ClassExpr(_) => true,
            ExprKind::Call(call) => {
                visit(&call.callee, class_name, false)
                    || call.args.iter().any(|expr| visit(expr, class_name, false))
            }
            ExprKind::New(new_expr) => {
                visit(&new_expr.callee, class_name, false)
                    || new_expr
                        .args
                        .as_ref()
                        .is_some_and(|args| args.iter().any(|expr| visit(expr, class_name, false)))
            }
            ExprKind::Member(member) => visit(&member.object, class_name, false),
            ExprKind::ElemAccess(access) => {
                visit(&access.object, class_name, false) || visit(&access.index, class_name, false)
            }
            ExprKind::Cond(cond) => {
                visit(&cond.test, class_name, false)
                    || visit(&cond.consequent, class_name, false)
                    || visit(&cond.alternate, class_name, false)
            }
            ExprKind::Binary(binary) => {
                visit(&binary.left, class_name, false) || visit(&binary.right, class_name, false)
            }
            ExprKind::Unary(unary) => visit(&unary.argument, class_name, false),
            ExprKind::Update(update) => visit(&update.argument, class_name, false),
            ExprKind::Assign(assign) => {
                visit(&assign.left, class_name, false) || visit(&assign.right, class_name, false)
            }
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => visit(inner, class_name, root),
            ExprKind::TypeAssertion(assertion) => visit(&assertion.expr, class_name, root),
            ExprKind::As(as_expr) => visit(&as_expr.expr, class_name, root),
            ExprKind::Satisfies(satisfies) => visit(&satisfies.expr, class_name, root),
            ExprKind::Instantiation(instantiation) => visit(&instantiation.expr, class_name, root),
            // Moving these out of their surrounding async/generator grammar
            // into the synchronous class IIFE is never environment-neutral.
            ExprKind::Await(_) | ExprKind::Yield(_, _) => true,
            ExprKind::ArrayLit(items) => items.iter().any(|expr| {
                expr.as_ref()
                    .is_some_and(|expr| visit(expr, class_name, false))
            }),
            ExprKind::ObjectLit(properties) => properties.iter().any(|property| match property {
                ObjLitProp::Property(property) => {
                    prop_name_has_hazard(&property.key, class_name)
                        || visit(&property.value, class_name, false)
                }
                ObjLitProp::Spread(expr, _) => visit(expr, class_name, false),
                ObjLitProp::Shorthand(name, _) => {
                    name == class_name || name.as_str() == "arguments"
                }
                ObjLitProp::ShorthandDefault(name, initializer, _) => {
                    name == class_name
                        || name.as_str() == "arguments"
                        || visit(initializer, class_name, false)
                }
                // Object methods/accessors introduce closures and may be used
                // during ToPropertyKey conversion. Keep them on the native
                // class path until that ownership is modeled explicitly.
                ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => true,
            }),
            ExprKind::Template(template) => template
                .exprs
                .iter()
                .any(|expr| visit(expr, class_name, false)),
            ExprKind::TaggedTemplate(tagged) => {
                visit(&tagged.tag, class_name, false)
                    || tagged
                        .quasi
                        .exprs
                        .iter()
                        .any(|expr| visit(expr, class_name, false))
            }
            ExprKind::Comma(expressions) => expressions
                .iter()
                .any(|expr| visit(expr, class_name, false)),
            ExprKind::JsxElement(element) => {
                visit(&element.name, class_name, false)
                    || element.attributes.iter().any(|attribute| match attribute {
                        JsxAttribute::Normal { value, .. } => value
                            .as_ref()
                            .is_some_and(|expr| visit(expr, class_name, false)),
                        JsxAttribute::Spread(expr, _) => visit(expr, class_name, false),
                    })
                    || element
                        .children
                        .iter()
                        .any(|child| jsx_child_has_hazard(child, class_name))
            }
            ExprKind::JsxSelfClosing(element) => {
                visit(&element.name, class_name, false)
                    || element.attributes.iter().any(|attribute| match attribute {
                        JsxAttribute::Normal { value, .. } => value
                            .as_ref()
                            .is_some_and(|expr| visit(expr, class_name, false)),
                        JsxAttribute::Spread(expr, _) => visit(expr, class_name, false),
                    })
            }
            ExprKind::JsxFragment(fragment) => fragment
                .children
                .iter()
                .any(|child| jsx_child_has_hazard(child, class_name)),
            ExprKind::StrLit(_)
            | ExprKind::NumLit(_)
            | ExprKind::BigIntLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::RegexpLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::Omitted => false,
        }
    }

    visit(expr, class_name, true)
}

fn params_have_external_decorator_new_target(params: &[Param]) -> bool {
    params
        .iter()
        .flat_map(|p| &p.decorators)
        .any(expr_has_lexical_new_target)
}

fn class_external_has_lexical_new_target(class_decl: &ClassDecl) -> bool {
    class_decl
        .decorators
        .iter()
        .any(expr_has_lexical_new_target)
        || class_decl
            .extends
            .as_ref()
            .is_some_and(|e| expr_has_lexical_new_target(e))
        || class_decl.members.iter().any(|member| match &member.kind {
            ClassMemberKind::Property(p) => {
                matches!(&p.name, PropName::Computed(e, _) if expr_has_lexical_new_target(e))
                    || p.decorators.iter().any(expr_has_lexical_new_target)
            }
            ClassMemberKind::Method(m) => {
                matches!(&m.name, PropName::Computed(e, _) if expr_has_lexical_new_target(e))
                    || m.decorators.iter().any(expr_has_lexical_new_target)
                    || params_have_external_decorator_new_target(&m.params)
            }
            ClassMemberKind::Constructor(c) => {
                c.decorators.iter().any(expr_has_lexical_new_target)
                    || params_have_external_decorator_new_target(&c.params)
            }
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                matches!(&a.name, PropName::Computed(e, _) if expr_has_lexical_new_target(e))
                    || a.decorators.iter().any(expr_has_lexical_new_target)
                    || params_have_external_decorator_new_target(&a.params)
            }
            ClassMemberKind::IndexSignature(_)
            | ClassMemberKind::StaticBlock(_)
            | ClassMemberKind::SemicolonClassElement => false,
        })
}

fn stmt_references_name(stmt: &tsc_rs_ast::Stmt, name: &str) -> bool {
    match &stmt.kind {
        tsc_rs_ast::StmtKind::Expr(e) => expr_references_name(e, name),
        tsc_rs_ast::StmtKind::Return(Some(e)) => expr_references_name(e, name),
        tsc_rs_ast::StmtKind::Throw(e) => expr_references_name(e, name),
        tsc_rs_ast::StmtKind::Var(vs) => vs.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|init| expr_references_name(init, name))
        }),
        tsc_rs_ast::StmtKind::If(if_stmt) => {
            expr_references_name(&if_stmt.test, name)
                || stmt_references_name(&if_stmt.consequent, name)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|s| stmt_references_name(s, name))
        }
        tsc_rs_ast::StmtKind::Block(stmts) => stmts_reference_name(stmts, name),
        tsc_rs_ast::StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|i| match i {
                tsc_rs_ast::ForInit::Expr(e) => expr_references_name(e, name),
                tsc_rs_ast::ForInit::Var(vs) => vs.declarations.iter().any(|d| {
                    d.init
                        .as_ref()
                        .is_some_and(|init| expr_references_name(init, name))
                }),
            }) || f
                .test
                .as_ref()
                .is_some_and(|e| expr_references_name(e, name))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_references_name(e, name))
                || stmt_references_name(&f.body, name)
        }
        tsc_rs_ast::StmtKind::While(w) => {
            expr_references_name(&w.test, name) || stmt_references_name(&w.body, name)
        }
        tsc_rs_ast::StmtKind::Switch(sw) => {
            expr_references_name(&sw.discriminant, name)
                || sw.cases.iter().any(|c| {
                    c.test
                        .as_ref()
                        .is_some_and(|e| expr_references_name(e, name))
                        || stmts_reference_name(&c.consequent, name)
                })
        }
        _ => false,
    }
}

fn stmts_reference_name(stmts: &[tsc_rs_ast::Stmt], name: &str) -> bool {
    stmts.iter().any(|s| stmt_references_name(s, name))
}

fn params_have_async(params: &[Param]) -> bool {
    params.iter().any(|p| {
        p.initializer.as_ref().is_some_and(|e| expr_has_async(e)) || pat_has_async(&p.name)
    })
}

fn params_have_awaiter(params: &[Param]) -> bool {
    params.iter().any(|p| {
        p.initializer.as_ref().is_some_and(|e| expr_has_awaiter(e)) || pat_has_awaiter(&p.name)
    })
}

fn params_have_async_generator(params: &[Param]) -> bool {
    params.iter().any(|p| {
        p.initializer
            .as_ref()
            .is_some_and(|e| expr_has_async_generator(e))
            || pat_has_async_generator(&p.name)
    })
}

fn prop_name_has_async(name: &PropName) -> bool {
    matches!(name, PropName::Computed(expr, _) if expr_has_async(expr))
}

fn prop_name_has_awaiter(name: &PropName) -> bool {
    matches!(name, PropName::Computed(expr, _) if expr_has_awaiter(expr))
}

fn prop_name_has_async_generator(name: &PropName) -> bool {
    matches!(name, PropName::Computed(expr, _) if expr_has_async_generator(expr))
}

fn pat_has_async(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(key, value) => prop_name_has_async(key) || pat_has_async(value),
            ObjPatProp::Shorthand(_, _) => false,
            ObjPatProp::ShorthandAssign(_, init, _) => expr_has_async(init),
            ObjPatProp::Rest(inner) => pat_has_async(inner),
        }),
        PatKind::Array(elements) => elements.iter().flatten().any(|elem| match elem {
            ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pat_has_async(p),
        }),
        PatKind::Assign(inner, init) => pat_has_async(inner) || expr_has_async(init),
        PatKind::Rest(inner) => pat_has_async(inner),
    }
}

fn pat_has_awaiter(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(key, value) => {
                prop_name_has_awaiter(key) || pat_has_awaiter(value)
            }
            ObjPatProp::Shorthand(_, _) => false,
            ObjPatProp::ShorthandAssign(_, init, _) => expr_has_awaiter(init),
            ObjPatProp::Rest(inner) => pat_has_awaiter(inner),
        }),
        PatKind::Array(elements) => elements.iter().flatten().any(|elem| match elem {
            ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pat_has_awaiter(p),
        }),
        PatKind::Assign(inner, init) => pat_has_awaiter(inner) || expr_has_awaiter(init),
        PatKind::Rest(inner) => pat_has_awaiter(inner),
    }
}

fn pat_has_async_generator(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(key, value) => {
                prop_name_has_async_generator(key) || pat_has_async_generator(value)
            }
            ObjPatProp::Shorthand(_, _) => false,
            ObjPatProp::ShorthandAssign(_, init, _) => expr_has_async_generator(init),
            ObjPatProp::Rest(inner) => pat_has_async_generator(inner),
        }),
        PatKind::Array(elements) => elements.iter().flatten().any(|elem| match elem {
            ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pat_has_async_generator(p),
        }),
        PatKind::Assign(inner, init) => {
            pat_has_async_generator(inner) || expr_has_async_generator(init)
        }
        PatKind::Rest(inner) => pat_has_async_generator(inner),
    }
}

/// Check if a statement contains an async function/method declaration.
pub(crate) fn source_has_async(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::FnDecl(fn_decl) => {
            fn_decl.is_async
                || params_have_async(&fn_decl.params)
                || fn_decl
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| source_has_async(s)))
        }
        StmtKind::ClassDecl(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                // Skip abstract methods without bodies — they are erased and don't need helpers.
                if method.modifiers & MOD_ABSTRACT != 0 && method.body.is_none() {
                    return false;
                }
                method.is_async
                    || params_have_async(&method.params)
                    || method
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_async(s)))
            }
            ClassMemberKind::Property(prop) => {
                prop.initializer.as_ref().is_some_and(|e| expr_has_async(e))
            }
            _ => false,
        }),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) => source_has_async(inner),
            ExportDeclKind::DefaultDecl(inner) => source_has_async(inner),
            ExportDeclKind::Default(e) => expr_has_async(e),
            _ => false,
        },
        StmtKind::Var(var_stmt) => var_stmt
            .declarations
            .iter()
            .any(|d| d.init.as_ref().is_some_and(|e| expr_has_async(e)) || pat_has_async(&d.name)),
        StmtKind::Expr(expr) => expr_has_async(expr),
        StmtKind::Block(stmts) => stmts.iter().any(|s| source_has_async(s)),
        StmtKind::If(if_stmt) => {
            source_has_async(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| source_has_async(a))
        }
        StmtKind::For(for_stmt) => source_has_async(&for_stmt.body),
        StmtKind::ForIn(fi) => source_has_async(&fi.body),
        StmtKind::ForOf(fo) => source_has_async(&fo.body),
        StmtKind::While(w) => source_has_async(&w.body),
        StmtKind::DoWhile(dw) => source_has_async(&dw.body),
        StmtKind::Return(ret) => ret.as_ref().is_some_and(|e| expr_has_async(e)),
        StmtKind::ModuleDecl(m) => match &m.body {
            Some(ModuleBody::Block(stmts)) => stmts.iter().any(|s| source_has_async(s)),
            Some(ModuleBody::Module(inner)) => {
                let tmp = Stmt {
                    kind: StmtKind::ModuleDecl(Box::new(inner.as_ref().clone())),
                    span: stmt.span,
                };
                source_has_async(&tmp)
            }
            None => false,
        },
        StmtKind::Try(t) => {
            t.block.iter().any(|s| source_has_async(s))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.iter().any(|s| source_has_async(s)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| source_has_async(s)))
        }
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(|s| source_has_async(s))),
        StmtKind::Labeled(l) => source_has_async(&l.body),
        _ => false,
    }
}

/// Check if a statement contains async constructs that need __awaiter
/// (async functions/methods excluding async generators).
pub(crate) fn source_has_awaiter(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::FnDecl(fn_decl) => {
            (fn_decl.is_async && !fn_decl.is_generator)
                || params_have_awaiter(&fn_decl.params)
                || fn_decl
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| source_has_awaiter(s)))
        }
        StmtKind::ClassDecl(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                // Skip abstract methods without bodies — they are erased and don't need helpers.
                if method.modifiers & MOD_ABSTRACT != 0 && method.body.is_none() {
                    return false;
                }
                (method.is_async && !method.is_generator)
                    || params_have_awaiter(&method.params)
                    || method
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_awaiter(s)))
            }
            ClassMemberKind::Property(prop) => prop
                .initializer
                .as_ref()
                .is_some_and(|e| expr_has_awaiter(e)),
            _ => false,
        }),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) => source_has_awaiter(inner),
            ExportDeclKind::DefaultDecl(inner) => source_has_awaiter(inner),
            ExportDeclKind::Default(e) => expr_has_awaiter(e),
            _ => false,
        },
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init.as_ref().is_some_and(|e| expr_has_awaiter(e)) || pat_has_awaiter(&d.name)
        }),
        StmtKind::Expr(expr) => expr_has_awaiter(expr),
        StmtKind::Block(stmts) => stmts.iter().any(|s| source_has_awaiter(s)),
        StmtKind::If(if_stmt) => {
            source_has_awaiter(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| source_has_awaiter(a))
        }
        StmtKind::For(for_stmt) => source_has_awaiter(&for_stmt.body),
        StmtKind::ForIn(fi) => source_has_awaiter(&fi.body),
        StmtKind::ForOf(fo) => source_has_awaiter(&fo.body),
        StmtKind::While(w) => source_has_awaiter(&w.body),
        StmtKind::DoWhile(dw) => source_has_awaiter(&dw.body),
        StmtKind::Return(ret) => ret.as_ref().is_some_and(|e| expr_has_awaiter(e)),
        StmtKind::ModuleDecl(m) => match &m.body {
            Some(ModuleBody::Block(stmts)) => stmts.iter().any(|s| source_has_awaiter(s)),
            Some(ModuleBody::Module(inner)) => {
                let tmp = Stmt {
                    kind: StmtKind::ModuleDecl(Box::new(inner.as_ref().clone())),
                    span: stmt.span,
                };
                source_has_awaiter(&tmp)
            }
            None => false,
        },
        StmtKind::Try(t) => {
            t.block.iter().any(|s| source_has_awaiter(s))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.iter().any(|s| source_has_awaiter(s)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| source_has_awaiter(s)))
        }
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(|s| source_has_awaiter(s))),
        StmtKind::Labeled(l) => source_has_awaiter(&l.body),
        _ => false,
    }
}

/// Check if an expression contains async constructs that need __awaiter
/// (async functions/arrows excluding async generators).
pub(crate) fn expr_has_awaiter(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(fn_decl) => {
            (fn_decl.is_async && !fn_decl.is_generator) || params_have_awaiter(&fn_decl.params)
        }
        ExprKind::Arrow(arrow) => {
            arrow.is_async
                || match &arrow.body {
                    ArrowBody::Expr(e) => expr_has_awaiter(e),
                    ArrowBody::Block(stmts) => stmts.iter().any(|s| source_has_awaiter(s)),
                }
        }
        ExprKind::Call(call) => {
            expr_has_awaiter(&call.callee) || call.args.iter().any(|a| expr_has_awaiter(a))
        }
        ExprKind::Assign(assign) => {
            expr_has_awaiter(&assign.left) || expr_has_awaiter(&assign.right)
        }
        ExprKind::Paren(inner) => expr_has_awaiter(inner),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Method(m) => m.is_async && !m.is_generator,
            ObjLitProp::Property(p) => prop_name_has_awaiter(&p.key) || expr_has_awaiter(&p.value),
            ObjLitProp::ShorthandDefault(_, init, _) => expr_has_awaiter(init),
            ObjLitProp::Spread(expr, _) => expr_has_awaiter(expr),
            _ => false,
        }),
        ExprKind::ClassExpr(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                (method.is_async && !method.is_generator) || params_have_awaiter(&method.params)
            }
            ClassMemberKind::Property(prop) => prop
                .initializer
                .as_ref()
                .is_some_and(|e| expr_has_awaiter(e)),
            _ => false,
        }),
        _ => false,
    }
}

/// Check if a statement contains async generator constructs that need
/// __await/__asyncGenerator helpers.
pub(crate) fn source_has_async_generator(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::FnDecl(fn_decl) => {
            (fn_decl.is_async && fn_decl.is_generator)
                || params_have_async_generator(&fn_decl.params)
                || fn_decl
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| source_has_async_generator(s)))
        }
        StmtKind::ClassDecl(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                // Skip abstract methods without bodies — they are erased and don't need helpers.
                if method.modifiers & MOD_ABSTRACT != 0 && method.body.is_none() {
                    return false;
                }
                (method.is_async && method.is_generator)
                    || params_have_async_generator(&method.params)
                    || method
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_async_generator(s)))
            }
            _ => false,
        }),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) => source_has_async_generator(inner),
            ExportDeclKind::DefaultDecl(inner) => source_has_async_generator(inner),
            ExportDeclKind::Default(e) => expr_has_async_generator(e),
            _ => false,
        },
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init.as_ref().is_some_and(|e| expr_has_async_generator(e))
                || pat_has_async_generator(&d.name)
        }),
        StmtKind::Expr(expr) => expr_has_async_generator(expr),
        StmtKind::Block(stmts) => stmts.iter().any(|s| source_has_async_generator(s)),
        StmtKind::If(if_stmt) => {
            source_has_async_generator(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| source_has_async_generator(a))
        }
        StmtKind::For(for_stmt) => source_has_async_generator(&for_stmt.body),
        StmtKind::ForIn(fi) => source_has_async_generator(&fi.body),
        StmtKind::ForOf(fo) => source_has_async_generator(&fo.body),
        StmtKind::While(w) => source_has_async_generator(&w.body),
        StmtKind::DoWhile(dw) => source_has_async_generator(&dw.body),
        StmtKind::Return(ret) => ret.as_ref().is_some_and(|e| expr_has_async_generator(e)),
        StmtKind::ModuleDecl(m) => match &m.body {
            Some(ModuleBody::Block(stmts)) => stmts.iter().any(|s| source_has_async_generator(s)),
            Some(ModuleBody::Module(inner)) => {
                let tmp = Stmt {
                    kind: StmtKind::ModuleDecl(Box::new(inner.as_ref().clone())),
                    span: stmt.span,
                };
                source_has_async_generator(&tmp)
            }
            None => false,
        },
        StmtKind::Try(t) => {
            t.block.iter().any(|s| source_has_async_generator(s))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.iter().any(|s| source_has_async_generator(s)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| source_has_async_generator(s)))
        }
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(|s| source_has_async_generator(s))),
        StmtKind::Labeled(l) => source_has_async_generator(&l.body),
        _ => false,
    }
}

/// Check if an expression contains async generator constructs.
pub(crate) fn expr_has_async_generator(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(fn_decl) => {
            (fn_decl.is_async && fn_decl.is_generator)
                || params_have_async_generator(&fn_decl.params)
        }
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(e) => expr_has_async_generator(e),
            ArrowBody::Block(stmts) => stmts.iter().any(|s| source_has_async_generator(s)),
        },
        ExprKind::Call(call) => {
            expr_has_async_generator(&call.callee)
                || call.args.iter().any(|a| expr_has_async_generator(a))
        }
        ExprKind::Assign(assign) => {
            expr_has_async_generator(&assign.left) || expr_has_async_generator(&assign.right)
        }
        ExprKind::Paren(inner) => expr_has_async_generator(inner),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Method(m) => m.is_async && m.is_generator,
            ObjLitProp::Property(p) => {
                prop_name_has_async_generator(&p.key) || expr_has_async_generator(&p.value)
            }
            ObjLitProp::ShorthandDefault(_, init, _) => expr_has_async_generator(init),
            ObjLitProp::Spread(expr, _) => expr_has_async_generator(expr),
            _ => false,
        }),
        ExprKind::ClassExpr(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                (method.is_async && method.is_generator)
                    || params_have_async_generator(&method.params)
            }
            _ => false,
        }),
        _ => false,
    }
}

/// Check if any async generator's `yield*` operand contains a nested async generator expression.
/// When true, `__await/__asyncGenerator` should be emitted BEFORE `__asyncValues/__asyncDelegator`.
pub(crate) fn source_yield_star_has_nested_async_gen(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::FnDecl(fn_decl) => {
            if fn_decl.is_async && fn_decl.is_generator {
                if let Some(body) = &fn_decl.body {
                    if yield_star_operand_has_async_gen(body) {
                        return true;
                    }
                }
            }
            // Recurse into nested declarations
            fn_decl
                .body
                .as_ref()
                .is_some_and(|b| b.iter().any(|s| source_yield_star_has_nested_async_gen(s)))
        }
        StmtKind::ClassDecl(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                if method.is_async && method.is_generator {
                    if let Some(body) = &method.body {
                        if yield_star_operand_has_async_gen(body) {
                            return true;
                        }
                    }
                }
                method
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| source_yield_star_has_nested_async_gen(s)))
            }
            _ => false,
        }),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) => source_yield_star_has_nested_async_gen(inner),
            ExportDeclKind::DefaultDecl(inner) => source_yield_star_has_nested_async_gen(inner),
            ExportDeclKind::Default(e) => expr_yield_star_has_nested_async_gen(e),
            _ => false,
        },
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_yield_star_has_nested_async_gen(e))
        }),
        StmtKind::Expr(expr) => expr_yield_star_has_nested_async_gen(expr),
        StmtKind::Block(stmts) => stmts
            .iter()
            .any(|s| source_yield_star_has_nested_async_gen(s)),
        _ => false,
    }
}

fn expr_yield_star_has_nested_async_gen(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(fn_decl) => {
            if fn_decl.is_async && fn_decl.is_generator {
                if let Some(body) = &fn_decl.body {
                    if yield_star_operand_has_async_gen(body) {
                        return true;
                    }
                }
            }
            false
        }
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(e) => expr_yield_star_has_nested_async_gen(e),
            ArrowBody::Block(stmts) => stmts
                .iter()
                .any(|s| source_yield_star_has_nested_async_gen(s)),
        },
        ExprKind::Call(call) => {
            expr_yield_star_has_nested_async_gen(&call.callee)
                || call
                    .args
                    .iter()
                    .any(|a| expr_yield_star_has_nested_async_gen(a))
        }
        ExprKind::Assign(assign) => expr_yield_star_has_nested_async_gen(&assign.right),
        ExprKind::Paren(inner) => expr_yield_star_has_nested_async_gen(inner),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Method(m) => {
                if m.is_async && m.is_generator {
                    return yield_star_operand_has_async_gen(&m.body);
                }
                false
            }
            ObjLitProp::Property(p) => expr_yield_star_has_nested_async_gen(&p.value),
            _ => false,
        }),
        ExprKind::ClassExpr(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                if method.is_async && method.is_generator {
                    if let Some(body) = &method.body {
                        return yield_star_operand_has_async_gen(body);
                    }
                }
                false
            }
            _ => false,
        }),
        _ => false,
    }
}

/// Check if a source file contains async generators with `yield*` delegates,
/// requiring `__asyncValues` and `__asyncDelegator` helpers.
pub(crate) fn source_has_async_generator_delegate(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::FnDecl(fn_decl) => {
            if fn_decl.is_async && fn_decl.is_generator {
                // Check if this async generator body has yield*
                if let Some(body) = &fn_decl.body {
                    if body_has_yield_delegate(body) {
                        return true;
                    }
                }
            }
            // Recurse into nested declarations
            fn_decl
                .body
                .as_ref()
                .is_some_and(|b| b.iter().any(|s| source_has_async_generator_delegate(s)))
        }
        StmtKind::ClassDecl(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                if method.is_async && method.is_generator {
                    if let Some(body) = &method.body {
                        if body_has_yield_delegate(body) {
                            return true;
                        }
                    }
                }
                method
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| source_has_async_generator_delegate(s)))
            }
            _ => false,
        }),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) => source_has_async_generator_delegate(inner),
            ExportDeclKind::DefaultDecl(inner) => source_has_async_generator_delegate(inner),
            ExportDeclKind::Default(e) => expr_has_async_generator_delegate(e),
            _ => false,
        },
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_async_generator_delegate(e))
        }),
        StmtKind::Expr(expr) => expr_has_async_generator_delegate(expr),
        StmtKind::Block(stmts) => stmts.iter().any(|s| source_has_async_generator_delegate(s)),
        _ => false,
    }
}

fn expr_has_async_generator_delegate(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(fn_decl) => {
            if fn_decl.is_async && fn_decl.is_generator {
                if let Some(body) = &fn_decl.body {
                    if body_has_yield_delegate(body) {
                        return true;
                    }
                }
            }
            false
        }
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(e) => expr_has_async_generator_delegate(e),
            ArrowBody::Block(stmts) => stmts.iter().any(|s| source_has_async_generator_delegate(s)),
        },
        ExprKind::Call(call) => {
            expr_has_async_generator_delegate(&call.callee)
                || call
                    .args
                    .iter()
                    .any(|a| expr_has_async_generator_delegate(a))
        }
        ExprKind::Assign(assign) => expr_has_async_generator_delegate(&assign.right),
        ExprKind::Paren(inner) => expr_has_async_generator_delegate(inner),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Method(m) => {
                if m.is_async && m.is_generator {
                    return body_has_yield_delegate(&m.body);
                }
                false
            }
            ObjLitProp::Property(p) => expr_has_async_generator_delegate(&p.value),
            _ => false,
        }),
        ExprKind::ClassExpr(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                if method.is_async && method.is_generator {
                    if let Some(body) = &method.body {
                        return body_has_yield_delegate(body);
                    }
                }
                false
            }
            _ => false,
        }),
        _ => false,
    }
}

/// Check if a function body (at the immediate level, not recursing into nested fns)
/// contains `yield*` expressions.
fn body_has_yield_delegate(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| stmt_has_yield_delegate(s))
}

fn stmt_has_yield_delegate(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) => expr_has_yield_delegate(expr),
        StmtKind::Return(ret) => ret.as_ref().is_some_and(|e| expr_has_yield_delegate(e)),
        StmtKind::Var(var_stmt) => var_stmt
            .declarations
            .iter()
            .any(|d| d.init.as_ref().is_some_and(|e| expr_has_yield_delegate(e))),
        StmtKind::Block(stmts) => stmts.iter().any(|s| stmt_has_yield_delegate(s)),
        StmtKind::If(if_stmt) => {
            stmt_has_yield_delegate(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| stmt_has_yield_delegate(a))
        }
        StmtKind::For(f) => stmt_has_yield_delegate(&f.body),
        StmtKind::ForIn(fi) => stmt_has_yield_delegate(&fi.body),
        StmtKind::ForOf(fo) => stmt_has_yield_delegate(&fo.body),
        StmtKind::While(w) => stmt_has_yield_delegate(&w.body),
        StmtKind::DoWhile(dw) => stmt_has_yield_delegate(&dw.body),
        StmtKind::Try(t) => {
            t.block.iter().any(|s| stmt_has_yield_delegate(s))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.iter().any(|s| stmt_has_yield_delegate(s)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| stmt_has_yield_delegate(s)))
        }
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(|s| stmt_has_yield_delegate(s))),
        StmtKind::Labeled(l) => stmt_has_yield_delegate(&l.body),
        _ => false,
    }
}

fn expr_has_yield_delegate(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Yield(delegate, arg) => {
            if *delegate {
                return true;
            }
            arg.as_ref().is_some_and(|a| expr_has_yield_delegate(a))
        }
        ExprKind::Paren(inner) => expr_has_yield_delegate(inner),
        ExprKind::Assign(assign) => {
            expr_has_yield_delegate(&assign.left) || expr_has_yield_delegate(&assign.right)
        }
        ExprKind::Binary(bin) => {
            expr_has_yield_delegate(&bin.left) || expr_has_yield_delegate(&bin.right)
        }
        ExprKind::Cond(cond) => {
            expr_has_yield_delegate(&cond.test)
                || expr_has_yield_delegate(&cond.consequent)
                || expr_has_yield_delegate(&cond.alternate)
        }
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_yield_delegate(e)),
        ExprKind::Await(inner) => expr_has_yield_delegate(inner),
        // Don't recurse into nested functions/generators — they have their own scope
        ExprKind::FnExpr(_) | ExprKind::Arrow(_) => false,
        ExprKind::Call(call) => {
            expr_has_yield_delegate(&call.callee)
                || call.args.iter().any(|a| expr_has_yield_delegate(a))
        }
        _ => false,
    }
}

/// Check if a `yield*` in an async gen body delegates to an operand that
/// contains a nested async generator expression. When true, TypeScript emits
/// `__await/__asyncGenerator` BEFORE `__asyncValues/__asyncDelegator`.
/// When false (simple operand like array), `__asyncValues` comes first.
pub(crate) fn yield_star_operand_has_async_gen(stmts: &[Stmt]) -> bool {
    stmts
        .iter()
        .any(|s| stmt_yield_star_operand_has_async_gen(s))
}

fn stmt_yield_star_operand_has_async_gen(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) => expr_yield_star_operand_has_async_gen(expr),
        StmtKind::Return(ret) => ret
            .as_ref()
            .is_some_and(|e| expr_yield_star_operand_has_async_gen(e)),
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_yield_star_operand_has_async_gen(e))
        }),
        StmtKind::Block(stmts) => stmts
            .iter()
            .any(|s| stmt_yield_star_operand_has_async_gen(s)),
        StmtKind::If(if_stmt) => {
            stmt_yield_star_operand_has_async_gen(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| stmt_yield_star_operand_has_async_gen(a))
        }
        StmtKind::For(f) => stmt_yield_star_operand_has_async_gen(&f.body),
        StmtKind::ForIn(fi) => stmt_yield_star_operand_has_async_gen(&fi.body),
        StmtKind::ForOf(fo) => stmt_yield_star_operand_has_async_gen(&fo.body),
        StmtKind::While(w) => stmt_yield_star_operand_has_async_gen(&w.body),
        StmtKind::DoWhile(dw) => stmt_yield_star_operand_has_async_gen(&dw.body),
        StmtKind::Try(t) => {
            t.block
                .iter()
                .any(|s| stmt_yield_star_operand_has_async_gen(s))
                || t.handler.as_ref().is_some_and(|h| {
                    h.body
                        .iter()
                        .any(|s| stmt_yield_star_operand_has_async_gen(s))
                })
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| stmt_yield_star_operand_has_async_gen(s)))
        }
        StmtKind::Switch(sw) => sw.cases.iter().any(|c| {
            c.consequent
                .iter()
                .any(|s| stmt_yield_star_operand_has_async_gen(s))
        }),
        StmtKind::Labeled(l) => stmt_yield_star_operand_has_async_gen(&l.body),
        _ => false,
    }
}

fn expr_yield_star_operand_has_async_gen(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Yield(delegate, arg) => {
            if *delegate {
                // This is yield* — check if the operand contains an async gen expression
                if let Some(operand) = arg {
                    return operand_contains_async_gen(operand);
                }
            }
            // Also recurse into non-delegate yield args
            arg.as_ref()
                .is_some_and(|a| expr_yield_star_operand_has_async_gen(a))
        }
        ExprKind::Paren(inner) => expr_yield_star_operand_has_async_gen(inner),
        ExprKind::Assign(assign) => {
            expr_yield_star_operand_has_async_gen(&assign.left)
                || expr_yield_star_operand_has_async_gen(&assign.right)
        }
        ExprKind::Binary(bin) => {
            expr_yield_star_operand_has_async_gen(&bin.left)
                || expr_yield_star_operand_has_async_gen(&bin.right)
        }
        ExprKind::Cond(cond) => {
            expr_yield_star_operand_has_async_gen(&cond.test)
                || expr_yield_star_operand_has_async_gen(&cond.consequent)
                || expr_yield_star_operand_has_async_gen(&cond.alternate)
        }
        ExprKind::Comma(exprs) => exprs
            .iter()
            .any(|e| expr_yield_star_operand_has_async_gen(e)),
        ExprKind::Await(inner) => expr_yield_star_operand_has_async_gen(inner),
        // Don't recurse into nested functions — they have their own scope
        ExprKind::FnExpr(_) | ExprKind::Arrow(_) => false,
        ExprKind::Call(call) => {
            expr_yield_star_operand_has_async_gen(&call.callee)
                || call
                    .args
                    .iter()
                    .any(|a| expr_yield_star_operand_has_async_gen(a))
        }
        _ => false,
    }
}

/// Check if an expression contains an async generator function expression.
fn operand_contains_async_gen(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(fn_decl) => {
            if fn_decl.is_async && fn_decl.is_generator {
                return true;
            }
            false
        }
        ExprKind::Paren(inner) => operand_contains_async_gen(inner),
        ExprKind::Call(call) => {
            operand_contains_async_gen(&call.callee)
                || call.args.iter().any(|a| operand_contains_async_gen(a))
        }
        ExprKind::Assign(assign) => operand_contains_async_gen(&assign.right),
        ExprKind::Cond(cond) => {
            operand_contains_async_gen(&cond.consequent)
                || operand_contains_async_gen(&cond.alternate)
        }
        ExprKind::Comma(exprs) => exprs.iter().any(|e| operand_contains_async_gen(e)),
        _ => false,
    }
}

/// Check if an expression contains a `this` reference that would bind
/// to the enclosing class (i.e., not inside a regular function/method
/// that creates its own `this` binding, but arrows are transparent).
pub(crate) fn expr_has_this(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::This => true,
        // Arrow functions do NOT create a new `this` binding, so `this` inside
        // an arrow still refers to the enclosing class.
        ExprKind::Arrow(arrow) => {
            arrow.params.iter().any(|p| {
                p.initializer
                    .as_ref()
                    .is_some_and(|init| expr_has_this(init))
            }) || match &arrow.body {
                tsc_rs_ast::ArrowBody::Expr(e) => expr_has_this(e),
                tsc_rs_ast::ArrowBody::Block(stmts) => stmts.iter().any(|s| stmt_has_this(s)),
            }
        }
        // Regular functions create their own `this`, so stop recursing.
        ExprKind::FnExpr(_) | ExprKind::ClassExpr(_) => false,
        ExprKind::Call(call) => {
            expr_has_this(&call.callee) || call.args.iter().any(|a| expr_has_this(a))
        }
        ExprKind::Assign(assign) => expr_has_this(&assign.left) || expr_has_this(&assign.right),
        ExprKind::Paren(inner) => expr_has_this(inner),
        ExprKind::Binary(b) => expr_has_this(&b.left) || expr_has_this(&b.right),
        ExprKind::Cond(c) => {
            expr_has_this(&c.test) || expr_has_this(&c.consequent) || expr_has_this(&c.alternate)
        }
        ExprKind::Unary(u) => expr_has_this(&u.argument),
        ExprKind::Update(u) => expr_has_this(&u.argument),
        ExprKind::Member(m) => expr_has_this(&m.object),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(p) => expr_has_this(&p.value),
            ObjLitProp::Spread(_, _) => false, // spread is `...expr`, rarely has `this`
            _ => false,
        }),
        ExprKind::ArrayLit(elems) => elems
            .iter()
            .any(|e| e.as_ref().is_some_and(|e| expr_has_this(e))),
        ExprKind::Template(t) => t.exprs.iter().any(|e| expr_has_this(e)),
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_this(e)),
        ExprKind::Spread(inner) => expr_has_this(inner),
        ExprKind::New(n) => {
            expr_has_this(&n.callee)
                || n.args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|a| expr_has_this(a)))
        }
        ExprKind::ElemAccess(ea) => expr_has_this(&ea.object) || expr_has_this(&ea.index),
        ExprKind::TaggedTemplate(tt) => {
            expr_has_this(&tt.tag) || tt.quasi.exprs.iter().any(|e| expr_has_this(e))
        }
        ExprKind::Yield(_, y) => y.as_ref().is_some_and(|e| expr_has_this(e)),
        ExprKind::Await(inner) => expr_has_this(inner),
        _ => false,
    }
}

/// Check if an expression contains a `super` reference that resolves to the
/// enclosing class static context (arrows are transparent, regular functions are not).
pub(crate) fn expr_has_super(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Super => true,
        ExprKind::Arrow(arrow) => {
            arrow.params.iter().any(|p| {
                p.initializer
                    .as_ref()
                    .is_some_and(|init| expr_has_super(init))
            }) || match &arrow.body {
                tsc_rs_ast::ArrowBody::Expr(e) => expr_has_super(e),
                tsc_rs_ast::ArrowBody::Block(stmts) => stmts.iter().any(stmt_has_super),
            }
        }
        // Regular functions have their own `super` context. A nested class is
        // only a boundary for class-owned bodies/initializers: heritage,
        // computed names, and decorator expressions evaluate outside it.
        ExprKind::FnExpr(_) => false,
        ExprKind::ClassExpr(class_decl) => class_external_has_super(class_decl),
        ExprKind::Call(call) => {
            expr_has_super(&call.callee) || call.args.iter().any(|a| expr_has_super(a))
        }
        ExprKind::Assign(assign) => expr_has_super(&assign.left) || expr_has_super(&assign.right),
        ExprKind::Paren(inner)
        | ExprKind::NonNull(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Await(inner)
        | ExprKind::Delete(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner) => expr_has_super(inner),
        ExprKind::TypeAssertion(ta) => expr_has_super(&ta.expr),
        ExprKind::As(a) => expr_has_super(&a.expr),
        ExprKind::Satisfies(s) => expr_has_super(&s.expr),
        ExprKind::Instantiation(inst) => expr_has_super(&inst.expr),
        ExprKind::Binary(b) => expr_has_super(&b.left) || expr_has_super(&b.right),
        ExprKind::Cond(c) => {
            expr_has_super(&c.test) || expr_has_super(&c.consequent) || expr_has_super(&c.alternate)
        }
        ExprKind::Unary(u) => expr_has_super(&u.argument),
        ExprKind::Update(u) => expr_has_super(&u.argument),
        ExprKind::Member(m) => expr_has_super(&m.object),
        ExprKind::ElemAccess(ea) => expr_has_super(&ea.object) || expr_has_super(&ea.index),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(p) => {
                matches!(&p.key, PropName::Computed(e, _) if expr_has_super(e))
                    || expr_has_super(&p.value)
            }
            ObjLitProp::Spread(e, _) => expr_has_super(e),
            ObjLitProp::ShorthandDefault(_, init, _) => expr_has_super(init),
            // Method/accessor bodies and parameter initializers have their own
            // `super` environment. A computed name is evaluated outside it.
            ObjLitProp::Method(m) => {
                matches!(&m.name, PropName::Computed(e, _) if expr_has_super(e))
            }
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                matches!(&a.name, PropName::Computed(e, _) if expr_has_super(e))
            }
            ObjLitProp::Shorthand(_, _) => false,
        }),
        ExprKind::ArrayLit(elems) => elems
            .iter()
            .any(|e| e.as_ref().is_some_and(|e| expr_has_super(e))),
        ExprKind::Template(t) => t.exprs.iter().any(|e| expr_has_super(e)),
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_super(e)),
        ExprKind::New(n) => {
            expr_has_super(&n.callee)
                || n.args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|a| expr_has_super(a)))
        }
        ExprKind::TaggedTemplate(tt) => {
            expr_has_super(&tt.tag) || tt.quasi.exprs.iter().any(|e| expr_has_super(e))
        }
        ExprKind::Yield(_, y) => y.as_ref().is_some_and(|e| expr_has_super(e)),
        ExprKind::JsxElement(el) => {
            expr_has_super(&el.name)
                || el.attributes.iter().any(|a| match a {
                    JsxAttribute::Normal { value, .. } => {
                        value.as_ref().is_some_and(|e| expr_has_super(e))
                    }
                    JsxAttribute::Spread(e, _) => expr_has_super(e),
                })
                || el.children.iter().any(jsx_child_has_super)
        }
        ExprKind::JsxSelfClosing(el) => {
            expr_has_super(&el.name)
                || el.attributes.iter().any(|a| match a {
                    JsxAttribute::Normal { value, .. } => {
                        value.as_ref().is_some_and(|e| expr_has_super(e))
                    }
                    JsxAttribute::Spread(e, _) => expr_has_super(e),
                })
        }
        ExprKind::JsxFragment(fragment) => fragment.children.iter().any(jsx_child_has_super),
        _ => false,
    }
}

fn jsx_child_has_super(child: &JsxChild) -> bool {
    match child {
        JsxChild::Element(e) => expr_has_super(e),
        JsxChild::Expression(e, _) => e.as_ref().is_some_and(|e| expr_has_super(e)),
        JsxChild::Fragment(f) => f.children.iter().any(jsx_child_has_super),
        JsxChild::Text(_, _) => false,
    }
}

fn prop_name_has_super(name: &PropName) -> bool {
    matches!(name, PropName::Computed(e, _) if expr_has_super(e))
}

fn params_have_external_decorator_super(params: &[Param]) -> bool {
    params
        .iter()
        .flat_map(|p| &p.decorators)
        .any(expr_has_super)
}

fn class_external_has_super(class_decl: &ClassDecl) -> bool {
    class_decl.decorators.iter().any(expr_has_super)
        || class_decl
            .extends
            .as_ref()
            .is_some_and(|e| expr_has_super(e))
        || class_decl.members.iter().any(|member| match &member.kind {
            ClassMemberKind::Property(p) => {
                prop_name_has_super(&p.name) || p.decorators.iter().any(expr_has_super)
            }
            ClassMemberKind::Method(m) => {
                prop_name_has_super(&m.name)
                    || m.decorators.iter().any(expr_has_super)
                    || params_have_external_decorator_super(&m.params)
            }
            ClassMemberKind::Constructor(c) => {
                c.decorators.iter().any(expr_has_super)
                    || params_have_external_decorator_super(&c.params)
            }
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                prop_name_has_super(&a.name)
                    || a.decorators.iter().any(expr_has_super)
                    || params_have_external_decorator_super(&a.params)
            }
            // Static blocks, field initializers, method/constructor bodies, and
            // parameter initializers execute in class-owned environments.
            ClassMemberKind::IndexSignature(_)
            | ClassMemberKind::StaticBlock(_)
            | ClassMemberKind::SemicolonClassElement => false,
        })
}

fn pat_has_super(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Assign(p, e) => pat_has_super(p) || expr_has_super(e),
        PatKind::Rest(p) => pat_has_super(p),
        PatKind::Array(items) => items.iter().any(|item| {
            item.as_ref().is_some_and(|item| match item {
                ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pat_has_super(p),
            })
        }),
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(name, p) => prop_name_has_super(name) || pat_has_super(p),
            ObjPatProp::ShorthandAssign(_, e, _) => expr_has_super(e),
            ObjPatProp::Rest(p) => pat_has_super(p),
            ObjPatProp::Shorthand(_, _) => false,
        }),
    }
}

fn for_in_of_left_has_super(left: &ForInOfLeft) -> bool {
    match left {
        ForInOfLeft::Pat(p) => pat_has_super(p),
        ForInOfLeft::Expr(e) => expr_has_super(e),
        ForInOfLeft::Var(v) => v
            .declarations
            .iter()
            .any(|d| pat_has_super(&d.name) || d.init.as_ref().is_some_and(|e| expr_has_super(e))),
    }
}

fn stmt_has_this(stmt: &tsc_rs_ast::Stmt) -> bool {
    match &stmt.kind {
        tsc_rs_ast::StmtKind::Expr(e) => expr_has_this(e),
        tsc_rs_ast::StmtKind::Return(r) => r.as_ref().is_some_and(|e| expr_has_this(e)),
        tsc_rs_ast::StmtKind::Var(v) => v
            .declarations
            .iter()
            .any(|d| d.init.as_ref().is_some_and(|e| expr_has_this(e))),
        tsc_rs_ast::StmtKind::If(i) => {
            expr_has_this(&i.test)
                || stmt_has_this(&i.consequent)
                || i.alternate.as_ref().is_some_and(|s| stmt_has_this(s))
        }
        tsc_rs_ast::StmtKind::Block(stmts) => stmts.iter().any(|s| stmt_has_this(s)),
        _ => false,
    }
}

pub(crate) fn stmt_has_super(stmt: &tsc_rs_ast::Stmt) -> bool {
    match &stmt.kind {
        tsc_rs_ast::StmtKind::Expr(e) => expr_has_super(e),
        tsc_rs_ast::StmtKind::Return(r) => r.as_ref().is_some_and(|e| expr_has_super(e)),
        tsc_rs_ast::StmtKind::Var(v) => v
            .declarations
            .iter()
            .any(|d| pat_has_super(&d.name) || d.init.as_ref().is_some_and(|e| expr_has_super(e))),
        tsc_rs_ast::StmtKind::If(i) => {
            expr_has_super(&i.test)
                || stmt_has_super(&i.consequent)
                || i.alternate.as_ref().is_some_and(|s| stmt_has_super(s))
        }
        tsc_rs_ast::StmtKind::Block(stmts) => stmts.iter().any(|s| stmt_has_super(s)),
        tsc_rs_ast::StmtKind::While(w) => expr_has_super(&w.test) || stmt_has_super(&w.body),
        tsc_rs_ast::StmtKind::DoWhile(dw) => stmt_has_super(&dw.body) || expr_has_super(&dw.test),
        tsc_rs_ast::StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|init| match init {
                ForInit::Expr(e) => expr_has_super(e),
                ForInit::Var(v) => v.declarations.iter().any(|d| {
                    pat_has_super(&d.name) || d.init.as_ref().is_some_and(|e| expr_has_super(e))
                }),
            }) || f.test.as_ref().is_some_and(|e| expr_has_super(e))
                || f.update.as_ref().is_some_and(|e| expr_has_super(e))
                || stmt_has_super(&f.body)
        }
        tsc_rs_ast::StmtKind::ForIn(f) => {
            for_in_of_left_has_super(&f.left) || expr_has_super(&f.right) || stmt_has_super(&f.body)
        }
        tsc_rs_ast::StmtKind::ForOf(f) => {
            for_in_of_left_has_super(&f.left) || expr_has_super(&f.right) || stmt_has_super(&f.body)
        }
        tsc_rs_ast::StmtKind::Switch(sw) => {
            expr_has_super(&sw.discriminant)
                || sw.cases.iter().any(|case| {
                    case.test.as_ref().is_some_and(|e| expr_has_super(e))
                        || case.consequent.iter().any(stmt_has_super)
                })
        }
        tsc_rs_ast::StmtKind::Try(t) => {
            t.block.iter().any(stmt_has_super)
                || t.handler.as_ref().is_some_and(|h| {
                    h.param.as_ref().is_some_and(pat_has_super) || h.body.iter().any(stmt_has_super)
                })
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_has_super))
        }
        tsc_rs_ast::StmtKind::Throw(e) => expr_has_super(e),
        tsc_rs_ast::StmtKind::Labeled(l) => stmt_has_super(&l.body),
        tsc_rs_ast::StmtKind::With(w) => expr_has_super(&w.object) || stmt_has_super(&w.body),
        tsc_rs_ast::StmtKind::ImportEquals(i) => expr_has_super(&i.module_ref),
        tsc_rs_ast::StmtKind::ExportAssign(e) => expr_has_super(e),
        tsc_rs_ast::StmtKind::Export(e) => match &e.kind {
            ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => stmt_has_super(s),
            ExportDeclKind::Default(e) => expr_has_super(e),
            _ => false,
        },
        // Normal functions/classes/modules establish their own `super` context.
        tsc_rs_ast::StmtKind::ClassDecl(class_decl) => class_external_has_super(class_decl),
        tsc_rs_ast::StmtKind::FnDecl(_)
        | tsc_rs_ast::StmtKind::ModuleDecl(_)
        | tsc_rs_ast::StmtKind::Import(_)
        | tsc_rs_ast::StmtKind::InterfaceDecl(_)
        | tsc_rs_ast::StmtKind::TypeAlias(_)
        | tsc_rs_ast::StmtKind::EnumDecl(_)
        | tsc_rs_ast::StmtKind::Break(_)
        | tsc_rs_ast::StmtKind::Continue(_)
        | tsc_rs_ast::StmtKind::Empty
        | tsc_rs_ast::StmtKind::Debugger => false,
    }
}

/// True when replacing an arrow with a normal function would change resolution
/// of a lexical environment reference. Nested arrows are transparent; normal
/// functions and classes establish boundaries.
pub(crate) fn arrow_has_lexical_environment_hazard(arrow: &ArrowFn) -> bool {
    arrow.params.iter().any(|p| {
        p.initializer
            .as_ref()
            .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
    }) || match &arrow.body {
        ArrowBody::Expr(e) => expr_has_arrow_lexical_environment_hazard(e),
        ArrowBody::Block(stmts) => stmts.iter().any(stmt_has_arrow_lexical_environment_hazard),
    }
}

fn prop_name_has_arrow_lexical_environment_hazard(name: &PropName) -> bool {
    matches!(name, PropName::Computed(e, _) if expr_has_arrow_lexical_environment_hazard(e))
}

fn params_have_external_decorator_arrow_hazard(params: &[Param]) -> bool {
    params
        .iter()
        .flat_map(|p| &p.decorators)
        .any(expr_has_arrow_lexical_environment_hazard)
}

fn class_external_has_arrow_lexical_environment_hazard(class_decl: &ClassDecl) -> bool {
    class_decl
        .decorators
        .iter()
        .any(expr_has_arrow_lexical_environment_hazard)
        || class_decl
            .extends
            .as_ref()
            .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
        || class_decl.members.iter().any(|member| match &member.kind {
            ClassMemberKind::Property(p) => {
                prop_name_has_arrow_lexical_environment_hazard(&p.name)
                    || p.decorators
                        .iter()
                        .any(expr_has_arrow_lexical_environment_hazard)
            }
            ClassMemberKind::Method(m) => {
                prop_name_has_arrow_lexical_environment_hazard(&m.name)
                    || m.decorators
                        .iter()
                        .any(expr_has_arrow_lexical_environment_hazard)
                    || params_have_external_decorator_arrow_hazard(&m.params)
            }
            ClassMemberKind::Constructor(c) => {
                c.decorators
                    .iter()
                    .any(expr_has_arrow_lexical_environment_hazard)
                    || params_have_external_decorator_arrow_hazard(&c.params)
            }
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                prop_name_has_arrow_lexical_environment_hazard(&a.name)
                    || a.decorators
                        .iter()
                        .any(expr_has_arrow_lexical_environment_hazard)
                    || params_have_external_decorator_arrow_hazard(&a.params)
            }
            ClassMemberKind::IndexSignature(_)
            | ClassMemberKind::StaticBlock(_)
            | ClassMemberKind::SemicolonClassElement => false,
        })
}

fn pat_has_arrow_lexical_environment_hazard(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Assign(p, e) => {
            pat_has_arrow_lexical_environment_hazard(p)
                || expr_has_arrow_lexical_environment_hazard(e)
        }
        PatKind::Rest(p) => pat_has_arrow_lexical_environment_hazard(p),
        PatKind::Array(items) => items.iter().any(|item| {
            item.as_ref().is_some_and(|item| match item {
                ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => {
                    pat_has_arrow_lexical_environment_hazard(p)
                }
            })
        }),
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(name, p) => {
                prop_name_has_arrow_lexical_environment_hazard(name)
                    || pat_has_arrow_lexical_environment_hazard(p)
            }
            ObjPatProp::ShorthandAssign(_, e, _) => expr_has_arrow_lexical_environment_hazard(e),
            ObjPatProp::Rest(p) => pat_has_arrow_lexical_environment_hazard(p),
            ObjPatProp::Shorthand(_, _) => false,
        }),
    }
}

fn for_in_of_left_has_arrow_lexical_environment_hazard(left: &ForInOfLeft) -> bool {
    match left {
        ForInOfLeft::Pat(p) => pat_has_arrow_lexical_environment_hazard(p),
        ForInOfLeft::Expr(e) => expr_has_arrow_lexical_environment_hazard(e),
        ForInOfLeft::Var(v) => v.declarations.iter().any(|d| {
            pat_has_arrow_lexical_environment_hazard(&d.name)
                || d.init
                    .as_ref()
                    .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
        }),
    }
}

fn stmt_has_arrow_lexical_environment_hazard(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
            expr_has_arrow_lexical_environment_hazard(e)
        }
        StmtKind::Return(e) => e
            .as_ref()
            .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e)),
        StmtKind::Var(v) => v.declarations.iter().any(|d| {
            pat_has_arrow_lexical_environment_hazard(&d.name)
                || d.init
                    .as_ref()
                    .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
        }),
        StmtKind::If(i) => {
            expr_has_arrow_lexical_environment_hazard(&i.test)
                || stmt_has_arrow_lexical_environment_hazard(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|s| stmt_has_arrow_lexical_environment_hazard(s))
        }
        StmtKind::While(w) => {
            expr_has_arrow_lexical_environment_hazard(&w.test)
                || stmt_has_arrow_lexical_environment_hazard(&w.body)
        }
        StmtKind::DoWhile(w) => {
            stmt_has_arrow_lexical_environment_hazard(&w.body)
                || expr_has_arrow_lexical_environment_hazard(&w.test)
        }
        StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|init| match init {
                ForInit::Expr(e) => expr_has_arrow_lexical_environment_hazard(e),
                ForInit::Var(v) => v.declarations.iter().any(|d| {
                    pat_has_arrow_lexical_environment_hazard(&d.name)
                        || d.init
                            .as_ref()
                            .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
                }),
            }) || f
                .test
                .as_ref()
                .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
                || stmt_has_arrow_lexical_environment_hazard(&f.body)
        }
        StmtKind::ForIn(f) => {
            for_in_of_left_has_arrow_lexical_environment_hazard(&f.left)
                || expr_has_arrow_lexical_environment_hazard(&f.right)
                || stmt_has_arrow_lexical_environment_hazard(&f.body)
        }
        StmtKind::ForOf(f) => {
            for_in_of_left_has_arrow_lexical_environment_hazard(&f.left)
                || expr_has_arrow_lexical_environment_hazard(&f.right)
                || stmt_has_arrow_lexical_environment_hazard(&f.body)
        }
        StmtKind::Switch(s) => {
            expr_has_arrow_lexical_environment_hazard(&s.discriminant)
                || s.cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
                        || case
                            .consequent
                            .iter()
                            .any(stmt_has_arrow_lexical_environment_hazard)
                })
        }
        StmtKind::Try(t) => {
            t.block
                .iter()
                .any(stmt_has_arrow_lexical_environment_hazard)
                || t.handler.as_ref().is_some_and(|h| {
                    h.param
                        .as_ref()
                        .is_some_and(pat_has_arrow_lexical_environment_hazard)
                        || h.body.iter().any(stmt_has_arrow_lexical_environment_hazard)
                })
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_has_arrow_lexical_environment_hazard))
        }
        StmtKind::Block(stmts) => stmts.iter().any(stmt_has_arrow_lexical_environment_hazard),
        StmtKind::Labeled(l) => stmt_has_arrow_lexical_environment_hazard(&l.body),
        StmtKind::With(w) => {
            expr_has_arrow_lexical_environment_hazard(&w.object)
                || stmt_has_arrow_lexical_environment_hazard(&w.body)
        }
        StmtKind::ImportEquals(i) => expr_has_arrow_lexical_environment_hazard(&i.module_ref),
        StmtKind::Export(e) => match &e.kind {
            ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => {
                stmt_has_arrow_lexical_environment_hazard(s)
            }
            ExportDeclKind::Default(e) => expr_has_arrow_lexical_environment_hazard(e),
            _ => false,
        },
        // These declarations establish lexical-environment boundaries for the
        // references relevant to arrow-to-function conversion.
        StmtKind::ClassDecl(class_decl) => {
            class_external_has_arrow_lexical_environment_hazard(class_decl)
        }
        StmtKind::FnDecl(_)
        | StmtKind::ModuleDecl(_)
        | StmtKind::Import(_)
        | StmtKind::InterfaceDecl(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::EnumDecl(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Empty
        | StmtKind::Debugger => false,
    }
}

thread_local! {
    /// While set, `this` is not a hazard: the caller captures it (`_this`).
    static ARROW_THIS_CAPTURED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether an arrow reads `arguments`, `super` or `new.target` (anything
/// besides `this`, which a `var _this = this` capture covers).
pub(crate) fn arrow_has_non_this_lexical_hazard(arrow: &ArrowFn) -> bool {
    let previous = ARROW_THIS_CAPTURED.with(|flag| flag.replace(true));
    let hazard = arrow_has_lexical_environment_hazard(arrow);
    ARROW_THIS_CAPTURED.with(|flag| flag.set(previous));
    hazard
}

fn expr_has_arrow_lexical_environment_hazard(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::This => !ARROW_THIS_CAPTURED.with(std::cell::Cell::get),
        ExprKind::Super => true,
        ExprKind::Ident(name) => name == "arguments",
        ExprKind::MetaProp(meta) => meta.meta == "new" && meta.property == "target",
        ExprKind::Arrow(arrow) => arrow_has_lexical_environment_hazard(arrow),
        ExprKind::FnExpr(_) => false,
        ExprKind::ClassExpr(class_decl) => {
            class_external_has_arrow_lexical_environment_hazard(class_decl)
        }
        ExprKind::Call(call) => {
            expr_has_arrow_lexical_environment_hazard(&call.callee)
                || call
                    .args
                    .iter()
                    .any(|e| expr_has_arrow_lexical_environment_hazard(e))
        }
        ExprKind::New(e) => {
            expr_has_arrow_lexical_environment_hazard(&e.callee)
                || e.args.as_ref().is_some_and(|args| {
                    args.iter()
                        .any(|e| expr_has_arrow_lexical_environment_hazard(e))
                })
        }
        ExprKind::Member(e) => expr_has_arrow_lexical_environment_hazard(&e.object),
        ExprKind::ElemAccess(e) => {
            expr_has_arrow_lexical_environment_hazard(&e.object)
                || expr_has_arrow_lexical_environment_hazard(&e.index)
        }
        ExprKind::Cond(e) => {
            expr_has_arrow_lexical_environment_hazard(&e.test)
                || expr_has_arrow_lexical_environment_hazard(&e.consequent)
                || expr_has_arrow_lexical_environment_hazard(&e.alternate)
        }
        ExprKind::Binary(e) => {
            expr_has_arrow_lexical_environment_hazard(&e.left)
                || expr_has_arrow_lexical_environment_hazard(&e.right)
        }
        ExprKind::Unary(e) => expr_has_arrow_lexical_environment_hazard(&e.argument),
        ExprKind::Update(e) => expr_has_arrow_lexical_environment_hazard(&e.argument),
        ExprKind::Assign(e) => {
            expr_has_arrow_lexical_environment_hazard(&e.left)
                || expr_has_arrow_lexical_environment_hazard(&e.right)
        }
        ExprKind::Paren(e)
        | ExprKind::NonNull(e)
        | ExprKind::Spread(e)
        | ExprKind::Await(e)
        | ExprKind::Delete(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e) => expr_has_arrow_lexical_environment_hazard(e),
        ExprKind::TypeAssertion(e) => expr_has_arrow_lexical_environment_hazard(&e.expr),
        ExprKind::As(e) => expr_has_arrow_lexical_environment_hazard(&e.expr),
        ExprKind::Satisfies(e) => expr_has_arrow_lexical_environment_hazard(&e.expr),
        ExprKind::Instantiation(e) => expr_has_arrow_lexical_environment_hazard(&e.expr),
        ExprKind::Yield(_, e) => e
            .as_ref()
            .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e)),
        ExprKind::ArrayLit(items) => items.iter().any(|e| {
            e.as_ref()
                .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e))
        }),
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(p) => {
                prop_name_has_arrow_lexical_environment_hazard(&p.key)
                    || expr_has_arrow_lexical_environment_hazard(&p.value)
            }
            ObjLitProp::Spread(e, _) => expr_has_arrow_lexical_environment_hazard(e),
            ObjLitProp::ShorthandDefault(_, e, _) => expr_has_arrow_lexical_environment_hazard(e),
            ObjLitProp::Method(m) => prop_name_has_arrow_lexical_environment_hazard(&m.name),
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                prop_name_has_arrow_lexical_environment_hazard(&a.name)
            }
            ObjLitProp::Shorthand(_, _) => false,
        }),
        ExprKind::Template(t) => t
            .exprs
            .iter()
            .any(|e| expr_has_arrow_lexical_environment_hazard(e)),
        ExprKind::TaggedTemplate(t) => {
            expr_has_arrow_lexical_environment_hazard(&t.tag)
                || t.quasi
                    .exprs
                    .iter()
                    .any(|e| expr_has_arrow_lexical_environment_hazard(e))
        }
        ExprKind::Comma(exprs) => exprs
            .iter()
            .any(|e| expr_has_arrow_lexical_environment_hazard(e)),
        ExprKind::JsxElement(el) => {
            expr_has_arrow_lexical_environment_hazard(&el.name)
                || el.attributes.iter().any(|a| match a {
                    JsxAttribute::Normal { value, .. } => value
                        .as_ref()
                        .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e)),
                    JsxAttribute::Spread(e, _) => expr_has_arrow_lexical_environment_hazard(e),
                })
                || el
                    .children
                    .iter()
                    .any(jsx_child_has_arrow_lexical_environment_hazard)
        }
        ExprKind::JsxSelfClosing(el) => {
            expr_has_arrow_lexical_environment_hazard(&el.name)
                || el.attributes.iter().any(|a| match a {
                    JsxAttribute::Normal { value, .. } => value
                        .as_ref()
                        .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e)),
                    JsxAttribute::Spread(e, _) => expr_has_arrow_lexical_environment_hazard(e),
                })
        }
        ExprKind::JsxFragment(f) => f
            .children
            .iter()
            .any(jsx_child_has_arrow_lexical_environment_hazard),
        _ => false,
    }
}

fn jsx_child_has_arrow_lexical_environment_hazard(child: &JsxChild) -> bool {
    match child {
        JsxChild::Element(e) => expr_has_arrow_lexical_environment_hazard(e),
        JsxChild::Expression(e, _) => e
            .as_ref()
            .is_some_and(|e| expr_has_arrow_lexical_environment_hazard(e)),
        JsxChild::Fragment(f) => f
            .children
            .iter()
            .any(jsx_child_has_arrow_lexical_environment_hazard),
        JsxChild::Text(_, _) => false,
    }
}

/// `new.target` is lexical through nested arrows but not through a nested
/// normal function or class. The simple parameter bridge leaves it untouched,
/// so reject such initializers until the dedicated meta-property transform is
/// available.
pub(crate) fn expr_has_lexical_new_target(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::MetaProp(meta) => meta.meta == "new" && meta.property == "target",
        ExprKind::Arrow(arrow) => {
            arrow.params.iter().any(|p| {
                p.initializer
                    .as_ref()
                    .is_some_and(|e| expr_has_lexical_new_target(e))
            }) || match &arrow.body {
                ArrowBody::Expr(e) => expr_has_lexical_new_target(e),
                ArrowBody::Block(stmts) => stmts.iter().any(stmt_has_lexical_new_target),
            }
        }
        ExprKind::FnExpr(_) => false,
        ExprKind::ClassExpr(class_decl) => class_external_has_lexical_new_target(class_decl),
        ExprKind::Call(e) => {
            expr_has_lexical_new_target(&e.callee)
                || e.args.iter().any(|e| expr_has_lexical_new_target(e))
        }
        ExprKind::New(e) => {
            expr_has_lexical_new_target(&e.callee)
                || e.args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|e| expr_has_lexical_new_target(e)))
        }
        ExprKind::Member(e) => expr_has_lexical_new_target(&e.object),
        ExprKind::ElemAccess(e) => {
            expr_has_lexical_new_target(&e.object) || expr_has_lexical_new_target(&e.index)
        }
        ExprKind::Cond(e) => {
            expr_has_lexical_new_target(&e.test)
                || expr_has_lexical_new_target(&e.consequent)
                || expr_has_lexical_new_target(&e.alternate)
        }
        ExprKind::Binary(e) => {
            expr_has_lexical_new_target(&e.left) || expr_has_lexical_new_target(&e.right)
        }
        ExprKind::Unary(e) => expr_has_lexical_new_target(&e.argument),
        ExprKind::Update(e) => expr_has_lexical_new_target(&e.argument),
        ExprKind::Assign(e) => {
            expr_has_lexical_new_target(&e.left) || expr_has_lexical_new_target(&e.right)
        }
        ExprKind::Paren(e)
        | ExprKind::NonNull(e)
        | ExprKind::Spread(e)
        | ExprKind::Await(e)
        | ExprKind::Delete(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e) => expr_has_lexical_new_target(e),
        ExprKind::TypeAssertion(e) => expr_has_lexical_new_target(&e.expr),
        ExprKind::As(e) => expr_has_lexical_new_target(&e.expr),
        ExprKind::Satisfies(e) => expr_has_lexical_new_target(&e.expr),
        ExprKind::Instantiation(e) => expr_has_lexical_new_target(&e.expr),
        ExprKind::Yield(_, e) => e.as_ref().is_some_and(|e| expr_has_lexical_new_target(e)),
        ExprKind::ArrayLit(items) => items
            .iter()
            .any(|e| e.as_ref().is_some_and(|e| expr_has_lexical_new_target(e))),
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(p) => {
                matches!(&p.key, PropName::Computed(e, _) if expr_has_lexical_new_target(e))
                    || expr_has_lexical_new_target(&p.value)
            }
            ObjLitProp::Spread(e, _) => expr_has_lexical_new_target(e),
            ObjLitProp::ShorthandDefault(_, e, _) => expr_has_lexical_new_target(e),
            ObjLitProp::Method(m) => {
                matches!(&m.name, PropName::Computed(e, _) if expr_has_lexical_new_target(e))
            }
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                matches!(&a.name, PropName::Computed(e, _) if expr_has_lexical_new_target(e))
            }
            ObjLitProp::Shorthand(_, _) => false,
        }),
        ExprKind::Template(t) => t.exprs.iter().any(|e| expr_has_lexical_new_target(e)),
        ExprKind::TaggedTemplate(t) => {
            expr_has_lexical_new_target(&t.tag)
                || t.quasi.exprs.iter().any(|e| expr_has_lexical_new_target(e))
        }
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_lexical_new_target(e)),
        // JSX/default initializers containing new.target are uncommon; the
        // complete arrow hazard walk conservatively keeps those native.
        ExprKind::JsxElement(_) | ExprKind::JsxSelfClosing(_) | ExprKind::JsxFragment(_) => {
            expr_has_arrow_lexical_environment_hazard(expr)
        }
        _ => false,
    }
}

fn stmt_has_lexical_new_target(stmt: &Stmt) -> bool {
    // Reuse the complete structural arrow hazard walk: in an arrow/default
    // initializer any lexical hazard makes conversion unsafe, and this helper
    // is only reached beneath a nested arrow while searching for new.target.
    match &stmt.kind {
        StmtKind::ClassDecl(class_decl) => class_external_has_lexical_new_target(class_decl),
        StmtKind::FnDecl(_) | StmtKind::ModuleDecl(_) => false,
        _ => stmt.span.start < stmt.span.end && self_contained_stmt_exprs_have_new_target(stmt),
    }
}

fn self_contained_stmt_exprs_have_new_target(stmt: &Stmt) -> bool {
    // The general hazard walker is deliberately stronger here. False positives
    // only narrow the simple bridge for an already-complex nested-arrow default.
    stmt_has_arrow_lexical_environment_hazard(stmt)
}

fn prop_name_has_lexical_arguments(name: &PropName) -> bool {
    matches!(name, PropName::Computed(expr, _) if expr_has_lexical_arguments(expr))
}

fn params_have_lexical_arguments(params: &[Param]) -> bool {
    params.iter().any(|p| {
        p.initializer
            .as_ref()
            .is_some_and(|init| expr_has_lexical_arguments(init))
    })
}

fn class_member_has_lexical_arguments(member: &ClassMember) -> bool {
    match &member.kind {
        ClassMemberKind::Property(prop) => {
            prop_name_has_lexical_arguments(&prop.name)
                || prop
                    .initializer
                    .as_ref()
                    .is_some_and(|init| expr_has_lexical_arguments(init))
        }
        ClassMemberKind::Method(method) => {
            prop_name_has_lexical_arguments(&method.name)
                || params_have_lexical_arguments(&method.params)
        }
        ClassMemberKind::Constructor(ctor) => params_have_lexical_arguments(&ctor.params),
        ClassMemberKind::GetAccessor(acc) => {
            prop_name_has_lexical_arguments(&acc.name) || params_have_lexical_arguments(&acc.params)
        }
        ClassMemberKind::SetAccessor(acc) => {
            prop_name_has_lexical_arguments(&acc.name) || params_have_lexical_arguments(&acc.params)
        }
        ClassMemberKind::StaticBlock(stmts) => stmts_have_lexical_arguments(stmts),
        _ => false,
    }
}

/// True when a statement list contains a lexical `arguments` reference.
/// Function/class declaration bodies are treated as argument-binding boundaries.
pub(crate) fn stmts_have_lexical_arguments(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_lexical_arguments)
}

fn stmt_has_lexical_arguments(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) => expr_has_lexical_arguments(expr),
        StmtKind::Return(expr) => expr.as_ref().is_some_and(|e| expr_has_lexical_arguments(e)),
        StmtKind::Var(v) => v.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_lexical_arguments(e))
        }),
        StmtKind::If(i) => {
            expr_has_lexical_arguments(&i.test)
                || stmt_has_lexical_arguments(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|s| stmt_has_lexical_arguments(s))
        }
        StmtKind::While(w) => {
            expr_has_lexical_arguments(&w.test) || stmt_has_lexical_arguments(&w.body)
        }
        StmtKind::DoWhile(dw) => {
            stmt_has_lexical_arguments(&dw.body) || expr_has_lexical_arguments(&dw.test)
        }
        StmtKind::For(f) => {
            let init_has = f.init.as_ref().is_some_and(|init| match init {
                ForInit::Var(v) => v.declarations.iter().any(|d| {
                    d.init
                        .as_ref()
                        .is_some_and(|e| expr_has_lexical_arguments(e))
                }),
                ForInit::Expr(e) => expr_has_lexical_arguments(e),
            });
            init_has
                || f.test
                    .as_ref()
                    .is_some_and(|e| expr_has_lexical_arguments(e))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_has_lexical_arguments(e))
                || stmt_has_lexical_arguments(&f.body)
        }
        StmtKind::ForIn(fi) => {
            for_in_of_left_has_lexical_arguments(&fi.left)
                || expr_has_lexical_arguments(&fi.right)
                || stmt_has_lexical_arguments(&fi.body)
        }
        StmtKind::ForOf(fo) => {
            for_in_of_left_has_lexical_arguments(&fo.left)
                || expr_has_lexical_arguments(&fo.right)
                || stmt_has_lexical_arguments(&fo.body)
        }
        StmtKind::Switch(sw) => {
            expr_has_lexical_arguments(&sw.discriminant)
                || sw.cases.iter().any(|c| {
                    c.test
                        .as_ref()
                        .is_some_and(|e| expr_has_lexical_arguments(e))
                        || c.consequent.iter().any(stmt_has_lexical_arguments)
                })
        }
        StmtKind::Try(t) => {
            t.block.iter().any(stmt_has_lexical_arguments)
                || t.handler.as_ref().is_some_and(|h| {
                    h.body.iter().any(stmt_has_lexical_arguments)
                        || h.param
                            .as_ref()
                            .is_some_and(|p| pat_has_lexical_arguments(p))
                })
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_has_lexical_arguments))
        }
        StmtKind::Throw(e) => expr_has_lexical_arguments(e),
        StmtKind::Labeled(l) => stmt_has_lexical_arguments(&l.body),
        StmtKind::With(w) => {
            expr_has_lexical_arguments(&w.object) || stmt_has_lexical_arguments(&w.body)
        }
        StmtKind::ImportEquals(ie) => expr_has_lexical_arguments(&ie.module_ref),
        StmtKind::ExportAssign(e) => expr_has_lexical_arguments(e),
        StmtKind::Export(ed) => export_decl_has_lexical_arguments(ed),
        StmtKind::Block(stmts) => stmts_have_lexical_arguments(stmts),
        // Function/class declarations create their own argument scope.
        StmtKind::FnDecl(_)
        | StmtKind::ClassDecl(_)
        | StmtKind::Import(_)
        | StmtKind::ModuleDecl(_)
        | StmtKind::InterfaceDecl(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::EnumDecl(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Empty
        | StmtKind::Debugger => false,
    }
}

fn export_decl_has_lexical_arguments(decl: &ExportDecl) -> bool {
    match &decl.kind {
        ExportDeclKind::Decl(stmt) => stmt_has_lexical_arguments(stmt),
        ExportDeclKind::Default(expr) => expr_has_lexical_arguments(expr),
        ExportDeclKind::DefaultDecl(stmt) => stmt_has_lexical_arguments(stmt),
        _ => false,
    }
}

fn for_in_of_left_has_lexical_arguments(left: &ForInOfLeft) -> bool {
    match left {
        ForInOfLeft::Pat(pat) => pat_has_lexical_arguments(pat),
        ForInOfLeft::Expr(expr) => expr_has_lexical_arguments(expr),
        ForInOfLeft::Var(v) => v.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_lexical_arguments(e))
        }),
    }
}

fn pat_has_lexical_arguments(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Assign(lhs, rhs) => {
            pat_has_lexical_arguments(lhs) || expr_has_lexical_arguments(rhs)
        }
        PatKind::Array(items) => items.iter().any(|item| {
            item.as_ref().is_some_and(|elem| match elem {
                ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pat_has_lexical_arguments(p),
            })
        }),
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(_, p) | ObjPatProp::Rest(p) => pat_has_lexical_arguments(p),
            ObjPatProp::ShorthandAssign(_, init, _) => expr_has_lexical_arguments(init),
            ObjPatProp::Shorthand(_, _) => false,
        }),
        PatKind::Rest(inner) => pat_has_lexical_arguments(inner),
        PatKind::Ident(_) => false,
    }
}

/// Check if an expression contains a lexical `arguments` reference.
/// Arrow functions are transparent; regular functions/methods are boundaries.
pub(crate) fn expr_has_lexical_arguments(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) => name == "arguments",
        ExprKind::Arrow(arrow) => {
            params_have_lexical_arguments(&arrow.params)
                || match &arrow.body {
                    ArrowBody::Expr(e) => expr_has_lexical_arguments(e),
                    ArrowBody::Block(stmts) => stmts_have_lexical_arguments(stmts),
                }
        }
        ExprKind::FnExpr(_) => false,
        ExprKind::ClassExpr(class_decl) => {
            class_decl
                .extends
                .as_ref()
                .is_some_and(|e| expr_has_lexical_arguments(e))
                || class_decl
                    .members
                    .iter()
                    .any(class_member_has_lexical_arguments)
        }
        ExprKind::Call(call) => {
            expr_has_lexical_arguments(&call.callee)
                || call.args.iter().any(|a| expr_has_lexical_arguments(a))
        }
        ExprKind::New(new_expr) => {
            expr_has_lexical_arguments(&new_expr.callee)
                || new_expr
                    .args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|a| expr_has_lexical_arguments(a)))
        }
        ExprKind::Member(mem) => expr_has_lexical_arguments(&mem.object),
        ExprKind::ElemAccess(ea) => {
            expr_has_lexical_arguments(&ea.object) || expr_has_lexical_arguments(&ea.index)
        }
        ExprKind::Cond(c) => {
            expr_has_lexical_arguments(&c.test)
                || expr_has_lexical_arguments(&c.consequent)
                || expr_has_lexical_arguments(&c.alternate)
        }
        ExprKind::Binary(b) => {
            expr_has_lexical_arguments(&b.left) || expr_has_lexical_arguments(&b.right)
        }
        ExprKind::Unary(u) => expr_has_lexical_arguments(&u.argument),
        ExprKind::Update(u) => expr_has_lexical_arguments(&u.argument),
        ExprKind::Assign(a) => {
            expr_has_lexical_arguments(&a.left) || expr_has_lexical_arguments(&a.right)
        }
        ExprKind::Paren(inner)
        | ExprKind::NonNull(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Await(inner)
        | ExprKind::Delete(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner) => expr_has_lexical_arguments(inner),
        ExprKind::TypeAssertion(ta) => expr_has_lexical_arguments(&ta.expr),
        ExprKind::As(a) => expr_has_lexical_arguments(&a.expr),
        ExprKind::Satisfies(s) => expr_has_lexical_arguments(&s.expr),
        ExprKind::Instantiation(inst) => expr_has_lexical_arguments(&inst.expr),
        ExprKind::Yield(_, arg) => arg.as_ref().is_some_and(|e| expr_has_lexical_arguments(e)),
        ExprKind::ArrayLit(items) => items
            .iter()
            .any(|item| item.as_ref().is_some_and(|e| expr_has_lexical_arguments(e))),
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(p) => {
                prop_name_has_lexical_arguments(&p.key) || expr_has_lexical_arguments(&p.value)
            }
            ObjLitProp::Spread(e, _) => expr_has_lexical_arguments(e),
            ObjLitProp::ShorthandDefault(_, init, _) => expr_has_lexical_arguments(init),
            ObjLitProp::Method(m) => {
                prop_name_has_lexical_arguments(&m.name) || params_have_lexical_arguments(&m.params)
            }
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                prop_name_has_lexical_arguments(&a.name) || params_have_lexical_arguments(&a.params)
            }
            ObjLitProp::Shorthand(_, _) => false,
        }),
        ExprKind::Template(t) => t.exprs.iter().any(|e| expr_has_lexical_arguments(e)),
        ExprKind::TaggedTemplate(tt) => {
            expr_has_lexical_arguments(&tt.tag)
                || tt.quasi.exprs.iter().any(|e| expr_has_lexical_arguments(e))
        }
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_lexical_arguments(e)),
        _ => false,
    }
}

/// Check if an expression contains an async function/arrow.
pub(crate) fn expr_has_async(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(fn_decl) => fn_decl.is_async || params_have_async(&fn_decl.params),
        ExprKind::Arrow(arrow) => {
            arrow.is_async
                || match &arrow.body {
                    ArrowBody::Expr(e) => expr_has_async(e),
                    ArrowBody::Block(stmts) => stmts.iter().any(|s| source_has_async(s)),
                }
        }
        ExprKind::Call(call) => {
            expr_has_async(&call.callee) || call.args.iter().any(|a| expr_has_async(a))
        }
        ExprKind::Assign(assign) => expr_has_async(&assign.right),
        ExprKind::Paren(inner) => expr_has_async(inner),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Method(m) => m.is_async,
            ObjLitProp::Property(p) => expr_has_async(&p.value),
            _ => false,
        }),
        // Class expressions create their own scope — async methods inside them
        // don't affect whether the enclosing scope needs an async wrapper.
        ExprKind::ClassExpr(_) => false,
        _ => false,
    }
}

/// Check if a statement contains a for-of loop (for __values helper scanning).
pub(crate) fn source_has_for_of(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ForOf(_) => true,
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                source_has_for_of(inner)
            }
            _ => false,
        },
        StmtKind::Block(stmts) => stmts.iter().any(source_has_for_of),
        StmtKind::If(if_stmt) => {
            source_has_for_of(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|s| source_has_for_of(s))
        }
        StmtKind::While(wh) => source_has_for_of(&wh.body),
        StmtKind::DoWhile(dw) => source_has_for_of(&dw.body),
        StmtKind::For(f) => source_has_for_of(&f.body),
        StmtKind::ForIn(fi) => source_has_for_of(&fi.body),
        StmtKind::FnDecl(fn_decl) => fn_decl
            .body
            .as_ref()
            .is_some_and(|stmts| stmts.iter().any(source_has_for_of)),
        StmtKind::Try(try_stmt) => {
            try_stmt.block.iter().any(source_has_for_of)
                || try_stmt
                    .handler
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(source_has_for_of))
                || try_stmt
                    .finalizer
                    .as_ref()
                    .is_some_and(|f: &Vec<Stmt>| f.iter().any(source_has_for_of))
        }
        StmtKind::ModuleDecl(m) => match &m.body {
            Some(ModuleBody::Block(stmts)) => stmts.iter().any(source_has_for_of),
            Some(ModuleBody::Module(inner)) => {
                let tmp = Stmt {
                    kind: StmtKind::ModuleDecl(Box::new(inner.as_ref().clone())),
                    span: stmt.span,
                };
                source_has_for_of(&tmp)
            }
            None => false,
        },
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(source_has_for_of)),
        StmtKind::Labeled(l) => source_has_for_of(&l.body),
        _ => false,
    }
}

pub(crate) fn source_has_for_await(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ForOf(fo) => fo.is_await || source_has_for_await(&fo.body),
        StmtKind::Export(export_decl) => {
            if let ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) =
                &export_decl.kind
            {
                source_has_for_await(inner)
            } else {
                false
            }
        }
        StmtKind::Block(stmts) => stmts.iter().any(source_has_for_await),
        StmtKind::If(if_stmt) => {
            source_has_for_await(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|s| source_has_for_await(s))
        }
        StmtKind::While(wh) => source_has_for_await(&wh.body),
        StmtKind::DoWhile(dw) => source_has_for_await(&dw.body),
        StmtKind::For(f) => source_has_for_await(&f.body),
        StmtKind::ForIn(fi) => source_has_for_await(&fi.body),
        StmtKind::FnDecl(fn_decl) => fn_decl
            .body
            .as_ref()
            .is_some_and(|stmts| stmts.iter().any(source_has_for_await)),
        StmtKind::Try(try_stmt) => {
            try_stmt.block.iter().any(source_has_for_await)
                || try_stmt
                    .handler
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(source_has_for_await))
                || try_stmt
                    .finalizer
                    .as_ref()
                    .is_some_and(|f: &Vec<Stmt>| f.iter().any(source_has_for_await))
        }
        StmtKind::ModuleDecl(m) => match &m.body {
            Some(ModuleBody::Block(stmts)) => stmts.iter().any(source_has_for_await),
            Some(ModuleBody::Module(inner)) => {
                let tmp = Stmt {
                    kind: StmtKind::ModuleDecl(Box::new(inner.as_ref().clone())),
                    span: Span::default(),
                };
                source_has_for_await(&tmp)
            }
            None => false,
        },
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(source_has_for_await)),
        StmtKind::Labeled(l) => source_has_for_await(&l.body),
        StmtKind::Expr(expr) => expr_has_for_await(expr),
        StmtKind::Var(var_stmt) => var_stmt
            .declarations
            .iter()
            .any(|d| d.init.as_ref().is_some_and(|e| expr_has_for_await(e))),
        _ => false,
    }
}

fn expr_has_for_await(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(e) => expr_has_for_await(e),
            ArrowBody::Block(stmts) => stmts.iter().any(source_has_for_await),
        },
        ExprKind::FnExpr(fn_expr) => fn_expr
            .body
            .as_ref()
            .is_some_and(|b: &Vec<Stmt>| b.iter().any(source_has_for_await)),
        ExprKind::Call(call) => {
            expr_has_for_await(&call.callee) || call.args.iter().any(|a| expr_has_for_await(a))
        }
        ExprKind::Paren(inner) => expr_has_for_await(inner),
        _ => false,
    }
}

#[allow(dead_code)]
pub(crate) fn class_uses_private_field_helpers_stmt(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ClassDecl(c) => class_uses_private_field_helpers(c),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                class_uses_private_field_helpers_stmt(inner)
            }
            _ => false,
        },
        _ => false,
    }
}

/// Check if a statement contains a class that needs __classPrivateFieldGet
/// and/or __classPrivateFieldSet helpers. Returns (needs_get, needs_set).
pub(crate) fn class_needs_private_field_helpers_stmt(stmt: &Stmt) -> (bool, bool) {
    match &stmt.kind {
        StmtKind::ClassDecl(c) => class_needs_private_field_helpers(c),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                class_needs_private_field_helpers_stmt(inner)
            }
            _ => (false, false),
        },
        StmtKind::Var(var_stmt) => {
            let mut needs_get = false;
            let mut needs_set = false;
            for d in &var_stmt.declarations {
                if let Some(init) = &d.init {
                    if let ExprKind::ClassExpr(c) = &init.kind {
                        let (g, s) = class_needs_private_field_helpers(c);
                        needs_get |= g;
                        needs_set |= s;
                    }
                }
            }
            (needs_get, needs_set)
        }
        _ => (false, false),
    }
}

/// Analyze a class to determine which private field helpers are needed.
/// Returns (needs_get, needs_set).
/// Only counts accesses to private fields that are actually declared in this class.
fn class_needs_private_field_helpers(class: &ClassDecl) -> (bool, bool) {
    // First check if the class has any declared private fields/methods/accessors.
    // If not, no helper is needed even if the body references undeclared private names.
    let has_declared_private = class.members.iter().any(|m| match &m.kind {
        ClassMemberKind::Property(p) => matches!(p.name, PropName::Private(_, _)),
        ClassMemberKind::Method(m) => matches!(m.name, PropName::Private(_, _)),
        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
            matches!(a.name, PropName::Private(_, _))
        }
        _ => false,
    });
    if !has_declared_private {
        return (false, false);
    }
    let declared_private_names: std::collections::HashSet<String> = class
        .members
        .iter()
        .filter_map(|m| match &m.kind {
            ClassMemberKind::Property(p) => {
                if let PropName::Private(name, _) = &p.name {
                    Some(normalize_unicode_escapes(name))
                } else {
                    None
                }
            }
            ClassMemberKind::Method(m) => {
                if let PropName::Private(name, _) = &m.name {
                    Some(normalize_unicode_escapes(name))
                } else {
                    None
                }
            }
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                if let PropName::Private(name, _) = &a.name {
                    Some(normalize_unicode_escapes(name))
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    let mut needs_get = false;
    let mut needs_set = false;
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Method(m) => {
                if let Some(ref body) = m.body {
                    let (g, s) =
                        stmts_private_member_access_kind_declared(body, &declared_private_names);
                    needs_get |= g;
                    needs_set |= s;
                }
            }
            ClassMemberKind::Constructor(c) => {
                if let Some(ref body) = c.body {
                    let (g, s) =
                        stmts_private_member_access_kind_declared(body, &declared_private_names);
                    needs_get |= g;
                    needs_set |= s;
                }
            }
            ClassMemberKind::Property(p) => {
                if let Some(ref init) = p.initializer {
                    let (g, s) =
                        expr_private_member_access_kind_declared(init, &declared_private_names);
                    needs_get |= g;
                    needs_set |= s;
                }
            }
            ClassMemberKind::StaticBlock(stmts) => {
                let (g, s) =
                    stmts_private_member_access_kind_declared(stmts, &declared_private_names);
                needs_get |= g;
                needs_set |= s;
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if let Some(ref body) = acc.body {
                    let (g, s) =
                        stmts_private_member_access_kind_declared(body, &declared_private_names);
                    needs_get |= g;
                    needs_set |= s;
                }
            }
            _ => {}
        }
        if needs_get && needs_set {
            break;
        }
    }
    (needs_get, needs_set)
}

fn member_matches_declared_private(
    mem: &MemberExpr,
    declared_private_names: &std::collections::HashSet<String>,
) -> bool {
    mem.property
        .strip_prefix('#')
        .is_some_and(|name| declared_private_names.contains(name))
}

fn lhs_pattern_has_declared_private_write_target(
    expr: &Expr,
    declared_private_names: &std::collections::HashSet<String>,
) -> bool {
    match &expr.kind {
        ExprKind::Member(m) => member_matches_declared_private(m, declared_private_names),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(op) => {
                lhs_pattern_has_declared_private_write_target(&op.value, declared_private_names)
            }
            ObjLitProp::Spread(e, _) => {
                lhs_pattern_has_declared_private_write_target(e, declared_private_names)
            }
            _ => false,
        }),
        ExprKind::ArrayLit(elements) => elements
            .iter()
            .flatten()
            .any(|e| lhs_pattern_has_declared_private_write_target(e, declared_private_names)),
        ExprKind::Assign(a) => {
            lhs_pattern_has_declared_private_write_target(&a.left, declared_private_names)
        }
        ExprKind::Paren(e) | ExprKind::NonNull(e) => {
            lhs_pattern_has_declared_private_write_target(e, declared_private_names)
        }
        ExprKind::As(a) => {
            lhs_pattern_has_declared_private_write_target(&a.expr, declared_private_names)
        }
        ExprKind::Satisfies(s) => {
            lhs_pattern_has_declared_private_write_target(&s.expr, declared_private_names)
        }
        ExprKind::TypeAssertion(ta) => {
            lhs_pattern_has_declared_private_write_target(&ta.expr, declared_private_names)
        }
        _ => false,
    }
}

/// Determine the access kind of the FIRST private field usage in a class, in member source order.
/// Returns (first_is_get, first_is_set) for just the first member that has any private access.
/// This determines helper emission order (the helper referenced first should be emitted first).
pub(crate) fn class_first_private_access_stmt(stmt: &Stmt) -> (bool, bool) {
    match &stmt.kind {
        StmtKind::ClassDecl(c) => class_first_private_access(c),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                class_first_private_access_stmt(inner)
            }
            _ => (false, false),
        },
        StmtKind::Var(var_stmt) => {
            for d in &var_stmt.declarations {
                if let Some(init) = &d.init {
                    if let ExprKind::ClassExpr(c) = &init.kind {
                        let result = class_first_private_access(c);
                        if result.0 || result.1 {
                            return result;
                        }
                    }
                }
            }
            (false, false)
        }
        _ => (false, false),
    }
}

fn class_first_private_access(class: &ClassDecl) -> (bool, bool) {
    // Collect this class's own private member names so we only count
    // accesses to our own privates (not cross-class error references).
    let own_private_names: HashSet<&str> = class
        .members
        .iter()
        .filter_map(|m| match &m.kind {
            ClassMemberKind::Property(p) => {
                if let PropName::Private(name, _) = &p.name {
                    Some(name.as_str())
                } else {
                    None
                }
            }
            ClassMemberKind::Method(method) => {
                if let PropName::Private(name, _) = &method.name {
                    Some(name.as_str())
                } else {
                    None
                }
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if let PropName::Private(name, _) = &acc.name {
                    Some(name.as_str())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    for member in &class.members {
        let (g, s) = match &member.kind {
            ClassMemberKind::Method(m) => {
                if let Some(ref body) = m.body {
                    first_private_access_in_stmts_filtered(body, &own_private_names)
                } else {
                    (false, false)
                }
            }
            ClassMemberKind::Constructor(c) => {
                if let Some(ref body) = c.body {
                    first_private_access_in_stmts_filtered(body, &own_private_names)
                } else {
                    (false, false)
                }
            }
            ClassMemberKind::Property(p) => {
                if let Some(ref init) = p.initializer {
                    first_private_access_in_expr_filtered(init, &own_private_names)
                } else {
                    (false, false)
                }
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if let Some(ref body) = acc.body {
                    first_private_access_in_stmts_filtered(body, &own_private_names)
                } else {
                    (false, false)
                }
            }
            ClassMemberKind::StaticBlock(stmts) => {
                first_private_access_in_stmts_filtered(stmts, &own_private_names)
            }
            _ => (false, false),
        };
        if g || s {
            // Return the access kind of just this first member, not the aggregate.
            return (g, s);
        }
    }
    (false, false)
}

/// Filtered version of first_private_access_in_stmts that only counts
/// accesses to names in the given set.
fn first_private_access_in_stmts_filtered(
    stmts: &[Stmt],
    own_names: &HashSet<&str>,
) -> (bool, bool) {
    for stmt in stmts {
        let (g, s) = first_private_access_in_stmt_filtered(stmt, own_names);
        if g || s {
            return (g, s);
        }
    }
    (false, false)
}

fn first_private_access_in_stmt_filtered(stmt: &Stmt, own_names: &HashSet<&str>) -> (bool, bool) {
    match &stmt.kind {
        StmtKind::Expr(e) => first_private_access_in_expr_filtered(e, own_names),
        StmtKind::Return(Some(e)) | StmtKind::Throw(e) => {
            first_private_access_in_expr_filtered(e, own_names)
        }
        StmtKind::Var(var_stmt) => {
            for d in &var_stmt.declarations {
                if let Some(ref init) = d.init {
                    let r = first_private_access_in_expr_filtered(init, own_names);
                    if r.0 || r.1 {
                        return r;
                    }
                }
            }
            (false, false)
        }
        StmtKind::Block(stmts) => first_private_access_in_stmts_filtered(stmts, own_names),
        StmtKind::If(if_stmt) => {
            let r = first_private_access_in_expr_filtered(&if_stmt.test, own_names);
            if r.0 || r.1 {
                return r;
            }
            let r = first_private_access_in_stmt_filtered(&if_stmt.consequent, own_names);
            if r.0 || r.1 {
                return r;
            }
            if let Some(ref alt) = if_stmt.alternate {
                return first_private_access_in_stmt_filtered(alt, own_names);
            }
            (false, false)
        }
        StmtKind::For(f) => {
            if let Some(ref init) = f.init {
                let r = match init {
                    ForInit::Expr(e) => first_private_access_in_expr_filtered(e, own_names),
                    ForInit::Var(var_stmt) => {
                        let mut result = (false, false);
                        for d in &var_stmt.declarations {
                            if let Some(ref e) = d.init {
                                let r = first_private_access_in_expr_filtered(e, own_names);
                                if r.0 || r.1 {
                                    result = r;
                                    break;
                                }
                            }
                        }
                        result
                    }
                };
                if r.0 || r.1 {
                    return r;
                }
            }
            if let Some(ref e) = f.test {
                let r = first_private_access_in_expr_filtered(e, own_names);
                if r.0 || r.1 {
                    return r;
                }
            }
            if let Some(ref e) = f.update {
                let r = first_private_access_in_expr_filtered(e, own_names);
                if r.0 || r.1 {
                    return r;
                }
            }
            first_private_access_in_stmt_filtered(&f.body, own_names)
        }
        _ => (false, false),
    }
}

/// Check if an expression is or contains a write to an own private field.
/// Handles destructuring patterns like `{ key: this.#field } = arg`.
fn lhs_has_own_private_write_target(expr: &Expr, own_names: &HashSet<&str>) -> bool {
    match &expr.kind {
        ExprKind::Member(m) => m.property.starts_with('#') && own_names.contains(&m.property[1..]),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(op) => lhs_has_own_private_write_target(&op.value, own_names),
            ObjLitProp::Spread(e, _) => lhs_has_own_private_write_target(e, own_names),
            _ => false,
        }),
        ExprKind::ArrayLit(elements) => elements
            .iter()
            .flatten()
            .any(|e| lhs_has_own_private_write_target(e, own_names)),
        ExprKind::Assign(a) => lhs_has_own_private_write_target(&a.left, own_names),
        ExprKind::Paren(e) | ExprKind::NonNull(e) => lhs_has_own_private_write_target(e, own_names),
        ExprKind::As(a) => lhs_has_own_private_write_target(&a.expr, own_names),
        ExprKind::Satisfies(s) => lhs_has_own_private_write_target(&s.expr, own_names),
        ExprKind::TypeAssertion(ta) => lhs_has_own_private_write_target(&ta.expr, own_names),
        _ => false,
    }
}

fn first_private_access_in_expr_filtered(expr: &Expr, own_names: &HashSet<&str>) -> (bool, bool) {
    match &expr.kind {
        ExprKind::Assign(a) => {
            let lhs_is_private = lhs_has_own_private_write_target(&a.left, own_names);
            if lhs_is_private {
                if a.op == AssignOp::Assign {
                    return (false, true);
                } else {
                    return (true, true);
                }
            }
            let r = first_private_access_in_expr_filtered(&a.left, own_names);
            if r.0 || r.1 {
                return r;
            }
            first_private_access_in_expr_filtered(&a.right, own_names)
        }
        ExprKind::Update(u) => {
            if lhs_has_own_private_write_target(&u.argument, own_names) {
                return (true, true);
            }
            first_private_access_in_expr_filtered(&u.argument, own_names)
        }
        ExprKind::Member(m) => {
            if m.property.starts_with('#') && own_names.contains(&m.property[1..]) {
                return (true, false);
            }
            first_private_access_in_expr_filtered(&m.object, own_names)
        }
        ExprKind::Binary(b) => {
            let r = first_private_access_in_expr_filtered(&b.left, own_names);
            if r.0 || r.1 {
                return r;
            }
            first_private_access_in_expr_filtered(&b.right, own_names)
        }
        ExprKind::Paren(e)
        | ExprKind::NonNull(e)
        | ExprKind::Await(e)
        | ExprKind::Spread(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e)
        | ExprKind::Delete(e) => first_private_access_in_expr_filtered(e, own_names),
        ExprKind::Call(call) => {
            let r = first_private_access_in_expr_filtered(&call.callee, own_names);
            if r.0 || r.1 {
                return r;
            }
            for arg in &call.args {
                let r = first_private_access_in_expr_filtered(arg, own_names);
                if r.0 || r.1 {
                    return r;
                }
            }
            (false, false)
        }
        ExprKind::Cond(c) => {
            let r = first_private_access_in_expr_filtered(&c.test, own_names);
            if r.0 || r.1 {
                return r;
            }
            let r = first_private_access_in_expr_filtered(&c.consequent, own_names);
            if r.0 || r.1 {
                return r;
            }
            first_private_access_in_expr_filtered(&c.alternate, own_names)
        }
        ExprKind::Unary(u) => first_private_access_in_expr_filtered(&u.argument, own_names),
        ExprKind::TypeAssertion(ta) => first_private_access_in_expr_filtered(&ta.expr, own_names),
        ExprKind::As(a) => first_private_access_in_expr_filtered(&a.expr, own_names),
        ExprKind::Satisfies(s) => first_private_access_in_expr_filtered(&s.expr, own_names),
        ExprKind::FnExpr(f) => {
            if let Some(ref body) = f.body {
                first_private_access_in_stmts_filtered(body, own_names)
            } else {
                (false, false)
            }
        }
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Block(stmts) => first_private_access_in_stmts_filtered(stmts, own_names),
            ArrowBody::Expr(e) => first_private_access_in_expr_filtered(e, own_names),
        },
        ExprKind::ElemAccess(ea) => {
            let r = first_private_access_in_expr_filtered(&ea.object, own_names);
            if r.0 || r.1 {
                return r;
            }
            first_private_access_in_expr_filtered(&ea.index, own_names)
        }
        ExprKind::New(new) => {
            let r = first_private_access_in_expr_filtered(&new.callee, own_names);
            if r.0 || r.1 {
                return r;
            }
            if let Some(ref args) = new.args {
                for arg in args {
                    let r = first_private_access_in_expr_filtered(arg, own_names);
                    if r.0 || r.1 {
                        return r;
                    }
                }
            }
            (false, false)
        }
        ExprKind::TaggedTemplate(tt) => {
            let r = first_private_access_in_expr_filtered(&tt.tag, own_names);
            if r.0 || r.1 {
                return r;
            }
            for e in &tt.quasi.exprs {
                let r = first_private_access_in_expr_filtered(e, own_names);
                if r.0 || r.1 {
                    return r;
                }
            }
            (false, false)
        }
        ExprKind::Template(t) => {
            for e in &t.exprs {
                let r = first_private_access_in_expr_filtered(e, own_names);
                if r.0 || r.1 {
                    return r;
                }
            }
            (false, false)
        }
        ExprKind::ObjectLit(props) => {
            for p in props {
                match p {
                    ObjLitProp::Property(op) => {
                        let r = first_private_access_in_expr_filtered(&op.value, own_names);
                        if r.0 || r.1 {
                            return r;
                        }
                    }
                    ObjLitProp::Spread(e, _) => {
                        let r = first_private_access_in_expr_filtered(e, own_names);
                        if r.0 || r.1 {
                            return r;
                        }
                    }
                    _ => {}
                }
            }
            (false, false)
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems.iter().flatten() {
                let r = first_private_access_in_expr_filtered(e, own_names);
                if r.0 || r.1 {
                    return r;
                }
            }
            (false, false)
        }
        ExprKind::Comma(exprs) => {
            for e in exprs {
                let r = first_private_access_in_expr_filtered(e, own_names);
                if r.0 || r.1 {
                    return r;
                }
            }
            (false, false)
        }
        ExprKind::Yield(_, Some(e)) => first_private_access_in_expr_filtered(e, own_names),
        _ => (false, false),
    }
}

#[allow(dead_code)] // used by private field downlevel
pub(crate) fn class_has_private_fields_stmt(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ClassDecl(c) => class_has_private_fields(c),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                class_has_private_fields_stmt(inner)
            }
            _ => false,
        },
        _ => false,
    }
}

pub(crate) fn class_has_accessor_stmt(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ClassDecl(c) => class_has_accessor(c),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                class_has_accessor_stmt(inner)
            }
            _ => false,
        },
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init.as_ref().is_some_and(
                |init| matches!(&init.kind, ExprKind::ClassExpr(c) if class_has_accessor(c)),
            )
        }),
        _ => false,
    }
}

pub(crate) fn class_has_accessor(class: &ClassDecl) -> bool {
    class.members.iter().any(|m| {
        if let ClassMemberKind::Property(ref prop) = m.kind {
            prop.modifiers & MOD_ACCESSOR != 0
                && prop.modifiers & MOD_DECLARE == 0
                && prop.modifiers & MOD_ABSTRACT == 0
        } else {
            false
        }
    })
}

/// Check if an expression is a member access `ClassName.prop` where ClassName
/// matches the given class name. Returns Some(prop) if so.
pub(crate) fn expr_is_class_member_access<'a>(expr: &'a Expr, class_name: &str) -> Option<&'a str> {
    if let ExprKind::Member(ref member) = expr.kind {
        if let ExprKind::Ident(ref obj) = member.object.kind {
            if obj == class_name {
                return Some(member.property.as_str());
            }
        }
    }
    None
}

/// Compute which computed property names in a class need temp captures because
/// they reference the class (or its static members) before the class is fully
/// constructed. Returns prop keys (e.g. "A.p1") in first-appearance order.
/// Only applies in legacy mode (!use_define).
pub(crate) fn class_computed_name_prop_keys(
    class: &ClassDecl,
    class_name: &str,
    use_define: bool,
) -> Vec<String> {
    if use_define || class_name.is_empty() {
        return Vec::new();
    }
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for member in &class.members {
        let computed_expr = match &member.kind {
            ClassMemberKind::Property(prop) => {
                if let PropName::Computed(expr, _) = &prop.name {
                    Some(expr)
                } else {
                    None
                }
            }
            ClassMemberKind::Method(m) => {
                if let PropName::Computed(expr, _) = &m.name {
                    Some(expr)
                } else {
                    None
                }
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                if let PropName::Computed(expr, _) = &acc.name {
                    Some(expr)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(expr) = computed_expr {
            if let Some(prop) = expr_is_class_member_access(expr, class_name) {
                let prop_key = format!("{}.{}", class_name, prop);
                if seen.insert(prop_key.clone()) {
                    result.push(prop_key);
                }
            }
        }
    }
    result
}

/// Check if a class has static members that need the `var _a; ... _a = C;`
/// class alias wrapper when static initialization is moved out of the class body.
pub(crate) fn class_needs_static_alias(class: &ClassDecl, downlevel_static_blocks: bool) -> bool {
    class.members.iter().any(|m| match &m.kind {
        ClassMemberKind::Property(prop) => {
            prop.modifiers & MOD_STATIC != 0
                && prop.modifiers & MOD_DECLARE == 0
                && prop.modifiers & MOD_ABSTRACT == 0
                && prop.initializer.as_ref().is_some_and(|init| {
                    expr_has_async(init)
                        || expr_has_this(init)
                        || (downlevel_static_blocks && expr_has_super(init))
                })
        }
        ClassMemberKind::StaticBlock(stmts) => {
            downlevel_static_blocks && stmts.iter().any(|s| stmt_has_this(s) || stmt_has_super(s))
        }
        _ => false,
    })
}

fn expr_is_named_class_private_receiver(expr: &Expr, class_name: &str) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) => name == class_name,
        ExprKind::Paren(inner)
        | ExprKind::NonNull(inner)
        | ExprKind::Await(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner)
        | ExprKind::Delete(inner) => expr_is_named_class_private_receiver(inner, class_name),
        ExprKind::TypeAssertion(ta) => expr_is_named_class_private_receiver(&ta.expr, class_name),
        ExprKind::As(a) => expr_is_named_class_private_receiver(&a.expr, class_name),
        ExprKind::Satisfies(s) => expr_is_named_class_private_receiver(&s.expr, class_name),
        ExprKind::New(new_expr) => {
            expr_is_named_class_private_receiver(&new_expr.callee, class_name)
        }
        _ => false,
    }
}

fn class_members_have_private_helper_class_access(class: &ClassDecl, class_name: &str) -> bool {
    class.members.iter().any(|member| match &member.kind {
        ClassMemberKind::Property(prop) => prop
            .initializer
            .as_ref()
            .is_some_and(|init| expr_has_private_helper_class_access(init, class_name)),
        ClassMemberKind::Method(method) => method
            .body
            .as_ref()
            .is_some_and(|body| stmts_have_private_helper_class_access(body, class_name)),
        ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => acc
            .body
            .as_ref()
            .is_some_and(|body| stmts_have_private_helper_class_access(body, class_name)),
        ClassMemberKind::StaticBlock(stmts) => {
            stmts_have_private_helper_class_access(stmts, class_name)
        }
        _ => false,
    })
}

fn stmt_has_private_helper_class_access(stmt: &Stmt, class_name: &str) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) => expr_has_private_helper_class_access(expr, class_name),
        StmtKind::Return(Some(expr)) | StmtKind::Throw(expr) => {
            expr_has_private_helper_class_access(expr, class_name)
        }
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
            decl.init
                .as_ref()
                .is_some_and(|expr| expr_has_private_helper_class_access(expr, class_name))
        }),
        StmtKind::If(if_stmt) => {
            expr_has_private_helper_class_access(&if_stmt.test, class_name)
                || stmt_has_private_helper_class_access(&if_stmt.consequent, class_name)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|alt| stmt_has_private_helper_class_access(alt, class_name))
        }
        StmtKind::Block(stmts) => stmts_have_private_helper_class_access(stmts, class_name),
        StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|init| match init {
                ForInit::Expr(expr) => expr_has_private_helper_class_access(expr, class_name),
                ForInit::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
                    decl.init
                        .as_ref()
                        .is_some_and(|expr| expr_has_private_helper_class_access(expr, class_name))
                }),
            }) || f
                .test
                .as_ref()
                .is_some_and(|expr| expr_has_private_helper_class_access(expr, class_name))
                || f.update
                    .as_ref()
                    .is_some_and(|expr| expr_has_private_helper_class_access(expr, class_name))
                || stmt_has_private_helper_class_access(&f.body, class_name)
        }
        StmtKind::ForIn(fi) => {
            expr_has_private_helper_class_access(&fi.right, class_name)
                || stmt_has_private_helper_class_access(&fi.body, class_name)
        }
        StmtKind::ForOf(fo) => {
            expr_has_private_helper_class_access(&fo.right, class_name)
                || stmt_has_private_helper_class_access(&fo.body, class_name)
        }
        StmtKind::While(w) => {
            expr_has_private_helper_class_access(&w.test, class_name)
                || stmt_has_private_helper_class_access(&w.body, class_name)
        }
        StmtKind::DoWhile(dw) => {
            expr_has_private_helper_class_access(&dw.test, class_name)
                || stmt_has_private_helper_class_access(&dw.body, class_name)
        }
        StmtKind::Switch(sw) => {
            expr_has_private_helper_class_access(&sw.discriminant, class_name)
                || sw.cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_has_private_helper_class_access(expr, class_name))
                        || stmts_have_private_helper_class_access(&case.consequent, class_name)
                })
        }
        StmtKind::Try(t) => {
            stmts_have_private_helper_class_access(&t.block, class_name)
                || t.handler.as_ref().is_some_and(|handler| {
                    stmts_have_private_helper_class_access(&handler.body, class_name)
                })
                || t.finalizer.as_ref().is_some_and(|finalizer| {
                    stmts_have_private_helper_class_access(finalizer, class_name)
                })
        }
        StmtKind::ClassDecl(class_decl) => {
            class_members_have_private_helper_class_access(class_decl, class_name)
        }
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                stmt_has_private_helper_class_access(inner, class_name)
            }
            _ => false,
        },
        StmtKind::Labeled(labeled) => {
            stmt_has_private_helper_class_access(&labeled.body, class_name)
        }
        _ => false,
    }
}

fn stmts_have_private_helper_class_access(stmts: &[Stmt], class_name: &str) -> bool {
    stmts
        .iter()
        .any(|stmt| stmt_has_private_helper_class_access(stmt, class_name))
}

fn expr_has_private_helper_class_access(expr: &Expr, class_name: &str) -> bool {
    match &expr.kind {
        ExprKind::Member(member) => {
            (member.property.starts_with('#')
                && expr_is_named_class_private_receiver(&member.object, class_name))
                || expr_has_private_helper_class_access(&member.object, class_name)
        }
        ExprKind::ElemAccess(ea) => {
            expr_has_private_helper_class_access(&ea.object, class_name)
                || expr_has_private_helper_class_access(&ea.index, class_name)
        }
        ExprKind::Assign(assign) => {
            expr_has_private_helper_class_access(&assign.left, class_name)
                || expr_has_private_helper_class_access(&assign.right, class_name)
        }
        ExprKind::Binary(binary) => {
            expr_has_private_helper_class_access(&binary.left, class_name)
                || expr_has_private_helper_class_access(&binary.right, class_name)
        }
        ExprKind::Unary(unary) => expr_has_private_helper_class_access(&unary.argument, class_name),
        ExprKind::Update(update) => {
            expr_has_private_helper_class_access(&update.argument, class_name)
        }
        ExprKind::Paren(inner)
        | ExprKind::NonNull(inner)
        | ExprKind::Await(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner)
        | ExprKind::Delete(inner) => expr_has_private_helper_class_access(inner, class_name),
        ExprKind::TypeAssertion(ta) => expr_has_private_helper_class_access(&ta.expr, class_name),
        ExprKind::As(a) => expr_has_private_helper_class_access(&a.expr, class_name),
        ExprKind::Satisfies(s) => expr_has_private_helper_class_access(&s.expr, class_name),
        ExprKind::Yield(_, Some(expr)) => expr_has_private_helper_class_access(expr, class_name),
        ExprKind::Call(call) => {
            expr_has_private_helper_class_access(&call.callee, class_name)
                || call
                    .args
                    .iter()
                    .any(|arg| expr_has_private_helper_class_access(arg, class_name))
        }
        ExprKind::New(new_expr) => {
            expr_has_private_helper_class_access(&new_expr.callee, class_name)
                || new_expr.args.as_ref().is_some_and(|args| {
                    args.iter()
                        .any(|arg| expr_has_private_helper_class_access(arg, class_name))
                })
        }
        ExprKind::TaggedTemplate(tt) => {
            expr_has_private_helper_class_access(&tt.tag, class_name)
                || tt
                    .quasi
                    .exprs
                    .iter()
                    .any(|expr| expr_has_private_helper_class_access(expr, class_name))
        }
        ExprKind::Cond(cond) => {
            expr_has_private_helper_class_access(&cond.test, class_name)
                || expr_has_private_helper_class_access(&cond.consequent, class_name)
                || expr_has_private_helper_class_access(&cond.alternate, class_name)
        }
        ExprKind::Comma(exprs) => exprs
            .iter()
            .any(|expr| expr_has_private_helper_class_access(expr, class_name)),
        ExprKind::ArrayLit(elems) => elems.iter().any(|elem| {
            elem.as_ref()
                .is_some_and(|expr| expr_has_private_helper_class_access(expr, class_name))
        }),
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(prop) => {
                expr_has_private_helper_class_access(&prop.value, class_name)
            }
            ObjLitProp::Spread(expr, _) => expr_has_private_helper_class_access(expr, class_name),
            ObjLitProp::Method(method) => {
                stmts_have_private_helper_class_access(&method.body, class_name)
            }
            ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                stmts_have_private_helper_class_access(&acc.body, class_name)
            }
            _ => false,
        }),
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(expr) => expr_has_private_helper_class_access(expr, class_name),
            ArrowBody::Block(stmts) => stmts_have_private_helper_class_access(stmts, class_name),
        },
        ExprKind::FnExpr(fn_decl) => fn_decl
            .body
            .as_ref()
            .is_some_and(|body| stmts_have_private_helper_class_access(body, class_name)),
        ExprKind::ClassExpr(class_decl) => {
            class_members_have_private_helper_class_access(class_decl, class_name)
        }
        _ => false,
    }
}

pub(crate) fn class_needs_private_helper_alias(class: &ClassDecl, class_name: &str) -> bool {
    if class_name.is_empty() {
        return false;
    }

    class.members.iter().any(|member| match &member.kind {
        ClassMemberKind::Method(method) => {
            method.modifiers & MOD_STATIC == 0
                && matches!(method.name, PropName::Private(_, _))
                && method
                    .body
                    .as_ref()
                    .is_some_and(|body| stmts_have_private_helper_class_access(body, class_name))
        }
        ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
            acc.modifiers & MOD_STATIC == 0
                && matches!(acc.name, PropName::Private(_, _))
                && acc
                    .body
                    .as_ref()
                    .is_some_and(|body| stmts_have_private_helper_class_access(body, class_name))
        }
        _ => false,
    })
}

/// Check whether downleveled static initializers need a base-class temp alias
/// to rewrite `super.x` to `Reflect.get(_base, "x", _class)`.
pub(crate) fn class_needs_static_super_base_alias(
    class: &ClassDecl,
    downlevel_static_blocks: bool,
) -> bool {
    if !downlevel_static_blocks {
        return false;
    }
    class.members.iter().any(|m| match &m.kind {
        ClassMemberKind::Property(prop) => {
            prop.modifiers & MOD_STATIC != 0
                && prop.modifiers & MOD_DECLARE == 0
                && prop.modifiers & MOD_ABSTRACT == 0
                && prop
                    .initializer
                    .as_ref()
                    .is_some_and(|init| expr_has_super(init))
        }
        ClassMemberKind::StaticBlock(stmts) => stmts.iter().any(stmt_has_super),
        _ => false,
    })
}

/// Check if a class expression has any static property initializers,
/// which requires the comma-operator IIFE pattern in legacy mode.
pub(crate) fn class_has_static_initializers(class: &ClassDecl) -> bool {
    class.members.iter().any(|m| match &m.kind {
        ClassMemberKind::Property(ref prop) => {
            prop.modifiers & MOD_STATIC != 0
                && prop.modifiers & MOD_DECLARE == 0
                && prop.modifiers & MOD_ABSTRACT == 0
                && prop.initializer.is_some()
        }
        ClassMemberKind::StaticBlock(_) => true,
        _ => false,
    })
}

/// Check if a class has any member-level decorators (property, method, or accessor decorators).
/// Used for propagating `class_expr_binding_name` so that the narrow standard-decorator
/// IIFE path can trigger.
pub(crate) fn class_has_member_decorators(class: &ClassDecl) -> bool {
    class.members.iter().any(|m| match &m.kind {
        ClassMemberKind::Property(ref prop) => !prop.decorators.is_empty(),
        ClassMemberKind::Method(ref method) => !method.decorators.is_empty(),
        ClassMemberKind::GetAccessor(ref acc) | ClassMemberKind::SetAccessor(ref acc) => {
            !acc.decorators.is_empty()
        }
        _ => false,
    })
}

/// Check if a class has any static private members (fields, methods, or accessors).
/// Used for `__setFunctionName` detection: class expressions with static private
/// members need the helper even if there's no explicit field initializer.
pub(crate) fn class_has_static_private_members(class: &ClassDecl) -> bool {
    class.members.iter().any(|m| match &m.kind {
        ClassMemberKind::Property(ref prop) => {
            matches!(&prop.name, PropName::Private(_, _))
                && prop.modifiers & MOD_STATIC != 0
                && prop.modifiers & MOD_DECLARE == 0
                && prop.modifiers & MOD_ABSTRACT == 0
        }
        _ => false,
    })
}

#[allow(dead_code)]
pub(crate) fn expr_has_class_expr_with_static_inits(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ClassExpr(cd) => class_has_static_initializers(cd),
        ExprKind::Paren(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Await(inner)
        | ExprKind::NonNull(inner)
        | ExprKind::Delete(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner) => expr_has_class_expr_with_static_inits(inner),
        ExprKind::Unary(u) => expr_has_class_expr_with_static_inits(&u.argument),
        ExprKind::Update(u) => expr_has_class_expr_with_static_inits(&u.argument),
        ExprKind::As(a) => expr_has_class_expr_with_static_inits(&a.expr),
        ExprKind::Satisfies(s) => expr_has_class_expr_with_static_inits(&s.expr),
        ExprKind::TypeAssertion(ta) => expr_has_class_expr_with_static_inits(&ta.expr),
        ExprKind::Call(call) => {
            expr_has_class_expr_with_static_inits(&call.callee)
                || call
                    .args
                    .iter()
                    .any(|e| expr_has_class_expr_with_static_inits(e))
        }
        ExprKind::New(new_expr) => {
            expr_has_class_expr_with_static_inits(&new_expr.callee)
                || new_expr.args.as_ref().is_some_and(|args| {
                    args.iter()
                        .any(|e| expr_has_class_expr_with_static_inits(e))
                })
        }
        ExprKind::Assign(a) => {
            expr_has_class_expr_with_static_inits(&a.left)
                || expr_has_class_expr_with_static_inits(&a.right)
        }
        ExprKind::Binary(b) => {
            expr_has_class_expr_with_static_inits(&b.left)
                || expr_has_class_expr_with_static_inits(&b.right)
        }
        ExprKind::Cond(c) => {
            expr_has_class_expr_with_static_inits(&c.test)
                || expr_has_class_expr_with_static_inits(&c.consequent)
                || expr_has_class_expr_with_static_inits(&c.alternate)
        }
        ExprKind::Comma(exprs) => exprs
            .iter()
            .any(|e| expr_has_class_expr_with_static_inits(e)),
        ExprKind::ArrayLit(elems) => elems.iter().any(|e| {
            e.as_ref()
                .is_some_and(|e| expr_has_class_expr_with_static_inits(e))
        }),
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(p) => expr_has_class_expr_with_static_inits(&p.value),
            ObjLitProp::ShorthandDefault(_, init, _) => expr_has_class_expr_with_static_inits(init),
            ObjLitProp::Spread(expr, _) => expr_has_class_expr_with_static_inits(expr),
            ObjLitProp::Method(method) => stmts_have_class_expr_iife(&method.body),
            ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => stmts_have_class_expr_iife(&acc.body),
            ObjLitProp::Shorthand(_, _) => false,
        }),
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(expr) => expr_has_class_expr_with_static_inits(expr),
            ArrowBody::Block(stmts) => stmts_have_class_expr_iife(stmts),
        },
        ExprKind::FnExpr(func) => func
            .body
            .as_ref()
            .is_some_and(|body| stmts_have_class_expr_iife(body)),
        ExprKind::Member(mem) => expr_has_class_expr_with_static_inits(&mem.object),
        ExprKind::ElemAccess(elem) => {
            expr_has_class_expr_with_static_inits(&elem.object)
                || expr_has_class_expr_with_static_inits(&elem.index)
        }
        ExprKind::Template(tpl) => tpl
            .exprs
            .iter()
            .any(|expr| expr_has_class_expr_with_static_inits(expr)),
        ExprKind::TaggedTemplate(tagged) => {
            expr_has_class_expr_with_static_inits(&tagged.tag)
                || tagged
                    .quasi
                    .exprs
                    .iter()
                    .any(|expr| expr_has_class_expr_with_static_inits(expr))
        }
        _ => false,
    }
}

#[allow(dead_code)]
pub(crate) fn stmts_have_class_expr_iife(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| stmt_has_class_expr_in_body(stmt))
}

#[allow(dead_code)]
pub(crate) fn stmt_has_class_expr_in_body(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Var(v) => v.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_class_expr_with_static_inits(e))
        }),
        StmtKind::Expr(e) => expr_has_class_expr_with_static_inits(e),
        StmtKind::Return(e) => e
            .as_ref()
            .is_some_and(|e| expr_has_class_expr_with_static_inits(e)),
        StmtKind::For(f) => {
            stmt_has_class_expr_in_body(&f.body)
                || f.init.as_ref().is_some_and(|init| match init {
                    ForInit::Expr(e) => expr_has_class_expr_with_static_inits(e),
                    ForInit::Var(v) => v.declarations.iter().any(|d| {
                        d.init
                            .as_ref()
                            .is_some_and(|e| expr_has_class_expr_with_static_inits(e))
                    }),
                })
        }
        StmtKind::ForIn(f) => stmt_has_class_expr_in_body(&f.body),
        StmtKind::ForOf(f) => stmt_has_class_expr_in_body(&f.body),
        StmtKind::While(w) => stmt_has_class_expr_in_body(&w.body),
        StmtKind::If(i) => {
            stmt_has_class_expr_in_body(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|a| stmt_has_class_expr_in_body(a))
        }
        StmtKind::Block(stmts) => stmts_have_class_expr_iife(stmts),
        StmtKind::Export(export) => match &export.kind {
            ExportDeclKind::Decl(inner) => stmt_has_class_expr_in_body(inner),
            ExportDeclKind::Default(expr) => expr_has_class_expr_with_static_inits(expr),
            ExportDeclKind::DefaultDecl(inner) => stmt_has_class_expr_in_body(inner),
            _ => false,
        },
        _ => false,
    }
}

/// Check if any statement assigns an anonymous class expression with static
/// initializers to a named binding. This means __setFunctionName is needed.
pub(crate) fn stmts_need_set_function_name(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| stmt_needs_set_function_name(stmt))
}

fn class_decl_needs_set_function_name(class_decl: &ClassDecl) -> bool {
    class_decl.members.iter().any(|member| match &member.kind {
        ClassMemberKind::Property(prop) => prop.initializer.as_ref().is_some_and(|expr| {
            matches!(&expr.kind, ExprKind::ClassExpr(cd)
                if cd.name.is_none()
                    && (class_has_static_initializers(cd)
                        || !cd.decorators.is_empty()
                        || class_has_member_decorators(cd)))
        }),
        ClassMemberKind::Method(method) => {
            params_need_set_function_name(&method.params)
                || method
                    .body
                    .as_ref()
                    .is_some_and(|body| stmts_need_set_function_name(body))
        }
        ClassMemberKind::Constructor(ctor) => {
            params_need_set_function_name(&ctor.params)
                || ctor
                    .body
                    .as_ref()
                    .is_some_and(|body| stmts_need_set_function_name(body))
        }
        ClassMemberKind::GetAccessor(acc) => acc
            .body
            .as_ref()
            .is_some_and(|body| stmts_need_set_function_name(body)),
        ClassMemberKind::SetAccessor(acc) => {
            params_need_set_function_name(&acc.params)
                || acc
                    .body
                    .as_ref()
                    .is_some_and(|body| stmts_need_set_function_name(body))
        }
        ClassMemberKind::StaticBlock(stmts) => stmts_need_set_function_name(stmts),
        _ => false,
    })
}

fn stmt_needs_set_function_name(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Var(v) => v.declarations.iter().any(|d| {
            if let PatKind::Ident(_) = &d.name.kind {
                d.init.as_ref().is_some_and(|e| {
                    matches!(&e.kind, ExprKind::ClassExpr(cd)
                        if cd.name.is_none()
                            && (matches!(v.kind, VarKind::Using | VarKind::AwaitUsing)
                                || class_has_static_initializers(cd)
                                || class_has_static_private_members(cd)))
                })
            } else {
                false
            }
        }),
        StmtKind::ClassDecl(class_decl) => class_decl_needs_set_function_name(class_decl),
        StmtKind::Export(export) => match &export.kind {
            ExportDeclKind::Decl(inner) => stmt_needs_set_function_name(inner),
            ExportDeclKind::DefaultDecl(inner) => {
                // `export default @dec class { static y = 1; }` needs __setFunctionName
                if let StmtKind::ClassDecl(cd) = &inner.kind {
                    cd.name.is_none()
                        && !cd.decorators.is_empty()
                        && (class_has_static_initializers(cd)
                            || class_has_static_private_members(cd))
                } else {
                    false
                }
            }
            _ => false,
        },
        StmtKind::Block(stmts) => stmts_need_set_function_name(stmts),
        StmtKind::FnDecl(f) => {
            params_need_set_function_name(&f.params)
                || f.body
                    .as_ref()
                    .is_some_and(|body| stmts_need_set_function_name(body))
        }
        StmtKind::If(i) => {
            stmt_needs_set_function_name(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|alt| stmt_needs_set_function_name(alt))
        }
        StmtKind::For(f) => stmt_needs_set_function_name(&f.body),
        StmtKind::ForIn(f) => stmt_needs_set_function_name(&f.body),
        StmtKind::ForOf(f) => stmt_needs_set_function_name(&f.body),
        StmtKind::While(w) => stmt_needs_set_function_name(&w.body),
        StmtKind::DoWhile(d) => stmt_needs_set_function_name(&d.body),
        StmtKind::Switch(s) => s
            .cases
            .iter()
            .any(|c| stmts_need_set_function_name(&c.consequent)),
        StmtKind::Try(t) => {
            stmts_need_set_function_name(&t.block)
                || t.handler
                    .as_ref()
                    .is_some_and(|c| stmts_need_set_function_name(&c.body))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| stmts_need_set_function_name(f))
        }
        StmtKind::Labeled(l) => stmt_needs_set_function_name(&l.body),
        StmtKind::With(w) => stmt_needs_set_function_name(&w.body),
        StmtKind::ModuleDecl(m) => {
            if let Some(ModuleBody::Block(stmts)) = &m.body {
                stmts_need_set_function_name(stmts)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn params_need_set_function_name(params: &[Param]) -> bool {
    params.iter().any(|p| {
        matches!(&p.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
            && p.initializer.as_ref().is_some_and(|e| {
                matches!(
                    &e.kind,
                    ExprKind::ClassExpr(cd) if cd.name.is_none() && class_has_static_initializers(cd)
                )
            })
    })
}

/// Check if a class declaration has any private fields.
#[allow(dead_code)] // used by private field downlevel
pub(crate) fn class_has_private_fields(class: &ClassDecl) -> bool {
    class.members.iter().any(|m| {
        if let ClassMemberKind::Property(ref prop) = m.kind {
            matches!(prop.name, PropName::Private(_, _))
        } else {
            false
        }
    })
}

#[allow(dead_code)]
pub(crate) fn class_uses_private_field_helpers(class: &ClassDecl) -> bool {
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Method(m) => {
                if let Some(ref body) = m.body {
                    if stmts_have_private_member_access(body) {
                        return true;
                    }
                }
            }
            ClassMemberKind::Constructor(c) => {
                if let Some(ref body) = c.body {
                    if stmts_have_private_member_access(body) {
                        return true;
                    }
                }
            }
            ClassMemberKind::Property(p) => {
                if let Some(ref init) = p.initializer {
                    if expr_has_private_member_access(init) {
                        return true;
                    }
                }
            }
            ClassMemberKind::StaticBlock(stmts) => {
                if stmts_have_private_member_access(stmts) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[allow(dead_code)]
fn stmts_have_private_member_access(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        if stmt_has_private_member_access(stmt) {
            return true;
        }
    }
    false
}

#[allow(dead_code)]
fn stmt_has_private_member_access(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) => expr_has_private_member_access(e),
        StmtKind::Return(Some(e)) | StmtKind::Throw(e) => expr_has_private_member_access(e),
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_private_member_access(e))
        }),
        StmtKind::If(if_stmt) => {
            expr_has_private_member_access(&if_stmt.test)
                || stmt_has_private_member_access(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|e| stmt_has_private_member_access(e))
        }
        StmtKind::Block(stmts) => stmts_have_private_member_access(stmts),
        StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|e| match e {
                ForInit::Expr(e) => expr_has_private_member_access(e),
                ForInit::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
                    d.init
                        .as_ref()
                        .is_some_and(|e| expr_has_private_member_access(e))
                }),
            }) || f
                .test
                .as_ref()
                .is_some_and(|e| expr_has_private_member_access(e))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_has_private_member_access(e))
                || stmt_has_private_member_access(&f.body)
        }
        StmtKind::While(w) => {
            expr_has_private_member_access(&w.test) || stmt_has_private_member_access(&w.body)
        }
        StmtKind::DoWhile(dw) => {
            expr_has_private_member_access(&dw.test) || stmt_has_private_member_access(&dw.body)
        }
        StmtKind::Switch(sw) => {
            expr_has_private_member_access(&sw.discriminant)
                || sw.cases.iter().any(|c| {
                    c.test
                        .as_ref()
                        .is_some_and(|e| expr_has_private_member_access(e))
                        || stmts_have_private_member_access(&c.consequent)
                })
        }
        StmtKind::Try(t) => {
            stmts_have_private_member_access(&t.block)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| stmts_have_private_member_access(&h.body))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| stmts_have_private_member_access(f))
        }
        _ => false,
    }
}

#[allow(dead_code)]
fn expr_has_private_member_access(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Member(m) => {
            m.property.starts_with('#') || expr_has_private_member_access(&m.object)
        }
        ExprKind::ElemAccess(ea) => {
            expr_has_private_member_access(&ea.object) || expr_has_private_member_access(&ea.index)
        }
        ExprKind::Assign(a) => {
            expr_has_private_member_access(&a.left) || expr_has_private_member_access(&a.right)
        }
        ExprKind::Binary(b) => {
            expr_has_private_member_access(&b.left) || expr_has_private_member_access(&b.right)
        }
        ExprKind::Unary(u) => expr_has_private_member_access(&u.argument),
        ExprKind::Update(u) => expr_has_private_member_access(&u.argument),
        ExprKind::Paren(e)
        | ExprKind::NonNull(e)
        | ExprKind::Await(e)
        | ExprKind::Spread(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e)
        | ExprKind::Delete(e) => expr_has_private_member_access(e),
        ExprKind::TypeAssertion(ta) => expr_has_private_member_access(&ta.expr),
        ExprKind::As(a) => expr_has_private_member_access(&a.expr),
        ExprKind::Satisfies(s) => expr_has_private_member_access(&s.expr),
        ExprKind::Yield(_, Some(e)) => expr_has_private_member_access(e),
        ExprKind::Call(call) => {
            expr_has_private_member_access(&call.callee)
                || call.args.iter().any(|a| expr_has_private_member_access(a))
        }
        ExprKind::New(new) => {
            expr_has_private_member_access(&new.callee)
                || new
                    .args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|a| expr_has_private_member_access(a)))
        }
        ExprKind::TaggedTemplate(tt) => {
            expr_has_private_member_access(&tt.tag)
                || tt
                    .quasi
                    .exprs
                    .iter()
                    .any(|e| expr_has_private_member_access(e))
        }
        ExprKind::Cond(c) => {
            expr_has_private_member_access(&c.test)
                || expr_has_private_member_access(&c.consequent)
                || expr_has_private_member_access(&c.alternate)
        }
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_private_member_access(e)),
        ExprKind::ArrayLit(elems) => elems.iter().any(|e| {
            e.as_ref()
                .is_some_and(|e| expr_has_private_member_access(e))
        }),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(prop) => expr_has_private_member_access(&prop.value),
            ObjLitProp::Spread(e, _) => expr_has_private_member_access(e),
            ObjLitProp::Method(m) => stmts_have_private_member_access(&m.body),
            ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                stmts_have_private_member_access(&acc.body)
            }
            _ => false,
        }),
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(e) => expr_has_private_member_access(e),
            ArrowBody::Block(stmts) => stmts_have_private_member_access(stmts),
        },
        ExprKind::FnExpr(fn_decl) => fn_decl
            .body
            .as_ref()
            .is_some_and(|body| stmts_have_private_member_access(body)),
        _ => false,
    }
}

/// Scan statements for private member access, distinguishing reads from writes.
/// Returns (needs_get, needs_set).
fn stmts_private_member_access_kind_declared(
    stmts: &[Stmt],
    declared_private_names: &std::collections::HashSet<String>,
) -> (bool, bool) {
    let mut needs_get = false;
    let mut needs_set = false;
    for stmt in stmts {
        let (g, s) = stmt_private_member_access_kind_declared(stmt, declared_private_names);
        needs_get |= g;
        needs_set |= s;
        if needs_get && needs_set {
            break;
        }
    }
    (needs_get, needs_set)
}

fn stmt_private_member_access_kind_declared(
    stmt: &Stmt,
    declared_private_names: &std::collections::HashSet<String>,
) -> (bool, bool) {
    match &stmt.kind {
        StmtKind::Expr(e) => expr_private_member_access_kind_declared(e, declared_private_names),
        StmtKind::Return(Some(e)) | StmtKind::Throw(e) => {
            expr_private_member_access_kind_declared(e, declared_private_names)
        }
        StmtKind::Var(var_stmt) => {
            let mut g = false;
            let mut s = false;
            for d in &var_stmt.declarations {
                if let Some(ref init) = d.init {
                    let (gg, ss) =
                        expr_private_member_access_kind_declared(init, declared_private_names);
                    g |= gg;
                    s |= ss;
                }
            }
            (g, s)
        }
        StmtKind::If(if_stmt) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&if_stmt.test, declared_private_names);
            let (g2, s2) = stmt_private_member_access_kind_declared(
                &if_stmt.consequent,
                declared_private_names,
            );
            g |= g2;
            s |= s2;
            if let Some(ref alt) = if_stmt.alternate {
                let (g3, s3) =
                    stmt_private_member_access_kind_declared(alt, declared_private_names);
                g |= g3;
                s |= s3;
            }
            (g, s)
        }
        StmtKind::Block(stmts) => {
            stmts_private_member_access_kind_declared(stmts, declared_private_names)
        }
        StmtKind::For(f) => {
            let mut g = false;
            let mut s = false;
            if let Some(ref init) = f.init {
                let (gg, ss) = match init {
                    ForInit::Expr(e) => {
                        expr_private_member_access_kind_declared(e, declared_private_names)
                    }
                    ForInit::Var(var_stmt) => {
                        let mut g = false;
                        let mut s = false;
                        for d in &var_stmt.declarations {
                            if let Some(ref e) = d.init {
                                let (gg, ss) = expr_private_member_access_kind_declared(
                                    e,
                                    declared_private_names,
                                );
                                g |= gg;
                                s |= ss;
                            }
                        }
                        (g, s)
                    }
                };
                g |= gg;
                s |= ss;
            }
            if let Some(ref e) = f.test {
                let (gg, ss) = expr_private_member_access_kind_declared(e, declared_private_names);
                g |= gg;
                s |= ss;
            }
            if let Some(ref e) = f.update {
                let (gg, ss) = expr_private_member_access_kind_declared(e, declared_private_names);
                g |= gg;
                s |= ss;
            }
            let (gg, ss) =
                stmt_private_member_access_kind_declared(&f.body, declared_private_names);
            g |= gg;
            s |= ss;
            (g, s)
        }
        StmtKind::While(w) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&w.test, declared_private_names);
            let (g2, s2) =
                stmt_private_member_access_kind_declared(&w.body, declared_private_names);
            g |= g2;
            s |= s2;
            (g, s)
        }
        StmtKind::DoWhile(dw) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&dw.test, declared_private_names);
            let (g2, s2) =
                stmt_private_member_access_kind_declared(&dw.body, declared_private_names);
            g |= g2;
            s |= s2;
            (g, s)
        }
        StmtKind::Switch(sw) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&sw.discriminant, declared_private_names);
            for case in &sw.cases {
                if let Some(ref test) = case.test {
                    let (gg, ss) =
                        expr_private_member_access_kind_declared(test, declared_private_names);
                    g |= gg;
                    s |= ss;
                }
                let (gg, ss) = stmts_private_member_access_kind_declared(
                    &case.consequent,
                    declared_private_names,
                );
                g |= gg;
                s |= ss;
            }
            (g, s)
        }
        StmtKind::Try(t) => {
            let (mut g, mut s) =
                stmts_private_member_access_kind_declared(&t.block, declared_private_names);
            if let Some(ref h) = t.handler {
                let (gg, ss) =
                    stmts_private_member_access_kind_declared(&h.body, declared_private_names);
                g |= gg;
                s |= ss;
            }
            if let Some(ref f) = t.finalizer {
                let (gg, ss) = stmts_private_member_access_kind_declared(f, declared_private_names);
                g |= gg;
                s |= ss;
            }
            (g, s)
        }
        StmtKind::ClassDecl(c) => {
            let mut g = false;
            let mut s = false;
            for member in &c.members {
                let (gg, ss) = match &member.kind {
                    ClassMemberKind::Method(m) => {
                        if let Some(ref body) = m.body {
                            stmts_private_member_access_kind_declared(body, declared_private_names)
                        } else {
                            (false, false)
                        }
                    }
                    ClassMemberKind::Constructor(ctor) => {
                        if let Some(ref body) = ctor.body {
                            stmts_private_member_access_kind_declared(body, declared_private_names)
                        } else {
                            (false, false)
                        }
                    }
                    ClassMemberKind::Property(p) => {
                        if let Some(ref init) = p.initializer {
                            expr_private_member_access_kind_declared(init, declared_private_names)
                        } else {
                            (false, false)
                        }
                    }
                    ClassMemberKind::StaticBlock(stmts) => {
                        stmts_private_member_access_kind_declared(stmts, declared_private_names)
                    }
                    ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                        if let Some(ref body) = acc.body {
                            stmts_private_member_access_kind_declared(body, declared_private_names)
                        } else {
                            (false, false)
                        }
                    }
                    _ => (false, false),
                };
                g |= gg;
                s |= ss;
                if g && s {
                    break;
                }
            }
            (g, s)
        }
        _ => (false, false),
    }
}

fn expr_private_member_access_kind_declared(
    expr: &Expr,
    declared_private_names: &std::collections::HashSet<String>,
) -> (bool, bool) {
    match &expr.kind {
        ExprKind::Member(m) => {
            let (mut g, s) =
                expr_private_member_access_kind_declared(&m.object, declared_private_names);
            if member_matches_declared_private(m, declared_private_names) {
                g = true;
            }
            (g, s)
        }
        ExprKind::ElemAccess(ea) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&ea.object, declared_private_names);
            let (g2, s2) =
                expr_private_member_access_kind_declared(&ea.index, declared_private_names);
            g |= g2;
            s |= s2;
            (g, s)
        }
        ExprKind::Assign(a) => {
            let lhs_is_private =
                lhs_pattern_has_declared_private_write_target(&a.left, declared_private_names);
            let (mut g, mut s) = if lhs_is_private {
                if a.op == AssignOp::Assign {
                    (false, true)
                } else {
                    (true, true)
                }
            } else {
                expr_private_member_access_kind_declared(&a.left, declared_private_names)
            };
            let (g2, s2) =
                expr_private_member_access_kind_declared(&a.right, declared_private_names);
            g |= g2;
            s |= s2;
            (g, s)
        }
        ExprKind::Update(u) => {
            let arg_is_private = matches!(
                &u.argument.kind,
                ExprKind::Member(m) if member_matches_declared_private(m, declared_private_names)
            );
            if arg_is_private {
                (true, true)
            } else {
                expr_private_member_access_kind_declared(&u.argument, declared_private_names)
            }
        }
        ExprKind::Binary(b) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&b.left, declared_private_names);
            let (g2, s2) =
                expr_private_member_access_kind_declared(&b.right, declared_private_names);
            g |= g2;
            s |= s2;
            (g, s)
        }
        ExprKind::Unary(u) => {
            expr_private_member_access_kind_declared(&u.argument, declared_private_names)
        }
        ExprKind::Paren(e)
        | ExprKind::NonNull(e)
        | ExprKind::Await(e)
        | ExprKind::Spread(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e)
        | ExprKind::Delete(e) => {
            expr_private_member_access_kind_declared(e, declared_private_names)
        }
        ExprKind::TypeAssertion(ta) => {
            expr_private_member_access_kind_declared(&ta.expr, declared_private_names)
        }
        ExprKind::As(a) => {
            expr_private_member_access_kind_declared(&a.expr, declared_private_names)
        }
        ExprKind::Satisfies(s) => {
            expr_private_member_access_kind_declared(&s.expr, declared_private_names)
        }
        ExprKind::Yield(_, Some(e)) => {
            expr_private_member_access_kind_declared(e, declared_private_names)
        }
        ExprKind::Call(call) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&call.callee, declared_private_names);
            for arg in &call.args {
                let (g2, s2) =
                    expr_private_member_access_kind_declared(arg, declared_private_names);
                g |= g2;
                s |= s2;
            }
            (g, s)
        }
        ExprKind::New(new) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&new.callee, declared_private_names);
            if let Some(ref args) = new.args {
                for arg in args {
                    let (g2, s2) =
                        expr_private_member_access_kind_declared(arg, declared_private_names);
                    g |= g2;
                    s |= s2;
                }
            }
            (g, s)
        }
        ExprKind::TaggedTemplate(tt) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&tt.tag, declared_private_names);
            for e in &tt.quasi.exprs {
                let (g2, s2) = expr_private_member_access_kind_declared(e, declared_private_names);
                g |= g2;
                s |= s2;
            }
            (g, s)
        }
        ExprKind::Cond(c) => {
            let (mut g, mut s) =
                expr_private_member_access_kind_declared(&c.test, declared_private_names);
            let (g2, s2) =
                expr_private_member_access_kind_declared(&c.consequent, declared_private_names);
            let (g3, s3) =
                expr_private_member_access_kind_declared(&c.alternate, declared_private_names);
            g |= g2 | g3;
            s |= s2 | s3;
            (g, s)
        }
        ExprKind::Comma(exprs) => {
            let mut g = false;
            let mut s = false;
            for e in exprs {
                let (gg, ss) = expr_private_member_access_kind_declared(e, declared_private_names);
                g |= gg;
                s |= ss;
            }
            (g, s)
        }
        ExprKind::ArrayLit(elems) => {
            let mut g = false;
            let mut s = false;
            for e in elems.iter().flatten() {
                let (gg, ss) = expr_private_member_access_kind_declared(e, declared_private_names);
                g |= gg;
                s |= ss;
            }
            (g, s)
        }
        ExprKind::ObjectLit(props) => {
            let mut g = false;
            let mut s = false;
            for p in props {
                let (gg, ss) = match p {
                    ObjLitProp::Property(prop) => expr_private_member_access_kind_declared(
                        &prop.value,
                        declared_private_names,
                    ),
                    ObjLitProp::Spread(e, _) => {
                        expr_private_member_access_kind_declared(e, declared_private_names)
                    }
                    ObjLitProp::Method(m) => {
                        stmts_private_member_access_kind_declared(&m.body, declared_private_names)
                    }
                    ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                        stmts_private_member_access_kind_declared(&acc.body, declared_private_names)
                    }
                    _ => (false, false),
                };
                g |= gg;
                s |= ss;
            }
            (g, s)
        }
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(e) => {
                expr_private_member_access_kind_declared(e, declared_private_names)
            }
            ArrowBody::Block(stmts) => {
                stmts_private_member_access_kind_declared(stmts, declared_private_names)
            }
        },
        ExprKind::FnExpr(fn_decl) => fn_decl.body.as_ref().map_or((false, false), |body| {
            stmts_private_member_access_kind_declared(body, declared_private_names)
        }),
        _ => (false, false),
    }
}

/// Check if a statement contains a class with `#field in obj` expressions.
pub(crate) fn class_needs_private_field_in_stmt(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ClassDecl(c) => class_has_private_in_expr(c),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                class_needs_private_field_in_stmt(inner)
            }
            _ => false,
        },
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            d.init.as_ref().is_some_and(
                |init| matches!(&init.kind, ExprKind::ClassExpr(c) if class_has_private_in_expr(c)),
            )
        }),
        _ => false,
    }
}

fn class_has_private_in_expr(class: &ClassDecl) -> bool {
    for member in &class.members {
        let found = match &member.kind {
            ClassMemberKind::Method(m) => m.body.as_ref().is_some_and(|b| stmts_have_private_in(b)),
            ClassMemberKind::Constructor(c) => {
                c.body.as_ref().is_some_and(|b| stmts_have_private_in(b))
            }
            ClassMemberKind::Property(p) => p
                .initializer
                .as_ref()
                .is_some_and(|e| expr_has_private_in(e)),
            ClassMemberKind::StaticBlock(stmts) => stmts_have_private_in(stmts),
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                acc.body.as_ref().is_some_and(|b| stmts_have_private_in(b))
            }
            _ => false,
        };
        if found {
            return true;
        }
    }
    false
}

fn stmts_have_private_in(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        if stmt_has_private_in(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_private_in(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Return(Some(e)) | StmtKind::Throw(e) => {
            expr_has_private_in(e)
        }
        StmtKind::Var(vs) => vs
            .declarations
            .iter()
            .any(|d| d.init.as_ref().is_some_and(|e| expr_has_private_in(e))),
        StmtKind::If(if_stmt) => {
            expr_has_private_in(&if_stmt.test)
                || stmt_has_private_in(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| stmt_has_private_in(a))
        }
        StmtKind::Block(stmts) => stmts_have_private_in(stmts),
        StmtKind::For(fo) => {
            fo.init.as_ref().is_some_and(|init| match init {
                ForInit::Expr(e) => expr_has_private_in(e),
                ForInit::Var(vs) => vs
                    .declarations
                    .iter()
                    .any(|d| d.init.as_ref().is_some_and(|e| expr_has_private_in(e))),
            }) || fo.test.as_ref().is_some_and(|e| expr_has_private_in(e))
                || fo.update.as_ref().is_some_and(|e| expr_has_private_in(e))
                || stmt_has_private_in(&fo.body)
        }
        StmtKind::ForOf(fo) => expr_has_private_in(&fo.right) || stmt_has_private_in(&fo.body),
        StmtKind::ForIn(fi) => expr_has_private_in(&fi.right) || stmt_has_private_in(&fi.body),
        StmtKind::While(w) => expr_has_private_in(&w.test) || stmt_has_private_in(&w.body),
        StmtKind::DoWhile(dw) => expr_has_private_in(&dw.test) || stmt_has_private_in(&dw.body),
        StmtKind::Switch(sw) => {
            expr_has_private_in(&sw.discriminant)
                || sw.cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(|t| expr_has_private_in(t))
                        || stmts_have_private_in(&c.consequent)
                })
        }
        _ => false,
    }
}

fn expr_has_private_in(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Binary(b) if b.op == BinaryOp::In => {
            if matches!(&b.left.kind, ExprKind::Ident(name) if name.starts_with('#')) {
                return true;
            }
            expr_has_private_in(&b.left) || expr_has_private_in(&b.right)
        }
        ExprKind::Binary(b) => expr_has_private_in(&b.left) || expr_has_private_in(&b.right),
        ExprKind::Assign(a) => expr_has_private_in(&a.left) || expr_has_private_in(&a.right),
        ExprKind::Cond(c) => {
            expr_has_private_in(&c.test)
                || expr_has_private_in(&c.consequent)
                || expr_has_private_in(&c.alternate)
        }
        ExprKind::Call(call) => {
            expr_has_private_in(&call.callee) || call.args.iter().any(|a| expr_has_private_in(a))
        }
        ExprKind::Member(m) => expr_has_private_in(&m.object),
        ExprKind::Paren(e)
        | ExprKind::NonNull(e)
        | ExprKind::Await(e)
        | ExprKind::Spread(e)
        | ExprKind::Unary(UnaryExpr { argument: e, .. })
        | ExprKind::Typeof(e)
        | ExprKind::Void(e)
        | ExprKind::Delete(e) => expr_has_private_in(e),
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_private_in(e)),
        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(e) => expr_has_private_in(e),
            ArrowBody::Block(stmts) => stmts_have_private_in(stmts),
        },
        _ => false,
    }
}

/// Extract the binding variable name from a ForInOfLeft as a string.
/// For destructuring patterns, returns a placeholder (the actual pattern
/// is emitted separately).
pub(crate) fn for_in_of_left_binding_str(left: &ForInOfLeft) -> String {
    match left {
        ForInOfLeft::Var(vs) => {
            if let Some(decl) = vs.declarations.first() {
                pat_to_simple_name(&decl.name).unwrap_or_else(|| "_v".to_string())
            } else {
                "_v".to_string()
            }
        }
        ForInOfLeft::Pat(pat) => pat_to_simple_name(pat).unwrap_or_else(|| "_v".to_string()),
        // Expression targets are assignments, not bindings. Downlevel paths
        // emit the expression directly and must not manufacture a name.
        ForInOfLeft::Expr(_) => "_v".to_string(),
    }
}

/// Check if a ForInOfLeft uses a destructuring pattern.
pub(crate) fn for_in_of_left_is_destructuring(left: &ForInOfLeft) -> bool {
    match left {
        ForInOfLeft::Var(vs) => vs
            .declarations
            .first()
            .is_some_and(|d| !matches!(d.name.kind, PatKind::Ident(_))),
        ForInOfLeft::Pat(pat) => !matches!(pat.kind, PatKind::Ident(_)),
        ForInOfLeft::Expr(_) => false,
    }
}

/// Return the sole declaration when a for-of declaration header is eligible
/// for the narrow ES5 binding expansion. Comment ownership and loop-level
/// native-preservation are emitter concerns.
pub(crate) fn eligible_es5_for_of_declaration(left: &ForInOfLeft) -> Option<&VarDeclarator> {
    let ForInOfLeft::Var(var) = left else {
        return None;
    };
    if matches!(var.kind, VarKind::Using | VarKind::AwaitUsing) || var.declarations.len() != 1 {
        return None;
    }
    let declaration = &var.declarations[0];
    declaration.init.is_none().then_some(declaration)
}

/// Whether an array binding can be expanded into a single ES5 `var`
/// declarator list without owning defaults, rest elements, nested patterns,
/// or recovery placeholders. Comment ownership is checked by the emitter.
pub(crate) fn simple_es5_for_of_array_binding(pat: &Pat) -> bool {
    let PatKind::Array(elements) = &pat.kind else {
        return false;
    };
    elements.iter().flatten().all(|element| {
        matches!(
            element,
            ArrayPatElem::Pat(Pat {
                kind: PatKind::Ident(name),
                ..
            }) if !name.is_empty() && name != "<error>"
        )
    })
}

/// Object counterpart to [`simple_es5_for_of_array_binding`]. Computed keys,
/// defaults, rest properties, and nested targets deliberately remain on the
/// established fail-closed path.
pub(crate) fn simple_es5_for_of_object_binding(pat: &Pat) -> bool {
    let PatKind::Object(properties) = &pat.kind else {
        return false;
    };
    properties.iter().all(|property| match property {
        ObjPatProp::Shorthand(name, _) => !name.is_empty() && name != "<error>",
        ObjPatProp::KeyValue(PropName::Ident(key, _), value) => {
            !key.is_empty()
                && key != "<error>"
                && matches!(&value.kind, PatKind::Ident(name) if !name.is_empty() && name != "<error>")
        }
        _ => false,
    })
}

/// Whether a pattern contains an expression target represented by the parser's
/// recovery marker. For-in/of assignment headers use this to source-copy the
/// target instead of silently dropping member accesses inside a pattern.
pub(crate) fn pattern_contains_error_binding(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(name) => name == "<error>",
        PatKind::Array(elements) => elements.iter().flatten().any(|element| match element {
            ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => pattern_contains_error_binding(pat),
        }),
        PatKind::Object(properties) => properties.iter().any(|property| match property {
            ObjPatProp::KeyValue(_, pat) | ObjPatProp::Rest(pat) => {
                pattern_contains_error_binding(pat)
            }
            ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) => {
                name == "<error>"
            }
        }),
        PatKind::Assign(pat, _) | PatKind::Rest(pat) => pattern_contains_error_binding(pat),
    }
}

/// Extract a simple identifier name from a pattern, or None for destructuring.
pub(crate) fn pat_to_simple_name(pat: &Pat) -> Option<String> {
    match &pat.kind {
        PatKind::Ident(name) => Some(name.to_string()),
        _ => None,
    }
}

// (const enum reference detection is defined above near Transformation detection)

/// For namespace scope: collect import-equals names that are only used in type
/// positions (not referenced in value expressions by sibling statements).
/// These should be elided from the emitted namespace body.
pub fn collect_ns_type_only_import_equals(stmts: &[Stmt]) -> HashSet<String> {
    let mut import_equals_names: HashSet<AstString> = HashSet::new();
    for stmt in stmts {
        if let StmtKind::ImportEquals(ie) = &stmt.kind {
            import_equals_names.insert(AstString::from(ie.name.as_str()));
        }
        if let StmtKind::Export(ed) = &stmt.kind {
            if let ExportDeclKind::Decl(inner) = &ed.kind {
                if let StmtKind::ImportEquals(ie) = &inner.kind {
                    import_equals_names.insert(AstString::from(ie.name.as_str()));
                }
            }
        }
    }
    if import_equals_names.is_empty() {
        return HashSet::new();
    }
    let ie_nf = FilteredNames::new(&import_equals_names);
    let mut value_used: HashSet<AstString> = HashSet::new();
    let mut value_used_non_member: HashSet<AstString> = HashSet::new();
    for stmt in stmts {
        match &stmt.kind {
            // For import-equals, scan the RHS for value refs to other import-equals
            // (e.g. `export import X = R` references `R` in value position)
            StmtKind::ImportEquals(ie) => {
                collect_value_refs_expr(
                    &ie.module_ref,
                    &ie_nf,
                    &mut value_used,
                    &mut value_used_non_member,
                    false,
                );
                continue;
            }
            StmtKind::Export(ed) => {
                if let ExportDeclKind::Decl(inner) = &ed.kind {
                    if let StmtKind::ImportEquals(ie) = &inner.kind {
                        collect_value_refs_expr(
                            &ie.module_ref,
                            &ie_nf,
                            &mut value_used,
                            &mut value_used_non_member,
                            false,
                        );
                        continue;
                    }
                }
                if let ExportDeclKind::Named {
                    specifiers: ref specs,
                    ..
                } = &ed.kind
                {
                    for spec in specs {
                        if !spec.is_type && import_equals_names.contains(spec.local.as_str()) {
                            value_used.insert(AstString::from(spec.local.as_str()));
                        }
                    }
                }
            }
            _ => {}
        }
        collect_value_refs_stmt(stmt, &ie_nf, &mut value_used, &mut value_used_non_member);
    }
    import_equals_names
        .difference(&value_used)
        .map(|s| String::from(s.as_str()))
        .collect()
}

// ---------------------------------------------------------------------------
// Import elision value-reference collection
// ---------------------------------------------------------------------------

/// The import-name set plus a 256-bit first-byte Bloom filter over its members.
/// The value-ref walk calls `contains()` for EVERY identifier in the file, but
/// the import set is tiny — so most identifiers can be rejected by a single
/// array probe on their first byte, skipping the SipHash lookup entirely. The
/// filter is a conservative superset (if the underlying set shrinks via local
/// shadowing, a stale bit just causes a redundant `set.contains()` that returns
/// false), so it is always sound: no false negatives.
pub(crate) struct FilteredNames<'a> {
    set: &'a HashSet<AstString>,
    first_byte_bits: [u64; 4],
}

impl<'a> FilteredNames<'a> {
    pub(crate) fn new(set: &'a HashSet<AstString>) -> Self {
        Self::with_bits(set, Self::compute_bits(set))
    }

    /// Build the 256-bit first-byte mask for `set`. Callers that repeatedly
    /// re-borrow a set that only ever SHRINKS can compute this once and pair it
    /// with fresh borrows via `with_bits` — stale bits are sound (see above).
    pub(crate) fn compute_bits(set: &HashSet<AstString>) -> [u64; 4] {
        let mut first_byte_bits = [0u64; 4];
        for name in set {
            if let Some(&b) = name.as_bytes().first() {
                first_byte_bits[(b >> 6) as usize] |= 1u64 << (b & 63);
            }
        }
        first_byte_bits
    }

    pub(crate) fn with_bits(set: &'a HashSet<AstString>, first_byte_bits: [u64; 4]) -> Self {
        Self {
            set,
            first_byte_bits,
        }
    }

    #[inline]
    pub(crate) fn contains(&self, s: &str) -> bool {
        match s.as_bytes().first() {
            Some(&b) if self.first_byte_bits[(b >> 6) as usize] & (1u64 << (b & 63)) != 0 => {
                self.set.contains(s)
            }
            _ => false,
        }
    }
}

pub(crate) fn collect_value_refs_stmt(
    stmt: &Stmt,
    names: &FilteredNames,
    used: &mut HashSet<AstString>,
    used_non_member: &mut HashSet<AstString>,
) {
    match &stmt.kind {
        StmtKind::Expr(e) => collect_value_refs_expr(e, names, used, used_non_member, false),
        StmtKind::ImportEquals(ie) => {
            collect_value_refs_expr(&ie.module_ref, names, used, used_non_member, false)
        }
        StmtKind::Var(v) => {
            for d in &v.declarations {
                collect_value_refs_pat(&d.name, names, used, used_non_member);
                if let Some(ref init) = d.init {
                    collect_value_refs_expr(init, names, used, used_non_member, false);
                }
            }
        }
        StmtKind::Return(Some(e)) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
            collect_value_refs_expr(e, names, used, used_non_member, false)
        }
        StmtKind::If(i) => {
            collect_value_refs_expr(&i.test, names, used, used_non_member, false);
            collect_value_refs_stmt(&i.consequent, names, used, used_non_member);
            if let Some(ref a) = i.alternate {
                collect_value_refs_stmt(a, names, used, used_non_member);
            }
        }
        StmtKind::Block(ss) => {
            for s in ss {
                collect_value_refs_stmt(s, names, used, used_non_member);
            }
        }
        StmtKind::While(w) => {
            collect_value_refs_expr(&w.test, names, used, used_non_member, false);
            collect_value_refs_stmt(&w.body, names, used, used_non_member);
        }
        StmtKind::DoWhile(dw) => {
            collect_value_refs_stmt(&dw.body, names, used, used_non_member);
            collect_value_refs_expr(&dw.test, names, used, used_non_member, false);
        }
        StmtKind::For(f) => {
            match &f.init {
                Some(ForInit::Expr(e)) => {
                    collect_value_refs_expr(e, names, used, used_non_member, false)
                }
                Some(ForInit::Var(v)) => {
                    for d in &v.declarations {
                        if let Some(ref init) = d.init {
                            collect_value_refs_expr(init, names, used, used_non_member, false);
                        }
                    }
                }
                None => {}
            }
            if let Some(ref t) = f.test {
                collect_value_refs_expr(t, names, used, used_non_member, false);
            }
            if let Some(ref u) = f.update {
                collect_value_refs_expr(u, names, used, used_non_member, false);
            }
            collect_value_refs_stmt(&f.body, names, used, used_non_member);
        }
        StmtKind::ForIn(fi) => {
            match &fi.left {
                ForInOfLeft::Var(v) => {
                    for d in &v.declarations {
                        collect_value_refs_pat(&d.name, names, used, used_non_member);
                    }
                }
                ForInOfLeft::Pat(p) => {
                    collect_value_refs_pat(p, names, used, used_non_member);
                }
                ForInOfLeft::Expr(e) => {
                    collect_value_refs_expr(e, names, used, used_non_member, false);
                }
            }
            collect_value_refs_expr(&fi.right, names, used, used_non_member, false);
            collect_value_refs_stmt(&fi.body, names, used, used_non_member);
        }
        StmtKind::ForOf(fo) => {
            match &fo.left {
                ForInOfLeft::Var(v) => {
                    for d in &v.declarations {
                        collect_value_refs_pat(&d.name, names, used, used_non_member);
                    }
                }
                ForInOfLeft::Pat(p) => {
                    collect_value_refs_pat(p, names, used, used_non_member);
                }
                ForInOfLeft::Expr(e) => {
                    collect_value_refs_expr(e, names, used, used_non_member, false);
                }
            }
            collect_value_refs_expr(&fo.right, names, used, used_non_member, false);
            collect_value_refs_stmt(&fo.body, names, used, used_non_member);
        }
        StmtKind::Switch(sw) => {
            collect_value_refs_expr(&sw.discriminant, names, used, used_non_member, false);
            for case in &sw.cases {
                if let Some(ref test) = case.test {
                    collect_value_refs_expr(test, names, used, used_non_member, false);
                }
                for s in &case.consequent {
                    collect_value_refs_stmt(s, names, used, used_non_member);
                }
            }
        }
        StmtKind::Try(t) => {
            for s in &t.block {
                collect_value_refs_stmt(s, names, used, used_non_member);
            }
            if let Some(ref h) = t.handler {
                for s in &h.body {
                    collect_value_refs_stmt(s, names, used, used_non_member);
                }
            }
            if let Some(ref fin) = t.finalizer {
                for s in fin {
                    collect_value_refs_stmt(s, names, used, used_non_member);
                }
            }
        }
        StmtKind::FnDecl(f) => {
            let mut scoped_names = names.set.clone();
            let mut bindings = Vec::new();
            for p in &f.params {
                collect_binding_names(&p.name, &mut bindings);
            }
            if let Some(body) = &f.body {
                for stmt in body {
                    if let StmtKind::Var(var_stmt) = &stmt.kind {
                        for decl in &var_stmt.declarations {
                            collect_binding_names(&decl.name, &mut bindings);
                        }
                    }
                }
            }
            for binding in bindings {
                scoped_names.remove(binding.as_str());
            }
            let scoped = FilteredNames::new(&scoped_names);
            for p in &f.params {
                collect_value_refs_pat(&p.name, &scoped, used, used_non_member);
                if let Some(ref init) = p.initializer {
                    collect_value_refs_expr(init, &scoped, used, used_non_member, false);
                }
            }
            if let Some(ref body) = f.body {
                for s in body {
                    collect_value_refs_stmt(s, &scoped, used, used_non_member);
                }
            }
            for dec in &f.decorators {
                collect_value_refs_expr(dec, names, used, used_non_member, false);
            }
        }
        StmtKind::ClassDecl(c) => {
            if let Some(ref ext) = c.extends {
                collect_value_refs_expr(ext, names, used, used_non_member, false);
            }
            for dec in &c.decorators {
                collect_value_refs_expr(dec, names, used, used_non_member, false);
            }
            for m in &c.members {
                collect_value_refs_class_member(m, names, used, used_non_member);
            }
        }
        StmtKind::EnumDecl(en) => {
            for m in &en.members {
                collect_value_refs_prop_name(&m.name, names, used, used_non_member);
                if let Some(ref init) = m.initializer {
                    collect_value_refs_expr(init, names, used, used_non_member, false);
                }
            }
        }
        StmtKind::ModuleDecl(md) => {
            if let Some(ModuleBody::Block(ref stmts)) = md.body {
                for s in stmts {
                    collect_value_refs_stmt(s, names, used, used_non_member);
                }
            }
        }
        StmtKind::Export(exp) => match &exp.kind {
            ExportDeclKind::Decl(d) => collect_value_refs_stmt(d, names, used, used_non_member),
            ExportDeclKind::Default(e) => {
                // Treat `export default <expr>` like an export-position usage so
                // imported const-enum aliases don't get forced as runtime values.
                collect_value_refs_expr(e, names, used, used_non_member, true)
            }
            ExportDeclKind::DefaultDecl(d) => {
                collect_value_refs_stmt(d, names, used, used_non_member)
            }
            ExportDeclKind::Named {
                specifiers,
                source: None,
                type_only: false,
            } => {
                for spec in specifiers {
                    if !spec.is_type && names.contains(spec.local.as_str()) {
                        used.insert(AstString::from(spec.local.as_str()));
                    }
                }
            }
            _ => {}
        },
        StmtKind::Labeled(l) => collect_value_refs_stmt(&l.body, names, used, used_non_member),
        StmtKind::With(w) => {
            collect_value_refs_expr(&w.object, names, used, used_non_member, false);
            collect_value_refs_stmt(&w.body, names, used, used_non_member);
        }
        _ => {}
    }
}

/// If `key` is a computed property name, scan the expression for value refs.
pub(crate) fn collect_value_refs_prop_name(
    key: &PropName,
    names: &FilteredNames,
    used: &mut HashSet<AstString>,
    used_non_member: &mut HashSet<AstString>,
) {
    if let PropName::Computed(expr, _) = key {
        collect_value_refs_expr(expr, names, used, used_non_member, false);
    }
}

/// Scan a destructuring pattern for computed property keys that reference
/// imported names (e.g. `{ [importedKey]: value }` in a function parameter).
pub(crate) fn collect_value_refs_pat(
    pat: &Pat,
    names: &FilteredNames,
    used: &mut HashSet<AstString>,
    used_non_member: &mut HashSet<AstString>,
) {
    match &pat.kind {
        PatKind::Object(props) => {
            for prop in props {
                match prop {
                    ObjPatProp::KeyValue(key, inner_pat) => {
                        collect_value_refs_prop_name(key, names, used, used_non_member);
                        collect_value_refs_pat(inner_pat, names, used, used_non_member);
                    }
                    ObjPatProp::Rest(inner_pat) => {
                        collect_value_refs_pat(inner_pat, names, used, used_non_member);
                    }
                    _ => {}
                }
            }
        }
        PatKind::Array(elems) => {
            for elem in elems.iter().flatten() {
                match elem {
                    ArrayPatElem::Pat(p) => {
                        collect_value_refs_pat(p, names, used, used_non_member);
                    }
                    ArrayPatElem::Rest(p) => {
                        collect_value_refs_pat(p, names, used, used_non_member);
                    }
                }
            }
        }
        PatKind::Assign(inner_pat, init) => {
            collect_value_refs_pat(inner_pat, names, used, used_non_member);
            collect_value_refs_expr(init, names, used, used_non_member, false);
        }
        PatKind::Rest(inner_pat) => {
            collect_value_refs_pat(inner_pat, names, used, used_non_member);
        }
        PatKind::Ident(_) => {}
    }
}

pub(crate) fn collect_value_refs_class_member(
    member: &ClassMember,
    names: &FilteredNames,
    used: &mut HashSet<AstString>,
    used_non_member: &mut HashSet<AstString>,
) {
    match &member.kind {
        ClassMemberKind::Property(p) => {
            collect_value_refs_prop_name(&p.name, names, used, used_non_member);
            if let Some(ref init) = p.initializer {
                collect_value_refs_expr(init, names, used, used_non_member, false);
            }
            for dec in &p.decorators {
                collect_value_refs_expr(dec, names, used, used_non_member, false);
            }
        }
        ClassMemberKind::Method(m) => {
            collect_value_refs_prop_name(&m.name, names, used, used_non_member);
            for p in &m.params {
                collect_value_refs_pat(&p.name, names, used, used_non_member);
                if let Some(ref init) = p.initializer {
                    collect_value_refs_expr(init, names, used, used_non_member, false);
                }
                for dec in &p.decorators {
                    collect_value_refs_expr(dec, names, used, used_non_member, false);
                }
            }
            if let Some(ref body) = m.body {
                for s in body {
                    collect_value_refs_stmt(s, names, used, used_non_member);
                }
            }
            for dec in &m.decorators {
                collect_value_refs_expr(dec, names, used, used_non_member, false);
            }
        }
        ClassMemberKind::Constructor(ct) => {
            for p in &ct.params {
                collect_value_refs_pat(&p.name, names, used, used_non_member);
                if let Some(ref init) = p.initializer {
                    collect_value_refs_expr(init, names, used, used_non_member, false);
                }
                for dec in &p.decorators {
                    collect_value_refs_expr(dec, names, used, used_non_member, false);
                }
            }
            if let Some(ref body) = ct.body {
                for s in body {
                    collect_value_refs_stmt(s, names, used, used_non_member);
                }
            }
        }
        ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
            collect_value_refs_prop_name(&acc.name, names, used, used_non_member);
            for p in &acc.params {
                collect_value_refs_pat(&p.name, names, used, used_non_member);
                if let Some(ref init) = p.initializer {
                    collect_value_refs_expr(init, names, used, used_non_member, false);
                }
                for dec in &p.decorators {
                    collect_value_refs_expr(dec, names, used, used_non_member, false);
                }
            }
            if let Some(ref body) = acc.body {
                for s in body {
                    collect_value_refs_stmt(s, names, used, used_non_member);
                }
            }
            for dec in &acc.decorators {
                collect_value_refs_expr(dec, names, used, used_non_member, false);
            }
        }
        ClassMemberKind::StaticBlock(ss) => {
            for s in ss {
                collect_value_refs_stmt(s, names, used, used_non_member);
            }
        }
        _ => {}
    }
}

pub(crate) fn collect_value_refs_expr(
    expr: &Expr,
    names: &FilteredNames,
    used: &mut HashSet<AstString>,
    used_non_member: &mut HashSet<AstString>,
    in_member_object: bool,
) {
    let mut stack = vec![(expr, in_member_object)];
    while let Some((expr, in_member_object)) = stack.pop() {
        match &expr.kind {
            ExprKind::Ident(n) => {
                if names.contains(n.as_str()) {
                    used.insert(AstString::from(n.as_str()));
                    if !in_member_object {
                        used_non_member.insert(AstString::from(n.as_str()));
                    }
                }
            }
            ExprKind::Call(c) => {
                for arg in c.args.iter().rev() {
                    stack.push((arg, false));
                }
                stack.push((&c.callee, false));
            }
            ExprKind::New(n) => {
                if let Some(args) = &n.args {
                    for arg in args.iter().rev() {
                        stack.push((arg, false));
                    }
                }
                stack.push((&n.callee, false));
            }
            ExprKind::Member(m) => stack.push((&m.object, true)),
            ExprKind::ElemAccess(ea) => {
                stack.push((&ea.index, false));
                stack.push((&ea.object, true));
            }
            ExprKind::Binary(b) => {
                stack.push((&b.right, false));
                stack.push((&b.left, false));
            }
            ExprKind::Assign(a) => {
                stack.push((&a.right, false));
                stack.push((&a.left, false));
            }
            ExprKind::Unary(u) => stack.push((&u.argument, false)),
            ExprKind::Update(u) => stack.push((&u.argument, false)),
            ExprKind::Paren(e) => stack.push((e, false)),
            ExprKind::Cond(c) => {
                stack.push((&c.alternate, false));
                stack.push((&c.consequent, false));
                stack.push((&c.test, false));
            }
            ExprKind::Arrow(a) => {
                for p in &a.params {
                    collect_value_refs_pat(&p.name, names, used, used_non_member);
                    if let Some(init) = &p.initializer {
                        stack.push((init, false));
                    }
                }
                match &a.body {
                    ArrowBody::Expr(e) => stack.push((e, false)),
                    ArrowBody::Block(ss) => {
                        for s in ss {
                            collect_value_refs_stmt(s, names, used, used_non_member);
                        }
                    }
                }
            }
            ExprKind::FnExpr(f) => {
                for p in &f.params {
                    collect_value_refs_pat(&p.name, names, used, used_non_member);
                    if let Some(init) = &p.initializer {
                        stack.push((init, false));
                    }
                }
                if let Some(body) = &f.body {
                    for s in body {
                        collect_value_refs_stmt(s, names, used, used_non_member);
                    }
                }
            }
            ExprKind::ClassExpr(c) => {
                if let Some(ext) = &c.extends {
                    stack.push((ext, false));
                }
                for dec in c.decorators.iter().rev() {
                    stack.push((dec, false));
                }
                for m in &c.members {
                    collect_value_refs_class_member(m, names, used, used_non_member);
                }
            }
            ExprKind::ArrayLit(elems) => {
                for inner in elems.iter().rev().flatten() {
                    stack.push((inner, false));
                }
            }
            ExprKind::ObjectLit(props) => {
                for p in props.iter().rev() {
                    match p {
                        ObjLitProp::Property(pr) => {
                            collect_value_refs_prop_name(&pr.key, names, used, used_non_member);
                            stack.push((&pr.value, false));
                        }
                        ObjLitProp::Shorthand(n, _) => {
                            if names.contains(n.as_str()) {
                                used.insert(AstString::from(n.as_str()));
                                used_non_member.insert(AstString::from(n.as_str()));
                            }
                        }
                        ObjLitProp::ShorthandDefault(n, e, _) => {
                            if names.contains(n.as_str()) {
                                used.insert(AstString::from(n.as_str()));
                                used_non_member.insert(AstString::from(n.as_str()));
                            }
                            stack.push((e, false));
                        }
                        ObjLitProp::Spread(e, _) => stack.push((e, false)),
                        ObjLitProp::Method(m) => {
                            collect_value_refs_prop_name(&m.name, names, used, used_non_member);
                            for s in &m.body {
                                collect_value_refs_stmt(s, names, used, used_non_member);
                            }
                        }
                        ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                            collect_value_refs_prop_name(&acc.name, names, used, used_non_member);
                            for s in &acc.body {
                                collect_value_refs_stmt(s, names, used, used_non_member);
                            }
                        }
                    }
                }
            }
            ExprKind::Template(t) => {
                for inner in t.exprs.iter().rev() {
                    stack.push((inner, false));
                }
            }
            ExprKind::TaggedTemplate(t) => {
                for inner in t.quasi.exprs.iter().rev() {
                    stack.push((inner, false));
                }
                stack.push((&t.tag, false));
            }
            ExprKind::JsxElement(jsx) => {
                stack.push((&jsx.name, false));
                for attr in jsx.attributes.iter().rev() {
                    match attr {
                        JsxAttribute::Normal { value: Some(v), .. } => stack.push((v, false)),
                        JsxAttribute::Spread(e, _) => stack.push((e, false)),
                        _ => {}
                    }
                }
                for child in jsx.children.iter().rev() {
                    match child {
                        JsxChild::Element(e) => stack.push((e, false)),
                        JsxChild::Expression(Some(e), _) => stack.push((e, false)),
                        JsxChild::Fragment(frag) => {
                            for nested in frag.children.iter().rev() {
                                match nested {
                                    JsxChild::Element(e) => stack.push((e, false)),
                                    JsxChild::Expression(Some(e), _) => stack.push((e, false)),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::JsxSelfClosing(jsx) => {
                stack.push((&jsx.name, false));
                for attr in jsx.attributes.iter().rev() {
                    match attr {
                        JsxAttribute::Normal { value: Some(v), .. } => stack.push((v, false)),
                        JsxAttribute::Spread(e, _) => stack.push((e, false)),
                        _ => {}
                    }
                }
            }
            ExprKind::JsxFragment(frag) => {
                for child in frag.children.iter().rev() {
                    match child {
                        JsxChild::Element(e) => stack.push((e, false)),
                        JsxChild::Expression(Some(e), _) => stack.push((e, false)),
                        JsxChild::Fragment(nested) => {
                            for inner in nested.children.iter().rev() {
                                match inner {
                                    JsxChild::Element(e) => stack.push((e, false)),
                                    JsxChild::Expression(Some(e), _) => stack.push((e, false)),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::Await(e) => {
                // The parser may create Await nodes for `await(...)` even when `await`
                // is an imported identifier used as a function call (non-async context).
                // Mark "await" as value-used so it isn't elided from imports.
                if names.contains("await") {
                    used.insert(AstString::from("await"));
                }
                stack.push((e, false));
            }
            ExprKind::Spread(e)
            | ExprKind::NonNull(e)
            | ExprKind::Delete(e)
            | ExprKind::Typeof(e)
            | ExprKind::Void(e) => stack.push((e, false)),
            ExprKind::As(a) => stack.push((&a.expr, false)),
            ExprKind::Satisfies(s) => stack.push((&s.expr, false)),
            ExprKind::TypeAssertion(ta) => stack.push((&ta.expr, false)),
            ExprKind::Instantiation(inst) => stack.push((&inst.expr, false)),
            ExprKind::Comma(es) => {
                for inner in es.iter().rev() {
                    stack.push((inner, false));
                }
            }
            ExprKind::Yield(_, Some(e)) => stack.push((e, false)),
            _ => {}
        }
    }
}

/// Check if an expression tree contains any JSX nodes.
pub(crate) fn expr_contains_jsx(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::JsxElement(_) | ExprKind::JsxSelfClosing(_) | ExprKind::JsxFragment(_) => true,
        ExprKind::Paren(e)
        | ExprKind::Spread(e)
        | ExprKind::Await(e)
        | ExprKind::NonNull(e)
        | ExprKind::Delete(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e) => expr_contains_jsx(e),
        ExprKind::Unary(u) => expr_contains_jsx(&u.argument),
        ExprKind::Update(u) => expr_contains_jsx(&u.argument),
        ExprKind::As(a) => expr_contains_jsx(&a.expr),
        ExprKind::Satisfies(s) => expr_contains_jsx(&s.expr),
        ExprKind::TypeAssertion(ta) => expr_contains_jsx(&ta.expr),
        ExprKind::Binary(b) => expr_contains_jsx(&b.left) || expr_contains_jsx(&b.right),
        ExprKind::Assign(a) => expr_contains_jsx(&a.left) || expr_contains_jsx(&a.right),
        ExprKind::Cond(c) => {
            expr_contains_jsx(&c.test)
                || expr_contains_jsx(&c.consequent)
                || expr_contains_jsx(&c.alternate)
        }
        ExprKind::Call(c) => {
            expr_contains_jsx(&c.callee) || c.args.iter().any(|a| expr_contains_jsx(a))
        }
        ExprKind::New(n) => {
            expr_contains_jsx(&n.callee)
                || n.args
                    .as_ref()
                    .is_some_and(|a| a.iter().any(|e| expr_contains_jsx(e)))
        }
        ExprKind::Arrow(a) => match &a.body {
            ArrowBody::Expr(e) => expr_contains_jsx(e),
            ArrowBody::Block(ss) => ss.iter().any(stmt_contains_jsx),
        },
        ExprKind::FnExpr(f) => f
            .body
            .as_ref()
            .is_some_and(|b| b.iter().any(stmt_contains_jsx)),
        ExprKind::ArrayLit(elems) => elems.iter().flatten().any(|e| expr_contains_jsx(e)),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(pr) => expr_contains_jsx(&pr.value),
            ObjLitProp::Spread(e, _) => expr_contains_jsx(e),
            ObjLitProp::Method(m) => m.body.iter().any(stmt_contains_jsx),
            ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => acc.body.iter().any(stmt_contains_jsx),
            _ => false,
        }),
        ExprKind::Comma(es) => es.iter().any(|e| expr_contains_jsx(e)),
        ExprKind::Member(m) => expr_contains_jsx(&m.object),
        ExprKind::ElemAccess(ea) => expr_contains_jsx(&ea.object) || expr_contains_jsx(&ea.index),
        ExprKind::Template(t) => t.exprs.iter().any(|e| expr_contains_jsx(e)),
        ExprKind::TaggedTemplate(t) => {
            expr_contains_jsx(&t.tag) || t.quasi.exprs.iter().any(|e| expr_contains_jsx(e))
        }
        ExprKind::ClassExpr(c) => {
            c.extends.as_ref().is_some_and(|e| expr_contains_jsx(e))
                || c.members.iter().any(class_member_contains_jsx)
        }
        _ => false,
    }
}

pub(crate) fn expr_contains_class_expr(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ClassExpr(_) => true,
        ExprKind::Paren(e)
        | ExprKind::Spread(e)
        | ExprKind::Await(e)
        | ExprKind::NonNull(e)
        | ExprKind::Delete(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e) => expr_contains_class_expr(e),
        ExprKind::Unary(u) => expr_contains_class_expr(&u.argument),
        ExprKind::Update(u) => expr_contains_class_expr(&u.argument),
        ExprKind::As(a) => expr_contains_class_expr(&a.expr),
        ExprKind::Satisfies(s) => expr_contains_class_expr(&s.expr),
        ExprKind::TypeAssertion(ta) => expr_contains_class_expr(&ta.expr),
        ExprKind::Binary(b) => {
            expr_contains_class_expr(&b.left) || expr_contains_class_expr(&b.right)
        }
        ExprKind::Assign(a) => {
            expr_contains_class_expr(&a.left) || expr_contains_class_expr(&a.right)
        }
        ExprKind::Cond(c) => {
            expr_contains_class_expr(&c.test)
                || expr_contains_class_expr(&c.consequent)
                || expr_contains_class_expr(&c.alternate)
        }
        ExprKind::Call(c) => {
            expr_contains_class_expr(&c.callee)
                || c.args.iter().any(|arg| expr_contains_class_expr(arg))
        }
        ExprKind::New(n) => {
            expr_contains_class_expr(&n.callee)
                || n.args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|arg| expr_contains_class_expr(arg)))
        }
        ExprKind::Arrow(a) => match &a.body {
            ArrowBody::Expr(e) => {
                a.params.iter().any(param_contains_class_expr) || expr_contains_class_expr(e)
            }
            ArrowBody::Block(ss) => {
                a.params.iter().any(param_contains_class_expr)
                    || ss.iter().any(stmt_contains_class_expr)
            }
        },
        ExprKind::FnExpr(f) => {
            f.params.iter().any(param_contains_class_expr)
                || f.body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_contains_class_expr))
        }
        ExprKind::ArrayLit(elems) => elems
            .iter()
            .flatten()
            .any(|elem| expr_contains_class_expr(elem)),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(pr) => expr_contains_class_expr(&pr.value),
            ObjLitProp::ShorthandDefault(_, init, _) => expr_contains_class_expr(init),
            ObjLitProp::Spread(e, _) => expr_contains_class_expr(e),
            ObjLitProp::Method(m) => m.body.iter().any(stmt_contains_class_expr),
            ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                acc.body.iter().any(stmt_contains_class_expr)
            }
            ObjLitProp::Shorthand(_, _) => false,
        }),
        ExprKind::Comma(es) => es.iter().any(|expr| expr_contains_class_expr(expr)),
        ExprKind::Member(m) => expr_contains_class_expr(&m.object),
        ExprKind::ElemAccess(ea) => {
            expr_contains_class_expr(&ea.object) || expr_contains_class_expr(&ea.index)
        }
        ExprKind::Template(t) => t.exprs.iter().any(|expr| expr_contains_class_expr(expr)),
        ExprKind::TaggedTemplate(t) => {
            expr_contains_class_expr(&t.tag)
                || t.quasi
                    .exprs
                    .iter()
                    .any(|expr| expr_contains_class_expr(expr))
        }
        _ => false,
    }
}

pub(crate) fn class_member_contains_jsx(m: &ClassMember) -> bool {
    match &m.kind {
        ClassMemberKind::Method(method) => method
            .body
            .as_ref()
            .is_some_and(|b| b.iter().any(stmt_contains_jsx)),
        ClassMemberKind::Constructor(ctor) => ctor
            .body
            .as_ref()
            .is_some_and(|b| b.iter().any(stmt_contains_jsx)),
        ClassMemberKind::Property(prop) => prop
            .initializer
            .as_ref()
            .is_some_and(|e| expr_contains_jsx(e)),
        ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => acc
            .body
            .as_ref()
            .is_some_and(|b| b.iter().any(stmt_contains_jsx)),
        ClassMemberKind::StaticBlock(stmts) => stmts.iter().any(stmt_contains_jsx),
        _ => false,
    }
}

pub(crate) fn stmt_contains_class_expr(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) => expr_contains_class_expr(e),
        StmtKind::Return(Some(e)) => expr_contains_class_expr(e),
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            pat_contains_class_expr(&d.name)
                || d.init
                    .as_ref()
                    .is_some_and(|expr| expr_contains_class_expr(expr))
        }),
        StmtKind::If(if_stmt) => {
            expr_contains_class_expr(&if_stmt.test)
                || stmt_contains_class_expr(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|s| stmt_contains_class_expr(s))
        }
        StmtKind::Block(stmts) => stmts.iter().any(stmt_contains_class_expr),
        StmtKind::FnDecl(f) => {
            f.params.iter().any(param_contains_class_expr)
                || f.body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_contains_class_expr))
        }
        StmtKind::ClassDecl(_) => false,
        StmtKind::For(f) => stmt_contains_class_expr(&f.body),
        StmtKind::ForIn(f) => stmt_contains_class_expr(&f.body),
        StmtKind::ForOf(f) => stmt_contains_class_expr(&f.body),
        StmtKind::While(w) => stmt_contains_class_expr(&w.body),
        StmtKind::DoWhile(dw) => stmt_contains_class_expr(&dw.body),
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(stmt_contains_class_expr)),
        StmtKind::Try(t) => {
            t.block.iter().any(stmt_contains_class_expr)
                || t.handler
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_contains_class_expr))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_class_expr))
        }
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) => stmt_contains_class_expr(inner),
            ExportDeclKind::Default(e) => expr_contains_class_expr(e),
            _ => false,
        },
        StmtKind::Labeled(l) => stmt_contains_class_expr(&l.body),
        StmtKind::With(w) => stmt_contains_class_expr(&w.body),
        _ => false,
    }
}

fn param_contains_class_expr(param: &Param) -> bool {
    pat_contains_class_expr(&param.name)
        || param
            .initializer
            .as_ref()
            .is_some_and(|expr| expr_contains_class_expr(expr))
}

fn pat_contains_class_expr(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Array(elements) => elements.iter().flatten().any(|elem| match elem {
            ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => pat_contains_class_expr(pat),
        }),
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(key, value) => {
                prop_name_contains_class_expr(key) || pat_contains_class_expr(value)
            }
            ObjPatProp::Shorthand(_, _) => false,
            ObjPatProp::ShorthandAssign(_, init, _) => expr_contains_class_expr(init),
            ObjPatProp::Rest(pat) => pat_contains_class_expr(pat),
        }),
        PatKind::Assign(pat, init) => {
            pat_contains_class_expr(pat) || expr_contains_class_expr(init)
        }
        PatKind::Rest(pat) => pat_contains_class_expr(pat),
    }
}

fn prop_name_contains_class_expr(name: &PropName) -> bool {
    match name {
        PropName::Computed(expr, _) => expr_contains_class_expr(expr),
        _ => false,
    }
}

pub(crate) fn stmt_contains_jsx(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) => expr_contains_jsx(e),
        StmtKind::Return(Some(e)) => expr_contains_jsx(e),
        StmtKind::Var(var_stmt) => var_stmt
            .declarations
            .iter()
            .any(|d| d.init.as_ref().is_some_and(|e| expr_contains_jsx(e))),
        StmtKind::If(if_stmt) => {
            expr_contains_jsx(&if_stmt.test)
                || stmt_contains_jsx(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|s| stmt_contains_jsx(s))
        }
        StmtKind::Block(stmts) => stmts.iter().any(stmt_contains_jsx),
        StmtKind::FnDecl(f) => f
            .body
            .as_ref()
            .is_some_and(|b| b.iter().any(stmt_contains_jsx)),
        StmtKind::ClassDecl(c) => c.members.iter().any(class_member_contains_jsx),
        StmtKind::For(f) => stmt_contains_jsx(&f.body),
        StmtKind::ForIn(f) => stmt_contains_jsx(&f.body),
        StmtKind::ForOf(f) => stmt_contains_jsx(&f.body),
        StmtKind::While(w) => stmt_contains_jsx(&w.body),
        StmtKind::DoWhile(dw) => stmt_contains_jsx(&dw.body),
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(stmt_contains_jsx)),
        StmtKind::Try(t) => {
            t.block.iter().any(stmt_contains_jsx)
                || t.handler
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_contains_jsx))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_jsx))
        }
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) => stmt_contains_jsx(inner),
            ExportDeclKind::Default(e) => expr_contains_jsx(e),
            _ => false,
        },
        StmtKind::Labeled(l) => stmt_contains_jsx(&l.body),
        StmtKind::With(w) => stmt_contains_jsx(&w.body),
        _ => false,
    }
}

/// Collect uppercase JSX component identifiers from expressions.
/// In JSX preserve mode, uppercase tag names like `<MyComponent />`
/// reference a value that must not be import-elided.
fn collect_jsx_component_names_expr(expr: &Expr, names: &mut std::collections::HashSet<String>) {
    match &expr.kind {
        ExprKind::JsxElement(el) => {
            collect_jsx_tag_name_root(&el.name, names);
            for child in &el.children {
                match child {
                    JsxChild::Element(e) => collect_jsx_component_names_expr(e, names),
                    JsxChild::Expression(Some(e), _) => collect_jsx_component_names_expr(e, names),
                    JsxChild::Fragment(f) => {
                        for c in &f.children {
                            match c {
                                JsxChild::Element(e) => collect_jsx_component_names_expr(e, names),
                                JsxChild::Expression(Some(e), _) => {
                                    collect_jsx_component_names_expr(e, names)
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            for attr in &el.attributes {
                match attr {
                    JsxAttribute::Normal { value: Some(v), .. } => {
                        collect_jsx_component_names_expr(v, names)
                    }
                    JsxAttribute::Spread(e, _) => collect_jsx_component_names_expr(e, names),
                    _ => {}
                }
            }
        }
        ExprKind::JsxSelfClosing(el) => {
            collect_jsx_tag_name_root(&el.name, names);
            for attr in &el.attributes {
                match attr {
                    JsxAttribute::Normal { value: Some(v), .. } => {
                        collect_jsx_component_names_expr(v, names)
                    }
                    JsxAttribute::Spread(e, _) => collect_jsx_component_names_expr(e, names),
                    _ => {}
                }
            }
        }
        ExprKind::JsxFragment(frag) => {
            for child in &frag.children {
                match child {
                    JsxChild::Element(e) => collect_jsx_component_names_expr(e, names),
                    JsxChild::Expression(Some(e), _) => collect_jsx_component_names_expr(e, names),
                    _ => {}
                }
            }
        }
        ExprKind::Paren(e) | ExprKind::Spread(e) | ExprKind::Await(e) | ExprKind::NonNull(e) => {
            collect_jsx_component_names_expr(e, names)
        }
        ExprKind::As(a) => collect_jsx_component_names_expr(&a.expr, names),
        ExprKind::Satisfies(s) => collect_jsx_component_names_expr(&s.expr, names),
        ExprKind::TypeAssertion(ta) => collect_jsx_component_names_expr(&ta.expr, names),
        ExprKind::Cond(c) => {
            collect_jsx_component_names_expr(&c.test, names);
            collect_jsx_component_names_expr(&c.consequent, names);
            collect_jsx_component_names_expr(&c.alternate, names);
        }
        ExprKind::Call(c) => {
            collect_jsx_component_names_expr(&c.callee, names);
            for a in &c.args {
                collect_jsx_component_names_expr(a, names);
            }
        }
        ExprKind::Arrow(a) => match &a.body {
            ArrowBody::Expr(e) => collect_jsx_component_names_expr(e, names),
            ArrowBody::Block(ss) => {
                for s in ss {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        },
        ExprKind::FnExpr(f) => {
            if let Some(body) = &f.body {
                for s in body {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems.iter().flatten() {
                collect_jsx_component_names_expr(e, names);
            }
        }
        ExprKind::ObjectLit(props) => {
            for p in props {
                match p {
                    ObjLitProp::Property(pr) => collect_jsx_component_names_expr(&pr.value, names),
                    ObjLitProp::Spread(e, _) => collect_jsx_component_names_expr(e, names),
                    ObjLitProp::Method(m) => {
                        for s in &m.body {
                            collect_jsx_component_names_stmt(s, names);
                        }
                    }
                    _ => {}
                }
            }
        }
        ExprKind::ClassExpr(c) => {
            for m in &c.members {
                collect_jsx_component_names_member(m, names);
            }
        }
        ExprKind::Binary(b) => {
            collect_jsx_component_names_expr(&b.left, names);
            collect_jsx_component_names_expr(&b.right, names);
        }
        ExprKind::Assign(a) => {
            collect_jsx_component_names_expr(&a.left, names);
            collect_jsx_component_names_expr(&a.right, names);
        }
        ExprKind::Comma(es) => {
            for e in es {
                collect_jsx_component_names_expr(e, names);
            }
        }
        ExprKind::Template(t) => {
            for e in &t.exprs {
                collect_jsx_component_names_expr(e, names);
            }
        }
        _ => {}
    }
}

fn collect_jsx_tag_name_root(name: &Expr, names: &mut std::collections::HashSet<String>) {
    match &name.kind {
        ExprKind::Ident(n) => {
            // Only uppercase (component) names are value references
            if n.chars().next().is_some_and(|c| c.is_uppercase()) {
                names.insert(n.to_string());
            }
        }
        ExprKind::Member(m) => {
            // For `Ns.Component`, collect the root `Ns`
            collect_jsx_tag_name_root(&m.object, names);
        }
        _ => {}
    }
}

fn collect_jsx_component_names_stmt(stmt: &Stmt, names: &mut std::collections::HashSet<String>) {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Return(Some(e)) => {
            collect_jsx_component_names_expr(e, names);
        }
        StmtKind::Var(vs) => {
            for d in &vs.declarations {
                if let Some(init) = &d.init {
                    collect_jsx_component_names_expr(init, names);
                }
            }
        }
        StmtKind::Block(ss) => {
            for s in ss {
                collect_jsx_component_names_stmt(s, names);
            }
        }
        StmtKind::If(i) => {
            collect_jsx_component_names_expr(&i.test, names);
            collect_jsx_component_names_stmt(&i.consequent, names);
            if let Some(alt) = &i.alternate {
                collect_jsx_component_names_stmt(alt, names);
            }
        }
        StmtKind::FnDecl(f) => {
            if let Some(body) = &f.body {
                for s in body {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        }
        StmtKind::ClassDecl(c) => {
            for m in &c.members {
                collect_jsx_component_names_member(m, names);
            }
        }
        StmtKind::For(f) => collect_jsx_component_names_stmt(&f.body, names),
        StmtKind::ForIn(f) => collect_jsx_component_names_stmt(&f.body, names),
        StmtKind::ForOf(f) => collect_jsx_component_names_stmt(&f.body, names),
        StmtKind::While(w) => collect_jsx_component_names_stmt(&w.body, names),
        StmtKind::DoWhile(dw) => collect_jsx_component_names_stmt(&dw.body, names),
        StmtKind::Switch(sw) => {
            for c in &sw.cases {
                for s in &c.consequent {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        }
        StmtKind::Try(t) => {
            for s in &t.block {
                collect_jsx_component_names_stmt(s, names);
            }
            if let Some(h) = &t.handler {
                for s in &h.body {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
            if let Some(f) = &t.finalizer {
                for s in f {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        }
        StmtKind::Export(e) => match &e.kind {
            ExportDeclKind::Decl(inner) => collect_jsx_component_names_stmt(inner, names),
            ExportDeclKind::Default(e) => collect_jsx_component_names_expr(e, names),
            _ => {}
        },
        _ => {}
    }
}

fn collect_jsx_component_names_member(
    m: &ClassMember,
    names: &mut std::collections::HashSet<String>,
) {
    match &m.kind {
        ClassMemberKind::Method(method) => {
            if let Some(body) = &method.body {
                for s in body {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        }
        ClassMemberKind::Constructor(ctor) => {
            if let Some(body) = &ctor.body {
                for s in body {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        }
        ClassMemberKind::Property(prop) => {
            if let Some(init) = &prop.initializer {
                collect_jsx_component_names_expr(init, names);
            }
        }
        ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
            if let Some(body) = &acc.body {
                for s in body {
                    collect_jsx_component_names_stmt(s, names);
                }
            }
        }
        ClassMemberKind::StaticBlock(stmts) => {
            for s in stmts {
                collect_jsx_component_names_stmt(s, names);
            }
        }
        _ => {}
    }
}

/// Collect all JSX component names used in statements.
pub(crate) fn collect_jsx_component_names(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for s in stmts {
        collect_jsx_component_names_stmt(s, &mut names);
    }
    names
}
