//! Binder/symbol table layer for declaration-merging compatibility.
//!
//! The binder walks the AST and creates a symbol table, mapping identifiers
//! to their declarations. This is used by the type checker for name resolution
//! and by the baseline generator for .symbols output.

use std::collections::HashMap;
use tsc_rs_ast::*;

mod augmentation;
mod baseline;
mod linking;

pub use augmentation::*;
pub use baseline::*;
pub use linking::*;

// ---------------------------------------------------------------------------
// Symbol flags
// ---------------------------------------------------------------------------

pub type SymbolFlags = u32;
pub const SYM_NONE: u32 = 0;
pub const SYM_FUNCTION: u32 = 1;
pub const SYM_VARIABLE: u32 = 1 << 1;
pub const SYM_CLASS: u32 = 1 << 2;
pub const SYM_INTERFACE: u32 = 1 << 3;
pub const SYM_TYPE_ALIAS: u32 = 1 << 4;
pub const SYM_ENUM: u32 = 1 << 5;
pub const SYM_ENUM_MEMBER: u32 = 1 << 6;
pub const SYM_MODULE: u32 = 1 << 7;
pub const SYM_PROPERTY: u32 = 1 << 8;
pub const SYM_METHOD: u32 = 1 << 9;
pub const SYM_CONSTRUCTOR: u32 = 1 << 10;
pub const SYM_GET_ACCESSOR: u32 = 1 << 11;
pub const SYM_SET_ACCESSOR: u32 = 1 << 12;
pub const SYM_TYPE_PARAMETER: u32 = 1 << 13;
pub const SYM_PARAMETER: u32 = 1 << 14;
pub const SYM_EXPORT: u32 = 1 << 15;
pub const SYM_BLOCK_SCOPED: u32 = 1 << 16;
pub const SYM_MODULE_AUGMENTATION: u32 = 1 << 17;
pub const SYM_GLOBAL_AUGMENTATION: u32 = 1 << 18;
pub const SYM_IMPORT: u32 = 1 << 19;
pub const SYM_CONST: u32 = 1 << 20;
pub const SYM_CATCH_VARIABLE: u32 = 1 << 21;
pub const SYM_USING: u32 = 1 << 22;
pub const SYM_AWAIT_USING: u32 = 1 << 23;
pub const SYM_AUTO_ACCESSOR: u32 = 1 << 24;
pub const SYM_STATIC: u32 = 1 << 25;
/// Set on `declare module "specifier"` symbols — their names live in the
/// module-specifier namespace, not the value/type ident space.
pub const SYM_STRING_MODULE: u32 = 1 << 26;

