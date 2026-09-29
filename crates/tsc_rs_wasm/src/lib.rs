//! WASM bindings for tsc-rs — compile and type-check TypeScript in the browser,
//! on the edge (Cloudflare Workers), or embedded in any application.
//!
//! ## Usage (JavaScript)
//!
//! ```js
//! import init, { compile, validate } from '@tsc-rs/wasm';
//!
//! await init();
//!
//! // Compile TypeScript to JavaScript
//! const result = compile('const x: number = 42;', { target: 'es2020' });
//! console.log(result.js);
//!
//! // Type-check without emitting
//! const diagnostics = validate('const x: number = "oops";');
//! console.log(diagnostics);
//! ```

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Options for compilation.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompileOptions {
    /// Target ECMAScript version (e.g. "es5", "es2015", "es2020", "esnext").
    #[serde(default)]
    pub target: Option<String>,

    /// Module system (e.g. "commonjs", "esnext", "amd").
    #[serde(default)]
    pub module: Option<String>,

    /// JSX mode (e.g. "react", "react-jsx", "preserve").
    #[serde(default)]
    pub jsx: Option<String>,

    /// Enable source map generation.
    #[serde(default)]
    pub source_map: bool,

    /// Enable declaration (.d.ts) generation.
    #[serde(default)]
    pub declaration: bool,

    /// Enable strict mode.
    #[serde(default)]
    pub strict: bool,

    /// File name for diagnostics and source maps.
    #[serde(default)]
    pub file_name: Option<String>,
}

/// Result of compilation.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileResult {
    /// Emitted JavaScript source.
    pub js: String,

    /// Source map (if requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_map: Option<String>,

    /// Declaration file (if requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration: Option<String>,

    /// Diagnostic messages (errors/warnings).
    pub diagnostics: Vec<Diagnostic>,
}

/// A diagnostic message.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u32>,
}

fn parse_target(s: &str) -> tsc_rs_ast::ScriptTarget {
    use tsc_rs_ast::ScriptTarget;
    match s.to_ascii_lowercase().as_str() {
        "es3" => ScriptTarget::ES3,
        "es5" => ScriptTarget::ES5,
        "es2015" | "es6" => ScriptTarget::ES2015,
        "es2016" => ScriptTarget::ES2016,
        "es2017" => ScriptTarget::ES2017,
        "es2018" => ScriptTarget::ES2018,
        "es2019" => ScriptTarget::ES2019,
        "es2020" => ScriptTarget::ES2020,
        "es2021" => ScriptTarget::ES2021,
        "es2022" => ScriptTarget::ES2022,
        "es2023" => ScriptTarget::ES2023,
        "es2024" => ScriptTarget::ES2024,
        "es2025" => ScriptTarget::ES2025,
        _ => ScriptTarget::ESNext,
    }
}

fn parse_module(s: &str) -> tsc_rs_ast::ModuleKind {
    use tsc_rs_ast::ModuleKind;
    match s.to_ascii_lowercase().as_str() {
        "commonjs" | "cjs" => ModuleKind::CommonJS,
        "amd" => ModuleKind::AMD,
        "umd" => ModuleKind::UMD,
        "system" => ModuleKind::System,
        "es2015" | "es6" => ModuleKind::ES2015,
        "es2020" => ModuleKind::ES2020,
        "es2022" => ModuleKind::ES2022,
        "esnext" => ModuleKind::ESNext,
        "node16" => ModuleKind::Node16,
        "node18" => ModuleKind::Node18,
        "node20" => ModuleKind::Node20,
        "nodenext" => ModuleKind::NodeNext,
        _ => ModuleKind::ESNext,
    }
}

fn parse_jsx(s: &str) -> tsc_rs_ast::JsxEmit {
    use tsc_rs_ast::JsxEmit;
    match s.to_ascii_lowercase().as_str() {
        "react" => JsxEmit::React,
        "react-jsx" | "reactjsx" => JsxEmit::ReactJSX,
        "react-jsxdev" | "reactjsxdev" => JsxEmit::ReactJSXDev,
        "preserve" => JsxEmit::Preserve,
        _ => JsxEmit::React,
    }
}

