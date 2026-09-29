# ts-rs: Core Architecture Primitives Design

**Status**: Architecture Design Phase 0 — Emitter at 90.9% baseline parity (5,934/6,529 compiler, 4,620/5,907 conformance)
**Date**: 2026-03-06 (last updated)
**Objective**: Define the 5 core primitives and crate layout for the TypeScript analysis platform.

---

## Executive Summary

The ts-rs project is pivoting from a direct TypeScript compiler port to a **TypeScript analysis platform** with a compatibility layer. The current codebase has solid foundational pieces (parser, AST, basic type checker, symbol table) but lacks the explicit data structures needed for queryability, incrementalism, and extensibility.

This document designs:
1. **5 Core Primitives** – Explicit data structures for constraints, types, CFG, hashing, and queries
2. **New Crate Layout** – Reorganized dependency graph supporting extensibility
3. **Rust Type Definitions** – Concrete data structures and traits
4. **Migration Path** – How to refactor existing code without breaking Phase 0 parity

---

## Part 1: Current State Assessment

### Existing Crates (Good Foundation)

| Crate | Purpose | Strengths | Gaps |
|-------|---------|-----------|------|
| **tsc_rs_ast** | AST, spans, diagnostics | Good AST shape, comment tracking | No explicit graph nodes |
| **tsc_rs_scanner** | Lexical analysis | Complete token set | Stable, no changes needed |
| **tsc_rs_parser** | Syntax analysis | Reasonably complete | Passes most baseline tests |
| **tsc_rs_symbols** | Binder, symbol table | Per-file symbol tables | No cross-file linking, flags only |
| **tsc_rs_types** | Type checker, type ops | Basic inference works | Ad-hoc constraint logic, Type enum only |
| **tsc_rs_resolver** | Module resolution | Both Node/Classic algorithms | Minimal integration with type system |
| **tsc_rs_emitter** | Code generation | JS/sourcemap/d.ts emit, 91% baseline parity | No query interface |
| **tsc_rs_project** | Multi-file orchestration | Correct pipeline order | Orchestration only, no graph storage |
| **tsc_rs_server** | LSP server | Full LSP: diagnostics, hover, go-to-def, completions, references, rename, formatting | Per-file only, no cross-project graph |
| **tsc_rs_harness** | Test infrastructure | Baseline classification | Bootstrap-only |
| **tsc_rs_cli** | Binary entry point | Minimal coverage | No persistence |

### Key Bottlenecks

1. **Type System Implicitness**
   - Types are `enum Type` with no stable identity
   - Constraints embedded in type-checking logic (not explicit)
   - No way to ask "why is this type what it is?"

2. **Control Flow**
   - `CondFacts` in `tsc_rs_types::control_flow` exists but is ad-hoc
   - CFG reconstructed per-check, not stored
   - No way to replay narrowing paths

3. **Symbol Identity**
   - `SymbolId` is `u32` index into per-file symbol table
   - No cross-file symbol identity
   - Symbol merging happens at type-check time implicitly

4. **No Query API**
   - No way to ask: "what is the type at this location?"
   - No way to trace why a type is inferred
   - LSP is stubbed, not functional

5. **No Incremental State**
   - All type checking starts from scratch
   - No hashing of public interfaces
   - Perfect for Phase 0 but not scalable

---

## Part 2: The 5 Core Primitives

### Primitive 1: Explicit Constraint Solver

**Problem**: Type inference constraints are implicit in type-checking code. You cannot ask "what constraints determined this type?"

**Design**:

