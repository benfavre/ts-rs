---
title: CLI reference
---
# CLI reference

```bash
tsc-rs [options] [file ...]
```

This page lists what `tsc-rs --help` prints for version 0.4.1, grouped the same way, with notes where the behaviour was checked and differs from the help text.

## Compilation

| Command | What it does |
|---|---|
| `tsc-rs file.ts` | Compile one or more TypeScript files |
| `tsc-rs -p tsconfig.json` | Compile the project described by a `tsconfig.json` |
| `tsc-rs --noEmit` | Type-check only, write no output files |
| `tsc-rs -w` | Watch mode: recompile on changes |
| `tsc-rs --init` | Write a default `tsconfig.json` |
| `tsc-rs -b [tsconfig.json]` | Check project references in dependency order. Writes no output in 0.4.1, see [Projects and watch](/docs/projects) |

## Compiler flags

| Flag | Meaning |
|---|---|
| `--outDir <dir>` | Write output to a directory. The layout is flat: source subdirectories are not recreated |
| `--target <target>` | ECMAScript target: `es5`, `es2015`, `esnext`, and the versions in between |
| `--module <module>` | Module system: `commonjs`, `es2015`, `esnext`, and others |
| `--declaration`, `-d` | Generate `.d.ts` declaration files |
| `--sourceMap` | Generate source map files |
| `--listFiles` | Print every source file in the compilation |
| `--listFilesOnly` | Print the source files and exit |
| `--strict` | Enable all strict type-checking options |
| `--incremental` | Enable incremental compilation |
| `--composite` | Enable composite project (implies `--incremental -d`) |
| `--preserveTypeAnnotations` | Keep `as`, `satisfies` and angle-bracket assertions in the output |
| `--preserveComments` | Keep comments even when `removeComments` is set |
| `--preserveWhitespace` | Keep whitespace in the output |
| `--jobs`, `-j <N>` | Parallel worker count. Default: one per logical CPU (the help text says physical cores). Also `TSC_RS_JOBS` |
| `--maxErrors <N>` | Cap the diagnostics printed; the summary still reflects the full count |

The three `--preserve*` flags are specific to tsc-rs. See [Preserve modes](/docs/preserve-modes).

## JSX

| Flag | Meaning |
|---|---|
| `--jsx <mode>` | JSX emit mode: `react`, `react-jsx`, `react-jsxdev`, `preserve`. See [JSX](/docs/jsx) |
| `--jsxFactory <factory>` | JSX factory function. Default: `React.createElement` |
| `--jsxImportSource <source>` | JSX import source. Default: `react` |

## Transform modes

| Flag | Meaning |
|---|---|
| `--transpileOnly` | Skip type checking: parse and emit only |
| `--fast-emit` | Skip cosmetic formatting normalization. Statements that need no transform are copied verbatim. Valid JavaScript, not `tsc`-formatted |
| `--pipe` | Persistent transpile-only process, JSON requests on stdin. See [Transpile pipe](/docs/pipe) |
| `--check-pipe -p tsconfig.json` | Persistent type-check daemon, JSON requests on stdin. See [Type-check daemon](/docs/check-pipe) |

Parser diagnostics are still reported in `--transpileOnly` mode: a file that does not parse is an error, not silently emitted.

## Watch

| Flag | Meaning |
|---|---|
| `--watch`, `-w` | Recompile on changes |
| `--preserveWatchOutput` | Do not clear the screen on recompile |

## Analysis

| Command | What it does |
|---|---|
| `tsc-rs analyze externals` | Audit `serverExternalPackages` against the import graph |
| `tsc-rs analyze dep-graph` | Show dependency fan-out from entry files |

See [tsc-rs analyze](/docs/analyze).

## Other

| Flag | Meaning |
|---|---|
| `--lsp` | Start the language server on stdin and stdout. See [Language server](/docs/language-server) |
| `--version`, `-v` | Print the version |
| `--help`, `-h` | Print help |

## Harness commands

The binary also exposes two commands used by the test harness:

```bash
tsc-rs discover <ts-repo> <suite>
tsc-rs run-suite <ts-repo> <suite> '<oracle>' '<candidate>' [max]
```

`suite` is `compiler`, `fourslash` or `project`. Day to day, use the dedicated harness binaries described in [Testing and the harness](/docs/testing).

## Environment variables

| Variable | Default | Effect |
|---|---|---|
| `TSC_RS_JOBS` | logical CPUs | Parallel worker count, same as `--jobs` |
| `TSC_RS_STACK_MB` | 128 | Stack size of the main compile thread, in MB |
| `TSC_RS_WORKER_STACK_MB` | 32 | Stack size of each parallel worker thread, in MB |
| `TSC_RS_MAX_VMSIZE_MB` | 12288 | Cap on the process's virtual address space, in MB (Unix only). `0` disables |
| `TSC_RS_PIPE_MAX_REQUESTS` | 5000 | `--pipe` exits cleanly after this many requests. `0` disables |
| `TSC_RS_PIPE_MAX_MEMORY_MB` | 2048 | `--pipe` exits cleanly above this resident memory. `0` disables |
| `TSC_RS_CHECK_PIPE_MAX_REQUESTS` | 10000 | Same cap for `--check-pipe` |
| `TSC_RS_CHECK_PIPE_MAX_MEMORY_MB` | 2048 | Same cap for `--check-pipe` |
| `TSC_RS_TYPESCRIPT_LIB_DIR` | | Directory holding TypeScript's `lib.*.d.ts` files, for the standard-library tests |

## Exit status

`tsc-rs` exits with `0` when compilation succeeds and `1` when it reports errors.

Unlike `tsc`, version 0.4.1 writes no output files when the type checker reports errors. Use `--transpileOnly` to emit regardless of type errors.
