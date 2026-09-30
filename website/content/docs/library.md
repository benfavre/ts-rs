---
title: Rust library API
---
# Rust library API

You can call the tsc-rs crates from your own Rust code. There are two levels. The low-level pipeline gives you one function per stage (parse, bind, check, emit) for a single file. The `tsc_rs_query` crate wraps those stages in a `QueryEngine` that holds several files, checks them together, and answers questions about types, symbols, diagnostics and imports. The language server is built on it. This page documents both as they exist in the source today.

## Add the crates

The crates are not on crates.io. Depend on them through git and pin a `rev`:

```toml
[dependencies]
tsc_rs_ast = { git = "https://github.com/benfavre/ts-rs.git", rev = "<commit>" }
tsc_rs_parser = { git = "https://github.com/benfavre/ts-rs.git", rev = "<commit>" }
tsc_rs_symbols = { git = "https://github.com/benfavre/ts-rs.git", rev = "<commit>" }
tsc_rs_types = { git = "https://github.com/benfavre/ts-rs.git", rev = "<commit>" }
tsc_rs_emitter = { git = "https://github.com/benfavre/ts-rs.git", rev = "<commit>" }
tsc_rs_query = { git = "https://github.com/benfavre/ts-rs.git", rev = "<commit>" }
```

Take only the crates you call. `tsc_rs_ast` holds the shared types: `SourceFile`, `CompilerOptions`, `Diagnostic` and `Span`.

## The low-level pipeline

| Stage | Call | Returns |
|---|---|---|
| Parse | `tsc_rs_parser::parse(file_name: &str, source: &str)` | `tsc_rs_ast::SourceFile` |
| Bind | `tsc_rs_symbols::bind(file: &SourceFile)` | `tsc_rs_symbols::SymbolTable` |
| Check | `tsc_rs_types::TypeChecker::new().check_with_options(&file, &symbols, &options)` | `tsc_rs_types::TypeCheckOutput` |
| Emit | `tsc_rs_emitter::emit(file: &SourceFile, options: &CompilerOptions)` | `tsc_rs_emitter::EmitOutput` |

Notes on each stage:

- **Parse.** A file name ending in `.tsx` or `.jsx` turns JSX parsing on. `parse_with_jsx(file_name, source, jsx_enabled)` sets it explicitly. Parsing never fails: syntax errors are collected in `SourceFile::diagnostics`, and `tsc_rs_parser::has_syntax_errors(&file.diagnostics)` tells you whether any of them is a real syntax error.
- **Bind.** `tsc_rs_symbols::bind(&file)` is shorthand for `Binder::new().bind(&file)`.
- **Check.** `check` and `check_with_options` take the checker by value, so build one `TypeChecker` per file. The free functions `tsc_rs_types::check(&file, &symbols)` and `tsc_rs_types::check_with_options(&file, &symbols, &options)` do the same in one call. Checking is skipped, with an empty output, when `options.no_check` is `Some(true)` or the file starts with a `// @ts-nocheck` comment.
- **Emit.** Emit needs only the parsed file. It does not depend on binding or checking. `tsc_rs_emitter::emit_strip_types(&file)` is a preset that removes types and keeps modern syntax (ESNext target and module).

### Shared types

`CompilerOptions` derives `Default`, and almost every field is an `Option`. Set the fields you need and leave the rest:

```rust
use tsc_rs_ast::{CompilerOptions, JsxEmit, ModuleKind, ScriptTarget};

let options = CompilerOptions {
    target: Some(ScriptTarget::ES2022),
    module: Some(ModuleKind::ESNext),
    jsx: Some(JsxEmit::ReactJSX),
    strict: Some(true),
    ..Default::default()
};
```

| Type | Public fields |
|---|---|
| `Span` | `start: u32`, `end: u32`. Byte offsets into the source text |
| `Diagnostic` | `code: u32`, `message: String`, `category: DiagnosticCategory`, `file_name: Option<String>`, `span: Option<Span>`, `related: Option<Vec<RelatedDiagnostic>>` |
| `DiagnosticCategory` | `Error`, `Warning`, `Suggestion`, `Message` |
| `TypeCheckOutput` | `diagnostics: Vec<Diagnostic>`, `expression_types: HashMap<u32, String>` (offset to displayed type), `selected_overload_indices: HashMap<u32, usize>`, `stable_types: HashMap<u64, String>` |
| `EmitOutput` | `javascript: String`, `source_map: Option<String>`, `declaration_file: Option<String>`, `require_var_counters`, `system_register_counter` |