```rust
// New crate: tsc_rs_constraints

/// A constraint node in the constraint graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstraintId(u32);

/// The constraint solver context – stores all constraints and solutions
pub struct ConstraintSolver {
    constraints: Vec<Constraint>,
    solutions: Vec<Option<Type>>, // solutions[id] = inferred type for constraint id
    constraint_map: HashMap<ConstraintId, usize>, // id -> index
    next_id: u32,
}

/// A single constraint in the system
#[derive(Debug, Clone)]
pub enum Constraint {
    /// TypeA must be assignable to TypeB
    /// Source: where this constraint came from (e.g. "argument to param at foo.ts:5:12")
    Assignable {
        source: ConstraintSource,
        target: Box<Type>,
        source_type: Box<Type>,
    },

    /// TypeA extends TypeB (used in conditional type resolution)
    Extends {
        source: ConstraintSource,
        check_type: Box<Type>,
        extends_type: Box<Type>,
    },

    /// Generic instantiation: replace TypeParameter(name) with concrete_type
    Instantiation {
        source: ConstraintSource,
        param_name: String,
        concrete_type: Box<Type>,
    },

    /// Inferred type must match one of these candidates (used in overload resolution)
    Overload {
        source: ConstraintSource,
        candidates: Vec<Box<Type>>,
    },

    /// Contextual typing: infer parameter type from usage context
    Contextual {
        source: ConstraintSource,
        expected: Box<Type>,
    },
}

/// Trace where a constraint came from
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintSource {
    ArgumentAssignment {
        file: String,
        arg_span: Span,
        param_index: u32,
        function_name: String,
    },
    ReturnType {
        file: String,
        return_span: Span,
        function_name: String,
    },
    ConditionalTypeResolution {
        file: String,
        type_span: Span,
    },
    OverloadResolution {
        file: String,
        call_span: Span,
    },
    InheritanceClause {
        file: String,
        span: Span,
    },
    TypeAnnotation {
        file: String,
        span: Span,
    },
    InferredFromUsage {
        file: String,
        span: Span,
        context: String,
    },
}

impl ConstraintSolver {
    pub fn new() -> Self;

    /// Add a constraint and get its ID
    pub fn add_constraint(&mut self, constraint: Constraint) -> ConstraintId;

    /// Solve all constraints (returns map of id -> result type)
    pub fn solve(&mut self) -> ConstraintResult;

    /// Get the solution for a specific constraint
    pub fn get_solution(&self, id: ConstraintId) -> Option<&Type>;

    /// Get all constraints affecting a type variable
    pub fn get_constraints_for_param(&self, param_name: &str) -> Vec<ConstraintId>;

    /// Get the provenance chain for why a constraint was added
    pub fn get_constraint_chain(&self, id: ConstraintId) -> Vec<Constraint>;
}

pub struct ConstraintResult {
    pub success: bool,
    pub errors: Vec<ConstraintError>,
    pub solutions: HashMap<ConstraintId, Type>,
}

#[derive(Debug, Clone)]
pub enum ConstraintError {
    ConflictingConstraints {
        constraint1: ConstraintId,
        constraint2: ConstraintId,
        reason: String,
    },
    UnsatisfiableConstraint {
        constraint_id: ConstraintId,
        reason: String,
    },
    InfiniteConstraintLoop {
        involved_ids: Vec<ConstraintId>,
    },
}
```

**Integration Points**:
- `tsc_rs_types::inference` creates constraints instead of doing direct inference
- `is_assignable()` adds `Constraint::Assignable` instead of checking immediately
- Lazy solving allows re-running specific chains

**Benefits**:
- Query "why is T inferred here?" → walk constraint chain
- Deterministic, reproducible inference
- Basis for "explain type at location" feature (Phase 2)

---

### Primitive 2: Stable Type IDs

**Problem**: `Type` enum has no identity. The same type can be represented as `Type::Union([A, B])` or `Type::Union([B, A])` and they're different values.

**Design**:

```rust
// Extend tsc_rs_ast and tsc_rs_types

/// Unique, persistent identifier for a type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeId(u64);

impl TypeId {
    /// Generate a stable ID from a Type (canonical representation)
    pub fn from_type(ty: &Type) -> Self;

    /// Deterministic hash of type structure (ignoring order in unions)
    fn canonical_hash(ty: &Type) -> u64;
}

/// Type registry – deduplicates types and assigns stable IDs
pub struct TypeRegistry {
    // Map from canonical hash to (type, id) for deduplication
    types: HashMap<u64, (Type, TypeId)>,
    // Next ID to assign
    next_id: u64,
}

impl TypeRegistry {
    pub fn new() -> Self;

    /// Register or retrieve ID for a type
    pub fn intern(&mut self, ty: Type) -> TypeId;

    /// Get the Type for a TypeId
    pub fn get(&self, id: TypeId) -> Option<&Type>;

    /// Get ID for an already-known type
    pub fn lookup(&self, ty: &Type) -> Option<TypeId>;

    /// Export all types for persistence
    pub fn all_types(&self) -> Vec<(TypeId, Type)>;
}

/// Type with stable identity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeWithId {
    pub id: TypeId,
    pub ty: &'static Type, // reference from registry
}
```

**Caching Strategy**:
- Union types: sort members before hashing
- Intersection types: sort members before hashing
- Mapped types: use template + param name
- Generic instantiations: use generic ID + arg IDs

**Integration**:
- Return `(TypeId, &Type)` from type-checking functions
- Store `TypeId` in type nodes for IDE features
- Use `TypeId` as key in constraint solver

**Benefits**:
- Incremental builds: "did public API change?" = "do type IDs differ?"
- Caching: same type = same ID = no recomputation
- Cross-file queries: "find all uses of type X" = find all references to TypeId(X)

---

### Primitive 3: Control-Flow Graph as First-Class Object

**Problem**: `CondFacts` exists but CFG is reconstructed on every type-check. You cannot ask "what types are possible at line 42?"

**Design**:

