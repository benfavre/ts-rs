---
title: tsconfig support
---
# tsconfig support

`tsc-rs -p tsconfig.json` loads the project file, follows `extends`, resolves `files`, `include` and `exclude`, and reads the common `compilerOptions`. Options that are not listed here are not applied yet, and a few that are listed are read without taking full effect: see [Known gaps](#known-gaps).

## compilerOptions that are read

| Group | Options |
|---|---|
| Target and modules | `target`, `module`, `moduleResolution`, `jsx` |
| Emit | `noEmit`, `declaration`, `sourceMap`, `inlineSourceMap`, `outDir`, `outFile` |
| Strictness | `strict`, `noImplicitAny`, `noImplicitReturns`, `noUnusedLocals`, `noUnusedParameters`, `strictNullChecks`, `strictFunctionTypes` |
| Interop and features | `esModuleInterop`, `allowSyntheticDefaultImports`, `allowJs`, `checkJs`, `resolveJsonModule`, `experimentalDecorators`, `emitDecoratorMetadata` |
| Incremental and projects | `incremental`, `composite`, `tsBuildInfoFile` |
| Paths | `baseUrl`, `paths`, `lib`, `rootDir` |
| Preserve modes | `preserveTypeAnnotations`, `preserveComments`, `preserveWhitespace` |

## Project structure

- `files`, `include` and `exclude` select the source files.
- `extends` is followed, so shared base configs work.
- `.tsbuildinfo` files are written for incremental and composite projects, in a format specific to tsc-rs.
- `tsc-rs -b tsconfig.json` orders and checks project references.

## Known gaps

Checked against version 0.4.1. [Projects and watch](/docs/projects) and [JSX](/docs/jsx) show each one with real output.

- **`outDir` is flat.** Output files are written directly into `outDir`; source subdirectories are not recreated. `rootDir`, `outFile`, `declarationDir` and `declarationMap` do not change what is written.
- **Default `include` is not recursive.** A config with neither `files` nor `include` takes only the files directly inside the config directory. Write an explicit `include`.
- **Relative paths.** A relative `outDir` or `tsBuildInfoFile` resolves against the current directory, not the config's directory.
- **JSX options.** Only `jsx` is read from the config. `jsxFactory`, `jsxFragmentFactory` and `jsxImportSource` are ignored there; pass the command line flags or use a pragma.
- **`composite`.** In the config it turns on incremental builds only. Add `declaration` yourself.
- **`-b` writes nothing.** Build mode type-checks the referenced projects but emits no files.

## Option validation

Conflicts between dependent options and module-resolution settings are reported with the same codes `tsc` uses, including TS5052, TS5095, TS5109 and TS5110.

## Preserve modes in tsconfig

The three preserve options are tsc-rs extensions. They can be set in `compilerOptions` like any other option:

```json
{
  "compilerOptions": {
    "target": "es2022",
    "module": "esnext",
    "preserveTypeAnnotations": true
  }
}
```

`tsc` does not know these options and rejects them, so keep them in a config file that only tsc-rs reads if you run both compilers. See [Preserve modes](/docs/preserve-modes).

## Starter config

`tsc-rs --init` writes this file:

```json
{
  "compilerOptions": {
    "target": "es2016",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  }
}
```

## Emit coverage

Default emit matches every `tsc` JavaScript baseline that has an oracle. Coverage is lower when every option variant of every test is expanded: {{metrics.expanded.js.percent}} of JavaScript variants and {{metrics.expanded.declarations.percent}} of declaration variants pass, with the failures concentrated in ES5 downlevel transforms and `.d.ts` emit. If you target ES5 or rely on declaration output, compare against `tsc` before you depend on it. Details are in the [conformance report](/conformance).