/// Returns true if existing_flags and new_flags can be merged (no error).
fn can_merge(existing: SymbolFlags, incoming: SymbolFlags) -> bool {
    // var + var is always OK (hoisting)
    if existing & SYM_VARIABLE != 0
        && incoming & SYM_VARIABLE != 0
        && existing & SYM_BLOCK_SCOPED == 0
        && incoming & SYM_BLOCK_SCOPED == 0
    {
        return true;
    }

    // parameter + var (var declarations can shadow parameters)
    if (existing & SYM_PARAMETER != 0
        && incoming & SYM_VARIABLE != 0
        && incoming & SYM_BLOCK_SCOPED == 0)
        || (existing & SYM_VARIABLE != 0
            && existing & SYM_BLOCK_SCOPED == 0
            && incoming & SYM_PARAMETER != 0)
    {
        return true;
    }

    // interface + interface
    if existing & SYM_INTERFACE != 0 && incoming & SYM_INTERFACE != 0 {
        return true;
    }

    // namespace + namespace
    if existing & SYM_MODULE != 0 && incoming & SYM_MODULE != 0 {
        return true;
    }

    // namespace + function, namespace + class, function + namespace, class + namespace
    let ns_value = |f: u32| f & SYM_MODULE != 0;
    let value_kind = |f: u32| f & (SYM_FUNCTION | SYM_CLASS) != 0;
    if (ns_value(existing) && value_kind(incoming)) || (value_kind(existing) && ns_value(incoming))
    {
        return true;
    }

    // function + function (overloads)
    if existing & SYM_FUNCTION != 0 && incoming & SYM_FUNCTION != 0 {
        return true;
    }

    // method + method (overloads in interfaces/classes)
    if existing & SYM_METHOD != 0 && incoming & SYM_METHOD != 0 {
        return true;
    }

    // function + class (ambient class/function merging for callable classes)
    if (existing & SYM_FUNCTION != 0 && incoming & SYM_CLASS != 0)
        || (existing & SYM_CLASS != 0 && incoming & SYM_FUNCTION != 0)
    {
        return true;
    }

    // enum + enum
    if existing & SYM_ENUM != 0 && incoming & SYM_ENUM != 0 {
        return true;
    }

    // class + interface (TypeScript allows this for declaration merging)
    if (existing & SYM_CLASS != 0 && incoming & SYM_INTERFACE != 0)
        || (existing & SYM_INTERFACE != 0 && incoming & SYM_CLASS != 0)
    {
        return true;
    }

    // type alias + function — separate namespaces (type Meeting + function Meeting)
    if (existing & SYM_TYPE_ALIAS != 0 && incoming & SYM_FUNCTION != 0)
        || (existing & SYM_FUNCTION != 0 && incoming & SYM_TYPE_ALIAS != 0)
    {
        return true;
    }

    // interface/type alias + value (function/variable) — separate namespaces
    // e.g., `interface LinkItem { ... }` + `const LinkItem: React.FC<LinkItem> = ...`
    let is_type_ns = |f: u32| f & (SYM_INTERFACE | SYM_TYPE_ALIAS) != 0;
    let is_value_ns = |f: u32| f & (SYM_FUNCTION | SYM_VARIABLE) != 0;
    if (is_type_ns(existing) && is_value_ns(incoming))
        || (is_value_ns(existing) && is_type_ns(incoming))
    {
        return true;
    }

    // type parameter vs member/other declarations — separate spaces in tsc
    // (`interface I<TChange> { readonly TChange: TChange }` is legal).
    if existing & SYM_TYPE_PARAMETER != 0 || incoming & SYM_TYPE_PARAMETER != 0 {
        return true;
    }

    // string-named ambient module (`declare module "ext"`) + an import
    // binding of the same text (`import ext = require("ext")`) — separate
    // namespaces in tsc, never a duplicate. Ident-named namespaces still
    // interact with imports normally (aliasMergingWithNamespace).
    if (existing & SYM_STRING_MODULE != 0 && incoming & SYM_IMPORT != 0)
        || (existing & SYM_IMPORT != 0 && incoming & SYM_STRING_MODULE != 0)
    {
        return true;
    }

    // namespace + var — legal for non-instantiated namespaces; the checker
    // (check_namespace_var_merges) reports the instantiated case, so the
    // binder must not.
    if (existing & SYM_MODULE != 0
        && incoming & SYM_VARIABLE != 0
        && incoming & (SYM_BLOCK_SCOPED | SYM_IMPORT) == 0)
        || (existing & SYM_VARIABLE != 0
            && existing & (SYM_BLOCK_SCOPED | SYM_IMPORT) == 0
            && incoming & SYM_MODULE != 0)
    {
        return true;
    }

    // enum + namespace
    if (existing & SYM_ENUM != 0 && incoming & SYM_MODULE != 0)
        || (existing & SYM_MODULE != 0 && incoming & SYM_ENUM != 0)
    {
        return true;
    }

    // namespace + interface (TypeScript allows namespace/interface merging)
    if (existing & SYM_MODULE != 0 && incoming & SYM_INTERFACE != 0)
        || (existing & SYM_INTERFACE != 0 && incoming & SYM_MODULE != 0)
    {
        return true;
    }

    // type alias + class (separate type/value namespaces)
    if (existing & SYM_TYPE_ALIAS != 0 && incoming & SYM_CLASS != 0)
        || (existing & SYM_CLASS != 0 && incoming & SYM_TYPE_ALIAS != 0)
    {
        return true;
    }

    // type alias + namespace (separate namespaces)
    if (existing & SYM_TYPE_ALIAS != 0 && incoming & SYM_MODULE != 0)
        || (existing & SYM_MODULE != 0 && incoming & SYM_TYPE_ALIAS != 0)
    {
        return true;
    }

    // type alias + enum (separate type/value namespaces)
    if (existing & SYM_TYPE_ALIAS != 0 && incoming & SYM_ENUM != 0)
        || (existing & SYM_ENUM != 0 && incoming & SYM_TYPE_ALIAS != 0)
    {
        return true;
    }

    // accessor pairs and accessor + accessor (static vs instance share scope)
    let is_accessor = |f: u32| f & (SYM_GET_ACCESSOR | SYM_SET_ACCESSOR) != 0;
    if is_accessor(existing) && is_accessor(incoming) {
        return true;
    }

    // method + method (class method overloads; also handles static/instance sharing scope)
    if existing & SYM_METHOD != 0 && incoming & SYM_METHOD != 0 {
        return true;
    }

    // property + property (static vs instance share scope in binder)
    if existing & SYM_PROPERTY != 0 && incoming & SYM_PROPERTY != 0 {
        return true;
    }

    // property + accessor and accessor + property (e.g. abstract prop then get/set)
    if (existing & SYM_PROPERTY != 0 && is_accessor(incoming))
        || (is_accessor(existing) && incoming & SYM_PROPERTY != 0)
    {
        return true;
    }

    // property + method and method + property (overriding in subclass shares scope)
    if (existing & SYM_PROPERTY != 0 && incoming & SYM_METHOD != 0)
        || (existing & SYM_METHOD != 0 && incoming & SYM_PROPERTY != 0)
    {
        return true;
    }

    // constructor + constructor (constructor overloads)
    if existing & SYM_CONSTRUCTOR != 0 && incoming & SYM_CONSTRUCTOR != 0 {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// Symbol & Declaration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub flags: SymbolFlags,
    pub declarations: Vec<Declaration>,
    pub members: rustc_hash::FxHashMap<String, SymbolId>,
    pub exports: rustc_hash::FxHashMap<String, SymbolId>,
}

#[derive(Debug, Clone)]
pub struct Declaration {
    pub file_name: String,
    /// Full-start offset of the declaration node (`Node.pos` in TypeScript).
    /// `span` remains the name/token range used by navigation and diagnostics.
    pub full_start: u32,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Binding diagnostic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindDiagnostic {
    pub message: String,
    pub file_name: String,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Symbol table
// ---------------------------------------------------------------------------

/// Information about a single overload signature (function/method/constructor
/// declaration without a body).
#[derive(Debug, Clone)]
pub struct OverloadSignature {
    pub params: Vec<OverloadParam>,
    pub return_type: Option<OverloadTypeRef>,
    pub type_params: Vec<String>,
    pub type_param_defaults: Vec<Option<OverloadTypeRef>>,
}

/// Minimal parameter representation stored during binding.
#[derive(Debug, Clone)]
pub struct OverloadParam {
    pub name: String,
    /// Index into the source file's type annotations; `None` means untyped.
    pub type_ann: Option<OverloadTypeRef>,
    pub optional: bool,
    pub rest: bool,
}

/// A lightweight reference to a type-annotation AST node stored during binding.
/// We store the raw span so the type-checker can relocate the `TypeNode` in the
/// AST, but for practical purposes we also stash the byte-range so the checker
/// can index into the source and re-parse if needed.  In practice the type
/// checker will re-resolve the `TypeNode` directly; we keep the `Span` as a
/// key.
///
/// Because we want the symbols crate to stay AST-independent at the value level
/// we store a *cloned* `TypeNode` here (it is `Clone`).
pub type OverloadTypeRef = tsc_rs_ast::TypeNode;

pub type ScopeId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Global,
    Function,
    Class,
    Interface,
    TypeAlias,
    Enum,
    Block,
    Loop,
    Catch,
    Module,
    StaticBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Value,
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeInfo {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,
    pub span: Span,
    pub bindings: rustc_hash::FxHashMap<String, SymbolId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolReference {
    pub span: Span,
    pub symbol_id: SymbolId,
    pub scope_id: ScopeId,
    pub kind: ReferenceKind,
}

#[derive(Debug, Clone, Default)]
pub struct ScopeGraph {
    pub scopes: Vec<ScopeInfo>,
    pub symbol_scopes: rustc_hash::FxHashMap<SymbolId, ScopeId>,
    pub references: Vec<SymbolReference>,
}

impl ScopeGraph {
    pub fn scope(&self, id: ScopeId) -> Option<&ScopeInfo> {
        self.scopes.get(id)
    }

    pub fn binding_scope(&self, symbol_id: SymbolId) -> Option<ScopeId> {
        self.symbol_scopes.get(&symbol_id).copied()
    }

    pub fn innermost_scope_at(&self, offset: u32) -> Option<ScopeId> {
        self.scopes
            .iter()
            .filter(|scope| scope.span.start <= offset && offset < scope.span.end)
            .min_by_key(|scope| scope.span.len())
            .map(|scope| scope.id)
    }

    pub fn reference_at(&self, offset: u32) -> Option<&SymbolReference> {
        self.references
            .iter()
            .find(|reference| reference.span.start <= offset && offset < reference.span.end)
    }

    pub fn references_for_symbol(
        &self,
        symbol_id: SymbolId,
    ) -> impl Iterator<Item = &SymbolReference> + '_ {
        self.references
            .iter()
            .filter(move |reference| reference.symbol_id == symbol_id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    /// Map from source position to symbol ID for .symbols baseline generation
    pub position_to_symbol: rustc_hash::FxHashMap<u32, SymbolId>,
    pub diagnostics: Vec<BindDiagnostic>,
    /// Overload signatures for merged function/method/constructor symbols.
    /// Each entry maps a `SymbolId` to its ordered list of overload signatures
    /// (declarations that have no body).
    pub overload_signatures: rustc_hash::FxHashMap<SymbolId, Vec<OverloadSignature>>,
    /// Module augmentations: maps target module path (e.g. `"./path"` or
    /// `"package-name"`) to the symbol IDs of the augmentation modules.
    /// These are created by `declare module './path' { ... }` when the
    /// declaration appears inside a module (augmentation) rather than at
    /// the top level of a .d.ts file (ambient module declaration).
    pub module_augmentations: rustc_hash::FxHashMap<String, Vec<SymbolId>>,
    /// Ambient module declarations: maps module specifier to symbol ID.
    /// Created by `declare module 'package-name' { ... }` in .d.ts files.
    pub ambient_modules: rustc_hash::FxHashMap<String, SymbolId>,
    /// Wildcard module patterns: `declare module '*.css' { ... }` etc.
    /// Stores (pattern, symbol_id) pairs where pattern contains a `*`.
    pub wildcard_modules: Vec<(String, SymbolId)>,
    /// Symbols that were contributed to the global scope via
    /// `declare global { ... }` blocks.
    pub global_augmentation_symbols: Vec<SymbolId>,
    /// Lexical scope graph and identifier reference metadata captured by the binder.
    pub scope_graph: ScopeGraph,
    /// Interface/class extends relationships: maps type name to parent type names.
    /// Populated by the binder from `extends` clauses.
    pub extends_map: rustc_hash::FxHashMap<String, Vec<String>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_symbol(&mut self, name: String, flags: SymbolFlags) -> SymbolId {
        let id = self.symbols.len() as SymbolId;
        self.symbols.push(Symbol {
            id,
            name,
            flags,
            declarations: Vec::new(),
            members: rustc_hash::FxHashMap::default(),
            exports: rustc_hash::FxHashMap::default(),
        });
        id
    }

    pub fn get_symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id as usize)
    }

    pub fn get_symbol_mut(&mut self, id: SymbolId) -> Option<&mut Symbol> {
        self.symbols.get_mut(id as usize)
    }

    /// Retrieve all overload signatures for a given symbol (empty if none).
    pub fn get_overload_signatures(&self, id: SymbolId) -> &[OverloadSignature] {
        self.overload_signatures
            .get(&id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn scope_graph(&self) -> &ScopeGraph {
        &self.scope_graph
    }
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Scope {
    symbols: rustc_hash::FxHashMap<String, SymbolId>,
    parent: Option<ScopeId>,
    kind: ScopeKind,
    span: Span,
}

// ---------------------------------------------------------------------------
// Binder
// ---------------------------------------------------------------------------

pub struct Binder {
    table: SymbolTable,
    scopes: Vec<Scope>,
    current_scope: usize,
    /// Tracks whether we are inside an export statement
    in_export: bool,
    /// Leading trivia is part of a declaration's full start, but not its
    /// navigation span. Keys are the first byte after a trivia run.
    full_starts: rustc_hash::FxHashMap<u32, u32>,
    /// Resolve after collecting declarations so forward references and later
    /// declarations shadowing outer names use the complete lexical scope.
    pending_references: Vec<(String, Span, ScopeId, ReferenceKind)>,
}

impl Default for Binder {
    fn default() -> Self {
        Self::new()
    }
}

impl Binder {
    pub fn new() -> Self {
        let global_scope = Scope {
            symbols: rustc_hash::FxHashMap::default(),
            parent: None,
            kind: ScopeKind::Global,
            span: Span::default(),
        };
        Self {
            table: SymbolTable::new(),
            scopes: vec![global_scope],
            current_scope: 0,
            in_export: false,
            full_starts: rustc_hash::FxHashMap::default(),
            pending_references: Vec::new(),
        }
    }

    pub fn bind(mut self, file: &SourceFile) -> SymbolTable {
        let mut offset = 0usize;
        let mut comments = file.comments.iter().peekable();
        while offset < file.text.len() {
            let start = offset;
            loop {
                while comments
                    .peek()
                    .is_some_and(|comment| comment.end as usize <= offset)
                {
                    comments.next();
                }
                if let Some(comment) = comments
                    .peek()
                    .filter(|comment| comment.pos as usize == offset)
                {
                    offset = comment.end as usize;
                    comments.next();
                    continue;
                }
                let Some(ch) = file.text.get(offset..).and_then(|tail| tail.chars().next()) else {
                    break;
                };
                if (ch.is_whitespace() && ch != '\u{0085}') || ch == '\u{feff}' {
                    offset += ch.len_utf8();
                } else {
                    break;
                }
            }
            if offset > start {
                self.full_starts.insert(offset as u32, start as u32);
            }
            if let Some(ch) = file.text.get(offset..).and_then(|tail| tail.chars().next()) {
                offset += ch.len_utf8();
            }
        }
        self.scopes[0].span = file.span;
        for stmt in &file.statements {
            self.bind_stmt(stmt, &file.file_name);
        }
        for (name, span, scope, kind) in std::mem::take(&mut self.pending_references) {
            if let Some(symbol_id) = self.resolve_reference(&name, scope) {
                self.table.position_to_symbol.insert(span.start, symbol_id);
                self.table.scope_graph.references.push(SymbolReference {
                    span,
                    symbol_id,
                    scope_id: scope,
                    kind,
                });
            }
        }
        self.table.scope_graph.scopes = self
            .scopes
            .iter()
            .enumerate()
            .map(|(id, scope)| ScopeInfo {
                id,
                parent: scope.parent,
                kind: scope.kind,
                span: scope.span,
                bindings: scope.symbols.clone(),
            })
            .collect();
        self.table
    }

    fn push_scope(&mut self, kind: ScopeKind, span: Span) -> ScopeId {
        let idx = self.scopes.len();
        self.scopes.push(Scope {
            symbols: rustc_hash::FxHashMap::default(),
            parent: Some(self.current_scope),
            kind,
            span,
        });
        self.current_scope = idx;
        idx
    }

    fn pop_scope(&mut self) {
        if let Some(parent) = self.scopes[self.current_scope].parent {
            self.current_scope = parent;
        }
    }

    /// Declare a symbol in the current scope with declaration merging and
    /// duplicate detection. Returns the symbol ID (either new or existing).
    fn declare(&mut self, name: &str, flags: SymbolFlags, file_name: &str, span: Span) -> SymbolId {
        self.declare_node(name, flags, file_name, span, span.start)
    }

    fn full_start(&self, start: u32) -> u32 {
        self.full_starts.get(&start).copied().unwrap_or(start)
    }

    fn declare_node(
        &mut self,
        name: &str,
        flags: SymbolFlags,
        file_name: &str,
        name_span: Span,
        start: u32,
    ) -> SymbolId {
        self.declare_with_full_start(name, flags, file_name, name_span, self.full_start(start))
    }

    fn bind_parameter(&mut self, parameter: &Param, file_name: &str) {
        if let PatKind::Ident(name) = &parameter.name.kind {
            self.declare_node(
                name,
                SYM_PARAMETER,
                file_name,
                parameter.name.span,
                parameter.span.start,
            );
        } else {
            self.bind_pattern(&parameter.name, SYM_PARAMETER, file_name);
        }
    }

    fn declare_with_full_start(
        &mut self,
        name: &str,
        flags: SymbolFlags,
        file_name: &str,
        span: Span,
        full_start: u32,
    ) -> SymbolId {
        let final_flags = if self.in_export {
            flags | SYM_EXPORT
        } else {
            flags
        };

        // Check if name already exists in the current scope
        if let Some(&existing_id) = self.scopes[self.current_scope].symbols.get(name) {
            let existing_flags = self.table.symbols[existing_id as usize].flags;

            if can_merge(existing_flags, final_flags) {
                // Merge: add declaration, merge flags
                self.table.symbols[existing_id as usize].flags |= final_flags;
                self.table.symbols[existing_id as usize]
                    .declarations
                    .push(Declaration {
                        file_name: file_name.to_string(),
                        full_start,
                        span,
                    });
                self.table
                    .position_to_symbol
                    .insert(span.start, existing_id);
                return existing_id;
            } else {
                // Duplicate declaration error — tsc reports at EVERY
                // declaration of the conflicting symbol, once each
                // (span-deduped across repeated conflicts).
                // tsc distinguishes BLOCK-SCOPED redeclaration (let/const/class)
                // from a plain duplicate identifier: TS2451 vs TS2300.
                // Only block-scoped VARIABLES (let/const) get TS2451 —
                // classes are block-scoped too but their redeclaration is a
                // plain TS2300 duplicate identifier.
                let is_bs_var = |f: u32| (f & SYM_BLOCK_SCOPED) != 0 && (f & SYM_VARIABLE) != 0;
                let block_scoped = is_bs_var(existing_flags) || is_bs_var(final_flags);
                // An IMPORT colliding with a local declaration is TS2440,
                // reported once at the IMPORT — not a redeclaration error at
                // both sites.
                let import_conflict =
                    ((existing_flags & SYM_IMPORT) != 0) != ((final_flags & SYM_IMPORT) != 0);
                // A `var` hoisted into a BLOCK that also block-scopes the same
                // name is TS2481 — the var's write would clobber the const.
                // Reported once, at the block-scoped declaration.
                let is_plain_var = |f: u32| (f & SYM_VARIABLE) != 0 && (f & SYM_BLOCK_SCOPED) == 0;
                let var_vs_block_scoped = matches!(
                    self.scopes[self.current_scope].kind,
                    ScopeKind::Block | ScopeKind::Catch
                ) && ((is_plain_var(existing_flags)
                    && is_bs_var(final_flags))
                    || (is_bs_var(existing_flags) && is_plain_var(final_flags)));
                let msg = if var_vs_block_scoped {
                    format!(
                        "Cannot initialize outer scoped variable '{}' in the same scope as block scoped declaration '{}'.",
                        name, name
                    )
                } else if import_conflict {
                    format!(
                        "Import declaration conflicts with local declaration of '{}'.",
                        name
                    )
                } else if block_scoped {
                    format!("Cannot redeclare block-scoped variable '{}'.", name)
                } else {
                    format!("Duplicate identifier '{}'.", name)
                };
                let mut sites: Vec<(std::string::String, Span)> = Vec::new();
                if var_vs_block_scoped {
                    // Anchor at the BLOCK-SCOPED declaration.
                    if is_bs_var(existing_flags) {
                        if let Some(d) = self.table.symbols[existing_id as usize]
                            .declarations
                            .first()
                        {
                            sites.push((d.file_name.clone(), d.span));
                        }
                    } else {
                        sites.push((file_name.to_string(), span));
                    }
                } else if import_conflict {
                    // Anchor at whichever declaration is the import.
                    if (existing_flags & SYM_IMPORT) != 0 {
                        if let Some(d) = self.table.symbols[existing_id as usize]
                            .declarations
                            .first()
                        {
                            sites.push((d.file_name.clone(), d.span));
                        }
                    } else {
                        sites.push((file_name.to_string(), span));
                    }
                } else {
                    sites.extend(
                        self.table.symbols[existing_id as usize]
                            .declarations
                            .iter()
                            .map(|d| (d.file_name.clone(), d.span)),
                    );
                    sites.push((file_name.to_string(), span));
                }
                for (f, sp) in sites {
                    let already =
                        self.table.diagnostics.iter().any(|d| {
                            d.span.start == sp.start && d.message == msg && d.file_name == f
                        });
                    if !already {
                        self.table.diagnostics.push(BindDiagnostic {
                            message: msg.clone(),
                            file_name: f,
                            span: sp,
                        });
                    }
                }
                // Still create a new symbol so binding can continue
            }
        }

        let id = self.table.add_symbol(name.to_string(), final_flags);
        self.table.symbols[id as usize]
            .declarations
            .push(Declaration {
                file_name: file_name.to_string(),
                full_start,
                span,
            });
        self.scopes[self.current_scope]
            .symbols
            .insert(name.to_string(), id);
        self.table.position_to_symbol.insert(span.start, id);
        self.table
            .scope_graph
            .symbol_scopes
            .entry(id)
            .or_insert(self.current_scope);
        id
    }

    fn record_reference(&mut self, name: &str, span: Span, kind: ReferenceKind) {
        self.pending_references
            .push((name.to_string(), span, self.current_scope, kind));
    }

    fn resolve_reference(&self, name: &str, mut scope: ScopeId) -> Option<SymbolId> {
        loop {
            let current = &self.scopes[scope];
            if let Some(&id) = current.symbols.get(name) {
                let flags = self.table.symbols[id as usize].flags;
                // Class and interface members require a receiver; their names
                // do not shadow lexical bindings in a method or annotation.
                // Type parameters in the same scope remain visible.
                let member = matches!(current.kind, ScopeKind::Class | ScopeKind::Interface)
                    && flags & (SYM_PROPERTY | SYM_METHOD | SYM_GET_ACCESSOR | SYM_SET_ACCESSOR)
                        != 0
                    && flags & SYM_TYPE_PARAMETER == 0;
                if !member {
                    return Some(id);
                }
            }
            scope = current.parent?;
        }
    }

    /// Public-facing resolve: walk scope chain from given scope index.
    pub fn resolve_in_scope(&self, name: &str, scope: usize) -> Option<SymbolId> {
        let mut scope_idx = scope;
        loop {
            if let Some(&id) = self.scopes[scope_idx].symbols.get(name) {
                return Some(id);
            }
            if let Some(parent) = self.scopes[scope_idx].parent {
                scope_idx = parent;
            } else {
                return None;
            }
        }
    }

    fn bind_stmt(&mut self, stmt: &Stmt, file_name: &str) {
        match &stmt.kind {
            StmtKind::Var(var_stmt) => {
                let flags = match var_stmt.kind {
                    VarKind::Var => SYM_VARIABLE,
                    VarKind::Const => SYM_VARIABLE | SYM_BLOCK_SCOPED | SYM_CONST,
                    VarKind::Let => SYM_VARIABLE | SYM_BLOCK_SCOPED,
                    VarKind::Using => SYM_VARIABLE | SYM_BLOCK_SCOPED | SYM_USING,
                    VarKind::AwaitUsing => SYM_VARIABLE | SYM_BLOCK_SCOPED | SYM_AWAIT_USING,
                };
                for decl in &var_stmt.declarations {
                    self.bind_variable_declaration(decl, flags, file_name);
                    if let Some(ref type_ann) = decl.type_ann {
                        self.bind_type_node(type_ann, file_name);
                    }
                    // Walk initializer expression for function/class expressions
                    if let Some(ref init) = decl.init {
                        self.bind_expr(init, file_name);
                    }
                }
            }
            StmtKind::FnDecl(fn_decl) => {
                if let Some(ref name) = fn_decl.name {
                    let decl_span = fn_decl.name_span.unwrap_or(fn_decl.span);
                    let sym_id = self.declare_node(
                        name,
                        SYM_FUNCTION,
                        file_name,
                        decl_span,
                        fn_decl.span.start,
                    );
                    // If the declaration has no body, it is an overload signature.
                    if fn_decl.body.is_none() {
                        self.record_overload_signature(sym_id, fn_decl);
                    }
                }
                self.push_scope(ScopeKind::Function, fn_decl.span);
                if let Some(ref type_params) = fn_decl.type_params {
                    self.declare_type_params(type_params, file_name);
                }
                for param in &fn_decl.params {
                    self.bind_parameter(param, file_name);
                    if let Some(ref type_ann) = param.type_ann {
                        self.bind_type_node(type_ann, file_name);
                    }
                }
                if let Some(ref return_type) = fn_decl.return_type {
                    self.bind_type_node(return_type, file_name);
                }
                if let Some(ref body) = fn_decl.body {
                    for s in body {
                        self.bind_stmt(s, file_name);
                    }
                }
                self.pop_scope();
            }
            StmtKind::ClassDecl(class_decl) => {
                if let Some(ref name) = class_decl.name {
                    let decl_span = class_decl.name_span.unwrap_or(class_decl.span);
                    let class_sym = self.declare_node(
                        name,
                        SYM_CLASS,
                        file_name,
                        decl_span,
                        class_decl.span.start,
                    );
                    self.push_scope(ScopeKind::Class, class_decl.span);
                    if let Some(ref type_params) = class_decl.type_params {
                        self.declare_type_params(type_params, file_name);
                    }
                    // Bind extends expression (the base class identifier) and record in extends_map
                    if let Some(ref extends) = class_decl.extends {
                        self.bind_expr(extends, file_name);
                        if let ExprKind::Ident(ref parent_name) = extends.kind {
                            self.table
                                .extends_map
                                .entry(name.clone())
                                .or_default()
                                .push(parent_name.to_string());
                        }
                    }
                    if let Some(ref ext_args) = class_decl.extends_type_args {
                        for arg in ext_args {
                            self.bind_type_node(arg, file_name);
                        }
                    }
                    // Bind implements clauses (type references)
                    for impl_type in &class_decl.implements {
                        self.bind_type_node(impl_type, file_name);
                    }
                    for member in &class_decl.members {
                        self.bind_class_member(member, class_sym, file_name);
                    }
                    self.pop_scope();
                }
            }
            StmtKind::InterfaceDecl(iface) => {
                let decl_span = iface.name_span.unwrap_or(iface.span);
                let iface_sym = self.declare_node(
                    &iface.name,
                    SYM_INTERFACE,
                    file_name,
                    decl_span,
                    iface.span.start,
                );
                // Bind extends clauses (type references) and record in extends_map
                for ext_type in &iface.extends {
                    self.bind_type_node(ext_type, file_name);
                    // Extract parent type name from the type node
                    if let TypeNodeKind::Reference(ref type_ref) = ext_type.kind {
                        if let ExprKind::Ident(ref parent_name) = type_ref.name.kind {
                            self.table
                                .extends_map
                                .entry(iface.name.clone())
                                .or_default()
                                .push(parent_name.to_string());
                        }
                    }
                }
                // Bind interface members in a dedicated scope so that property
                // names from different interfaces don't conflict.
                self.push_scope(ScopeKind::Interface, iface.span);
                // Bind type parameters
                if let Some(ref type_params) = iface.type_params {
                    self.declare_type_params(type_params, file_name);
                }
                for member in &iface.members {
                    self.bind_type_member(member, iface_sym, file_name);
                }
                self.pop_scope();
            }
            StmtKind::TypeAlias(ta) => {
                let decl_span = ta.name_span.unwrap_or(ta.span);
                self.declare_node(
                    &ta.name,
                    SYM_TYPE_ALIAS,
                    file_name,
                    decl_span,
                    ta.span.start,
                );
                // Bind type parameters
                if let Some(ref type_params) = ta.type_params {
                    self.push_scope(ScopeKind::TypeAlias, ta.span);
                    self.declare_type_params(type_params, file_name);
                }
                self.bind_type_node(&ta.type_ann, file_name);
                if ta.type_params.is_some() {
                    self.pop_scope();
                }
            }
            StmtKind::EnumDecl(enum_decl) => {
                let decl_span = enum_decl.name_span.unwrap_or(enum_decl.span);
                let mut enum_flags = SYM_ENUM;
                if enum_decl.is_const {
                    enum_flags |= SYM_CONST;
                }
                let enum_sym = self.declare_node(
                    &enum_decl.name,
                    enum_flags,
                    file_name,
                    decl_span,
                    enum_decl.span.start,
                );
                self.push_scope(ScopeKind::Enum, enum_decl.span);
                for member in &enum_decl.members {
                    let name = match &member.name {
                        PropName::Ident(ref n, _) | PropName::String(ref n, _) => n,
                        _ => continue,
                    };
                    let mem_id = self.declare(name, SYM_ENUM_MEMBER, file_name, member.span);
                    self.table.symbols[enum_sym as usize]
                        .members
                        .insert(name.to_string(), mem_id);
                }
                self.pop_scope();
            }
            StmtKind::ModuleDecl(module_decl) => {
                self.bind_module_decl(module_decl, file_name);
            }
            StmtKind::Block(stmts) => {
                self.push_scope(ScopeKind::Block, stmt.span);
                for s in stmts {
                    self.bind_stmt(s, file_name);
                }
                self.pop_scope();
            }
            StmtKind::If(if_stmt) => {
                self.bind_expr(&if_stmt.test, file_name);
                self.bind_stmt(&if_stmt.consequent, file_name);
                if let Some(ref alt) = if_stmt.alternate {
                    self.bind_stmt(alt, file_name);
                }
            }
            StmtKind::While(while_stmt) => {
                self.bind_expr(&while_stmt.test, file_name);
                self.bind_stmt(&while_stmt.body, file_name);
            }
            StmtKind::DoWhile(dw) => {
                self.bind_stmt(&dw.body, file_name);
                self.bind_expr(&dw.test, file_name);
            }
            StmtKind::For(for_stmt) => {
                self.push_scope(ScopeKind::Loop, stmt.span);
                if let Some(ref init) = for_stmt.init {
                    match init {
                        ForInit::Var(vs) => {
                            let flags = match vs.kind {
                                VarKind::Var => SYM_VARIABLE,
                                _ => SYM_VARIABLE | SYM_BLOCK_SCOPED,
                            };
                            for decl in &vs.declarations {
                                self.bind_variable_declaration(decl, flags, file_name);
                                if let Some(ref init_expr) = decl.init {
                                    self.bind_expr(init_expr, file_name);
                                }
                            }
                        }
                        ForInit::Expr(e) => {
                            self.bind_expr(e, file_name);
                        }
                    }
                }
                self.bind_stmt(&for_stmt.body, file_name);
                self.pop_scope();
            }
            StmtKind::ForIn(fi) => {
                self.push_scope(ScopeKind::Loop, stmt.span);
                if let ForInOfLeft::Var(ref vs) = fi.left {
                    let flags = if matches!(vs.kind, tsc_rs_ast::VarKind::Var) {
                        SYM_VARIABLE
                    } else {
                        SYM_VARIABLE | SYM_BLOCK_SCOPED
                    };
                    for decl in &vs.declarations {
                        self.bind_variable_declaration(decl, flags, file_name);
                    }
                }
                if let ForInOfLeft::Expr(ref expr) = fi.left {
                    self.bind_expr(expr, file_name);
                }
                self.bind_expr(&fi.right, file_name);
                self.bind_stmt(&fi.body, file_name);
                self.pop_scope();
            }
            StmtKind::ForOf(fo) => {
                self.push_scope(ScopeKind::Loop, stmt.span);
                if let ForInOfLeft::Var(ref vs) = fo.left {
                    let flags = if matches!(vs.kind, tsc_rs_ast::VarKind::Var) {
                        SYM_VARIABLE
                    } else {
                        SYM_VARIABLE | SYM_BLOCK_SCOPED
                    };
                    for decl in &vs.declarations {
                        self.bind_variable_declaration(decl, flags, file_name);
                    }
                }
                if let ForInOfLeft::Expr(ref expr) = fo.left {
                    self.bind_expr(expr, file_name);
                }
                self.bind_expr(&fo.right, file_name);
                self.bind_stmt(&fo.body, file_name);
                self.pop_scope();
            }
            StmtKind::Try(try_stmt) => {
                self.push_scope(ScopeKind::Block, stmt.span);
                for s in &try_stmt.block {
                    self.bind_stmt(s, file_name);
                }
                self.pop_scope();
                if let Some(ref handler) = try_stmt.handler {
                    self.push_scope(ScopeKind::Catch, handler.span);
                    if let Some(ref param) = handler.param {
                        self.bind_pattern(
                            param,
                            SYM_VARIABLE | SYM_BLOCK_SCOPED | SYM_CATCH_VARIABLE,
                            file_name,
                        );
                    }
                    for s in &handler.body {
                        self.bind_stmt(s, file_name);
                    }
                    self.pop_scope();
                }
                if let Some(ref fin) = try_stmt.finalizer {
                    // A `finally` block is its own block scope — without one,
                    // its `let`/`const` leaked into the enclosing scope and
                    // collided with same-named bindings there.
                    self.push_scope(ScopeKind::Block, stmt.span);
                    for s in fin {
                        self.bind_stmt(s, file_name);
                    }
                    self.pop_scope();
                }
            }
            StmtKind::Export(export_decl) => {
                let was_export = self.in_export;
                self.in_export = true;
                match &export_decl.kind {
                    ExportDeclKind::Decl(decl) | ExportDeclKind::DefaultDecl(decl) => {
                        self.full_starts
                            .insert(decl.span.start, self.full_start(export_decl.span.start));
                        self.bind_stmt(decl, file_name);
                    }
                    _ => {}
                }
                self.in_export = was_export;
            }
            StmtKind::Labeled(labeled) => {
                self.bind_stmt(&labeled.body, file_name);
            }
            StmtKind::Switch(switch_stmt) => {
                self.bind_expr(&switch_stmt.discriminant, file_name);
                // The case block is a single block scope shared by all
                // clauses — `let` there must not collide with enclosing
                // bindings (capturedLetConstInLoop9).
                self.push_scope(ScopeKind::Block, stmt.span);
                for case in &switch_stmt.cases {
                    if let Some(ref test) = case.test {
                        self.bind_expr(test, file_name);
                    }
                    for s in &case.consequent {
                        self.bind_stmt(s, file_name);
                    }
                }
                self.pop_scope();
            }
            StmtKind::Import(import_decl) => match &import_decl.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    if let Some(ref name) = default {
                        self.declare(name, SYM_VARIABLE | SYM_IMPORT, file_name, import_decl.span);
                    }
                    for spec in named {
                        self.declare(&spec.local, SYM_VARIABLE | SYM_IMPORT, file_name, spec.span);
                    }
                    if let Some(ref ns) = namespace {
                        self.declare(ns, SYM_VARIABLE | SYM_IMPORT, file_name, import_decl.span);
                    }
                }
                ImportClause::Require(name) => {
                    self.declare(name, SYM_VARIABLE | SYM_IMPORT, file_name, import_decl.span);
                }
            },
            StmtKind::ImportEquals(import_eq) => {
                self.declare(
                    &import_eq.name,
                    SYM_VARIABLE | SYM_IMPORT,
                    file_name,
                    stmt.span,
                );
            }
            StmtKind::Expr(expr) => {
                self.bind_expr(expr, file_name);
            }
            StmtKind::Return(Some(expr)) => {
                self.bind_expr(expr, file_name);
            }
            StmtKind::Throw(expr) => {
                self.bind_expr(expr, file_name);
            }
            _ => {}
        }
    }

    /// Walk expressions to bind named function expressions, class expressions,
    /// arrow parameters, and object method shorthands.
    fn bind_expr(&mut self, expr: &Expr, file_name: &str) {
        let mut expr_stack = vec![expr];
        while let Some(expr) = expr_stack.pop() {
            match &expr.kind {
                ExprKind::FnExpr(fn_decl) => {
                    // Named function expression: `const f = function myFunc() {}`
                    // The name is scoped to the function body only.
                    self.push_scope(ScopeKind::Function, fn_decl.span);
                    if let Some(ref name) = fn_decl.name {
                        self.declare(name, SYM_FUNCTION, file_name, fn_decl.span);
                    }
                    if let Some(ref type_params) = fn_decl.type_params {
                        self.declare_type_params(type_params, file_name);
                    }
                    for param in &fn_decl.params {
                        self.bind_parameter(param, file_name);
                        if let Some(ref type_ann) = param.type_ann {
                            self.bind_type_node(type_ann, file_name);
                        }
                    }
                    if let Some(ref return_type) = fn_decl.return_type {
                        self.bind_type_node(return_type, file_name);
                    }
                    if let Some(ref body) = fn_decl.body {
                        for s in body {
                            self.bind_stmt(s, file_name);
                        }
                    }
                    self.pop_scope();
                }
                ExprKind::ClassExpr(class_decl) => {
                    // Named class expression: `const C = class MyClass {}`
                    self.push_scope(ScopeKind::Class, class_decl.span);
                    if let Some(ref name) = class_decl.name {
                        // Use name_span if available (points to class name),
                        // otherwise fall back to the full class expression span.
                        let decl_span = class_decl.name_span.unwrap_or(class_decl.span);
                        let class_sym = self.declare_node(
                            name,
                            SYM_CLASS,
                            file_name,
                            decl_span,
                            class_decl.span.start,
                        );
                        if let Some(ref type_params) = class_decl.type_params {
                            self.declare_type_params(type_params, file_name);
                        }
                        for member in &class_decl.members {
                            self.bind_class_member(member, class_sym, file_name);
                        }
                    } else {
                        // Anonymous class expression -- still bind members
                        let anon_sym = self.table.add_symbol("__class".to_string(), SYM_CLASS);
                        self.table
                            .scope_graph
                            .symbol_scopes
                            .entry(anon_sym)
                            .or_insert(self.current_scope);
                        for member in &class_decl.members {
                            self.bind_class_member(member, anon_sym, file_name);
                        }
                    }
                    self.pop_scope();
                }
                ExprKind::Arrow(arrow) => {
                    self.push_scope(ScopeKind::Function, arrow.span);
                    if let Some(ref type_params) = arrow.type_params {
                        self.declare_type_params(type_params, file_name);
                    }
                    for param in &arrow.params {
                        self.bind_parameter(param, file_name);
                        if let Some(ref type_ann) = param.type_ann {
                            self.bind_type_node(type_ann, file_name);
                        }
                    }
                    if let Some(ref return_type) = arrow.return_type {
                        self.bind_type_node(return_type, file_name);
                    }
                    match &arrow.body {
                        ArrowBody::Block(stmts) => {
                            for s in stmts {
                                self.bind_stmt(s, file_name);
                            }
                        }
                        ArrowBody::Expr(e) => self.bind_expr(e, file_name),
                    }
                    self.pop_scope();
                }
                ExprKind::ObjectLit(props) => {
                    for prop in props.iter().rev() {
                        match prop {
                            ObjLitProp::Method(method) => {
                                // Object method shorthand: `{ foo() {} }`
                                self.push_scope(ScopeKind::Function, method.span);
                                for param in &method.params {
                                    self.bind_parameter(param, file_name);
                                }
                                for s in &method.body {
                                    self.bind_stmt(s, file_name);
                                }
                                self.pop_scope();
                            }
                            ObjLitProp::Get(acc) => {
                                self.push_scope(ScopeKind::Function, acc.span);
                                for s in &acc.body {
                                    self.bind_stmt(s, file_name);
                                }
                                self.pop_scope();
                            }
                            ObjLitProp::Set(acc) => {
                                self.push_scope(ScopeKind::Function, acc.span);
                                for param in &acc.params {
                                    self.bind_parameter(param, file_name);
                                }
                                for s in &acc.body {
                                    self.bind_stmt(s, file_name);
                                }
                                self.pop_scope();
                            }
                            ObjLitProp::Property(p) => expr_stack.push(&p.value),
                            ObjLitProp::Spread(e, _) => expr_stack.push(e),
                            _ => {}
                        }
                    }
                }
                ExprKind::Call(call) => {
                    for arg in call.args.iter().rev() {
                        expr_stack.push(arg);
                    }
                    expr_stack.push(&call.callee);
                }
                ExprKind::New(new_expr) => {
                    if let Some(ref args) = new_expr.args {
                        for arg in args.iter().rev() {
                            expr_stack.push(arg);
                        }
                    }
                    expr_stack.push(&new_expr.callee);
                }
                ExprKind::Binary(bin) => {
                    expr_stack.push(&bin.right);
                    expr_stack.push(&bin.left);
                }
                ExprKind::Unary(u) => expr_stack.push(&u.argument),
                ExprKind::Update(u) => expr_stack.push(&u.argument),
                ExprKind::Assign(a) => {
                    expr_stack.push(&a.right);
                    expr_stack.push(&a.left);
                }
                ExprKind::Cond(c) => {
                    expr_stack.push(&c.alternate);
                    expr_stack.push(&c.consequent);
                    expr_stack.push(&c.test);
                }
                ExprKind::Member(m) => expr_stack.push(&m.object),
                ExprKind::ElemAccess(e) => {
                    expr_stack.push(&e.index);
                    expr_stack.push(&e.object);
                }
                ExprKind::Paren(e) => expr_stack.push(e),
                ExprKind::Spread(e) => expr_stack.push(e),
                ExprKind::As(a) => expr_stack.push(&a.expr),
                ExprKind::Satisfies(s) => expr_stack.push(&s.expr),
                ExprKind::NonNull(e) => expr_stack.push(e),
                ExprKind::TypeAssertion(ta) => expr_stack.push(&ta.expr),
                ExprKind::Await(e) => expr_stack.push(e),
                ExprKind::Yield(_, Some(e)) => expr_stack.push(e),
                ExprKind::ArrayLit(elems) => {
                    for elem in elems.iter().rev().flatten() {
                        expr_stack.push(elem);
                    }
                }
                ExprKind::Template(t) => {
                    for e in t.exprs.iter().rev() {
                        expr_stack.push(e);
                    }
                }
                ExprKind::TaggedTemplate(t) => {
                    for e in t.quasi.exprs.iter().rev() {
                        expr_stack.push(e);
                    }
                    expr_stack.push(&t.tag);
                }
                ExprKind::Comma(exprs) => {
                    for e in exprs.iter().rev() {
                        expr_stack.push(e);
                    }
                }
                ExprKind::Delete(e) | ExprKind::Typeof(e) | ExprKind::Void(e) => {
                    expr_stack.push(e);
                }
                ExprKind::Ident(name) => {
                    // Record identifier references in position_to_symbol so
                    // hover and go-to-definition work at usage sites, not only
                    // at declaration sites. Resolve the name through the scope
                    // chain once all declarations have been collected.
                    self.record_reference(name, expr.span, ReferenceKind::Value);
                }
                _ => {}
            }
        }
    }

    fn bind_type_name_expr(&mut self, expr: &Expr) {
        let mut expr_stack = vec![expr];
        while let Some(expr) = expr_stack.pop() {
            match &expr.kind {
                ExprKind::Ident(name) => {
                    self.record_reference(name, expr.span, ReferenceKind::Type);
                }
                ExprKind::Member(member) => expr_stack.push(&member.object),
                ExprKind::Paren(inner) | ExprKind::NonNull(inner) => expr_stack.push(inner),
                ExprKind::As(as_expr) => expr_stack.push(&as_expr.expr),
                ExprKind::Satisfies(satisfies) => expr_stack.push(&satisfies.expr),
                ExprKind::TypeAssertion(assertion) => expr_stack.push(&assertion.expr),
                ExprKind::Instantiation(instantiation) => expr_stack.push(&instantiation.expr),
                _ => {}
            }
        }
    }

    /// Walk a type annotation tree and record identifier references for type
    /// names so that hover and go-to-definition work on type references like
    /// `: RouteEntry`, `: Promise<SSRPageResult | null>`, etc.
    fn bind_type_node(&mut self, ty: &TypeNode, file_name: &str) {
        let mut stack = vec![ty];
        while let Some(node) = stack.pop() {
            match &node.kind {
                TypeNodeKind::Reference(type_ref) => {
                    // The type reference name is an Expr (usually Ident or Member)
                    self.bind_type_name_expr(&type_ref.name);
                    if let Some(ref type_args) = type_ref.type_args {
                        for arg in type_args.iter().rev() {
                            stack.push(arg);
                        }
                    }
                }
                TypeNodeKind::Array(inner) => stack.push(inner),
                TypeNodeKind::Tuple(elements) => {
                    for elem in elements.iter().rev() {
                        stack.push(&elem.type_node);
                    }
                }
                TypeNodeKind::Union(types) | TypeNodeKind::Intersection(types) => {
                    for t in types.iter().rev() {
                        stack.push(t);
                    }
                }
                TypeNodeKind::Function(fn_type) | TypeNodeKind::Constructor(fn_type)
                    if fn_type
                        .type_params
                        .as_ref()
                        .is_some_and(|tps| !tps.is_empty()) =>
                {
                    // `<T extends I>(p: T) => T`: the signature's own type
                    // parameters shadow outer ones.
                    self.bind_signature_types(
                        fn_type.type_params.as_ref(),
                        &fn_type.params,
                        Some(&fn_type.return_type),
                        node.span,
                        file_name,
                        false,
                    );
                }
                TypeNodeKind::Function(fn_type) | TypeNodeKind::Constructor(fn_type) => {
                    stack.push(&fn_type.return_type);
                    for param in &fn_type.params {
                        if let Some(ref ann) = param.type_ann {
                            stack.push(ann);
                        }
                    }
                }
                TypeNodeKind::Conditional(cond) => {
                    stack.push(&cond.false_type);
                    stack.push(&cond.true_type);
                    stack.push(&cond.extends);
                    stack.push(&cond.check);
                }
                TypeNodeKind::IndexedAccess(obj, idx) => {
                    stack.push(idx);
                    stack.push(obj);
                }
                TypeNodeKind::Keyof(inner)
                | TypeNodeKind::Unique(inner)
                | TypeNodeKind::Readonly(inner)
                | TypeNodeKind::Paren(inner)
                | TypeNodeKind::Rest(inner)
                | TypeNodeKind::Optional(inner) => {
                    stack.push(inner);
                }
                TypeNodeKind::Mapped(mapped) => {
                    if let Some(ref name_type) = mapped.name_type {
                        stack.push(name_type);
                    }
                    if let Some(ref type_ann) = mapped.type_ann {
                        stack.push(type_ann);
                    }
                }
                TypeNodeKind::TypeLit(members) => {
                    for member in members {
                        match &member.kind {
                            TypeMemberKind::PropertySig(prop) => {
                                if let Some(ref ann) = prop.type_ann {
                                    stack.push(ann);
                                }
                            }
                            TypeMemberKind::MethodSig(method) => {
                                if let Some(ref ret) = method.return_type {
                                    stack.push(ret);
                                }
                                for param in &method.params {
                                    if let Some(ref ann) = param.type_ann {
                                        stack.push(ann);
                                    }
                                }
                            }
                            TypeMemberKind::IndexSig(idx) => {
                                if let Some(ref ann) = idx.type_ann {
                                    stack.push(ann);
                                }
                            }
                            TypeMemberKind::ConstructSig(ctor) => {
                                if let Some(ref ret) = ctor.return_type {
                                    stack.push(ret);
                                }
                            }
                            TypeMemberKind::CallSig(call) => {
                                if let Some(ref ret) = call.return_type {
                                    stack.push(ret);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                TypeNodeKind::Infer(_, Some(constraint)) => stack.push(constraint),
                TypeNodeKind::TemplateLit(tl) => {
                    for t in tl.types.iter().rev() {
                        stack.push(t);
                    }
                }
                TypeNodeKind::TypeOperator(_, inner) => stack.push(inner),
                TypeNodeKind::NamedTupleMember(ntm) => stack.push(&*ntm.type_node),
                TypeNodeKind::TypeQuery(expr) => self.bind_expr(expr, file_name),
                TypeNodeKind::JSDocNullable(Some(inner)) => stack.push(inner),
                TypeNodeKind::ImportType(import_type) => {
                    if let Some(ref type_args) = import_type.type_args {
                        for arg in type_args.iter().rev() {
                            stack.push(arg);
                        }
                    }
                }
                _ => {} // Keyword, Literal, This, etc.
            }
        }
    }

    /// Bind a module declaration, handling dotted names like `namespace A.B.C`,
    /// `declare global { ... }`, module augmentations, and ambient module
    /// declarations.
    fn bind_module_decl(&mut self, module_decl: &ModuleDecl, file_name: &str) {
        match &module_decl.name {
            ModuleName::Ident(name) => {
                // `declare global { ... }` -- add declarations to the global
                // scope instead of creating a nested scope.
                if name == "global" {
                    self.bind_global_augmentation(module_decl, file_name);
                    return;
                }
                // Check for dotted name like A.B.C
                if name.contains('.') {
                    self.bind_dotted_module(name, &module_decl.body, file_name, module_decl.span);
                } else {
                    let decl_span = module_decl.name_span.unwrap_or(module_decl.span);
                    let mod_sym = self.declare_node(
                        name,
                        SYM_MODULE,
                        file_name,
                        decl_span,
                        module_decl.span.start,
                    );
                    if let Some(ref body) = module_decl.body {
                        self.push_scope(ScopeKind::Module, module_decl.span);
                        match body {
                            ModuleBody::Block(stmts) => {
                                for s in stmts {
                                    self.bind_stmt(s, file_name);
                                }
                                // Collect exports from this scope
                                self.collect_module_exports(mod_sym);
                            }
                            ModuleBody::Module(inner) => {
                                self.bind_module_decl(inner, file_name);
                            }
                        }
                        self.pop_scope();
                    }
                }
            }
            ModuleName::String(name) => {
                self.bind_string_module(name, module_decl, file_name);
            }
        }
    }

    /// Handle `declare global { ... }`: bind the body's declarations into the
    /// global (root) scope so they are visible everywhere.
    fn bind_global_augmentation(&mut self, module_decl: &ModuleDecl, file_name: &str) {
        let mod_sym = self
            .table
            .add_symbol("global".to_string(), SYM_MODULE | SYM_GLOBAL_AUGMENTATION);
        self.table
            .scope_graph
            .symbol_scopes
            .entry(mod_sym)
            .or_insert(0);
        self.table.symbols[mod_sym as usize]
            .declarations
            .push(Declaration {
                file_name: file_name.to_string(),
                full_start: module_decl.span.start,
                span: module_decl.span,
            });
        self.table
            .position_to_symbol
            .insert(module_decl.span.start, mod_sym);

        // Bind body statements in the global (root) scope
        if let Some(ref body) = module_decl.body {
            let saved_scope = self.current_scope;
            self.current_scope = 0; // global scope
            match body {
                ModuleBody::Block(stmts) => {
                    for s in stmts {
                        self.bind_stmt(s, file_name);
                    }
                    // Track which symbols were added to global scope
                    let scope_syms: Vec<SymbolId> =
                        self.scopes[0].symbols.values().copied().collect();
                    for sym_id in scope_syms {
                        if !self.table.global_augmentation_symbols.contains(&sym_id) {
                            self.table.global_augmentation_symbols.push(sym_id);
                        }
                    }
                }
                ModuleBody::Module(inner) => {
                    self.bind_module_decl(inner, file_name);
                }
            }
            self.current_scope = saved_scope;
        }
    }

    /// Handle `declare module 'specifier' { ... }`. Distinguishes between:
    /// - Module augmentation (path starts with `.` -- relative path)
    /// - Ambient module declaration (non-relative path like a package name)
    /// - Wildcard module declaration (contains `*`)
    fn bind_string_module(&mut self, name: &str, module_decl: &ModuleDecl, file_name: &str) {
        let is_wildcard = name.contains('*');
        let is_relative = name.starts_with('.') || name.starts_with('/');

        let flags = if is_relative {
            SYM_MODULE | SYM_MODULE_AUGMENTATION | SYM_STRING_MODULE
        } else {
            SYM_MODULE | SYM_STRING_MODULE
        };

        let mod_sym = self.declare(name, flags, file_name, module_decl.span);

        if let Some(ref body) = module_decl.body {
            self.push_scope(ScopeKind::Module, module_decl.span);
            match body {
                ModuleBody::Block(stmts) => {
                    for s in stmts {
                        self.bind_stmt(s, file_name);
                    }
                    // Collect exports for ambient/augmentation modules
                    self.collect_module_exports(mod_sym);
                }
                ModuleBody::Module(inner) => {
                    self.bind_module_decl(inner, file_name);
                }
            }
            self.pop_scope();
        }

        // Register in the appropriate tracking structure
        if is_wildcard {
            self.table
                .wildcard_modules
                .push((name.to_string(), mod_sym));
        } else if is_relative {
            self.table
                .module_augmentations
                .entry(name.to_string())
                .or_default()
                .push(mod_sym);
        } else {
            self.table.ambient_modules.insert(name.to_string(), mod_sym);
        }
    }

    /// Handle `namespace A.B.C {}` by creating chained scopes.
    fn bind_dotted_module(
        &mut self,
        dotted_name: &str,
        body: &Option<ModuleBody>,
        file_name: &str,
        span: Span,
    ) {
        let parts: Vec<&str> = dotted_name.split('.').collect();
        let mut parent_syms = Vec::new();

        // Create nested scopes for each part
        for (i, part) in parts.iter().enumerate() {
            let part_span = Span::new(
                span.start + i as u32,
                span.start + i as u32 + part.len() as u32,
            );
            let sym = self.declare(part, SYM_MODULE, file_name, part_span);
            parent_syms.push(sym);
            if i < parts.len() - 1 {
                self.push_scope(ScopeKind::Module, part_span);
            }
        }

        // Bind the body in the innermost scope
        if let Some(ref body) = body {
            self.push_scope(ScopeKind::Module, span);
            match body {
                ModuleBody::Block(stmts) => {
                    for s in stmts {
                        self.bind_stmt(s, file_name);
                    }
                    if let Some(&last_sym) = parent_syms.last() {
                        self.collect_module_exports(last_sym);
                    }
                }
                ModuleBody::Module(inner) => {
                    self.bind_module_decl(inner, file_name);
                }
            }
            self.pop_scope();
        }

        // Pop the intermediate scopes and collect exports for each parent
        for i in (0..parts.len() - 1).rev() {
            // Collect the child namespace as an export of the parent
            if i + 1 < parent_syms.len() {
                let child_sym_id = parent_syms[i + 1];
                let parent_sym_id = parent_syms[i];
                let child_name = parts[i + 1].to_string();
                self.table.symbols[parent_sym_id as usize]
                    .exports
                    .insert(child_name, child_sym_id);
            }
            self.pop_scope();
        }
    }

    /// Collect symbols from current scope that have SYM_EXPORT flag and
    /// add them to the module symbol's exports map.
    fn collect_module_exports(&mut self, module_sym: SymbolId) {
        let scope_syms: Vec<(String, SymbolId)> = self.scopes[self.current_scope]
            .symbols
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect();

        for (name, sym_id) in scope_syms {
            let flags = self.table.symbols[sym_id as usize].flags;
            if flags & SYM_EXPORT != 0 {
                self.table.symbols[module_sym as usize]
                    .exports
                    .insert(name, sym_id);
            }
        }
    }

    fn bind_variable_declaration(
        &mut self,
        declaration: &VarDeclarator,
        flags: SymbolFlags,
        file_name: &str,
    ) {
        if let PatKind::Ident(name) = &declaration.name.kind {
            self.declare_with_full_start(
                name,
                flags,
                file_name,
                declaration.name.span,
                declaration.full_start,
            );
        } else {
            self.bind_pattern_with_full_starts(
                &declaration.name,
                flags,
                file_name,
                &declaration.binding_name_full_starts,
            );
        }
    }

    fn bind_pattern(&mut self, pat: &Pat, flags: SymbolFlags, file_name: &str) {
        self.bind_pattern_with_full_starts(pat, flags, file_name, &[]);
    }

    fn bind_pattern_with_full_starts(
        &mut self,
        pat: &Pat,
        flags: SymbolFlags,
        file_name: &str,
        binding_name_full_starts: &[BindingNameFullStart],
    ) {
        let full_start_for = |span: Span| {
            binding_name_full_starts
                .iter()
                .find(|binding| binding.name_span == span)
                .map_or(span.start, |binding| binding.full_start)
        };
        match &pat.kind {
            PatKind::Ident(name) => {
                self.declare_with_full_start(
                    name,
                    flags,
                    file_name,
                    pat.span,
                    full_start_for(pat.span),
                );
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(_, p) => self.bind_pattern_with_full_starts(
                            p,
                            flags,
                            file_name,
                            binding_name_full_starts,
                        ),
                        ObjPatProp::Shorthand(name, span) => {
                            self.declare_with_full_start(
                                name,
                                flags,
                                file_name,
                                *span,
                                full_start_for(*span),
                            );
                        }
                        ObjPatProp::ShorthandAssign(name, _, span) => {
                            self.declare_with_full_start(
                                name,
                                flags,
                                file_name,
                                *span,
                                full_start_for(*span),
                            );
                        }
                        ObjPatProp::Rest(p) => self.bind_pattern_with_full_starts(
                            p,
                            flags,
                            file_name,
                            binding_name_full_starts,
                        ),
                    }
                }
            }
            PatKind::Array(elems) => {
                for elem in elems.iter().flatten() {
                    match elem {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => self
                            .bind_pattern_with_full_starts(
                                p,
                                flags,
                                file_name,
                                binding_name_full_starts,
                            ),
                    }
                }
            }
            PatKind::Assign(p, _) | PatKind::Rest(p) => {
                self.bind_pattern_with_full_starts(p, flags, file_name, binding_name_full_starts)
            }
        }
    }

    /// Record an overload signature from a function declaration with no body.
    fn record_overload_signature(&mut self, sym_id: SymbolId, fn_decl: &FnDecl) {
        let sig = OverloadSignature {
            params: fn_decl
                .params
                .iter()
                .map(|p| OverloadParam {
                    name: match &p.name.kind {
                        PatKind::Ident(n) => n.to_string(),
                        _ => "_".to_string(),
                    },
                    type_ann: p.type_ann.clone(),
                    optional: p.optional,
                    rest: p.dotdotdot,
                })
                .collect(),
            return_type: fn_decl.return_type.clone(),
            type_params: fn_decl
                .type_params
                .as_ref()
                .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                .unwrap_or_default(),
            type_param_defaults: fn_decl
                .type_params
                .as_ref()
                .map(|tps| {
                    tps.iter()
                        .map(|tp| tp.default.as_deref().cloned())
                        .collect()
                })
                .unwrap_or_default(),
        };
        self.table
            .overload_signatures
            .entry(sym_id)
            .or_default()
            .push(sig);
    }

    /// Record an overload signature from an interface method signature.
    fn record_interface_method_overload_signature(&mut self, sym_id: SymbolId, method: &MethodSig) {
        let sig = OverloadSignature {
            params: method
                .params
                .iter()
                .map(|p| OverloadParam {
                    name: match &p.name.kind {
                        PatKind::Ident(n) => n.to_string(),
                        _ => "_".to_string(),
                    },
                    type_ann: p.type_ann.clone(),
                    optional: p.optional,
                    rest: p.dotdotdot,
                })
                .collect(),
            return_type: method.return_type.clone(),
            type_params: method
                .type_params
                .as_ref()
                .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                .unwrap_or_default(),
            type_param_defaults: method
                .type_params
                .as_ref()
                .map(|tps| {
                    tps.iter()
                        .map(|tp| tp.default.as_deref().cloned())
                        .collect()
                })
                .unwrap_or_default(),
        };
        self.table
            .overload_signatures
            .entry(sym_id)
            .or_default()
            .push(sig);
    }

    /// Record an overload signature from a class method declaration with no body.
    fn record_method_overload_signature(&mut self, sym_id: SymbolId, method: &ClassMethod) {
        let sig = OverloadSignature {
            params: method
                .params
                .iter()
                .map(|p| OverloadParam {
                    name: match &p.name.kind {
                        PatKind::Ident(n) => n.to_string(),
                        _ => "_".to_string(),
                    },
                    type_ann: p.type_ann.clone(),
                    optional: p.optional,
                    rest: p.dotdotdot,
                })
                .collect(),
            return_type: method.return_type.clone(),
            type_params: method
                .type_params
                .as_ref()
                .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                .unwrap_or_default(),
            type_param_defaults: method
                .type_params
                .as_ref()
                .map(|tps| {
                    tps.iter()
                        .map(|tp| tp.default.as_deref().cloned())
                        .collect()
                })
                .unwrap_or_default(),
        };
        self.table
            .overload_signatures
            .entry(sym_id)
            .or_default()
            .push(sig);
    }

    /// Record an overload signature from a constructor declaration with no body.
    fn record_constructor_overload_signature(
        &mut self,
        class_sym: SymbolId,
        ctor: &ClassConstructor,
    ) {
        let sig = OverloadSignature {
            params: ctor
                .params
                .iter()
                .map(|p| OverloadParam {
                    name: match &p.name.kind {
                        PatKind::Ident(n) => n.to_string(),
                        _ => "_".to_string(),
                    },
                    type_ann: p.type_ann.clone(),
                    optional: p.optional,
                    rest: p.dotdotdot,
                })
                .collect(),
            return_type: None,
            type_params: Vec::new(), // constructors don't have their own type params
            type_param_defaults: Vec::new(),
        };
        // Store constructor overloads under a special key: the class symbol itself
        // with a constructor-specific marker. We'll use the class_sym directly since
        // the type checker can look up constructor overloads via the class symbol.
        self.table
            .overload_signatures
            .entry(class_sym)
            .or_default()
            .push(sig);
    }

    fn bind_class_member(&mut self, member: &ClassMember, class_sym: SymbolId, file_name: &str) {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                let prop_name_info = match prop.name {
                    PropName::Ident(ref name, _) => Some((name.to_string(), false)),
                    PropName::Private(ref name, _) => Some((format!("#{}", name), true)),
                    _ => None,
                };
                if let Some((name, _is_private)) = prop_name_info {
                    let mut flags = SYM_PROPERTY;
                    if prop.modifiers & MOD_ACCESSOR != 0 {
                        flags |= SYM_AUTO_ACCESSOR;
                    }
                    if prop.modifiers & MOD_STATIC != 0 {
                        flags |= SYM_STATIC;
                    }
                    // Use the property name span so goToDefinition points to the
                    // name identifier, not the leading modifier keyword.
                    let id = self.declare_node(
                        &name,
                        flags,
                        file_name,
                        prop.name.span(),
                        member.span.start,
                    );
                    self.table.symbols[class_sym as usize]
                        .members
                        .insert(name.to_string(), id);
                }
                if let Some(ref type_ann) = prop.type_ann {
                    self.bind_type_node(type_ann, file_name);
                }
                if let Some(ref init) = prop.initializer {
                    self.bind_expr(init, file_name);
                }
            }
            ClassMemberKind::Method(method) => {
                let method_name_info = match method.name {
                    PropName::Ident(ref name, _) => Some((name.to_string(), false)),
                    PropName::Private(ref name, _) => Some((format!("#{}", name), true)),
                    _ => None,
                };
                if let Some((name, _is_private)) = method_name_info {
                    // Use the method name span so goToDefinition points to the
                    // name identifier, not the leading modifier keyword.
                    let mut flags = SYM_METHOD;
                    if method.modifiers & MOD_STATIC != 0 {
                        flags |= SYM_STATIC;
                    }
                    let id = self.declare_node(
                        &name,
                        flags,
                        file_name,
                        method.name.span(),
                        member.span.start,
                    );
                    self.table.symbols[class_sym as usize]
                        .members
                        .insert(name.to_string(), id);
                    // Record overload signature for body-less method declarations
                    if method.body.is_none() {
                        self.record_method_overload_signature(id, method);
                    }
                }
                self.push_scope(ScopeKind::Function, member.span);
                if let Some(ref type_params) = method.type_params {
                    self.declare_type_params(type_params, file_name);
                }
                for param in &method.params {
                    self.bind_parameter(param, file_name);
                    if let Some(ref type_ann) = param.type_ann {
                        self.bind_type_node(type_ann, file_name);
                    }
                }
                if let Some(ref return_type) = method.return_type {
                    self.bind_type_node(return_type, file_name);
                }
                if let Some(ref body) = method.body {
                    for s in body {
                        self.bind_stmt(s, file_name);
                    }
                }
                self.pop_scope();
            }
            ClassMemberKind::Constructor(ctor) => {
                // Record constructor overload signature for body-less constructors
                if ctor.body.is_none() {
                    // Store constructor overloads on the class symbol itself
                    self.record_constructor_overload_signature(class_sym, ctor);
                }
                self.push_scope(ScopeKind::Function, member.span);
                for param in &ctor.params {
                    self.bind_parameter(param, file_name);
                    if let Some(ref type_ann) = param.type_ann {
                        self.bind_type_node(type_ann, file_name);
                    }
                }
                if let Some(ref body) = ctor.body {
                    for s in body {
                        self.bind_stmt(s, file_name);
                    }
                }
                self.pop_scope();
            }
            ClassMemberKind::GetAccessor(acc) => {
                let acc_name = match acc.name {
                    PropName::Ident(ref name, _) => Some(name.to_string()),
                    PropName::Private(ref name, _) => Some(format!("#{}", name)),
                    PropName::String(ref name, _) => Some(name.to_string()),
                    _ => None,
                };
                if let Some(name) = acc_name {
                    // Use the accessor name span so goToDefinition points to the
                    // name identifier, not the leading modifier keyword.
                    let id = self.declare_node(
                        &name,
                        SYM_GET_ACCESSOR,
                        file_name,
                        acc.name.span(),
                        member.span.start,
                    );
                    self.table.symbols[class_sym as usize]
                        .members
                        .insert(name, id);
                }
                self.push_scope(ScopeKind::Function, member.span);
                for param in &acc.params {
                    self.bind_parameter(param, file_name);
                    if let Some(ref type_ann) = param.type_ann {
                        self.bind_type_node(type_ann, file_name);
                    }
                }
                if let Some(ref return_type) = acc.return_type {
                    self.bind_type_node(return_type, file_name);
                }
                if let Some(ref body) = acc.body {
                    for s in body {
                        self.bind_stmt(s, file_name);
                    }
                }
                self.pop_scope();
            }
            ClassMemberKind::SetAccessor(acc) => {
                let acc_name = match acc.name {
                    PropName::Ident(ref name, _) => Some(name.to_string()),
                    PropName::Private(ref name, _) => Some(format!("#{}", name)),
                    PropName::String(ref name, _) => Some(name.to_string()),
                    _ => None,
                };
                if let Some(name) = acc_name {
                    // Use the accessor name span so goToDefinition points to the
                    // name identifier, not the leading modifier keyword.
                    let id = self.declare_node(
                        &name,
                        SYM_SET_ACCESSOR,
                        file_name,
                        acc.name.span(),
                        member.span.start,
                    );
                    self.table.symbols[class_sym as usize]
                        .members
                        .insert(name, id);
                }
                self.push_scope(ScopeKind::Function, member.span);
                for param in &acc.params {
                    self.bind_parameter(param, file_name);
                    if let Some(ref type_ann) = param.type_ann {
                        self.bind_type_node(type_ann, file_name);
                    }
                }
                if let Some(ref return_type) = acc.return_type {
                    self.bind_type_node(return_type, file_name);
                }
                if let Some(ref body) = acc.body {
                    for s in body {
                        self.bind_stmt(s, file_name);
                    }
                }
                self.pop_scope();
            }
            ClassMemberKind::StaticBlock(stmts) => {
                self.push_scope(ScopeKind::StaticBlock, member.span);
                for s in stmts {
                    self.bind_stmt(s, file_name);
                }
                self.pop_scope();
            }
            _ => {}
        }
    }

    /// Bind interface type members into the interface symbol's members map.
    /// Declare a type-parameter list in the current scope, then record the
    /// type references in its constraints and defaults (which may mention
    /// sibling parameters, so all names are declared first).
    fn declare_type_params<'t>(
        &mut self,
        type_params: impl IntoIterator<Item = &'t TypeParam>,
        file_name: &str,
    ) {
        let tps: Vec<&TypeParam> = type_params.into_iter().collect();
        for tp in &tps {
            self.declare(&tp.name, SYM_TYPE_PARAMETER, file_name, tp.span);
        }
        for tp in &tps {
            if let Some(ref constraint) = tp.constraint {
                self.bind_type_node(constraint, file_name);
            }
            if let Some(ref default) = tp.default {
                self.bind_type_node(default, file_name);
            }
        }
    }

    /// Record type references inside a signature (parameter annotations and
    /// return type), with the signature's own type parameters in scope.
    fn bind_signature_types(
        &mut self,
        type_params: Option<&Vec<TypeParam>>,
        params: &[Param],
        return_type: Option<&TypeNode>,
        span: Span,
        file_name: &str,
        declare_params: bool,
    ) {
        let scoped = type_params.is_some_and(|tps| !tps.is_empty())
            || (declare_params && !params.is_empty());
        if scoped {
            self.push_scope(ScopeKind::Function, span);
            self.declare_type_params(type_params.into_iter().flatten(), file_name);
        }
        for param in params {
            if declare_params {
                self.bind_parameter(param, file_name);
            }
            if let Some(ref type_ann) = param.type_ann {
                self.bind_type_node(type_ann, file_name);
            }
        }
        if let Some(return_type) = return_type {
            self.bind_type_node(return_type, file_name);
        }
        if scoped {
            self.pop_scope();
        }
    }

    fn bind_type_member(&mut self, member: &TypeMember, iface_sym: SymbolId, file_name: &str) {
        match &member.kind {
            TypeMemberKind::CallSig(sig) => {
                self.bind_signature_types(
                    sig.type_params.as_ref(),
                    &sig.params,
                    sig.return_type.as_ref(),
                    member.span,
                    file_name,
                    true,
                );
            }
            TypeMemberKind::ConstructSig(sig) => {
                self.bind_signature_types(
                    sig.type_params.as_ref(),
                    &sig.params,
                    sig.return_type.as_ref(),
                    member.span,
                    file_name,
                    true,
                );
            }
            TypeMemberKind::IndexSig(idx) => {
                self.bind_signature_types(
                    None,
                    &idx.params,
                    idx.type_ann.as_ref(),
                    member.span,
                    file_name,
                    false,
                );
            }
            TypeMemberKind::PropertySig(prop) => {
                if let PropName::Ident(ref name, _) | PropName::String(ref name, _) = prop.name {
                    let id = self.declare(name, SYM_PROPERTY, file_name, member.span);
                    self.table.symbols[iface_sym as usize]
                        .members
                        .insert(name.to_string(), id);
                }
                if let Some(ref type_ann) = prop.type_ann {
                    self.bind_type_node(type_ann, file_name);
                }
            }
            TypeMemberKind::MethodSig(method) => {
                if let PropName::Ident(ref name, _) | PropName::String(ref name, _) = method.name {
                    let id = self.declare(name, SYM_METHOD, file_name, member.span);
                    self.table.symbols[iface_sym as usize]
                        .members
                        .insert(name.to_string(), id);
                    // Record interface method signature as overload
                    self.record_interface_method_overload_signature(id, method);
                }
                self.bind_signature_types(
                    method.type_params.as_ref(),
                    &method.params,
                    method.return_type.as_ref(),
                    member.span,
                    file_name,
                    true,
                );
            }
            TypeMemberKind::GetAccessorSig(acc) => {
                if let PropName::Ident(ref name, _) | PropName::String(ref name, _) = acc.name {
                    let id = self.declare(name, SYM_GET_ACCESSOR, file_name, member.span);
                    self.table.symbols[iface_sym as usize]
                        .members
                        .insert(name.to_string(), id);
                }
                self.bind_signature_types(
                    None,
                    &acc.params,
                    acc.return_type.as_ref(),
                    member.span,
                    file_name,
                    true,
                );
            }
            TypeMemberKind::SetAccessorSig(acc) => {
                if let PropName::Ident(ref name, _) | PropName::String(ref name, _) = acc.name {
                    let id = self.declare(name, SYM_SET_ACCESSOR, file_name, member.span);
                    self.table.symbols[iface_sym as usize]
                        .members
                        .insert(name.to_string(), id);
                }
                self.bind_signature_types(
                    None,
                    &acc.params,
                    acc.return_type.as_ref(),
                    member.span,
                    file_name,
                    true,
                );
            }
            _ => {}
        }
    }
}

/// Convenience function to bind a source file and return the symbol table.
pub fn bind(file: &SourceFile) -> SymbolTable {
    Binder::new().bind(file)
}

/// Resolve a name starting from a given scope index using the retained scope graph.
pub fn resolve(table: &SymbolTable, name: &str, scope: ScopeId) -> Option<SymbolId> {
    let mut scope_idx = Some(scope);
    while let Some(id) = scope_idx {
        let scope = table.scope_graph.scope(id)?;
        if let Some(symbol_id) = scope.bindings.get(name) {
            return Some(*symbol_id);
        }
        scope_idx = scope.parent;
    }
    find_symbol(table, name)
}

/// Find a symbol by name in the symbol table (first match).
pub fn find_symbol(table: &SymbolTable, name: &str) -> Option<SymbolId> {
    table.symbols.iter().find(|s| s.name == name).map(|s| s.id)
}

/// Get all exported symbols from a source file's symbol table.
pub fn get_exports(_file: &SourceFile, table: &SymbolTable) -> Vec<SymbolId> {
    table
        .symbols
        .iter()
        .filter(|s| s.flags & SYM_EXPORT != 0)
        .map(|s| s.id)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a minimal SourceFile with a single statement.
    fn make_source(file_name: &str, text: &str, stmts: Vec<Stmt>) -> SourceFile {
        SourceFile {
            file_name: file_name.to_string(),
            text: text.to_string(),
            statements: stmts,
            diagnostics: Vec::new(),
            span: Span::new(0, text.len() as u32),
            comments: Vec::new(),
        }
    }

    fn make_var_stmt(kind: VarKind, name: &str, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::Var(Box::new(VarStmt {
                kind,
                declarations: vec![VarDeclarator {
                    name: Pat {
                        kind: PatKind::Ident(name.to_string().into()),
                        span,
                    },
                    type_ann: None,
                    init: None,
                    full_start: span.start,
                    binding_name_full_starts: Vec::new(),
                    span,
                    definite: false,
                }],
                modifiers: MOD_NONE,
            })),
            span,
        }
    }

    fn make_ident_pat(name: &str, span: Span) -> Pat {
        Pat {
            kind: PatKind::Ident(name.to_string().into()),
            span,
        }
    }

    fn make_ident_expr(name: &str, span: Span) -> Expr {
        Expr {
            kind: ExprKind::Ident(name.to_string().into()),
            span,
        }
    }

    fn make_param(name: &str, span: Span, type_ann: Option<TypeNode>) -> Param {
        Param {
            name: make_ident_pat(name, span),
            type_ann,
            initializer: None,
            dotdotdot: false,
            optional: false,
            modifiers: MOD_NONE,
            decorators: Vec::new(),
            span,
        }
    }

    fn make_fn_decl(name: &str, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::FnDecl(Box::new(FnDecl {
                name: Some(name.to_string()),
                name_span: None,
                type_params: None,
                params: Vec::new(),
                return_type: None,
                body: Some(Vec::new()),
                modifiers: MOD_NONE,
                is_generator: false,
                is_async: false,
                decorators: Vec::new(),
                span,
            })),
            span,
        }
    }

    fn make_class_decl(name: &str, members: Vec<ClassMember>, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::ClassDecl(Box::new(ClassDecl {
                name: Some(name.to_string()),
                name_span: None,
                type_params: None,
                extends: None,
                extends_type_args: None,
                implements: Vec::new(),
                members,
                modifiers: MOD_NONE,
                decorators: Vec::new(),
                span,
            })),
            span,
        }
    }

    fn make_interface_decl(name: &str, members: Vec<TypeMember>, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::InterfaceDecl(Box::new(InterfaceDecl {
                name: name.to_string(),
                name_span: None,
                type_params: None,
                extends: Vec::new(),
                members,
                modifiers: MOD_NONE,
                span,
            })),
            span,
        }
    }

    fn make_module_decl(name: &str, body: Option<Vec<Stmt>>, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::ModuleDecl(Box::new(ModuleDecl {
                name: ModuleName::Ident(name.to_string()),
                name_span: None,
                body: body.map(|stmts| ModuleBody::Block(stmts)),
                modifiers: MOD_NONE,
                span,
            })),
            span,
        }
    }

    fn make_enum_decl(name: &str, members: Vec<&str>, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::EnumDecl(Box::new(EnumDecl {
                name: name.to_string(),
                name_span: None,
                members: members
                    .into_iter()
                    .enumerate()
                    .map(|(i, m)| EnumMember {
                        name: PropName::Ident(
                            m.to_string().into(),
                            Span::new(
                                span.start + 10 + i as u32 * 5,
                                span.start + 10 + i as u32 * 5 + m.len() as u32,
                            ),
                        ),
                        initializer: None,
                        span: Span::new(
                            span.start + 10 + i as u32 * 5,
                            span.start + 10 + i as u32 * 5 + m.len() as u32,
                        ),
                    })
                    .collect(),
                modifiers: MOD_NONE,
                is_const: false,
                span,
            })),
            span,
        }
    }

    fn make_export(inner: Stmt) -> Stmt {
        let span = inner.span;
        Stmt {
            kind: StmtKind::Export(Box::new(ExportDecl {
                kind: ExportDeclKind::Decl(Box::new(inner)),
                span,
            })),
            span,
        }
    }

    fn make_property_sig(name: &str, span: Span) -> TypeMember {
        TypeMember {
            kind: TypeMemberKind::PropertySig(PropertySig {
                name: PropName::Ident(name.to_string().into(), span),
                type_ann: None,
                optional: false,
                readonly: false,
            }),
            span,
        }
    }

    fn make_method_sig(name: &str, span: Span) -> TypeMember {
        TypeMember {
            kind: TypeMemberKind::MethodSig(MethodSig {
                name: PropName::Ident(name.to_string().into(), span),
                type_params: None,
                params: Vec::new(),
                return_type: None,
                optional: false,
            }),
            span,
        }
    }

    // -----------------------------------------------------------------------
    // 1. Basic binding
    // -----------------------------------------------------------------------

    #[test]
    fn test_bind_var_declaration() {
        let file = make_source(
            "test.ts",
            "var x = 1;",
            vec![make_var_stmt(VarKind::Var, "x", Span::new(4, 5))],
        );
        let table = bind(&file);
        assert_eq!(table.symbols.len(), 1);
        assert_eq!(table.symbols[0].name, "x");
        assert_eq!(table.symbols[0].flags, SYM_VARIABLE);
    }

    #[test]
    fn test_bind_let_declaration() {
        let file = make_source(
            "test.ts",
            "let y = 2;",
            vec![make_var_stmt(VarKind::Let, "y", Span::new(4, 5))],
        );
        let table = bind(&file);
        assert_eq!(table.symbols.len(), 1);
        assert_eq!(table.symbols[0].name, "y");
        assert_eq!(table.symbols[0].flags, SYM_VARIABLE | SYM_BLOCK_SCOPED);
    }

    #[test]
    fn test_bind_const_declaration() {
        let file = make_source(
            "test.ts",
            "const z = 3;",
            vec![make_var_stmt(VarKind::Const, "z", Span::new(6, 7))],
        );
        let table = bind(&file);
        assert_eq!(table.symbols.len(), 1);
        assert_eq!(table.symbols[0].name, "z");
        assert_eq!(
            table.symbols[0].flags,
            SYM_VARIABLE | SYM_BLOCK_SCOPED | SYM_CONST
        );
    }

    #[test]
    fn test_bind_function_declaration() {
        let file = make_source(
            "test.ts",
            "function foo() {}",
            vec![make_fn_decl("foo", Span::new(0, 17))],
        );
        let table = bind(&file);
        assert_eq!(table.symbols.len(), 1);
        assert_eq!(table.symbols[0].name, "foo");
        assert_eq!(table.symbols[0].flags, SYM_FUNCTION);
    }

    #[test]
    fn test_bind_class_with_members() {
        let members = vec![
            ClassMember {
                kind: ClassMemberKind::Property(ClassProp {
                    name: PropName::Ident("x".to_string().into(), Span::new(20, 21)),
                    type_ann: None,
                    initializer: None,
                    modifiers: MOD_NONE,
                    optional: false,
                    definite: false,
                    decorators: Vec::new(),
                }),
                span: Span::new(20, 21),
            },
            ClassMember {
                kind: ClassMemberKind::Method(ClassMethod {
                    name: PropName::Ident("greet".to_string().into(), Span::new(30, 35)),
                    type_params: None,
                    params: Vec::new(),
                    return_type: None,
                    body: Some(Vec::new()),
                    modifiers: MOD_NONE,
                    is_generator: false,
                    is_async: false,
                    optional: false,
                    decorators: Vec::new(),
                }),
                span: Span::new(30, 35),
            },
        ];
        let file = make_source(
            "test.ts",
            "class Foo { x; greet() {} }",
            vec![make_class_decl("Foo", members, Span::new(0, 27))],
        );
        let table = bind(&file);
        // Foo + x + greet
        assert!(table.symbols.len() >= 3);
        let foo_sym = table.get_symbol(0).unwrap();
        assert_eq!(foo_sym.name, "Foo");
        assert_eq!(foo_sym.flags, SYM_CLASS);
        assert!(foo_sym.members.contains_key("x"));
        assert!(foo_sym.members.contains_key("greet"));
    }

    // -----------------------------------------------------------------------
    // 2. Interface merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_interface_merging_same_scope() {
        let file = make_source(
            "test.ts",
            "interface A { x: number; }\ninterface A { y: string; }",
            vec![
                make_interface_decl(
                    "A",
                    vec![make_property_sig("x", Span::new(14, 15))],
                    Span::new(0, 25),
                ),
                make_interface_decl(
                    "A",
                    vec![make_property_sig("y", Span::new(40, 41))],
                    Span::new(26, 52),
                ),
            ],
        );
        let table = bind(&file);
        // Should merge into one symbol with 2 declarations
        let a_sym = find_symbol(&table, "A").unwrap();
        let sym = table.get_symbol(a_sym).unwrap();
        assert_eq!(sym.name, "A");
        assert_eq!(sym.flags & SYM_INTERFACE, SYM_INTERFACE);
        assert_eq!(sym.declarations.len(), 2);
        // Members from both declarations should be present
        assert!(sym.members.contains_key("x"));
        assert!(sym.members.contains_key("y"));
    }

    #[test]
    fn test_interface_merging_methods() {
        let file = make_source(
            "test.ts",
            "interface B { foo(): void; }\ninterface B { bar(): void; }",
            vec![
                make_interface_decl(
                    "B",
                    vec![make_method_sig("foo", Span::new(14, 17))],
                    Span::new(0, 27),
                ),
                make_interface_decl(
                    "B",
                    vec![make_method_sig("bar", Span::new(42, 45))],
                    Span::new(28, 55),
                ),
            ],
        );
        let table = bind(&file);
        let b_sym = find_symbol(&table, "B").unwrap();
        let sym = table.get_symbol(b_sym).unwrap();
        assert_eq!(sym.declarations.len(), 2);
        assert!(sym.members.contains_key("foo"));
        assert!(sym.members.contains_key("bar"));
    }

    #[test]
    fn test_three_way_interface_merge() {
        let file = make_source(
            "test.ts",
            "interface C{a:any}\ninterface C{b:any}\ninterface C{c:any}",
            vec![
                make_interface_decl(
                    "C",
                    vec![make_property_sig("a", Span::new(12, 13))],
                    Span::new(0, 17),
                ),
                make_interface_decl(
                    "C",
                    vec![make_property_sig("b", Span::new(30, 31))],
                    Span::new(18, 35),
                ),
                make_interface_decl(
                    "C",
                    vec![make_property_sig("c", Span::new(48, 49))],
                    Span::new(36, 53),
                ),
            ],
        );
        let table = bind(&file);
        let c_sym = find_symbol(&table, "C").unwrap();
        let sym = table.get_symbol(c_sym).unwrap();
        assert_eq!(sym.declarations.len(), 3);
        assert!(sym.members.contains_key("a"));
        assert!(sym.members.contains_key("b"));
        assert!(sym.members.contains_key("c"));
    }

    // -----------------------------------------------------------------------
    // 3. Namespace + class merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_namespace_class_merging() {
        let file = make_source(
            "test.ts",
            "class Foo {}\nnamespace Foo { export var x = 1; }",
            vec![
                make_class_decl("Foo", Vec::new(), Span::new(0, 12)),
                make_module_decl(
                    "Foo",
                    Some(vec![make_export(make_var_stmt(
                        VarKind::Var,
                        "x",
                        Span::new(29, 30),
                    ))]),
                    Span::new(13, 47),
                ),
            ],
        );
        let table = bind(&file);
        let foo_sym = find_symbol(&table, "Foo").unwrap();
        let sym = table.get_symbol(foo_sym).unwrap();
        assert_eq!(sym.declarations.len(), 2);
        assert!(sym.flags & SYM_CLASS != 0);
        assert!(sym.flags & SYM_MODULE != 0);
    }

    // -----------------------------------------------------------------------
    // 4. Namespace + function merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_namespace_function_merging() {
        let file = make_source(
            "test.ts",
            "function bar() {}\nnamespace bar { }",
            vec![
                make_fn_decl("bar", Span::new(0, 17)),
                make_module_decl("bar", Some(Vec::new()), Span::new(18, 35)),
            ],
        );
        let table = bind(&file);
        let bar_sym = find_symbol(&table, "bar").unwrap();
        let sym = table.get_symbol(bar_sym).unwrap();
        assert_eq!(sym.declarations.len(), 2);
        assert!(sym.flags & SYM_FUNCTION != 0);
        assert!(sym.flags & SYM_MODULE != 0);
    }

    // -----------------------------------------------------------------------
    // 5. Namespace + namespace merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_namespace_namespace_merging() {
        let file = make_source(
            "test.ts",
            "namespace N { }\nnamespace N { }",
            vec![
                make_module_decl("N", Some(Vec::new()), Span::new(0, 15)),
                make_module_decl("N", Some(Vec::new()), Span::new(16, 31)),
            ],
        );
        let table = bind(&file);
        let n_sym = find_symbol(&table, "N").unwrap();
        let sym = table.get_symbol(n_sym).unwrap();
        assert_eq!(sym.declarations.len(), 2);
        assert!(sym.flags & SYM_MODULE != 0);
    }

    // -----------------------------------------------------------------------
    // 6. Duplicate let/const detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_duplicate_let_error() {
        let file = make_source(
            "test.ts",
            "let x = 1;\nlet x = 2;",
            vec![
                make_var_stmt(VarKind::Let, "x", Span::new(4, 5)),
                make_var_stmt(VarKind::Let, "x", Span::new(15, 16)),
            ],
        );
        let table = bind(&file);
        assert!(!table.diagnostics.is_empty());
        // let/const redeclaration is tsc's TS2451, not a plain duplicate.
        assert!(table.diagnostics[0]
            .message
            .contains("Cannot redeclare block-scoped variable"));
    }

    #[test]
    fn test_duplicate_const_error() {
        let file = make_source(
            "test.ts",
            "const a = 1;\nconst a = 2;",
            vec![
                make_var_stmt(VarKind::Const, "a", Span::new(6, 7)),
                make_var_stmt(VarKind::Const, "a", Span::new(19, 20)),
            ],
        );
        let table = bind(&file);
        assert!(!table.diagnostics.is_empty());
        assert!(table.diagnostics[0]
            .message
            .contains("Cannot redeclare block-scoped variable"));
    }

    #[test]
    fn test_duplicate_let_const_error() {
        let file = make_source(
            "test.ts",
            "let x = 1;\nconst x = 2;",
            vec![
                make_var_stmt(VarKind::Let, "x", Span::new(4, 5)),
                make_var_stmt(VarKind::Const, "x", Span::new(17, 18)),
            ],
        );
        let table = bind(&file);
        assert!(!table.diagnostics.is_empty());
    }

    // -----------------------------------------------------------------------
    // 7. Var hoisting (no error for duplicate var)
    // -----------------------------------------------------------------------

    #[test]
    fn test_var_hoisting_no_error() {
        let file = make_source(
            "test.ts",
            "var x = 1;\nvar x = 2;",
            vec![
                make_var_stmt(VarKind::Var, "x", Span::new(4, 5)),
                make_var_stmt(VarKind::Var, "x", Span::new(15, 16)),
            ],
        );
        let table = bind(&file);
        assert!(table.diagnostics.is_empty());
        // Should merge into one symbol with 2 declarations
        let x_sym = find_symbol(&table, "x").unwrap();
        let sym = table.get_symbol(x_sym).unwrap();
        assert_eq!(sym.declarations.len(), 2);
    }

    #[test]
    fn test_var_hoisting_preserves_flags() {
        let file = make_source(
            "test.ts",
            "var y = 1;\nvar y = 'hello';",
            vec![
                make_var_stmt(VarKind::Var, "y", Span::new(4, 5)),
                make_var_stmt(VarKind::Var, "y", Span::new(15, 16)),
            ],
        );
        let table = bind(&file);
        assert!(table.diagnostics.is_empty());
        let y_sym = find_symbol(&table, "y").unwrap();
        let sym = table.get_symbol(y_sym).unwrap();
        assert_eq!(sym.flags, SYM_VARIABLE); // no BLOCK_SCOPED
    }

    // -----------------------------------------------------------------------
    // 8. Function expression naming
    // -----------------------------------------------------------------------

    #[test]
    fn test_named_function_expression() {
        let fn_expr = Expr {
            kind: ExprKind::FnExpr(Box::new(FnDecl {
                name: Some("myFunc".to_string()),
                name_span: None,
                type_params: None,
                params: vec![Param {
                    name: Pat {
                        kind: PatKind::Ident("a".to_string().into()),
                        span: Span::new(30, 31),
                    },
                    type_ann: None,
                    initializer: None,
                    dotdotdot: false,
                    optional: false,
                    modifiers: MOD_NONE,
                    decorators: Vec::new(),
                    span: Span::new(30, 31),
                }],
                return_type: None,
                body: Some(Vec::new()),
                modifiers: MOD_NONE,
                is_generator: false,
                is_async: false,
                decorators: Vec::new(),
                span: Span::new(10, 50),
            })),
            span: Span::new(10, 50),
        };
        let stmt = Stmt {
            kind: StmtKind::Var(Box::new(VarStmt {
                kind: VarKind::Const,
                declarations: vec![VarDeclarator {
                    name: Pat {
                        kind: PatKind::Ident("f".to_string().into()),
                        span: Span::new(6, 7),
                    },
                    type_ann: None,
                    init: Some(Box::new(fn_expr)),
                    full_start: 6,
                    binding_name_full_starts: Vec::new(),
                    span: Span::new(6, 50),
                    definite: false,
                }],
                modifiers: MOD_NONE,
            })),
            span: Span::new(0, 50),
        };
        let file = make_source("test.ts", "const f = function myFunc(a) {};", vec![stmt]);
        let table = bind(&file);
        // f + myFunc + a
        assert!(find_symbol(&table, "f").is_some());
        assert!(find_symbol(&table, "myFunc").is_some());
        assert!(find_symbol(&table, "a").is_some());
        let my_func = table
            .get_symbol(find_symbol(&table, "myFunc").unwrap())
            .unwrap();
        assert_eq!(my_func.flags & SYM_FUNCTION, SYM_FUNCTION);
    }

    #[test]
    fn test_class_expression_binding() {
        let class_expr = Expr {
            kind: ExprKind::ClassExpr(Box::new(ClassDecl {
                name: Some("MyClass".to_string()),
                name_span: None,
                type_params: None,
                extends: None,
                extends_type_args: None,
                implements: Vec::new(),
                members: vec![ClassMember {
                    kind: ClassMemberKind::Property(ClassProp {
                        name: PropName::Ident("val".to_string().into(), Span::new(40, 43)),
                        type_ann: None,
                        initializer: None,
                        modifiers: MOD_NONE,
                        optional: false,
                        definite: false,
                        decorators: Vec::new(),
                    }),
                    span: Span::new(40, 43),
                }],
                modifiers: MOD_NONE,
                decorators: Vec::new(),
                span: Span::new(10, 50),
            })),
            span: Span::new(10, 50),
        };
        let stmt = Stmt {
            kind: StmtKind::Var(Box::new(VarStmt {
                kind: VarKind::Const,
                declarations: vec![VarDeclarator {
                    name: Pat {
                        kind: PatKind::Ident("C".to_string().into()),
                        span: Span::new(6, 7),
                    },
                    type_ann: None,
                    init: Some(Box::new(class_expr)),
                    full_start: 6,
                    binding_name_full_starts: Vec::new(),
                    span: Span::new(6, 50),
                    definite: false,
                }],
                modifiers: MOD_NONE,
            })),
            span: Span::new(0, 50),
        };
        let file = make_source("test.ts", "const C = class MyClass { val; };", vec![stmt]);
        let table = bind(&file);
        assert!(find_symbol(&table, "C").is_some());
        assert!(find_symbol(&table, "MyClass").is_some());
        let mc = table
            .get_symbol(find_symbol(&table, "MyClass").unwrap())
            .unwrap();
        assert_eq!(mc.flags & SYM_CLASS, SYM_CLASS);
    }

    // -----------------------------------------------------------------------
    // 9. Arrow function parameter binding
    // -----------------------------------------------------------------------

    #[test]
    fn test_arrow_function_params() {
        let arrow_expr = Expr {
            kind: ExprKind::Arrow(Box::new(ArrowFn {
                type_params: None,
                params: vec![
                    Param {
                        name: Pat {
                            kind: PatKind::Ident("x".to_string().into()),
                            span: Span::new(15, 16),
                        },
                        type_ann: None,
                        initializer: None,
                        dotdotdot: false,
                        optional: false,
                        modifiers: MOD_NONE,
                        decorators: Vec::new(),
                        span: Span::new(15, 16),
                    },
                    Param {
                        name: Pat {
                            kind: PatKind::Ident("y".to_string().into()),
                            span: Span::new(18, 19),
                        },
                        type_ann: None,
                        initializer: None,
                        dotdotdot: false,
                        optional: false,
                        modifiers: MOD_NONE,
                        decorators: Vec::new(),
                        span: Span::new(18, 19),
                    },
                ],
                return_type: None,
                body: ArrowBody::Expr(Box::new(Expr {
                    kind: ExprKind::Ident("x".to_string().into()),
                    span: Span::new(24, 25),
                })),
                is_async: false,
                span: Span::new(10, 25),
            })),
            span: Span::new(10, 25),
        };
        let stmt = Stmt {
            kind: StmtKind::Var(Box::new(VarStmt {
                kind: VarKind::Const,
                declarations: vec![VarDeclarator {
                    name: Pat {
                        kind: PatKind::Ident("add".to_string().into()),
                        span: Span::new(6, 9),
                    },
                    type_ann: None,
                    init: Some(Box::new(arrow_expr)),
                    full_start: 6,
                    binding_name_full_starts: Vec::new(),
                    span: Span::new(6, 25),
                    definite: false,
                }],
                modifiers: MOD_NONE,
            })),
            span: Span::new(0, 25),
        };
        let file = make_source("test.ts", "const add = (x, y) => x;", vec![stmt]);
        let table = bind(&file);
        assert!(find_symbol(&table, "add").is_some());
        assert!(find_symbol(&table, "x").is_some());
        assert!(find_symbol(&table, "y").is_some());
        let x = table.get_symbol(find_symbol(&table, "x").unwrap()).unwrap();
        assert_eq!(x.flags & SYM_PARAMETER, SYM_PARAMETER);
    }

    // -----------------------------------------------------------------------
    // 10. Object method shorthand
    // -----------------------------------------------------------------------

    #[test]
    fn test_object_method_shorthand_params() {
        let obj_expr = Expr {
            kind: ExprKind::ObjectLit(vec![ObjLitProp::Method(ObjMethod {
                name: PropName::Ident("foo".to_string().into(), Span::new(10, 13)),
                type_params: None,
                params: vec![Param {
                    name: Pat {
                        kind: PatKind::Ident("a".to_string().into()),
                        span: Span::new(14, 15),
                    },
                    type_ann: None,
                    initializer: None,
                    dotdotdot: false,
                    optional: false,
                    modifiers: MOD_NONE,
                    decorators: Vec::new(),
                    span: Span::new(14, 15),
                }],
                return_type: None,
                body: Vec::new(),
                is_generator: false,
                is_async: false,
                span: Span::new(10, 20),
            })]),
            span: Span::new(8, 22),
        };
        let stmt = Stmt {
            kind: StmtKind::Expr(Box::new(obj_expr)),
            span: Span::new(0, 22),
        };
        let file = make_source("test.ts", "const o = { foo(a) {} };", vec![stmt]);
        let table = bind(&file);
        assert!(find_symbol(&table, "a").is_some());
        let a = table.get_symbol(find_symbol(&table, "a").unwrap()).unwrap();
        assert_eq!(a.flags & SYM_PARAMETER, SYM_PARAMETER);
    }

    // -----------------------------------------------------------------------
    // 11. Nested namespace binding (dotted name)
    // -----------------------------------------------------------------------

    #[test]
    fn test_dotted_namespace() {
        let file = make_source(
            "test.ts",
            "namespace A.B.C { }",
            vec![make_module_decl(
                "A.B.C",
                Some(Vec::new()),
                Span::new(0, 19),
            )],
        );
        let table = bind(&file);
        assert!(find_symbol(&table, "A").is_some());
        assert!(find_symbol(&table, "B").is_some());
        assert!(find_symbol(&table, "C").is_some());
        for name in &["A", "B", "C"] {
            let sym = table
                .get_symbol(find_symbol(&table, name).unwrap())
                .unwrap();
            assert_eq!(sym.flags & SYM_MODULE, SYM_MODULE);
        }
    }

    #[test]
    fn test_nested_namespace_with_body() {
        let inner_var = make_var_stmt(VarKind::Var, "innerX", Span::new(30, 36));
        let file = make_source(
            "test.ts",
            "namespace A.B { var innerX = 1; }",
            vec![make_module_decl(
                "A.B",
                Some(vec![inner_var]),
                Span::new(0, 32),
            )],
        );
        let table = bind(&file);
        assert!(find_symbol(&table, "A").is_some());
        assert!(find_symbol(&table, "B").is_some());
        assert!(find_symbol(&table, "innerX").is_some());
    }

    // -----------------------------------------------------------------------
    // 12. Symbol baseline output format
    // -----------------------------------------------------------------------

    #[test]
    fn test_symbols_baseline_format() {
        let file = make_source(
            "test.ts",
            "var x: number = 1;",
            vec![make_var_stmt(VarKind::Var, "x", Span::new(4, 5))],
        );
        let table = bind(&file);
        let baseline = generate_symbols_baseline(&file, &table);
        assert!(baseline.contains("//// [test.ts] ////"));
        assert!(baseline.contains("=== test.ts ==="));
        assert!(baseline.contains("var x: number = 1;"));
        assert!(baseline.contains(">x : Symbol(x,"));
        assert!(baseline.contains("Decl(test.ts, 0, 4)"));
    }

    #[test]
    fn test_symbols_baseline_uses_variable_declaration_full_start() {
        let name_span = Span::new(4, 5);
        let mut statement = make_var_stmt(VarKind::Var, "x", name_span);
        let StmtKind::Var(var_statement) = &mut statement.kind else {
            unreachable!();
        };
        var_statement.declarations[0].full_start = 3;
        let file = make_source("test.ts", "var x=10;", vec![statement]);

        let baseline = generate_symbols_baseline(&file, &bind(&file));

        assert!(baseline.contains("Decl(test.ts, 0, 3)"));
        assert!(baseline.ends_with("\n\n"));
    }

    #[test]
    fn test_destructuring_symbols_use_binding_element_full_starts_and_name_spans() {
        let source = "let { color, key: renamed } = value;";
        let color_span = Span::new(6, 11);
        let renamed_span = Span::new(18, 25);
        let statement = Stmt {
            kind: StmtKind::Var(Box::new(VarStmt {
                kind: VarKind::Let,
                declarations: vec![VarDeclarator {
                    name: Pat {
                        kind: PatKind::Object(vec![
                            ObjPatProp::Shorthand("color".into(), color_span),
                            ObjPatProp::KeyValue(
                                PropName::Ident("key".into(), Span::new(13, 16)),
                                Pat {
                                    kind: PatKind::Ident("renamed".into()),
                                    span: renamed_span,
                                },
                            ),
                        ]),
                        span: Span::new(4, 27),
                    },
                    type_ann: None,
                    init: None,
                    full_start: 3,
                    binding_name_full_starts: vec![
                        BindingNameFullStart {
                            name_span: color_span,
                            full_start: 5,
                        },
                        BindingNameFullStart {
                            name_span: renamed_span,
                            full_start: 12,
                        },
                    ],
                    span: Span::new(4, 27),
                    definite: false,
                }],
                modifiers: MOD_NONE,
            })),
            span: Span::new(0, source.len() as u32),
        };
        let file = make_source("test.ts", source, vec![statement]);
        let table = bind(&file);

        let color = table
            .get_symbol(find_symbol(&table, "color").unwrap())
            .unwrap();
        assert_eq!(color.declarations[0].span, color_span);
        assert_eq!(color.declarations[0].full_start, 5);
        let renamed = table
            .get_symbol(find_symbol(&table, "renamed").unwrap())
            .unwrap();
        assert_eq!(renamed.declarations[0].span, renamed_span);
        assert_eq!(renamed.declarations[0].full_start, 12);

        let baseline = generate_symbols_baseline(&file, &table);
        assert!(baseline.contains(">color : Symbol(color, Decl(test.ts, 0, 5))"));
        assert!(baseline.contains(">renamed : Symbol(renamed, Decl(test.ts, 0, 12))"));
    }

    #[test]
    fn test_symbols_baseline_multiline() {
        let file = make_source(
            "test.ts",
            "var a = 1;\nvar b = 2;",
            vec![
                make_var_stmt(VarKind::Var, "a", Span::new(4, 5)),
                make_var_stmt(VarKind::Var, "b", Span::new(15, 16)),
            ],
        );
        let table = bind(&file);
        let baseline = generate_symbols_baseline(&file, &table);
        assert!(baseline.contains(">a : Symbol(a,"));
        assert!(baseline.contains(">b : Symbol(b,"));
    }

    #[test]
    fn test_symbols_baseline_function() {
        let file = make_source(
            "test.ts",
            "function greet() {}",
            vec![make_fn_decl("greet", Span::new(0, 19))],
        );
        let table = bind(&file);
        let baseline = generate_symbols_baseline(&file, &table);
        assert!(baseline.contains(">greet : Symbol(greet,"));
    }

    // -----------------------------------------------------------------------
    // 13. Export tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_export_tracking_var() {
        let file = make_source(
            "test.ts",
            "export var x = 1;",
            vec![make_export(make_var_stmt(
                VarKind::Var,
                "x",
                Span::new(11, 12),
            ))],
        );
        let table = bind(&file);
        let exports = get_exports(&file, &table);
        assert_eq!(exports.len(), 1);
        let sym = table.get_symbol(exports[0]).unwrap();
        assert_eq!(sym.name, "x");
        assert!(sym.flags & SYM_EXPORT != 0);
    }

    #[test]
    fn test_export_tracking_function() {
        let file = make_source(
            "test.ts",
            "export function myFn() {}",
            vec![make_export(make_fn_decl("myFn", Span::new(7, 25)))],
        );
        let table = bind(&file);
        let exports = get_exports(&file, &table);
        assert_eq!(exports.len(), 1);
        let sym = table.get_symbol(exports[0]).unwrap();
        assert_eq!(sym.name, "myFn");
        assert!(sym.flags & SYM_FUNCTION != 0);
        assert!(sym.flags & SYM_EXPORT != 0);
    }

    #[test]
    fn test_export_tracking_class() {
        let file = make_source(
            "test.ts",
            "export class MyClass {}",
            vec![make_export(make_class_decl(
                "MyClass",
                Vec::new(),
                Span::new(7, 23),
            ))],
        );
        let table = bind(&file);
        let exports = get_exports(&file, &table);
        assert_eq!(exports.len(), 1);
        assert_eq!(table.get_symbol(exports[0]).unwrap().name, "MyClass");
    }

    #[test]
    fn test_non_exported_not_in_exports() {
        let file = make_source(
            "test.ts",
            "var local = 1;\nexport var exported = 2;",
            vec![
                make_var_stmt(VarKind::Var, "local", Span::new(4, 9)),
                make_export(make_var_stmt(VarKind::Var, "exported", Span::new(26, 34))),
            ],
        );
        let table = bind(&file);
        let exports = get_exports(&file, &table);
        assert_eq!(exports.len(), 1);
        assert_eq!(table.get_symbol(exports[0]).unwrap().name, "exported");
    }

    // -----------------------------------------------------------------------
    // 14. Scope chain resolution (find_symbol / resolve)
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_symbol_basic() {
        let file = make_source(
            "test.ts",
            "var alpha = 1;",
            vec![make_var_stmt(VarKind::Var, "alpha", Span::new(4, 9))],
        );
        let table = bind(&file);
        assert!(find_symbol(&table, "alpha").is_some());
        assert!(find_symbol(&table, "beta").is_none());
    }

    #[test]
    fn test_resolve_basic() {
        let file = make_source(
            "test.ts",
            "var alpha = 1;",
            vec![make_var_stmt(VarKind::Var, "alpha", Span::new(4, 9))],
        );
        let table = bind(&file);
        assert!(resolve(&table, "alpha", 0).is_some());
        assert!(resolve(&table, "nonexistent", 0).is_none());
    }

    // -----------------------------------------------------------------------
    // 15. Enum merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_enum_merging() {
        let file = make_source(
            "test.ts",
            "enum Dir { Up }\nenum Dir { Down }",
            vec![
                make_enum_decl("Dir", vec!["Up"], Span::new(0, 15)),
                make_enum_decl("Dir", vec!["Down"], Span::new(16, 33)),
            ],
        );
        let table = bind(&file);
        let dir_sym = find_symbol(&table, "Dir").unwrap();
        let sym = table.get_symbol(dir_sym).unwrap();
        assert_eq!(sym.declarations.len(), 2);
        assert!(sym.flags & SYM_ENUM != 0);
    }

    // -----------------------------------------------------------------------
    // 16. Class + interface merge (TS allows this)
    // -----------------------------------------------------------------------

    #[test]
    fn test_class_interface_merge() {
        let file = make_source(
            "test.ts",
            "class Foo {}\ninterface Foo { x: number; }",
            vec![
                make_class_decl("Foo", Vec::new(), Span::new(0, 12)),
                make_interface_decl(
                    "Foo",
                    vec![make_property_sig("x", Span::new(24, 25))],
                    Span::new(13, 41),
                ),
            ],
        );
        let table = bind(&file);
        assert!(table.diagnostics.is_empty());
        let foo_sym = find_symbol(&table, "Foo").unwrap();
        let sym = table.get_symbol(foo_sym).unwrap();
        assert!(sym.flags & SYM_CLASS != 0);
        assert!(sym.flags & SYM_INTERFACE != 0);
        assert_eq!(sym.declarations.len(), 2);
    }

    // -----------------------------------------------------------------------
    // 17. Function overloads merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_function_overloads_merge() {
        let file = make_source(
            "test.ts",
            "function f(x: number): number;\nfunction f(x: string): string;\nfunction f(x: any): any { return x; }",
            vec![
                make_fn_decl("f", Span::new(0, 30)),
                make_fn_decl("f", Span::new(31, 61)),
                make_fn_decl("f", Span::new(62, 99)),
            ],
        );
        let table = bind(&file);
        assert!(table.diagnostics.is_empty());
        let f_sym = find_symbol(&table, "f").unwrap();
        let sym = table.get_symbol(f_sym).unwrap();
        assert_eq!(sym.declarations.len(), 3);
    }

    // -----------------------------------------------------------------------
    // 18. Block scoping isolation
    // -----------------------------------------------------------------------

    #[test]
    fn test_block_scoping_isolation() {
        // let x in two different blocks should not conflict
        let block1 = Stmt {
            kind: StmtKind::Block(vec![make_var_stmt(VarKind::Let, "x", Span::new(2, 3))]),
            span: Span::new(0, 15),
        };
        let block2 = Stmt {
            kind: StmtKind::Block(vec![make_var_stmt(VarKind::Let, "x", Span::new(20, 21))]),
            span: Span::new(16, 30),
        };
        let file = make_source(
            "test.ts",
            "{ let x = 1; } { let x = 2; }",
            vec![block1, block2],
        );
        let table = bind(&file);
        assert!(table.diagnostics.is_empty());
    }

    // -----------------------------------------------------------------------
    // 19. Position to symbol mapping
    // -----------------------------------------------------------------------

    #[test]
    fn test_position_to_symbol_mapping() {
        let file = make_source(
            "test.ts",
            "var x = 1;",
            vec![make_var_stmt(VarKind::Var, "x", Span::new(4, 5))],
        );
        let table = bind(&file);
        assert!(table.position_to_symbol.contains_key(&4));
        let sym_id = table.position_to_symbol[&4];
        assert_eq!(table.get_symbol(sym_id).unwrap().name, "x");
    }

    #[test]
    fn test_scope_graph_tracks_nested_function_references() {
        let source = "function outer(input) { const value = input; return () => value; }";
        let value_decl = Stmt {
            kind: StmtKind::Var(Box::new(VarStmt {
                kind: VarKind::Const,
                declarations: vec![VarDeclarator {
                    name: make_ident_pat("value", Span::new(30, 35)),
                    type_ann: None,
                    init: Some(Box::new(make_ident_expr("input", Span::new(38, 43)))),
                    full_start: 30,
                    binding_name_full_starts: Vec::new(),
                    span: Span::new(30, 43),
                    definite: false,
                }],
                modifiers: MOD_NONE,
            })),
            span: Span::new(24, 44),
        };
        let return_stmt = Stmt {
            kind: StmtKind::Return(Some(Box::new(Expr {
                kind: ExprKind::Arrow(Box::new(ArrowFn {
                    type_params: None,
                    params: Vec::new(),
                    return_type: None,
                    body: ArrowBody::Expr(Box::new(make_ident_expr("value", Span::new(58, 63)))),
                    is_async: false,
                    span: Span::new(52, 63),
                })),
                span: Span::new(52, 63),
            }))),
            span: Span::new(45, 63),
        };
        let file = make_source(
            "test.ts",
            source,
            vec![Stmt {
                kind: StmtKind::FnDecl(Box::new(FnDecl {
                    name: Some("outer".to_string()),
                    name_span: Some(Span::new(9, 14)),
                    type_params: None,
                    params: vec![make_param("input", Span::new(15, 20), None)],
                    return_type: None,
                    body: Some(vec![value_decl, return_stmt]),
                    modifiers: MOD_NONE,
                    is_generator: false,
                    is_async: false,
                    decorators: Vec::new(),
                    span: Span::new(0, source.len() as u32),
                })),
                span: Span::new(0, source.len() as u32),
            }],
        );

        let table = bind(&file);
        assert_eq!(table.scope_graph.scopes.len(), 3);
        assert_eq!(table.scope_graph.scopes[0].kind, ScopeKind::Global);

        let value_sym = find_symbol(&table, "value").unwrap();
        assert_eq!(table.scope_graph.binding_scope(value_sym), Some(1));

        let arrow_scope = table.scope_graph.innermost_scope_at(58).unwrap();
        assert_eq!(
            table.scope_graph.scope(arrow_scope).unwrap().kind,
            ScopeKind::Function
        );
        assert_eq!(resolve(&table, "value", arrow_scope), Some(value_sym));

        let value_ref = table.scope_graph.reference_at(58).unwrap();
        assert_eq!(value_ref.kind, ReferenceKind::Value);
        assert_eq!(value_ref.scope_id, arrow_scope);
        assert_eq!(value_ref.symbol_id, value_sym);
    }

    #[test]
    fn test_scope_graph_marks_type_references() {
        let source = "type Box = string; function render(value: Box) { return value; }";
        let box_type_ref = TypeNode {
            kind: TypeNodeKind::Reference(Box::new(TypeRef {
                name: Box::new(make_ident_expr("Box", Span::new(41, 44))),
                type_args: None,
            })),
            span: Span::new(41, 44),
        };
        let file = make_source(
            "test.ts",
            source,
            vec![
                Stmt {
                    kind: StmtKind::TypeAlias(Box::new(TypeAliasDecl {
                        name: "Box".to_string(),
                        name_span: Some(Span::new(5, 8)),
                        type_params: None,
                        type_ann: TypeNode {
                            kind: TypeNodeKind::Keyword(KeywordTypeKind::String),
                            span: Span::new(11, 17),
                        },
                        modifiers: MOD_NONE,
                        span: Span::new(0, 17),
                    })),
                    span: Span::new(0, 17),
                },
                Stmt {
                    kind: StmtKind::FnDecl(Box::new(FnDecl {
                        name: Some("render".to_string()),
                        name_span: Some(Span::new(28, 34)),
                        type_params: None,
                        params: vec![make_param("value", Span::new(35, 44), Some(box_type_ref))],
                        return_type: None,
                        body: Some(vec![Stmt {
                            kind: StmtKind::Return(Some(Box::new(make_ident_expr(
                                "value",
                                Span::new(55, 60),
                            )))),
                            span: Span::new(48, 60),
                        }]),
                        modifiers: MOD_NONE,
                        is_generator: false,
                        is_async: false,
                        decorators: Vec::new(),
                        span: Span::new(19, source.len() as u32),
                    })),
                    span: Span::new(19, source.len() as u32),
                },
            ],
        );

        let table = bind(&file);
        let box_sym = find_symbol(&table, "Box").unwrap();
        assert_eq!(table.scope_graph.binding_scope(box_sym), Some(0));

        let type_ref = table.scope_graph.reference_at(41).unwrap();
        assert_eq!(type_ref.symbol_id, box_sym);
        assert_eq!(type_ref.kind, ReferenceKind::Type);

        let type_refs: Vec<_> = table
            .scope_graph
            .references_for_symbol(box_sym)
            .filter(|reference| reference.kind == ReferenceKind::Type)
            .collect();
        assert_eq!(type_refs.len(), 1);
    }

    // -----------------------------------------------------------------------
    // 20. Declaration list tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_declaration_file_and_span() {
        let file = make_source(
            "myFile.ts",
            "var hello = 'world';",
            vec![make_var_stmt(VarKind::Var, "hello", Span::new(4, 9))],
        );
        let table = bind(&file);
        let sym = table.get_symbol(0).unwrap();
        assert_eq!(sym.declarations.len(), 1);
        assert_eq!(sym.declarations[0].file_name, "myFile.ts");
        assert_eq!(sym.declarations[0].span, Span::new(4, 9));
    }

    // -----------------------------------------------------------------------
    // 21. Type alias binding
    // -----------------------------------------------------------------------

    #[test]
    fn test_type_alias_binding() {
        let stmt = Stmt {
            kind: StmtKind::TypeAlias(Box::new(TypeAliasDecl {
                name: "MyType".to_string(),
                name_span: None,
                type_params: None,
                type_ann: TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::String),
                    span: Span::new(15, 21),
                },
                modifiers: MOD_NONE,
                span: Span::new(0, 22),
            })),
            span: Span::new(0, 22),
        };
        let file = make_source("test.ts", "type MyType = string;", vec![stmt]);
        let table = bind(&file);
        let sym = find_symbol(&table, "MyType").unwrap();
        assert_eq!(
            table.get_symbol(sym).unwrap().flags & SYM_TYPE_ALIAS,
            SYM_TYPE_ALIAS
        );
    }

    // -----------------------------------------------------------------------
    // 22. Enum member binding
    // -----------------------------------------------------------------------

    #[test]
    fn test_enum_members_bound() {
        let file = make_source(
            "test.ts",
            "enum Color { Red, Green, Blue }",
            vec![make_enum_decl(
                "Color",
                vec!["Red", "Green", "Blue"],
                Span::new(0, 31),
            )],
        );
        let table = bind(&file);
        let color_sym = find_symbol(&table, "Color").unwrap();
        let sym = table.get_symbol(color_sym).unwrap();
        assert!(sym.members.contains_key("Red"));
        assert!(sym.members.contains_key("Green"));
        assert!(sym.members.contains_key("Blue"));
    }

    // -----------------------------------------------------------------------
    // 23. Multiple symbols in one file
    // -----------------------------------------------------------------------

    #[test]
    fn test_multiple_symbols() {
        let file = make_source(
            "test.ts",
            "var a = 1;\nfunction b() {}\nclass C {}",
            vec![
                make_var_stmt(VarKind::Var, "a", Span::new(4, 5)),
                make_fn_decl("b", Span::new(11, 26)),
                make_class_decl("C", Vec::new(), Span::new(27, 37)),
            ],
        );
        let table = bind(&file);
        assert!(find_symbol(&table, "a").is_some());
        assert!(find_symbol(&table, "b").is_some());
        assert!(find_symbol(&table, "C").is_some());
    }

    // -----------------------------------------------------------------------
    // 24. For-loop scoping
    // -----------------------------------------------------------------------

    #[test]
    fn test_for_loop_scoping() {
        let for_stmt = Stmt {
            kind: StmtKind::For(Box::new(ForStmt {
                init: Some(ForInit::Var(VarStmt {
                    kind: VarKind::Let,
                    declarations: vec![VarDeclarator {
                        name: Pat {
                            kind: PatKind::Ident("i".to_string().into()),
                            span: Span::new(9, 10),
                        },
                        type_ann: None,
                        init: None,
                        full_start: 9,
                        binding_name_full_starts: Vec::new(),
                        span: Span::new(9, 10),
                        definite: false,
                    }],
                    modifiers: MOD_NONE,
                })),
                test: None,
                update: None,
                body: Box::new(Stmt {
                    kind: StmtKind::Empty,
                    span: Span::new(20, 21),
                }),
            })),
            span: Span::new(0, 21),
        };
        let file = make_source("test.ts", "for (let i = 0;;) {}", vec![for_stmt]);
        let table = bind(&file);
        assert!(find_symbol(&table, "i").is_some());
    }

    // -----------------------------------------------------------------------
    // 25. Interface with no members
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_interface() {
        let file = make_source(
            "test.ts",
            "interface Empty {}",
            vec![make_interface_decl("Empty", Vec::new(), Span::new(0, 18))],
        );
        let table = bind(&file);
        let sym = find_symbol(&table, "Empty").unwrap();
        assert_eq!(
            table.get_symbol(sym).unwrap().flags & SYM_INTERFACE,
            SYM_INTERFACE
        );
        assert!(table.get_symbol(sym).unwrap().members.is_empty());
    }

    // -----------------------------------------------------------------------
    // 26. Module with exports
    // -----------------------------------------------------------------------

    #[test]
    fn test_module_export_collection() {
        let file = make_source(
            "test.ts",
            "namespace M { export var x = 1; export function f() {} }",
            vec![make_module_decl(
                "M",
                Some(vec![
                    make_export(make_var_stmt(VarKind::Var, "x", Span::new(25, 26))),
                    make_export(make_fn_decl("f", Span::new(35, 55))),
                ]),
                Span::new(0, 57),
            )],
        );
        let table = bind(&file);
        let m_sym = find_symbol(&table, "M").unwrap();
        let sym = table.get_symbol(m_sym).unwrap();
        assert!(sym.exports.contains_key("x"));
        assert!(sym.exports.contains_key("f"));
    }

    // -----------------------------------------------------------------------
    // 27. Var + let conflict
    // -----------------------------------------------------------------------

    #[test]
    fn test_var_let_same_scope_error() {
        // var then let in same scope => error (block-scoped conflicts)
        let file = make_source(
            "test.ts",
            "var x = 1;\nlet x = 2;",
            vec![
                make_var_stmt(VarKind::Var, "x", Span::new(4, 5)),
                make_var_stmt(VarKind::Let, "x", Span::new(15, 16)),
            ],
        );
        let table = bind(&file);
        // This should produce a diagnostic since let is block-scoped
        // and conflicts with existing var
        assert!(!table.diagnostics.is_empty());
    }

    // -----------------------------------------------------------------------
    // 28. get_exports on file with no exports
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_exports_none() {
        let file = make_source(
            "test.ts",
            "var x = 1;",
            vec![make_var_stmt(VarKind::Var, "x", Span::new(4, 5))],
        );
        let table = bind(&file);
        let exports = get_exports(&file, &table);
        assert!(exports.is_empty());
    }

    // -----------------------------------------------------------------------
    // 29. Baseline with no symbols
    // -----------------------------------------------------------------------

    #[test]
    fn test_baseline_empty_file() {
        let file = make_source("empty.ts", "", Vec::new());
        let table = bind(&file);
        let baseline = generate_symbols_baseline(&file, &table);
        assert!(baseline.contains("//// [empty.ts] ////"));
        assert!(baseline.contains("=== empty.ts ==="));
    }

    // -----------------------------------------------------------------------
    // 30. Try-catch parameter binding
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // 31. Overload signature recording
    // -----------------------------------------------------------------------

    /// Helper: create a function declaration without a body (overload signature).
    fn make_fn_overload(
        name: &str,
        params: Vec<Param>,
        return_type: Option<TypeNode>,
        span: Span,
    ) -> Stmt {
        Stmt {
            kind: StmtKind::FnDecl(Box::new(FnDecl {
                name: Some(name.to_string()),
                name_span: None,
                type_params: None,
                params,
                return_type,
                body: None,
                modifiers: MOD_NONE,
                is_generator: false,
                is_async: false,
                decorators: Vec::new(),
                span,
            })),
            span,
        }
    }

    fn make_number_param(name: &str, span: Span) -> Param {
        Param {
            name: Pat {
                kind: PatKind::Ident(name.to_string().into()),
                span,
            },
            type_ann: Some(TypeNode {
                kind: TypeNodeKind::Keyword(KeywordTypeKind::Number),
                span,
            }),
            initializer: None,
            dotdotdot: false,
            optional: false,
            modifiers: MOD_NONE,
            decorators: Vec::new(),
            span,
        }
    }

    fn make_string_param(name: &str, span: Span) -> Param {
        Param {
            name: Pat {
                kind: PatKind::Ident(name.to_string().into()),
                span,
            },
            type_ann: Some(TypeNode {
                kind: TypeNodeKind::Keyword(KeywordTypeKind::String),
                span,
            }),
            initializer: None,
            dotdotdot: false,
            optional: false,
            modifiers: MOD_NONE,
            decorators: Vec::new(),
            span,
        }
    }

    fn number_type_node(span: Span) -> TypeNode {
        TypeNode {
            kind: TypeNodeKind::Keyword(KeywordTypeKind::Number),
            span,
        }
    }

    fn string_type_node(span: Span) -> TypeNode {
        TypeNode {
            kind: TypeNodeKind::Keyword(KeywordTypeKind::String),
            span,
        }
    }

    #[test]
    fn test_overload_signatures_recorded_for_functions() {
        // function f(x: number): number;   // overload 1
        // function f(x: string): string;   // overload 2
        // function f(x: any): any { ... }  // implementation
        let file = make_source(
            "test.ts",
            "function f(x: number): number;\nfunction f(x: string): string;\nfunction f(x: any): any { return x; }",
            vec![
                make_fn_overload(
                    "f",
                    vec![make_number_param("x", Span::new(11, 12))],
                    Some(number_type_node(Span::new(23, 29))),
                    Span::new(0, 30),
                ),
                make_fn_overload(
                    "f",
                    vec![make_string_param("x", Span::new(42, 43))],
                    Some(string_type_node(Span::new(54, 60))),
                    Span::new(31, 61),
                ),
                make_fn_decl("f", Span::new(62, 99)),
            ],
        );
        let table = bind(&file);
        assert!(table.diagnostics.is_empty());

        let f_sym = find_symbol(&table, "f").unwrap();
        let overloads = table.get_overload_signatures(f_sym);
        assert_eq!(overloads.len(), 2, "Expected 2 overload signatures");
        assert_eq!(overloads[0].params.len(), 1);
        assert_eq!(overloads[0].params[0].name, "x");
        assert!(overloads[0].return_type.is_some());
        assert_eq!(overloads[1].params.len(), 1);
        assert_eq!(overloads[1].params[0].name, "x");
    }

    #[test]
    fn test_overload_signatures_not_recorded_for_implementation() {
        // A function with a body should NOT be recorded as an overload
        let file = make_source(
            "test.ts",
            "function g(x: number): number { return x; }",
            vec![make_fn_decl("g", Span::new(0, 44))],
        );
        let table = bind(&file);
        let g_sym = find_symbol(&table, "g").unwrap();
        let overloads = table.get_overload_signatures(g_sym);
        assert!(
            overloads.is_empty(),
            "Implementation should not be recorded as overload"
        );
    }

    #[test]
    fn test_overload_single_signature() {
        // Single overload + implementation
        let file = make_source(
            "test.ts",
            "function h(x: number): number;\nfunction h(x: any): any { return x; }",
            vec![
                make_fn_overload(
                    "h",
                    vec![make_number_param("x", Span::new(11, 12))],
                    Some(number_type_node(Span::new(23, 29))),
                    Span::new(0, 30),
                ),
                make_fn_decl("h", Span::new(31, 68)),
            ],
        );
        let table = bind(&file);
        let h_sym = find_symbol(&table, "h").unwrap();
        let overloads = table.get_overload_signatures(h_sym);
        assert_eq!(overloads.len(), 1);
    }

    #[test]
    fn test_get_overload_signatures_nonexistent_symbol() {
        let table = SymbolTable::new();
        let overloads = table.get_overload_signatures(999);
        assert!(overloads.is_empty());
    }

    #[test]
    fn test_try_catch_param_binding() {
        let try_stmt = Stmt {
            kind: StmtKind::Try(Box::new(TryStmt {
                block: Vec::new(),
                handler: Some(CatchClause {
                    param: Some(Pat {
                        kind: PatKind::Ident("err".to_string().into()),
                        span: Span::new(20, 23),
                    }),
                    param_type: None,
                    body: Vec::new(),
                    span: Span::new(15, 30),
                }),
                finalizer: None,
            })),
            span: Span::new(0, 30),
        };
        let file = make_source("test.ts", "try {} catch (err) {}", vec![try_stmt]);
        let table = bind(&file);
        assert!(find_symbol(&table, "err").is_some());
    }

    // -----------------------------------------------------------------------
    // Module augmentation helpers
    // -----------------------------------------------------------------------

    fn make_string_module_decl(
        name: &str,
        body: Option<Vec<Stmt>>,
        modifiers: ModifierFlags,
        span: Span,
    ) -> Stmt {
        Stmt {
            kind: StmtKind::ModuleDecl(Box::new(ModuleDecl {
                name: ModuleName::String(name.to_string()),
                name_span: None,
                body: body.map(|stmts| ModuleBody::Block(stmts)),
                modifiers,
                span,
            })),
            span,
        }
    }

    fn make_global_decl(body: Vec<Stmt>, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::ModuleDecl(Box::new(ModuleDecl {
                name: ModuleName::Ident("global".to_string()),
                name_span: None,
                body: Some(ModuleBody::Block(body)),
                modifiers: MOD_DECLARE,
                span,
            })),
            span,
        }
    }

    // -----------------------------------------------------------------------
    // 32. Module augmentation
    // -----------------------------------------------------------------------

    #[test]
    fn test_module_augmentation_creates_augmentation_symbol() {
        // declare module './path' { export interface Foo { bar: string; } }
        let inner = make_export(make_interface_decl(
            "Foo",
            vec![make_property_sig("bar", Span::new(50, 53))],
            Span::new(40, 60),
        ));
        let file = make_source(
            "augment.ts",
            "declare module './path' { export interface Foo { bar: string; } }",
            vec![make_string_module_decl(
                "./path",
                Some(vec![inner]),
                MOD_DECLARE,
                Span::new(0, 65),
            )],
        );
        let table = bind(&file);

        // The module symbol should be marked as an augmentation
        let mod_sym = find_symbol(&table, "./path").unwrap();
        let sym = table.get_symbol(mod_sym).unwrap();
        assert!(sym.flags & SYM_MODULE != 0);
        assert!(sym.flags & SYM_MODULE_AUGMENTATION != 0);

        // Should be tracked in module_augmentations
        assert!(table.module_augmentations.contains_key("./path"));
        assert_eq!(table.module_augmentations["./path"].len(), 1);

        // The augmentation module should have Foo in its exports
        assert!(sym.exports.contains_key("Foo"));
    }

    // -----------------------------------------------------------------------
    // 33. Global augmentation
    // -----------------------------------------------------------------------

    #[test]
    fn test_global_augmentation() {
        // declare global { interface Array<T> { customMethod(): void; } }
        let inner = make_interface_decl(
            "Array",
            vec![make_method_sig("customMethod", Span::new(40, 52))],
            Span::new(20, 60),
        );
        let file = make_source(
            "global-aug.ts",
            "declare global { interface Array<T> { customMethod(): void; } }",
            vec![make_global_decl(vec![inner], Span::new(0, 63))],
        );
        let table = bind(&file);

        // Array should be bound in the global scope (findable at top level)
        assert!(find_symbol(&table, "Array").is_some());

        // Should be tracked as a global augmentation symbol
        assert!(!table.global_augmentation_symbols.is_empty());

        // The "global" module symbol should have the augmentation flag
        let global_sym = table.symbols.iter().find(|s| s.name == "global").unwrap();
        assert!(global_sym.flags & SYM_GLOBAL_AUGMENTATION != 0);
    }

    #[test]
    fn test_global_augmentation_adds_to_root() {
        // declare global { function myGlobalFn(): void; }
        let inner = make_fn_decl("myGlobalFn", Span::new(20, 45));
        let file = make_source(
            "global-aug.ts",
            "declare global { function myGlobalFn(): void; }",
            vec![make_global_decl(vec![inner], Span::new(0, 48))],
        );
        let table = bind(&file);

        // myGlobalFn should be declared at the top-level
        let sym = find_symbol(&table, "myGlobalFn");
        assert!(sym.is_some());
        let sym = table.get_symbol(sym.unwrap()).unwrap();
        assert!(sym.flags & SYM_FUNCTION != 0);
    }

    // -----------------------------------------------------------------------
    // 34. Ambient module declarations
    // -----------------------------------------------------------------------

    #[test]
    fn test_ambient_module_declaration() {
        // declare module 'lodash' { export function chunk<T>(arr: T[]): T[][]; }
        let inner = make_export(make_fn_decl("chunk", Span::new(30, 50)));
        let file = make_source(
            "lodash.d.ts",
            "declare module 'lodash' { export function chunk<T>(arr: T[]): T[][]; }",
            vec![make_string_module_decl(
                "lodash",
                Some(vec![inner]),
                MOD_DECLARE,
                Span::new(0, 70),
            )],
        );
        let table = bind(&file);

        // Should be registered as an ambient module
        assert!(table.ambient_modules.contains_key("lodash"));

        let mod_sym_id = table.ambient_modules["lodash"];
        let mod_sym = table.get_symbol(mod_sym_id).unwrap();
        assert!(mod_sym.flags & SYM_MODULE != 0);
        // Should NOT have the augmentation flag
        assert!(mod_sym.flags & SYM_MODULE_AUGMENTATION == 0);

        // The module should have chunk in its exports
        assert!(mod_sym.exports.contains_key("chunk"));
    }

    // -----------------------------------------------------------------------
    // 35. Wildcard module declarations
    // -----------------------------------------------------------------------

    #[test]
    fn test_wildcard_module_declaration() {
        // declare module '*.css' { const content: string; export default content; }
        let inner = make_export(make_var_stmt(VarKind::Const, "content", Span::new(30, 37)));
        let file = make_source(
            "css.d.ts",
            "declare module '*.css' { const content: string; export default content; }",
            vec![make_string_module_decl(
                "*.css",
                Some(vec![inner]),
                MOD_DECLARE,
                Span::new(0, 73),
            )],
        );
        let table = bind(&file);

        // Should be registered as a wildcard module
        assert_eq!(table.wildcard_modules.len(), 1);
        assert_eq!(table.wildcard_modules[0].0, "*.css");

        // resolve_ambient_module should match
        assert!(resolve_ambient_module(&table, "styles.css").is_some());
        assert!(resolve_ambient_module(&table, "foo/bar.css").is_some());
        assert!(resolve_ambient_module(&table, "foo.js").is_none());
    }

    #[test]
    fn test_wildcard_matches_fn() {
        assert!(wildcard_matches("*.css", "foo.css"));
        assert!(wildcard_matches("*.css", "bar/baz.css"));
        assert!(!wildcard_matches("*.css", "foo.js"));
        assert!(wildcard_matches("*.module.css", "styles.module.css"));
        assert!(!wildcard_matches("*.module.css", "styles.css"));
        assert!(wildcard_matches("image!*", "image!foo.png"));
    }

    // -----------------------------------------------------------------------
    // 36. Cross-file module augmentation merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_module_augmentations() {
        // Target: a module with an exported interface
        let target_iface = make_export(make_interface_decl(
            "Foo",
            vec![make_property_sig("x", Span::new(30, 31))],
            Span::new(20, 40),
        ));
        let target_file = make_source(
            "./path.ts",
            "export interface Foo { x: number; }",
            vec![target_iface],
        );
        let mut target_table = bind(&target_file);

        // Augmenting: declare module './path' { export interface Bar { y: string; } }
        let aug_iface = make_export(make_interface_decl(
            "Bar",
            vec![make_property_sig("y", Span::new(70, 71))],
            Span::new(60, 80),
        ));
        let aug_file = make_source(
            "augment.ts",
            "declare module './path' { export interface Bar { y: string; } }",
            vec![make_string_module_decl(
                "./path",
                Some(vec![aug_iface]),
                MOD_DECLARE,
                Span::new(0, 63),
            )],
        );
        let aug_table = bind(&aug_file);

        // We need "./path" to exist in the target table as a module symbol
        let path_id = target_table.add_symbol("./path".to_string(), SYM_MODULE);
        // Add Foo as an export of the module
        let foo_id = find_symbol(&target_table, "Foo").unwrap();
        if let Some(sym) = target_table.get_symbol_mut(path_id) {
            sym.exports.insert("Foo".to_string(), foo_id);
        }

        let merged = apply_module_augmentations(&mut target_table, &aug_table);
        assert!(merged > 0);

        // The target module should now also have Bar
        let target_mod = target_table.get_symbol(path_id).unwrap();
        assert!(target_mod.exports.contains_key("Bar"));
    }

    // -----------------------------------------------------------------------
    // 37. Cross-file global augmentation merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_global_augmentations() {
        // Target: a file with some existing symbols
        let target_file = make_source(
            "target.ts",
            "var existing = 1;",
            vec![make_var_stmt(VarKind::Var, "existing", Span::new(4, 12))],
        );
        let mut target_table = bind(&target_file);

        // Augmenting: declare global { function newGlobal(): void; }
        let inner = make_fn_decl("newGlobal", Span::new(20, 42));
        let aug_file = make_source(
            "augment.ts",
            "declare global { function newGlobal(): void; }",
            vec![make_global_decl(vec![inner], Span::new(0, 47))],
        );
        let aug_table = bind(&aug_file);

        let merged = apply_global_augmentations(&mut target_table, &aug_table);
        assert!(merged > 0);

        // target_table should now have the newGlobal symbol
        assert!(find_symbol(&target_table, "newGlobal").is_some());
    }

    // -----------------------------------------------------------------------
    // 38. Cross-file namespace merging via augmentation
    // -----------------------------------------------------------------------

    #[test]
    fn test_namespace_augmentation_cross_file() {
        // File 1: namespace Foo { export var x = 1; }
        let file1 = make_source(
            "file1.ts",
            "namespace Foo { export var x = 1; }",
            vec![make_module_decl(
                "Foo",
                Some(vec![make_export(make_var_stmt(
                    VarKind::Var,
                    "x",
                    Span::new(27, 28),
                ))]),
                Span::new(0, 35),
            )],
        );
        let table1 = bind(&file1);

        // File 2: namespace Foo { export var y = 2; }
        let file2 = make_source(
            "file2.ts",
            "namespace Foo { export var y = 2; }",
            vec![make_module_decl(
                "Foo",
                Some(vec![make_export(make_var_stmt(
                    VarKind::Var,
                    "y",
                    Span::new(27, 28),
                ))]),
                Span::new(0, 35),
            )],
        );
        let table2 = bind(&file2);

        // Both should have a Foo symbol with SYM_MODULE
        let foo1 = find_symbol(&table1, "Foo").unwrap();
        let foo2 = find_symbol(&table2, "Foo").unwrap();
        assert!(table1.get_symbol(foo1).unwrap().flags & SYM_MODULE != 0);
        assert!(table2.get_symbol(foo2).unwrap().flags & SYM_MODULE != 0);

        // Both modules should have their respective exports
        let sym1 = table1.get_symbol(foo1).unwrap();
        let sym2 = table2.get_symbol(foo2).unwrap();
        assert!(sym1.exports.contains_key("x"));
        assert!(sym2.exports.contains_key("y"));
    }

    // -----------------------------------------------------------------------
    // 39. Ambient module resolution with wildcard
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_ambient_module_exact() {
        let inner = make_export(make_fn_decl("parse", Span::new(30, 40)));
        let file = make_source(
            "types.d.ts",
            "declare module 'json5' { export function parse(): any; }",
            vec![make_string_module_decl(
                "json5",
                Some(vec![inner]),
                MOD_DECLARE,
                Span::new(0, 56),
            )],
        );
        let table = bind(&file);

        // Exact match should work
        let resolved = resolve_ambient_module(&table, "json5");
        assert!(resolved.is_some());

        // Non-matching should fail
        assert!(resolve_ambient_module(&table, "json6").is_none());
    }

    #[test]
    fn test_resolve_ambient_module_wildcard() {
        let inner = make_var_stmt(VarKind::Const, "url", Span::new(30, 33));
        let file = make_source(
            "images.d.ts",
            "declare module '*.png' { const url: string; export default url; }",
            vec![make_string_module_decl(
                "*.png",
                Some(vec![inner]),
                MOD_DECLARE,
                Span::new(0, 65),
            )],
        );
        let table = bind(&file);

        assert!(resolve_ambient_module(&table, "logo.png").is_some());
        assert!(resolve_ambient_module(&table, "assets/icon.png").is_some());
        assert!(resolve_ambient_module(&table, "logo.jpg").is_none());
    }

    #[test]
    fn test_bind_deep_binary_expression_without_stack_overflow() {
        let mut expr = Expr {
            kind: ExprKind::NumLit("0".to_string().into()),
            span: Span::new(0, 1),
        };
        for i in 1..20_000u32 {
            expr = Expr {
                span: Span::new(0, i + 1),
                kind: ExprKind::Binary(BinaryExpr {
                    left: Box::new(expr),
                    op: BinaryOp::Add,
                    right: Box::new(Expr {
                        kind: ExprKind::NumLit(i.to_string().into()),
                        span: Span::new(i, i + 1),
                    }),
                }),
            };
        }

        let file = make_source(
            "stress.ts",
            "0",
            vec![Stmt {
                kind: StmtKind::Expr(Box::new(expr)),
                span: Span::new(0, 1),
            }],
        );

        let table = bind(&file);
        assert!(table.diagnostics.is_empty());
    }
}