```rust
// New crate: tsc_rs_control_flow

/// A node in the control flow graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CfgNodeId(u32);

/// A control flow graph for a single function or block
pub struct ControlFlowGraph {
    nodes: Vec<CfgNode>,
    edges: Vec<(CfgNodeId, CfgNodeId, EdgeKind)>,
    node_id_counter: u32,
}

/// A single CFG node
#[derive(Debug, Clone)]
pub struct CfgNode {
    id: CfgNodeId,
    /// Which AST node(s) this represents (for mapping back)
    ast_nodes: Vec<NodeId>,
    /// Facts known to be true reaching this node
    incoming_facts: Facts,
    /// The statement at this location (if any)
    kind: CfgNodeKind,
}

#[derive(Debug, Clone)]
pub enum CfgNodeKind {
    /// Start of a block
    Entry,

    /// A simple statement
    Statement { span: Span },

    /// A condition branch (if/switch/&&/|| etc)
    Condition {
        span: Span,
        condition_type: ConditionKind,
    },

    /// Merge point (join of multiple branches, loop back edge)
    Merge,

    /// Exit from function/block
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionKind {
    If,
    Switch,
    LogicalAnd,
    LogicalOr,
    Ternary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Normal sequential flow
    Sequential,
    /// True branch of condition
    CondTrue,
    /// False branch of condition
    CondFalse,
    /// Loop back edge (for/while)
    Loop,
    /// Throw/error path
    Throw,
}

/// Type narrowing facts at a location
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// For each variable name, what type is it narrowed to?
    pub narrowed_types: HashMap<String, TypeId>,
    /// Type facts (truthiness, typeof checks, etc) per variable
    pub type_facts: HashMap<String, TypeFacts>,
    /// Union types to exclude from a variable
    pub excludes: HashMap<String, Vec<TypeId>>,
    /// Whether null/undefined is definitely excluded
    pub definitely_not_null: HashSet<String>,
    pub definitely_not_undefined: HashSet<String>,
}

impl ControlFlowGraph {
    pub fn new() -> Self;

    /// Add a node and get its ID
    pub fn add_node(&mut self, kind: CfgNodeKind) -> CfgNodeId;

    /// Connect two nodes with an edge
    pub fn add_edge(&mut self, from: CfgNodeId, to: CfgNodeId, kind: EdgeKind);

    /// Get the facts reaching a node (computed by dataflow analysis)
    pub fn facts_at(&self, node_id: CfgNodeId) -> Option<&Facts>;

    /// Run dataflow analysis to compute facts at each node
    pub fn analyze(&mut self, registry: &TypeRegistry);

    /// Get all nodes reachable from a starting point
    pub fn reachable(&self, from: CfgNodeId) -> Vec<CfgNodeId>;

    /// Get all predecessors of a node
    pub fn predecessors(&self, node_id: CfgNodeId) -> Vec<(CfgNodeId, EdgeKind)>;
}

/// Builder for constructing CFG from AST
pub struct CfgBuilder {
    graph: ControlFlowGraph,
    scope_stack: Vec<ScopeInfo>,
}

pub struct ScopeInfo {
    entry_node: CfgNodeId,
    pending_edges: Vec<(CfgNodeId, EdgeKind)>, // edges to connect when scope exits
}

impl CfgBuilder {
    pub fn new() -> Self;

    /// Build CFG for a function/block
    pub fn build(&mut self, stmts: &[Stmt]) -> ControlFlowGraph;

    fn visit_stmt(&mut self, stmt: &Stmt, current: CfgNodeId) -> CfgNodeId;
    fn visit_if(&mut self, cond: &Expr, then_stmt: &Stmt, else_stmt: Option<&Stmt>, current: CfgNodeId) -> CfgNodeId;
    fn visit_switch(&mut self, discriminant: &Expr, cases: &[SwitchCase], current: CfgNodeId) -> CfgNodeId;
    fn visit_while(&mut self, cond: &Expr, body: &Stmt, current: CfgNodeId) -> CfgNodeId;
}
```

**Integration**:
- Build CFG during binding phase, store in symbol table
- Pass CFG to type checker for narrowing queries
- Each file has a CFG forest (one per function)

**Benefits**:
- Explicit representation of what types are possible where
- IDE: "show all narrowed types at this location"
- Analysis: "find unreachable code" or "find exhaustiveness issues"
- Caching: CFG structure doesn't change if types don't change

---

### Primitive 4: Public Interface Hashing

**Problem**: Incremental builds must know "did this file's public API change?" Currently no way to compute this efficiently.

**Design**:

```rust
// New crate: tsc_rs_incremental

/// Hash of a module's public interface
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceHash(u64);

/// Computes and caches hashes of public interfaces
pub struct InterfaceHasher {
    file_hashes: HashMap<String, FileInterfaceHash>,
}

pub struct FileInterfaceHash {
    /// Hash of all exported symbols and their types
    pub exports_hash: InterfaceHash,
    /// Hash of the file's content
    pub content_hash: u64,
    /// Dependency hashes (what other files does this import?)
    pub deps: HashMap<String, InterfaceHash>,
    /// When the file was last checked
    pub timestamp: u64,
}

impl InterfaceHasher {
    pub fn new() -> Self;

    /// Compute interface hash for a file
    /// Takes: file content, export symbols, their types
    pub fn compute_interface(
        &mut self,
        file_name: &str,
        content: &str,
        exports: &[(String, TypeId)],
        deps: &[(String, InterfaceHash)],
    ) -> InterfaceHash;

    /// Check if file's interface changed since last time
    pub fn did_interface_change(&self, file_name: &str, new_hash: InterfaceHash) -> bool;

    /// Mark file as needing recheck
    pub fn invalidate(&mut self, file_name: &str);

    /// Get all downstream files that depend on a file
    pub fn dependents(&self, file_name: &str) -> Vec<String>;
}

/// Incremental state loaded from disk
#[derive(Debug, Clone)]
pub struct IncrementalState {
    /// Saved hashes from last run
    pub interface_hashes: HashMap<String, FileInterfaceHash>,
    /// Build info version
    pub version: u32,
}

impl IncrementalState {
    /// Load from .tsbuildinfo file
    pub fn load(path: &str) -> Result<Self, String>;

    /// Save to .tsbuildinfo file
    pub fn save(&self, path: &str) -> Result<(), String>;
}
```

