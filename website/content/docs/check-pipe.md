---
title: Type-check daemon
---
# Type-check daemon

`tsc-rs --check-pipe` is a long-running type checker. It loads a project once, then answers "check this file" requests over JSON lines. It is built for editors and coding agents, where the question is asked many times against the same project.

The cost model:

- **Start-up** parses and binds the whole project once. That is a few seconds on a monorepo of 5,000 files.
- **Each request** re-checks a single file against the cached project state, typically in tens of milliseconds.

## Start

Any of these forms works. With no argument, `./tsconfig.json` is used.

```bash
tsc-rs --check-pipe -p tsconfig.json
tsc-rs --check-pipe tsconfig.json
tsc-rs --check-pipe
```

After initialization the daemon prints one banner line:

```json
{"event":"ready","files":5732,"elapsed_ms":4321}
```

If any project file could not be read, one more line follows:

```json
{"event":"init_diagnostics","diagnostics":[]}
```

## Requests

One JSON object per line.

Check a file as it is on disk:

```json
{"id":"1","cmd":"check","file":"src/foo.ts"}
```

Check a file with unsaved content:

```json
{"id":"1","cmd":"check","file":"src/foo.ts","source":"export const x: number = 1;"}
```

Check several files in one request:

```json
{"id":"2","cmd":"check_files","files":[{"file":"a.ts"},{"file":"b.ts","source":"..."}]}
```

Stop the daemon:

```json
{"id":"3","cmd":"shutdown"}
```

## Response

```json
{
  "id": "1",
  "ok": true,
  "elapsed_ms": 48,
  "file": "src/foo.ts",
  "diagnostics": [
    {
      "file": "src/foo.ts",
      "line": 117,
      "col": 38,
      "code": 2345,
      "severity": "error",
      "message": "Argument of type 'number' is not assignable to parameter of type 'number[]'."
    }
  ]
}
```

`line` and `col` are 1-based, matching the command line output.

## Staleness

The project state is built once at start-up. While the daemon runs, edits to files other than the one being checked are not visible to it.

- To answer "did I break this file?", send a `check` for that file. This is the common case and it is always current for that file.
- To answer "did changing A break B?", send both files in one `check_files` request.
- To pick up other cross-file changes, restart the daemon.

## Lifetime caps

| Variable | Default | Effect |
|---|---|---|
| `TSC_RS_CHECK_PIPE_MAX_REQUESTS` | 10000 | Exit cleanly after this many requests |
| `TSC_RS_CHECK_PIPE_MAX_MEMORY_MB` | 2048 | Exit cleanly when resident memory passes this value |

When a cap is reached the daemon answers the current request and exits, so a supervisor can respawn it. Set `TSC_RS_INIT_TIMING` to log a timing breakdown of start-up.

## Accuracy

The daemon uses the same checker as `tsc-rs --noEmit`, with the same gaps. See [Limitations](/docs/limitations).
