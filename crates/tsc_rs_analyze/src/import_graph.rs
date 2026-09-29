//! Import graph walker.
//!
//! Recursively parses TypeScript files, extracts import/export/require
//! specifiers, resolves them, and builds a complete import graph.
//! Tracks which bare package names are reachable and the file chains
//! that led to each, distinguishing type-only from runtime imports.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use tsc_rs_ast::{
    ArrowBody, ClassMemberKind, CompilerOptions, ExportDeclKind, Expr, ExprKind, ModuleBody,
    ObjLitProp, Stmt, StmtKind,
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single import edge: file A imports specifier S, resolved to file B (or unresolved).
#[derive(Debug, Clone)]
pub struct ImportEdge {
    /// The raw specifier as written in the source (e.g. `"@acme/db"`, `"./utils"`).
    pub specifier: String,
    /// The resolved absolute file path, if resolution succeeded.
    pub resolved_path: Option<String>,
    /// The bare package name extracted from the specifier (e.g. `@acme/db`).
    /// `None` for relative imports and path-aliased imports that resolve locally.
    pub package_name: Option<String>,
    /// Whether this is a type-only import (`import type { ... }`).
    pub type_only: bool,
    /// Whether this is a dynamic import (`import("...")`).
    pub dynamic: bool,
}

/// Per-package usage info in the import graph.
#[derive(Debug, Clone)]
pub struct PackageUsage {
    /// Example import chain showing how this package was first reached.
    pub chain: ImportChain,
    /// Whether ALL imports of this package are type-only.
    /// If true, the package doesn't affect the runtime bundle.
    pub type_only: bool,
    /// Number of files that import this package.
    pub import_count: u32,
}

/// Complete import graph built from walking entry files.
#[derive(Debug)]
pub struct ImportGraph {
    /// Map from source file path → list of import edges from that file.
    pub edges: HashMap<String, Vec<ImportEdge>>,
    /// All bare package names encountered (non-relative specifiers), with
    /// usage info for each.
    pub packages: BTreeMap<String, PackageUsage>,
    /// Files that were visited during the walk.
    pub visited_files: HashSet<String>,
    /// Specifiers that could not be resolved.
    pub unresolved: Vec<UnresolvedImport>,
    /// Files that could not be read (permissions, encoding, etc.).
    pub read_errors: u32,
}

/// An example chain showing how a package was reached.
#[derive(Debug, Clone)]
pub struct ImportChain {
    /// Sequence of file paths from entry to the importing file.
    pub files: Vec<String>,
    /// The specifier that references the package.
    pub specifier: String,
}

/// An import that could not be resolved.
#[derive(Debug, Clone)]
pub struct UnresolvedImport {
    pub from_file: String,
    pub specifier: String,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Configuration for building an import graph.
pub struct ImportGraphBuilder {
    pub options: CompilerOptions,
    /// Stop recursing into files under node_modules (default: true).
    pub skip_node_modules: bool,
    /// Maximum depth to recurse (0 = unlimited).
    pub max_depth: usize,
    /// Extra filenames to try when resolving directory imports (beyond index.ts/tsx).
    /// e.g., `["router.ts", "router.tsx"]` for TRPC routers.
    pub extra_dir_entries: Vec<String>,
}

impl ImportGraphBuilder {
    pub fn new(options: CompilerOptions) -> Self {
        Self {
            options,
            skip_node_modules: true,
            max_depth: 0,
            extra_dir_entries: vec!["router.ts".into(), "router.tsx".into(), "service.ts".into()],
        }
    }