**Hash Computation**:
```rust
/// Compute hash of a set of exports
fn hash_exports(exports: &[(String, TypeId)]) -> u64 {
    let mut hasher = DefaultHasher::new();

    // Sort for determinism
    let mut sorted = exports.to_vec();
    sorted.sort_by_key(|(name, _)| name.clone());

    for (name, type_id) in sorted {
        name.hash(&mut hasher);
        type_id.hash(&mut hasher);
    }

    hasher.finish()
}

/// Compute hash of a file's dependencies
fn hash_dependencies(deps: &[(String, InterfaceHash)]) -> u64 {
    let mut hasher = DefaultHasher::new();

    let mut sorted = deps.to_vec();
    sorted.sort_by_key(|(name, _)| name.clone());

    for (name, hash) in sorted {
        name.hash(&mut hasher);
        hash.hash(&mut hasher);
    }

    hasher.finish()
}
```

**Integration**:
- After type-checking a file, compute interface hash
- Compare with saved hash from last build
- Only recheck downstream files if interface changed

**Benefits**:
- Monorepo builds: N files → only affected subset rechecked
- 100s of files: goes from O(N) checking to O(affected) checking
- Foundation for Phase 3 (Monorepo Performance Engine)

---

### Primitive 5: Query Engine API

**Problem**: No way to ask the compiler "what is the type at this location?" API is purely imperative (run checker, get output).

**Design**:

```rust
// New crate: tsc_rs_query

/// The query engine – provides all queryable information about a program
pub struct QueryEngine {
    // Persistent data
    files: HashMap<String, ParsedFile>,
    symbol_table: GlobalSymbolTable,
    type_registry: TypeRegistry,
    constraint_solver: ConstraintSolver,
    cfg_forest: HashMap<String, Vec<ControlFlowGraph>>,
    interface_hasher: InterfaceHasher,

    // Caches
    type_cache: HashMap<(NodeId, u32), TypeId>, // (node, scope) -> type
    narrowing_cache: HashMap<(NodeId, u32), Facts>, // (node, scope) -> facts
}

pub struct ParsedFile {
    pub path: String,
    pub source: String,
    pub ast: SourceFile,
    pub symbols: SymbolTable,
    pub exports: Vec<(String, SymbolId)>,
}

pub struct GlobalSymbolTable {
    /// Per-file symbol tables
    file_symbols: HashMap<String, SymbolTable>,
    /// Cross-file symbol links (resolution of imports)
    imports: HashMap<(String, String), Vec<SymbolId>>, // (from_file, import_name) -> [exported_id, ...]
}

impl QueryEngine {
    pub fn new() -> Self;

    /// Add a source file
    pub fn add_source(&mut self, path: String, source: String) -> Result<(), String>;

    /// Full type-check pass
    pub fn check_all(&mut self) -> Result<(), Vec<Diagnostic>>;

    /// ─────────────────────────────────────────────────────
    /// Type queries
    /// ─────────────────────────────────────────────────────

    /// Get the type of an expression at a location
    pub fn get_type_at(&self, file: &str, offset: u32) -> Option<TypeId>;

    /// Get the inferred type of an identifier
    pub fn get_symbol_type(&self, symbol_id: SymbolId) -> Option<TypeId>;

    /// Resolve a type reference to its definition
    pub fn resolve_type_ref(&self, type_ref: &str, from_file: &str) -> Option<TypeId>;

    /// ─────────────────────────────────────────────────────
    /// Symbol queries
    /// ─────────────────────────────────────────────────────

    /// Find a symbol by name
    pub fn find_symbol(&self, name: &str) -> Vec<SymbolId>;

    /// Get symbol's locations (file + spans)
    pub fn get_symbol_locations(&self, symbol_id: SymbolId) -> Vec<(String, Span)>;

    /// Get all uses of a symbol
    pub fn get_symbol_uses(&self, symbol_id: SymbolId) -> Vec<(String, Span)>;

    /// ─────────────────────────────────────────────────────
    /// Constraint & inference queries
    /// ─────────────────────────────────────────────────────

    /// Why is this type what it is?
    pub fn explain_type_at(&self, file: &str, offset: u32) -> Option<TypeExplanation>;

    /// Get the constraint chain for a type inference
    pub fn get_constraint_chain(&self, file: &str, offset: u32) -> Vec<Constraint>;

    /// Get all generic instantiations of a function
    pub fn get_instantiations(&self, symbol_id: SymbolId) -> Vec<(TypeId, Vec<TypeId>)>;

    /// ─────────────────────────────────────────────────────
    /// Control flow & narrowing queries
    /// ─────────────────────────────────────────────────────

    /// Get the type narrowings active at a location
    pub fn get_narrowings_at(&self, file: &str, offset: u32) -> Option<Facts>;

    /// Find where a variable is narrowed
    pub fn find_narrowing_sources(&self, file: &str, symbol_id: SymbolId) -> Vec<(Span, NarrowingReason)>;

    /// ─────────────────────────────────────────────────────
    /// Public interface queries
    /// ─────────────────────────────────────────────────────

    /// Get the public API hash for a file
    pub fn get_interface_hash(&self, file: &str) -> Option<InterfaceHash>;

    /// Did this file's public API change?
    pub fn interface_changed(&self, file: &str) -> bool;

    /// Get all exported symbols from a file
    pub fn get_exports(&self, file: &str) -> Vec<(String, SymbolId, TypeId)>;

    /// ─────────────────────────────────────────────────────
    /// Diagnostic queries
    /// ─────────────────────────────────────────────────────

    /// Get all diagnostics
    pub fn diagnostics(&self) -> Vec<Diagnostic>;

    /// Get diagnostics for a file
    pub fn file_diagnostics(&self, file: &str) -> Vec<Diagnostic>;

    /// Get diagnostics at a location
    pub fn diagnostics_at(&self, file: &str, offset: u32) -> Vec<Diagnostic>;
}

/// Explanation of why a type is what it is
#[derive(Debug, Clone)]
pub struct TypeExplanation {
    pub type_id: TypeId,
    pub ty: Type,
    pub origin: TypeOrigin,
    pub constraints_applied: Vec<ConstraintId>,
    pub narrowings_applied: Vec<NarrowingReason>,
}

#[derive(Debug, Clone)]
pub enum TypeOrigin {
    Explicit { span: Span }, // From type annotation
    Inferred { reason: String }, // Inferred from context
    Defaulted { reason: String }, // Used default (e.g. any)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NarrowingReason {
    Typeof { variable: String, value: String },
    Truthiness { variable: String },
    Equality { variable: String, value: String },
    InstanceOf { variable: String, type_name: String },
    CustomGuard { function_name: String },
}
```