`source_map` is filled when `options.source_map` is `Some(true)`, and `declaration_file` when `options.declaration` is `Some(true)`. The two counter fields only matter when you concatenate several files into one AMD or System bundle.

### Example: check and emit one file

```rust
use tsc_rs_ast::{CompilerOptions, ModuleKind, ScriptTarget};

fn main() {
    let source = "export const total: number = \"12\";\n";

    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2022),
        module: Some(ModuleKind::ESNext),
        strict: Some(true),
        ..Default::default()
    };

    // 1. Parse. Syntax errors are collected on the SourceFile.
    let file = tsc_rs_parser::parse("input.ts", source);

    // 2. Bind: build the symbol table for this file.
    let symbols = tsc_rs_symbols::bind(&file);

    // 3. Check. `check_with_options` consumes the checker.
    let checked = tsc_rs_types::TypeChecker::new().check_with_options(&file, &symbols, &options);

    for d in file.diagnostics.iter().chain(checked.diagnostics.iter()) {
        let start = d.span.map_or(0, |span| span.start);
        println!("input.ts@{start} TS{}: {}", d.code, d.message);
    }

    // 4. Emit. This step only needs the parsed file.
    let output = tsc_rs_emitter::emit(&file, &options);
    print!("{}", output.javascript);
}
```

This prints:

```bash
input.ts@13 TS2322: Type 'string' is not assignable to type 'number'.
export const total = "12";
```

Diagnostics from this path carry `file_name: None`, and positions are byte offsets, not line and column. The checker here sees one file. For several files that import each other, use the query engine.

## QueryEngine

