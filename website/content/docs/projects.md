---
title: Projects, watch and incremental builds
---
# Projects, watch and incremental builds

`tsc-rs -p tsconfig.json` compiles a whole project: it reads the config, selects the source files, type-checks them and writes JavaScript. This page covers project builds, output layout, declarations and source maps, incremental builds, project references, watch mode and parallelism. Several of these behave differently from `tsc` in version 0.4.1, and each section says where. The differences were re-checked with a build of the public `main` branch on 30 September 2026. In the samples, absolute paths are shortened to `/work`.

## Compile a project

```bash
tsc-rs -p tsconfig.json
```

- `-p` (or `--project`) takes the path of a config file. A directory is not accepted: `tsc-rs -p .` fails with TS6053.
- `tsc-rs` with no arguments prints the help. With at least one flag and no file names, it uses `./tsconfig.json` if there is one, so `tsc-rs --noEmit` checks the project in the current directory.
- File names given next to `-p` are ignored. The config decides what is compiled.
- Flags override the config: `--outDir`, `--target`, `--module`, `--jsx`, `--strict`, `-d`, `--sourceMap`, `--noEmit`, `--incremental`, `--composite`.

`files`, `include`, `exclude` and `extends` select the sources, see [tsconfig support](/docs/tsconfig). One difference matters: when a config has neither `files` nor `include`, tsc-rs takes only the `.ts` and `.tsx` files directly inside the config directory. `tsc` would take every subdirectory as well. Write an explicit `"include": ["src/**/*.ts"]`.

Diagnostics in a project build carry absolute paths. If any file has an error, nothing is written for any file. `--transpileOnly -p tsconfig.json` skips the checker and always emits.

## Output layout

For this project:

```json
{
  "compilerOptions": {
    "target": "es2022",
    "module": "esnext",
    "strict": true,
    "outDir": "dist"
  },
  "include": ["src/**/*.ts"]
}
```

with `src/index.ts` importing `./lib/math` from `src/lib/math.ts`, `tsc-rs -p tsconfig.json` writes:

```bash
dist/index.js
dist/math.js
```

:::warning
`outDir` is flat. Every output is written as `<outDir>/<file name>.js`; the source directory structure is not recreated. `dist/index.js` above still contains `import { add } from "./lib/math";`, which no longer resolves. Two sources with the same base name, such as `src/index.ts` and `src/util/index.ts`, overwrite each other without a warning. `tsc` writes `dist/lib/math.js`.
:::

Other things to know about output paths:

- Without `outDir`, each `.js` file is written next to its source. This is the layout that keeps relative imports working for nested sources.
- A relative `outDir` (and `tsBuildInfoFile`) is resolved against the current directory, not the directory of the config. Run tsc-rs from the project directory.
- `rootDir`, `outFile`, `declarationDir` and `declarationMap` in the config do not change where or what tsc-rs writes.
- A `.d.ts` file that is part of the project is emitted too: `globals.d.ts` produces `globals.d.js`, which holds only `"use strict";` and can be deleted. In a `--transpileOnly` build the same happens to `@types` packages that are loaded automatically.
- JSX sources always produce `.js`. See [JSX](/docs/jsx).

## Declarations and source maps

```bash
tsc-rs -p tsconfig.json -d --sourceMap
```

```bash
dist/index.d.ts
dist/index.js
dist/index.js.map
dist/math.d.ts
dist/math.js
dist/math.js.map
```

`-d` writes a `.d.ts` file per source, next to the `.js`. This is `dist/math.d.ts`:

```ts
export declare function add(a: number, b: number): number;
```

`--sourceMap` writes a `.js.map` file and appends `//# sourceMappingURL=index.js.map`. `inlineSourceMap` is read from `tsconfig.json` only, there is no command-line flag for it, and it appends the map as a base64 `data:` URL instead.

```json
{"version":3,"file":"index.js","sourceRoot":"","sources":["index.ts"],"names":[],"mappings":"AAAA;AAEA,sBAA6B"}
```

Two limits show in that map. `sources` holds the bare file name, not a path from the output directory back to the source (`tsc` writes `../src/index.ts`), so a debugger will not find the file when `outDir` is used. And mappings are coarse: one or two segments per line, where `tsc` maps every token.

Declaration emit is early. In the example, `index.d.ts` keeps an `import` that `tsc` removes. Compare against `tsc` before you publish types.

## List the files

`--listFiles` prints the files in the compilation, one absolute path per line, then compiles. `--listFilesOnly` prints them and exits.

```bash
tsc-rs -p tsconfig.json --listFilesOnly
```

```bash
/work/app/src/index.ts
/work/app/src/lib/math.ts
```

The list holds the files selected by the config plus automatically loaded `@types` entry points. Unlike `tsc`, it does not include the `lib.*.d.ts` files or files that are only reached through an import.

## Incremental builds

`--incremental`, or `"incremental": true`, stores a record of the build in a `.tsbuildinfo` file and recompiles only what changed.

```bash
tsc-rs -p tsconfig.json --incremental
```