**Integration Points**:
- Built on top of primitives 1-4
- Consumed by IDE (LSP), CLI tools, and tests
- No dependency on previous layers' implementation details

**Benefits**:
- Uniform API for all tooling
- Basis for LSP, IDE plugins, CLI analysis tools
- Foundation for Phases 2-6

---

## Part 3: New Crate Layout

### Current Dependencies

```
tsc_rs_ast ──────────────────────────┐
    ↑                                   ↓
    └─── tsc_rs_scanner                 tsc_rs_project
    └─── tsc_rs_parser      ────────┐   ↓
    └─── tsc_rs_symbols             └─→ tsc_rs_resolver
    └─── tsc_rs_types               ├─→ tsc_rs_emitter
    └─── tsc_rs_resolver            ├─→ tsc_rs_harness
    └─── tsc_rs_emitter             ├─→ tsc_rs_server
                                    └─→ tsc_rs_cli
```

### Proposed New Structure

```
├─ tsc_rs_ast              (AST, spans, core types) [unchanged]
│
├─ tsc_rs_scanner          (lexing) [unchanged]
│
├─ tsc_rs_parser           (parsing) [unchanged]
│   └─ depends on: ast, scanner
│
├─ tsc_rs_symbols          (symbol binding) [refactored]
│   └─ depends on: ast, parser
│   └─ NEW: builds per-file CFG during binding
│
├─ tsc_rs_types            (core type system) [refactored]
│   └─ depends on: ast, symbols
│   └─ REMOVED: generic type operations (→ tsc_rs_constraints)
│   └─ REMOVED: control flow (→ tsc_rs_control_flow)
│   └─ REDUCED: just type definitions and basic operations
│
├─ tsc_rs_constraints      [NEW] (Primitive 1)
│   └─ depends on: ast, types
│   └─ Explicit constraint solver
│   └─ Constraint source tracing
│
├─ tsc_rs_control_flow     [NEW] (Primitive 3)
│   └─ depends on: ast, symbols, constraints
│   └─ CFG construction from AST
│   └─ Dataflow analysis
│   └─ Facts propagation
│
├─ tsc_rs_incremental      [NEW] (Primitive 4)
│   └─ depends on: types
│   └─ Interface hashing
│   └─ Incremental state management
│
├─ tsc_rs_query            [NEW] (Primitive 5)
│   └─ depends on: ast, symbols, types, constraints, control_flow, incremental
│   └─ Unified query interface
│   └─ Type explanations
│   └─ Cache management
│
├─ tsc_rs_resolver         (module resolution) [refactored]
│   └─ depends on: ast, query (for resolution tracing)
│
├─ tsc_rs_emitter          (code generation) [refactored]
│   └─ depends on: ast, query (for type information)
│
├─ tsc_rs_project          (multi-file orchestration) [refactored]
│   └─ depends on: ast, query, incremental
│   └─ NEW: uses query engine instead of direct checker calls
│
├─ tsc_rs_harness          (test infrastructure) [refactored]
│   └─ depends on: ast, query, project
│
├─ tsc_rs_server           (LSP) [NEW]
│   └─ depends on: query
│   └─ Uses query engine exclusively
│
└─ tsc_rs_cli              (binary) [refactored]
    └─ depends on: project, query, incremental
```

