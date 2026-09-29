# tsc_rs_query: Query Engine API Reference

**Crate**: `tsc_rs_query`
**Purpose**: Unified queryable interface for all TypeScript analysis
**Status**: Design specification (implementation pending)

---

## QueryEngine Struct

The main entry point for all compiler queries.

```rust
pub struct QueryEngine {
    // Internal state
    files: HashMap<String, ParsedFile>,
    symbol_table: GlobalSymbolTable,
    type_registry: TypeRegistry,
    constraint_solver: ConstraintSolver,
    cfg_forest: HashMap<String, Vec<ControlFlowGraph>>,
    interface_hasher: InterfaceHasher,

    // Caches
    type_cache: HashMap<(NodeId, u32), TypeId>,
    narrowing_cache: HashMap<(NodeId, u32), Facts>,
}
```

---

## Setup & Management

### `pub fn new() -> Self`
Create a new query engine.

```rust
let mut engine = QueryEngine::new();
```

### `pub fn add_source(&mut self, path: String, source: String) -> Result<(), String>`
Add a source file for analysis.

```rust
engine.add_source("src/app.ts".to_string(), source_code)?;
```

### `pub fn check_all(&mut self) -> Result<(), Vec<Diagnostic>>`
Run full type-checking pass on all added files.

```rust
match engine.check_all() {
    Ok(()) => println!("Type checking succeeded"),
    Err(diagnostics) => {
        for diag in diagnostics {
            println!("{:?}", diag);
        }
    }
}
```

---

## Type Queries

### `pub fn get_type_at(&self, file: &str, offset: u32) -> Option<TypeId>`
Get the type of an expression at a byte offset.

```rust
if let Some(type_id) = engine.get_type_at("src/app.ts", 42) {
    println!("Type at offset 42: {:?}", type_id);
}
```

**Returns**:
- `Some(TypeId)` if there's a type at that location
- `None` if offset is out of range or not an expression

### `pub fn get_symbol_type(&self, symbol_id: SymbolId) -> Option<TypeId>`
Get the inferred type of a symbol.

```rust
if let Some(type_id) = engine.get_symbol_type(symbol_id) {
    // This symbol has an inferred type
}
```

### `pub fn resolve_type_ref(&self, type_ref: &str, from_file: &str) -> Option<TypeId>`
Resolve a type reference (e.g., "MyClass") to its type ID from a given file context.

```rust
if let Some(type_id) = engine.resolve_type_ref("Promise<string>", "src/app.ts") {
    // Found the type
}
```

### `pub fn get_all_types(&self) -> Vec<(TypeId, Type)>`
Get all types in the program.

```rust
for (type_id, ty) in engine.get_all_types() {
    println!("{:?}: {:?}", type_id, ty);
}
```

---

## Symbol Queries

### `pub fn find_symbol(&self, name: &str) -> Vec<SymbolId>`
Find all symbols with a given name across all files.

```rust
let symbols = engine.find_symbol("MyClass");
for symbol_id in symbols {
    println!("Found: {:?}", symbol_id);
}
```

### `pub fn get_symbol_locations(&self, symbol_id: SymbolId) -> Vec<(String, Span)>`
Get all declaration locations of a symbol.

```rust
let locations = engine.get_symbol_locations(symbol_id);
for (file, span) in locations {
    println!("Declared at {}:{}-{}", file, span.start, span.end);
}
```

### `pub fn get_symbol_uses(&self, symbol_id: SymbolId) -> Vec<(String, Span)>`
Get all uses of a symbol (references).

```rust
let uses = engine.get_symbol_uses(symbol_id);
println!("Symbol used {} times", uses.len());
```

### `pub fn get_symbol_info(&self, symbol_id: SymbolId) -> Option<SymbolInfo>`
Get detailed information about a symbol.

```rust
pub struct SymbolInfo {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,      // function, class, interface, etc.
    pub type_id: Option<TypeId>,
    pub declarations: Vec<(String, Span)>,
    pub is_exported: bool,
}
```

---

## Constraint & Inference Queries

### `pub fn explain_type_at(&self, file: &str, offset: u32) -> Option<TypeExplanation>`
Get a detailed explanation of why a type is what it is.

```rust
if let Some(explanation) = engine.explain_type_at("src/app.ts", 42) {
    println!("Type: {:?}", explanation.ty);
    println!("Origin: {:?}", explanation.origin);
    println!("Constraints applied: {} constraints",
             explanation.constraints_applied.len());
}
```

**Returns**:
```rust
pub struct TypeExplanation {
    pub type_id: TypeId,
    pub ty: Type,
    pub origin: TypeOrigin,
    pub constraints_applied: Vec<ConstraintId>,
    pub narrowings_applied: Vec<NarrowingReason>,
}

pub enum TypeOrigin {
    Explicit { span: Span },        // From type annotation
    Inferred { reason: String },    // Inferred from context
    Defaulted { reason: String },   // Used default (e.g., 'any')
}
```

### `pub fn get_constraint_chain(&self, file: &str, offset: u32) -> Vec<Constraint>`
Get the full constraint chain for a type inference at a location.