    /// Build an import graph starting from the given entry files.
    pub fn build(&self, entry_files: &[String]) -> ImportGraph {
        let mut graph = ImportGraph {
            edges: HashMap::new(),
            packages: BTreeMap::new(),
            visited_files: HashSet::new(),
            unresolved: Vec::new(),
            read_errors: 0,
        };

        // BFS queue: (file_path, chain_so_far)
        let mut queue: VecDeque<(String, Vec<String>)> = VecDeque::new();
        for entry in entry_files {
            let abs = normalize_to_absolute(entry);
            queue.push_back((abs.clone(), vec![abs]));
        }

        while let Some((file_path, chain)) = queue.pop_front() {
            if graph.visited_files.contains(&file_path) {
                continue;
            }
            if self.max_depth > 0 && chain.len() > self.max_depth {
                continue;
            }
            graph.visited_files.insert(file_path.clone());

            // Read and parse
            let source = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(_) => {
                    graph.read_errors += 1;
                    continue;
                }
            };
            let source_file = tsc_rs_parser::parse(&file_path, &source);

            // Extract all import specifiers
            let raw_edges = extract_all_specifiers(&source_file);
            let mut resolved_edges = Vec::new();

            for (specifier, type_only, dynamic) in &raw_edges {
                // Resolve the import
                let resolved =
                    tsc_rs_resolver::resolve_module_name(specifier, &file_path, &self.options);
                let resolved_path = resolved
                    .as_ref()
                    .map(|r| r.resolved_file_name.clone())
                    .or_else(|| {
                        // Fallback: for relative imports, try extra directory entry files
                        // (e.g., router.ts for TRPC routers)
                        if !is_relative(specifier) || self.extra_dir_entries.is_empty() {
                            return None;
                        }
                        let dir = Path::new(&file_path).parent()?;
                        let target_dir = dir.join(specifier);
                        for entry_name in &self.extra_dir_entries {
                            let candidate = target_dir.join(entry_name);
                            if candidate.is_file() {
                                return candidate
                                    .canonicalize()
                                    .ok()
                                    .map(|p| p.to_string_lossy().to_string());
                            }
                        }
                        None
                    });
                // Determine if this is a real package or a project-internal path alias.
                // Project aliases like @/*, ~/* resolve to local files and are NOT packages.
                // Workspace packages like @acme/db also resolve via path mapping
                // but ARE packages — they must be counted.
                let package_name = if is_relative(specifier) {
                    None
                } else if is_project_alias(specifier) {
                    // @/foo, ~/foo, etc. — project-internal aliases, never packages
                    None
                } else {
                    let pkg = extract_package_name(specifier);
                    if is_node_builtin(pkg) {
                        None
                    } else {
                        Some(pkg.to_string())
                    }
                };

                // Track package usage
                if let Some(ref pkg) = package_name {
                    match graph.packages.get_mut(pkg) {
                        Some(usage) => {
                            // Package already seen — update type_only flag
                            if !type_only {
                                usage.type_only = false;
                            }
                            usage.import_count += 1;
                        }
                        None => {
                            graph.packages.insert(
                                pkg.clone(),
                                PackageUsage {
                                    chain: ImportChain {
                                        files: chain.clone(),
                                        specifier: specifier.clone(),
                                    },
                                    type_only: *type_only,
                                    import_count: 1,
                                },
                            );
                        }
                    }
                }

                // Record unresolved relative imports
                if resolved_path.is_none() && is_relative(specifier) {
                    graph.unresolved.push(UnresolvedImport {
                        from_file: file_path.clone(),
                        specifier: specifier.clone(),
                    });
                }

                // Enqueue resolved local files for further walking
                if let Some(ref rp) = resolved_path {
                    if !graph.visited_files.contains(rp)
                        && should_follow(rp, self.skip_node_modules)
                    {
                        let mut next_chain = chain.clone();
                        next_chain.push(rp.clone());
                        queue.push_back((rp.clone(), next_chain));
                    }
                }

                resolved_edges.push(ImportEdge {
                    specifier: specifier.clone(),
                    resolved_path,
                    package_name,
                    type_only: *type_only,
                    dynamic: *dynamic,
                });
            }

            graph.edges.insert(file_path, resolved_edges);
        }