### Dependency Graph (Clean)

```
Foundation layer:
  ast → scanner, parser, symbols

Core analysis (orthogonal):
  types ← symbols, ast
  constraints ← types
  control_flow ← symbols, constraints

Composition layer:
  query ← all of the above
  incremental ← types, query

Integration layer:
  resolver ← query
  emitter ← query
  project ← query, incremental
  harness ← query, project
  server ← query
  cli ← query, project, incremental
```

**Key Principle**: Every crate depends on `query` for actual compiler information, not on implementation details of lower crates.

---

## Part 4: Detailed Crate Designs

### tsc_rs_constraints (NEW)

**Purpose**: Explicit constraint system for type inference

**Public API**:
```rust
pub struct ConstraintSolver { ... }
pub enum Constraint { ... }
pub enum ConstraintSource { ... }
pub struct ConstraintResult { ... }

impl ConstraintSolver {
    pub fn new() -> Self;
    pub fn add_constraint(&mut self, constraint: Constraint) -> ConstraintId;
    pub fn solve(&mut self) -> ConstraintResult;
    pub fn get_solution(&self, id: ConstraintId) -> Option<&Type>;
}
```

**Integration with tsc_rs_types**:
- `inference.rs` calls `solver.add_constraint()` instead of direct type operations
- Solver returns `ConstraintResult` with solutions
- Each constraint source points back to AST span

**Testing**:
- Unit tests for each constraint type
- Integration tests: complex generic inference scenarios

---

### tsc_rs_control_flow (NEW)

**Purpose**: First-class control flow graph with dataflow analysis

**Public API**:
```rust
pub struct ControlFlowGraph { ... }
pub struct CfgBuilder { ... }

impl ControlFlowGraph {
    pub fn new() -> Self;
    pub fn add_node(&mut self, kind: CfgNodeKind) -> CfgNodeId;
    pub fn add_edge(&mut self, from: CfgNodeId, to: CfgNodeId, kind: EdgeKind);
    pub fn analyze(&mut self, registry: &TypeRegistry);
    pub fn facts_at(&self, node_id: CfgNodeId) -> Option<&Facts>;
}

impl CfgBuilder {
    pub fn build(&mut self, stmts: &[Stmt]) -> ControlFlowGraph;
}
```

**Integration with tsc_rs_symbols**:
- Built during binding phase
- Stored in function-scope symbol info

**Integration with tsc_rs_types**:
- Used during type narrowing
- Replaces ad-hoc `CondFacts` logic

**Testing**:
- Unit tests: individual statement types
- Integration tests: nested control flow

---

### tsc_rs_incremental (NEW)

**Purpose**: Public interface hashing for incremental builds

**Public API**:
```rust
pub struct InterfaceHasher { ... }
pub struct InterfaceHash(u64);
pub struct IncrementalState { ... }

impl InterfaceHasher {
    pub fn compute_interface(&mut self, file_name: &str, content: &str,
                            exports: &[(String, TypeId)],
                            deps: &[(String, InterfaceHash)]) -> InterfaceHash;
    pub fn did_interface_change(&self, file_name: &str, new_hash: InterfaceHash) -> bool;
}

impl IncrementalState {
    pub fn load(path: &str) -> Result<Self, String>;
    pub fn save(&self, path: &str) -> Result<(), String>;
}
```

**Data Layout**: `.tsbuildinfo` format
```json
{
  "version": "0.1",
  "files": {
    "src/foo.ts": {
      "exports_hash": "0x123abc...",
      "content_hash": "0x456def...",
      "deps": {
        "src/bar.ts": "0x789ghi..."
      },
      "timestamp": 1708500000
    }
  }
}
```

**Integration with tsc_rs_project**:
- Load/save incremental state before/after compilation
- Check interface hashes to determine what to recheck

---

### tsc_rs_query (NEW)

**Purpose**: Unified queryable interface to the compiler

**Public API**: See Primitive 5 section

**Internal Structure**:
```rust
pub struct QueryEngine {
    // Persistent
    files: HashMap<String, ParsedFile>,
    symbol_table: GlobalSymbolTable,
    type_registry: TypeRegistry,
    constraint_solver: ConstraintSolver,
    cfg_forest: HashMap<String, Vec<ControlFlowGraph>>,
    interface_hasher: InterfaceHasher,

    // Mutable state during checking
    current_scope: ScopeId,
    pending_diagnostics: Vec<Diagnostic>,

    // Caches
    type_cache: LruCache<(NodeId, u32), TypeId>,
    narrowing_cache: LruCache<(NodeId, u32), Facts>,
}
```

**Entry Points**:
1. `add_source()` – Add a source file
2. `check_all()` – Run full type checking
3. Query methods – Access compiled information

**Benefits**:
- Single source of truth for all compiler state
- Caches prevent repeated computation
- Query API doesn't expose implementation

---

### tsc_rs_types (REFACTORED)