```rust
let chain = engine.get_constraint_chain("src/app.ts", 42);
for constraint in chain {
    println!("{:?}", constraint);
}
```

**Returns**: Constraints in order from most general to most specific.

### `pub fn get_instantiations(&self, symbol_id: SymbolId) -> Vec<(TypeId, Vec<TypeId>)>`
Get all generic instantiations of a function or class.

```rust
let instantiations = engine.get_instantiations(symbol_id);
for (instance_type_id, type_args) in instantiations {
    println!("Instantiation with {:?}", type_args);
}
```

---

## Control Flow & Narrowing Queries

### `pub fn get_narrowings_at(&self, file: &str, offset: u32) -> Option<Facts>`
Get the type narrowings active at a specific location.

```rust
if let Some(facts) = engine.get_narrowings_at("src/app.ts", 42) {
    for (var_name, narrowed_type_id) in &facts.narrowed_types {
        println!("{} is narrowed to {:?}", var_name, narrowed_type_id);
    }
}
```

**Returns**:
```rust
pub struct Facts {
    pub narrowed_types: HashMap<String, TypeId>,
    pub type_facts: HashMap<String, TypeFacts>,
    pub definitely_not_null: HashSet<String>,
    pub definitely_not_undefined: HashSet<String>,
}
```

### `pub fn find_narrowing_sources(&self, file: &str, symbol_id: SymbolId) -> Vec<(Span, NarrowingReason)>`
Find all locations where a variable is narrowed.

```rust
let sources = engine.find_narrowing_sources("src/app.ts", symbol_id);
for (span, reason) in sources {
    println!("Narrowing at {:?}: {:?}", span, reason);
}
```

### `pub fn is_narrowed_to(&self, file: &str, offset: u32, symbol_id: SymbolId, target_type_id: TypeId) -> bool`
Check if a variable is narrowed to a specific type at a location.

```rust
if engine.is_narrowed_to("src/app.ts", 42, symbol_id, string_type_id) {
    println!("Variable is narrowed to string at that location");
}
```

---

## Public Interface Queries

### `pub fn get_interface_hash(&self, file: &str) -> Option<InterfaceHash>`
Get the public API hash for a file.

```rust
if let Some(hash) = engine.get_interface_hash("src/api.ts") {
    println!("Interface hash: 0x{:x}", hash.0);
}
```

### `pub fn interface_changed(&self, file: &str) -> bool`
Did this file's public API change since last check?

```rust
if engine.interface_changed("src/api.ts") {
    println!("Public API changed, dependents need rechecking");
}
```

### `pub fn get_exports(&self, file: &str) -> Vec<(String, SymbolId, TypeId)>`
Get all exported symbols from a file.

```rust
let exports = engine.get_exports("src/api.ts");
for (name, symbol_id, type_id) in exports {
    println!("Export: {} : {:?}", name, type_id);
}
```

### `pub fn get_dependencies(&self, file: &str) -> Vec<(String, Vec<String>)>`
Get the import dependencies of a file.

```rust
let deps = engine.get_dependencies("src/app.ts");
for (imported_file, imported_names) in deps {
    println!("Imports {} from {}", imported_names.join(", "), imported_file);
}
```

### `pub fn get_dependent_files(&self, file: &str) -> Vec<String>`
Get all files that import from this file.

```rust
let dependents = engine.get_dependent_files("src/api.ts");
println!("{} files depend on this API", dependents.len());
```

---

## Diagnostic Queries

### `pub fn diagnostics(&self) -> Vec<Diagnostic>`
Get all diagnostics from the entire program.

```rust
let all = engine.diagnostics();
println!("{} total diagnostics", all.len());
```

### `pub fn file_diagnostics(&self, file: &str) -> Vec<Diagnostic>`
Get diagnostics for a specific file.

```rust
let file_diags = engine.file_diagnostics("src/app.ts");
for diag in file_diags {
    println!("[{}] {}: {}", diag.category, diag.code, diag.message);
}
```

### `pub fn diagnostics_at(&self, file: &str, offset: u32) -> Vec<Diagnostic>`
Get diagnostics at a specific location.

```rust
let at_location = engine.diagnostics_at("src/app.ts", 42);
```

### `pub fn error_count(&self) -> usize`
Get count of errors (not warnings).

```rust
if engine.error_count() > 0 {
    println!("Compilation has errors");
}
```

---

## Utility Queries

### `pub fn get_type(&self, type_id: TypeId) -> Option<&Type>`
Look up a type by its ID.

```rust
if let Some(ty) = engine.get_type(type_id) {
    println!("Type definition: {:?}", ty);
}
```

### `pub fn type_to_string(&self, type_id: TypeId) -> String`
Get a human-readable string representation of a type.

```rust
let type_str = engine.type_to_string(type_id);
println!("Type: {}", type_str);
// Output: Type: (x: string) => Promise<number>
```

### `pub fn get_source_file(&self, file: &str) -> Option<&SourceFile>`
Get the parsed source file.