        graph
    }
}

// ---------------------------------------------------------------------------
// Specifier extraction from AST (deep walk)
// ---------------------------------------------------------------------------

/// Extract all import/export/require specifiers from a parsed source file.
/// Walks the full AST including function bodies, class methods, arrow
/// functions, etc. to find dynamic imports and require() calls.
/// Returns (specifier, is_type_only, is_dynamic).
fn extract_all_specifiers(file: &tsc_rs_ast::SourceFile) -> Vec<(String, bool, bool)> {
    let mut specifiers = Vec::new();
    for stmt in &file.statements {
        extract_from_stmt(stmt, &mut specifiers);
    }
    specifiers
}

/// Public wrapper for statement extraction (used by opaque_imports scanner).
pub fn extract_from_stmt_public(stmt: &Stmt, out: &mut Vec<(String, bool, bool)>) {
    extract_from_stmt(stmt, out);
}

/// Walk a statement, extracting import specifiers.
fn extract_from_stmt(stmt: &Stmt, out: &mut Vec<(String, bool, bool)>) {
    match &stmt.kind {
        // import ... from "specifier"
        // import "specifier"
        StmtKind::Import(decl) => {
            out.push((decl.source.clone(), decl.type_only, false));
        }

        // export { ... } from "specifier"
        // export * from "specifier"
        // export default <expr>
        StmtKind::Export(decl) => match &decl.kind {
            ExportDeclKind::Named {
                source: Some(src),
                type_only,
                ..
            } => {
                out.push((src.clone(), *type_only, false));
            }
            ExportDeclKind::All {
                source, type_only, ..
            } => {
                out.push((source.clone(), *type_only, false));
            }
            ExportDeclKind::Default(expr) => {
                extract_from_expr(expr, out);
            }
            ExportDeclKind::Decl(inner_stmt) => {
                extract_from_stmt(inner_stmt, out);
            }
            ExportDeclKind::DefaultDecl(inner_stmt) => {
                extract_from_stmt(inner_stmt, out);
            }
            _ => {}
        },

        // Expression statements: might contain import() or require()
        StmtKind::Expr(expr) => {
            extract_from_expr(expr, out);
        }

        // Variable declarations: const x = require("mod")
        StmtKind::Var(var_stmt) => {
            for decl in &var_stmt.declarations {
                if let Some(init) = &decl.init {
                    extract_from_expr(init, out);
                }
            }
        }

        // Function declaration: walk body
        StmtKind::FnDecl(fn_decl) => {
            if let Some(body) = &fn_decl.body {
                for s in body {
                    extract_from_stmt(s, out);
                }
            }
        }

        // Class declaration: walk method bodies
        StmtKind::ClassDecl(class_decl) => {
            extract_from_class_members(&class_decl.members, out);
        }

        // Block-like statements
        StmtKind::If(if_stmt) => {
            extract_from_stmt(&if_stmt.consequent, out);
            if let Some(alt) = &if_stmt.alternate {
                extract_from_stmt(alt, out);
            }
        }
        StmtKind::While(w) => {
            extract_from_stmt(&w.body, out);
        }
        StmtKind::DoWhile(dw) => {
            extract_from_stmt(&dw.body, out);
        }
        StmtKind::For(f) => {
            extract_from_stmt(&f.body, out);
        }
        StmtKind::ForIn(fi) => {
            extract_from_stmt(&fi.body, out);
        }
        StmtKind::ForOf(fo) => {
            extract_from_stmt(&fo.body, out);
        }
        StmtKind::Block(stmts) => {
            for s in stmts {
                extract_from_stmt(s, out);
            }
        }
        StmtKind::Switch(sw) => {
            for case in &sw.cases {
                for s in &case.consequent {
                    extract_from_stmt(s, out);
                }
            }
        }
        StmtKind::Try(try_stmt) => {
            for s in &try_stmt.block {
                extract_from_stmt(s, out);
            }
            if let Some(catch) = &try_stmt.handler {
                for s in &catch.body {
                    extract_from_stmt(s, out);
                }
            }
            if let Some(fin) = &try_stmt.finalizer {
                for s in fin {
                    extract_from_stmt(s, out);
                }
            }
        }
        StmtKind::Return(Some(expr)) => {
            extract_from_expr(expr, out);
        }
        StmtKind::Labeled(labeled) => {
            extract_from_stmt(&labeled.body, out);
        }
        StmtKind::With(with_stmt) => {
            extract_from_stmt(&with_stmt.body, out);
        }
        StmtKind::ExportAssign(expr) => {
            extract_from_expr(expr, out);
        }
        StmtKind::ModuleDecl(module_decl) => {
            if let Some(body) = &module_decl.body {
                match body {
                    ModuleBody::Block(stmts) => {
                        for s in stmts {
                            extract_from_stmt(s, out);
                        }
                    }
                    ModuleBody::Module(inner) => {
                        // Nested module declaration
                        let synth = Stmt {
                            kind: StmtKind::ModuleDecl(Box::new((**inner).clone())),
                            span: inner.span,
                        };
                        extract_from_stmt(&synth, out);
                    }
                }
            }
        }

        _ => {}
    }
}