```json
{
  "version": "0.4.1",
  "optionsHash": "a97a5e03080b3b47",
  "fileHashes": {
    "/work/app/src/index.ts": "50dfa0e81c4df7b2",
    "/work/app/src/lib/math.ts": "5b880e2cb07e7bde"
  },
  "dependencies": {
    "/work/app/src/index.ts": ["/work/app/src/lib/math.ts"]
  }
}
```

On the next run a file is recompiled when its content hash changed, when it is new, or when a file it imports changed. Editing `math.ts` rebuilds `math.js` and `index.js`; editing `index.ts` rebuilds only `index.js`. If the hashed options changed (`target`, `module`, `strict`, `declaration`, `sourceMap`, `outDir`, `noEmit`, `jsx`), everything is rebuilt.

The file is written to `tsBuildInfoFile` if set, else to `<outDir>/<config name>.tsbuildinfo` (`dist/tsconfig.tsbuildinfo` here), else next to the config. The format is specific to tsc-rs. It is not the format `tsc` writes.

:::warning
The build record is saved even when the build fails. If a run reports a type error and you run the same command again without editing anything, the second run finds nothing to recompile, prints nothing and exits with status 0, while the output on disk is still the one from before the error. In CI, do not trust an incremental run that follows a failed one: delete the `.tsbuildinfo` file or build without `--incremental`.
:::

Also:

- Only inputs are tracked. If you delete `dist/math.js` and nothing changed in the sources, it is not written again.
- With `--noEmit`, no `.tsbuildinfo` is written.

## Composite projects

`--composite` on the command line turns on `--incremental` and `-d`. `"composite": true` in `tsconfig.json` turns on incremental builds only: add `"declaration": true` yourself if you want `.d.ts` files.

## Project references

```bash
tsc-rs -b tsconfig.json
```

`-b` (or `--build`) must be the first argument. It reads the `references` of the given config, which defaults to `./tsconfig.json`, orders the projects so that a referenced project comes before the one that references it, and compiles them in that order. A reference `path` may be a directory or a config file. A cycle is an error.

```bash
Built 3 project(s) in order: /work/core/tsconfig.json -> /work/app/tsconfig.json -> /work/tsconfig.json
```

:::warning
In 0.4.1, `-b` type-checks the projects but writes nothing: no `.js`, no `.d.ts` and no `.tsbuildinfo`. To produce output, run `tsc-rs -p` on each project, in dependency order.
:::

Other gaps in build mode:

- Only the references of the config you pass are followed. References of a referenced project are not built.
- The argument must be a config file, not a directory. Only the first argument after `-b` is read; anything after it, such as a second config, `--verbose` or `--clean`, is ignored without an error.
- Errors are listed per project with a code and a message but no file or position. Use `tsc-rs -p <project> --noEmit` to locate them.
- `tsc-rs -p` on a project that has `references` compiles that project alone.

## Watch mode

```bash
tsc-rs -w -p tsconfig.json
```

tsc-rs compiles once, then checks the project every 500 ms. It recompiles when the content of a source file changes (a `touch` alone does nothing), when `tsconfig.json` changes, and when a file matching `include` appears or disappears. Status lines and diagnostics go to stderr:

```bash
  New file detected: /work/app/src/extra.ts
File change detected. Starting incremental compilation...

Watching for file changes...

File change detected. Starting incremental compilation...

/work/app/src/extra.ts(1,14): error TS2322: Type 'number' is not assignable to type 'string'.

Found 1 error in 1 file.
Watching for file changes...
```

That excerpt shows a new file being picked up and compiled, then an edit that introduces an error.

- The screen is cleared before each recompile. `--preserveWatchOutput` keeps the previous output.
- `-w` also works with file names instead of `-p`. New files are then not picked up.
- Despite the message, each change recompiles the whole project unless `--incremental` is set.
- As in a normal build, a type error means no file is written for that round, and the output of a deleted source is not removed.
- The process runs until you stop it with Ctrl+C.

## Parallelism

`--jobs <N>`, `-j <N>`, `--jobs=<N>` or the `TSC_RS_JOBS` environment variable set the number of worker threads. The flag wins over the variable. `--jobs 1` makes the run sequential, which helps when you reproduce a problem. The default is one worker per logical CPU: 24 on a machine with 12 cores and 24 threads. A value that is not a positive number is ignored without an error.

## Summary of gaps

| Feature | Status in 0.4.1 |
|---|---|
| `-p`, `include`, `exclude`, `files`, `extends` | Works. `-p` needs a file path; default `include` is not recursive |
| `--outDir` | Flat, relative to the current directory |
| `-d`, `--sourceMap`, `inlineSourceMap` | Work. Maps are coarse and `sources` is a bare file name |
| `--incremental` | Works. A failed build is recorded as built |
| `--composite` | Shortcut for incremental, plus declarations on the command line only |
| `-b` | Checks direct references in order, writes no output |
| `-w` | Works, polling every 500 ms |

See [Limitations](/docs/limitations) for the type checker and emit coverage, and the [CLI reference](/docs/cli) for every flag.
