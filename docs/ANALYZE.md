# tsc-rs analyze — Static Analysis for TypeScript Monorepos

The `analyze` subcommand provides fast, parser-level static analysis without requiring full type checking. It builds an import graph by parsing TypeScript/JavaScript files and resolving imports, then runs audits against it.

## Commands

### `tsc-rs analyze externals` — Audit serverExternalPackages

Determines which entries in Next.js `serverExternalPackages` are actually reachable from your source code. Reports three categories:

| Category | Meaning | Action |
|----------|---------|--------|
| **Unreachable** | No file in the import graph references this package | Safe to remove |
| **Type-only** | Referenced only via `import type` — no runtime effect | Safe to remove |
| **Runtime** | Referenced with runtime imports — needed by the bundler | Keep |

#### Usage

```bash
tsc-rs analyze externals \
  --entry <dir>              # Source directory to scan (discovers .ts/.tsx recursively)
  -p <tsconfig.json>         # Load path aliases from tsconfig (optional)
  --externals <file>         # File containing externals list
  [--json]                   # Output as JSON instead of human-readable
```

#### Externals file formats

The `--externals` flag accepts:

1. **JSON array**: `["pino", "sharp", "@aws-sdk/client-s3"]`
2. **next.config.mjs**: Extracts `serverExternalPackages: [...]` automatically
3. **One per line**: Plain text, one package name per line

#### Example

```bash
# Audit a Next.js app's externals
cd my-monorepo
tsc-rs analyze externals \
  --entry apps/app/src \
  -p apps/app/tsconfig.json \
  --externals apps/app/next.config.mjs

# JSON output for scripting
tsc-rs analyze externals \
  --entry apps/app/src \
  --externals externals.json \
  --json
```

#### Output

```
Externals Audit: 46 runtime, 8 type-only, 57 unreachable — 65 removable out of 111
Scanned 7856 files, found 285 unique packages

=== UNREACHABLE (57) — safe to remove ===
  @auth/prisma-adapter
  @babel/runtime
  ...

=== TYPE-ONLY (8) — safe to remove (no runtime effect) ===
  @acme/services-calendar  (1 imports)
    via: .../service-registry/src/types.ts → import type "..."
  ...

=== RUNTIME (46) — keep ===
  @aws-sdk/client-s3  (48 imports)
    via: .../upload/route.ts → import "@aws-sdk/client-s3"
  ...
```

### `tsc-rs analyze dep-graph` — Dependency Fan-Out

Shows which files have the most imports, revealing architectural bottlenecks (e.g., every route pulling in the entire Prisma schema).

#### Usage

```bash
tsc-rs analyze dep-graph \
  --entry <dir>              # Source directory to scan
  [-p <tsconfig.json>]       # Load path aliases (optional)
  [--top <N>]                # Show top N files by fan-out (default: 20)
```

#### Example

```bash
tsc-rs analyze dep-graph --entry apps/app/src/server/api/routers --top 10
```

#### Output

```
Top 10 files by import fan-out:
Count  File
--------------------------------------------------------------------------------
1193   packages/db/dist/prisma-client/models.d.ts
11     packages/shared-types/dist/index.d.ts
9      apps/app/src/server/api/routers/__tests__/generated/shop.test.ts
...

Total: 1319 files, 4784 edges, 22 unique packages
```

## How It Works

1. **File discovery**: Recursively finds `.ts`/`.tsx` files, skipping `node_modules`, `.next`, `dist`, `.git`, `.turbo`, `.bext`
2. **Parsing**: Uses tsc-rs's TypeScript parser (no type checking needed)
3. **Specifier extraction**: Deep-walks the entire AST including function bodies, class methods, arrow functions, object methods, try/catch blocks, etc. Extracts:
   - `import ... from "pkg"` / `import "pkg"` (static)
   - `export { ... } from "pkg"` / `export * from "pkg"` (re-exports)
   - `import("pkg")` (dynamic)
   - `require("pkg")` (CommonJS)
   - Tracks `import type` vs runtime imports per package
4. **Resolution**: Uses tsc-rs's Node module resolver with path alias support from tsconfig
5. **Classification**: For each specifier:
   - Relative imports (`./`, `../`) → follow into the file
   - Path-aliased imports (e.g., `@/*`) → resolved to local file, not counted as package
   - Node builtins (`fs`, `path`, `node:*`) → filtered out
   - External packages → recorded with import chain

## Performance

| Metric | Value |
|--------|-------|
| 5,700 entry files | ~3.5 seconds |
| 7,800+ files walked | (follows resolved imports) |
| 285 unique packages | (after filtering aliases/builtins) |

No type checking, no Turbopack, no Node.js — pure Rust parsing + resolution.

## Interpreting Results

The tool reports three categories. For a production audit, cross-reference:

1. **Unreachable** — check against dynamic loaders like `opaqueImport()` or `eval(import(...))` that the parser can't trace. Workspace service packages loaded via service-registry definitions are invisible to static analysis.

2. **Type-only** — safe to remove from `serverExternalPackages` UNLESS the package is loaded dynamically at runtime (e.g., via opaqueImport). Type-only imports don't affect bundles, but `serverExternalPackages` is a bundler directive that also prevents tree-tracing the package.

3. **Runtime** — keep. The import count helps prioritize: a package with 574 imports is core infrastructure; one with 1 import might be in a rarely-used code path.

Transitive dependencies of externalized packages (e.g., `@smithy/*` for `@aws-sdk/client-s3`) do NOT need to be in `serverExternalPackages` — Node.js resolves them from `node_modules` at runtime. Only packages directly imported from your source code need listing.

## Limitations

- **Dynamic specifiers**: `require(variable)` or `import(template)` with non-literal arguments cannot be resolved
- **`opaqueImport()` / service-registry**: Packages loaded via a service-registry pattern (opaque dynamic imports) are invisible to static analysis. Cross-reference with the definition files manually.
- **Conditional requires**: `if (condition) require("pkg")` is detected (deep walk), but the condition isn't evaluated — the package is always counted as used
- **Re-exports through node_modules**: The walker stops at `node_modules` boundaries by default, so transitive dependencies within packages aren't traced
- **Self-referencing tsconfig**: Handled gracefully (detected and loaded directly), but `extends` chains are not followed in this case
- **Path aliases**: Workspace packages resolved via tsconfig path mappings (e.g., `@acme/db` → `packages/db/dist/index.js`) are correctly recognized as packages. Project-internal aliases (`@/*`, `~/*`) are filtered out.