/// Walk an expression tree looking for `import("...")` and `require("...")`.
fn extract_from_expr(expr: &Expr, out: &mut Vec<(String, bool, bool)>) {
    match &expr.kind {
        ExprKind::Call(call) => {
            if let ExprKind::Ident(name) = &call.callee.kind {
                if call.args.len() >= 1 {
                    if let ExprKind::StrLit(s) = &call.args[0].kind {
                        match name.as_str() {
                            "require" => out.push((s.to_string(), false, false)),
                            "import" => out.push((s.to_string(), false, true)),
                            _ => {}
                        }
                    }
                }
            }
            // Recurse into callee and args
            extract_from_expr(&call.callee, out);
            for arg in &call.args {
                extract_from_expr(arg, out);
            }
        }

        ExprKind::Await(inner)
        | ExprKind::Paren(inner)
        | ExprKind::Spread(inner)
        | ExprKind::NonNull(inner)
        | ExprKind::Delete(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner) => {
            extract_from_expr(inner, out);
        }

        ExprKind::Arrow(arrow) => match &arrow.body {
            ArrowBody::Expr(body) => extract_from_expr(body, out),
            ArrowBody::Block(stmts) => {
                for s in stmts {
                    extract_from_stmt(s, out);
                }
            }
        },

        ExprKind::FnExpr(fn_decl) => {
            if let Some(body) = &fn_decl.body {
                for s in body {
                    extract_from_stmt(s, out);
                }
            }
        }

        ExprKind::ClassExpr(class_decl) => {
            extract_from_class_members(&class_decl.members, out);
        }

        ExprKind::Binary(bin) => {
            extract_from_expr(&bin.left, out);
            extract_from_expr(&bin.right, out);
        }

        ExprKind::Assign(assign) => {
            extract_from_expr(&assign.right, out);
        }

        ExprKind::Cond(cond) => {
            extract_from_expr(&cond.test, out);
            extract_from_expr(&cond.consequent, out);
            extract_from_expr(&cond.alternate, out);
        }

        ExprKind::ArrayLit(items) => {
            for item in items.iter().flatten() {
                extract_from_expr(item, out);
            }
        }

        ExprKind::ObjectLit(props) => {
            for prop in props {
                match prop {
                    ObjLitProp::Property(p) => extract_from_expr(&p.value, out),
                    ObjLitProp::Spread(expr, _) => extract_from_expr(expr, out),
                    ObjLitProp::Method(m) => {
                        for s in &m.body {
                            extract_from_stmt(s, out);
                        }
                    }
                    ObjLitProp::Get(g) => {
                        for s in &g.body {
                            extract_from_stmt(s, out);
                        }
                    }
                    ObjLitProp::Set(st) => {
                        for s in &st.body {
                            extract_from_stmt(s, out);
                        }
                    }
                    ObjLitProp::ShorthandDefault(_, expr, _) => {
                        extract_from_expr(expr, out);
                    }
                    _ => {}
                }
            }
        }

        ExprKind::Template(tpl) => {
            for expr in &tpl.exprs {
                extract_from_expr(expr, out);
            }
        }

        ExprKind::TaggedTemplate(tt) => {
            extract_from_expr(&tt.tag, out);
            for expr in &tt.quasi.exprs {
                extract_from_expr(expr, out);
            }
        }

        ExprKind::Member(m) => {
            extract_from_expr(&m.object, out);
        }

        ExprKind::ElemAccess(ea) => {
            extract_from_expr(&ea.object, out);
            extract_from_expr(&ea.index, out);
        }

        ExprKind::New(n) => {
            extract_from_expr(&n.callee, out);
            if let Some(args) = &n.args {
                for arg in args {
                    extract_from_expr(arg, out);
                }
            }
        }

        ExprKind::Unary(u) => {
            extract_from_expr(&u.argument, out);
        }

        ExprKind::Update(u) => {
            extract_from_expr(&u.argument, out);
        }

        ExprKind::As(as_expr) => {
            extract_from_expr(&as_expr.expr, out);
        }

        ExprKind::Satisfies(sat) => {
            extract_from_expr(&sat.expr, out);
        }

        ExprKind::TypeAssertion(ta) => {
            extract_from_expr(&ta.expr, out);
        }

        ExprKind::Yield(_, Some(inner)) => {
            extract_from_expr(inner, out);
        }

        _ => {}
    }
}