```rust
if let Some(sf) = engine.get_source_file("src/app.ts") {
    println!("File has {} statements", sf.statements.len());
}
```

---

## Usage Examples

### Example 1: Find All Type Errors

```rust
fn find_type_errors(engine: &QueryEngine) {
    for diag in engine.diagnostics() {
        if diag.category == DiagnosticCategory::Error {
            if let Some((file, span)) = (&diag.file_name, diag.span) {
                println!("ERROR at {}:{}:{}", file, span.start, span.end);
                println!("  {}", diag.message);
            }
        }
    }
}
```

### Example 2: Understand Why a Type Was Inferred

```rust
fn explain_inference(engine: &QueryEngine, file: &str, offset: u32) {
    if let Some(explanation) = engine.explain_type_at(file, offset) {
        println!("Type: {}", engine.type_to_string(explanation.type_id));
        println!("Origin: {:?}", explanation.origin);
        println!("\nConstraints:");
        for constraint in explanation.constraints_applied {
            // Use constraint_id to look up details
        }
        println!("\nNarrowings:");
        for narrowing in explanation.narrowings_applied {
            println!("  {:?}", narrowing);
        }
    }
}
```

### Example 3: Find Unused Exports

```rust
fn find_unused_exports(engine: &QueryEngine, file: &str) {
    for (name, symbol_id, _type_id) in engine.get_exports(file) {
        let uses = engine.get_symbol_uses(symbol_id);
        // Filter to uses outside this file
        let external_uses: Vec<_> = uses.iter()
            .filter(|(use_file, _)| use_file != file)
            .collect();

        if external_uses.is_empty() {
            println!("Unused export: {}", name);
        }
    }
}
```

### Example 4: Check If Interface Changed

```rust
fn check_incremental(engine: &QueryEngine) {
    let files = vec!["src/api.ts", "src/utils.ts"];
    for file in files {
        let changed = engine.interface_changed(file);
        if changed {
            let dependents = engine.get_dependent_files(file);
            println!("{}: changed → invalidate {} files", file, dependents.len());
        }
    }
}
```

---

## Performance Considerations

### Caching
- Query results are cached (LRU)
- Call `engine.invalidate_cache()` if you modify sources
- Type ID lookups are O(1) with registry
- Symbol lookups are O(1) with symbol table

### Batch Queries
- Use `get_exports()` instead of repeated `get_symbol_type()`
- Use `diagnostics()` instead of looping `diagnostics_at()`

### Large Programs
- Query engine is designed for monorepos (100s of files)
- Constraint solver is lazy (only solves for relevant constraints)
- CFG is incremental (updated only for changed files)

---

## Integration with Other Crates

### With IDE/LSP
```rust
// Language server hover handler
pub fn on_hover(engine: &QueryEngine, file: &str, position: u32) -> Option<String> {
    let type_id = engine.get_type_at(file, position)?;
    Some(engine.type_to_string(type_id))
}
```

### With CLI Tools
```rust
// CLI: tsc-rs explain src/foo.ts:42:10
pub fn explain_cli(file: &str, line: u32, col: u32) -> Result<String> {
    let mut engine = QueryEngine::new();
    engine.add_source(file.to_string(), fs::read_to_string(file)?)?;
    engine.check_all()?;

    // Convert line:col to byte offset
    let offset = compute_offset(file, line, col)?;

    if let Some(explanation) = engine.explain_type_at(file, offset) {
        Ok(format!("{:?}", explanation))
    } else {
        Ok("No type information at this location".to_string())
    }
}
```

### With Test Harness
```rust
// Baseline testing
pub fn run_baseline_test(test_file: &str) -> TestResult {
    let mut engine = QueryEngine::new();
    let source = load_test_file(test_file)?;
    engine.add_source(test_file.to_string(), source)?;
    engine.check_all()?;

    // Compare diagnostics with baseline
    let actual = engine.diagnostics();
    let expected = load_baseline(test_file)?;

    TestResult {
        passed: actual == expected,
        diagnostics: actual,
    }
}
```

---

## Error Handling

All query methods return `Option<T>` or `Result<T, E>`. No panics.

```rust
// Safe to call with invalid inputs
let type_at = engine.get_type_at("nonexistent.ts", 999999);
assert_eq!(type_at, None);  // Returns None, doesn't panic
```

---

## Thread Safety

`QueryEngine` is designed for single-threaded use within each query session.

For multi-threaded analysis:
- Create a separate `QueryEngine` per thread
- Load `.tsbuildinfo` incremental state
- Each thread checks disjoint set of files
- Merge results afterward

```rust
use std::sync::Arc;
use std::thread;

let files: Vec<_> = /* list of files */;
let handles: Vec<_> = files.chunks(10).map(|chunk| {
    let files = chunk.to_vec();
    thread::spawn(move || {
        let mut engine = QueryEngine::new();
        for file in files {
            // process file
        }
        engine.diagnostics()
    })
}).collect();

let all_diagnostics: Vec<_> = handles.into_iter()
    .map(|h| h.join().unwrap())
    .flatten()
    .collect();
```