**Changes**:
- Keep: Basic type definitions (`Type` enum), primitives
- Keep: Simple type operations (`simplify_union`, `substitute`)
- **Remove**: `ConstraintSolver` logic → move to `tsc_rs_constraints`
- **Remove**: CFG and narrowing logic → move to `tsc_rs_control_flow`
- **Keep**: `assign.rs` (but refactored to add constraints instead of checking immediately)

**Public API** (reduced):
```rust
pub type TypeId = u32;
pub enum Type { ... }

pub fn simplify_union(types: Vec<Type>) -> Type;
pub fn substitute(ty: &Type, name: &str, replacement: &Type) -> Type;
pub fn is_assignable_basic(target: &Type, source: &Type) -> bool;
```

---

### tsc_rs_symbols (REFACTORED)

**Changes**:
- **Add**: CFG construction during binding
- Store CFG in per-function scope info
- Keep symbol merging logic (it's correct)

**Public API**:
```rust
pub struct Symbol { ... }
pub struct SymbolTable { ... }

pub fn bind(source_file: &SourceFile) -> (SymbolTable, HashMap<SymbolId, ControlFlowGraph>);
```

---

### tsc_rs_project (REFACTORED)

**Changes**:
- Use `QueryEngine` instead of building directly
- Load/save incremental state
- Orchestrate file compilation through query API

**Public API**:
```rust
pub struct TsProject { ... }

impl TsProject {
    pub fn compile(&self) -> Result<QueryEngine, Vec<Diagnostic>>;
}
```

---

## Part 5: Phase 0 to Phase 1 Migration Strategy

### Phase 0 (Current): Compiler Parity

**Constraint**: Must pass existing baseline tests
**Approach**: Implement 5 primitives **in parallel** without breaking current code

1. **Build new crates** (`tsc_rs_constraints`, `tsc_rs_control_flow`, `tsc_rs_query`) with stubs
2. **Add Primitive 2** (TypeId/TypeRegistry) to `tsc_rs_types` incrementally
3. **Preserve existing paths**: Keep `tsc_rs_types::check()` and `tsc_rs_project::compile()` working
4. **Internal refactoring**: Move constraint logic under the hood, but API stays the same

### Phase 1: Query Engine Launch

**Constraint**: Query API must be the source of truth
**Approach**: Gradually move internal APIs to use query engine

1. `tsc_rs_types` type-checking calls `query.add_constraint()` instead of local logic
2. `tsc_rs_resolver` returns resolution info via query API
3. `tsc_rs_emitter` reads types via `query.get_type_at()` instead of symbol table
4. Public `TsProject::compile()` internally builds QueryEngine and returns it
5. Remove `tsc_rs_project::check()` direct access to types

### Phase 2+: Explainability and Beyond

Once Query API is stable, all future features build on it.

---

## Part 6: Rust Type Definitions (Complete)

### Core Types

```rust
// ─────────────────────────────────────────────────────
// Type Identity
// ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeId(u64);

impl TypeId {
    pub fn from_type(ty: &Type) -> Self;
    pub fn as_u64(self) -> u64;
}

// ─────────────────────────────────────────────────────
// Constraints
// ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstraintId(u32);

#[derive(Debug, Clone)]
pub enum Constraint {
    Assignable { source: ConstraintSource, target: Box<Type>, source_type: Box<Type> },
    Extends { source: ConstraintSource, check_type: Box<Type>, extends_type: Box<Type> },
    Instantiation { source: ConstraintSource, param_name: String, concrete_type: Box<Type> },
    Overload { source: ConstraintSource, candidates: Vec<Box<Type>> },
    Contextual { source: ConstraintSource, expected: Box<Type> },
}

// ─────────────────────────────────────────────────────
// Control Flow
// ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CfgNodeId(u32);

#[derive(Debug, Clone)]
pub enum EdgeKind {
    Sequential,
    CondTrue,
    CondFalse,
    Loop,
    Throw,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub narrowed_types: HashMap<String, TypeId>,
    pub type_facts: HashMap<String, TypeFacts>,
    pub excludes: HashMap<String, Vec<TypeId>>,
    pub definitely_not_null: HashSet<String>,
    pub definitely_not_undefined: HashSet<String>,
}

// ─────────────────────────────────────────────────────
// Interface Hashing
// ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceHash(u64);

// ─────────────────────────────────────────────────────
// Query Engine
// ─────────────────────────────────────────────────────

pub struct QueryEngine {
    files: HashMap<String, ParsedFile>,
    symbol_table: GlobalSymbolTable,
    type_registry: TypeRegistry,
    constraint_solver: ConstraintSolver,
    cfg_forest: HashMap<String, Vec<ControlFlowGraph>>,
    interface_hasher: InterfaceHasher,
    type_cache: HashMap<(NodeId, u32), TypeId>,
    narrowing_cache: HashMap<(NodeId, u32), Facts>,
}
```

---

## Part 7: Implementation Roadmap

### Phase 0a: Primitives 2 & 4 (Low Risk)

**Duration**: 2-3 weeks
**Rationale**: Type IDs and hashing don't affect existing code paths

1. Add `TypeId` and `TypeRegistry` to `tsc_rs_types`
   - New types, no API changes
   - Intern types as they're created
   - Enable deduplication

2. Create `tsc_rs_incremental`
   - Compute hashes after type-checking
   - Don't load from disk yet (Phase 1)
   - Verifiable: hashes must be stable

### Phase 0b: Primitive 1 (Constraints, Isolated)

**Duration**: 2-3 weeks
**Rationale**: Build constraint solver alongside existing type checker

1. Create `tsc_rs_constraints` crate (stub)
2. Add `ConstraintId` and `Constraint` types
3. Build constraint solver logic
4. **Do NOT** change existing type checker yet
5. Add unit tests for constraint solver
6. Verifiable: Solver must produce same results as old logic

### Phase 0c: Primitive 3 (CFG, Build Phase)

**Duration**: 3 weeks
**Rationale**: CFG is data structure, doesn't immediately affect type checking

1. Create `tsc_rs_control_flow` crate
2. Implement `CfgBuilder` to construct CFG from AST
3. Integrate into binding phase (build CFG, don't use it yet)
4. Implement dataflow analysis
5. Add tests for individual statement types
6. Verifiable: CFG facts must match `CondFacts` from current code

### Phase 0d: Primitive 5 (Query Engine, Adapter)

**Duration**: 2 weeks
**Rationale**: Adapter layer, no breaking changes

1. Create `tsc_rs_query` crate
2. Implement `QueryEngine` struct
3. Build adapters that call existing type checker
4. Add basic query methods
5. Integrate into `TsProject`
6. Verifiable: Query results must match direct type checker results

### Phase 0e: Integration & Testing

**Duration**: 1-2 weeks

1. Run full baseline test suite
2. Fix any regressions
3. Validate all 5 primitives work together
4. Document Phase 0 → Phase 1 migration steps

---

## Part 8: Success Criteria

### Phase 0 Completion

- [ ] All 5 primitives designed and documented
- [ ] New crates created with public APIs defined
- [ ] Type IDs stable and deduplicating correctly
- [ ] Constraint solver produces correct results
- [ ] CFG analysis matches control flow reality
- [ ] Public interface hashing is deterministic
- [ ] Query engine returns correct results
- [ ] All baseline tests pass
- [ ] Zero breaking changes to external API

### Phase 1 Launch

- [ ] Query engine is primary API
- [ ] Type checker calls constraint solver
- [ ] Type narrowing uses CFG
- [ ] Incremental state can be loaded/saved
- [ ] LSP uses query API
- [ ] All internal tools use query API
- [ ] New tests for query API coverage

---

## Part 9: Dependencies Between Primitives

```
Primitive 2 (TypeId)
  ↑ needed by all others

Primitive 1 (Constraints)
  ↑ needed by: inference

Primitive 3 (CFG)
  ↑ needed by: type narrowing

Primitive 4 (Interface Hashing)
  ↑ needed by: incremental builds

Primitive 5 (Query API)
  ↓ depends on: all of the above
```

**Recommended Order**:
1. Primitive 2 (TypeId) – foundation
2. Primitive 4 (Hashing) – independent of others
3. Primitive 1 (Constraints) – uses TypeId
4. Primitive 3 (CFG) – uses TypeId and Constraints
5. Primitive 5 (Query) – uses all of the above

---

## Part 10: File Structure Reference

### New Files to Create

```
crates/tsc_rs_constraints/
  Cargo.toml
  src/
    lib.rs          - ConstraintSolver, Constraint enum
    source.rs       - ConstraintSource enum
    solver.rs       - Solving algorithm

crates/tsc_rs_control_flow/
  Cargo.toml
  src/
    lib.rs          - ControlFlowGraph, CfgNode
    builder.rs      - CfgBuilder implementation
    dataflow.rs     - Dataflow analysis

crates/tsc_rs_incremental/
  Cargo.toml
  src/
    lib.rs          - InterfaceHasher, InterfaceHash
    state.rs        - IncrementalState, .tsbuildinfo format

crates/tsc_rs_query/
  Cargo.toml
  src/
    lib.rs          - QueryEngine struct
    api/
      types.rs      - Type queries
      symbols.rs    - Symbol queries
      explain.rs    - Explanation queries
    cache.rs        - Caching layer
```

### Files to Modify

```
crates/tsc_rs_ast/src/
  lib.rs            - Add TypeId re-export (from tsc_rs_types)

crates/tsc_rs_types/src/
  lib.rs            - Add TypeId, TypeRegistry
  assign.rs         - Refactored to use ConstraintSolver

crates/tsc_rs_symbols/src/
  lib.rs            - Add CFG construction

crates/tsc_rs_project/src/
  lib.rs            - Integrate QueryEngine

crates/tsc_rs_cli/src/
  main.rs           - Use QueryEngine
```

---

## Conclusion

This architecture provides:

1. **Explicitness**: Every type decision is traceable
2. **Incrementalism**: Only affected files are rechecked
3. **Queryability**: Every compiler fact is accessible
4. **Extensibility**: New features build on stable primitives
5. **Testability**: Each primitive is independently verifiable

The path from Phase 0 (parity) to Phase 2+ (analysis platform) is clear and doesn't require wholesale rewrites. Each primitive can be built and tested independently while preserving compiler correctness.