`tsc_rs_query::QueryEngine` stores source text by path, runs parse, bind and check over all of it, and keeps the results for lookup. The whole crate is one file: [crates/tsc_rs_query/src/lib.rs](https://github.com/benfavre/ts-rs/blob/main/crates/tsc_rs_query/src/lib.rs).

### Build and check

| Method | What it does |
|---|---|
| `QueryEngine::new() -> Self` | Engine with `CompilerOptions::default()`. `QueryEngine::default()` is the same |
| `QueryEngine::with_options(options: CompilerOptions) -> Self` | Engine with your options |
| `options(&self) -> &CompilerOptions` | Borrows the current options |
| `set_options(&mut self, options: CompilerOptions)` | Replaces the options and drops every cached result. Sources are kept |
| `add_source(&mut self, path: String, source: String) -> Result<(), String>` | Adds or replaces one file. The only error is an empty path |
| `check_all(&mut self) -> Result<(), Vec<Diagnostic>>` | Parses, binds and checks every loaded file |

How these behave:

- **Paths are normalized.** `./src/a.ts` and `src/a.ts` are the same file, and results report the normalized form. There is no method to remove a source.
- **Queries read the last `check_all`.** After `add_source`, call `check_all` again before you query. A second `check_all` reuses the parse and check results of every file whose text did not change.
- **`Err` does not mean failure.** `check_all` returns `Err` when at least one diagnostic is an error. The vector is a copy of all current diagnostics, and the engine is fully queryable afterwards.
- **`check_all` reads the disk.** Imports are resolved with the real module resolver. Declaration files (`.d.ts`, `.d.mts`, `.d.cts`) reached through imports are loaded automatically and become sources of the engine. Set `TSC_RS_AUTODISCOVER=0` to turn that off. Imported `.ts` files are not loaded for you: add each one.
- **The standard library comes from a TypeScript install.** The `lib.*.d.ts` files are read from the directory named by `TSC_RS_TYPESCRIPT_LIB_DIR`, or from the nearest `node_modules/typescript/lib` above the `tsc_rs_types` crate sources or above the working directory. When none is found, the engine falls back to a stub that declares only `Object.assign`.

:::warning
Give the engine the real on-disk path of each file. With paths that exist only in memory, imported classes and interfaces still resolve, but imported functions and variables are typed `any`, so errors that depend on them are not reported.
:::

### Diagnostics

| Method | Returns |
|---|---|
| `diagnostics(&self) -> Vec<String>` | Every diagnostic, formatted as `file:start-end category TScode: message` |
| `file_diagnostics(&self, file: &str) -> Vec<String>` | The same format, for one file |
| `diagnostics_at(&self, file: &str, offset: u32) -> Vec<String>` | Diagnostics whose span contains `offset`. If there is none, those within 16 bytes, nearest first |
| `raw_diagnostics(&self) -> &[Diagnostic]` | The structured diagnostics, with code, category, file name and span |
| `error_count(&self) -> usize` | Number of diagnostics with category `Error` |

### Types

Types are exposed as `u64` ids. An id stands for a displayed type string such as `number` or `(a: number) => number`. Turn it back into text with `type_to_string`.

| Method | Returns |
|---|---|
| `get_type_at(&self, file: &str, offset: u32) -> Option<u64>` | Type id of the expression that starts at `offset`, else of the nearest expression that starts up to 32 bytes before it |
| `type_to_string(&self, type_id: u64) -> Option<String>` | Displayed type for an id |
| `get_type(&self, type_id: u64) -> Option<String>` | Same as `type_to_string` |
| `get_expression_types(&self, file: &str) -> Vec<(u32, String)>` | Every recorded `(offset, displayed type)` pair of a file, in no particular order |
| `resolve_type_ref(&self, type_ref: &str, from_file: &str) -> Option<u64>` | Id for a primitive keyword (`string`, `number`, `boolean`, ...) or for the type of the first symbol with that name |
| `get_file_check_output(&self, file: &str) -> Option<TypeCheckOutput>` | The stored expression types of a file as a `TypeCheckOutput`. Its `diagnostics` field is always empty |

`type_to_string` only knows ids that were recorded during the check. The id that `resolve_type_ref` returns for a primitive keyword is computed separately and may not be among them.

### Symbols

| Method | Returns |
|---|---|
| `get_symbol_at(&self, file: &str, offset: u32) -> Option<u64>` | Id of the symbol declared or referenced at `offset` |
| `find_symbol(&self, name: &str) -> Vec<u64>` | Ids of all symbols with exactly this name, sorted, without duplicates |
| `get_symbol_info(&self, symbol_id: u64) -> Option<SymbolInfo>` | Name, kind, type id, declarations and export flag |
| `get_symbol_locations(&self, symbol_id: u64) -> Vec<(String, Span)>` | Declaration sites as `(file, span)`, sorted |
| `get_symbol_uses(&self, symbol_id: u64) -> Vec<(String, Span)>` | Identifier positions bound to the symbol, declaration included, sorted |
| `get_symbol_type(&self, symbol_id: u64) -> Option<u64>` | Type id recorded at the first declaration |
| `get_symbol_type_in_file(&self, symbol_id: u64, file_name: &str) -> Option<u64>` | The same, looking only at one file |
| `get_file_symbols(&self, file: &str) -> Option<&SymbolTable>` | The symbol table of a file, with imports linked to their source declarations |
| `all_global_symbols(&self) -> Vec<SymbolInfo>` | One entry per name across all files, for completion lists. `type_id` is always `None` |

```rust
pub struct SymbolInfo {
    pub id: u64,
    pub name: String,
    pub kind: SymbolKind,
    pub type_id: Option<u64>,
    pub declarations: Vec<(String, Span)>,
    pub is_exported: bool,
}
```

`SymbolKind` is one of `Function`, `Variable`, `Class`, `Interface`, `TypeAlias`, `Enum`, `EnumMember`, `Module`, `Property`, `Method`, `Constructor`, `Unknown`.

:::warning
Symbol ids are per file. They are small indexes into that file's symbol table, so two files can use the same id for unrelated symbols. `get_symbol_info`, `get_symbol_locations`, `get_symbol_uses` and `get_symbol_type` match an id across every file and can return results from the wrong one. `find_symbol` does not tell you which file an id belongs to. With more than one file loaded, keep the file next to the id: use `get_symbol_type_in_file`, read the symbol from `get_file_symbols(file)`, and filter locations and uses by file name.
:::

### Members and signatures

These methods look a type up by name, not by id.

| Method | Returns |
|---|---|
| `get_type_member_info(&self, type_name: &str, member_name: &str) -> Option<TypeMemberInfo>` | The member of the first type with that name, in your files and then in the standard library |
| `find_member_declaration(&self, type_name: &str, member_name: &str) -> Option<(String, Span)>` | First declaration site of that member |
| `get_member_hover(&self, type_name: &str, member_name: &str) -> Option<String>` | Hover text such as `(property) Sq.side: number`. The type is left out when none was recorded |
| `get_type_member_completions(&self, type_name: &str) -> Vec<(String, u32, Option<String>)>` | `(name, symbol flags, displayed type)` per member, following `extends` up to 8 levels |
| `lib_function_overloads(&self, name: &str) -> Option<Vec<OverloadSignature>>` | Overloads of a standard library function such as `parseInt` |
| `global_function_overloads(&self, name: &str) -> Vec<OverloadSignature>` | Overload signatures (declarations without a body) of a function declared in your files |
| `global_function_declaration_header(&self, name: &str) -> Option<String>` | Source text of a function declaration up to its body, for example `f(x: number): string` |

`TypeMemberInfo` has the fields `owner_name: String`, `member_name: String`, `flags: u32`, `declarations: Vec<(String, Span)>`, `type_display: Option<String>` and `overloads: Vec<OverloadSignature>`. The flags are the `SYM_*` constants of `tsc_rs_symbols`, for example `SYM_METHOD` and `SYM_PROPERTY`. `OverloadSignature` also comes from `tsc_rs_symbols`.

### Files and imports

| Method | Returns |
|---|---|
| `get_source_file(&self, file: &str) -> Option<&str>` | The stored text of a file |
| `source_file_names(&self) -> Vec<String>` | Every loaded path, in no particular order. Includes declaration files found automatically |
| `list_analyzed_files(&self) -> Vec<String>` | Paths that have check results, sorted |
| `get_dependencies(&self, file: &str) -> Vec<String>` | Loaded files that `file` imports directly, sorted |
| `get_dependent_files(&self, file: &str) -> Vec<String>` | Loaded files that import `file` directly, sorted |
| `resolve_import_path(&self, module_specifier: &str, from_file: &str) -> Option<String>` | The loaded file an import specifier points to |
| `file_is_module(&self, file: &str) -> bool` | `true` when the text has import, export or CommonJS syntax. This one does not normalize `file`: pass the normalized path |
| `ambient_string_module_names(&self) -> Vec<String>` | Names declared with `declare module "name"`, without relative and wildcard ones |

Dependencies only cover files that are loaded in the engine. An import of a file you did not add produces no edge.

## Examples

### Check files from disk

```rust
use tsc_rs_ast::CompilerOptions;
use tsc_rs_query::QueryEngine;

fn main() -> Result<(), String> {
    let options = CompilerOptions {
        strict: Some(true),
        ..Default::default()
    };
    let mut engine = QueryEngine::with_options(options);

    // Usage: check-files src/main.ts src/math.ts
    let files: Vec<String> = std::env::args().skip(1).collect();
    for path in &files {
        let absolute = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
        let text = std::fs::read_to_string(&absolute).map_err(|e| format!("{path}: {e}"))?;
        engine.add_source(absolute.to_string_lossy().into_owned(), text)?;
    }

    // Err means at least one error diagnostic. The engine stays queryable.
    if engine.check_all().is_err() {
        for line in engine.diagnostics() {
            eprintln!("{line}");
        }
    }

    for file in engine.list_analyzed_files() {
        println!("{file} imports {:?}", engine.get_dependencies(&file));
    }

    std::process::exit(if engine.error_count() == 0 { 0 } else { 1 });
}
```

### Type and symbol under a cursor

```rust
use tsc_rs_query::QueryEngine;

fn main() -> Result<(), String> {
    let file = "demo.ts";
    let source = "function area(w: number, h: number) {\n  return w * h;\n}\nconst size = area(2, 3);\n";

    let mut engine = QueryEngine::new();
    engine.add_source(file.to_string(), source.to_string())?;
    let _ = engine.check_all();

    // Offsets are byte offsets into the source text.
    let offset = source.find("size").unwrap() as u32;

    if let Some(type_id) = engine.get_type_at(file, offset) {
        println!("type: {:?}", engine.type_to_string(type_id));
    }

    if let Some(symbol_id) = engine.get_symbol_at(file, offset) {
        if let Some(info) = engine.get_symbol_info(symbol_id) {
            println!("{} is a {:?}, exported: {}", info.name, info.kind, info.is_exported);
        }
        for (path, span) in engine.get_symbol_uses(symbol_id) {
            println!("{path}: {}..{}", span.start, span.end);
        }
    }
    Ok(())
}
```

With a single file loaded, symbol ids are unambiguous. The program prints:

```bash
type: Some("number")
size is a Variable, exported: false
demo.ts: 62..66
```

## Limits

Query results are only as good as the checker behind them. The type checker is incomplete (see [Limitations](/docs/limitations)), so a missing diagnostic, an `any` where `tsc` infers a precise type, or a `None` from a type query can be a gap in tsc-rs and not a fact about your code. The [conformance report](/conformance) gives the current numbers.

## Stability

- The crates are not published to crates.io. Use a git dependency.
- There is no stable API. Function signatures, struct fields and behaviour change between commits without a deprecation period. Pin a `rev` and read the diff before you move it.
- The crates are the internals of the compiler, exposed as they are. For the overall layout see [Architecture](/docs/architecture). For a ready-made browser build of the same pipeline see [WebAssembly](/docs/wasm).