/// Walk class members for import extraction.
fn extract_from_class_members(
    members: &[tsc_rs_ast::ClassMember],
    out: &mut Vec<(String, bool, bool)>,
) {
    for member in members {
        match &member.kind {
            ClassMemberKind::Method(m) => {
                if let Some(body) = &m.body {
                    for s in body {
                        extract_from_stmt(s, out);
                    }
                }
            }
            ClassMemberKind::Constructor(c) => {
                if let Some(body) = &c.body {
                    for s in body {
                        extract_from_stmt(s, out);
                    }
                }
            }
            ClassMemberKind::Property(p) => {
                if let Some(init) = &p.initializer {
                    extract_from_expr(init, out);
                }
            }
            ClassMemberKind::StaticBlock(stmts) => {
                for s in stmts {
                    extract_from_stmt(s, out);
                }
            }
            ClassMemberKind::GetAccessor(g) | ClassMemberKind::SetAccessor(g) => {
                if let Some(body) = &g.body {
                    for s in body {
                        extract_from_stmt(s, out);
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_relative(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

/// Extract the bare package name from a specifier.
/// `@scope/pkg/sub/path` → `@scope/pkg`
/// `lodash/fp` → `lodash`
/// `lodash` → `lodash`
fn extract_package_name(specifier: &str) -> &str {
    if specifier.starts_with('@') {
        // Scoped: @scope/name[/subpath]
        if let Some(first_slash) = specifier.find('/') {
            if let Some(second_slash) = specifier[first_slash + 1..]
                .find('/')
                .map(|i| i + first_slash + 1)
            {
                &specifier[..second_slash]
            } else {
                specifier
            }
        } else {
            specifier
        }
    } else if let Some(slash) = specifier.find('/') {
        &specifier[..slash]
    } else {
        specifier
    }
}

/// Check if a specifier is a project-internal path alias, not an npm package.
/// Project aliases use short prefixes like `@/`, `~/`, or bare `@` without
/// a proper scope name. Real npm scoped packages have `@org-name/pkg`.
fn is_project_alias(specifier: &str) -> bool {
    // ~/anything is always a project alias
    if specifier.starts_with("~/") {
        return true;
    }

    // @/anything — single-char scope is always a project alias
    if specifier.starts_with("@/") {
        return true;
    }

    // Bare `@` without a slash is not a valid package
    if specifier == "@" || (specifier.starts_with('@') && !specifier.contains('/')) {
        return true;
    }

    false
}

/// Check if a specifier is a Node.js builtin module.
fn is_node_builtin(specifier: &str) -> bool {
    if specifier.starts_with("node:") {
        return true;
    }
    matches!(
        specifier,
        "fs" | "path"
            | "os"
            | "util"
            | "events"
            | "stream"
            | "http"
            | "https"
            | "crypto"
            | "child_process"
            | "net"
            | "tls"
            | "dns"
            | "dgram"
            | "url"
            | "querystring"
            | "assert"
            | "buffer"
            | "cluster"
            | "console"
            | "domain"
            | "module"
            | "process"
            | "punycode"
            | "readline"
            | "repl"
            | "string_decoder"
            | "sys"
            | "timers"
            | "tty"
            | "v8"
            | "vm"
            | "worker_threads"
            | "zlib"
            | "perf_hooks"
            | "async_hooks"
            | "inspector"
            | "trace_events"
            | "wasi"
    )
}

fn should_follow(path: &str, skip_node_modules: bool) -> bool {
    if skip_node_modules && path.contains("/node_modules/") {
        return false;
    }
    // Only follow TypeScript/JavaScript files
    let p = Path::new(path);
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mts" | "mjs" | "cts" | "cjs")
    )
}

fn normalize_to_absolute(path: &str) -> String {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p.to_string_lossy().to_string()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&p))
            .unwrap_or(p)
            .to_string_lossy()
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// File discovery (standalone, without tsconfig)
// ---------------------------------------------------------------------------

/// Recursively discover all .ts/.tsx files under a directory.
pub fn discover_ts_files(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    discover_recursive(dir, &mut files);
    files.sort();
    files
}

fn discover_recursive(dir: &Path, files: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip common non-source directories
        if path.is_dir() {
            if matches!(
                name_str.as_ref(),
                "node_modules" | ".next" | "dist" | ".git" | "__pycache__" | ".turbo" | ".bext"
            ) {
                continue;
            }
            discover_recursive(&path, files);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ts" | "tsx")
        ) {
            // Skip .d.ts files
            if !name_str.ends_with(".d.ts") {
                files.push(path.to_string_lossy().to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name() {
        assert_eq!(extract_package_name("lodash"), "lodash");
        assert_eq!(extract_package_name("lodash/fp"), "lodash");
        assert_eq!(extract_package_name("@scope/pkg"), "@scope/pkg");
        assert_eq!(extract_package_name("@scope/pkg/sub/path"), "@scope/pkg");
        assert_eq!(extract_package_name("@scope"), "@scope");
    }

    #[test]
    fn test_is_relative() {
        assert!(is_relative("./foo"));
        assert!(is_relative("../bar"));
        assert!(!is_relative("lodash"));
        assert!(!is_relative("@scope/pkg"));
    }

    #[test]
    fn test_is_project_alias() {
        assert!(is_project_alias("@/components/foo"));
        assert!(is_project_alias("@/lib/utils"));
        assert!(is_project_alias("~/src/foo"));
        assert!(is_project_alias("@")); // bare @
        assert!(!is_project_alias("@acme/db"));
        assert!(!is_project_alias("@aws-sdk/client-s3"));
        assert!(!is_project_alias("lodash"));
        assert!(!is_project_alias("react"));
    }

    #[test]
    fn test_is_node_builtin() {
        assert!(is_node_builtin("fs"));
        assert!(is_node_builtin("path"));
        assert!(is_node_builtin("node:crypto"));
        assert!(!is_node_builtin("lodash"));
        assert!(!is_node_builtin("@scope/pkg"));
    }
}