/// Compile TypeScript source to JavaScript.
///
/// Returns a JSON-serialized `CompileResult` with the emitted JS, optional
/// source map, optional declaration, and any diagnostics.
#[wasm_bindgen]
pub fn compile(source: &str, options: JsValue) -> Result<JsValue, JsError> {
    let opts: CompileOptions = if options.is_undefined() || options.is_null() {
        CompileOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(|e| JsError::new(&e.to_string()))?
    };

    let file_name = opts
        .file_name
        .as_deref()
        .unwrap_or("input.ts")
        .to_string();

    let mut compiler_options = tsc_rs_ast::CompilerOptions::default();

    if let Some(ref t) = opts.target {
        compiler_options.target = Some(parse_target(t));
    }
    if let Some(ref m) = opts.module {
        compiler_options.module = Some(parse_module(m));
    }
    if let Some(ref j) = opts.jsx {
        compiler_options.jsx = Some(parse_jsx(j));
    }
    compiler_options.source_map = Some(opts.source_map);
    compiler_options.declaration = Some(opts.declaration);
    if opts.strict {
        compiler_options.strict = Some(true);
    }

    // Parse
    let source_file = tsc_rs_parser::parse(&file_name, source);

    // Collect parse diagnostics
    let diagnostics: Vec<Diagnostic> = source_file
        .diagnostics
        .iter()
        .map(|d| convert_diagnostic(source, d))
        .collect();

    // Emit
    let emit_output = tsc_rs_emitter::emit(&source_file, &compiler_options);

    let result = CompileResult {
        js: emit_output.javascript,
        source_map: if opts.source_map {
            emit_output.source_map
        } else {
            None
        },
        declaration: emit_output.declaration_file,
        diagnostics,
    };

    serde_wasm_bindgen::to_value(&result).map_err(|e| JsError::new(&e.to_string()))
}

/// Type-check TypeScript source and return diagnostics only (no emit).
///
/// Returns a JSON array of diagnostic objects.
#[wasm_bindgen]
pub fn validate(source: &str, options: JsValue) -> Result<JsValue, JsError> {
    let opts: CompileOptions = if options.is_undefined() || options.is_null() {
        CompileOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(|e| JsError::new(&e.to_string()))?
    };

    let file_name = opts
        .file_name
        .as_deref()
        .unwrap_or("input.ts")
        .to_string();

    let mut compiler_options = tsc_rs_ast::CompilerOptions::default();
    if opts.strict {
        compiler_options.strict = Some(true);
    }
    if let Some(ref t) = opts.target {
        compiler_options.target = Some(parse_target(t));
    }

    // Parse
    let source_file = tsc_rs_parser::parse(&file_name, source);

    // Bind
    let binder = tsc_rs_symbols::Binder::new();
    let symbol_table = binder.bind(&source_file);

    // Type-check
    let checker = tsc_rs_types::TypeChecker::new();
    let check_output = checker.check_with_options(&source_file, &symbol_table, &compiler_options);

    // Collect all diagnostics (parse + type-check)
    let mut diagnostics: Vec<Diagnostic> = source_file
        .diagnostics
        .iter()
        .map(|d| convert_diagnostic(source, d))
        .collect();
    diagnostics.extend(
        check_output
            .diagnostics
            .iter()
            .map(|d| convert_diagnostic(source, d)),
    );

    serde_wasm_bindgen::to_value(&diagnostics).map_err(|e| JsError::new(&e.to_string()))
}

/// Return the tsc-rs version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn convert_diagnostic(source: &str, diag: &tsc_rs_ast::Diagnostic) -> Diagnostic {
    let (line, col) = if let Some(ref span) = diag.span {
        line_col_from_pos(source, span.start as usize)
    } else {
        (0, 0)
    };
    Diagnostic {
        message: diag.message.clone(),
        line,
        column: col,
        severity: "error".to_string(),
        code: Some(diag.code),
    }
}

fn line_col_from_pos(source: &str, pos: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= pos {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
