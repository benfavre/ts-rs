use super::*;

impl TypeChecker {
    pub(super) fn missing_module_diagnostic(&self, source: &str, span: Span) -> Diagnostic {
        // Node's built-ins need the Node declaration package. This suggestion
        // takes precedence over classic resolution's generic paths suggestion.
        // Side-effect imports select their separate diagnostic at the caller.
        if is_node_builtin(source) {
            let wildcard_types = self
                .compiler_options
                .types
                .as_ref()
                .is_some_and(|types| types.iter().any(|name| name == "*"));
            return Diagnostic {
                code: if wildcard_types { 2580 } else { 2591 },
                message: format!(
                    "Cannot find name '{source}'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node`{}.",
                    if wildcard_types { "" } else { " and then add 'node' to the types field in your tsconfig" },
                ),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            };
        }
        if self.effective_module_resolution_is_classic() {
            error_cannot_find_module_classic(source, span)
        } else {
            error_cannot_find_module(source, span)
        }
    }
}

fn is_node_builtin(source: &str) -> bool {
    if matches!(
        source,
        "node:quic" | "node:sea" | "node:sqlite" | "node:test" | "node:test/reporters"
    ) {
        return true;
    }
    // Only recognized built-ins receive the suggestion: `node:missing` and
    // similarly named packages or relative paths still use module-not-found.
    matches!(
        source.strip_prefix("node:").unwrap_or(source),
        "assert"
            | "assert/strict"
            | "async_hooks"
            | "buffer"
            | "child_process"
            | "cluster"
            | "console"
            | "constants"
            | "crypto"
            | "dgram"
            | "diagnostics_channel"
            | "dns"
            | "dns/promises"
            | "domain"
            | "events"
            | "fs"
            | "fs/promises"
            | "http"
            | "http2"
            | "https"
            | "inspector"
            | "inspector/promises"
            | "module"
            | "net"
            | "os"
            | "path"
            | "path/posix"
            | "path/win32"
            | "perf_hooks"
            | "process"
            | "punycode"
            | "querystring"
            | "readline"
            | "readline/promises"
            | "repl"
            | "stream"
            | "stream/consumers"
            | "stream/promises"
            | "stream/web"
            | "string_decoder"
            | "sys"
            | "timers"
            | "timers/promises"
            | "tls"
            | "trace_events"
            | "tty"
            | "url"
            | "util"
            | "util/types"
            | "v8"
            | "vm"
            | "wasi"
            | "worker_threads"
            | "zlib"
    )
}
