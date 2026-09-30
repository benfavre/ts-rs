---
title: tsc-rs analyze
---
# tsc-rs analyze

`tsc-rs analyze` runs fast, parser-level static analysis over a TypeScript monorepo. It does not type-check. It parses TypeScript and JavaScript files, resolves their imports into an import graph, and runs audits against that graph.

## analyze externals

Determines which entries of a Next.js `serverExternalPackages` list are actually reachable from your source code.

| Category | Meaning | Action |
|---|---|---|
| Unreachable | No file in the import graph references the package | Safe to remove |
| Type-only | Referenced only through `import type` | Safe to remove |
| Runtime | Referenced with runtime imports | Keep |

### Usage

```bash
tsc-rs analyze externals --entry <dir> --externals <file> [-p <tsconfig.json>] [--json]
```

| Flag | Meaning |
|---|---|
| `--entry <dir>` | Source directory to scan. `.ts` and `.tsx` files are discovered recursively |
| `--externals <file>` | File that contains the externals list |
| `-p <tsconfig.json>` | Load path aliases from a tsconfig. Optional |
| `--json` | Print JSON instead of the human-readable report |

The `--externals` file can be a JSON array (`["pino", "sharp"]`), a `next.config.mjs` (the `serverExternalPackages` array is extracted), or plain text with one package name per line.

### Example

```bash
tsc-rs analyze externals \
  --entry apps/app/src \
  -p apps/app/tsconfig.json \
  --externals apps/app/next.config.mjs
```

```bash
Externals Audit: 46 runtime, 8 type-only, 57 unreachable. 65 removable out of 111
Scanned 7856 files, found 285 unique packages

=== UNREACHABLE (57): safe to remove ===
  @auth/prisma-adapter
  @babel/runtime
  ...

=== TYPE-ONLY (8): safe to remove (no runtime effect) ===
  @acme/services-calendar  (1 imports)
  ...

=== RUNTIME (46): keep ===
  @aws-sdk/client-s3  (48 imports)
  ...
```

The exact punctuation of the report may differ; the structure is as shown.

## analyze dep-graph

Shows which files have the most imports, which reveals architectural bottlenecks, for example every route pulling in an entire generated database client.

### Usage

```bash
tsc-rs analyze dep-graph --entry <dir> [-p <tsconfig.json>] [--top <N>]
```

`--top` sets how many files to show, 20 by default.

### Example

```bash
tsc-rs analyze dep-graph --entry apps/app/src/server/api/routers --top 10
```

```bash
Top 10 files by import fan-out:
Count  File
1193   packages/db/dist/prisma-client/models.d.ts
11     packages/shared-types/dist/index.d.ts
9      apps/app/src/server/api/routers/__tests__/generated/shop.test.ts
...

Total: 1319 files, 4784 edges, 22 unique packages
```

## How it works

1. **File discovery.** Finds `.ts` and `.tsx` files recursively, skipping `node_modules`, `.next`, `dist`, `.git`, `.turbo` and `.bext`.
2. **Parsing.** Uses the tsc-rs parser. No type checking is needed.
3. **Specifier extraction.** Walks the whole AST, including function bodies, class methods and `try` blocks, and collects static imports, re-exports, dynamic `import()` calls and `require()` calls. It records whether each import is type-only.
4. **Resolution.** Uses the tsc-rs Node module resolver, with path aliases from the tsconfig.
5. **Classification.** Relative imports are followed into the file. Path-aliased imports resolve to local files and are not counted as packages. Node built-ins are filtered out. External packages are recorded with the import chain that reaches them.

## Performance

On a real monorepo, 5,700 entry files (more than 7,800 files walked once resolved imports are followed, 285 unique packages) take about 3.5 seconds. There is no type checking, no bundler and no Node.js process involved.

## Reading the results

- **Unreachable.** Cross-check against dynamic loaders that the parser cannot trace. A package loaded through an opaque dynamic import is invisible to static analysis.
- **Type-only.** Safe to remove unless the package is also loaded dynamically at run time.
- **Runtime.** Keep. The import count helps you prioritize: hundreds of imports means core infrastructure, one import may be a rarely used path.

Transitive dependencies of an externalized package do not need to be listed. Only packages imported directly from your source do.

## Limitations

- **Dynamic specifiers.** `require(variable)` and `import()` with a non-literal argument cannot be resolved.
- **Conditional requires.** `if (condition) require("pkg")` is detected, but the condition is not evaluated: the package always counts as used.
- **node_modules boundary.** The walker stops at `node_modules`, so dependencies inside packages are not traced.
- **Self-referencing tsconfig.** Handled, but `extends` chains are not followed in that case.
