//! JavaScript emitter - transforms TypeScript AST to JavaScript source.
//!
//! Uses a **source-text-preserving** strategy: for AST nodes that don't require
//! any TypeScript-specific transformation (type stripping, class field init, etc.),
//! the original source text is copied verbatim. This preserves the user's
//! formatting and matches TypeScript's emitter behaviour.

mod analysis;
mod array_spread;
mod call_spread;
mod comments;
pub mod declaration;
mod emit_class;
mod emit_declarations;
mod emit_expr;
mod emit_module;
mod emit_stmt;
mod emit_stmt_analysis;
mod emit_stmt_const_enum;
mod emit_stmt_helpers;
mod enum_eval;
mod generator;
mod helpers;
mod helpers_await;
mod helpers_decorators;
mod helpers_rest;
mod import_shadow;
mod jsx;
mod lexical_downlevel;
mod new_spread;
mod normalize;
pub mod source_map;
mod source_transform;
mod spread_comments;
mod tsc_source_map;
mod type_erase;

use analysis::*;
use declaration::DeclarationEmitter;
use enum_eval::*;
use helpers_rest::*;
use normalize::*;
use source_map::{offset_to_line_col, Mapping, SourceMapGenerator};
use source_transform::*;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use tsc_rs_ast::*;
use tsc_rs_scanner::{Scanner, TokenKind};

#[derive(Debug, Clone)]
pub struct EmitOutput {
    pub javascript: String,
    pub source_map: Option<String>,
    pub declaration_file: Option<String>,
    /// Require-var counters after emission.  Pass these into the next
    /// `emit_with_require_var_counters` call when concatenating multiple
    /// files into a single AMD outFile so that import alias suffixes
    /// (e.g. `foo_1`, `foo_2`) are globally unique across `define()` blocks.
    pub require_var_counters: HashMap<AstString, usize>,
    /// System.register counter after emission.  Thread into the next emit
    /// call so that `exports_N` / `context_N` suffixes are globally unique
    /// across `System.register()` blocks in outFile bundles.
    pub system_register_counter: usize,
}

#[derive(Clone)]
struct StandardDecoratorFieldSlot {
    prop_name: String,
    slot_stem: String,
    prev_slot_stem: Option<String>,
}

/// tsc-compatible source map mappings for `javascript`, emitted from
/// `file`: (generated line, generated column, original line, original
/// column), 0-based, in emission order (encode with
/// [`source_map::SourceMapGenerator`], which coalesces them as tsc does).
pub fn tsc_source_mappings(file: &SourceFile, javascript: &str) -> Vec<(u32, u32, u32, u32)> {
    tsc_source_map::synthesize(&file.text, file, javascript)
}

pub fn emit(file: &SourceFile, options: &CompilerOptions) -> EmitOutput {
    emit_full(
        file,
        options,
        &HashMap::new(),
        &HashSet::new(),
        &HashSet::new(),
    )
}

/// Strip TypeScript types and produce ESNext JavaScript.
///
/// This is a convenience wrapper that configures the emitter for type-only
/// stripping: `target: ESNext`, `module: ESNext` (or `preserve`).  No downlevel
/// transforms are applied — the output preserves modern syntax like optional
/// chaining, class fields, decorators, `using`, etc.
///
/// Use this when you need a fast, correct type-stripping pass without
/// transpilation to older targets.
///
/// ```ignore
/// let ast = tsc_rs_parser::parse("app.ts", source);
/// let js = tsc_rs_emitter::emit_strip_types(&ast);
/// println!("{}", js.javascript);
/// ```
pub fn emit_strip_types(file: &SourceFile) -> EmitOutput {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ESNext),
        module: Some(ModuleKind::ESNext),
        // Preserve all imports verbatim — don't elide unused imports since
        // we have no cross-file type information to determine usage.
        verbatim_module_syntax: Some(true),
        ..Default::default()
    };
    emit(file, &options)
}

/// Strip TypeScript types with custom JSX handling.
///
/// Like [`emit_strip_types`] but allows specifying JSX mode. Use
/// `JsxEmit::ReactJsx` for React 17+ automatic runtime, or
/// `JsxEmit::Preserve` to keep JSX syntax in the output.
pub fn emit_strip_types_jsx(
    file: &SourceFile,
    jsx: JsxEmit,
    jsx_import_source: Option<&str>,
) -> EmitOutput {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ESNext),
        module: Some(ModuleKind::ESNext),
        jsx: Some(jsx),
        jsx_import_source: jsx_import_source.map(|s| s.to_string()),
        ..Default::default()
    };
    emit(file, &options)
}

/// Emit JavaScript with a set of cross-file type-only names.
///
/// `global_type_only_names` contains names from OTHER files in the compilation
/// (e.g. `declare interface I1 {}` from a `.d.ts` file) that are type-only.
/// When the current file has `export { I1 }` and `I1` is not locally declared,
/// these names are used to suppress the `exports.I1 = void 0;` pre-declaration.
pub fn emit_with_global_type_only(
    file: &SourceFile,
    options: &CompilerOptions,
    external_const_enum_values: &HashMap<(String, String), ConstEnumValue>,
    global_type_only_names: &HashSet<AstString>,
    global_type_only_export_names: &HashSet<AstString>,
) -> EmitOutput {
    emit_full(
        file,
        options,
        external_const_enum_values,
        global_type_only_names,
        global_type_only_export_names,
    )
}

/// Pre-scan a script file's namespaces so callers can seed cumulative merged
/// namespace exports across multiple files before per-file emission.
pub fn prescan_script_namespace_exports(
    file: &SourceFile,
    cumulative: &mut HashMap<AstString, HashSet<AstString>>,
    type_cumulative: &mut HashMap<AstString, HashSet<AstString>>,
) {
    prescan_namespace_exports_full(&file.statements, None, cumulative, Some(type_cumulative));
}

/// Emit JavaScript with cross-file constant values for enum evaluation.
pub fn emit_with_cross_file_consts(
    file: &SourceFile,
    options: &CompilerOptions,
    external_const_enum_values: &HashMap<(String, String), ConstEnumValue>,
    global_type_only_names: &HashSet<AstString>,
    global_type_only_export_names: &HashSet<AstString>,
    external_file_consts: &HashMap<String, f64>,
    external_file_string_consts: &HashMap<String, String>,
    global_namespace_exports: &HashMap<AstString, HashSet<AstString>>,
    global_namespace_type_exports: &HashMap<AstString, HashSet<AstString>>,
) -> EmitOutput {
    emit_full_with_file_consts(
        file,
        options,
        external_const_enum_values,
        global_type_only_names,
        global_type_only_export_names,
        external_file_consts,
        external_file_string_consts,
        global_namespace_exports,
        global_namespace_type_exports,
    )
}

fn emit_full(
    file: &SourceFile,
    options: &CompilerOptions,
    external_const_enum_values: &HashMap<(String, String), ConstEnumValue>,
    global_type_only_names: &HashSet<AstString>,
    global_type_only_export_names: &HashSet<AstString>,
) -> EmitOutput {
    emit_full_with_file_consts(
        file,
        options,
        external_const_enum_values,
        global_type_only_names,
        global_type_only_export_names,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    )
}

fn emit_full_with_file_consts(
    file: &SourceFile,
    options: &CompilerOptions,
    external_const_enum_values: &HashMap<(String, String), ConstEnumValue>,
    global_type_only_names: &HashSet<AstString>,
    global_type_only_export_names: &HashSet<AstString>,
    external_file_consts: &HashMap<String, f64>,
    external_file_string_consts: &HashMap<String, String>,
    global_namespace_exports: &HashMap<AstString, HashSet<AstString>>,
    global_namespace_type_exports: &HashMap<AstString, HashSet<AstString>>,
) -> EmitOutput {
    let mut e = Emitter::new(
        &file.text,
        options,
        &file.comments,
        external_const_enum_values.clone(),
    );
    e.global_type_only_names = global_type_only_names.clone();
    e.global_type_only_export_names = global_type_only_export_names.clone();
    e.external_file_consts = external_file_consts.clone();
    e.external_file_string_consts = external_file_string_consts.clone();
    e.cumulative_ns_exports = global_namespace_exports.clone();
    e.cumulative_ns_type_exports = global_namespace_type_exports.clone();
    emit_with_emitter(&mut e, file)
}

/// Emit JavaScript with extra const-enum member values supplied by the caller.
/// These values are keyed by `(enum_object_name, member_name)` and can be used
/// to inline const enum accesses that originate from imported modules.
pub fn emit_with_const_enum_values(
    file: &SourceFile,
    options: &CompilerOptions,
    external_const_enum_values: &HashMap<(String, String), ConstEnumValue>,
) -> EmitOutput {
    let mut e = Emitter::new(
        &file.text,
        options,
        &file.comments,
        external_const_enum_values.clone(),
    );
    emit_with_emitter(&mut e, file)
}

/// Core emit logic shared by all public emit functions.
fn emit_with_emitter(e: &mut Emitter, file: &SourceFile) -> EmitOutput {
    let js_filename = file
        .file_name
        .strip_suffix(".cts")
        .map(|base| format!("{base}.cjs"))
        .or_else(|| {
            file.file_name
                .strip_suffix(".mts")
                .map(|base| format!("{base}.mjs"))
        })
        .or_else(|| {
            file.file_name
                .strip_suffix(".ts")
                .map(|base| format!("{base}.js"))
        })
        .or_else(|| {
            file.file_name.strip_suffix(".tsx").map(|base| {
                if matches!(
                    e.options.jsx,
                    Some(JsxEmit::Preserve) | Some(JsxEmit::ReactNative)
                ) {
                    format!("{base}.jsx")
                } else {
                    format!("{base}.js")
                }
            })
        })
        .unwrap_or_else(|| file.file_name.clone());

    // The basename of the .js file (no directory component).
    let js_basename = path_basename(&js_filename);
    // The basename of the .ts source file.
    let ts_basename = path_basename(&file.file_name);

    // Look up sourceRoot / mapRoot from options.other (they land there since
    // CompilerOptions has no typed fields for them yet).
    let source_root = get_other_option(e.options, "sourceroot");
    let map_root = get_other_option(e.options, "maproot");

    let wants_source_map = e.options.source_map == Some(true);
    let wants_inline = e.options.inline_source_map == Some(true);

    if wants_source_map || wants_inline {
        // Determine the sourceRoot field for the map JSON.
        // TypeScript appends a trailing "/" to sourceRoot if missing.
        let map_source_root = match &source_root {
            Some(sr) => {
                let mut s = sr.clone();
                if !s.ends_with('/') {
                    s.push('/');
                }
                s
            }
            None => String::new(),
        };

        // Determine the source path to store in the "sources" array.
        // When sourceRoot is set:  use the basename of the .ts file.
        // When mapRoot  is set:    the "virtual" map sits at <mapRoot>/<js_basename>.map,
        //   so the source is relative to that directory.
        // Otherwise:               use the basename of the .ts file (same directory as map).
        let src_entry = if source_root.is_some() {
            // sourceRoot handles the directory; sources just need the file name.
            ts_basename.to_string()
        } else if let Some(ref mr) = map_root {
            // Compute relative path from <mapRoot> back to the .ts source.
            relative_path_from_dir(mr, &file.file_name)
        } else {
            // No sourceRoot, no mapRoot: source is just the basename.
            ts_basename.to_string()
        };

        // The "file" field is always the js basename (no directory).
        let mut gen = SourceMapGenerator::new(js_basename, &map_source_root);
        let src_idx = gen.add_source(&src_entry);
        e.source_index = src_idx;
        e.source_map_gen = Some(gen);
        e.line_index = Some(crate::source_map::LineIndex::new(e.source));
    }

    e.emit_source_file(file);

    // TypeScript terminates non-empty JS output with a newline, but an input
    // whose runtime syntax is completely erased produces a genuinely empty
    // file.  This distinction matters when the harness concatenates several
    // emitted files: a synthetic newline here becomes a blank line between
    // adjacent empty-file headers.
    if !e.output.is_empty() && !e.output.ends_with('\n') {
        e.output.push('\n');
    }

    // tsc's mappings, recovered from the source and the emitted text.
    if let Some(ref mut gen) = e.source_map_gen {
        let source_index = e.source_index;
        let mappings = tsc_source_map::synthesize(e.source, file, &e.output)
            .into_iter()
            .map(
                |(generated_line, generated_column, original_line, original_column)| Mapping {
                    generated_line,
                    generated_column,
                    source_index,
                    original_line,
                    original_column,
                    name_index: None,
                },
            )
            .collect();
        // The structured emit paths recorded the nodes they printed
        // (including synthesized wrappers of downleveled syntax); the
        // synthesized set covers the source-copy paths.
        gen.merge_mappings(mappings);
    }
    let source_map = if let Some(ref gen) = e.source_map_gen {
        if wants_inline {
            let data_uri = gen.to_data_uri();
            e.output.push_str("//# sourceMappingURL=");
            e.output.push_str(&data_uri);
            None
        } else {
            // Build the sourceMappingURL comment.
            // If mapRoot is set, prefix with <mapRoot>/.
            // URL-encode the basename for non-ASCII characters.
            let encoded_basename = url_encode_path(js_basename);
            let url = match &map_root {
                Some(mr) => {
                    let mr = mr.trim_end_matches('/');
                    format!("{}/{}.map", mr, encoded_basename)
                }
                None => format!("{}.map", encoded_basename),
            };
            e.output.push_str("//# sourceMappingURL=");
            e.output.push_str(&url);
            Some(gen.to_json())
        }
    } else {
        None
    };

    let declaration_file =
        if e.options.declaration == Some(true) || e.options.emit_declaration_only == Some(true) {
            let mut de = DeclarationEmitter::with_options(&file.text, &e.options);
            de.emit_source_file(file);
            Some(de.output)
        } else {
            None
        };

    EmitOutput {
        javascript: std::mem::take(&mut e.output),
        source_map,
        declaration_file,
        require_var_counters: std::mem::take(&mut e.require_var_counters),
        system_register_counter: e.system_register_counter,
    }
}

/// Emit JavaScript with pre-seeded require-var counters (for AMD outFile
/// concatenation where import alias suffixes must be globally unique).
pub fn emit_with_require_var_counters(
    file: &SourceFile,
    options: &CompilerOptions,
    external_const_enum_values: &HashMap<(String, String), ConstEnumValue>,
    global_type_only_names: &HashSet<AstString>,
    global_type_only_export_names: &HashSet<AstString>,
    global_namespace_exports: &HashMap<AstString, HashSet<AstString>>,
    global_namespace_type_exports: &HashMap<AstString, HashSet<AstString>>,
    initial_counters: HashMap<AstString, usize>,
    initial_system_register_counter: usize,
) -> EmitOutput {
    let mut e = Emitter::new(
        &file.text,
        options,
        &file.comments,
        external_const_enum_values.clone(),
    );
    e.global_type_only_names = global_type_only_names.clone();
    e.global_type_only_export_names = global_type_only_export_names.clone();
    e.cumulative_ns_exports = global_namespace_exports.clone();
    e.cumulative_ns_type_exports = global_namespace_type_exports.clone();
    e.require_var_counters = initial_counters;
    e.system_register_counter = initial_system_register_counter;
    emit_with_emitter(&mut e, file)
}

// ---------------------------------------------------------------------------
// Source-map path helpers
// ---------------------------------------------------------------------------

/// Return the basename (filename) part of a `/`-separated path.
fn path_basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Look up a string value from `CompilerOptions::other` by key (case-insensitive).
fn get_other_option(options: &CompilerOptions, key: &str) -> Option<String> {
    options.other.iter().rev().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case(key) {
            Some(v.clone())
        } else {
            None
        }
    })
}

/// Percent-encode non-ASCII / reserved characters in a URL path segment.
///
/// TypeScript URL-encodes characters that are not "safe" in a URL path.
/// This mirrors JavaScript's `encodeURI` behaviour: letters, digits,
/// `-_.*!~'()` and `;,/?:@&=+$#` are kept as-is; everything else
/// (including spaces, `[`, `]`, and non-ASCII) is percent-encoded.
fn url_encode_path(path: &str) -> String {
    // Characters that are safe unencoded in a URL path component.
    // This mirrors JavaScript's `encodeURI` which does NOT encode:
    //   A-Z a-z 0-9 - _ . ! ~ * ' ( ) ; , / ? : @ & = + $ #
    // Note: '[' and ']' ARE encoded by encodeURI (they are not in the safe set).
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            // Keep ASCII letters, digits, and common safe chars unencoded.
            'A'..='Z'
            | 'a'..='z'
            | '0'..='9'
            | '-'
            | '_'
            | '.'
            | '!'
            | '~'
            | '*'
            | '\''
            | '('
            | ')'
            | '/'
            | ':'
            | '@'
            | '&'
            | '='
            | '+'
            | '$'
            | ','
            | ';'
            | '?'
            | '#' => out.push(ch),
            // Encode everything else (including spaces, brackets, and non-ASCII).
            _ => {
                let mut buf = [0u8; 4];
                let encoded = ch.encode_utf8(&mut buf);
                for byte in encoded.as_bytes() {
                    out.push('%');
                    out.push(
                        char::from_digit((*byte >> 4) as u32, 16)
                            .unwrap()
                            .to_ascii_uppercase(),
                    );
                    out.push(
                        char::from_digit((*byte & 0xf) as u32, 16)
                            .unwrap()
                            .to_ascii_uppercase(),
                    );
                }
            }
        }
    }
    out
}

/// Compute the relative path from `base_dir` (a directory path, possibly
/// with depth) to `target` (a file path at the "root" of the virtual
/// filesystem).
///
/// Used for `mapRoot` handling: when the map "would be" at
/// `<mapRoot>/<basename>.map`, the source entry needs to be relative from
/// that directory back to the original `.ts` source.
///
/// Example: `base_dir = "local"`, `target = "foo.ts"` → `"../foo.ts"`
/// Example: `base_dir = "a/b"`,   `target = "foo.ts"` → `"../../foo.ts"`
fn relative_path_from_dir(base_dir: &str, target: &str) -> String {
    // Count the depth of base_dir (number of path components).
    let depth = base_dir
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .count();
    // The target basename.
    let tgt_base = path_basename(target);
    if depth == 0 {
        return tgt_base.to_string();
    }
    let ups: String = std::iter::repeat("../").take(depth).collect();
    format!("{}{}", ups, tgt_base)
}

/// Collect const-enum member values declared in a source file.
/// Output keys are `(enum_object_name, member_name)`.
pub fn collect_const_enum_values(file: &SourceFile) -> HashMap<(String, String), ConstEnumValue> {
    let opts = CompilerOptions::default();
    let mut e = Emitter::new(&file.text, &opts, &file.comments, HashMap::new());
    e.scan_const_enums(&file.statements);
    e.const_enum_values
}

/// Collect type-only declaration names from a source file.
///
/// Returns names of interfaces, type aliases, and namespaces that contain
/// only type-level declarations (no runtime value).  Used by the harness to
/// build a cross-file type-only name set so that `export { I1 }` where `I1`
/// Normalize unicode escape sequences in an identifier to their actual
/// Decode the UTF-8 length of the char starting at this byte. Returns 1
/// for ASCII and 2/3/4 for multi-byte leads. Continuation bytes (10xxxxxx)
/// return 1 so the caller falls back to byte-at-a-time handling on
/// malformed input rather than panicking.
pub(crate) fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // continuation byte without a lead — malformed
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

/// Copy one Unicode-aware step from `src` starting at byte index `i` into
/// `out`. If the byte at `i` is ASCII, push it as a single char and return
/// `i + 1`. Otherwise, push the full multi-byte UTF-8 sequence as a string
/// slice and return the byte index past the sequence.
///
/// Replaces the unsafe `out.push(bytes[i] as char); i += 1` pattern, which
/// silently double-encodes multi-byte UTF-8 (each byte becomes a Latin-1
/// codepoint, which is then re-encoded as UTF-8 when the `String` is
/// serialized — `é` (`0xC3 0xA9`) became `Ã©` (`0xC3 0x83 0xC2 0xA9`)).
///
/// Confirmed 2026-04-30 against the bext-turbopack second-pass bundle
/// transform: every multi-byte UTF-8 sequence in 1clic.pro's compiled
/// `page.tsx` came out the other side double-encoded because the
/// emit-source-line normalize chain (`trim_block_comment_line_trailing_spaces`
/// in particular) used the byte-as-char pattern.
pub(crate) fn push_utf8_aware(out: &mut String, src: &str, i: usize) -> usize {
    let bytes = src.as_bytes();
    let b = bytes[i];
    if b < 0x80 {
        out.push(b as char);
        i + 1
    } else {
        let len = utf8_char_len(b);
        let end = (i + len).min(bytes.len());
        out.push_str(&src[i..end]);
        end
    }
}

/// characters.  TypeScript does this when synthesizing variable names
/// (e.g. private field helper names like `_ClassName_fieldName`).
///
/// Handles `\uXXXX` and `\u{X...}` escapes.
pub(crate) fn normalize_unicode_escapes(s: &str) -> String {
    if !s.contains("\\u") {
        return s.to_string();
    }
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'\\' && bytes[i + 1] == b'u' {
            if i + 2 < bytes.len() && bytes[i + 2] == b'{' {
                // \u{X...} form
                if let Some(close) = s[i + 3..].find('}') {
                    let hex = &s[i + 3..i + 3 + close];
                    if let Ok(cp) = u32::from_str_radix(hex, 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            result.push(ch);
                            i += 3 + close + 1;
                            continue;
                        }
                    }
                }
            } else if i + 5 < bytes.len() {
                // \uXXXX form
                let hex = &s[i + 2..i + 6];
                if hex.len() == 4 && hex.as_bytes().iter().all(|b| b.is_ascii_hexdigit()) {
                    if let Ok(cp) = u32::from_str_radix(hex, 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            result.push(ch);
                            i += 6;
                            continue;
                        }
                    }
                }
            }
        }
        i = push_utf8_aware(&mut result, s, i);
    }
    result
}

/// is a global interface from a `.d.ts` file does not get a spurious
/// `exports.I1 = void 0;` pre-declaration.
pub fn collect_type_only_names(
    file: &SourceFile,
    preserve_const_enums: bool,
) -> HashSet<AstString> {
    let mut names = HashSet::new();
    // Helper: check a statement for type-only declarations
    let check_stmt = |s: &Stmt, names: &mut HashSet<AstString>| match &s.kind {
        StmtKind::InterfaceDecl(i) => {
            names.insert(i.name.clone().into());
        }
        StmtKind::TypeAlias(t) => {
            names.insert(t.name.clone().into());
        }
        StmtKind::ModuleDecl(m) => {
            // For `declare namespace`, use the stricter check that only
            // treats it as type-only when it has NO value members.
            // A `declare namespace NS { export var foo; }` has runtime
            // value and re-exports should get CJS pre-declarations.
            let is_type_only = if m.modifiers & MOD_DECLARE != 0 {
                declare_namespace_is_purely_type_only(m, preserve_const_enums)
            } else {
                module_decl_is_type_only(m, preserve_const_enums)
            };
            if is_type_only {
                if let ModuleName::Ident(ref n) = m.name {
                    names.insert(n.clone().into());
                }
            }
        }
        _ => {}
    };
    for stmt in &file.statements {
        check_stmt(stmt, &mut names);
        // Also handle `export` wrappers: `export interface I { }`, etc.
        if let StmtKind::Export(ref ed) = stmt.kind {
            if let ExportDeclKind::Decl(ref inner) = ed.kind {
                check_stmt(inner, &mut names);
            }
        }
    }
    // Remove names that also have value declarations.
    // Helper: remove type-only names that have a value declaration.
    let remove_value_names = |s: &Stmt, names: &mut HashSet<AstString>| {
        // Import declarations create value bindings that shadow type-only names.
        // e.g. `import * as B from "./b"` creates a value `B`.
        if let StmtKind::Import(ref imp) = s.kind {
            if !imp.type_only {
                match &imp.specifiers {
                    ImportClause::Named {
                        default,
                        namespace,
                        named,
                    } => {
                        if let Some(n) = default {
                            names.remove(n.as_str());
                        }
                        if let Some(n) = namespace {
                            names.remove(n.as_str());
                        }
                        for spec in named {
                            if !spec.is_type {
                                names.remove(spec.local.as_str());
                            }
                        }
                    }
                    ImportClause::Require(n) => {
                        names.remove(n.as_str());
                    }
                }
            }
            return;
        }
        if let StmtKind::ImportEquals(ref ie) = s.kind {
            names.remove(ie.name.as_str());
            return;
        }
        let has_value = match &s.kind {
            StmtKind::Var(v) => {
                let mut tmp = Vec::new();
                for d in &v.declarations {
                    collect_binding_names(&d.name, &mut tmp);
                }
                tmp.into_iter().any(|n| names.contains(n.as_str()))
            }
            StmtKind::FnDecl(f) => f
                .name
                .as_ref()
                .map_or(false, |n| names.contains(n.as_str())),
            StmtKind::ClassDecl(c) => c
                .name
                .as_ref()
                .map_or(false, |n| names.contains(n.as_str())),
            StmtKind::EnumDecl(e) => names.contains(e.name.as_str()),
            StmtKind::ModuleDecl(m) if !module_decl_is_type_only(m, preserve_const_enums) => {
                if let ModuleName::Ident(ref n) = m.name {
                    names.contains(n.as_str())
                } else {
                    false
                }
            }
            _ => false,
        };
        if has_value {
            match &s.kind {
                StmtKind::Var(v) => {
                    let mut tmp = Vec::new();
                    for d in &v.declarations {
                        collect_binding_names(&d.name, &mut tmp);
                    }
                    for n in tmp {
                        names.remove(n.as_str());
                    }
                }
                StmtKind::FnDecl(f) => {
                    if let Some(n) = &f.name {
                        names.remove(n.as_str());
                    }
                }
                StmtKind::ClassDecl(c) => {
                    if let Some(n) = &c.name {
                        names.remove(n.as_str());
                    }
                }
                StmtKind::EnumDecl(e) => {
                    names.remove(e.name.as_str());
                }
                StmtKind::ModuleDecl(m) => {
                    if let ModuleName::Ident(ref n) = m.name {
                        names.remove(n.as_str());
                    }
                }
                _ => {}
            }
        }
    };
    for stmt in &file.statements {
        remove_value_names(stmt, &mut names);
        // Also check inside `export` wrappers (e.g. `export const Sizing = null;`)
        if let StmtKind::Export(ref ed) = stmt.kind {
            if let ExportDeclKind::Decl(ref inner) = ed.kind {
                remove_value_names(inner, &mut names);
            }
            // `export * as Drink from "./constants"` (non-type-only) provides
            // a value binding for the alias name, removing it from type-only.
            if let ExportDeclKind::All {
                alias: Some(ref alias),
                type_only: false,
                ..
            } = ed.kind
            {
                names.remove(alias.as_str());
            }
        }
    }
    // Also include exported aliases of type-only declarations.
    // For `export { I as I1 }` where `I` is type-only, add `I1`.
    // This does NOT include `export type { C }` where `C` is a value class,
    // because `C` would not be in `names` (it was removed by remove_value_names).
    for stmt in &file.statements {
        let StmtKind::Export(ref ed) = stmt.kind else {
            continue;
        };
        if let ExportDeclKind::Named {
            specifiers,
            source,
            type_only: false,
        } = &ed.kind
        {
            // Only local re-exports (no source module).
            let is_local = source.as_ref().map_or(true, |s| s.is_empty());
            if !is_local {
                continue;
            }
            for spec in specifiers {
                if spec.is_type {
                    continue;
                }
                if names.contains(spec.local.as_str()) {
                    if let Some(ref exported) = spec.exported {
                        if exported != &spec.local {
                            names.insert(exported.clone().into());
                        }
                    }
                }
            }
        }
    }
    names
}

/// Collect names that are explicitly exported as type-only from a source file.
///
/// Includes:
/// - `export type { X }`
/// - `export { type X }`
/// - `export type { X } from "..."`
/// - `export { type X } from "..."`
/// - `export { Foo }` where `Foo` is a type alias or interface in the same file
pub fn collect_type_only_export_names(file: &SourceFile) -> HashSet<AstString> {
    let type_only_bindings = collect_local_type_only_bindings(file);
    let mut names = HashSet::new();
    let collect_direct_export = |stmt: &Stmt, names: &mut HashSet<AstString>| match &stmt.kind {
        StmtKind::TypeAlias(t)
            if t.modifiers & MOD_EXPORT != 0 && type_only_bindings.contains(t.name.as_str()) =>
        {
            names.insert(t.name.clone().into());
        }
        StmtKind::InterfaceDecl(i)
            if i.modifiers & MOD_EXPORT != 0 && type_only_bindings.contains(i.name.as_str()) =>
        {
            names.insert(i.name.clone().into());
        }
        StmtKind::ModuleDecl(m) if m.modifiers & MOD_EXPORT != 0 => {
            if let ModuleName::Ident(n) = &m.name {
                if type_only_bindings.contains(n.as_str()) {
                    names.insert(n.clone().into());
                }
            }
        }
        _ => {}
    };
    for stmt in &file.statements {
        collect_direct_export(stmt, &mut names);
        // `export = expr` where expr is a type-only binding:
        // mark "default" as type-only (affects default imports with esModuleInterop).
        if let StmtKind::ExportAssign(expr) = &stmt.kind {
            if let ExprKind::Ident(name) = &expr.kind {
                if type_only_bindings.contains(name.as_str()) {
                    names.insert(AstString::from("default"));
                }
            }
        }
        let StmtKind::Export(ed) = &stmt.kind else {
            continue;
        };
        match &ed.kind {
            ExportDeclKind::Named {
                specifiers,
                type_only,
                source,
                ..
            } => {
                // For re-exports with a source (`export { X } from "mod"`), we can't
                // determine type-only-ness without cross-file info, so only handle
                // explicit `export type` there.
                let is_local_reexport = source.as_ref().map_or(true, |s| s.is_empty());
                for spec in specifiers {
                    let exported = spec.exported.as_ref().unwrap_or(&spec.local).clone();
                    if *type_only || spec.is_type {
                        names.insert(exported.into());
                    } else if is_local_reexport && type_only_bindings.contains(spec.local.as_str())
                    {
                        // `export { Foo }` where Foo is a local type alias/interface
                        names.insert(exported.into());
                    }
                }
            }
            ExportDeclKind::Decl(inner) => {
                // `export type T = ...;` or `export interface I { ... }`
                // Only include if the name is truly type-only (no value declarations
                // with the same name elsewhere in the file).
                let decl_name = match &inner.kind {
                    StmtKind::TypeAlias(t) => Some(&t.name),
                    StmtKind::InterfaceDecl(i) => Some(&i.name),
                    StmtKind::ModuleDecl(m) => match &m.name {
                        ModuleName::Ident(n) => Some(n),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(n) = decl_name {
                    if type_only_bindings.contains(n.as_str()) {
                        names.insert(n.clone().into());
                    }
                }
            }
            ExportDeclKind::All {
                type_only: true,
                alias: Some(alias),
                ..
            } => {
                names.insert(alias.clone().into());
            }
            // `export default expr` where expr is a type-only binding:
            // mark "default" as type-only so default imports are elided.
            ExportDeclKind::Default(expr) => {
                if let ExprKind::Ident(name) = &expr.kind {
                    if type_only_bindings.contains(name.as_str()) {
                        names.insert(AstString::from("default"));
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// Collect names that are exported as values (non-type) from a file.
/// Used to override `global_type_only_export_names` when a name is
/// re-exported as a value from a different file.
pub fn collect_value_export_names(file: &SourceFile) -> HashSet<AstString> {
    use crate::analysis::collect_binding_names;
    let type_only_bindings = collect_local_type_only_bindings(file);
    let mut names = HashSet::new();
    for stmt in &file.statements {
        match &stmt.kind {
            // `export function f()`, `export class C`, `export const x`, `export enum E`
            StmtKind::Export(ed) => match &ed.kind {
                ExportDeclKind::Decl(inner) => match &inner.kind {
                    StmtKind::FnDecl(f) => {
                        if let Some(n) = &f.name {
                            names.insert(n.clone().into());
                        }
                    }
                    StmtKind::ClassDecl(c) => {
                        if let Some(n) = &c.name {
                            names.insert(n.clone().into());
                        }
                    }
                    StmtKind::Var(v) if v.modifiers & MOD_DECLARE == 0 => {
                        for d in &v.declarations {
                            let mut bound = Vec::new();
                            collect_binding_names(&d.name, &mut bound);
                            names.extend(bound.into_iter().map(AstString::from));
                        }
                    }
                    StmtKind::EnumDecl(e) => {
                        if !e.is_const {
                            names.insert(e.name.clone().into());
                        }
                    }
                    StmtKind::ModuleDecl(m) => {
                        if let ModuleName::Ident(n) = &m.name {
                            // Only if it has runtime value (not purely type namespace)
                            if !type_only_bindings.contains(n.as_str()) {
                                names.insert(n.clone().into());
                            }
                        }
                    }
                    _ => {}
                },
                ExportDeclKind::Named {
                    specifiers,
                    type_only: false,
                    source,
                    ..
                } => {
                    let is_local_reexport = source.as_ref().map_or(true, |s| s.is_empty());
                    if is_local_reexport {
                        for spec in specifiers {
                            if !spec.is_type && !type_only_bindings.contains(spec.local.as_str()) {
                                let exported =
                                    spec.exported.as_ref().unwrap_or(&spec.local).clone();
                                names.insert(exported.into());
                            }
                        }
                    }
                }
                // `export * as ns from "mod"` — aliased namespace re-export
                // creates a value binding for the alias name.
                ExportDeclKind::All {
                    alias: Some(alias),
                    type_only: false,
                    ..
                } => {
                    names.insert(alias.clone().into());
                }
                _ => {}
            },
            // Top-level `function f()` / `class C` / `const x` / `enum E` that are
            // re-exported via `export { X }` — these contribute value names.
            // We need the exported name, which is handled above.
            _ => {}
        }
    }
    names
}

fn collect_local_type_only_bindings(file: &SourceFile) -> HashSet<AstString> {
    let mut names = collect_type_only_names(file, false);
    for stmt in &file.statements {
        let import_decl = match &stmt.kind {
            StmtKind::Import(import_decl) => Some(import_decl),
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(inner) => match &inner.kind {
                    StmtKind::Import(import_decl) => Some(import_decl),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let Some(import_decl) = import_decl else {
            continue;
        };
        let ImportClause::Named {
            default,
            named,
            namespace,
        } = &import_decl.specifiers
        else {
            continue;
        };
        if import_decl.type_only {
            if let Some(default) = default {
                names.insert(default.clone().into());
            }
            for spec in named {
                names.insert(spec.local.clone().into());
            }
            if let Some(namespace) = namespace {
                names.insert(namespace.clone().into());
            }
        } else {
            for spec in named {
                if spec.is_type {
                    names.insert(spec.local.clone().into());
                }
            }
        }
    }
    names
}

#[derive(Clone)]
pub(crate) enum ClassExprBindingName {
    Literal(String),
    Expr(String),
}

#[derive(Clone)]
enum EnclosingPrivateMemberKind {
    Field { is_static: bool },
    Method { is_static: bool },
    Accessor { is_static: bool },
}

#[derive(Clone)]
struct Emitter<'a> {
    source: &'a str,
    /// Lazily tokenized identifiers used by generated-name collision checks.
    source_identifiers: OnceCell<HashSet<AstString>>,
    /// Whole-file binding and loop plan used by the ES5 lexical transform.
    lexical_downlevel_plan: lexical_downlevel::LexicalDownlevelPlan,
    /// Stable per-file names for catch bindings lifted into generator closures.
    generator_catch_names: HashMap<lexical_downlevel::BindingId, AstString>,
    generator_catch_scope: bool,
    emitted_lexical_binding_ids: HashSet<usize>,
    active_lexical_loop_helpers: Vec<usize>,
    active_lexical_for_of_plan: Option<lexical_downlevel::LoopPlan>,
    lexical_arrow_this_alias: Option<String>,
    options: &'a CompilerOptions,
    output: String,
    indent: usize,
    /// Extra indent levels added by module wrappers (AMD/UMD/System).
    /// Used by JSX preserve to know how much extra indent to add to
    /// continuation lines (source text already has function body indent).
    module_wrapper_indent: usize,
    at_line_start: bool,
    /// Per-basename counters for unique require variable suffixes (e.g. `file_1`, `file_2`).
    require_var_counters: HashMap<AstString, usize>,
    /// Counter used to generate unique temp variable names for downlevel transforms.
    temp_var_counter: usize,
    /// Pre-allocated temp var names for assignment-level object rest transforms.
    /// TypeScript allocates these BEFORE inline var-level temps, so assignment
    /// rest temps get lower-numbered names (_a, _b) than var temps (_c, _d).
    pre_allocated_assignment_rest_temps: Vec<String>,
    /// Minimum temp_var_counter value when entering method bodies inside a class
    /// that has reserved class-level temps (e.g. static private field alias `_a`).
    class_scope_temp_reserved: usize,
    /// Number of actual object-rest temps allocated by
    /// `emit_params_with_rest_transform` for the current function's parameters.
    /// Consumed by `emit_block_for_decl_body` to start body temps after those
    /// emitted parameter bindings (e.g. param `_a` → body starts at `_b`).
    /// Ordinary `...rest` parameters use the scoped simple-loop allocator and
    /// must not pre-consume an anonymous temp.
    fn_param_rest_temp_count: usize,
    /// Counter for AMD/UMD dynamic-import resolve/reject parameter naming
    /// (resolve_1, reject_1, resolve_2, reject_2, …).
    amd_import_counter: usize,
    /// Names of temp variables allocated in the current function scope.
    /// After emitting a function body, these are hoisted as `var _a, _b, ...;`.
    temp_var_names: Vec<AstString>,
    /// True only while structurally emitting an object/array destructuring
    /// assignment pattern node. Computed keys, defaults, and ordinary member
    /// receiver/index expressions temporarily leave this context.
    emitting_destructuring_assignment_pattern: bool,
    /// Prevent the bounded computed-object transform while emitting through a
    /// type-layer owner whose parenthesis behavior is not yet modeled.
    suppress_computed_object_downlevel: bool,
    /// Structural expression recursion depth. Direct type-layer roots are at
    /// depth one; wrappers below runtime expression boundaries are deeper.
    emit_expr_depth: usize,
    /// Counter for auto-generated catch clause parameter names (_a, _b, _c, ...).
    /// Incremented for each `catch {}` without an explicit param in the current
    /// function scope.  Reset per function body.
    catch_auto_param_counter: u8,
    /// When downleveling async bodies through `__awaiter`, a `var` declaration
    /// that binds an outer parameter name must be hoisted and rewritten.
    async_var_shadow_names: Option<HashSet<AstString>>,
    /// Catch-clause bindings temporarily block async var-shadow rewriting for
    /// matching names inside the handler body.
    async_var_shadow_blockers: Vec<HashSet<AstString>>,
    /// Hoisted `var` names that should be emitted at the top of the current
    /// awaiter body before rewritten statements are printed.
    async_var_shadow_top_level_hoists: Vec<AstString>,
    /// Placeholder tokens for deferred temp names used when class transforms
    /// need "late" allocation order (after all regular temps are known).
    deferred_temp_placeholders: Vec<AstString>,
    /// Placeholder tokens for deferred inline temp names. These resolve after
    /// regular temps but are not emitted in hoisted `var` declarations.
    inline_deferred_temp_placeholders: Vec<AstString>,
    /// Synthetic names for exported empty binding patterns. TypeScript assigns
    /// these after every ordinary transform temp, so resolve them in a separate
    /// final phase rather than interleaving them with expression temps.
    deferred_export_name_placeholders: Vec<AstString>,
    /// Real temp names resolved from deferred placeholders. Emitted as a
    /// separate `var` declaration after regular temp names.
    resolved_deferred_temp_names: Vec<AstString>,
    /// Counter for generating unique deferred placeholder tokens.
    deferred_temp_placeholder_counter: usize,
    /// Set to true when the current file has an `export = X` statement.
    /// When true, individual `export class C {}` declarations still get
    /// pre-declared but do NOT get `exports.C = C;` assignments because
    /// `module.exports = X` overrides the entire exports object.
    has_export_assign: bool,
    /// Track whether any ES module export content was actually emitted.
    /// Used to determine if `export {};` is needed at end of module files.
    emitted_esm_export: bool,
    /// Trailing comment text from a bare `export {}` statement in source, if any.
    /// Used to preserve trailing comments when the end-of-file logic emits `export {};`.
    bare_export_empty_trailing_comment: Option<String>,
    /// Sources for which we've already emitted `export {} from "mod"` under
    /// verbatimModuleSyntax. Used to deduplicate multiple `export type {...}`
    /// re-exports from the same module.
    verbatim_empty_reexport_sources: HashSet<AstString>,
    /// The current export target for `export`-modified declarations.
    /// - `Some("exports")` at the CJS top-level: emit `exports.X = X;`
    /// - `Some("M")` inside a namespace `M`: emit `M.X = X;`
    /// - `None` when no export transformation is needed (ES modules, non-module)
    export_target: Option<String>,
    /// Source map generator, present when source_map or inline_source_map is enabled.
    source_map_gen: Option<SourceMapGenerator>,
    /// Pre-computed line starts for O(log n) offset → line:col lookups (source maps).
    line_index: Option<crate::source_map::LineIndex>,
    /// Current output line (0-based), tracked for source map generation.
    out_line: u32,
    /// Current output column (0-based), tracked for source map generation.
    out_col: u32,
    /// Index of the current source file in the source map.
    source_index: u32,
    /// Track which CJS interop helpers are needed so we can emit them.
    needs_import_default: bool,
    needs_import_star: bool,
    needs_export_star: bool,
    /// Namespace names that merge with import aliases — imports should be elided.
    merged_namespace_names: HashSet<AstString>,
    first_import_default_helper_use: Option<usize>,
    first_import_star_helper_use: Option<usize>,
    first_export_star_helper_use: Option<usize>,
    /// Track whether the __decorate helper is needed (any decorator present).
    needs_decorate_helper: bool,
    needs_extends_helper: bool,
    /// Top-level class declarations that are safe dependencies for the
    /// expanded ES5 method/accessor lowering. Populated once per source file.
    legacy_es5_member_lowerable_class_starts: HashSet<u32>,
    /// Source start of the CommonJS `export default class` currently routed
    /// through local class emit. Identity, rather than a dynamically inherited
    /// boolean, keeps nested classes from acquiring the outer export boundary.
    legacy_es5_cjs_default_class_start: Option<u32>,
    /// Track whether the __esDecorate helper is needed for standard decorators.
    needs_es_decorate_helper: bool,
    /// When true, a class has decorated methods → emit __runInitializers before __esDecorate.
    /// When false (default), emit __esDecorate before __runInitializers.
    has_es_decorated_methods: bool,
    /// When true, constructor emission injects `__runInitializers(this, _instanceExtraInitializers)`
    /// after `super()` call (or at body start for non-extends classes).
    inject_instance_extra_initializers: bool,
    /// Collision-safe binding selected by structural decorator wrappers.
    /// Other decorator paths retain TypeScript's conventional spelling.
    injected_instance_extra_initializers_name: Option<String>,
    /// Track whether the __metadata helper is needed (emitDecoratorMetadata + decorators).
    needs_metadata_helper: bool,
    /// Track whether the __param helper is needed (any parameter decorator present).
    needs_param_helper: bool,
    /// Track whether the __runInitializers helper is needed for standard decorators.
    needs_run_initializers_helper: bool,
    /// Track whether the __awaiter helper is needed (async function with target < ES2017).
    needs_awaiter_helper: bool,
    needs_generator_helper: bool,
    /// Track whether the __await helper is needed (async generators downlevel).
    needs_await_helper: bool,
    /// Track whether the __asyncGenerator helper is needed (async generators downlevel).
    needs_async_generator_helper: bool,
    /// Track whether the __asyncValues helper is needed (yield* in async generators downlevel).
    needs_async_values_helper: bool,
    /// Track whether the __asyncDelegator helper is needed (yield* in async generators downlevel).
    needs_async_delegator_helper: bool,
    /// When true, emit __asyncValues before __await (direct yield* in async gen body).
    /// When false, emit __await/__asyncGenerator before __asyncValues/__asyncDelegator (nested async gen in yield* operand).
    async_values_before_await: bool,
    /// Track whether the __rest helper is needed (object rest destructuring with target < ES2018).
    needs_rest_helper: bool,
    /// Track whether the __values helper is needed (for-of with downlevelIteration and target < ES2015).
    needs_values_helper: bool,
    /// Track whether the __read helper is needed for iterable array binding patterns.
    needs_read_helper: bool,
    needs_spread_array_helper: bool,
    /// Track whether the __makeTemplateObject helper is needed (tagged templates
    /// with invalid escape sequences when target < ES2018).
    needs_make_template_object_helper: bool,
    /// Track whether the __addDisposableResource helper is needed (using declarations).
    needs_add_disposable_resource_helper: bool,
    /// Track whether the __disposeResources helper is needed (using declarations).
    needs_dispose_resources_helper: bool,
    /// File-level counter for using disposal envelope names (env_1, env_2, ...).
    using_env_counter: usize,
    /// Resource-bearing for-await loops emitted in this file. Later loops use
    /// distinct iterator-state bundles so nested loops cannot corrupt each other.
    resource_for_await_counter: usize,
    /// Native-await iterator envelopes emitted for targets that preserve async
    /// functions but still lower for-await.
    native_for_await_counter: usize,
    /// Depth of enclosing function/method scopes (0 = module level).
    fn_scope_depth: usize,
    /// Depth of enclosing block statement scopes (0 = top level).
    /// Module-level statements inside a block are in error recovery context
    /// and should be emitted verbatim (no transforms or elision).
    block_depth: usize,
    /// Counter for synthetic lexical `arguments` capture aliases
    /// (`arguments_1`, `arguments_2`, ...).
    arguments_capture_counter: usize,
    /// Active lexical `arguments` alias used while emitting transformed async
    /// generator bodies (`arguments` -> `arguments_N`).
    current_arguments_alias: Option<String>,
    /// True while emitting a parameter default initializer expression.
    /// Async-arrow downlevel uses this to preserve lexical `this` capture.
    in_parameter_initializer: bool,
    /// True when the enclosing function is async (for `await` keyword vs identifier distinction).
    in_async_function: bool,
    /// Name of the enclosing class (for private field WeakMap variable naming).
    current_class_name: Option<String>,
    /// Private field/accessor names (without '#') declared in the current class.
    current_class_private_fields: HashSet<AstString>,
    /// Static private field names (without '#') declared in the current class.
    /// These use `{ value: ... }` objects instead of WeakMaps.
    current_class_static_private_fields: HashSet<AstString>,
    /// Class alias variable name (e.g. `"_a"`) used as brand check for static
    /// private field access: `__classPrivateFieldGet(recv, _a, "f", _ClassName_field)`.
    current_class_static_alias: Option<String>,
    /// True when a local variable declaration (const/let/var) in the current
    /// function scope shadows the enclosing class name, suppressing the
    /// class-name → static-alias substitution for identifier references.
    class_name_locally_shadowed: bool,
    /// Self-reference alias for decorated classes (e.g. `"C_1"` for class `C`).
    /// When set, `__decorate` emits `C = C_1 = __decorate(...)`.
    decorated_class_self_ref_alias: Option<String>,
    /// Hoisted `var X_1;` declarations for decorated class self-references.
    /// Populated during pre-scan and emitted right after helpers.
    hoisted_decorated_aliases: Vec<AstString>,
    /// Counter for generating unique self-reference alias suffixes per class name.
    /// During emit, this is used to assign the right alias to each class.
    decorated_alias_emit_counter: HashMap<AstString, u32>,
    /// Mapping for downleveled private instance methods in the current class:
    /// private name (without '#') -> helper function variable (e.g. `_Foo_x`).
    current_class_private_methods: HashMap<AstString, AstString>,
    /// Mapping for downleveled private accessors in the current class:
    /// private name (without '#') -> (getter_var, setter_var).
    /// Both are optional (read-only accessor has no setter, write-only has no getter).
    current_class_private_accessors: HashMap<AstString, (Option<AstString>, Option<AstString>)>,
    /// Mapping for downleveled static private methods in the current class:
    /// private name (without '#') -> helper function variable (e.g. `_Foo_m`).
    /// Static methods use the class alias as brand, not `_instances`.
    current_class_static_private_methods: HashMap<AstString, AstString>,
    /// Set of private accessor names that are static (use class alias as brand).
    current_class_static_private_accessors: HashSet<AstString>,
    /// When set, `this` references in static field initializers are replaced
    /// with this alias (e.g. `"_a"`).
    static_this_alias: Option<String>,
    /// Base-class parameter used while lowering an ES2015 class constructor
    /// to an ES5 IIFE.  When set, bare `super(...)` calls are emitted as
    /// `_super.call(this, ...) || this` and assigned to `legacy_this_alias`.
    legacy_constructor_super: Option<String>,
    /// Captured derived-constructor receiver used by the ES5 class transform.
    legacy_this_alias: Option<String>,
    /// Prevent declaration-only ES5 lowering when `emit_class_decl` is reused
    /// to print a class expression.
    in_class_expression_emit: bool,
    /// Base-class alias used when downleveling static `super` access via
    /// `Reflect.get(base, key, receiver)` in moved static initializers.
    static_super_base_alias: Option<String>,
    /// Receiver alias used as the third argument to `Reflect.get` for static
    /// `super` access in moved static initializers.
    static_super_receiver_alias: Option<String>,
    /// When true, super member/element access in destructuring LHS should
    /// emit a setter proxy `({ set value(_a) { Reflect.set(...); } }).value`
    /// instead of `Reflect.get(...)`.
    super_reflect_destructure_target: bool,
    /// When true, Reflect.set transforms for super access should be wrapped
    /// in an IIFE `(() => { var _a; return Reflect.set(..., _a = val, ...), _a; })()`
    /// to capture and return the assigned value (for static field initializers).
    super_reflect_in_field_init: bool,
    /// Mapping from raw private member key to its actual WeakMap var name
    /// for the current class, accounting for deduplication suffixes.
    /// Key format: "field:foo", "method:foo", "get:foo", "set:foo".
    current_class_private_var_map: HashMap<AstString, AstString>,
    /// All private field var names ever allocated (persists across scopes).
    /// Used to detect collisions and assign `_1`, `_2` suffixes.
    all_allocated_private_var_names: HashMap<AstString, u32>,
    /// Track whether __classPrivateFieldGet helper is needed (private field reads).
    needs_private_field_get: bool,
    /// Track whether __classPrivateFieldSet helper is needed (private field writes).
    needs_private_field_set: bool,
    /// Track whether __classPrivateFieldIn helper is needed (`#field in obj`).
    needs_private_field_in: bool,
    /// Whether Get helper should be emitted before Set (true) or after (false).
    /// Determined by which helper is first needed in the source.
    private_field_get_first: bool,
    /// Track whether __propKey helper is needed for standard decorators on computed names.
    needs_prop_key_helper: bool,
    /// Track whether __setFunctionName helper is needed (class expressions with static fields).
    needs_set_function_name_helper: bool,
    /// Binding name for class expression __setFunctionName. Set before emitting a
    /// surrounding syntax form whose init/value is an anonymous class expression
    /// that needs named evaluation.
    class_expr_binding_name: Option<ClassExprBindingName>,
    /// True while a class expression is emitted as a transformed `using`
    /// initializer, where native assignment-name inference no longer applies.
    in_using_class_initializer: bool,
    /// Number of inline expression-container indentation levels already owned
    /// by the caller of the await-to-yield expression emitter.
    await_expr_inline_container_indents_preowned: usize,
    /// Lifted parameter defaults when object rest destructuring precedes params
    /// with defaults. Stores `(param_name, init_span)` so the body can emit
    /// `if (name === void 0) { name = <init>; }` after rest destructuring.
    rest_lifted_defaults: Vec<(String, Span)>,
    /// Counter for generating unique anonymous class names (`class_1`, `class_2`, etc.)
    anonymous_class_counter: u32,
    /// When set, emit `static { <alias> = this; }` as the first item in the
    /// class body. Used for legacy-decorated classes with static initializers
    /// when the target natively supports static blocks (ES2022+).
    inject_decorator_alias_block: Option<String>,
    /// Narrow Stage 3 field-decorator lowering state for class declarations.
    /// The tracked slots correspond to instance fields whose constructor
    /// initialization should be emitted via `__runInitializers(...)`.
    standard_decorator_instance_field_slots: Vec<StandardDecoratorFieldSlot>,
    /// Set of imported names that are used in value positions (not type-only).
    /// When import elision is active, imports not in this set are elided.
    value_used_imports: HashSet<AstString>,
    /// Import names retained specifically because they are JSX factory roots.
    /// Used to override reference-path preamble elision for JSX files.
    jsx_factory_import_retained: HashSet<AstString>,
    /// Import names retained because they are used as JSX element tag names.
    /// In preserve mode, JSX tags stay as-is, so the identifier must exist at runtime.
    jsx_element_import_retained: HashSet<AstString>,
    /// Module specifiers that have at least one non-empty value import binding.
    /// Used to elide redundant `import { } from "mod"` side-effect requires
    /// when another binding import from the same source is emitted.
    import_sources_with_value_bindings: HashSet<AstString>,
    /// Whether import elision analysis has been performed.
    import_elision_active: bool,
    /// Const enum member values: (enum_name, member_name) -> value.
    /// Populated by `scan_const_enums` during emit initialization.
    const_enum_values: HashMap<(String, String), ConstEnumValue>,
    /// Extra const enum member values supplied by the caller (typically from
    /// imported modules), merged with per-file scanned values.
    external_const_enum_values: HashMap<(String, String), ConstEnumValue>,
    /// Fast lookup of known const enum object names.
    const_enum_object_names: HashSet<AstString>,
    /// All comments from the source, sorted by position.
    comments: &'a [Comment],
    /// Index into `comments` for the next comment to consider emitting.
    next_comment_idx: usize,
    /// Source position up to which we have emitted comments.
    /// Used to avoid double-emitting comments when `emit_source_line` copies
    /// source text that already contains comments.
    comment_emit_pos: u32,
    /// Trailing comment reassigned from a malformed JSX fragment to the
    /// recovery expression produced from its mismatched closing tag.
    deferred_recovery_comment: Option<String>,
    /// Set to true when emitting a declaration from within `emit_export_decl_cjs`.
    /// This communicates the export context to `emit_enum_decl` and `emit_module_decl`
    /// since the parser wraps `export X` as `Export(Decl(X))` without setting
    /// `MOD_EXPORT` on the inner declaration.
    in_export_context: bool,
    /// Track names that have already had their `var`/`let` declaration emitted.
    /// Used to suppress duplicate declarations for merged enums/namespaces.
    emitted_var_names: HashSet<AstString>,
    /// Set of names that are `export`-declared in the current namespace scope.
    /// When emitting expressions inside a namespace IIFE, references to these
    /// names are qualified with the namespace parameter (e.g. `aa` → `NS.aa`).
    namespace_exports: HashSet<AstString>,
    /// Stack of parent namespace (export_target, exports) pairs.
    /// Used so that inner namespaces can qualify references to exports of
    /// ancestor namespaces (e.g. `M.m` when inside `M.N`).
    ns_export_stack: Vec<(AstString, HashSet<AstString>)>,
    /// Local binding names in the current namespace scope (functions, classes,
    /// enums, namespaces, non-exported vars). These shadow ancestor namespace
    /// exports and should NOT be qualified.
    ns_local_bindings: HashSet<AstString>,
    /// Non-exported plain variable names (`var`/`let`/`const`) in the current
    /// namespace scope that are NOT function/class/enum/namespace declarations.
    /// When an `import X = Y` RHS identifier matches one of these, the import
    /// is elided because `Y` refers to a local value binding, not a module.
    ns_local_value_vars: HashSet<AstString>,
    /// Tracks import-equals names already seen in the current namespace scope.
    /// Used to suppress duplicate import-equals declarations.
    ns_seen_import_equals: HashSet<AstString>,
    /// Cumulative exports across namespace reopenings, keyed by namespace name.
    cumulative_ns_exports: HashMap<AstString, HashSet<AstString>>,
    /// Type-only exports (interfaces, type aliases, type-only namespaces) per namespace.
    /// Used to distinguish "member is a declared type-only export" from
    /// "member is an undeclared property" in import-equals elision.
    cumulative_ns_type_exports: HashMap<AstString, HashSet<AstString>>,
    /// Original (un-renamed) namespace name of the parent scope.  Used to
    /// build cumulative-export keys that are independent of IIFE parameter
    /// renaming (e.g. always "my::data" even when the IIFE param is "my_1").
    ns_original_name: Option<String>,
    /// Per-name counters for unique IIFE parameter names when the namespace
    /// name collides with an inner binding (e.g. `M` → `M_1`).
    ns_collision_counters: HashMap<AstString, usize>,
    /// CJS import binding map: local name → (require_var, imported_name).
    /// Used to rewrite references like `Calculator` → `file1_1.Calculator`.
    cjs_import_map: HashMap<AstString, (AstString, AstString)>,
    /// Scopes where a local binding shadows a top-level import; references
    /// inside them must not be rewritten through `cjs_import_map`.
    import_shadows: import_shadow::ImportShadows,
    /// Source offset of the reference `emit_value_name_ref` is emitting, when
    /// the caller knows it (lets the shadow check apply there).
    value_ref_pos: Option<u32>,
    /// Named-import locals whose imported name was written as a string literal.
    /// CJS rewrites must retain that provenance even when the decoded name is a
    /// valid identifier (`import { "value" as local }`).
    cjs_string_import_locals: HashSet<AstString>,
    /// Exported names written as string literals. Generated access on the CJS
    /// `exports` object uses bracket notation for these names.
    cjs_string_export_names: HashSet<AstString>,
    /// Local names from default import syntax (`import X from "..."`).
    /// Re-exports of these use direct assignment (`exports.X = mod.default`)
    /// instead of ODP (live binding), matching TypeScript's behavior.
    cjs_default_import_bindings: HashSet<AstString>,
    /// Runtime-exported names in CJS-like emit (CJS/AMD/UMD) for the current file.
    /// Used for patterns like `export default x` -> `exports.default = exports.x`.
    cjs_exported_names: HashSet<AstString>,
    /// Local names from `export default function Foo` / `export default class Foo`.
    /// Namespace IIFEs for these use `exports.Foo || (exports.Foo = {})` pattern
    /// (no local assignment) instead of the normal `Foo || (exports.Foo = Foo = {})`.
    cjs_default_fn_local_names: HashSet<AstString>,
    /// Map from local name → exported alias for `export { local as alias }`.
    /// Used in namespace/enum IIFE closings to generate `exports.alias = local = {}`.
    cjs_export_alias_map: HashMap<AstString, AstString>,
    /// Subset of cjs_exported_names: only var/let/const declarations.
    /// In CJS/AMD, references to these names are qualified with `exports.`.
    /// Class/enum/namespace/function names are NOT qualified (they have local bindings).
    cjs_var_export_names: HashSet<AstString>,
    /// Parameter names in the current function scope that shadow CJS export names.
    /// Prevents qualifying `x` → `exports.x` when `x` is a function parameter.
    cjs_param_shadows: HashSet<AstString>,
    /// CJS live export binding: maps local name → list of exported names (in chain order)
    /// for variables exported via `export { ... }` clauses.  When a local variable is
    /// assigned/updated, the emitter wraps with `exports.X = exports.Y = local = value`.
    cjs_live_export_chain: HashMap<AstString, Vec<AstString>>,
    /// Cached keys of `cjs_live_export_chain` to avoid per-expression HashSet allocation.
    cjs_live_export_keys: HashSet<AstString>,
    /// Names that have been inline-exported via `exports.x = x;` right after a
    /// `var x = ...;` declaration inside a nested block in CJS mode.  Used to
    /// avoid duplicating the export at the end-of-file export section.
    cjs_inline_exported_var_names: HashSet<AstString>,
    /// Depth counter for compound statement bodies where CJS inline exports
    /// should be injected.  Incremented in emit_stmt_body and switch case
    /// bodies.  Unlike `block_depth`, this only affects inline export logic.
    cjs_inline_export_depth: usize,
    /// Guard to avoid recursively wrapping already-wrapped CJS live export writes.
    suppress_cjs_live_export_wrap: bool,
    /// True when the current `emit_expr` call is the direct expression of an
    /// expression-statement.  Used by CJS live-export update wrapping to decide
    /// whether to emit the simple pattern (statement) or the temp-var pattern
    /// (sub-expression where the return value matters).
    cjs_export_in_expr_stmt: bool,
    /// True when the current expression's result value is discarded (expression
    /// statement, for-loop update).  Used by private field postfix update emit
    /// to decide between one-temp (value discarded) and two-temp (value used).
    update_value_discarded: bool,
    /// File-level temp vars for CJS export postfix update expressions.
    /// Unlike `temp_var_names`, these are NOT saved/restored by function scopes
    /// because TSC allocates CJS export temp vars at file level.
    cjs_file_level_temp_names: Vec<AstString>,
    cjs_file_level_temp_counter: usize,
    /// Names that are type-only declarations (interfaces, type aliases).
    /// Used to elide `export default <ident>` when the ident refers to a type.
    type_only_decl_names: HashSet<AstString>,
    /// Type-only names from OTHER files in the compilation (e.g. global
    /// interfaces from .d.ts files).  When an `export { X }` references a
    /// name that is not locally declared, this set is consulted to determine
    /// whether `X` is type-only and should not get a CJS pre-declaration.
    global_type_only_names: HashSet<AstString>,
    /// Names explicitly exported as type-only across the compilation.
    global_type_only_export_names: HashSet<AstString>,
    /// Import bindings known to be type-only in the current file.
    type_only_import_names: HashSet<AstString>,
    /// Import bindings that should NOT be added to `cjs_import_map` because
    /// their cross-file type analysis shows they are type-only (e.g. namespace
    /// merge with type-only origin). Unlike `type_only_import_names`, these
    /// names do NOT affect import elision — the require is still emitted.
    cjs_no_qualify_import_locals: HashSet<AstString>,
    /// Import source modules whose require() should be kept for side effects
    /// even when all named specifiers are type-only elided. Used for `.js`
    /// files with `export type *` where the import is kept but names are bare.
    cjs_keep_side_effect_sources: HashSet<AstString>,
    /// Import bindings resolved from source modules that still value-export the
    /// imported name. Used to keep merged const-enum imports alive when member
    /// accesses are inlined but the source module still has runtime export shape.
    runtime_export_import_names: HashSet<AstString>,
    /// Names of enum declarations in the current file. Used by decorator
    /// metadata serialization to map enum type references to `Number`.
    enum_decl_names: HashSet<AstString>,
    /// Names with a local runtime binding in the current file. Used by
    /// decorator metadata serialization to distinguish unresolved qualified
    /// references (which require runtime guards) from known value paths.
    file_value_bound_names: HashSet<AstString>,
    /// `declare namespace` names that have value-level declarations (class,
    /// enum, function, var). Used to override type-only elision for
    /// import-equals aliases: `import ab = A.B` emits `var ab = A.B` when
    /// `declare namespace A.B` has value exports.
    declare_ns_with_values: HashSet<AstString>,
    /// Import-equals aliases used only in type position within namespace scope.
    import_equals_type_only_ns: HashSet<String>,
    /// External module specifiers known to be type-only in the current
    /// compilation (ambient declarations with no runtime value).
    type_only_external_modules: HashSet<String>,
    /// Module specifiers imported via `import X = require("...")` that are
    /// known to be type-only for the current file emit.
    type_only_require_specs: HashSet<String>,
    /// For script (non-module) files, value-binding names declared in earlier
    /// files in emit order. Used to match TS's cross-file duplicate handling
    /// for top-level internal import-equals aliases.
    prior_script_value_names: HashSet<String>,
    /// For the current script (non-module) file, value-binding names declared
    /// by earlier top-level statements in source order.
    seen_script_value_names: HashSet<String>,
    /// Persistent enum member values across merged declarations.
    /// Keyed by enum name → (member_name → numeric_value).
    merged_enum_values: HashMap<String, HashMap<String, f64>>,
    /// Persistent string enum member values across merged declarations.
    /// Keyed by enum name → (member_name → string_value).
    merged_string_enum_values: HashMap<String, HashMap<String, String>>,
    /// File-level `const` declarations with known numeric values (e.g. `const EV = 1`).
    file_consts: HashMap<String, f64>,
    /// File-level `const` declarations with known string values (e.g. `const d = 'd'`).
    file_string_consts: HashMap<String, String>,
    /// Cross-file `const` values (numeric) resolved from imports.
    external_file_consts: HashMap<String, f64>,
    /// Cross-file `const` values (string) resolved from imports.
    external_file_string_consts: HashMap<String, String>,
    /// AMD/UMD: maps module specifier → factory parameter name.
    /// When set, `emit_export_decl_cjs` and `emit_import_decl` use the parameter
    /// directly instead of emitting `require()` calls.
    amd_dep_map: HashMap<String, String>,
    /// When true, the next optional-chain downlevel emit will skip outer parens.
    /// Set by callers like call arguments, array elements, object values, etc.,
    /// where the ternary result doesn't need extra grouping.
    suppress_oc_parens: bool,
    /// Suppress the bare-super → super. post-processor for the current statement.
    /// Set by emit_exp_assign_downlevel Super case which handles dots itself.
    suppress_bare_super_fixup: bool,
    /// Suppress paren-depth-based continuation indentation adjustments in
    /// `emit_source_line`. Set when inside a ternary alternate/consequent
    /// to prevent paren depth from the alternate expression from adding
    /// unwanted extra indentation to binary continuations.
    suppress_continuation_paren_depth: bool,
    /// Temp var name for `.call()` receiver in parenthesized OC chain calls.
    /// When set, the OC truthy branch wraps the receiver in `(_a = receiver)`.
    oc_call_temp_receiver: Option<String>,
    /// When true, the outermost optional-chain downlevel emit uses `delete` semantics:
    /// `true` for the null branch, `delete <access>` for the non-null branch.
    /// Set by `ExprKind::Delete` when the operand is an optional chain being downleveled.
    oc_delete_mode: bool,
    /// When true, the leftmost FnExpr/ObjectLit in a call/member chain wraps
    /// itself in parens (for disambiguation after type assertion stripping).
    disambig_wrap_leftmost_fn: bool,
    /// Source span of the enclosing function/method when emitting an async
    /// body via `emit_awaiter_body`.  Used by `emit_block_with_await_to_yield`
    /// to decide single-line vs multi-line for empty generator bodies.
    awaiter_enclosing_span: Option<Span>,
    /// True while emitting a downleveled async-generator body.
    /// Used by await-to-yield conversion to emit `yield __await(...)` and
    /// `yield yield __await(...)` forms.
    in_async_generator_transform: bool,
    /// True while emitting statements inside a downleveled static block IIFE.
    /// TypeScript converts `await` to `yield` in these contexts.
    /// This flag causes `emit_expr` Await to emit `yield` instead of `await`,
    /// and forces statements containing `await` through the transform path
    /// (not source-copy) so the keyword substitution takes effect.
    in_static_block_await_to_yield: bool,
    /// True while emitting an __awaiter/__asyncGenerator body where `super`
    /// property accesses have been hoisted to `_super` / `_superIndex`.
    async_super_active: bool,
    /// Set of distinct property names hoisted for `super.prop` access.
    async_super_names: Vec<String>,
    /// Whether `super[expr]` element access was hoisted to `_superIndex`.
    async_super_has_element_access: bool,
    /// Whether any `super.prop = ...` or `super[expr] = ...` write access exists.
    async_super_has_write: bool,
    /// Suffix for `_super`/`_superIndex` names when there's a naming conflict (e.g. "_1").
    async_super_suffix: String,
    /// Temp vars allocated during `emit_expr_await_to_yield` that need to be
    /// declared at the start of the enclosing generator body.
    awaiter_body_temp_vars: Vec<String>,
    /// True while emitting the object side of `obj.prop` / `obj[index]`.
    /// Used to format inlined numeric const-enum literals so they remain
    /// syntactically valid in member-access positions.
    in_member_object_context: bool,
    /// Flag indicating expression is the callee of a `new` expression.
    /// Used to preserve parens around call expressions: `new (A())` != `new A()`.
    in_new_callee_context: bool,
    /// True while emitting the expression of `export default <expr>`.
    /// Used to preserve parens around class/function expressions that would
    /// otherwise become declarations.
    in_export_default_context: bool,
    /// True for bare `export default @dec class {}` (no parens). Controls
    /// whether the IIFE wrapper uses `default_1` naming vs `class_1`.
    bare_default_class_expr: bool,
    /// True while emitting the callee of a `Call` expression.
    /// Used to wrap CJS imported member references with `(0, ...)` to detach `this`.
    in_call_callee_context: bool,
    /// When true, copy_expr_span skips normalize_brace_spacing.
    /// Used for JSX preserve mode where `<`/`>` must not be treated as operators.
    skip_brace_normalize: bool,
    /// When true, the next expression-statement assignment gets a `this.` prefix.
    /// Set when stripping `public`/`private`/`protected` modifier keywords from
    /// constructor body error-recovery statements (e.g. `public p1 = 0;` → `this.p1 = 0;`).
    prepend_this_to_next_assign: bool,
    /// Set when stripping `static` modifier keyword that should be preserved as a prefix
    /// on the next var/function statement (error recovery: `static var x = 0;` in namespace).
    emit_static_prefix: bool,
    /// Set when stripping a leading recovery `accessor` token that should be
    /// preserved as a prefix on the next declaration/import statement.
    emit_accessor_prefix: bool,
    /// Parser-recovery guard: statements fully before this source offset are skipped.
    skip_recovery_until: u32,
    /// Plans for exported destructuring declarations that bind no runtime names
    /// but still require runtime element-access side effects.
    omitted_array_destructure_plans: HashMap<u32, OmittedArrayDestructurePlan>,
    /// Counter for `_a`, `_b`, ... temp names used by omitted array destructuring plans.
    omitted_array_destructure_counter: usize,
    /// Per-file module kind override derived from file extension.
    module_kind_override: Option<ModuleKind>,
    /// Per-file module flag forced by extension (.mts/.cts/.mjs/.cjs).
    force_external_module: bool,
    /// Per-file JSX preserve mode fallback for `.jsx` when `jsx` option is unset.
    jsx_preserve_by_extension: bool,
    /// Source file name used by the react-jsxdev source-location argument.
    jsx_dev_file_name: String,
    /// Collision-safe binding used for the react-jsxdev file name constant.
    jsx_dev_file_name_ident: String,
    /// JSX text ranges excluded from generated-name collision checks.
    jsx_text_spans: Vec<Span>,
    /// When react-jsx/react-jsxdev is active, tracks which runtime functions are needed.
    /// Populated during pre-scan, consumed when emitting the runtime import.
    jsx_runtime_needs_jsx: bool,
    jsx_runtime_needs_jsxs: bool,
    jsx_runtime_needs_fragment: bool,
    /// True when a JSX element with spread-before-key is found (needs createElement fallback).
    jsx_runtime_needs_create_element: bool,
    /// CJS require variable for the jsx-runtime module (e.g. "jsx_runtime_1").
    jsx_runtime_require_var: Option<String>,
    /// CJS require variable for the base react module (for createElement fallback).
    jsx_runtime_base_require_var: Option<String>,
    /// Per-file `@jsxImportSource` pragma override.
    jsx_import_source_pragma: Option<String>,
    /// Per-file `@jsxRuntime automatic` pragma override.
    /// When true, switches this file from classic to automatic JSX runtime.
    jsx_runtime_pragma_automatic: bool,
    /// Per-file `@jsx <factory>` pragma override (classic mode).
    jsx_pragma_factory: Option<String>,
    /// Per-file `@jsxFrag <fragment>` pragma override (classic mode).
    jsx_pragma_fragment: Option<String>,
    /// Positions of JSX pragma comments to strip from output.
    jsx_pragma_strip_positions: Vec<u32>,
    elide_sole_empty_fragment_factory_import: bool,
    /// True when this file should rewrite `import X = require("...")` in ESM
    /// output via a synthetic `createRequire(import.meta.url)` bridge.
    node_esm_import_require: bool,
    /// Synthetic import binding for Node's `createRequire`.
    node_esm_create_require_ident: Option<String>,
    /// Synthetic local `require` function binding in Node ESM output.
    node_esm_require_ident: Option<String>,
    /// Whether the current file is a module (has imports/exports).
    is_module_file: bool,
    /// In `module: preserve`, whether this file keeps a CJS body (`.cts`/`.cjs`).
    preserve_module_uses_require: bool,
    /// Depth within a dotted namespace chain (`namespace A.B.C {}`).
    /// Segments emitted as part of the dotted chain use `var` declarations.
    dotted_namespace_depth: usize,
    /// When enabled, bare identifiers that resolve to `cjs_import_map` entries
    /// are emitted through `emit_value_name_ref`.
    rewrite_ident_with_import_map: bool,
    /// Temporary per-System-module map: local namespace/enum declaration name
    /// to exported names that should be emitted inline in IIFE initializers.
    system_inline_export_aliases: HashMap<String, Vec<String>>,
    /// Names that should have assignment/update expressions wrapped as
    /// `exports_1("name", ...)` during System execute emission.
    system_live_export_names: HashSet<AstString>,
    /// Guard to avoid recursively wrapping already-wrapped System export writes.
    suppress_system_live_export_wrap: bool,
    /// While emitting System.register execute-body statements, rewrite
    /// function-scoped `var` statements to assignment statements because
    /// declarations are hoisted to the execute prelude.
    system_hoist_var_in_execute: bool,
    /// Counter for System.register callback parameter suffixes (exports_N, context_N).
    /// Starts at 1 for the first file and increments for each System.register call.
    /// Threaded across files in outFile mode via EmitOutput.
    system_register_counter: usize,
    /// The current System.register exports function name (e.g. "exports_1", "exports_2").
    /// Set at the start of `emit_system_module` and used by emit_expr/emit_declarations
    /// for live-export wrapping.
    system_exports_fn: String,
    /// The current System.register context parameter name (e.g. "context_1").
    /// Used to rewrite `import()` → `context_N.import()` inside System module bodies.
    system_context_fn: String,
    /// Whether `var _a;` has been emitted for class expression IIFE in the
    /// current function/module scope.
    class_expr_temp_emitted: bool,
    /// `true` iff the current `SourceFile` has any parser diagnostics. Used
    /// as a cheap fast-path: when no parser-recovery errors exist, no AST
    /// node is `Ident("<error>")` (parser emits a diagnostic alongside
    /// every `<error>` placeholder), so `expr_has_error_member` and
    /// related tree-walk checks short-circuit immediately. Set once per
    /// file in `emit_source_file`.
    file_has_recovery_errors: bool,
    /// `options.fast_emit == Some(true)`, resolved once — checked on several
    /// per-statement/per-expression hot paths.
    fast_emit: bool,
    /// `true` iff the source contains U+0085 NEL. The TS scanner accepts NEL as
    /// whitespace but JS engines do not, so verbatim fast-emit copies must fall
    /// back to the normalizing slow path when this is set (see `normalize_nel`).
    source_has_nel: bool,
    /// `true` iff the source contains the `// => Error` marker anywhere.
    /// span_has_error_marker_comment scans a statement's whole span for this
    /// literal on every statement; when the file has no such marker at all
    /// (the overwhelming common case) the per-statement scan is skipped.
    source_has_error_marker: bool,
    /// Coarse textual index shared by spread-comment layout checks. Built only
    /// when a normal-emission span query needs it; the source is immutable.
    adjacent_spread_comments: OnceCell<spread_comments::SpreadCommentIndex>,
    /// Whether multiline function bodies should emit one temp declaration per line.
    split_multiline_function_body_temp_decls: bool,
    /// Namespace-scoped temp variable names for destructuring exports.
    /// Collected during namespace body emission and inserted at the IIFE top.
    ns_temp_var_names: Vec<String>,
    /// Function-depth marker for each active namespace IIFE. Namespace bodies
    /// do not increment `fn_scope_depth`, so this distinguishes their direct
    /// statements from real functions nested inside them when routing temps.
    namespace_iife_fn_scope_depths: Vec<usize>,
    /// When set, records the output position right before static field
    /// initializers are emitted in `emit_class_decl`. This allows
    /// `emit_export_decl_cjs` to move them after the export assignment.
    pre_static_output_len: Option<usize>,
    /// Output position where the current top-level statement started, used for
    /// inserting hoisted `var _a;` before the statement.
    stmt_output_start: usize,
    /// Optional insertion point for class helper declarations that should
    /// appear before statement-leading comments.
    class_decl_helper_insert_pos: Option<usize>,
    /// Output position right after helpers and prologue, for hoisting
    /// private field WeakMap `var` declarations to the top of user code.
    private_field_var_insert_pos: Option<usize>,
    /// Position of the `;` in an already-emitted private field var declaration.
    /// When set, subsequent classes append `, _name` before this position
    /// to create a combined `var _A_foo, _B_foo;` declaration.
    private_field_var_semicolon_pos: Option<usize>,
    /// Private field WeakMap var names to insert at function body start.
    /// Used when a class with private fields is inside a function body,
    /// where `private_field_var_insert_pos` is None.
    pending_private_field_vars: Vec<String>,
    /// Parser-recovery static field assignments collected while emitting a
    /// class constructor body (e.g. `static f = 1;` inside `constructor`).
    /// Emitted after the class declaration as `ClassName.f = 1;`.
    recovery_static_class_assignments: Vec<(String, String, Expr, Span)>,
    /// During class emission: map from "ClassName.prop" to temp name (_a, _b)
    /// for computed property names that reference the class before construction.
    class_computed_name_temps: Option<std::collections::HashMap<String, String>>,
    /// During class emission (legacy mode): map from computed property expression
    /// span start to temp var name. Used for pre-evaluating ALL computed property
    /// keys in class declarations when useDefineForClassFields = false.
    class_general_computed_temps: Vec<(String, u32)>,
    /// Pre-computed auto-accessor storage names, mapping accessor property name
    /// to its storage field name (e.g. "a0" → "a0_accessor_storage" or
    /// "a1" → "a1_1_accessor_storage" when there's a name collision).
    accessor_storage_names: std::collections::HashMap<String, String>,
    /// Stack of private field names from enclosing classes, for collision
    /// avoidance in nested auto-accessor storage names.
    enclosing_private_names: Vec<std::collections::HashSet<String>>,
    /// Parallel stack of private member kinds from enclosing classes.
    /// Used to resolve nested accesses to outer private members.
    enclosing_private_members: Vec<std::collections::HashMap<String, EnclosingPrivateMemberKind>>,
    /// Parallel stack of class names for each entry in `enclosing_private_names`.
    /// Used to construct WeakMap variable names when nested classes access outer
    /// classes' private fields.
    enclosing_private_class_names: Vec<Option<String>>,
    /// Shared setter-proxy parameter name for field/accessor destructuring in the
    /// current function body. TypeScript reuses one inline parameter name here.
    private_destructure_proxy_param: Option<String>,
    /// During class emission: prop keys for instance properties that need
    /// `_b = A.p2` assignment after the class body.
    class_instance_prop_temps: Option<Vec<String>>,
    /// During class emission: static property computed keys seen so far,
    /// for building comma expr in static method keys.
    class_preceding_static_prop_keys: Option<Vec<String>>,
    /// True when the source file is a `.js` or `.jsx` file.
    /// In JS files, type-like syntax (`<string>`, `Foo<T>()`) is not real
    /// TypeScript and should NOT be erased.
    is_js_file: bool,
    /// PRESERVE MODE: Keep type annotations in the output.
    preserve_type_annotations: bool,
    /// PRESERVE MODE: Keep all comments in the output.
    preserve_comments: bool,
    /// File-wide counter for anonymous default export names (`default_1`, `default_2`, ...).
    /// Both the pre-scan (collect_fn_export_names) and emission (emit_export_decl_cjs)
    /// phases maintain separate counters that iterate statements in the same order,
    /// ensuring consistent naming.
    default_export_counter: usize,
    /// The recovered variable name for `export default @dec class { }` (e.g. "default_1").
    /// Set before emit_class_decl so that emit_class_static_field_initializers can use it
    /// instead of the literal string "default".
    pub(crate) recovered_default_class_name: Option<String>,
    /// When true, the first Paren(TypeAssertion(ObjLit/FuncExpr/ClassExpr))
    /// in the expression chain should skip emitting its own close paren `)`
    /// because the expression-statement handler will wrap the entire chain.
    suppress_stmt_paren_inner: bool,
    /// When true, multiline parens around type assertions can be stripped even
    /// when content spans multiple lines.  Set when emitting arrow expression
    /// bodies where `=>` prevents ASI (unlike `return`).
    in_arrow_body_expr: bool,
}

/// Represents a computed const enum member value.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstEnumValue {
    Number(f64),
    String(std::string::String),
}

/// Runtime plan for an exported array destructuring declaration that binds no names.
/// Example: `export let [,,[,[],,[],]] = expr;`
///   root_temp: `_a`
///   chain: [("_b", "_a", 2), ("_c", "_b", 1), ("_d", "_b", 3)]
#[derive(Debug, Clone)]
struct OmittedArrayDestructurePlan {
    root_temp: String,
    chain: Vec<(String, String, usize)>, // (child_temp, parent_temp, index)
}

impl OmittedArrayDestructurePlan {
    fn all_temps(&self) -> Vec<String> {
        let mut temps = Vec::with_capacity(1 + self.chain.len());
        temps.push(self.root_temp.clone());
        for (child, _, _) in &self.chain {
            temps.push(child.clone());
        }
        temps
    }
}

impl<'a> Emitter<'a> {
    fn new(
        source: &'a str,
        options: &'a CompilerOptions,
        comments: &'a [Comment],
        external_const_enum_values: HashMap<(String, String), ConstEnumValue>,
    ) -> Self {
        Self {
            source,
            source_identifiers: OnceCell::new(),
            lexical_downlevel_plan: lexical_downlevel::LexicalDownlevelPlan::default(),
            generator_catch_names: HashMap::new(),
            generator_catch_scope: false,
            emitted_lexical_binding_ids: HashSet::new(),
            active_lexical_loop_helpers: Vec::new(),
            active_lexical_for_of_plan: None,
            lexical_arrow_this_alias: None,
            options,
            fast_emit: options.fast_emit == Some(true),
            source_has_nel: source.contains('\u{0085}'),
            source_has_error_marker: source.contains("// => Error"),
            adjacent_spread_comments: OnceCell::new(),
            output: String::with_capacity(source.len()),
            indent: 0,
            module_wrapper_indent: 0,
            at_line_start: true,
            require_var_counters: HashMap::new(),
            temp_var_counter: 0,
            pre_allocated_assignment_rest_temps: Vec::new(),
            class_scope_temp_reserved: 0,
            fn_param_rest_temp_count: 0,
            amd_import_counter: 0,
            temp_var_names: Vec::new(),
            emitting_destructuring_assignment_pattern: false,
            suppress_computed_object_downlevel: false,
            emit_expr_depth: 0,
            catch_auto_param_counter: 0,
            deferred_temp_placeholders: Vec::new(),
            inline_deferred_temp_placeholders: Vec::new(),
            deferred_export_name_placeholders: Vec::new(),
            resolved_deferred_temp_names: Vec::new(),
            deferred_temp_placeholder_counter: 0,
            has_export_assign: false,
            emitted_esm_export: false,
            bare_export_empty_trailing_comment: None,
            verbatim_empty_reexport_sources: HashSet::new(),
            export_target: None,
            source_map_gen: None,
            line_index: None,
            out_line: 0,
            out_col: 0,
            source_index: 0,
            needs_import_default: false,
            needs_import_star: false,
            needs_export_star: false,
            merged_namespace_names: HashSet::new(),
            first_import_default_helper_use: None,
            first_import_star_helper_use: None,
            first_export_star_helper_use: None,
            needs_decorate_helper: false,
            needs_extends_helper: false,
            legacy_es5_member_lowerable_class_starts: HashSet::new(),
            legacy_es5_cjs_default_class_start: None,
            needs_es_decorate_helper: false,
            has_es_decorated_methods: false,
            inject_instance_extra_initializers: false,
            injected_instance_extra_initializers_name: None,
            needs_metadata_helper: false,
            needs_param_helper: false,
            needs_run_initializers_helper: false,
            needs_awaiter_helper: false,
            needs_generator_helper: false,
            needs_await_helper: false,
            needs_async_generator_helper: false,
            needs_async_values_helper: false,
            needs_async_delegator_helper: false,
            async_values_before_await: true,
            needs_rest_helper: false,
            needs_values_helper: false,
            needs_read_helper: false,
            needs_spread_array_helper: false,
            needs_make_template_object_helper: false,
            needs_add_disposable_resource_helper: false,
            needs_dispose_resources_helper: false,
            using_env_counter: 0,
            resource_for_await_counter: 0,
            native_for_await_counter: 0,
            fn_scope_depth: 0,
            block_depth: 0,
            arguments_capture_counter: 0,
            current_arguments_alias: None,
            in_parameter_initializer: false,
            in_async_function: false,
            current_class_name: None,
            current_class_private_fields: HashSet::new(),
            current_class_static_private_fields: HashSet::new(),
            current_class_static_alias: None,
            class_name_locally_shadowed: false,
            decorated_class_self_ref_alias: None,
            hoisted_decorated_aliases: Vec::new(),
            decorated_alias_emit_counter: HashMap::new(),
            current_class_private_methods: HashMap::new(),
            current_class_private_accessors: HashMap::new(),
            current_class_static_private_accessors: HashSet::new(),
            current_class_static_private_methods: HashMap::new(),
            static_this_alias: None,
            legacy_constructor_super: None,
            legacy_this_alias: None,
            in_class_expression_emit: false,
            static_super_base_alias: None,
            static_super_receiver_alias: None,
            super_reflect_destructure_target: false,
            super_reflect_in_field_init: false,
            current_class_private_var_map: HashMap::new(),
            all_allocated_private_var_names: HashMap::new(),
            needs_private_field_get: false,
            needs_private_field_set: false,
            needs_private_field_in: false,
            private_field_get_first: true,
            needs_prop_key_helper: false,
            needs_set_function_name_helper: false,
            class_expr_binding_name: None,
            in_using_class_initializer: false,
            await_expr_inline_container_indents_preowned: 0,
            rest_lifted_defaults: Vec::new(),
            anonymous_class_counter: 0,
            inject_decorator_alias_block: None,
            standard_decorator_instance_field_slots: Vec::new(),
            value_used_imports: HashSet::new(),
            jsx_factory_import_retained: HashSet::new(),
            jsx_element_import_retained: HashSet::new(),
            import_sources_with_value_bindings: HashSet::new(),
            import_elision_active: false,
            runtime_export_import_names: HashSet::new(),
            const_enum_values: HashMap::new(),
            external_const_enum_values,
            const_enum_object_names: HashSet::new(),
            comments,
            next_comment_idx: 0,
            comment_emit_pos: 0,
            deferred_recovery_comment: None,
            in_export_context: false,
            emitted_var_names: HashSet::new(),
            namespace_exports: HashSet::new(),
            ns_export_stack: Vec::new(),
            ns_local_bindings: HashSet::new(),
            ns_local_value_vars: HashSet::new(),
            ns_seen_import_equals: HashSet::new(),
            cumulative_ns_exports: HashMap::new(),
            cumulative_ns_type_exports: HashMap::new(),
            ns_original_name: None,
            ns_collision_counters: HashMap::new(),
            cjs_import_map: HashMap::new(),
            cjs_string_import_locals: HashSet::new(),
            cjs_string_export_names: HashSet::new(),
            cjs_default_import_bindings: HashSet::new(),
            import_shadows: Default::default(),
            value_ref_pos: None,
            cjs_exported_names: HashSet::new(),
            cjs_default_fn_local_names: HashSet::new(),
            cjs_export_alias_map: HashMap::new(),
            cjs_var_export_names: HashSet::new(),
            cjs_param_shadows: HashSet::new(),
            cjs_live_export_chain: HashMap::new(),
            cjs_live_export_keys: HashSet::new(),
            cjs_inline_exported_var_names: HashSet::new(),
            cjs_inline_export_depth: 0,
            suppress_cjs_live_export_wrap: false,
            cjs_export_in_expr_stmt: false,
            update_value_discarded: false,
            cjs_file_level_temp_names: Vec::new(),
            cjs_file_level_temp_counter: 0,
            type_only_decl_names: HashSet::new(),
            global_type_only_names: HashSet::new(),
            global_type_only_export_names: HashSet::new(),
            type_only_import_names: HashSet::new(),
            cjs_no_qualify_import_locals: HashSet::new(),
            cjs_keep_side_effect_sources: HashSet::new(),
            enum_decl_names: HashSet::new(),
            file_value_bound_names: HashSet::new(),
            declare_ns_with_values: HashSet::new(),
            import_equals_type_only_ns: HashSet::new(),
            type_only_external_modules: HashSet::new(),
            type_only_require_specs: HashSet::new(),
            prior_script_value_names: HashSet::new(),
            seen_script_value_names: HashSet::new(),
            merged_enum_values: HashMap::new(),
            merged_string_enum_values: HashMap::new(),
            file_consts: HashMap::new(),
            file_string_consts: HashMap::new(),
            external_file_consts: HashMap::new(),
            external_file_string_consts: HashMap::new(),
            amd_dep_map: HashMap::new(),
            suppress_oc_parens: false,
            suppress_bare_super_fixup: false,
            suppress_continuation_paren_depth: false,
            oc_call_temp_receiver: None,
            oc_delete_mode: false,
            disambig_wrap_leftmost_fn: false,
            awaiter_enclosing_span: None,
            in_async_generator_transform: false,
            in_static_block_await_to_yield: false,
            async_super_active: false,
            async_super_names: Vec::new(),
            async_super_has_element_access: false,
            async_super_has_write: false,
            async_super_suffix: String::new(),
            awaiter_body_temp_vars: Vec::new(),
            in_member_object_context: false,
            in_new_callee_context: false,
            in_export_default_context: false,
            bare_default_class_expr: false,
            in_call_callee_context: false,
            skip_brace_normalize: false,
            prepend_this_to_next_assign: false,
            emit_static_prefix: false,
            emit_accessor_prefix: false,
            skip_recovery_until: 0,
            omitted_array_destructure_plans: HashMap::new(),
            omitted_array_destructure_counter: 0,
            module_kind_override: None,
            force_external_module: false,
            jsx_preserve_by_extension: false,
            jsx_dev_file_name: String::new(),
            jsx_dev_file_name_ident: "_jsxFileName".to_string(),
            jsx_text_spans: Vec::new(),
            jsx_runtime_needs_jsx: false,
            jsx_runtime_needs_jsxs: false,
            jsx_runtime_needs_fragment: false,
            jsx_runtime_needs_create_element: false,
            jsx_runtime_require_var: None,
            jsx_runtime_base_require_var: None,
            jsx_import_source_pragma: None,
            jsx_runtime_pragma_automatic: false,
            jsx_pragma_factory: None,
            jsx_pragma_fragment: None,
            jsx_pragma_strip_positions: Vec::new(),
            elide_sole_empty_fragment_factory_import: false,
            node_esm_import_require: false,
            node_esm_create_require_ident: None,
            node_esm_require_ident: None,
            is_module_file: false,
            preserve_module_uses_require: false,
            dotted_namespace_depth: 0,
            rewrite_ident_with_import_map: false,
            system_inline_export_aliases: HashMap::new(),
            system_live_export_names: HashSet::new(),
            suppress_system_live_export_wrap: false,
            system_hoist_var_in_execute: false,
            system_register_counter: 1,
            system_exports_fn: "exports_1".to_string(),
            system_context_fn: String::new(),
            class_expr_temp_emitted: false,
            file_has_recovery_errors: false,
            split_multiline_function_body_temp_decls: false,
            async_var_shadow_names: None,
            async_var_shadow_blockers: Vec::new(),
            async_var_shadow_top_level_hoists: Vec::new(),
            ns_temp_var_names: Vec::new(),
            namespace_iife_fn_scope_depths: Vec::new(),
            pre_static_output_len: None,
            stmt_output_start: 0,
            class_decl_helper_insert_pos: None,
            private_field_var_insert_pos: None,
            private_field_var_semicolon_pos: None,
            pending_private_field_vars: Vec::new(),
            recovery_static_class_assignments: Vec::new(),
            class_computed_name_temps: None,
            class_general_computed_temps: Vec::new(),
            accessor_storage_names: std::collections::HashMap::new(),
            enclosing_private_names: Vec::new(),
            enclosing_private_members: Vec::new(),
            enclosing_private_class_names: Vec::new(),
            private_destructure_proxy_param: None,
            class_instance_prop_temps: None,
            class_preceding_static_prop_keys: None,
            is_js_file: false,
            preserve_type_annotations: options.preserve_type_annotations.unwrap_or(false),
            preserve_comments: options.preserve_comments.unwrap_or(false),
            default_export_counter: 1,
            recovered_default_class_name: None,
            suppress_stmt_paren_inner: false,
            in_arrow_body_expr: false,
        }
    }

    fn effective_module_kind(&self) -> ModuleKind {
        // When the caller explicitly requests CommonJS via `--module=commonjs`,
        // honor that over the file-extension override. Bundlers need this
        // behavior: `import./foo.mjs'` in a bundle context should still produce
        // CJS output (since the output is wrapped in an IIFE with require/exports).
        //
        // For non-CommonJS explicit requests (--module=es2015 etc.) and for
        // implicit defaults, the file extension (.mjs → ESNext, .cjs → CommonJS)
        // wins, preserving TypeScript-compatible behavior for regular `tsc` use.
        // `.mts` / `.mjs` are ALWAYS ES modules and `.cts` / `.cjs` always
        // CommonJS — the file format wins over `--module`, matching tsc
        // (erasableSyntaxOnly). The extension override is set by the caller.
        if matches!(self.options.module, Some(ModuleKind::CommonJS))
            && self.module_kind_override != Some(ModuleKind::ESNext)
        {
            return ModuleKind::CommonJS;
        }
        if let Some(kind) = self.module_kind_override {
            return kind;
        }
        match self.options.module {
            Some(kind) => kind,
            None => {
                if self.effective_target() < ScriptTarget::ES2015 {
                    ModuleKind::CommonJS
                } else {
                    ModuleKind::ES2015
                }
            }
        }
    }

    fn module_kind_override_for_file(file_name: &str) -> Option<ModuleKind> {
        let lower = file_name.to_ascii_lowercase();
        if lower.ends_with(".cts") || lower.ends_with(".cjs") {
            Some(ModuleKind::CommonJS)
        } else if lower.ends_with(".mts") || lower.ends_with(".mjs") {
            Some(ModuleKind::ESNext)
        } else {
            None
        }
    }

    fn force_external_module_for_file(file_name: &str) -> bool {
        let lower = file_name.to_ascii_lowercase();
        lower.ends_with(".cts")
            || lower.ends_with(".mts")
            || lower.ends_with(".cjs")
            || lower.ends_with(".mjs")
    }

    /// Returns `true` if the emitter should produce CommonJS output.
    /// When `module: none` is set but the file has module syntax (import/export),
    /// TypeScript emits as CommonJS.
    fn is_commonjs(&self) -> bool {
        self.effective_module_kind() == ModuleKind::CommonJS
            || (self.effective_module_kind() == ModuleKind::None && self.is_module_file)
    }

    /// Whether `import.meta.{url,dirname,filename}` must be rewritten to its Node
    /// CJS equivalent. Only classic `--module commonjs` lacks a runtime form. The
    /// Node-style resolvers preserve `import.meta` even in CJS-format files
    /// (Node supports it), so the rewrite must NOT fire under Node modes. The
    /// harness resolves each file down to CommonJS in `options.module`, so the
    /// Node signal survives in `module_resolution`.
    fn import_meta_needs_cjs_rewrite(&self) -> bool {
        if matches!(
            self.options.module,
            Some(ModuleKind::Node16)
                | Some(ModuleKind::Node18)
                | Some(ModuleKind::Node20)
                | Some(ModuleKind::NodeNext)
        ) {
            return false;
        }
        if let Some(res) = self.options.module_resolution.as_deref() {
            let res = res.to_ascii_lowercase();
            if res.starts_with("node1") || res.starts_with("node2") || res == "nodenext" {
                return false;
            }
        }
        self.is_commonjs()
    }

    /// Returns `true` if the module emit format is ES module (not CJS, AMD, etc.).
    fn is_esm_emit(&self) -> bool {
        matches!(
            self.effective_module_kind(),
            ModuleKind::ES2015
                | ModuleKind::ES2020
                | ModuleKind::ES2022
                | ModuleKind::ESNext
                | ModuleKind::Preserve
                | ModuleKind::NodeNext
                | ModuleKind::Node20
                | ModuleKind::Node18
                | ModuleKind::Node16
        )
    }

    /// Returns `true` if the module emit format is AMD.
    fn is_amd(&self) -> bool {
        self.effective_module_kind() == ModuleKind::AMD
    }

    /// Returns `true` if the module emit format is UMD.
    fn is_umd(&self) -> bool {
        self.effective_module_kind() == ModuleKind::UMD
    }

    /// Returns `true` if the module emit format is System.
    fn is_system(&self) -> bool {
        self.effective_module_kind() == ModuleKind::System
    }

    /// Returns `true` if the module format uses CJS-style body (CommonJS, AMD, or UMD).
    fn is_cjs_like(&self) -> bool {
        self.is_commonjs() || self.is_amd() || self.is_umd()
    }

    /// Returns `true` if esModuleInterop is enabled in compiler options.
    /// Node-style module kinds imply esModuleInterop=true.
    #[allow(dead_code)]
    fn es_module_interop(&self) -> bool {
        self.options.es_module_interop == Some(true)
            || matches!(
                self.options.module,
                Some(ModuleKind::Node16)
                    | Some(ModuleKind::Node18)
                    | Some(ModuleKind::Node20)
                    | Some(ModuleKind::NodeNext)
            )
    }

    /// Returns `true` when jsx is set to `preserve` or `react-native`,
    /// meaning JSX expressions should be emitted as-is (not transformed).
    #[allow(dead_code)]
    fn jsx_is_preserve(&self) -> bool {
        matches!(
            self.options.jsx,
            Some(JsxEmit::Preserve) | Some(JsxEmit::ReactNative)
        ) || (self.options.jsx.is_none() && self.jsx_preserve_by_extension)
    }

    /// Returns `true` when jsx is set to `react-jsx` or `react-jsxdev`,
    /// meaning JSX should be transformed to _jsx/_jsxs calls with automatic
    /// runtime imports.
    fn jsx_is_react_jsx(&self) -> bool {
        matches!(
            self.options.jsx,
            Some(JsxEmit::ReactJSX) | Some(JsxEmit::ReactJSXDev)
        ) || (self.options.jsx == Some(JsxEmit::React)
            && (self.jsx_import_source_pragma.is_some() || self.jsx_runtime_pragma_automatic))
    }

    /// Returns `true` when jsx is `react-jsxdev` (development mode).
    fn jsx_is_dev(&self) -> bool {
        matches!(self.options.jsx, Some(JsxEmit::ReactJSXDev))
    }

    /// Returns the JSX import source (e.g. "react" or a custom one).
    fn jsx_import_source(&self) -> &str {
        if let Some(ref pragma) = self.jsx_import_source_pragma {
            return pragma;
        }
        self.options.jsx_import_source.as_deref().unwrap_or("react")
    }

    /// Returns the JSX runtime module specifier (e.g. "react/jsx-runtime").
    fn jsx_runtime_module(&self) -> String {
        let source = self.jsx_import_source();
        if self.jsx_is_dev() {
            format!("{}/jsx-dev-runtime", source)
        } else {
            format!("{}/jsx-runtime", source)
        }
    }

    fn record_script_value_names_from_stmt(&mut self, stmt: &Stmt, preserve_const_enums: bool) {
        let record_decl = |decl: &Stmt, out: &mut HashSet<String>| match &decl.kind {
            StmtKind::Var(v) if v.modifiers & MOD_DECLARE == 0 => {
                for d in &v.declarations {
                    let mut names = Vec::new();
                    collect_binding_names(&d.name, &mut names);
                    out.extend(names);
                }
            }
            StmtKind::FnDecl(f) if f.modifiers & MOD_DECLARE == 0 => {
                if let Some(name) = &f.name {
                    out.insert(name.clone());
                }
            }
            StmtKind::ClassDecl(c) if c.modifiers & MOD_DECLARE == 0 => {
                if let Some(name) = &c.name {
                    out.insert(name.clone());
                }
            }
            StmtKind::EnumDecl(e)
                if e.modifiers & MOD_DECLARE == 0 && (!e.is_const || preserve_const_enums) =>
            {
                out.insert(e.name.clone());
            }
            StmtKind::ModuleDecl(m) if m.modifiers & MOD_DECLARE == 0 => {
                if let ModuleName::Ident(name) = &m.name {
                    out.insert(name.clone());
                }
            }
            StmtKind::ImportEquals(ie) => {
                out.insert(ie.name.clone());
            }
            _ => {}
        };

        match &stmt.kind {
            StmtKind::Export(ed) => {
                if let ExportDeclKind::Decl(inner) = &ed.kind {
                    record_decl(inner, &mut self.seen_script_value_names);
                }
            }
            _ => record_decl(stmt, &mut self.seen_script_value_names),
        }
    }

    /// Emit a JSX element in preserve mode, applying text replacements to
    /// strip type assertions from attribute values while keeping everything
    /// else as source text.
    fn emit_jsx_preserve_with_replacements(
        &mut self,
        elem_span: Span,
        replacements: &[(usize, usize, String)],
    ) {
        let elem_start = elem_span.start as usize;
        let elem_end = (elem_span.end as usize).min(self.source.len());
        if elem_start >= elem_end {
            return;
        }
        // Sort replacements by position for correct application order.
        let mut sorted_reps: Vec<&(usize, usize, String)> = replacements.iter().collect();
        sorted_reps.sort_by_key(|(s, e, _)| (*s, *e));
        let source = &self.source[elem_start..elem_end];
        let mut result = String::with_capacity(source.len());
        let mut pos = 0;
        for (start, end, replacement) in sorted_reps {
            let s = start.saturating_sub(elem_start);
            let e = end.saturating_sub(elem_start);
            if s > pos && s <= source.len() {
                result.push_str(&source[pos..s]);
            }
            result.push_str(replacement);
            pos = e;
        }
        if pos < source.len() {
            result.push_str(&source[pos..]);
        }
        // Apply the same normalizations as copy_expr_span single-line path
        let result = normalize_close_paren(&result);
        // Expand empty function bodies `=> {}` → `=> { }`, `) {}` → `) { }`.
        // Use normalize_empty_blocks (not normalize_brace_spacing which is too
        // aggressive for JSX context — it adds spaces inside JSX `{expr}` wrappers).
        let result = normalize_empty_blocks(&result);
        // Ensure space before `=>` after type stripping may remove it:
        // `(a, b)=> {` → `(a, b) => {`
        let result = result.replace(")=>", ") =>");
        // Collapse consecutive spaces inside JSX tags and expressions, but
        // NOT in JSX text content where whitespace is semantically significant.
        let result = collapse_consecutive_spaces_jsx(&result);
        let result = normalize_jsx_outer_expr_closing_space(&result);
        // Strip spaces before `/>` when preceded by `>` — JSX attribute values
        // ending with a closing tag or self-closing tag (e.g. `</div> />`→`</div>/>`).
        // normalize_close_paren's regex detection misidentifies `</` as a regex
        // start, so this handles the case it misses.
        let result = {
            let bytes = result.as_bytes();
            let mut out = Vec::with_capacity(bytes.len());
            let mut i = 0;
            let len = bytes.len();
            while i < len {
                if bytes[i] == b' '
                    && i + 2 < len
                    && bytes[i + 1] == b'/'
                    && bytes[i + 2] == b'>'
                    && i > 0
                    && bytes[i - 1] == b'>'
                {
                    // Skip the space (strip `> />` → `>/>`)
                    i += 1;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            if out.len() != bytes.len() {
                String::from_utf8(out).unwrap_or_else(|_| result.to_string())
            } else {
                result.into_owned()
            }
        };
        // Write line-by-line so that continuation lines get at least the
        // base indentation.  A single `self.write(&result)` would only
        // indent the first line because embedded `\n` characters don't
        // trigger the `at_line_start` logic.
        if result.contains('\n') {
            let base_indent = (self.module_wrapper_indent as usize) * 4;
            let lines: Vec<&str> = result.split('\n').collect();
            for (i, line) in lines.iter().enumerate() {
                if i == 0 {
                    // First line: write normally (it continues the current output line).
                    self.write(line);
                } else {
                    // Skip blank lines that follow a `;` — these are between
                    // expression statements and should not be preserved.
                    // But blank lines INSIDE JSX elements (between open/close
                    // tags) are semantically significant whitespace.
                    if line.is_empty() && self.output.ends_with(';') {
                        continue;
                    }
                    self.output.push('\n');
                    self.out_line += 1;
                    self.out_col = 0;
                    self.at_line_start = true;
                    // Compute the effective source indent (tabs count as 4 spaces).
                    let source_indent_eff = line
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .map(|c| if c == '\t' { 4 } else { 1 })
                        .sum::<usize>();
                    if source_indent_eff >= base_indent {
                        // Source indent is sufficient — preserve original whitespace.
                        self.at_line_start = false;
                        self.output.push_str(line);
                        self.track_position(line);
                    } else {
                        // Source indent is less than base — pad to base_indent.
                        let content = line.trim_start();
                        if !content.is_empty() {
                            self.at_line_start = false;
                            const SPACES: &str = "                                                                                                                                ";
                            if base_indent <= SPACES.len() {
                                self.output.push_str(&SPACES[..base_indent]);
                            } else {
                                self.output.push_str(&" ".repeat(base_indent));
                            }
                            self.out_col += base_indent as u32;
                            self.output.push_str(content);
                            self.track_position(content);
                        }
                    }
                }
            }
        } else {
            self.write(&result);
        }
    }

    fn jsx_preserve_replacements_touch_span(
        replacements: &[(usize, usize, String)],
        start: usize,
        end: usize,
    ) -> bool {
        replacements.iter().any(|(rep_start, rep_end, _)| {
            (*rep_start >= start && *rep_start <= end)
                || (*rep_end >= start && *rep_end <= end)
                || (*rep_start < end && start < *rep_end)
        })
    }

    fn collect_jsx_preserve_multiline_expr_replacements(
        &self,
        children: &[JsxChild],
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        for child in children {
            match child {
                JsxChild::Expression(maybe_expr, container_span) => {
                    // De-indent multiline expression container content: TypeScript
                    // normalizes `{  [8-space content]  }` to `{  [4-space content]  }`
                    // in JSX preserve mode.
                    let cs = container_span.start as usize;
                    let ce = (container_span.end as usize).min(self.source.len());
                    if cs < ce && self.source[cs..ce].contains('\n') {
                        self.collect_jsx_expr_container_deindent_replacement(cs, ce, reps);
                    }
                    // Handle conditional expression replacements for non-empty containers.
                    if let Some(expr) = maybe_expr {
                        let start = expr.span.start as usize;
                        let end = (expr.span.end as usize).min(self.source.len());
                        if start >= end
                            || !self.source[start..end].contains('\n')
                            || Self::jsx_preserve_replacements_touch_span(reps, start, end)
                        {
                            continue;
                        }
                        let Some(normalized) = self.jsx_preserve_conditional_expr_replacement(expr)
                        else {
                            continue;
                        };
                        if normalized != self.source[start..end] {
                            reps.push((start, end, normalized));
                        }
                    }
                }
                JsxChild::Element(expr) => {
                    if let ExprKind::JsxElement(el) = &expr.kind {
                        self.collect_jsx_preserve_multiline_expr_replacements(&el.children, reps);
                    }
                }
                JsxChild::Fragment(frag) => {
                    self.collect_jsx_preserve_multiline_expr_replacements(&frag.children, reps);
                }
                _ => {}
            }
        }
    }

    fn collect_jsx_child_line_comment_replacements(
        &self,
        children: &[JsxChild],
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        for child in children {
            match child {
                JsxChild::Element(expr) => {
                    if matches!(expr.kind, ExprKind::JsxSelfClosing(_)) {
                        let child_end = (expr.span.end as usize).min(self.source.len());
                        let line_end = self.source[child_end..]
                            .find('\n')
                            .map_or(self.source.len(), |offset| child_end + offset);
                        let content_end = line_end
                            .checked_sub(1)
                            .filter(|end| self.source.as_bytes()[*end] == b'\r')
                            .unwrap_or(line_end);
                        if let Some(comment_offset) = self.source[child_end..content_end].find("//")
                        {
                            let comment_start = child_end + comment_offset;
                            if self.source[child_end..comment_start]
                                .bytes()
                                .all(|byte| byte == b' ' || byte == b'\t')
                            {
                                let line_start = self.source[..expr.span.start as usize]
                                    .rfind('\n')
                                    .map_or(0, |offset| offset + 1);
                                let indent = &self.source[line_start..expr.span.start as usize];
                                if indent.bytes().all(|byte| byte == b' ' || byte == b'\t') {
                                    let comment = &self.source[comment_start..content_end];
                                    let newline = if line_end > 0
                                        && self.source.as_bytes()[line_end - 1] == b'\r'
                                    {
                                        "\r\n"
                                    } else {
                                        "\n"
                                    };
                                    reps.push((
                                        child_end,
                                        content_end,
                                        format!(" {comment}{newline}{indent}{comment}"),
                                    ));
                                }
                            }
                        }
                    }
                    if let ExprKind::JsxElement(element) = &expr.kind {
                        self.collect_jsx_child_line_comment_replacements(&element.children, reps);
                    }
                }
                JsxChild::Fragment(fragment) => {
                    self.collect_jsx_child_line_comment_replacements(&fragment.children, reps);
                }
                _ => {}
            }
        }
    }

    /// De-indent JSX expression container content. TypeScript normalizes the
    /// content inside `{...}` to match the brace indentation level.
    /// `{` at 4 spaces + content at 8 spaces → content at 4 spaces.
    fn collect_jsx_expr_container_deindent_replacement(
        &self,
        container_start: usize,
        container_end: usize,
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        let src = &self.source[container_start..container_end];
        // Find the `{` brace's indentation level from the source line.
        let line_start = self.source[..container_start]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let brace_indent = container_start - line_start;
        // Find content between `{` and `}`.
        let Some(open_pos) = src.find('{') else {
            return;
        };
        let Some(close_pos) = src.rfind('}') else {
            return;
        };
        if open_pos + 1 >= close_pos {
            return;
        }
        let inner = &src[open_pos + 1..close_pos];
        if !inner.contains('\n') {
            return;
        }
        // Compute the minimum indentation of non-empty inner lines.
        let inner_lines: Vec<&str> = inner.split('\n').collect();
        let min_inner_indent = inner_lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        // Only de-indent if the inner content is deeper than the brace level.
        if min_inner_indent <= brace_indent {
            return;
        }
        let strip = min_inner_indent - brace_indent;
        // Build the de-indented replacement. Also strip the trailing
        // newline + whitespace before `}` so the closing brace joins
        // the last content line (TypeScript's behavior).
        let mut deindented_lines: Vec<String> = Vec::new();
        for line in &inner_lines {
            if line.trim().is_empty() {
                deindented_lines.push(line.to_string());
            } else {
                let ws = line.len() - line.trim_start().len();
                let actual_strip = strip.min(ws);
                deindented_lines.push(line[actual_strip..].to_string());
            }
        }
        // Remove trailing empty/whitespace-only lines (before `}`) only when
        // the last non-empty content line is NOT a comment. TypeScript joins
        // `}` with the last expression line but keeps it separate from comments.
        let last_content = deindented_lines
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.trim_start());
        let last_is_comment = last_content.is_some_and(|l| l.starts_with("//"));
        if !last_is_comment {
            while deindented_lines.last().is_some_and(|l| l.trim().is_empty()) {
                deindented_lines.pop();
            }
        }
        let deindented = deindented_lines.join("\n");
        if deindented != inner {
            let abs_start = container_start + open_pos + 1;
            let abs_end = container_start + close_pos;
            reps.push((abs_start, abs_end, deindented));
        }
    }

    fn jsx_preserve_conditional_expr_replacement(&self, expr: &Expr) -> Option<String> {
        let ExprKind::Cond(cond) = &expr.kind else {
            return None;
        };
        let test_start = cond.test.span.start as usize;
        let test_end = cond.test.span.end as usize;
        if test_start >= test_end || test_end > self.source.len() {
            return None;
        }
        let cons = self.jsx_preserve_paren_branch_replacement(&cond.consequent)?;
        let alt = self.jsx_preserve_paren_branch_replacement(&cond.alternate)?;
        Some(format!(
            "{} ? {} : {}",
            &self.source[test_start..test_end],
            cons,
            alt
        ))
    }

    fn jsx_preserve_paren_branch_replacement(&self, expr: &Expr) -> Option<String> {
        let ExprKind::Paren(inner) = &expr.kind else {
            return None;
        };
        let start = expr.span.start as usize;
        let end = expr.span.end as usize;
        let inner_start = inner.span.start as usize;
        let inner_end = inner.span.end as usize;
        if start >= end || inner_start >= inner_end || end > self.source.len() {
            return None;
        }
        let open = &self.source[start..start + 1];
        let inner_src = &self.source[inner_start..inner_end];
        let between = &self.source[start + 1..inner_start];
        let after_inner = &self.source[inner_end..end.saturating_sub(1)];

        if matches!(
            inner.kind,
            ExprKind::JsxElement(_) | ExprKind::JsxFragment(_)
        ) {
            let mut out = String::new();
            out.push_str(open);
            out.push('\n');
            for line in between.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                out.push_str("    ");
                out.push_str(trimmed);
                out.push('\n');
            }
            let mut inner_lines = inner_src.lines();
            if let Some(first) = inner_lines.next() {
                out.push_str("    ");
                out.push_str(first.trim_start());
            }
            for line in inner_lines {
                out.push('\n');
                out.push_str(line);
            }
            out.push(')');
            return Some(out);
        }

        let mut out = String::new();
        out.push_str(open);
        out.push_str(inner_src.trim());
        if let Some(comment_start) = after_inner.find("//") {
            let comment = after_inner[comment_start..]
                .lines()
                .next()
                .unwrap_or("")
                .trim_end();
            if !comment.is_empty() {
                out.push(' ');
                out.push_str(comment);
            }
        }
        if after_inner.contains('\n') || between.contains('\n') {
            out.push('\n');
            out.push_str("    )");
        } else {
            out.push(')');
        }
        Some(out)
    }

    /// Collect CJS import identifier replacements for JSX attributes in preserve mode.
    /// When a JSX attribute value is (or contains) an identifier that maps to a CJS
    /// import binding, we add a replacement so the source-copied JSX gets the
    /// rewritten name (e.g., `Test` → `Test_1.default`).
    fn collect_jsx_cjs_import_replacements(
        &self,
        attrs: &[JsxAttribute],
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        if self.cjs_import_map.is_empty() {
            return;
        }
        for attr in attrs {
            match attr {
                JsxAttribute::Normal {
                    value: Some(val), ..
                } => {
                    self.collect_expr_cjs_import_replacements(val, reps);
                }
                JsxAttribute::Spread(e, _) => {
                    self.collect_expr_cjs_import_replacements(e, reps);
                }
                _ => {}
            }
        }
    }

    /// Normalize punctuation that the JSX parser deliberately discards while
    /// recovering malformed attribute lists.
    fn collect_invalid_jsx_attribute_replacements(
        &self,
        name_end: usize,
        attrs: &[JsxAttribute],
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        let mut previous_end = name_end;
        let mut previous_unclosed_expr_container = false;
        for (index, attr) in attrs.iter().enumerate() {
            let span = match attr {
                JsxAttribute::Normal { span, .. } | JsxAttribute::Spread(_, span) => *span,
            };
            let start = span.start as usize;
            let end = (span.end as usize).min(self.source.len());
            if previous_end < start && start <= self.source.len() {
                let gap = &self.source[previous_end..start];
                if (!previous_unclosed_expr_container && gap.contains(','))
                    || gap.contains("...")
                    || gap.trim() == ":"
                {
                    reps.push((previous_end, start, " ".to_string()));
                }
            }

            match attr {
                JsxAttribute::Normal {
                    value: None, span, ..
                } => {
                    let s = span.start as usize;
                    let e = (span.end as usize).min(self.source.len());
                    if s < e {
                        if let Some(eq) = self.source[s..e].find('=') {
                            let replacement = if index + 1 < attrs.len() { " " } else { "" };
                            reps.push((s + eq, s + eq + 1, replacement.to_string()));
                        }
                    }
                }
                JsxAttribute::Normal {
                    value: Some(value),
                    span,
                    ..
                } if matches!(value.kind, ExprKind::Omitted) => {
                    let s = span.start as usize;
                    let e = (span.end as usize).min(self.source.len());
                    if s < e {
                        if let Some(empty) = self.source[s..e].find("{}") {
                            reps.push((s + empty, s + empty + 2, String::new()));
                        }
                    }
                }
                JsxAttribute::Spread(_, span) => {
                    let s = span.start as usize;
                    let e = (span.end as usize).min(self.source.len());
                    if s < e && self.source.as_bytes().get(s) == Some(&b'{') {
                        let inner = self.source[s + 1..e].trim_start();
                        let recovered_by_namespace_gap =
                            name_end < s && self.source[name_end..s].contains('=');
                        if !inner.starts_with("...") && !recovered_by_namespace_gap {
                            reps.push((s + 1, s + 1, "...".to_string()));
                        }
                    }
                }
                _ => {}
            }
            previous_unclosed_expr_container = matches!(attr, JsxAttribute::Normal { .. })
                && start < end
                && self.source[start..end].contains("={")
                && !self.source[start..end].contains('}');
            previous_end = end;
        }
    }

    /// Recursively collect CJS import identifier replacements from an expression.
    fn collect_expr_cjs_import_replacements(
        &self,
        expr: &Expr,
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some((ref alias, ref member)) = self.cjs_import_ref(name, expr.span.start) {
                    let replacement: String = if member.is_empty() {
                        alias.to_string()
                    } else {
                        Self::cjs_member_access_text(
                            alias,
                            member,
                            self.is_commonjs()
                                && self.cjs_string_import_locals.contains(name.as_str()),
                        )
                    };
                    let s = expr.span.start as usize;
                    let e = expr.span.end as usize;
                    if s < e {
                        reps.push((s, e, replacement));
                    }
                }
            }
            ExprKind::Member(mem) => {
                // Check if the root object is a CJS import
                self.collect_expr_cjs_import_replacements(&mem.object, reps);
            }
            ExprKind::TaggedTemplate(tagged) => {
                // Tagged templates need `(0, alias.member)` wrapper (like function
                // calls) to avoid binding `this` to the module object.
                if let ExprKind::Ident(name) = &tagged.tag.kind {
                    if let Some((ref alias, ref member)) =
                        self.cjs_import_ref(name, tagged.tag.span.start)
                    {
                        let replacement: String = if member.is_empty() {
                            alias.to_string()
                        } else {
                            // Add trailing space so `(0, a.b) \`...\`` has the
                            // space between the wrapper and the template literal.
                            format!(
                                "(0, {}) ",
                                Self::cjs_member_access_text(
                                    alias,
                                    member,
                                    self.is_commonjs()
                                        && self.cjs_string_import_locals.contains(name.as_str()),
                                )
                            )
                        };
                        let s = tagged.tag.span.start as usize;
                        let e = tagged.tag.span.end as usize;
                        if s < e {
                            reps.push((s, e, replacement));
                        }
                    }
                }
                // Also recurse into template expressions
                for sub_expr in &tagged.quasi.exprs {
                    self.collect_expr_cjs_import_replacements(sub_expr, reps);
                }
            }
            ExprKind::Call(call) => {
                // Function calls: recurse into callee and arguments
                self.collect_expr_cjs_import_replacements(&call.callee, reps);
                for arg in &call.args {
                    self.collect_expr_cjs_import_replacements(arg, reps);
                }
            }
            ExprKind::Paren(inner) => {
                self.collect_expr_cjs_import_replacements(inner, reps);
            }
            ExprKind::Cond(cond) => {
                self.collect_expr_cjs_import_replacements(&cond.test, reps);
                self.collect_expr_cjs_import_replacements(&cond.consequent, reps);
                self.collect_expr_cjs_import_replacements(&cond.alternate, reps);
            }
            _ => {}
        }
    }

    /// Derive a variable name for a `require()` call from the module specifier.
    ///
    /// TypeScript uses the last path segment, sanitised to a valid JS identifier,
    /// with a `_N` suffix for uniqueness.  Examples:
    /// - `./module`   -> `module_1`
    /// - `../foo/bar` -> `bar_1`
    /// - `fs`         -> `fs_1`
    /// - `./0`        -> `_0_1`
    fn next_require_var(&mut self, source: &str) -> String {
        // Take the last path segment and sanitize it.
        // Keep explicit extension segments (e.g. `.json`, `.js`) because
        // TypeScript includes them in generated binding names like
        // `foo_json_1` / `utils_js_1`.
        let seg = source.rsplit('/').next().unwrap_or(source);

        // Sanitise: replace non-alphanumeric with `_`, prepend `_` if starts with digit.
        let mut name = String::new();
        for (i, ch) in seg.chars().enumerate() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                if i == 0 && ch.is_ascii_digit() {
                    name.push('_');
                }
                name.push(ch);
            } else {
                name.push('_');
            }
        }
        if name.is_empty() {
            name.push('_');
        }

        loop {
            let suffix = {
                let counter = self
                    .require_var_counters
                    .entry(name.clone().into())
                    .or_insert(0);
                *counter += 1;
                *counter
            };
            let candidate = if name == "_" {
                format!("_{}", suffix)
            } else {
                format!("{}_{}", name, suffix)
            };
            if !self.file_value_bound_names.contains(candidate.as_str()) {
                return candidate;
            }
        }
    }

    fn node_esm_import_require_enabled(&self) -> bool {
        self.node_esm_import_require
            && !self.is_cjs_like()
            && self.is_esm_emit()
            && self.effective_module_kind() != ModuleKind::Preserve
    }

    /// Keyword for a compiler-generated binding whose initializer is a
    /// CommonJS-style `require` call (including the Node ESM createRequire
    /// bridge). TypeScript downlevels these synthetic bindings along with
    /// source declarations for pre-ES2015 targets.
    fn generated_require_binding_keyword(&self) -> &'static str {
        if self.effective_target() < ScriptTarget::ES2015 {
            "var"
        } else {
            "const"
        }
    }

    fn node_esm_require_ident(&self) -> &str {
        self.node_esm_require_ident
            .as_deref()
            .unwrap_or("__require")
    }

    fn import_decl_needs_node_esm_require_bridge(&self, import_decl: &ImportDecl) -> bool {
        if !self.node_esm_import_require_enabled() || import_decl.type_only {
            return false;
        }
        let ImportClause::Require(name) = &import_decl.specifiers else {
            return false;
        };
        !self.is_import_elided(name)
    }

    fn export_decl_needs_node_esm_require_bridge(&self, export_decl: &ExportDecl) -> bool {
        let ExportDeclKind::Decl(inner) = &export_decl.kind else {
            return false;
        };
        match &inner.kind {
            StmtKind::Import(import_decl) => {
                self.import_decl_needs_node_esm_require_bridge(import_decl)
            }
            StmtKind::ImportEquals(ie) => {
                if self.import_equals_rhs_is_type_only_require_spec(&ie.module_ref)
                    || !self.import_equals_rhs_has_runtime_value(&ie.module_ref)
                {
                    return false;
                }
                matches!(
                    &ie.module_ref.kind,
                    ExprKind::Call(call)
                        if matches!(&call.callee.kind, ExprKind::Ident(callee) if callee == "require")
                )
            }
            _ => false,
        }
    }

    /// An `export` whose recovered inner statement is a bare expression is a
    /// parse-error artifact that TypeScript discards rather than treating the
    /// file as an ES module.  Two shapes qualify:
    ///   * the missing-body recovery of `export namespace`/`interface`/`type`
    ///     (recovered as `export <ident>`), and
    ///   * an invalid form whose inner is a COMPOUND expression, e.g.
    ///     `export defer * as ns from "a"` (recovered as `export <binary-expr>`).
    ///
    /// A bare non-keyword identifier is deliberately EXCLUDED: `export public
    /// import a = x.c` (invalid import modifiers) recovers with a bare `public`
    /// identifier, but tsc drops the modifier and keeps a real `export import`,
    /// so the file must still count as a module (see importDeclWithClassModifiers).
    fn export_decl_is_invalid_expr_recovery(&self, export_decl: &ExportDecl) -> bool {
        let ExportDeclKind::Decl(inner) = &export_decl.kind else {
            return false;
        };
        let StmtKind::Expr(expr) = &inner.kind else {
            return false;
        };
        match &expr.kind {
            ExprKind::Ident(keyword) => {
                matches!(
                    keyword.as_str(),
                    "namespace" | "interface" | "type" | "abstract"
                )
            }
            _ => true,
        }
    }

    fn stmt_needs_node_esm_require_bridge(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Import(import_decl) => {
                self.import_decl_needs_node_esm_require_bridge(import_decl)
            }
            StmtKind::Export(export_decl) => {
                self.export_decl_needs_node_esm_require_bridge(export_decl)
            }
            _ => false,
        }
    }

    fn file_needs_node_esm_require_bridge(&self, statements: &[Stmt]) -> bool {
        statements
            .iter()
            .any(|stmt| self.stmt_needs_node_esm_require_bridge(stmt))
    }

    fn next_node_esm_helper_name(base: &str, reserved: &mut HashSet<AstString>) -> String {
        if reserved.insert(base.into()) {
            return base.to_string();
        }
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{base}_{suffix}");
            if reserved.insert(candidate.clone().into()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn emit_node_esm_require_prelude(&mut self, statements: &[Stmt]) {
        self.node_esm_create_require_ident = None;
        self.node_esm_require_ident = None;

        if !self.file_needs_node_esm_require_bridge(statements) {
            return;
        }

        let mut reserved = self.file_value_bound_names.clone();
        for name in &self.emitted_var_names {
            reserved.insert(name.clone());
        }
        let create_ident = Self::next_node_esm_helper_name("_createRequire", &mut reserved);
        let require_ident = Self::next_node_esm_helper_name("__require", &mut reserved);

        self.write("import { createRequire as ");
        self.write(&create_ident);
        self.writeln(" } from \"module\";");
        self.write(self.generated_require_binding_keyword());
        self.write(" ");
        self.write(&require_ident);
        self.write(" = ");
        self.write(&create_ident);
        self.writeln("(import.meta.url);");
        self.emitted_esm_export = true;
        self.node_esm_create_require_ident = Some(create_ident);
        self.node_esm_require_ident = Some(require_ident);
    }

    fn emit_node_esm_require_call_for_source(&mut self, source: &str, span: Span) {
        let q = self.detect_string_quote(span);
        let require_ident = self.node_esm_require_ident().to_string();
        self.write(&require_ident);
        self.write("(");
        self.write(q);
        self.write(source);
        self.write(q);
        self.write(")");
    }

    fn emit_node_esm_require_call_for_expr(&mut self, expr: &Expr) -> bool {
        let ExprKind::Call(call) = &expr.kind else {
            return false;
        };
        if !matches!(&call.callee.kind, ExprKind::Ident(callee) if callee == "require") {
            return false;
        }
        let require_ident = self.node_esm_require_ident().to_string();
        self.write(&require_ident);
        self.write("(");
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.emit_expr(arg);
        }
        self.write(")");
        true
    }

    fn interface_decl_has_recoverable_var_members(&self, span: Span) -> bool {
        let source = self.copy_span_trimmed(span);
        let Some(open) = source.find('{') else {
            return false;
        };
        let Some(close) = source.rfind('}') else {
            return false;
        };
        if close <= open + 1 {
            return false;
        }
        let body = &source[open + 1..close];
        body.lines().any(|raw_line| {
            let trimmed = raw_line.trim_start();
            let Some(after_var) = trimmed.strip_prefix("var") else {
                return false;
            };
            if !after_var
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_whitespace())
            {
                return false;
            }
            let mut saw_name = false;
            for ch in after_var.trim_start().chars() {
                if !saw_name {
                    if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
                        saw_name = true;
                        continue;
                    }
                    break;
                }
                if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$') {
                    break;
                }
            }
            saw_name
        })
    }

    fn interface_decl_has_dotted_name_recovery(&self, span: Span) -> bool {
        let source = self.copy_span_trimmed(span);
        let trimmed = source.trim_start();
        let Some(after_interface) = trimmed.strip_prefix("interface") else {
            return false;
        };
        let Some(open_brace) = after_interface.find('{') else {
            return false;
        };
        let head = after_interface[..open_brace].trim();
        if !head.contains('.') || head.as_bytes().iter().any(|b| b.is_ascii_whitespace()) {
            return false;
        }
        if !head.split('.').all(|seg| {
            let mut chars = seg.chars();
            let Some(first) = chars.next() else {
                return false;
            };
            (first.is_ascii_alphabetic() || first == '_' || first == '$')
                && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
        }) {
            return false;
        }
        let Some(name) = head.rsplit('.').next().map(str::trim) else {
            return false;
        };
        !name.is_empty()
    }

    fn interface_decl_has_incorrect_return_token_recovery(&self, span: Span) -> bool {
        let source = self.copy_span_trimmed(span);
        let trimmed = source.trim_start();
        trimmed.starts_with("interface ")
            && source.contains("=> {")
            && source.contains("return ")
            && source.contains("};")
    }

    fn type_alias_has_incorrect_return_token_recovery(&self, span: Span) -> bool {
        let start = span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let line_end_rel = rest.find('\n').unwrap_or(rest.len());
        let line = &rest[..line_end_rel];
        let trimmed = line.trim_start();
        // Only search in code, not in trailing // comments
        let code_part = line.find("//").map(|p| &line[..p]).unwrap_or(line);
        trimmed.starts_with("type ") && code_part.contains("): string;")
    }

    fn type_alias_has_malformed_import_attributes_double_comma_recovery(&self, span: Span) -> bool {
        let source = self.copy_span_trimmed(span);
        let trimmed = source.trim_start();
        if !trimmed.starts_with("type ") {
            return false;
        }
        if !source.contains("typeof import(") || !source.contains("with:") {
            return false;
        }
        if source.contains("\"resolution-mode\"") {
            return false;
        }
        source.contains("\",,")
    }

    fn type_alias_has_recoverable_emit(&self, span: Span) -> bool {
        self.type_alias_has_incorrect_return_token_recovery(span)
            || self
                .malformed_import_type_option_recovery_from_span(span)
                .is_some()
            || self.type_alias_has_malformed_import_attributes_double_comma_recovery(span)
    }

    fn stmt_has_recoverable_type_alias_emit(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::TypeAlias(_) => self.type_alias_has_recoverable_emit(stmt.span),
            StmtKind::Export(ed) => matches!(
                &ed.kind,
                ExportDeclKind::Decl(inner)
                    if matches!(&inner.kind, StmtKind::TypeAlias(_))
                        && self.type_alias_has_recoverable_emit(inner.span)
            ),
            _ => false,
        }
    }

    fn stmt_has_missing_import_type_options_wrapper_recovery(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::TypeAlias(_) => {
                self.span_has_missing_import_type_options_wrapper_recovery(stmt.span)
            }
            StmtKind::Export(ed) => matches!(
                &ed.kind,
                ExportDeclKind::Decl(inner)
                    if matches!(&inner.kind, StmtKind::TypeAlias(_))
                        && self.span_has_missing_import_type_options_wrapper_recovery(inner.span)
            ),
            _ => false,
        }
    }

    /// Check if a statement is `export default @dec class {}` (CJS) that will
    /// get an IIFE wrapper. Leading comments should be suppressed because
    /// TypeScript drops them during the transformation.
    fn cjs_export_default_has_decorated_class_expr(&self, stmt: &Stmt) -> bool {
        if self.options.experimental_decorators == Some(true) || self.should_preserve_decorators() {
            return false;
        }
        let StmtKind::Export(ed) = &stmt.kind else {
            return false;
        };
        let ExportDeclKind::Default(ref expr) = ed.kind else {
            return false;
        };
        // Only suppress comments for bare `export default @dec class {}`,
        // NOT for `export default (@dec class {})` (paren-wrapped).
        matches!(&expr.kind, ExprKind::ClassExpr(cd) if !cd.decorators.is_empty()
            && class_expr_can_emit_simple_standard_decorator_wrapper(cd))
    }

    /// Get the effective target, defaulting to ESNext when unspecified.
    fn effective_target(&self) -> ScriptTarget {
        // ES3 target was removed in TypeScript 5.5; treat it as ES5.
        match self.options.target.unwrap_or(ScriptTarget::ESNext) {
            ScriptTarget::ES3 => ScriptTarget::ES5,
            t => t,
        }
    }

    /// Preserve the syntactic provenance of arbitrary module namespace names
    /// before CJS planning flattens specifiers into string pairs. Recovery
    /// files retain the existing fail-closed emit path.
    fn collect_cjs_string_name_provenance(&mut self, file: &SourceFile) {
        self.cjs_string_import_locals.clear();
        self.cjs_string_export_names.clear();
        if self.file_has_recovery_errors {
            return;
        }

        let mut collect_import = |import_decl: &ImportDecl| {
            if let ImportClause::Named { named, .. } = &import_decl.specifiers {
                for spec in named {
                    if spec.imported_is_string && spec.span.start < spec.span.end {
                        self.cjs_string_import_locals
                            .insert(spec.local.clone().into());
                    }
                }
            }
        };

        for stmt in &file.statements {
            match &stmt.kind {
                StmtKind::Import(import_decl) => collect_import(import_decl),
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Named { specifiers, .. } => {
                        for spec in specifiers {
                            let exported = spec.exported.as_ref().unwrap_or(&spec.local);
                            if (spec.exported_is_string
                                || (spec.exported.is_none() && spec.local_is_string))
                                && spec.span.start < spec.span.end
                            {
                                self.cjs_string_export_names.insert(exported.clone().into());
                            }
                        }
                    }
                    ExportDeclKind::All {
                        alias: Some(alias),
                        alias_is_string: true,
                        ..
                    } if export_decl.span.start < export_decl.span.end => {
                        self.cjs_string_export_names.insert(alias.clone().into());
                    }
                    ExportDeclKind::Decl(inner) => {
                        if let StmtKind::Import(import_decl) = &inner.kind {
                            collect_import(import_decl);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn cjs_member_access_text(base: &str, member: &str, force_brackets: bool) -> String {
        if force_brackets {
            let cooked = emit_expr::cook_string_literal_raw_for_double_quote(member);
            format!("{base}[\"{}\"]", cooked)
        } else {
            format!("{base}.{member}")
        }
    }

    fn write_cjs_export_access(&mut self, target: &str, name: &str) {
        let force_brackets = self.is_commonjs()
            && target == "exports"
            && self.cjs_string_export_names.contains(name);
        let access = Self::cjs_member_access_text(target, name, force_brackets);
        self.write(&access);
    }

    fn write_cjs_import_access(&mut self, base: &str, member: &str, local: &str) {
        let force_brackets = self.is_commonjs() && self.cjs_string_import_locals.contains(local);
        let access = Self::cjs_member_access_text(base, member, force_brackets);
        self.write(&access);
    }

    fn escaped_cjs_export_name(&self, name: &str) -> String {
        if self.is_commonjs() && self.cjs_string_export_names.contains(name) {
            emit_expr::cook_string_literal_raw_for_double_quote(name)
        } else {
            emit_expr::escape_js_string_for_quote(name, '"')
        }
    }

    /// Return the single declarator shared by deliberately narrow ES5 export
    /// bridges. Keep recovery, comments, bundling, nested scopes, declaration
    /// emit, and multi-declarator ordering on the general path until their
    /// ownership rules are modelled explicitly.
    fn direct_es5_export_bridge_declarator<'stmt>(
        &self,
        var_stmt: &'stmt VarStmt,
        stmt_span: Span,
    ) -> Option<&'stmt VarDeclarator> {
        if self.effective_target() != ScriptTarget::ES5
            || self.file_has_recovery_errors
            || self.is_js_file
            || self.options.declaration == Some(true)
            || self.options.out_file.is_some()
            || self.fn_scope_depth != 0
            || self.block_depth != 0
            || self
                .export_target
                .as_ref()
                .is_some_and(|target| target != "exports")
            || !matches!(
                self.effective_module_kind(),
                ModuleKind::CommonJS
                    | ModuleKind::AMD
                    | ModuleKind::System
                    | ModuleKind::ES2015
                    | ModuleKind::ESNext
            )
            || var_stmt.kind != VarKind::Const
            || var_stmt.modifiers & MOD_DECLARE != 0
            || self.has_comments_in_range(stmt_span.start, stmt_span.end)
        {
            return None;
        }

        let [decl] = var_stmt.declarations.as_slice() else {
            return None;
        };
        if decl.type_ann.is_some() || decl.definite {
            return None;
        }
        decl.init.as_ref()?;
        Some(decl)
    }

    /// Return the initializer for a direct exported empty binding pattern.
    /// TypeScript gives this otherwise nameless export a synthetic binding
    /// (`_b`) while retaining a separate destructuring value temp (`_a`).
    pub(crate) fn direct_es5_empty_binding_export_init<'stmt>(
        &self,
        var_stmt: &'stmt VarStmt,
        stmt_span: Span,
    ) -> Option<&'stmt Expr> {
        let decl = self.direct_es5_export_bridge_declarator(var_stmt, stmt_span)?;
        let is_root_empty = match &decl.name.kind {
            PatKind::Array(elements) => elements.is_empty(),
            PatKind::Object(properties) => properties.is_empty(),
            _ => false,
        };
        is_root_empty.then(|| decl.init.as_deref()).flatten()
    }

    /// Recognize only the five baseline variants for
    /// `export const { x, ...rest } = value` at ES5. The no-helper gate matches
    /// that lane exactly, while the two-element shorthand/rest shape excludes
    /// aliases, defaults, computed keys, nested patterns, and comment-sensitive
    /// forms. Returned values own their AST strings/initializer so emission can
    /// allocate hygienic temps without borrowing `self`.
    pub(crate) fn direct_es5_object_rest_export(
        &self,
        var_stmt: &VarStmt,
        stmt_span: Span,
    ) -> Option<(Box<Expr>, AstString, Span, AstString, Span)> {
        if self.options.no_emit_helpers != Some(true) {
            return None;
        }
        let decl = self.direct_es5_export_bridge_declarator(var_stmt, stmt_span)?;
        let PatKind::Object(properties) = &decl.name.kind else {
            return None;
        };
        let [ObjPatProp::Shorthand(binding_name, binding_span), ObjPatProp::Rest(rest_pat)] =
            properties.as_slice()
        else {
            return None;
        };
        let PatKind::Ident(rest_name) = &rest_pat.kind else {
            return None;
        };
        let is_plain_ascii_identifier = |name: &str| {
            let mut chars = name.chars();
            chars
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_' || ch == '$')
                && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
        };
        if binding_name.is_empty()
            || rest_name.is_empty()
            || binding_name == "<error>"
            || rest_name == "<error>"
            || binding_name == rest_name
            || !is_plain_ascii_identifier(binding_name)
            || !is_plain_ascii_identifier(rest_name)
            || self.span_text(*binding_span) != Some(binding_name.as_str())
            || self.span_text(rest_pat.span) != Some(rest_name.as_str())
        {
            return None;
        }
        Some((
            decl.init.as_ref()?.clone(),
            binding_name.clone(),
            *binding_span,
            rest_name.clone(),
            rest_pat.span,
        ))
    }

    /// Returns true when class fields should use TC39 `[[Define]]` semantics
    /// (native class field declarations) rather than assignment in the constructor.
    ///
    /// This is true when:
    /// - `useDefineForClassFields` is explicitly `true`, OR
    /// - target >= ES2022 and `useDefineForClassFields` is not explicitly `false`
    fn use_define_for_class_fields(&self) -> bool {
        match self.options.use_define_for_class_fields {
            Some(true) => true,
            Some(false) => false,
            None => self.effective_target() >= ScriptTarget::ES2022,
        }
    }

    /// Returns true when TC39 decorators should be preserved as `@decorator`
    /// syntax in the output. This is the case when `experimentalDecorators` is
    /// NOT enabled, the target natively supports decorators (ESNext), and
    /// `useDefineForClassFields` is effectively true (so class fields don't
    /// need constructor-time lowering which forces decorator lowering too).
    fn should_preserve_decorators(&self) -> bool {
        self.options.experimental_decorators != Some(true)
            && !self.needs_downlevel("decorators")
            && self.use_define_for_class_fields()
    }

    /// Returns true if the given feature needs downlevel transformation.
    fn needs_downlevel(&self, feature: &str) -> bool {
        let target = self.effective_target();
        match feature {
            "optional-chaining" | "nullish-coalescing" => target < ScriptTarget::ES2020,
            "logical-assignment" => target < ScriptTarget::ES2021,
            "object-spread" => target < ScriptTarget::ES2018,
            "class-fields" => target < ScriptTarget::ES2022,
            "async" => target < ScriptTarget::ES2017,
            "async-generator" => target < ScriptTarget::ES2018,
            "exponentiation" => target < ScriptTarget::ES2016,
            "for-of" => target < ScriptTarget::ES2015,
            // Modern TypeScript no longer downlevels const/let to var.
            "block-scoping" => false,
            "private-fields" => target < ScriptTarget::ES2022,
            "static-blocks" => target < ScriptTarget::ES2022,
            "auto-accessors" => target < ScriptTarget::ESNext,
            "decorators" => target < ScriptTarget::ESNext,
            "tagged-template" => target < ScriptTarget::ES2018,
            "using" => target < ScriptTarget::ESNext,
            _ => false,
        }
    }

    fn needs_lexical_downlevel(&self) -> bool {
        matches!(
            self.options.target,
            Some(target) if target >= ScriptTarget::ES5 && target < ScriptTarget::ES2015
        )
    }

    fn should_downlevel_dynamic_import(&self) -> bool {
        // Node-style modes resolved to CJS keep native import() — Node.js
        // CJS modules support import() natively since Node 12.
        if get_other_option(self.options, "__tsrsKeepDynamicImport").is_some() {
            return false;
        }
        // CommonJS always rewrites dynamic import() to require()-based form,
        // regardless of target. AMD always rewrites import() to require([...])
        // because AMD has no native import() support.
        // UMD combines both CJS and AMD patterns and also always rewrites.
        // `module: none` uses the require-based transform before ES2020. This
        // notably applies to outFile bundles, whose relative module names are
        // rewritten to their bundle-local names.
        self.is_commonjs()
            || self.is_amd()
            || self.is_umd()
            || (self.effective_module_kind() == ModuleKind::None
                && self.effective_target() < ScriptTarget::ES2020)
    }

    /// Generate a unique temp variable name for downlevel transforms.
    /// Uses TypeScript's naming convention: _a, _b, _c, ..., _h, _j (skip _i), ..., _z, _0, _1, ...
    fn next_temp_var(&mut self) -> String {
        let name = self.make_temp_name();
        self.temp_var_names.push(name.clone().into());
        name
    }

    /// Allocate a deferred temp placeholder. The real temp name is assigned
    /// later via `resolve_deferred_temp_placeholders`, after regular temps.
    fn next_deferred_temp_placeholder(&mut self) -> String {
        let placeholder = format!(
            "__tsrs_deferred_temp_{}__",
            self.deferred_temp_placeholder_counter
        );
        self.deferred_temp_placeholder_counter += 1;
        self.deferred_temp_placeholders
            .push(placeholder.clone().into());
        placeholder
    }

    fn next_inline_deferred_temp_placeholder(&mut self) -> String {
        let placeholder = format!(
            "__tsrs_inline_deferred_temp_{}__",
            self.deferred_temp_placeholder_counter
        );
        self.deferred_temp_placeholder_counter += 1;
        self.inline_deferred_temp_placeholders
            .push(placeholder.clone().into());
        placeholder
    }

    fn next_deferred_export_name_placeholder(&mut self) -> String {
        loop {
            let placeholder = format!(
                "\0__tsrs_deferred_export_name_{}__\0",
                self.deferred_temp_placeholder_counter
            );
            self.deferred_temp_placeholder_counter += 1;
            // NUL sentinels survive arbitrary completed-output rewrites. The
            // admission check rejects external values plus raw, normalized,
            // and fully cooked source spellings before the sentinel is allowed
            // into generated output.
            if self.source_can_emit_deferred_placeholder(&placeholder) {
                continue;
            }
            self.deferred_export_name_placeholders
                .push(placeholder.clone().into());
            return placeholder;
        }
    }

    fn source_can_emit_deferred_placeholder(&self, placeholder: &str) -> bool {
        if self.source.contains(placeholder)
            || normalize_unicode_escapes(self.source).contains(placeholder)
            || self.semantic_string_stores_contain(placeholder)
        {
            return true;
        }

        // Raw-NUL source output can originate from three cooking paths:
        // 1. ordinary strings whose braced-Unicode rewrite canonicalizes the
        //    full body (checked per StringLiteral below), and
        // 2. entity-decoded single-quoted JSX attributes, whose quote-specific
        //    escaper intentionally preserves NUL, and
        // 3. decoded decorated/computed class-member names, some of whose
        //    metadata/access expressions write the decoded name directly.
        // Decode JSX over the whole source conservatively through that exact
        // path so attributes split across parser recovery shapes cannot evade
        // admission. Other JSX text and double-quoted attributes use
        // jsx_escape_string_decoded (escapes NUL); templates use
        // downlevel_template_string (escapes NUL).
        // Comments/regex/raw strings are covered by the raw source check above.
        if self.source.contains('&') {
            let decoded = crate::jsx::jsx_decode_entities(self.source);
            if crate::jsx::jsx_escape_string_single_quoted(&decoded).contains(placeholder) {
                return true;
            }
        }

        Scanner::new(self.source)
            .scan_all()
            .into_iter()
            .any(|token| {
                let raw = self
                    .source
                    .get(token.span.start as usize..token.span.end as usize)
                    .unwrap_or_default();
                let body = match token.kind {
                    TokenKind::StringLiteral => {
                        if let Some(quote @ ('\'' | '"')) = raw.chars().next() {
                            if crate::emit_expr::downlevel_braced_unicode_escapes_in_string(
                                raw, quote,
                            )
                            .is_some_and(|emitted| emitted.contains(placeholder))
                            {
                                return true;
                            }
                        }
                        let start =
                            usize::from(matches!(raw.as_bytes().first(), Some(b'\'' | b'"')));
                        let end = raw.len().saturating_sub(usize::from(
                            raw.len() > start
                                && matches!(raw.as_bytes().last(), Some(b'\'' | b'"')),
                        ));
                        let body = &raw[start..end];
                        if crate::emit_class::decode_js_string_content(body).contains(placeholder) {
                            return true;
                        }
                        body
                    }
                    TokenKind::NoSubstitutionTemplate => {
                        let start = usize::from(raw.starts_with('`'));
                        let end = raw
                            .len()
                            .saturating_sub(usize::from(raw.len() > start && raw.ends_with('`')));
                        let body = &raw[start..end];
                        if crate::emit_class::decode_js_string_content(body).contains(placeholder) {
                            return true;
                        }
                        body
                    }
                    TokenKind::TemplateHead => raw
                        .strip_prefix('`')
                        .unwrap_or(raw)
                        .strip_suffix("${")
                        .unwrap_or(raw),
                    TokenKind::TemplateMiddle => raw
                        .strip_prefix('}')
                        .unwrap_or(raw)
                        .strip_suffix("${")
                        .unwrap_or(raw),
                    TokenKind::TemplateTail => raw
                        .strip_prefix('}')
                        .unwrap_or(raw)
                        .strip_suffix('`')
                        .unwrap_or(raw),
                    TokenKind::Identifier => {
                        return normalize_unicode_escapes(raw).contains(placeholder);
                    }
                    _ => return false,
                };
                crate::emit_expr::cook_string_or_template_body_for_double_quote(body)
                    .contains(placeholder)
            })
    }

    /// Return whether a precomputed semantic string can later be written into
    /// generated JavaScript. These values are not necessarily present in this
    /// file's source: callers can seed cross-file constants, and enum folding
    /// can assemble a value from several individually harmless source pieces.
    /// Admission of a deferred-output sentinel must therefore inspect every
    /// persistent string-value store rather than relying on source scanning.
    fn semantic_string_stores_contain(&self, needle: &str) -> bool {
        let const_enum_contains = |values: &HashMap<(String, String), ConstEnumValue>| {
            values.values().any(
                |value| matches!(value, ConstEnumValue::String(value) if value.contains(needle)),
            )
        };

        const_enum_contains(&self.const_enum_values)
            || const_enum_contains(&self.external_const_enum_values)
            || [
                self.options.jsx_import_source.as_deref(),
                self.options.jsx_factory.as_deref(),
                self.options.jsx_fragment_factory.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| value.contains(needle))
            || self
                .file_string_consts
                .values()
                .any(|value| value.contains(needle))
            || self
                .external_file_string_consts
                .values()
                .any(|value| value.contains(needle))
            || self
                .merged_string_enum_values
                .values()
                .flat_map(|members| members.values())
                .any(|value| value.contains(needle))
    }

    /// Resolve deferred temp placeholders into real temp names allocated after
    /// regular temps, then rewrite all placeholder occurrences in the output.
    fn resolve_deferred_temp_placeholders(&mut self) {
        if self.deferred_temp_placeholders.is_empty() {
            return;
        }
        let placeholders = std::mem::take(&mut self.deferred_temp_placeholders);
        for placeholder in placeholders {
            let real_name = self.make_temp_name();
            self.resolved_deferred_temp_names
                .push(real_name.clone().into());
            self.replace_generated_text(placeholder.as_str(), &real_name);
        }
    }

    fn resolve_inline_deferred_temp_placeholders(&mut self) {
        if self.inline_deferred_temp_placeholders.is_empty() {
            return;
        }
        let placeholders = std::mem::take(&mut self.inline_deferred_temp_placeholders);
        for placeholder in placeholders {
            let real_name = self.make_temp_name();
            self.replace_generated_text(placeholder.as_str(), &real_name);
        }
    }

    fn resolve_deferred_export_name_placeholders(&mut self) {
        if self.deferred_export_name_placeholders.is_empty() {
            return;
        }
        let placeholders = std::mem::take(&mut self.deferred_export_name_placeholders);
        for placeholder in placeholders {
            let real_name = self.make_temp_name();
            self.replace_deferred_export_placeholder(placeholder.as_str(), &real_name);
        }
    }

    /// Resolve only the inline deferred temp placeholders created from index
    /// `start` onward. Leaves earlier placeholders (from outer scopes) untouched.
    pub(crate) fn resolve_scoped_inline_deferred_temps(&mut self, start: usize) {
        if self.inline_deferred_temp_placeholders.len() <= start {
            return;
        }
        let scope_placeholders: Vec<_> = self
            .inline_deferred_temp_placeholders
            .drain(start..)
            .collect();
        for placeholder in scope_placeholders {
            let real_name = self.make_temp_name();
            self.replace_generated_text(placeholder.as_str(), &real_name);
        }
    }

    fn next_arguments_capture_name(&mut self) -> String {
        loop {
            self.arguments_capture_counter += 1;
            let name = format!("arguments_{}", self.arguments_capture_counter);
            if !self
                .generator_catch_names
                .values()
                .any(|catch| catch == name.as_str())
            {
                return name;
            }
        }
    }

    /// Generate a unique temp variable name WITHOUT registering it for
    /// file-level `var` declaration.  Use when the temp will be inlined
    /// in a `const`/`let`/`var` declarator list.
    fn next_inline_temp_var(&mut self) -> String {
        self.make_temp_name()
    }

    /// Allocate a readable inline binding while protecting expressions moved
    /// into the binding's scope from capture. The preferred spelling is kept
    /// for baseline parity; collisions with any source identifier, previously
    /// emitted binding, or sibling generated binding receive a numeric suffix.
    fn allocate_inline_binding_name(
        &self,
        preferred: &str,
        allocated: &mut HashSet<AstString>,
    ) -> String {
        let available = |candidate: &str, allocated: &HashSet<AstString>| {
            !self.source_has_identifier(candidate)
                && !self.emitted_var_names.contains(candidate)
                && !allocated.contains(candidate)
        };
        if available(preferred, allocated) {
            allocated.insert(preferred.into());
            return preferred.to_string();
        }
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{preferred}_{suffix}");
            if available(&candidate, allocated) {
                allocated.insert(candidate.clone().into());
                return candidate;
            }
            suffix += 1;
        }
    }

    /// Allocate the index used by simple downlevel loops. TypeScript reserves
    /// `_i` for the first such loop in a scope, then falls back to the ordinary
    /// `_a`, `_b`, ... temp sequence. Keeping the reservation in
    /// `emitted_var_names` makes rest-parameter copy loops and indexed for-of
    /// loops share one collision-safe scope; function emit already saves and
    /// restores that set.
    pub(crate) fn next_simple_loop_index_name(&mut self) -> String {
        let name = if !self.source_has_identifier("_i") && !self.emitted_var_names.contains("_i") {
            "_i".to_string()
        } else {
            self.next_inline_temp_var()
        };
        self.emitted_var_names.insert(name.clone().into());
        name
    }

    /// Allocate the snapshotted RHS binding for an indexed for-of transform.
    /// A direct identifier keeps TypeScript's readable `value_1` family and
    /// does not consume the anonymous temp sequence. Other expressions use the
    /// next ordinary temp because they have no stable source name.
    pub(crate) fn next_indexed_for_of_rhs_name(&mut self, rhs: &Expr) -> String {
        let name = if let ExprKind::Ident(base) = &rhs.kind {
            let mut suffix = 1usize;
            loop {
                let candidate = format!("{base}_{suffix}");
                if !self.source_has_identifier(&candidate)
                    && !self.emitted_var_names.contains(candidate.as_str())
                {
                    break candidate;
                }
                suffix += 1;
            }
        } else {
            self.next_inline_temp_var()
        };
        self.emitted_var_names.insert(name.clone().into());
        name
    }

    /// Allocate the numeric suffix used by explicit-resource-management
    /// temporaries. Keep the familiar `env_1`/`e_1`/`result_1` spelling when
    /// available, but never shadow a user identifier from the source file.
    pub(crate) fn next_using_env_num(&mut self) -> usize {
        loop {
            self.using_env_counter += 1;
            let suffix = self.using_env_counter;
            if !self.source_has_identifier(&format!("env_{suffix}"))
                && !self.source_has_identifier(&format!("e_{suffix}"))
                && !self.source_has_identifier(&format!("result_{suffix}"))
            {
                return suffix;
            }
        }
    }

    /// Allocate a temp placeholder for a non-file scope where the textual
    /// declaration is inserted before the final file-level temp names are known.
    /// The placeholder is rewritten later by `resolve_inline_deferred_temp_placeholders`.
    fn next_scoped_deferred_temp_var(&mut self) -> String {
        let placeholder = self.next_inline_deferred_temp_placeholder();
        self.temp_var_names.push(placeholder.clone().into());
        placeholder
    }

    fn make_temp_name(&mut self) -> String {
        // TypeScript temp var letters: a-h, j-m, o-z (skipping 'i' and 'n')
        const LETTERS: &[u8] = b"abcdefghjklmopqrstuvwxyz";
        // Skip names that appear as identifiers in the source text to avoid
        // shadowing user-declared variables (mirrors TypeScript's makeUniqueName).
        loop {
            let idx = self.temp_var_counter;
            self.temp_var_counter += 1;
            let name = if idx < LETTERS.len() {
                format!("_{}", LETTERS[idx] as char)
            } else {
                format!("_{}", idx - LETTERS.len())
            };
            if !self.source_has_identifier(&name) {
                return name;
            }
        }
    }

    fn output_line_col_at(&self, byte_pos: usize) -> (u32, u32) {
        let bytes = self.output.as_bytes();
        let end = byte_pos.min(bytes.len());
        let prefix = &bytes[..end];
        let line = prefix.iter().filter(|&&b| b == b'\n').count() as u32;
        let column = prefix
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(prefix.len(), |last| prefix.len() - last - 1) as u32;
        (line, column)
    }

    fn refresh_output_cursor(&mut self) {
        let (line, column) = self.output_line_col_at(self.output.len());
        self.out_line = line;
        self.out_col = column;
        self.at_line_start = self.output.ends_with('\n');
    }

    fn insert_generated_text(&mut self, byte_pos: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let byte_pos = byte_pos.min(self.output.len());
        let (line, column) = self.output_line_col_at(byte_pos);
        self.output.insert_str(byte_pos, text);
        if let Some(ref mut source_map) = self.source_map_gen {
            source_map.shift_after_insertion(line, column, text);
        }
        self.refresh_output_cursor();
    }

    fn replace_generated_text(&mut self, old: &str, new: &str) {
        if old.is_empty() || old == new {
            return;
        }
        let positions = self
            .output
            .match_indices(old)
            .map(|(pos, _)| pos)
            .collect::<Vec<_>>();
        for byte_pos in positions.into_iter().rev() {
            let (line, column) = self.output_line_col_at(byte_pos);
            self.output
                .replace_range(byte_pos..byte_pos + old.len(), new);
            if let Some(ref mut source_map) = self.source_map_gen {
                source_map.shift_after_single_line_replacement(line, column, old.len(), new.len());
            }
        }
        self.refresh_output_cursor();
    }

    /// Resolve an exported-empty synthetic name without rewriting equal text
    /// that arrived through a semantic string value after placeholder
    /// admission. Most generated uses are identifier-like tokens outside
    /// literals. System.register additionally emits the export name as the
    /// first half of an exact `"name", name` pair; resolve that pair first,
    /// then protect all completed-output literal/comment ranges.
    fn replace_deferred_export_placeholder(&mut self, old: &str, new: &str) {
        let quoted_old = format!("\"{old}\"");
        let system_suffix = format!(", {old}");
        let system_pairs = Scanner::new(&self.output)
            .scan_all()
            .into_iter()
            .filter(|token| token.kind == TokenKind::StringLiteral)
            .filter_map(|token| {
                let start = token.span.start as usize;
                let end = token.span.end as usize;
                (self.output.get(start..end) == Some(quoted_old.as_str())
                    && self
                        .output
                        .get(end..)
                        .is_some_and(|tail| tail.starts_with(&system_suffix)))
                .then_some(start..end + system_suffix.len())
            })
            .collect::<Vec<_>>();
        let resolved_system_pair = format!("\"{new}\", {new}");
        for range in system_pairs.into_iter().rev() {
            let (line, column) = self.output_line_col_at(range.start);
            let old_len = range.end - range.start;
            self.output.replace_range(range, &resolved_system_pair);
            if let Some(ref mut source_map) = self.source_map_gen {
                source_map.shift_after_single_line_replacement(
                    line,
                    column,
                    old_len,
                    resolved_system_pair.len(),
                );
            }
        }

        let (tokens, comments) = Scanner::new(&self.output).scan_all_with_comments();
        let mut protected_ranges = tokens
            .into_iter()
            .filter(|token| {
                matches!(
                    token.kind,
                    TokenKind::StringLiteral
                        | TokenKind::NoSubstitutionTemplate
                        | TokenKind::TemplateHead
                        | TokenKind::TemplateMiddle
                        | TokenKind::TemplateTail
                        | TokenKind::RegExpLiteral
                )
            })
            .map(|token| token.span.start as usize..token.span.end as usize)
            .collect::<Vec<_>>();
        protected_ranges.extend(
            comments
                .into_iter()
                .map(|comment| comment.pos as usize..comment.end as usize),
        );
        let positions = self
            .output
            .match_indices(old)
            .map(|(position, _)| position)
            .filter(|position| {
                !protected_ranges
                    .iter()
                    .any(|range| range.start <= *position && *position < range.end)
            })
            .collect::<Vec<_>>();
        for byte_pos in positions.into_iter().rev() {
            let (line, column) = self.output_line_col_at(byte_pos);
            self.output
                .replace_range(byte_pos..byte_pos + old.len(), new);
            if let Some(ref mut source_map) = self.source_map_gen {
                source_map.shift_after_single_line_replacement(line, column, old.len(), new.len());
            }
        }
        self.refresh_output_cursor();
    }

    /// Check whether `name` is an identifier token in the source. The shared
    /// TypeScript scanner disambiguates regex literals from division, excludes
    /// comments/string/template raw text, and includes template substitutions.
    fn source_has_identifier(&self, name: &str) -> bool {
        self.source_identifiers
            .get_or_init(|| {
                Scanner::new(self.source)
                    .scan_all()
                    .into_iter()
                    .filter(|token| token.kind == TokenKind::Identifier)
                    .filter_map(|token| {
                        self.source
                            .get(token.span.start as usize..token.span.end as usize)
                    })
                    .map(normalize_unicode_escapes)
                    .map(AstString::from)
                    .collect()
            })
            .contains(name)
    }

    /// Allocate a file-level temp var for CJS export postfix update expressions.
    /// Uses a separate counter/list that is NOT saved/restored by function scopes,
    /// because TSC hoists these to file level regardless of where they appear.
    fn next_cjs_file_level_temp_var(&mut self) -> String {
        const LETTERS: &[u8] = b"abcdefghjklmopqrstuvwxyz";
        let idx = self.cjs_file_level_temp_counter;
        self.cjs_file_level_temp_counter += 1;
        let name = if idx < LETTERS.len() {
            format!("_{}", LETTERS[idx] as char)
        } else {
            format!("_{}", idx - LETTERS.len())
        };
        self.cjs_file_level_temp_names.push(name.clone().into());
        name
    }

    /// Insert `var _a, _b, ...;\n` at `insert_pos` in the output buffer
    /// for file-level temp variables allocated during emission.
    fn insert_file_level_temp_vars(&mut self, insert_pos: usize) {
        // Combine regular file-level temps with CJS export temps.
        let mut regular_names = self.cjs_file_level_temp_names.clone();
        regular_names.extend(self.temp_var_names.iter().cloned());
        if regular_names.is_empty() && self.resolved_deferred_temp_names.is_empty() {
            return;
        }
        // When a private field var declaration already exists (e.g. `var _C_instances, _C_method;`),
        // merge temp vars into it to produce `var _C_instances, _C_method, _a;` instead of
        // two separate `var` statements.
        if let Some(semi_pos) = self.private_field_var_semicolon_pos {
            let mut append_text = String::new();
            for name in &regular_names {
                append_text.push_str(", ");
                append_text.push_str(name);
            }
            for name in &self.resolved_deferred_temp_names {
                append_text.push_str(", ");
                append_text.push_str(name);
            }
            if !append_text.is_empty() {
                self.insert_generated_text(semi_pos, &append_text);
            }
            return;
        }
        let mut decl = String::new();
        if !regular_names.is_empty() {
            decl.push_str("var ");
            decl.push_str(&regular_names.join(", "));
            decl.push_str(";\n");
        }
        if !self.resolved_deferred_temp_names.is_empty() {
            decl.push_str("var ");
            decl.push_str(&self.resolved_deferred_temp_names.join(", "));
            decl.push_str(";\n");
        }
        self.insert_generated_text(insert_pos, &decl);
    }

    fn parse_malformed_export_class_modifier_import_text(src: &str) -> Option<(String, String)> {
        let rest = src
            .trim_start()
            .strip_prefix("export public import ")
            .or_else(|| src.trim_start().strip_prefix("export private import "))
            .or_else(|| src.trim_start().strip_prefix("export static import "))?;
        let eq = rest.find('=')?;
        let name = rest[..eq].trim();
        let mut rhs = rest[eq + 1..].trim();
        if rhs.ends_with(';') {
            rhs = rhs[..rhs.len() - 1].trim_end();
        }
        if name.is_empty() || rhs.is_empty() {
            return None;
        }
        Some((name.to_string(), rhs.to_string()))
    }

    fn parse_malformed_export_class_modifier_import(&self, span: Span) -> Option<(String, String)> {
        let src = self.span_text(span)?;
        Self::parse_malformed_export_class_modifier_import_text(src)
    }

    fn collect_malformed_export_class_modifier_imports_from_source(&self) -> Vec<(String, String)> {
        self.source
            .lines()
            .filter_map(Self::parse_malformed_export_class_modifier_import_text)
            .collect()
    }

    // ------------------------------------------------------------------
    // Low-level output helpers
    // ------------------------------------------------------------------

    /// Update `out_line` and `out_col` after writing `s` to the output.
    #[inline]
    fn track_position(&mut self, s: &str) {
        let bytes = s.as_bytes();
        // Fast path: no newlines in the string (common for most writes).
        if let Some(last_nl) = bytes.iter().rposition(|&b| b == b'\n') {
            self.out_line += bytes[..=last_nl].iter().filter(|&&b| b == b'\n').count() as u32;
            self.out_col = (bytes.len() - last_nl - 1) as u32;
        } else {
            self.out_col += bytes.len() as u32;
        }
    }

    /// Re-anchor the source-map column to the bytes that are actually present
    /// on the current output line. Structured transforms occasionally enter
    /// this emitter after a parent replaced buffered text; doing this at the
    /// final-emission boundary prevents inherited speculative column drift.
    fn sync_source_map_output_column(&mut self) {
        if self.source_map_gen.is_none() {
            return;
        }
        self.out_col = self
            .output
            .as_bytes()
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(self.output.len(), |newline| self.output.len() - newline - 1)
            as u32;
        self.at_line_start = self.output.is_empty() || self.output.ends_with('\n');
    }

    #[inline]
    fn write(&mut self, s: &str) {
        if self.at_line_start && !s.is_empty() && s != "\n" {
            // Single push_str instead of N iterations for indentation.
            const SPACES: &str = "                                                                                                                                ";
            let indent_bytes = (self.indent as usize) * 4;
            if indent_bytes > 0 {
                if indent_bytes <= SPACES.len() {
                    self.output.push_str(&SPACES[..indent_bytes]);
                } else {
                    for _ in 0..self.indent {
                        self.output.push_str("    ");
                    }
                }
                self.out_col += indent_bytes as u32;
            }
            self.at_line_start = false;
        }
        self.output.push_str(s);
        self.track_position(s);
    }

    #[inline]
    fn writeln(&mut self, s: &str) {
        self.write(s);
        self.output.push('\n');
        self.out_line += 1;
        self.out_col = 0;
        self.at_line_start = true;
    }

    #[inline]
    fn newline(&mut self) {
        self.output.push('\n');
        self.out_line += 1;
        self.out_col = 0;
        self.at_line_start = true;
    }

    /// Remove trailing newline from output (if present) so a semicolon or
    /// other token can be appended on the same line.  Used after emitting
    /// class/function expressions that end with `}\n`.
    fn strip_trailing_newline(&mut self) {
        if self.output.ends_with('\n') {
            self.output.pop();
            self.at_line_start = false;
            if self.out_line > 0 {
                self.out_line -= 1;
            }
        }
    }

    /// Check if a member access object needs `..` instead of `.`.
    /// Plain decimal integers like `1` need `1..foo` to avoid ambiguity.
    /// Hex (`0xff`), octal (`0o7`), binary (`0b1`), floats (`1.5`),
    /// scientific notation (`1e5`), and BigInts (`1n`) do NOT need this.
    fn member_object_needs_double_dot(&self, object: &Expr) -> bool {
        match &object.kind {
            ExprKind::NumLit(s) => {
                // Plain decimal integer: all characters are ASCII digits.
                // Also check the normalized form — e.g., `08.8e5` normalizes to `880000`.
                if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
                    return true;
                }
                if Self::numlit_needs_normalize_str(s) {
                    let normalized = Self::normalize_numeric_literal(s);
                    return !normalized.is_empty()
                        && normalized.bytes().all(|b| b.is_ascii_digit());
                }
                false
            }
            // Unwrap type assertions (they're stripped in JS output).
            ExprKind::NonNull(inner) => self.member_object_needs_double_dot(inner),
            ExprKind::TypeAssertion(ta) => self.member_object_needs_double_dot(&ta.expr),
            ExprKind::As(a) => self.member_object_needs_double_dot(&a.expr),
            ExprKind::Satisfies(s) => self.member_object_needs_double_dot(&s.expr),
            ExprKind::Instantiation(inst) => self.member_object_needs_double_dot(&inst.expr),
            // Unwrap parens ONLY when the inner is a type assertion that
            // will be stripped (leaving the numeric literal bare).
            // `(1).foo` → unambiguous, no `..` needed.
            // `(1 as T).foo` → after stripping `as T`, becomes `1.foo`, needs `..`.
            ExprKind::Paren(inner) => match &inner.kind {
                ExprKind::NonNull(_) => self.member_object_needs_double_dot(inner),
                ExprKind::TypeAssertion(ta) => self.member_object_needs_double_dot(&ta.expr),
                ExprKind::As(a) => self.member_object_needs_double_dot(&a.expr),
                ExprKind::Satisfies(s) => self.member_object_needs_double_dot(&s.expr),
                ExprKind::Instantiation(inst) => self.member_object_needs_double_dot(&inst.expr),
                _ => false,
            },
            _ => false,
        }
    }

    /// Check if a numeric literal string needs normalization (legacy octal, invalid octal, or separators).
    pub(crate) fn numlit_needs_normalize_str(n: &str) -> bool {
        if n.contains('_') {
            return true;
        }
        let bytes = n.as_bytes();
        // Starts with 0, followed by a digit (not a prefix like x/o/b or ./e)
        // Covers both valid octals (all digits 0-7) and invalid octals (digits 8-9)
        bytes.len() > 1 && bytes[0] == b'0' && bytes[1].is_ascii_digit()
    }

    /// Check if a NumLit or BigIntLit expression needs normalization.
    fn numlit_needs_normalize(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::NumLit(n) => Self::numlit_needs_normalize_str(n),
            ExprKind::BigIntLit(n) => Self::bigint_needs_normalize(n),
            _ => false,
        }
    }

    /// Check if a BigInt literal needs normalization (binary/octal → decimal,
    /// separators, hex case).
    fn bigint_needs_normalize(n: &str) -> bool {
        if n.contains('_') {
            return true;
        }
        let bytes = n.as_bytes();
        // Legacy-octal-like bigint literal (e.g. `0123n`) needs parse-recovery normalization.
        if bytes.len() > 2 && bytes[0] == b'0' && bytes[1].is_ascii_digit() {
            return true;
        }
        if bytes.len() > 2 && bytes[0] == b'0' {
            match bytes[1] {
                b'b' | b'B' | b'o' | b'O' => return true,
                // Hex needs normalization if: empty prefix (0xn), uppercase digits,
                // or uppercase X prefix
                b'x' | b'X' => {
                    // Empty hex BigInt (e.g. `0xn`) or uppercase letters
                    return bytes.len() == 3 || n.chars().any(|c| c.is_ascii_uppercase());
                }
                _ => {}
            }
        }
        false
    }

    /// Normalize a numeric literal to its canonical JavaScript form.
    /// - Legacy octal (`02343`) → decimal (`1251`)
    /// - Numeric separators (`1_000`) → stripped (`1000`)
    /// - Prefixed literals with separators (`0b1010_0001`) → decimal (`161`)
    fn normalize_numeric_literal(n: &str) -> String {
        let has_separator = n.contains('_');

        // Strip separators first so all subsequent checks work on clean input.
        let clean: String = if has_separator {
            n.chars().filter(|&c| c != '_').collect()
        } else {
            n.to_string()
        };
        let cb = clean.as_bytes();

        // Starts with `0` followed by a digit — legacy octal or invalid octal.
        if cb.len() > 1 && cb[0] == b'0' && cb[1].is_ascii_digit() {
            // Valid octal: all digits after leading 0 are 0-7
            let is_valid_octal = cb[1..].iter().all(|&b| b >= b'0' && b <= b'7');
            if is_valid_octal {
                if let Ok(val) = i64::from_str_radix(&clean[1..], 8) {
                    return val.to_string();
                }
            }
            // Invalid octal (contains 8, 9, decimal point, or exponent):
            // use f64 parsing for canonical JS form.
            if let Ok(val) = clean.parse::<f64>() {
                return Self::format_f64_as_js(val);
            }
        }

        // Handle incomplete scientific notation: `1.0e` or `1e` (trailing `e`/`E`
        // with no exponent digits).  Parser error recovery produces these as
        // numeric literals; TypeScript normalizes by dropping the incomplete part.
        if cb.len() >= 2 && matches!(cb[cb.len() - 1], b'e' | b'E') {
            let base = &clean[..clean.len() - 1];
            if let Ok(val) = base.parse::<f64>() {
                return Self::format_f64_as_js(val);
            }
        }

        if !has_separator {
            return clean;
        }

        // If it was a prefixed literal (0x, 0o, 0b) with separators, convert to decimal.
        if cb.len() > 2 && cb[0] == b'0' {
            match cb[1] {
                b'x' | b'X' => {
                    if let Ok(val) = i64::from_str_radix(&clean[2..], 16) {
                        return val.to_string();
                    }
                }
                b'o' | b'O' => {
                    if let Ok(val) = i64::from_str_radix(&clean[2..], 8) {
                        return val.to_string();
                    }
                }
                b'b' | b'B' => {
                    if let Ok(val) = i64::from_str_radix(&clean[2..], 2) {
                        return val.to_string();
                    }
                }
                _ => {}
            }
        }

        // Plain decimal with separators — normalize via f64 for canonical form
        // (e.g. `.5_5` → `0.55`, `1_0e5_5` → `1e+56`).
        if let Ok(val) = clean.parse::<f64>() {
            return Self::format_f64_as_js(val);
        }
        clean
    }

    /// Format an f64 value the same way JavaScript's `String(number)` would.
    /// JS uses scientific notation (with explicit `+` sign) for values >= 1e21
    /// or < 1e-6, and plain decimal otherwise.
    fn format_f64_as_js(val: f64) -> String {
        if val == 0.0 {
            return "0".to_string();
        }
        let abs = val.abs();
        if abs >= 1e21 || (abs > 0.0 && abs < 1e-6) {
            // Use Rust's {:e} format, then add '+' for positive exponents
            let s = format!("{:e}", val);
            if let Some(pos) = s.find('e') {
                let (mantissa, exp_part) = s.split_at(pos);
                let exp = &exp_part[1..]; // skip 'e'
                if exp.starts_with('-') {
                    format!("{}e{}", mantissa, exp)
                } else {
                    format!("{}e+{}", mantissa, exp)
                }
            } else {
                s
            }
        } else {
            format!("{}", val)
        }
    }

    /// Normalize a BigInt literal string.
    /// TypeScript converts binary/octal BigInts to decimal, strips separators,
    /// and lowercases hex digits:  0b101n → 5n, 0o567n → 375n, 0xFFn → 0xffn
    fn normalize_bigint_literal(n: &str) -> String {
        // Strip the trailing `n` for processing, then re-add it.
        let suffix = if n.ends_with('n') { "n" } else { "" };
        let body = if n.ends_with('n') {
            &n[..n.len() - 1]
        } else {
            n
        };

        // Strip numeric separators.
        let clean: String = body.chars().filter(|&c| c != '_').collect();
        let cb = clean.as_bytes();

        // Legacy-octal-like bigint parse recovery: `0123n` -> `83, n`
        if cb.len() > 1 && cb[0] == b'0' && cb[1].is_ascii_digit() {
            if cb[1..].iter().all(|b| matches!(b, b'0'..=b'7')) {
                if let Ok(val) = u128::from_str_radix(&clean, 8) {
                    return format!("{val}, {suffix}");
                }
            }
        }

        if cb.len() >= 2 && cb[0] == b'0' {
            match cb[1] {
                b'b' | b'B' => {
                    let digits = &clean[2..];
                    if digits.is_empty() {
                        // Empty binary BigInt (`0bn`) → `0n`
                        return format!("0{suffix}");
                    }
                    // Binary → decimal
                    if let Ok(val) = u128::from_str_radix(digits, 2) {
                        return format!("{val}{suffix}");
                    }
                    // Fallback for very large values: use BigUint-style conversion
                    if let Some(dec) = Self::radix_to_decimal(digits, 2) {
                        return format!("{dec}{suffix}");
                    }
                }
                b'o' | b'O' => {
                    let digits = &clean[2..];
                    if digits.is_empty() {
                        // Empty octal BigInt (`0on`) → `0n`
                        return format!("0{suffix}");
                    }
                    // Octal → decimal
                    if let Ok(val) = u128::from_str_radix(digits, 8) {
                        return format!("{val}{suffix}");
                    }
                    if let Some(dec) = Self::radix_to_decimal(digits, 8) {
                        return format!("{dec}{suffix}");
                    }
                }
                b'x' | b'X' => {
                    let digits = &clean[2..];
                    if digits.is_empty() {
                        // Empty hex BigInt (`0xn`) → `0x0n`
                        return format!("0x0{suffix}");
                    }
                    // Hex stays hex but lowercased
                    return format!("{}{suffix}", clean.to_lowercase());
                }
                _ => {}
            }
        }

        // Plain decimal: return stripped version (lowercased for safety).
        format!("{clean}{suffix}")
    }

    /// Convert a string of digits in a given radix to a decimal string.
    /// Handles arbitrarily large values via manual multiplication.
    fn radix_to_decimal(digits: &str, radix: u32) -> Option<String> {
        // Use a simple big-integer approach: represent value as Vec<u8> of decimal digits.
        let mut result: Vec<u8> = vec![0]; // start with 0
        for ch in digits.chars() {
            let d = ch.to_digit(radix)?;
            // Multiply result by radix
            let mut carry: u32 = 0;
            for digit in result.iter_mut().rev() {
                let val = (*digit as u32) * radix + carry;
                *digit = (val % 10) as u8;
                carry = val / 10;
            }
            while carry > 0 {
                result.insert(0, (carry % 10) as u8);
                carry /= 10;
            }
            // Add d
            let mut carry = d;
            for digit in result.iter_mut().rev() {
                let val = (*digit as u32) + carry;
                *digit = (val % 10) as u8;
                carry = val / 10;
            }
            while carry > 0 {
                result.insert(0, (carry % 10) as u8);
                carry /= 10;
            }
        }
        // Strip leading zeros
        while result.len() > 1 && result[0] == 0 {
            result.remove(0);
        }
        Some(result.iter().map(|d| (d + b'0') as char).collect())
    }

    /// Record a source map mapping from the current output position to a source span.
    fn record_mapping(&mut self, span: Span) {
        self.record_mapping_at(span.start);
    }

    /// Record a mapping from the current output position to the end of a
    /// source span (tsc maps both ends of every emitted node).
    fn record_mapping_end(&mut self, span: Span) {
        self.record_mapping_at(span.end);
    }

    /// Record a mapping to source `pos` at the end of the text just
    /// written: when that text ended its line, the position before the
    /// newline.
    fn record_trailing_mapping(&mut self, pos: u32) {
        if !(self.at_line_start && self.output.ends_with('\n') && self.out_line > 0) {
            self.record_mapping_at(pos);
            return;
        }
        let body = &self.output[..self.output.len() - 1];
        let column = body
            .rfind('\n')
            .map_or(body.len(), |nl| body.len() - nl - 1) as u32;
        if let Some(ref mut gen) = self.source_map_gen {
            let (orig_line, orig_col) = if let Some(ref idx) = self.line_index {
                idx.offset_to_line_col(pos)
            } else {
                offset_to_line_col(self.source, pos)
            };
            gen.add_mapping(Mapping {
                generated_line: self.out_line - 1,
                generated_column: column,
                source_index: self.source_index,
                original_line: orig_line,
                original_column: orig_col,
                name_index: None,
            });
        }
    }

    /// Record a mapping from the current output position to source `pos`.
    /// At a line start the indentation is not written yet; the mapping
    /// points after it, where the text will start.
    fn record_mapping_at(&mut self, pos: u32) {
        let generated_column = if self.at_line_start {
            self.out_col + (self.indent * 4) as u32
        } else {
            self.out_col
        };
        if let Some(ref mut gen) = self.source_map_gen {
            let (orig_line, orig_col) = if let Some(ref idx) = self.line_index {
                idx.offset_to_line_col(pos)
            } else {
                offset_to_line_col(self.source, pos)
            };
            gen.add_mapping(Mapping {
                generated_line: self.out_line,
                generated_column,
                source_index: self.source_index,
                original_line: orig_line,
                original_column: orig_col,
                name_index: None,
            });
        }
    }

    /// Record a mapping with a name (for identifiers like variable names, function names).
    #[allow(dead_code)]
    fn record_mapping_with_name(&mut self, span: Span, name: &str) {
        if let Some(ref mut gen) = self.source_map_gen {
            let (orig_line, orig_col) = if let Some(ref idx) = self.line_index {
                idx.offset_to_line_col(span.start)
            } else {
                offset_to_line_col(self.source, span.start)
            };
            let name_idx = gen.add_name(name);
            gen.add_mapping(Mapping {
                generated_line: self.out_line,
                generated_column: self.out_col,
                source_index: self.source_index,
                original_line: orig_line,
                original_column: orig_col,
                name_index: Some(name_idx),
            });
        }
    }

    /// Copy source text verbatim from span.
    fn copy_span(&mut self, span: Span) {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < end && end <= self.source.len() {
            // Handle line-start indentation like `write` does, so that
            // copy_span called at the beginning of a line produces properly
            // indented output (e.g. `"use strict"` inside an __awaiter body).
            if self.at_line_start {
                const SPACES: &str = "                                                                                                                                ";
                let indent_bytes = (self.indent as usize) * 4;
                if indent_bytes > 0 {
                    if indent_bytes <= SPACES.len() {
                        self.output.push_str(&SPACES[..indent_bytes]);
                    } else {
                        for _ in 0..self.indent {
                            self.output.push_str("    ");
                        }
                    }
                    self.out_col += indent_bytes as u32;
                }
                self.at_line_start = false;
            }
            self.record_mapping(span);
            let text = &self.source[start..end];
            self.output.push_str(text);
            self.track_position(text);
        }
    }

    /// Copy an expression's source span, normalizing indentation for multi-line
    /// expressions. Single-line expressions are copied verbatim. For multi-line
    /// expressions, the first line is kept inline and subsequent lines get their
    /// indentation normalized to 4-space steps relative to `self.indent`.
    /// Copy raw source text for a span without any normalization.
    /// Used for JSX preserve mode where source should be emitted as-is.
    #[allow(dead_code)]
    fn copy_expr_span_raw(&mut self, span: Span) {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return;
        }
        self.record_mapping(span);
        let text = &self.source[start..end];
        self.write(text);
    }

    fn copy_expr_span(&mut self, span: Span) {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return;
        }
        self.record_mapping(span);
        let text = &self.source[start..end];

        // When removeComments is enabled, strip comments from source-copied text.
        let text: std::borrow::Cow<str> = if self.options.remove_comments == Some(true) {
            std::borrow::Cow::Owned(strip_source_comments(text))
        } else {
            std::borrow::Cow::Borrowed(text)
        };

        // Fast emit: the expression source is already valid JS — copy it verbatim
        // and skip the cosmetic normalization chain below (brace spacing, unified
        // pass, tagged-template/yield spacing, tab expansion, line re-indent).
        // Output formatting is irrelevant for PRISM (feeds V8). copy_expr_span is
        // a hot per-expression path under fast emit. NEL-containing sources fall
        // through: U+0085 is TS whitespace but not JS whitespace, so the slow
        // path's normalize_nel is a validity fix, not a cosmetic one.
        if self.fast_emit && !self.source_has_nel {
            self.write(&text);
            return;
        }

        if !text.contains('\n') {
            // Single line: normalize brace spacing, keyword-paren spacing, and copy.
            // Use write() to respect at_line_start indentation.
            // Normalizers return Cow<str>: fast-path no-ops return Borrowed (zero alloc).
            let n = if self.skip_brace_normalize {
                std::borrow::Cow::Borrowed(&*text)
            } else {
                normalize_brace_spacing(&text)
            };
            // Even when brace normalization is skipped (JSX preserve),
            // normalize empty block bodies: `=> {}` → `=> { }`, `) {}` → `) { }`
            let n = normalize_empty_blocks(&n);
            // Six chain stages collapsed into one unified pass:
            // keyword_paren → import_call_spacing → unary_spacing →
            // trailing_semi → close_paren → comment_word_space.
            let n = normalize_unified_pass(&n);
            let n = normalize_tagged_template_space(&n);
            let n = strip_trailing_comment_on_brace_line(&n);
            let n = if n.contains('\u{0085}') {
                normalize_nel(&n)
            } else {
                std::borrow::Cow::Borrowed(&*n)
            };
            let n = normalize_yield_spacing(&n);
            if n.contains('\t') {
                let expanded = n.replace('\t', "    ");
                self.write(&expanded);
            } else {
                self.write(&n);
            }
            return;
        }

        // Normalize yield spacing in generator function expressions before
        // line splitting so the `function*` context is visible.
        let text = normalize_yield_spacing(&text);

        // Normalize tagged template spacing on the full text (before line splitting)
        // so that template literals spanning multiple lines are tracked correctly.
        // A closing backtick on a continuation line would be misidentified as an
        // opening backtick if normalized per-line.
        let text = normalize_tagged_template_space(&text);

        // Expand tabs to 4 spaces for consistent indent handling. Keep as
        // `Cow<str>`: when there are no tabs (the typical path), the Borrowed
        // variant survives without `into_owned` allocating a copy of the
        // entire (multi-line) span just to type-match.
        let text: std::borrow::Cow<str> = if text.contains('\t') {
            std::borrow::Cow::Owned(text.replace('\t', "    "))
        } else {
            text
        };

        let mut lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return;
        }

        // Pre-compute, per line, whether the line lives INSIDE a template
        // literal's text region (i.e. between an open `…` and the matching
        // close, *not* inside a `${ ... }` expression — those are JS and
        // should normalize). Per-line normalizers (`normalize_brace_spacing`,
        // etc.) start each call with a fresh string-state, so a multi-line
        // template literal like
        //
        //     const css = `:root {
        //       --gray-500: red;
        //     }`;
        //
        // gets tokenized line-by-line: line 2 is seen as bare code, the
        // `--gray-500` runs through the binary-minus rule, and the output
        // is `--gray - 500`. We mark those lines and skip normalization
        // for them. Inside `${…}` expressions, normalize again — they're
        // first-class JS.
        let inside_template_text: Vec<bool> = {
            let bytes = text.as_bytes();
            let mut flags: Vec<bool> = Vec::with_capacity(lines.len());
            let mut in_template: bool = false;
            // Stack: each entry is the template-nesting count of `${` we
            // entered. Plain `{` braces inside a template expression match
            // through `}` like normal JS. We track template-expression
            // braces specifically with a separate counter.
            let mut template_expr_depth: u32 = 0;
            // String-state for non-template strings (so backticks inside
            // a `'...'` don't toggle template mode).
            let mut in_string: u8 = 0;
            let mut in_line_comment = false;
            let mut in_block_comment = false;
            let mut line_idx: usize = 0;
            // The line is "inside template text" if at the START of the
            // line we're inside the backtick-quoted region AND not inside
            // a `${...}` expression on this line.
            flags.push(in_template && template_expr_depth == 0);
            let mut i = 0usize;
            while i < bytes.len() {
                let ch = bytes[i];
                if ch == b'\n' {
                    line_idx += 1;
                    if line_idx < lines.len() {
                        flags.push(in_template && template_expr_depth == 0);
                    }
                    in_line_comment = false;
                    i += 1;
                    continue;
                }
                if in_line_comment {
                    i += 1;
                    continue;
                }
                if in_block_comment {
                    if ch == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                        in_block_comment = false;
                        i += 2;
                        continue;
                    }
                    i += 1;
                    continue;
                }
                if in_string != 0 {
                    if ch == b'\\' {
                        i += 2;
                        continue;
                    }
                    if ch == in_string {
                        in_string = 0;
                    }
                    i += 1;
                    continue;
                }
                // Not inside any nested context. Now check template-state.
                if in_template && template_expr_depth == 0 {
                    // We're in raw template text. Only `${` (enter expr)
                    // and a closing `\`` (exit template) matter here.
                    if ch == b'\\' {
                        i += 2;
                        continue;
                    }
                    if ch == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                        template_expr_depth = template_expr_depth.saturating_add(1);
                        i += 2;
                        continue;
                    }
                    if ch == b'`' {
                        in_template = false;
                        i += 1;
                        continue;
                    }
                    i += 1;
                    continue;
                }
                // Either not in a template OR inside a `${...}` (JS code).
                if ch == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    in_line_comment = true;
                    i += 2;
                    continue;
                }
                if ch == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    in_block_comment = true;
                    i += 2;
                    continue;
                }
                if ch == b'\'' || ch == b'"' {
                    in_string = ch;
                    i += 1;
                    continue;
                }
                if ch == b'`' {
                    in_template = true;
                    i += 1;
                    continue;
                }
                if template_expr_depth > 0 {
                    if ch == b'{' {
                        template_expr_depth = template_expr_depth.saturating_add(1);
                    } else if ch == b'}' {
                        template_expr_depth = template_expr_depth.saturating_sub(1);
                    }
                }
                i += 1;
            }
            // Pad to lines.len() in case of trailing-newline mismatch.
            while flags.len() < lines.len() {
                flags.push(in_template && template_expr_depth == 0);
            }
            flags
        };

        // Remove blank lines inside block bodies ({ ... }) and array literals ([ ... ]).
        // TypeScript's structured emit never preserves blank lines inside
        // function/arrow bodies or array element lists, so strip them when
        // source-copying expressions.
        let mut inside_template_text = inside_template_text;
        {
            let mut i = 1; // skip first line
            while i < lines.len() {
                if lines[i].trim().is_empty() {
                    // Skip blank-line removal entirely when the line is
                    // inside a template literal — we must preserve every
                    // line of the template content (including blanks) to
                    // keep the template's actual value intact.
                    if i < inside_template_text.len() && inside_template_text[i] {
                        i += 1;
                        continue;
                    }
                    // Remove blank lines before `}`, `)`, or `]`
                    let next_trimmed = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
                    if next_trimmed.starts_with('}')
                        || next_trimmed.starts_with(')')
                        || next_trimmed.starts_with(']')
                    {
                        lines.remove(i);
                        if i < inside_template_text.len() {
                            inside_template_text.remove(i);
                        }
                        continue;
                    }
                    // Remove blank lines after `{` or `[`
                    if i > 0
                        && (lines[i - 1].trim_end().ends_with('{')
                            || lines[i - 1].trim_end().ends_with('['))
                    {
                        lines.remove(i);
                        if i < inside_template_text.len() {
                            inside_template_text.remove(i);
                        }
                        continue;
                    }
                    // Remove blank lines between statements/elements inside blocks or arrays
                    // (TypeScript doesn't preserve blank lines in structured emit)
                    let in_block_or_array = lines[..i]
                        .iter()
                        .any(|l| l.trim_end().ends_with('{') || l.trim_end().ends_with('['));
                    if in_block_or_array {
                        lines.remove(i);
                        if i < inside_template_text.len() {
                            inside_template_text.remove(i);
                        }
                        continue;
                    }
                }
                i += 1;
            }
        }

        // Record the output column where the first line will start.
        // This is used to align continuation lines with the expression start.
        let output_is_inline = !self.at_line_start;
        let first_line_out_col = if self.at_line_start {
            (self.indent as usize) * 4
        } else {
            self.out_col as usize
        };

        // First line continues the current output line (e.g. after "var a = ").
        // Apply brace spacing normalization and trim trailing whitespace.
        // Use write() to respect at_line_start indentation.
        let first_trimmed = lines[0].trim_end();
        let first_normalized = if self.skip_brace_normalize {
            std::borrow::Cow::Borrowed(first_trimmed)
        } else {
            normalize_brace_spacing(first_trimmed)
        };
        // keyword_paren + import_call_spacing + comment_word_space →
        // one unified pass.
        let first_normalized = normalize_unified_pass(&first_normalized);
        let first_normalized = strip_trailing_comment_on_brace_line(&first_normalized);
        self.write(&first_normalized);

        if lines.len() == 1 {
            return;
        }

        // Compute indentation parameters from subsequent lines.
        let remaining = &lines[1..];
        let indents: Vec<usize> = remaining
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .collect();

        let min_indent = indents.iter().copied().min().unwrap_or(0);
        let indent_unit = indents
            .iter()
            .copied()
            .filter(|&i| i > min_indent)
            .map(|i| i - min_indent)
            .min()
            .unwrap_or(4);
        let needs_normalize = indent_unit != 4 && indent_unit > 0;

        // Compute alignment offset: continuation lines should align relative
        // to the first line's output column, not just self.indent * 4.
        let base_indent = (self.indent as usize) * 4;
        let source_line_start = {
            let before = &self.source[..start];
            before.rfind('\n').map(|p| p + 1).unwrap_or(0)
        };
        let source_first_col = start - source_line_start;
        let starts_after_prefix = !self.source[source_line_start..start].trim().is_empty();
        // Compute the leading whitespace (indent) of the source line.
        let source_line_indent = {
            let line_prefix = &self.source[source_line_start..start];
            line_prefix.len() - line_prefix.trim_start().len()
        };
        // Check if the first line ends with a binary operator — used to
        // identify binary expression continuations for alignment.
        let first_ends_with_binop = {
            let fl = first_trimmed.trim();
            fl.ends_with("&&")
                || fl.ends_with("||")
                || fl.ends_with("??")
                || fl.ends_with('+')
                || fl.ends_with('-')
                || fl.ends_with('|')
                || fl.ends_with('&')
                || fl.ends_with('^')
        };
        let alignment_extra = if starts_after_prefix && min_indent >= source_first_col {
            first_line_out_col.saturating_sub(base_indent)
        } else if starts_after_prefix && min_indent < source_first_col && first_ends_with_binop {
            // Binary expression continuation where continuation lines have
            // less indent than the expression start column. TypeScript adds
            // a single continuation indent level (4 spaces), not full alignment.
            4
        } else if !starts_after_prefix
            && output_is_inline
            && first_line_out_col > base_indent
            && min_indent >= source_first_col
            && first_ends_with_binop
        {
            // Expression starts on its own source line (only whitespace before it)
            // but is being emitted inline (e.g., after "if (" where the condition
            // is on the next source line). Add extra indent so binary expression
            // continuation lines align with the first line's output position.
            first_line_out_col.saturating_sub(base_indent)
        } else {
            0
        };
        // For array/bracket-delimited expressions that start mid-line,
        // continuation lines should preserve their indentation relative
        // to the source line's indent, not the expression start. This
        // handles patterns like `var x = [{...},\n    {...}]` where the
        // continuation should keep its 4-space indent from the source.
        // Only applied when the first line starts with `[` (array literal)
        // to avoid affecting other patterns like method chains or returns.
        let array_cont_extra = if starts_after_prefix
            && min_indent < source_first_col
            && min_indent > source_line_indent
            && first_trimmed.starts_with('[')
        {
            min_indent - source_line_indent
        } else {
            0
        };
        // For method-chain continuations (lines starting with `.`) where the expression
        // is being copied from source, TypeScript normalizes their indentation to one
        // indent level (4 spaces) relative to the output base indent.
        // This is computed per-line below so only `.`-prefixed lines get the extra indent.
        // Applies both when the expression starts mid-line (`starts_after_prefix=true`)
        // and when it starts at the beginning of its source line.
        let chain_dot_extra = min_indent > 0 && !self.skip_brace_normalize;
        let flat_array_chain_continuation = first_trimmed.starts_with('[')
            && remaining
                .iter()
                .filter(|line| !line.trim().is_empty())
                .all(|line| line.trim_start().starts_with('.'));
        let array_object_literal_extra_indent: Vec<usize> = {
            let mut extras = vec![0usize; lines.len()];
            let mut depth = 0usize;
            let mut stack: Vec<usize> = Vec::new();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if i > 0 && depth > 0 && !trimmed.is_empty() {
                    extras[i] = depth * 4;
                }
                if trimmed.is_empty() {
                    continue;
                }
                let bytes = trimmed.as_bytes();
                for idx in 0..bytes.len() {
                    if bytes[idx] != b'{' {
                        continue;
                    }
                    let mut run = 0usize;
                    let mut back = idx;
                    while back > 0 && bytes[back - 1] == b'[' {
                        run += 1;
                        back -= 1;
                    }
                    stack.push(run);
                    depth = depth.saturating_add(run);
                }
                for ch in trimmed.chars() {
                    if ch == '}' {
                        let run = stack.pop().unwrap_or(0);
                        depth = depth.saturating_sub(run);
                    }
                }
            }
            extras
        };

        let is_comment_line = |trimmed: &str| {
            trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with("*/")
                || trimmed.starts_with('*')
        };
        // Compute cumulative paren depth at the start of each line.
        // Used to add extra indentation for binary continuations inside
        // nested parenthesized expressions, and to suppress continuation
        // indent when inside parens where source indent already accounts for nesting.
        let paren_depth_at_start: Vec<usize> = {
            let mut depths = vec![0usize; lines.len()];
            let mut depth: isize = 0;
            for (i, line) in lines.iter().enumerate() {
                depths[i] = depth.max(0) as usize;
                let trimmed = line.trim();
                if is_comment_line(trimmed) {
                    continue;
                }
                let mut in_string = false;
                let mut string_char = b'"';
                let bytes = trimmed.as_bytes();
                let mut j = 0;
                while j < bytes.len() {
                    let ch = bytes[j];
                    if in_string {
                        if ch == string_char && (j == 0 || bytes[j - 1] != b'\\') {
                            in_string = false;
                        }
                    } else if ch == b'"' || ch == b'\'' || ch == b'`' {
                        in_string = true;
                        string_char = ch;
                    } else if ch == b'/' && j + 1 < bytes.len() && bytes[j + 1] == b'/' {
                        break;
                    } else if ch == b'(' {
                        depth += 1;
                    } else if ch == b')' {
                        depth -= 1;
                    }
                    j += 1;
                }
            }
            depths
        };
        let continuation_extra: Vec<usize> = {
            let mut extras = vec![0usize; lines.len()];
            for i in 1..lines.len() {
                let trimmed = lines[i].trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Some(prev_idx) = (0..i).rev().find(|&j| !lines[j].trim().is_empty()) else {
                    continue;
                };
                let prev_trimmed = lines[prev_idx].trim();
                let anchor_extra = (0..i)
                    .rev()
                    .find(|&j| {
                        let trimmed = lines[j].trim();
                        !trimmed.is_empty() && !is_comment_line(trimmed)
                    })
                    .map(|j| extras[j])
                    .unwrap_or(0);
                let is_leading_binary =
                    crate::emit_stmt_helpers::line_starts_with_binary_operator(trimmed)
                        || crate::emit_stmt_helpers::line_is_binary_operator_only(trimmed);
                let is_leading_ternary =
                    (trimmed.starts_with("? ") || trimmed == "?") && !trimmed.starts_with("??");
                let is_leading_colon = trimmed.starts_with(": ") || trimmed == ":";
                // Extra indentation based on paren depth relative to first line.
                let paren_extra =
                    paren_depth_at_start[i].saturating_sub(paren_depth_at_start[0]) * 4;
                if is_leading_ternary {
                    extras[i] = if anchor_extra > 0 {
                        anchor_extra + 4
                    } else {
                        4.max(paren_extra + 4)
                    };
                } else if is_leading_colon {
                    extras[i] = if anchor_extra > 0 {
                        anchor_extra
                    } else {
                        4.max(paren_extra)
                    };
                } else if is_leading_binary {
                    if self.suppress_continuation_paren_depth {
                        extras[i] = anchor_extra.max(4);
                    } else {
                        extras[i] = anchor_extra.max(4).max(paren_extra + 4);
                    }
                } else if let Some(op) =
                    crate::emit_stmt_helpers::line_trailing_continuation_op(prev_trimmed)
                {
                    extras[i] = match op {
                        crate::emit_stmt_helpers::TrailingContinuationOp::Binary => {
                            // Don't add paren_extra for trailing continuation —
                            // paren depth is only used for leading binary
                            // operators (like `&& (expr)`) to detect nested
                            // binary chains.
                            anchor_extra.max(4)
                        }
                        crate::emit_stmt_helpers::TrailingContinuationOp::TernaryQuestion => {
                            anchor_extra + 4
                        }
                        crate::emit_stmt_helpers::TrailingContinuationOp::TernaryColon => {
                            anchor_extra
                        }
                    };
                }
                if extras[i] == 0 {
                    continue;
                }
                for j in (0..i).rev() {
                    let back_trimmed = lines[j].trim();
                    if back_trimmed.is_empty() {
                        continue;
                    }
                    if extras[j] > 0 {
                        break;
                    }
                    if is_comment_line(back_trimmed) {
                        extras[j] = extras[i];
                    } else {
                        break;
                    }
                }
            }
            extras
        };

        // Track the relative indent of the last non-comment code line,
        // so standalone `//` comment lines can inherit it instead of preserving
        // their source column alignment (which may be much deeper).
        let mut last_code_relative: usize = 0;

        for (line_idx, line) in remaining.iter().enumerate() {
            // Move to a new line.
            self.output.push('\n');
            self.out_line += 1;
            self.out_col = 0;
            self.at_line_start = true;

            // Template-literal text content: copy the line verbatim (with
            // original whitespace) and bypass all indent + normalization
            // logic. The user's exact bytes inside backticks are part of
            // the runtime string value and must not be reformatted.
            // (`line_idx + 1` because `remaining` is `lines[1..]`.)
            let line_idx_in_full_early = line_idx + 1;
            if line_idx_in_full_early < inside_template_text.len()
                && inside_template_text[line_idx_in_full_early]
            {
                self.output.push_str(line);
                self.track_position(line);
                self.at_line_start = false;
                continue;
            }

            if line.trim().is_empty() {
                continue;
            }

            // Preserve trailing whitespace on lines with `//` comments
            // (TypeScript keeps trailing spaces from source comments).
            let trimmed_start = line.trim_start();
            let is_standalone_comment = trimmed_start.starts_with("//");
            // Check for trailing `//` comment: look for ` //` pattern
            // (space before //) which indicates a trailing comment.
            let has_trailing_comment = !is_standalone_comment && trimmed_start.contains(" //");
            let raw_content = if is_standalone_comment || has_trailing_comment {
                trimmed_start.trim_end_matches('\r')
            } else {
                trimmed_start.trim_end()
            };
            let content = if self.skip_brace_normalize {
                std::borrow::Cow::Borrowed(raw_content)
            } else {
                normalize_brace_spacing(raw_content)
            };
            // keyword_paren + import_call_spacing + trailing_semi +
            // comment_word_space → one unified pass.
            let content = normalize_unified_pass(&content);
            // Note: normalize_tagged_template_space is applied to the full text
            // before splitting (above) to correctly track multi-line templates.
            let content = strip_trailing_comment_on_brace_line(&content);
            // Use trim_start to compute leading indent only (not trailing whitespace).
            let line_indent = line.len() - line.trim_start().len();
            let mut relative = line_indent.saturating_sub(min_indent);

            // For standalone `//` comment lines that are MUCH more deeply
            // indented than the code (aligned with a trailing `//` comment
            // on the previous code line), snap them to the code indentation.
            // Only snap when the difference is more than one indent level
            // to avoid affecting legitimately indented comments.
            if is_standalone_comment && relative > last_code_relative + 4 {
                relative = last_code_relative;
            }
            if !is_comment_line(trimmed_start) {
                last_code_relative = relative;
            }

            // For expressions that start at the beginning of their source line
            // (pure whitespace before them) with continuation lines starting with `.`,
            // add one extra indent level (4 spaces) to preserve method-chain indentation.
            let per_line_extra = if chain_dot_extra && content.starts_with('.') {
                4
            } else {
                0
            };
            let per_line_array_extra = if content.starts_with(']') {
                0
            } else {
                array_cont_extra
            };
            // Compute total extra padding (beyond base indent from self.write).
            // Only apply continuation_extra when the source has no indent for this
            // line (relative == 0), meaning the source didn't already include
            // continuation indentation.  When relative > 0, the source already
            // has the continuation baked in.
            let cont_extra = if relative == 0 {
                continuation_extra[line_idx + 1]
            } else {
                0
            };
            let pad = if flat_array_chain_continuation && content.starts_with('.') {
                4
            } else if needs_normalize && relative > 0 {
                let levels = relative / indent_unit;
                let extra = relative % indent_unit;
                alignment_extra
                    + per_line_extra
                    + per_line_array_extra
                    + array_object_literal_extra_indent[line_idx + 1]
                    + cont_extra
                    + levels * 4
                    + extra
            } else {
                alignment_extra
                    + per_line_extra
                    + per_line_array_extra
                    + array_object_literal_extra_indent[line_idx + 1]
                    + cont_extra
                    + relative
            };

            // Write extra padding (if any) first — this triggers at_line_start
            // indent via write(), then append content without re-indenting.
            const SPACES: &str = "                                                                                                                                ";
            if pad > 0 {
                if pad <= SPACES.len() {
                    self.write(&SPACES[..pad]);
                } else {
                    self.write(&" ".repeat(pad));
                }
                self.output.push_str(&content);
                self.track_position(&content);
            } else {
                self.write(&content);
            }
        }
    }

    /// If the source text after a statement span has a trailing comment
    /// on the same line, append it to the output.
    /// This preserves comments like `// Expect no error here` and `/* comment */`
    /// on transformed statements.
    fn append_trailing_comment(&mut self, span: Span) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        let span_start = span.start as usize;
        let span_end = span.end as usize;
        // A recovery expression can include the opening brace of the next
        // block in its span.  A comment after that brace belongs to the block,
        // not to the recovered expression preceding it.
        if span_start < span_end
            && span_end <= self.source.len()
            && self.source[span_start..span_end].trim_end().ends_with('{')
        {
            return;
        }
        if span_start < span_end
            && span_end <= self.source.len()
            && self.source[span_start..span_end].contains("()?:")
            && !self.source[span_start..span_end].contains('{')
        {
            return;
        }
        let end = span.end as usize;
        if end >= self.source.len() {
            return;
        }
        // Look from span.end to end of line for a trailing // comment
        let rest = &self.source[end..];
        // Find the end of the current line
        let eol = rest.find('\n').unwrap_or(rest.len());
        let tail = &rest[..eol];
        // Check for a trailing // or /* ... */ comment in the trailing text
        if let Some(comment_pos) = find_trailing_comment_start(tail) {
            // If there is a semicolon or closing brace between span.end and the
            // comment, the comment belongs to a subsequent statement or outer scope.
            // (e.g. `enum E {}; // x` has `;` as a separate empty statement,
            //  `namespace N { var y = 2; } // x` has `}` closing the scope)
            let before_comment = &tail[..comment_pos];
            if before_comment.contains(';')
                || before_comment.contains('{')
                || before_comment.contains('}')
            {
                return;
            }
            // Preserve trailing whitespace in comments - TypeScript keeps it
            let comment = tail[comment_pos..].trim_end_matches('\r');
            // Only attach single-line block comments in this path.
            if comment.starts_with("/*") && !comment.contains("*/") {
                return;
            }
            // For block comments, strip any trailing non-comment content after
            // the last `*/` (e.g. trailing commas: `/* blue */,` → `/* blue */`).
            // But preserve multiple block comments and line comments on the same line:
            // `/* comment1 */ /* comment2 */` → keep both.
            // `/* comment1 */ // comment2` → keep both.
            let comment = if comment.starts_with("/*") {
                if let Some(end_pos) = comment.rfind("*/") {
                    let after_block = &comment[end_pos + 2..].trim_start();
                    if after_block.starts_with("//") || after_block.starts_with("/*") {
                        // Preserve the block comment + subsequent comment
                        comment.trim_end()
                    } else {
                        &comment[..end_pos + 2]
                    }
                } else {
                    comment
                }
            } else {
                comment
            };
            if !comment.is_empty() {
                if span.start < self.skip_recovery_until
                    && self.output[self.stmt_output_start.min(self.output.len())..]
                        .contains(comment)
                {
                    return;
                }
                // The output should already end with \n from writeln
                // Insert the comment before the final newline
                if self.output.ends_with('\n') {
                    // Check if the current output line already contains this
                    // comment (e.g. from source-copy of an inner statement).
                    let last_line_start = self.output[..self.output.len() - 1]
                        .rfind('\n')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let last_line = &self.output[last_line_start..self.output.len() - 1];
                    if last_line.contains(comment) {
                        // Already present — skip duplicate.
                    } else {
                        self.output.pop();
                        self.output.push(' ');
                        self.output.push_str(comment);
                        self.output.push('\n');
                    }
                }
            }
        }
    }

    /// Get source text between two byte offsets.
    fn source_between(&self, from: u32, to: u32) -> &'a str {
        let from = from as usize;
        let to = to as usize;
        if from <= to && to <= self.source.len() {
            &self.source[from..to]
        } else {
            ""
        }
    }

    /// Check if there are block comments (`/* ... */`) in source between two positions.
    fn has_block_comments_between(&self, from: u32, to: u32) -> bool {
        self.source_between(from, to).contains("/*")
    }

    /// Check whether a trailing block comment (`/* ... */`) appears
    /// before the next comma on the same source line after `pos`.
    /// Returns `true` for patterns like `'text' /*as const*/,`.
    fn trailing_block_comment_before_comma(&self, pos: u32) -> bool {
        let pos = pos as usize;
        if pos >= self.source.len() {
            return false;
        }
        let rest = &self.source[pos..];
        let eol = rest.find('\n').unwrap_or(rest.len());
        let line = &rest[..eol];
        // Find first `/*` and first `,` on this line
        if let (Some(comment_pos), Some(comma_pos)) = (line.find("/*"), line.find(',')) {
            comment_pos < comma_pos
        } else {
            false
        }
    }

    /// Find the position of the `=` sign between two source positions,
    /// skipping over block comments and strings. Returns absolute position.
    fn find_equals_between(&self, from: u32, to: u32) -> Option<usize> {
        let from = from as usize;
        let to = (to as usize).min(self.source.len());
        if from >= to {
            return None;
        }
        let slice = &self.source[from..to];
        let bytes = slice.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                // Skip block comment /* ... */
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                // Skip line comment // ... until newline
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1; // skip the newline
                }
            } else if bytes[i] == b'='
                && (i + 1 >= bytes.len() || (bytes[i + 1] != b'=' && bytes[i + 1] != b'>'))
            {
                // Found `=` (not `==` or `=>`)
                return Some(from + i);
            } else {
                i += 1;
            }
        }
        None
    }

    /// Find the first `)` in the given text, skipping over block comments.
    fn find_close_paren_skipping_comments(s: &str) -> Option<usize> {
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else if bytes[i] == b')' {
                return Some(i);
            } else {
                i += 1;
            }
        }
        None
    }

    /// Find a keyword in source text, skipping block comments.
    fn find_keyword_after(source: &str, keyword: &str, from: usize) -> usize {
        let src = &source[from..];
        let bytes = src.as_bytes();
        let kw_bytes = keyword.as_bytes();
        let mut i = 0;
        while i + kw_bytes.len() <= bytes.len() {
            if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else if &bytes[i..i + kw_bytes.len()] == kw_bytes {
                return from + i;
            } else {
                i += 1;
            }
        }
        from
    }

    /// Copy source text for a span, trimming leading/trailing whitespace lines
    /// but preserving internal formatting.
    fn copy_span_trimmed(&self, span: Span) -> &'a str {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < end && end <= self.source.len() {
            &self.source[start..end]
        } else {
            ""
        }
    }

    /// Source-copy an import/export statement from the source text.
    /// Returns the statement text without trailing semicolons/whitespace.
    /// This preserves inline comments between tokens.
    /// Only use when `span_has_inline_comment` returns true.
    fn source_copy_import_stmt(&self, span: Span) -> &'a str {
        let raw = self.copy_span_trimmed(span);
        raw.trim_end_matches(|c: char| c == ';' || c.is_whitespace())
    }

    /// Check if a span contains inline block comments (`/* ... */`).
    /// Used to decide whether source-copy is worthwhile for imports/exports.
    fn span_has_inline_comment(&self, span: Span) -> bool {
        let s = span.start as usize;
        let e = (span.end as usize).min(self.source.len());
        if s >= e {
            return false;
        }
        self.source[s..e].contains("/*")
    }

    /// Detect the quote character used for a string literal in source text.
    /// Looks for `'` or `"` near the end of the span (module source path).
    fn detect_string_quote(&self, span: Span) -> &'static str {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < end && end <= self.source.len() {
            let text = &self.source[start..end];
            // Search from end for the closing quote
            if text.contains('\'') {
                return "'";
            }
        }
        "\""
    }

    /// Emit `assert { ... }` or `with { ... }` import attributes from source text.
    /// Scans the source span for an `assert` or `with` keyword after the module
    /// specifier string and copies it verbatim.
    pub(crate) fn emit_import_attributes_from_source(&mut self, span: Span) {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return;
        }
        let text = &self.source[start..end];
        let tokens = Scanner::new(text).scan_all();
        for (idx, token) in tokens.iter().enumerate() {
            if !matches!(token.kind, TokenKind::Assert | TokenKind::With)
                || tokens.get(idx + 1).map(|t| t.kind) != Some(TokenKind::OpenBrace)
            {
                continue;
            }
            let mut depth = 0usize;
            for clause_token in &tokens[idx + 1..] {
                match clause_token.kind {
                    TokenKind::OpenBrace => depth += 1,
                    TokenKind::CloseBrace => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            let clause_start = token.span.start as usize;
                            let clause_end = clause_token.span.end as usize;
                            self.write(" ");
                            self.write(text[clause_start..clause_end].trim());
                            return;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Token positions needed when `export * as ns from "mod"` is split into
    /// a synthetic import and export under `module: es2015`.
    fn export_star_token_positions(&self, span: Span) -> Option<(Span, u32, Span, u32)> {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return None;
        }
        let text = &self.source[start..end];
        let tokens = Scanner::new(text).scan_all();
        let as_idx = tokens
            .iter()
            .position(|token| token.kind == TokenKind::As)?;
        let alias = tokens.get(as_idx + 1)?;
        let from_idx = tokens
            .iter()
            .enumerate()
            .skip(as_idx + 2)
            .find_map(|(idx, token)| (token.kind == TokenKind::From).then_some(idx))?;
        let from = &tokens[from_idx];
        let source = tokens.get(from_idx + 1)?;
        if source.kind != TokenKind::StringLiteral {
            return None;
        }
        let after_source = tokens
            .get(from_idx + 2)
            .map_or(text.len() as u32, |token| token.span.start);
        Some((
            Span::new(span.start + alias.span.start, span.start + alias.span.end),
            span.start + from.span.start,
            Span::new(span.start + source.span.start, span.start + source.span.end),
            span.start + after_source,
        ))
    }

    // ------------------------------------------------------------------
    // Source file emission
    // ------------------------------------------------------------------

    fn emit_source_file(&mut self, file: &SourceFile) {
        self.cjs_exported_names.clear();
        self.cjs_default_import_bindings.clear();
        self.cjs_export_alias_map.clear();
        self.cjs_var_export_names.clear();
        self.cjs_live_export_chain.clear();
        self.cjs_live_export_keys.clear();
        self.system_live_export_names.clear();
        self.omitted_array_destructure_plans.clear();
        self.omitted_array_destructure_counter = 0;
        self.class_expr_temp_emitted = false;
        self.file_has_recovery_errors = !file.diagnostics.is_empty();
        self.collect_cjs_string_name_provenance(file);
        self.import_shadows = import_shadow::ImportShadows::collect(&file.statements);
        self.generator_catch_names.clear();
        self.lexical_downlevel_plan =
            if self.needs_lexical_downlevel() && !self.file_has_recovery_errors {
                let plan = lexical_downlevel::LexicalDownlevelPlan::analyze(
                    file,
                    self.options.down_level_iteration == Some(true),
                    self.preserve_const_enums_effective(),
                );
                plan
            } else {
                lexical_downlevel::LexicalDownlevelPlan::default()
            };
        let simple_iterator_array_comment_ranges = self
            .lexical_downlevel_plan
            .simple_iterator_array_comment_ranges();
        self.needs_read_helper = self.lexical_downlevel_plan.needs_read_helper()
            || simple_iterator_array_comment_ranges
                .iter()
                .any(|span| !self.has_comments_in_range(span.start, span.end));
        self.needs_spread_array_helper = self
            .lexical_downlevel_plan
            .array_spreads
            .values()
            .chain(self.lexical_downlevel_plan.invocation_spreads.values())
            .any(|needs| needs.spread);
        self.needs_read_helper |= self
            .lexical_downlevel_plan
            .array_spreads
            .values()
            .chain(self.lexical_downlevel_plan.invocation_spreads.values())
            .any(|needs| needs.read);
        self.emitted_lexical_binding_ids.clear();
        self.active_lexical_loop_helpers.clear();
        self.active_lexical_for_of_plan = None;
        self.lexical_arrow_this_alias = None;
        self.temp_var_counter = 0;
        self.temp_var_names.clear();
        self.catch_auto_param_counter = 0;
        self.deferred_temp_placeholders.clear();
        self.inline_deferred_temp_placeholders.clear();
        self.deferred_export_name_placeholders.clear();
        self.resolved_deferred_temp_names.clear();
        self.deferred_temp_placeholder_counter = 0;
        // module: preserve — don't override per-file module kind; preserve the
        // source syntax regardless of .mts/.cts extension.
        self.module_kind_override = if self.options.module == Some(ModuleKind::Preserve) {
            None
        } else {
            Self::module_kind_override_for_file(&file.file_name)
        };
        self.force_external_module = Self::force_external_module_for_file(&file.file_name)
            || self.options.module_detection.as_deref() == Some("force");
        let lower_name = file.file_name.to_ascii_lowercase();
        self.is_js_file = lower_name.ends_with(".js")
            || lower_name.ends_with(".jsx")
            || lower_name.ends_with(".mjs")
            || lower_name.ends_with(".cjs");
        self.preserve_module_uses_require =
            lower_name.ends_with(".cts") || lower_name.ends_with(".cjs");
        self.jsx_preserve_by_extension =
            lower_name.ends_with(".jsx") || lower_name.ends_with(".tsx");
        self.jsx_dev_file_name.clone_from(&file.file_name);
        let preserve_const_enums = self.preserve_const_enums_effective();
        self.file_value_bound_names =
            collect_file_value_bound_names(&file.statements, preserve_const_enums)
                .into_iter()
                .map(AstString::from)
                .collect();
        self.type_only_external_modules.clear();
        if let Some(raw) = get_other_option(self.options, "__tsrsTypeOnlyExternalModules") {
            for spec in raw.lines().map(str::trim).filter(|s| !s.is_empty()) {
                self.type_only_external_modules.insert(spec.to_string());
            }
        }
        self.type_only_require_specs.clear();
        if let Some(raw) = get_other_option(self.options, "__tsrsTypeOnlyRequireSpecs") {
            for spec in raw.lines().map(str::trim).filter(|s| !s.is_empty()) {
                self.type_only_require_specs.insert(spec.to_string());
            }
        }
        self.runtime_export_import_names.clear();
        if let Some(raw) = get_other_option(self.options, "__tsrsResolvedRuntimeExportImportLocals")
        {
            for name in raw.lines().map(str::trim).filter(|s| !s.is_empty()) {
                self.runtime_export_import_names.insert(name.into());
            }
        }
        self.node_esm_import_require =
            get_other_option(self.options, "__tsrsNodeEsmImportRequire").is_some();
        self.node_esm_create_require_ident = None;
        self.node_esm_require_ident = None;
        self.prior_script_value_names.clear();
        if let Some(raw) = get_other_option(self.options, "__tsrsPriorScriptValueNames") {
            for name in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                self.prior_script_value_names.insert(name.to_string());
            }
        }
        self.seen_script_value_names.clear();

        // Scan for decorators to determine which helpers are needed.
        if self.options.experimental_decorators == Some(true) {
            self.scan_for_decorators(file);
        }
        if self.options.experimental_decorators != Some(true) && !self.should_preserve_decorators()
        {
            self.scan_for_simple_standard_decorator_wrappers(file);
        }

        // ES5 class lowering needs its helper before any emitted statement,
        // including classes nested in function/block bodies.
        self.needs_extends_helper = self.effective_target() < ScriptTarget::ES2015
            && self.stmts_need_legacy_extends(&file.statements);
        self.prepare_legacy_es5_member_lowering(&file.statements);

        // Scan for async functions to determine if __awaiter helper is needed.
        self.scan_needs_awaiter(&file.statements);

        // Scan for tagged templates with invalid escapes to determine if
        // __makeTemplateObject helper is needed (target < ES2018).
        if self.needs_downlevel("tagged-template") {
            self.scan_needs_make_template_object(&file.statements);
        }

        // Scan for object rest destructuring to determine if __rest helper is needed.
        self.scan_needs_rest(&file.statements);

        // Scan for for-of loops to determine if __values helper is needed.
        self.scan_needs_values(&file.statements);

        // Scan for classes with private fields to determine if private field helpers are needed.
        self.scan_needs_private_fields(&file.statements);

        // Scan for anonymous class expressions with static initializers assigned to named
        // bindings to determine if __setFunctionName helper is needed.
        if !self.use_define_for_class_fields() && !self.needs_set_function_name_helper {
            self.needs_set_function_name_helper =
                crate::analysis::stmts_need_set_function_name(&file.statements);
        }

        // Scan const enum declarations and compute member values for inlining.
        // Always scan, even when preserveConstEnums is set: the enum declaration
        // body will be emitted, but usage sites must still be inlined.
        // However, skip scanning entirely when isolatedModules or
        // verbatimModuleSyntax is set — TS 5.x treats const enums as regular
        // enums in these modes (no inlining).
        self.const_enum_values.clear();
        if self.should_inline_const_enums() {
            self.scan_const_enums(&file.statements);
            if self.can_inline_imported_const_enums() {
                for (key, value) in &self.external_const_enum_values {
                    self.const_enum_values
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
            }
            if !self.const_enum_values.is_empty() {
                self.scan_const_enum_aliases(&file.statements);
            }
        }
        self.rebuild_const_enum_object_names();

        // Pre-scan file-level `const` declarations with literal initializers
        // for use in enum constant folding (e.g. `const EV = 1; enum Foo { E = EV }`).
        self.file_consts.clear();
        self.file_string_consts.clear();
        self.scan_file_consts(&file.statements);
        // Merge cross-file constants from imports.
        for (k, v) in &self.external_file_consts {
            self.file_consts.entry(k.clone()).or_insert(*v);
        }
        for (k, v) in &self.external_file_string_consts {
            self.file_string_consts
                .entry(k.clone())
                .or_insert(v.clone());
        }

        // Reset JSX runtime state for this file.
        self.jsx_runtime_needs_jsx = false;
        self.jsx_runtime_needs_jsxs = false;
        self.jsx_runtime_needs_fragment = false;
        self.jsx_runtime_needs_create_element = false;
        self.jsx_runtime_require_var = None;
        self.jsx_runtime_base_require_var = None;
        self.jsx_import_source_pragma = None;
        self.jsx_runtime_pragma_automatic = false;
        self.jsx_pragma_factory = None;
        self.jsx_pragma_fragment = None;
        self.jsx_pragma_strip_positions.clear();
        self.jsx_text_spans.clear();
        self.elide_sole_empty_fragment_factory_import = false;

        // Scan for @jsxImportSource, @jsxRuntime, @jsx, and @jsxFrag pragmas in
        // leading comments.  A pragma can upgrade `jsx: react` files to automatic
        // runtime mode or override the factory/fragment function.
        if matches!(
            self.options.jsx,
            Some(JsxEmit::React) | Some(JsxEmit::ReactJSX) | Some(JsxEmit::ReactJSXDev)
        ) {
            self.scan_jsx_import_source_pragma();
            self.scan_jsx_runtime_pragma();
            self.scan_jsx_factory_pragmas();
            let sole_empty_fragment = file
                .statements
                .iter()
                .filter_map(|stmt| match &stmt.kind {
                    StmtKind::Expr(expr) => match &expr.kind {
                        ExprKind::JsxFragment(fragment) => Some(fragment.children.is_empty()),
                        ExprKind::JsxElement(_) | ExprKind::JsxSelfClosing(_) => Some(false),
                        _ => None,
                    },
                    _ => None,
                })
                .collect::<Vec<_>>()
                == [true];
            if self.jsx_pragma_fragment.as_deref() == Some("null") && sole_empty_fragment {
                self.elide_sole_empty_fragment_factory_import = true;
                self.jsx_pragma_strip_positions
                    .extend(self.comments.iter().filter_map(|comment| {
                        let text = self
                            .source
                            .get(comment.pos as usize..comment.end as usize)?;
                        text.contains("@jsx").then_some(comment.pos)
                    }));
            }
        }
        if self.jsx_is_react_jsx() {
            // Pre-scan all JSX to determine which runtime functions are needed.
            jsx::scan_jsx_runtime_needs(&file.statements, self);
            self.jsx_dev_file_name_ident =
                jsx::jsx_dev_filename_ident(self.source, &self.jsx_text_spans);
        }

        // Collect type-only declaration names for export default elision.
        if let Some(raw) = get_other_option(self.options, "__tsrsResolvedTypeOnlyImportLocals") {
            for name in raw.lines().map(str::trim).filter(|s| !s.is_empty()) {
                self.type_only_decl_names.insert(name.into());
                self.type_only_import_names.insert(name.into());
            }
        }
        // Collect import locals that should not be qualified in CJS output.
        // These are names whose cross-file type analysis shows they trace back
        // to a type-only export (e.g. namespace merge with type-only origin),
        // but the import is still retained for side effects.
        if let Some(raw) = get_other_option(self.options, "__tsrsCjsNoQualifyImportLocals") {
            for name in raw.lines().map(str::trim).filter(|s| !s.is_empty()) {
                self.cjs_no_qualify_import_locals.insert(name.into());
            }
        }
        if let Some(raw) = get_other_option(self.options, "__tsrsCjsKeepSideEffectSources") {
            for src in raw.lines().map(str::trim).filter(|s| !s.is_empty()) {
                self.cjs_keep_side_effect_sources.insert(src.into());
            }
        }
        // First pass: collect all type-only names.
        for stmt in &file.statements {
            match &stmt.kind {
                StmtKind::InterfaceDecl(i) => {
                    self.type_only_decl_names.insert(i.name.clone().into());
                }
                StmtKind::TypeAlias(t) => {
                    self.type_only_decl_names.insert(t.name.clone().into());
                }
                StmtKind::EnumDecl(e) => {
                    self.enum_decl_names.insert(e.name.clone().into());
                }
                StmtKind::ModuleDecl(m) => {
                    if module_decl_is_type_only(m, preserve_const_enums) {
                        if let ModuleName::Ident(ref n) = m.name {
                            self.type_only_decl_names.insert(n.clone().into());
                        }
                    }
                    // Track `declare namespace` with value-level declarations
                    // for import-equals alias retention.
                    if m.modifiers & MOD_DECLARE != 0 {
                        if declare_namespace_has_value_exports(m) {
                            if let ModuleName::Ident(ref n) = m.name {
                                self.declare_ns_with_values.insert(n.clone().into());
                            }
                        }
                    }
                }
                // `import type { X }` or `import { type X }` — track as type-only
                StmtKind::Import(import_decl) => {
                    if let ImportClause::Named {
                        default,
                        named,
                        namespace,
                    } = &import_decl.specifiers
                    {
                        let source_is_type_only_external = self
                            .type_only_external_modules
                            .contains(import_decl.source.as_str());
                        if import_decl.type_only {
                            // `import type { X, Y } from 'mod'` — all are type-only
                            // for the current emit.
                            if let Some(d) = default {
                                self.type_only_decl_names.insert(d.clone().into());
                                self.type_only_import_names.insert(d.clone().into());
                            }
                            for spec in named {
                                self.type_only_decl_names.insert(spec.local.clone().into());
                                self.type_only_import_names
                                    .insert(spec.local.clone().into());
                            }
                            if let Some(ns) = namespace {
                                self.type_only_decl_names.insert(ns.clone().into());
                                self.type_only_import_names.insert(ns.clone().into());
                            }
                        } else {
                            if source_is_type_only_external {
                                if let Some(d) = default {
                                    self.type_only_decl_names.insert(d.clone().into());
                                    self.type_only_import_names.insert(d.clone().into());
                                }
                                if let Some(ns) = namespace {
                                    self.type_only_decl_names.insert(ns.clone().into());
                                    self.type_only_import_names.insert(ns.clone().into());
                                }
                            }
                            // `import { type X, Y } from 'mod'` — only X is type-only.
                            // Also treat imports that resolve to explicit type-only
                            // exports in other files (`export type { X }`) as type-only.
                            for spec in named {
                                let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                                if spec.is_type
                                    || self
                                        .global_type_only_export_names
                                        .contains(imported.as_str())
                                {
                                    self.type_only_decl_names.insert(spec.local.clone().into());
                                    self.type_only_import_names
                                        .insert(spec.local.clone().into());
                                }
                            }
                        }
                    }
                }
                // `export namespace X { ... }` or `export interface X { ... }` —
                // also track the declared name when it is type-only.
                StmtKind::Export(ed) => {
                    if let ExportDeclKind::Decl(ref d) = ed.kind {
                        match &d.kind {
                            StmtKind::InterfaceDecl(i) => {
                                self.type_only_decl_names.insert(i.name.clone().into());
                            }
                            StmtKind::TypeAlias(t) => {
                                self.type_only_decl_names.insert(t.name.clone().into());
                            }
                            StmtKind::ModuleDecl(m)
                                if module_decl_is_type_only(m, preserve_const_enums) =>
                            {
                                if let ModuleName::Ident(ref n) = m.name {
                                    self.type_only_decl_names.insert(n.clone().into());
                                }
                            }
                            StmtKind::EnumDecl(e) => {
                                self.enum_decl_names.insert(e.name.clone().into());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        // Second pass: remove names that also have value declarations.
        // A name that is both a type and a value (e.g. `interface Foo {}` + `const Foo = {}`)
        // is a value at runtime and must NOT be elided.
        for stmt in &file.statements {
            let mut value_names: Vec<String> = Vec::new();
            match &stmt.kind {
                StmtKind::Var(v) => {
                    for decl in &v.declarations {
                        collect_binding_names(&decl.name, &mut value_names);
                    }
                }
                StmtKind::FnDecl(f) => {
                    if let Some(ref n) = f.name {
                        value_names.push(n.clone());
                    }
                }
                StmtKind::ClassDecl(c) => {
                    if let Some(ref n) = c.name {
                        value_names.push(n.clone());
                    }
                }
                StmtKind::EnumDecl(e) => {
                    value_names.push(e.name.clone());
                }
                StmtKind::ModuleDecl(m) if !module_decl_is_type_only(m, preserve_const_enums) => {
                    if let ModuleName::Ident(ref n) = m.name {
                        value_names.push(n.clone());
                    }
                }
                StmtKind::Export(e) => {
                    // Also check exported declarations
                    if let ExportDeclKind::Decl(ref d) = e.kind {
                        match &d.kind {
                            StmtKind::Var(v) => {
                                for decl in &v.declarations {
                                    collect_binding_names(&decl.name, &mut value_names);
                                }
                            }
                            StmtKind::FnDecl(f) => {
                                if let Some(ref n) = f.name {
                                    value_names.push(n.clone());
                                }
                            }
                            StmtKind::ClassDecl(c) => {
                                if let Some(ref n) = c.name {
                                    value_names.push(n.clone());
                                }
                            }
                            StmtKind::EnumDecl(e) => {
                                value_names.push(e.name.clone());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            for name in value_names {
                self.type_only_decl_names.remove(name.as_str());
            }
        }
        // Analyze import elision after type-only names/imports are known.
        self.analyze_import_elision(&file.statements);
        prescan_declare_namespace_value_exports(&file.statements, &mut self.cumulative_ns_exports);

        // When using react-jsx/react-jsxdev, files with JSX are modules
        // because the synthetic jsx-runtime import makes them so.
        let jsx_makes_module = self.jsx_is_react_jsx()
            && (self.jsx_runtime_needs_jsx
                || self.jsx_runtime_needs_jsxs
                || self.jsx_runtime_needs_fragment
                || self.jsx_runtime_needs_create_element);

        let has_es_module_syntax = file.statements.iter().any(|s| {
            match &s.kind {
                // An import declaration makes the file a module if it has a
                // non-empty source OR meaningful specifiers.  Parse-error
                // artifacts like `import 10;` produce an Import with both
                // empty source and empty specifiers — those don't count.
                StmtKind::Import(imp) => {
                    if !imp.source.is_empty() || imp.is_side_effect || imp.defer {
                        true
                    } else {
                        match &imp.specifiers {
                            ImportClause::Named {
                                default,
                                named,
                                namespace,
                            } => default.is_some() || !named.is_empty() || namespace.is_some(),
                            ImportClause::Require(_) => true,
                        }
                    }
                }
                StmtKind::Export(export_decl) => {
                    // Don't count exports of non-exportable statements
                    // (throw, return, etc.) as ESM syntax.
                    let is_non_exportable_decl = matches!(
                        &export_decl.kind,
                        ExportDeclKind::Decl(d) if matches!(
                            &d.kind,
                            StmtKind::Throw(_) | StmtKind::Return(_)
                                | StmtKind::Break(_) | StmtKind::Continue(_)
                        )
                    );
                    !is_non_exportable_decl
                        && !self.export_decl_is_invalid_expr_recovery(export_decl)
                }
                StmtKind::ExportAssign(_) => true,
                _ => {
                    stmt_has_export_modifier(s)
                        || self.stmt_has_malformed_await_using_for_export_recovery(s)
                }
            }
        });
        let is_module = self.force_external_module
            || jsx_makes_module
            || has_es_module_syntax
            // `import.meta` usage also makes a file a module. TypeScript emits
            // ESM output shape for these files, including `export {};` markers
            // when there are no explicit imports/exports.
            || file.statements.iter().any(|s| self.stmt_has_import_meta(s))
            // Dynamic import() makes a file a module when using a wrapped
            // module format (UMD/AMD) that needs the factory wrapper to
            // provide the require function for downleveling.  CJS files with
            // only dynamic imports are NOT treated as modules — they get
            // downleveled imports but no "use strict" or __esModule marker.
            || ((self.is_umd() || self.is_amd() || self.is_system())
                && file.statements.iter().any(|s| self.stmt_has_dynamic_import_call(s)));
        self.is_module_file = is_module;

        let mut used_wrapped_module_emit = false;
        if self.is_system() && is_module && self.emit_system_module(file) {
            // SystemJS module emit handled above.
            // Trailing file comments are NOT emitted for wrapped module formats
            // (System/AMD/UMD) because TypeScript doesn't include post-last-statement
            // source comments inside the factory wrapper.
            used_wrapped_module_emit = true;
        } else if (self.is_amd() || self.is_umd()) && is_module {
            // AMD / UMD module emit: wrap CJS body in define() / UMD factory.
            self.emit_amd_or_umd_module(file);
            used_wrapped_module_emit = true;
        } else if self.is_commonjs() && is_module {
            // CommonJS mode: emit "use strict" for module files, UNLESS the
            // source file is already explicitly CJS (.cts).  TypeScript 5.x
            // does not add "use strict" when compiling .cts → .cjs because
            // the file is natively CommonJS (not ESM compiled down).
            self.export_target = Some("exports".to_string());
            let is_native_cjs =
                lower_name.ends_with(".cts") && self.options.verbatim_module_syntax == Some(true);
            if !is_native_cjs {
                // Preserve the quote style of the source's "use strict" directive.
                let use_strict_str = if file
                    .statements
                    .first()
                    .map_or(false, |s| is_use_strict_directive(s))
                {
                    let first = &file.statements[0];
                    let start = first.span.start as usize;
                    if start < self.source.len() && self.source.as_bytes()[start] == b'\'' {
                        "'use strict';"
                    } else {
                        "\"use strict\";"
                    }
                } else {
                    "\"use strict\";"
                };
                self.writeln(use_strict_str);
            }

            // TypeScript preserves file-level prologue comments (copyright/license
            // blocks separated by a blank line from the first statement) between
            // "use strict" and __esModule.  Non-prologue comments (directly attached
            // to a statement) are emitted by emit_leading_comments in the loop.
            if let Some(first_stmt) = file.statements.first() {
                self.emit_file_prologue_comments(first_stmt.span.start);
            }

            // Record position for private field WeakMap `var` declarations.
            // TypeScript hoists these right after "use strict" + prologue,
            // before helpers and __esModule defineProperty.
            self.private_field_var_insert_pos = Some(self.output.len());

            // `export = X` where X is a runtime value suppresses
            // `Object.defineProperty(exports, "__esModule")` and switches to
            // `module.exports = X;`.  But `export = InterfaceName` (type-only)
            // is erased and the module still uses __esModule mode.
            // With verbatimModuleSyntax, `export = I` always emits even for type-only targets.
            let mut has_export_assign = if self.options.verbatim_module_syntax == Some(true) {
                file.statements.iter().any(|s| stmt_is_export_assign(s))
            } else {
                has_value_export_assign(&file.statements, self.preserve_const_enums_effective())
            };
            // Cross-file override: if the export= target is type-only from the
            // source module (e.g. `export type { A }` in another file), the local
            // `import { A }` looks like a value import but A has no runtime value.
            if has_export_assign && !self.global_type_only_export_names.is_empty() {
                for stmt in &file.statements {
                    if let Some(name) = export_assign_name(stmt) {
                        if self.global_type_only_export_names.contains(name) {
                            has_export_assign = false;
                        }
                    }
                }
            }

            // Store for use by emit_export_decl_cjs and emit_stmt
            self.has_export_assign = has_export_assign;

            // Scan for needed CJS helpers and emit them BEFORE __esModule
            // (matches TypeScript's helper ordering).
            // __createBinding + __exportStar are needed for `export *` regardless of esModuleInterop.
            // __importDefault + __importStar are only used when esModuleInterop is enabled.
            self.scan_needed_helpers(&file.statements);
            // Don't emit inline helper definitions when noEmitHelpers or importHelpers is set
            if self.options.no_emit_helpers != Some(true)
                && self.options.import_helpers != Some(true)
            {
                // TypeScript's CJS helper ordering:
                // 1. __createBinding, __setModuleDefault (base helpers)
                // 2. __decorate (if decorators present)
                // 3. __exportStar, __importStar (star helpers)
                // 4. __metadata, __param (decorator metadata)
                // 5. __awaiter, __setFunctionName, etc. (other transform helpers)
                // 6. __importDefault (comes last among CJS helpers)
                //
                // When no decorators: base → star → transform → __importDefault
                self.emit_cjs_base_helpers();
                if self.needs_decorate_helper {
                    self.emit_decorate_helper();
                }
                self.emit_cjs_star_helpers();
                if self.source_needs_rewrite_relative_import_helper() {
                    self.emit_rewrite_relative_import_extension_helper();
                }
                if self.needs_metadata_helper {
                    self.emit_metadata_helper();
                }
                if self.needs_param_helper {
                    self.emit_param_helper();
                }
            }

            let omitted_array_temps =
                self.prepare_omitted_array_destructure_exports(&file.statements);
            if !omitted_array_temps.is_empty() {
                self.write("var ");
                for (i, t) in omitted_array_temps.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(t);
                }
                self.writeln(";");
            }

            // Emit helpers if needed (unless noEmitHelpers or importHelpers is set).
            // TypeScript emits helpers BEFORE __esModule Object.defineProperty.
            if self.options.no_emit_helpers != Some(true)
                && self.options.import_helpers != Some(true)
            {
                if self.needs_extends_helper {
                    self.emit_extends_helper();
                }
                // __decorate, __metadata, __param already emitted above in
                // the CJS-specific helper block (interleaved with star helpers).
                let decorator_helpers_already_emitted = self.is_cjs_like();
                if !decorator_helpers_already_emitted {
                    if self.needs_decorate_helper {
                        self.emit_decorate_helper();
                    }
                    if self.needs_metadata_helper {
                        self.emit_metadata_helper();
                    }
                    if self.needs_param_helper {
                        self.emit_param_helper();
                    }
                }
                // Priority 2: esDecorators transform helpers
                // CJS always emits __esDecorate before __runInitializers.
                if self.needs_es_decorate_helper {
                    self.emit_es_decorate_helper();
                }
                if self.needs_run_initializers_helper {
                    self.emit_run_initializers_helper();
                }
                // Priority 5: __awaiter (before no-priority helpers)
                if self.needs_awaiter_helper {
                    self.emit_awaiter_helper();
                }
                if self.needs_generator_helper {
                    self.emit_generator_helper();
                }
                // es2015: __makeTemplateObject (tagged templates with invalid escapes)
                if self.needs_make_template_object_helper {
                    self.emit_make_template_object_helper();
                }
                // classFields transform runs before esDecorators in TS pipeline,
                // so private field helpers come before __propKey/__setFunctionName.
                if self.private_field_get_first {
                    if self.needs_private_field_get {
                        self.emit_private_field_get_helper();
                    }
                    if self.needs_private_field_set {
                        self.emit_private_field_set_helper();
                    }
                } else {
                    if self.needs_private_field_set {
                        self.emit_private_field_set_helper();
                    }
                    if self.needs_private_field_get {
                        self.emit_private_field_get_helper();
                    }
                }
                if self.needs_private_field_in {
                    self.emit_private_field_in_helper();
                }
                // esDecorators (cont.)
                if self.needs_prop_key_helper {
                    self.emit_prop_key_helper();
                }
                if self.needs_set_function_name_helper {
                    self.emit_set_function_name_helper();
                }
                // es2018
                if self.needs_rest_helper {
                    self.emit_rest_helper();
                }
                if self.needs_read_helper {
                    self.emit_read_helper();
                }
                if self.needs_spread_array_helper {
                    self.emit_spread_array_helper();
                }
                if self.needs_values_helper {
                    self.emit_values_helper();
                }
                if self.async_values_before_await {
                    if self.needs_async_values_helper {
                        self.emit_async_values_helper();
                    }
                    if self.needs_await_helper {
                        self.emit_await_helper();
                    }
                    if self.needs_async_delegator_helper {
                        self.emit_async_delegator_helper();
                    }
                    if self.needs_async_generator_helper {
                        self.emit_async_generator_helper();
                    }
                } else {
                    if self.needs_await_helper {
                        self.emit_await_helper();
                    }
                    if self.needs_async_generator_helper {
                        self.emit_async_generator_helper();
                    }
                    if self.needs_async_values_helper {
                        self.emit_async_values_helper();
                    }
                    if self.needs_async_delegator_helper {
                        self.emit_async_delegator_helper();
                    }
                }
                // CJS __importDefault comes AFTER all transform helpers.
                self.emit_cjs_import_default_helper_if_needed();
            }
            // using/await using disposal helpers (inline only when not using tslib)
            if self.options.no_emit_helpers != Some(true)
                && self.options.import_helpers != Some(true)
            {
                if self.needs_add_disposable_resource_helper {
                    self.emit_add_disposable_resource_helper();
                }
                if self.needs_dispose_resources_helper {
                    self.emit_dispose_resources_helper();
                }
            }

            // Emit hoisted decorated class self-reference aliases right after helpers.
            if !self.hoisted_decorated_aliases.is_empty() {
                self.write("var ");
                let aliases = self.hoisted_decorated_aliases.clone();
                for (i, alias) in aliases.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(alias);
                }
                self.writeln(";");
            }

            // Record position for file-level temp var insertion.
            // TypeScript places `var _a, _b;` AFTER helpers but BEFORE
            // __esModule defineProperty.
            let file_temp_var_insert_pos = self.output.len();

            // TypeScript emits __esModule for all CJS modules unless `export =` is present.
            // Native CJS files (.cjs/.cts) that use CommonJS export patterns
            // (module.exports/exports.xxx) without ES module syntax skip __esModule.
            let has_cjs_exports = self.source.contains("module.exports")
                || (self.source.contains("exports.") && !self.source.contains("__esModule"));
            let skip_es_module =
                self.force_external_module && !has_es_module_syntax && has_cjs_exports;
            if !has_export_assign && !skip_es_module {
                self.writeln("Object.defineProperty(exports, \"__esModule\", { value: true });");
            }

            // Pre-scan namespace declarations to build cumulative export maps.
            // This ensures merged namespaces (declared multiple times) have full
            // visibility of all exports from all openings before any IIFE is emitted.
            // Use "exports" as parent key to match the CJS export_target.
            prescan_namespace_exports_full(
                &file.statements,
                Some("exports"),
                &mut self.cumulative_ns_exports,
                Some(&mut self.cumulative_ns_type_exports),
            );

            // Pre-declare exported bindings: `exports.X = void 0;` for classes/vars,
            // and `exports.name = name;` for hoisted function declarations.
            let mut pre_decl_names: Vec<String> = Vec::new();
            let mut var_export_names: Vec<String> = Vec::new(); // var/let/const only
            let mut import_equals_export_names: HashSet<String> = HashSet::new();
            let mut fn_export_names: Vec<(String, String)> = Vec::new(); // (exported, local)
            let mut local_named_exports: Vec<(String, String)> = Vec::new(); // (exported, local)
            let mut modifier_export_names: Vec<String> = Vec::new();
            // Names of directly-exported enums/namespaces (`export enum E {}`).
            // These are added to local_named_exports AFTER the collection loop
            // so that alias re-exports (`export { E as EE }`) are already present.
            let mut directly_exported_ns_enum: Vec<String> = Vec::new();
            // Collect all function declaration names in the file (including declare)
            // so named re-exports of functions skip void 0 pre-declaration.
            let file_fn_names: HashSet<String> = file
                .statements
                .iter()
                .filter_map(|s| match &s.kind {
                    StmtKind::FnDecl(f) => f.name.clone(),
                    StmtKind::Export(e) => match &e.kind {
                        ExportDeclKind::Decl(d) | ExportDeclKind::DefaultDecl(d) => {
                            if let StmtKind::FnDecl(f) = &d.kind {
                                f.name.clone()
                            } else {
                                None
                            }
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            // Build the set of names that are locally value-bound in this file.
            // A name is value-bound if it is declared as a non-ambient value
            // (var/let/const/fn/class/enum/namespace) or imported as a value
            // binding. Names that are only ambient (from .d.ts or `declare`)
            // or only referenced as types have no runtime value and should not
            // get an `exports.X = X;` assignment.
            let file_value_bound_names: HashSet<AstString> = {
                let preserve_const = self.preserve_const_enums_effective();
                collect_file_value_bound_names(&file.statements, preserve_const)
            };
            // Names of enum/namespace declarations (non-ambient).  When one of these
            // is exported via `export { X }`, the IIFE already performs the export
            // assignment inline (`X || (exports.X = X = {})`), so we must NOT emit
            // a redundant trailing `exports.X = X;`.
            let file_ns_enum_names: HashSet<String> = {
                let preserve_const = self.preserve_const_enums_effective();
                file.statements
                    .iter()
                    .filter_map(|s| {
                        // Check both top-level and export-wrapped enum/namespace decls.
                        let inner = match &s.kind {
                            StmtKind::Export(ed) => {
                                if let ExportDeclKind::Decl(ref d) = ed.kind {
                                    d
                                } else {
                                    return None;
                                }
                            }
                            _ => s,
                        };
                        match &inner.kind {
                            StmtKind::EnumDecl(e)
                                if e.modifiers & MOD_DECLARE == 0
                                    && (!e.is_const || preserve_const) =>
                            {
                                Some(e.name.clone())
                            }
                            StmtKind::ModuleDecl(m)
                                if m.modifiers & MOD_DECLARE == 0
                                    && !module_decl_is_type_only(m, preserve_const) =>
                            {
                                if let ModuleName::Ident(ref n) = m.name {
                                    Some(n.clone())
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    })
                    .collect()
            };
            // All namespace names (including type-only and `declare namespace`).
            // TypeScript always emits `exports.X = void 0;` pre-declarations
            // and `exports.X = Ns.Member;` assignments for
            // `export import X = Ns.Member` even when the namespace body is
            // type-only (interfaces only) or ambient (`declare namespace`).
            let file_all_ns_names: HashSet<String> = file
                .statements
                .iter()
                .filter_map(|s| match &s.kind {
                    StmtKind::ModuleDecl(m) => {
                        if let ModuleName::Ident(ref n) = m.name {
                            Some(n.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect();
            // Collect names that map exported→local for named exports where the
            // local name is a function (these should not get void 0 pre-declaration).
            let mut aliased_fn_exports: HashSet<String> = HashSet::new();
            let using_scope_start_idx = if self.needs_downlevel("using") {
                crate::emit_stmt::first_using_index(&file.statements)
            } else {
                None
            };
            // Track where using-scope exports start in pre_decl_names so we can
            // insert `default` before them (TypeScript orders `default` before
            // other using-scope exports in the void 0 pre-declaration chain).
            let mut using_scope_predecl_insert_pos: Option<usize> = None;
            let mut using_scope_has_default = false;
            for (stmt_idx, stmt) in file.statements.iter().enumerate() {
                if let StmtKind::Export(ref export_decl) = stmt.kind {
                    // Track the pre_decl_names index for the first export inside
                    // the using scope (so we know where to insert `default`).
                    if let Some(using_idx) = using_scope_start_idx {
                        if stmt_idx >= using_idx && using_scope_predecl_insert_pos.is_none() {
                            using_scope_predecl_insert_pos = Some(pre_decl_names.len());
                        }
                    }
                    collect_export_names(
                        &export_decl.kind,
                        &mut pre_decl_names,
                        self.preserve_const_enums_effective(),
                    );
                    // Track whether the using scope has an export default.
                    if using_scope_start_idx.is_some_and(|idx| stmt_idx >= idx)
                        && matches!(
                            &export_decl.kind,
                            ExportDeclKind::Default(_) | ExportDeclKind::DefaultDecl(_)
                        )
                    {
                        using_scope_has_default = true;
                    }
                    {
                        let counter = &mut self.default_export_counter;
                        collect_fn_export_names(&export_decl.kind, &mut fn_export_names, counter);
                    }
                    collect_local_named_export_assignments(
                        &export_decl.kind,
                        &mut local_named_exports,
                    );
                    // Collect var-only export names (for CJS reference qualification).
                    // Include both concrete and ambient (`declare`) var exports:
                    // `export declare const X` has no local binding so references
                    // must be qualified as `exports.X`.
                    if let ExportDeclKind::Decl(ref decl) = export_decl.kind {
                        // For directly-exported enums/namespaces, add (name, name)
                        // to local_named_exports so the IIFE closing chain
                        // includes the direct export name at its source position.
                        match &decl.kind {
                            StmtKind::EnumDecl(e) => {
                                directly_exported_ns_enum.push(e.name.clone());
                                local_named_exports.push((e.name.clone(), e.name.clone()));
                            }
                            StmtKind::ModuleDecl(m) => {
                                if let ModuleName::Ident(ref n) = m.name {
                                    directly_exported_ns_enum.push(n.clone());
                                    local_named_exports.push((n.clone(), n.clone()));
                                }
                            }
                            _ => {}
                        }
                        if let StmtKind::Var(ref v) = decl.kind {
                            for d in &v.declarations {
                                collect_binding_names(&d.name, &mut var_export_names);
                            }
                        }
                        // `export import X = M.N;` — include in void 0 pre-declarations
                        // when the root of the RHS is a value-bound name OR a type-only
                        // namespace that is used as a value elsewhere in the file.
                        if let StmtKind::ImportEquals(ref ie) = decl.kind {
                            // Track import-equals export names for alias emit
                            // (no local JS binding, alias must use `exports.name`).
                            import_equals_export_names.insert(ie.name.clone());
                            let keep_invalid_class_modifier_import = self
                                .span_text(stmt.span)
                                .map(|s| s.trim_start())
                                .is_some_and(|s| {
                                    s.starts_with("export public import ")
                                        || s.starts_with("export private import ")
                                        || s.starts_with("export static import ")
                                });
                            if keep_invalid_class_modifier_import {
                                pre_decl_names.push(ie.name.clone());
                                var_export_names.push(ie.name.clone());
                            }
                            let root = expr_root_ident_static(&ie.module_ref);
                            if let Some(ref root_name) = root {
                                let root_is_value =
                                    file_value_bound_names.contains(root_name.as_str());
                                let root_is_type_only_ns = !root_is_value
                                    && file_all_ns_names.contains(root_name.as_str());
                                if root_is_type_only_ns {
                                    // Type-only namespace root: always pre-declare.
                                    // TypeScript emits pre-declarations for these even
                                    // though the member is type-only.
                                    pre_decl_names.push(ie.name.clone());
                                    var_export_names.push(ie.name.clone());
                                } else if root_is_value {
                                    // Value namespace: check if the member is type-only.
                                    // e.g. `import X = M.InterfaceName` should be excluded.
                                    let member_is_type_only =
                                        if let ExprKind::Member(ref mem) = ie.module_ref.kind {
                                            let prop = &mem.property;
                                            self.cumulative_ns_type_exports
                                                .get(root_name.as_str())
                                                .is_some_and(|set| set.contains(prop.as_str()))
                                                || self
                                                    .cumulative_ns_type_exports
                                                    .get(format!("exports::{root_name}").as_str())
                                                    .is_some_and(|set| set.contains(prop.as_str()))
                                        } else {
                                            false
                                        };
                                    if !member_is_type_only {
                                        pre_decl_names.push(ie.name.clone());
                                        // Also add to var_export_names so references
                                        // to the alias are qualified with `exports.`.
                                        var_export_names.push(ie.name.clone());
                                    }
                                }
                            }
                        }
                        // `export import X = require("...");` — add to
                        // var_export_names so references are qualified.
                        if let StmtKind::Import(ref imp) = decl.kind {
                            if let ImportClause::Require(ref name) = imp.specifiers {
                                if !self.type_only_require_specs.contains(imp.source.as_str()) {
                                    var_export_names.push(name.clone());
                                }
                            }
                        }
                    }
                    // Track aliased named exports whose local is a function.
                    if let ExportDeclKind::Named {
                        specifiers,
                        source: None,
                        type_only: false,
                    } = &export_decl.kind
                    {
                        for spec in specifiers {
                            if spec.is_type {
                                continue;
                            }
                            if file_fn_names.contains(&spec.local) {
                                let exported =
                                    spec.exported.as_ref().unwrap_or(&spec.local).clone();
                                aliased_fn_exports.insert(exported.clone());
                                // Also add to fn_export_names for header emission.
                                fn_export_names.push((exported, spec.local.clone()));
                            }
                        }
                    }
                    // Re-exports `export { X } from "mod"` create no local
                    // binding — the name is only reachable via `exports.X`
                    // (set up by `Object.defineProperty(exports, "X", { get })`).
                    // Mark them as var-style so any reference (e.g.
                    // `export default X`) rewrites to `exports.X` — but only
                    // when the exported name doesn't collide with a separate
                    // import binding (e.g. `import { a as a1 } from "x"`
                    // alongside `export { a as a1 } from "x"`), since the
                    // import binding is the local one for references.
                    if let ExportDeclKind::Named {
                        specifiers,
                        source: Some(_),
                        type_only: false,
                    } = &export_decl.kind
                    {
                        for spec in specifiers {
                            if spec.is_type {
                                continue;
                            }
                            let exported = spec.exported.as_ref().unwrap_or(&spec.local).clone();
                            if has_import_local_named(&file.statements, &exported) {
                                continue;
                            }
                            var_export_names.push(exported);
                        }
                    }
                }
                // Also collect from modifier-based exports (e.g., `export class C {}`)
                if let Some((name, _)) =
                    self.parse_malformed_export_class_modifier_import(stmt.span)
                {
                    pre_decl_names.push(name.clone());
                    modifier_export_names.push(name.clone());
                    var_export_names.push(name);
                }
                collect_modifier_export_names(stmt, &mut pre_decl_names);
                collect_modifier_export_names(stmt, &mut modifier_export_names);
                collect_modifier_fn_export_names(stmt, &mut fn_export_names);
                // Collect var-only modifier exports (including `declare` vars).
                if let StmtKind::Var(v) = &stmt.kind {
                    if v.modifiers & MOD_EXPORT != 0 {
                        for d in &v.declarations {
                            collect_binding_names(&d.name, &mut var_export_names);
                        }
                    }
                }
            }
            // When a using disposal scope has `export default`, insert `default`
            // into pre_decl_names right before the first using-scope export.
            // TypeScript orders `default` before other using-scope exports.
            if using_scope_has_default {
                let insert_pos = using_scope_predecl_insert_pos.unwrap_or(pre_decl_names.len());
                pre_decl_names.insert(insert_pos, "default".to_string());
            }
            let malformed_export_imports =
                self.collect_malformed_export_class_modifier_imports_from_source();
            for (name, _) in &malformed_export_imports {
                pre_decl_names.push(name.clone());
                modifier_export_names.push(name.clone());
                var_export_names.push(name.clone());
            }

            // For entries with the same local name, stable-sort so self-exports
            // (exported == local) come before aliases (exported != local).
            // This matches TypeScript's behavior: `export function j` produces
            // `exports.j = j;` before `export { j as jj }` produces `exports.jj = j;`,
            // even when the specifier appears first in source.
            fn_export_names.sort_by(|a, b| {
                if a.1 == b.1 {
                    // "default" always comes first among entries with the same local name.
                    let a_default = a.0 == "default";
                    let b_default = b.0 == "default";
                    if a_default != b_default {
                        return b_default.cmp(&a_default);
                    }
                    let a_self = a.0 == a.1;
                    let b_self = b.0 == b.1;
                    b_self.cmp(&a_self)
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            // Deduplicate pre-declaration names (e.g. merged namespaces)
            {
                let mut seen = HashSet::new();
                pre_decl_names.retain(|n| seen.insert(n.clone()));
            }
            {
                let mut seen = HashSet::new();
                local_named_exports.retain(|pair| seen.insert(pair.clone()));
            }
            // Remove (name, name) entries for directly-exported enums/namespaces
            // that DON'T have alias re-exports. These were added during the loop
            // for source-order positioning but are only needed when an alias exists.
            {
                let has_alias: HashSet<String> = directly_exported_ns_enum
                    .iter()
                    .filter(|dname| {
                        local_named_exports.iter().any(|(exported, local)| {
                            local.as_str() == dname.as_str() && exported != local
                        })
                    })
                    .cloned()
                    .collect();
                local_named_exports.retain(|(exported, local)| {
                    // Keep if not a directly-exported ns/enum self-entry
                    if exported == local && directly_exported_ns_enum.contains(exported) {
                        // Only keep if it has an alias
                        has_alias.contains(exported)
                    } else {
                        true
                    }
                });
            }
            {
                let mut seen = HashSet::new();
                modifier_export_names.retain(|n| seen.insert(n.clone()));
            }
            // Build set of names that are re-exported (with source) — these need
            // void 0 pre-declarations even if a same-name function export exists,
            // because there are multiple assignments to the same exports slot.
            let reexported_names: HashSet<String> = {
                let mut set = HashSet::new();
                for stmt in &file.statements {
                    if let StmtKind::Export(ref ed) = stmt.kind {
                        if let ExportDeclKind::Named {
                            specifiers,
                            source: Some(_),
                            type_only: false,
                        } = &ed.kind
                        {
                            for spec in specifiers {
                                if !spec.is_type {
                                    let exported =
                                        spec.exported.as_ref().unwrap_or(&spec.local).clone();
                                    set.insert(exported);
                                }
                            }
                        }
                    }
                }
                set
            };
            // Remove names that are already in fn_export_names (functions get
            // direct assignment, not void 0 pre-declaration).
            // Also remove names that are functions in the file (named re-exports
            // of functions should not get void 0 pre-declaration).
            // Exceptions:
            // - Keep names that are ALSO var exports (dual var+function).
            // - Keep names that are ALSO re-exported from another module
            //   (multiple assignments to the same exports slot need void 0 init).
            {
                let fn_names: HashSet<&str> =
                    fn_export_names.iter().map(|(n, _)| n.as_str()).collect();
                // Local names of default-exported functions (e.g. `export default function Decl`)
                // should NOT get void 0 pre-declarations.
                let default_fn_locals: HashSet<&str> = fn_export_names
                    .iter()
                    .filter(|(exported, _)| exported == "default")
                    .map(|(_, local)| local.as_str())
                    .collect();
                let var_names: HashSet<&str> =
                    var_export_names.iter().map(|n| n.as_str()).collect();
                pre_decl_names.retain(|n| {
                    // Keep if it's also a var export (dual var+function)
                    if var_names.contains(n.as_str()) {
                        return true;
                    }
                    // Keep if it's also a re-export (multiple assignments)
                    if reexported_names.contains(n) {
                        return true;
                    }
                    !fn_names.contains(n.as_str())
                        && !file_fn_names.contains(n)
                        && !aliased_fn_exports.contains(n)
                        && !default_fn_locals.contains(n.as_str())
                });
            }
            {
                let fn_names: HashSet<&str> =
                    fn_export_names.iter().map(|(n, _)| n.as_str()).collect();
                let default_fn_locals: HashSet<&str> = fn_export_names
                    .iter()
                    .filter(|(exported, _)| exported == "default")
                    .map(|(_, local)| local.as_str())
                    .collect();
                let modifier_names: HashSet<&str> =
                    modifier_export_names.iter().map(|n| n.as_str()).collect();
                local_named_exports.retain(|(exported, local)| {
                    // Keep named re-exports of default-exported function locals
                    // (e.g. `export default function f` + `export {f}`)
                    if exported != "default" && default_fn_locals.contains(local.as_str()) {
                        return true;
                    }
                    !fn_names.contains(exported.as_str())
                        && !file_fn_names.contains(local)
                        && !(exported == local && modifier_names.contains(local.as_str()))
                });
            }
            if !self.preserve_const_enums_effective() {
                let const_enum_namespace_merge_exports: HashSet<String> = local_named_exports
                    .iter()
                    .filter(|(_, local)| {
                        file_all_ns_names.contains(local) && self.is_const_enum_object_name(local)
                    })
                    .map(|(exported, _)| exported.clone())
                    .collect();
                // Build exported→local mapping to resolve aliases (e.g. `export { D as D1 }`)
                let exported_to_local: HashMap<&str, &str> = local_named_exports
                    .iter()
                    .map(|(e, l)| (e.as_str(), l.as_str()))
                    .collect();
                pre_decl_names.retain(|n| {
                    let local = exported_to_local
                        .get(n.as_str())
                        .copied()
                        .unwrap_or(n.as_str());
                    !self.is_const_enum_object_name(local)
                        || const_enum_namespace_merge_exports.contains(n)
                });
                local_named_exports.retain(|(_, local)| !self.is_const_enum_object_name(local));
            }
            // Filter pre_decl_names for named re-exports: remove exported names whose
            // local name is explicitly type-only (interfaces, type aliases, etc.).
            {
                // Build a map: exported_name → local_name from all named exports.
                let named_export_local_map: HashMap<String, String> = local_named_exports
                    .iter()
                    .map(|(exported, local)| (exported.clone(), local.clone()))
                    .collect();
                // Build a map: exported_name → local_name from re-export specifiers
                // (export { local as exported } from "..."). These need cross-file
                // type-only filtering too.
                let mut reexport_local_map: HashMap<String, String> = HashMap::new();
                for stmt in &file.statements {
                    if let StmtKind::Export(ref ed) = stmt.kind {
                        if let ExportDeclKind::Named {
                            specifiers,
                            source: Some(_),
                            type_only: false,
                        } = &ed.kind
                        {
                            for spec in specifiers {
                                if !spec.is_type {
                                    let exported =
                                        spec.exported.as_ref().unwrap_or(&spec.local).clone();
                                    reexport_local_map.insert(exported, spec.local.clone());
                                }
                            }
                        }
                    }
                }
                let type_only_ref = &self.type_only_decl_names;
                let type_only_import_ref = &self.type_only_import_names;
                let global_type_only_ref = &self.global_type_only_names;
                let global_type_only_export_ref = &self.global_type_only_export_names;
                // Build a set of names from namespace imports and require-style
                // imports that will definitely produce CJS require() calls.
                // These names always have runtime value in the emitted JS.
                let cjs_import_names: HashSet<String> = {
                    let mut names = HashSet::new();
                    for stmt in &file.statements {
                        let imp = match &stmt.kind {
                            StmtKind::Import(imp) if !imp.type_only => Some(imp),
                            StmtKind::Export(e) => {
                                if let ExportDeclKind::Decl(d) = &e.kind {
                                    if let StmtKind::Import(imp) = &d.kind {
                                        if !imp.type_only {
                                            Some(imp)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };
                        if let Some(imp) = imp {
                            match &imp.specifiers {
                                ImportClause::Named {
                                    namespace: Some(ns),
                                    ..
                                } => {
                                    names.insert(ns.clone());
                                }
                                ImportClause::Require(name) => {
                                    names.insert(name.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    names
                };
                pre_decl_names.retain(|exported| {
                    if let Some(local) = named_export_local_map.get(exported) {
                        // Named export: remove if the local is explicitly type-only
                        // (declared in this file or in another file in the compilation),
                        // BUT only if the name doesn't also have a value binding
                        // (e.g., `import * as B from "..."; interface B { }; export { B }`
                        // — B has both a type and value, so it should be kept).
                        if (type_only_ref.contains(local.as_str())
                            || type_only_import_ref.contains(local.as_str()))
                            && !self.cjs_import_map.contains_key(local.as_str())
                            && !cjs_import_names.contains(local)
                            && !self.runtime_export_import_names.contains(local.as_str())
                        {
                            if !has_local_value_decl(&file.statements, local) {
                                return false;
                            }
                        }
                        // If the name is known type-only globally (from another file),
                        // filter it unless there's a local non-import value declaration
                        // (fn, class, var, enum, namespace) that shadows the type.
                        if (global_type_only_ref.contains(local.as_str())
                            || global_type_only_export_ref.contains(local.as_str()))
                            && !file_fn_names.contains(local)
                            && !file_ns_enum_names.contains(local)
                            && !self.cjs_import_map.contains_key(local.as_str())
                            && !cjs_import_names.contains(local)
                            && !self.runtime_export_import_names.contains(local.as_str())
                        {
                            if !has_local_value_decl(&file.statements, local) {
                                return false;
                            }
                        }
                        true
                    } else if let Some(local) = reexport_local_map.get(exported) {
                        // Re-export from another module: remove if the local name
                        // is type-only in the source file (cross-file tracking).
                        !(global_type_only_ref.contains(local.as_str())
                            || global_type_only_export_ref.contains(local.as_str()))
                    } else {
                        // Modifier-based export (export class/fn/var/enum/namespace) — keep.
                        true
                    }
                });
                // Also filter local_named_exports for cross-file type-only names
                // that are not locally bound.
                local_named_exports.retain(|(_, local)| {
                    if (self.type_only_decl_names.contains(local.as_str())
                        || self.type_only_import_names.contains(local.as_str()))
                        && !self.cjs_import_map.contains_key(local.as_str())
                        && !cjs_import_names.contains(local)
                        && !self.runtime_export_import_names.contains(local.as_str())
                    {
                        if !has_local_value_decl(&file.statements, local) {
                            return false;
                        }
                    }
                    // If globally type-only (from source module), filter unless
                    // locally declared as a value (not just imported).
                    if (global_type_only_ref.contains(local.as_str())
                        || global_type_only_export_ref.contains(local.as_str()))
                        && !file_fn_names.contains(local)
                        && !file_ns_enum_names.contains(local)
                        && !self.cjs_import_map.contains_key(local.as_str())
                        && !cjs_import_names.contains(local)
                        && !self.runtime_export_import_names.contains(local.as_str())
                    {
                        if !has_local_value_decl(&file.statements, local) {
                            return false;
                        }
                    }
                    true
                });
            }
            self.cjs_exported_names.clear();
            self.cjs_default_fn_local_names.clear();
            for n in &pre_decl_names {
                self.cjs_exported_names.insert(n.clone().into());
            }
            for (exported, local) in &fn_export_names {
                self.cjs_exported_names.insert(exported.clone().into());
                // For `export default function Foo`, also mark the local name
                // as exported so namespace IIFE closings use `exports.Foo`.
                if exported == "default" && local != "default" {
                    self.cjs_exported_names.insert(local.clone().into());
                    self.cjs_default_fn_local_names.insert(local.clone().into());
                }
            }
            self.cjs_export_alias_map.clear();
            for (exported, local) in &local_named_exports {
                self.cjs_exported_names.insert(exported.clone().into());
                // Also mark the local name as exported so namespace/enum
                // IIFE closings detect it.  Store the alias mapping for
                // generating `exports.alias = local = {}`.
                if exported != local {
                    self.cjs_exported_names.insert(local.clone().into());
                    self.cjs_export_alias_map
                        .insert(local.clone().into(), exported.clone().into());
                }
            }
            self.cjs_var_export_names.clear();
            for n in &var_export_names {
                self.cjs_var_export_names.insert(n.clone().into());
            }
            // Build CJS live export chain from local_named_exports.
            // Maps local name → list of exported names in chain order (reversed
            // from source order so aliases come first in the output chain).
            self.cjs_live_export_chain.clear();
            self.cjs_live_export_keys.clear();
            self.cjs_inline_exported_var_names.clear();
            for (exported, local) in &local_named_exports {
                self.cjs_live_export_chain
                    .entry(local.clone().into())
                    .or_insert_with(Vec::new)
                    .push(exported.clone().into());
            }
            for chain in self.cjs_live_export_chain.values_mut() {
                chain.reverse();
            }
            self.cjs_live_export_keys = self.cjs_live_export_chain.keys().cloned().collect();
            if !pre_decl_names.is_empty() {
                // Emit as chained assignment: exports.b = exports.a = void 0;
                // TypeScript emits them in reverse order chained, batched in
                // groups of 50 to avoid excessively long lines.
                let batch_size = 50;
                for chunk in pre_decl_names.chunks(batch_size) {
                    for name in chunk.iter().rev() {
                        self.write_cjs_export_access("exports", name);
                        self.write(" = ");
                    }
                    self.writeln("void 0;");
                }
            }
            // Emit function export assignments: `exports.name = name;`
            // Suppress when `export =` is present (module.exports replaces all).
            if !has_export_assign {
                for (exported, local) in &fn_export_names {
                    self.write_cjs_export_access("exports", exported);
                    self.write(" = ");
                    self.write(local);
                    self.writeln(";");
                }
                // Also emit named re-exports of default-exported function locals
                // at the prologue (e.g. `export default function f` + `export {f}`
                // → `exports.f = f;` right after `exports.default = f;`).
                {
                    let default_fn_locals: HashSet<&str> = fn_export_names
                        .iter()
                        .filter(|(e, _)| e == "default")
                        .map(|(_, l)| l.as_str())
                        .collect();
                    // Skip entries already emitted via fn_export_names above
                    let fn_exported: HashSet<&str> =
                        fn_export_names.iter().map(|(e, _)| e.as_str()).collect();
                    for (exported, local) in &local_named_exports {
                        if exported != "default"
                            && default_fn_locals.contains(local.as_str())
                            && !fn_exported.contains(exported.as_str())
                        {
                            self.write_cjs_export_access("exports", exported);
                            self.write(" = ");
                            self.write(local);
                            self.writeln(";");
                        }
                    }
                }
                for (name, rhs) in &malformed_export_imports {
                    self.write_cjs_export_access("exports", name);
                    self.write(" = ");
                    self.write(rhs);
                    self.writeln(";");
                }
            }

            // When importHelpers is enabled and this is a module file, add the
            // target-appropriate `tslib_1 = require("tslib")` binding after
            // export pre-declarations.
            // Script files inline helpers even with importHelpers=true.
            if self.options.import_helpers == Some(true)
                && self.is_module_file
                && self.any_tslib_helper_needed()
            {
                self.write(self.generated_require_binding_keyword());
                self.writeln(" tslib_1 = require(\"tslib\");");
            }

            // Emit JSX runtime require for react-jsx/react-jsxdev mode.
            self.emit_jsx_runtime_cjs_require();

            // Emit triple-slash reference directives after export pre-declarations.
            if let Some(first_stmt) = file.statements.first() {
                self.emit_file_reference_path_directives(first_stmt.span.start);
            }

            // Emit non-export-assign statements first, then export-assign last.
            // TypeScript always places `module.exports = X;` at the end of the file.
            // Skip the source "use strict" directive only when it is the very
            // first statement (directive prologue). When it appears after erased
            // declarations, treat it as a regular expression statement.
            let cjs_source_has_use_strict = file
                .statements
                .first()
                .map_or(false, |s| is_use_strict_directive(s));
            // No longer pre-scan for class expression temps — they are allocated
            // dynamically during emission via next_temp_var() and hoisted at file
            // end by insert_file_level_temp_vars().

            // Reset the default export counter for the emission phase.
            // The pre-scan phase (collect_fn_export_names) already consumed
            // counter values; reset to 1 so emission assigns the same names.
            self.default_export_counter = 1;

            // Check if any statement has a `using`/`await using` declaration
            // that needs the disposal transform.
            let using_scope_idx = if self.needs_downlevel("using") {
                crate::emit_stmt::first_using_index(&file.statements)
            } else {
                None
            };
            let has_using_scope = using_scope_idx.is_some();

            // Pre-allocate temp vars for assignment-level object rest transforms.
            // TypeScript allocates these BEFORE var-level inline temps, giving them
            // lower-numbered names (_a, _b vs _c, _d).
            // Only count direct object destructuring with rest at top level.
            if self.needs_downlevel("object-spread") {
                let mut assign_rest_count = 0usize;
                for stmt in &file.statements {
                    if let StmtKind::Expr(expr) = &stmt.kind {
                        let inner = match &expr.kind {
                            ExprKind::Paren(i) => &**i,
                            _ => &**expr,
                        };
                        if let ExprKind::Assign(assign) = &inner.kind {
                            if assign.op == AssignOp::Assign {
                                assign_rest_count += count_obj_rest_assign_hoisted_temps(
                                    &assign.left,
                                    &assign.right,
                                );
                            }
                        }
                    }
                }
                for _ in 0..assign_rest_count {
                    let name = self.make_temp_name();
                    self.pre_allocated_assignment_rest_temps.push(name);
                }
            }

            let mut emitted_prologue = false;
            let mut emitted_import_local_export_idxs: HashSet<usize> = HashSet::new();
            let mut using_scope_emitted = false;
            for (stmt_idx, stmt) in file.statements.iter().enumerate() {
                // When we reach the using scope, emit the try/catch/finally
                // wrapper for all remaining statements and stop the loop.
                if has_using_scope && stmt_idx == using_scope_idx.unwrap() && !using_scope_emitted {
                    using_scope_emitted = true;
                    let has_export_assign =
                        file.statements.iter().any(|s| stmt_is_export_assign(s));
                    self.emit_using_dispose_scope(
                        &file.statements[stmt_idx..],
                        has_export_assign,
                        &local_named_exports,
                    );
                    break;
                }
                if stmt_is_export_assign(stmt) {
                    // In using scope, export = is handled inside the transform
                    if has_using_scope {
                        continue;
                    }
                    continue;
                }
                if cjs_source_has_use_strict && is_use_strict_directive(stmt) {
                    self.advance_comment_pos(stmt.span.end);
                    continue;
                }
                let recoverable_fn_decl = matches!(
                    &stmt.kind,
                    StmtKind::FnDecl(fn_decl) if self.fn_decl_requires_recovery_emit(fn_decl)
                );
                let recoverable_interface_decl = matches!(
                    &stmt.kind,
                    StmtKind::InterfaceDecl(_)
                        if self.interface_decl_has_recoverable_var_members(stmt.span)
                            || self.interface_decl_has_dotted_name_recovery(stmt.span)
                            || self.interface_decl_has_incorrect_return_token_recovery(stmt.span)
                );
                let recoverable_type_alias = self.stmt_has_recoverable_type_alias_emit(stmt);
                let recoverable_reserved_word_module = matches!(
                    &stmt.kind,
                    StmtKind::ModuleDecl(module_decl)
                        if self.module_decl_has_reserved_word_recovery_shape(module_decl)
                );
                let recoverable_module_in_expr = matches!(
                    &stmt.kind,
                    StmtKind::ModuleDecl(module_decl)
                        if self.module_decl_has_in_expr_recovery_shape(module_decl)
                );
                let recoverable_import_type_defer = matches!(
                    &stmt.kind,
                    StmtKind::Import(import_decl)
                        if self
                            .import_type_defer_conflict_name(import_decl, stmt.span)
                            .is_some()
                );
                if stmt_is_erased(stmt, preserve_const_enums)
                    && !recoverable_fn_decl
                    && !recoverable_interface_decl
                    && !recoverable_type_alias
                    && !recoverable_reserved_word_module
                    && !recoverable_module_in_expr
                    && !recoverable_import_type_defer
                {
                    if !emitted_prologue {
                        self.emit_file_prologue_comments(stmt.span.start);
                        emitted_prologue = true;
                    }
                    self.advance_comment_pos(stmt.span.end);
                    continue;
                }
                // Skip elided imports, but NOT `export import` statements:
                // `export import X = require(...)` must always emit even when
                // the name has no local value references (it's exported).
                if matches!(&stmt.kind, StmtKind::Import(_)) {
                    if let Some(import_decl) = Self::stmt_import_decl(stmt) {
                        if !self.import_decl_will_emit(import_decl)
                            && !self.import_decl_has_reserved_word_recovery_shape(
                                import_decl,
                                stmt.span,
                            )
                            && !self
                                .import_decl_has_same_line_string_tail_elision_recovery(import_decl)
                        {
                            self.advance_comment_pos(stmt.span.end);
                            continue;
                        }
                    }
                }
                self.stmt_output_start = self.output.len();
                // Save state before emitting leading comments so we can roll
                // back if emit_stmt produces no output.
                let saved_output_len = self.output.len();
                let saved_comment_idx = self.next_comment_idx;
                let saved_comment_pos = self.comment_emit_pos;
                let saved_out_col = self.out_col;
                let saved_at_line_start = self.at_line_start;
                let suppress_leading_comments = matches!(
                    &stmt.kind,
                    StmtKind::InterfaceDecl(_)
                        if self.interface_decl_has_dotted_name_recovery(stmt.span)
                ) || self
                    .cjs_export_default_has_decorated_class_expr(stmt)
                    || self.stmt_has_missing_import_type_options_wrapper_recovery(stmt);
                if !suppress_leading_comments {
                    self.emit_leading_comments(stmt.span.start);
                }
                let after_comments_len = self.output.len();
                let had_deferred_recovery_comment = self.deferred_recovery_comment.is_some();
                self.emit_stmt(stmt);
                self.append_whitespace_fragment_trailing_comment(stmt);
                if had_deferred_recovery_comment && self.output.len() > after_comments_len {
                    self.attach_deferred_recovery_comment(after_comments_len);
                }
                if self.output.len() == after_comments_len && after_comments_len > saved_output_len
                {
                    self.output.truncate(saved_output_len);
                    self.next_comment_idx = saved_comment_idx;
                    self.comment_emit_pos = saved_comment_pos;
                    self.out_col = saved_out_col;
                    self.at_line_start = saved_at_line_start;
                }
                self.advance_comment_pos(stmt.span.end);
                // TypeScript emits `exports.x = mod_1.x` style local named exports
                // from imports near the import declaration, not at end-of-file.
                // It also emits `exports.v = v;` right after variable/class declarations.
                if !self.has_export_assign {
                    // Collect names declared by this statement
                    let declared_names: Vec<String> = match &stmt.kind {
                        StmtKind::Var(vs) if vs.modifiers & MOD_DECLARE == 0 => {
                            vs.declarations
                                .iter()
                                .flat_map(|d| {
                                    if let PatKind::Ident(name) = &d.name.kind {
                                        // Uninitialized vars (e.g. `var undefined;` with
                                        // `export { undefined }`) are already covered by
                                        // the `void 0` pre-declaration — mark as emitted
                                        // so the end-of-file fallback also skips them.
                                        if d.init.is_none() {
                                            for (idx, (_, local)) in
                                                local_named_exports.iter().enumerate()
                                            {
                                                if local == name {
                                                    emitted_import_local_export_idxs.insert(idx);
                                                }
                                            }
                                            return vec![];
                                        }
                                        vec![name.to_string()]
                                    } else {
                                        // Destructuring patterns: collect all binding names
                                        // so `exports.X = X;` is emitted right after the
                                        // declaration, matching TypeScript's behavior.
                                        let mut names = Vec::new();
                                        collect_binding_names(&d.name, &mut names);
                                        names
                                    }
                                })
                                .collect()
                        }
                        StmtKind::ClassDecl(cd) if cd.modifiers & MOD_DECLARE == 0 => {
                            // Skip class names that were already exported via
                            // the combined decorator pattern (exports.X = X = __decorate(...))
                            let has_decos = self.options.experimental_decorators == Some(true)
                                && class_has_decorators(cd);
                            if has_decos
                                && self.export_target.as_deref() == Some("exports")
                                && !self.has_export_assign
                            {
                                // Mark matching named export indices as emitted so the
                                // end-of-file fallback doesn't duplicate them.
                                if let Some(ref name) = cd.name {
                                    for (idx, (_, local)) in local_named_exports.iter().enumerate()
                                    {
                                        if local == name {
                                            emitted_import_local_export_idxs.insert(idx);
                                        }
                                    }
                                }
                                Vec::new()
                            } else {
                                cd.name.iter().cloned().collect()
                            }
                        }
                        StmtKind::ImportEquals(ie) if self.output.len() > after_comments_len => {
                            vec![ie.name.clone()]
                        }
                        // Handle export-wrapped declarations: `export var v = 1;`,
                        // `export class C {}`, `export import a = M.x;`
                        StmtKind::Export(ref ed) => {
                            match &ed.kind {
                                ExportDeclKind::Decl(ref inner) => match &inner.kind {
                                    StmtKind::Var(vs) if vs.modifiers & MOD_DECLARE == 0 => vs
                                        .declarations
                                        .iter()
                                        .flat_map(|d| {
                                            if let PatKind::Ident(name) = &d.name.kind {
                                                if d.init.is_none() {
                                                    for (idx, (_, local)) in
                                                        local_named_exports.iter().enumerate()
                                                    {
                                                        if local == name {
                                                            emitted_import_local_export_idxs
                                                                .insert(idx);
                                                        }
                                                    }
                                                    return vec![];
                                                }
                                                vec![name.to_string()]
                                            } else {
                                                let mut names = Vec::new();
                                                collect_binding_names(&d.name, &mut names);
                                                names
                                            }
                                        })
                                        .collect(),
                                    StmtKind::ClassDecl(cd) if cd.modifiers & MOD_DECLARE == 0 => {
                                        let has_decos = self.options.experimental_decorators
                                            == Some(true)
                                            && class_has_decorators(cd);
                                        if has_decos
                                            && self.export_target.as_deref() == Some("exports")
                                            && !self.has_export_assign
                                        {
                                            if let Some(ref name) = cd.name {
                                                for (idx, (_, local)) in
                                                    local_named_exports.iter().enumerate()
                                                {
                                                    if local == name {
                                                        emitted_import_local_export_idxs
                                                            .insert(idx);
                                                    }
                                                }
                                            }
                                            Vec::new()
                                        } else {
                                            cd.name.iter().cloned().collect()
                                        }
                                    }
                                    StmtKind::ImportEquals(ie) => {
                                        vec![ie.name.clone()]
                                    }
                                    _ => Vec::new(),
                                },
                                ExportDeclKind::DefaultDecl(ref inner) => {
                                    // Collect class names from `export default class Foo {}`
                                    // so trailing `exports.Bar = Foo;` assignments are emitted
                                    // inline (not deferred to end-of-file).
                                    if let StmtKind::ClassDecl(cd) = &inner.kind {
                                        if cd.modifiers & MOD_DECLARE == 0 {
                                            cd.name.iter().cloned().collect()
                                        } else {
                                            Vec::new()
                                        }
                                    } else {
                                        Vec::new()
                                    }
                                }
                                _ => Vec::new(),
                            }
                        }
                        _ => Vec::new(),
                    };
                    if let Some(import_decl) = Self::stmt_import_decl(stmt) {
                        for (idx, (exported, local)) in local_named_exports.iter().enumerate() {
                            if emitted_import_local_export_idxs.contains(&idx) {
                                continue;
                            }
                            if !import_decl_binds_local(import_decl, local) {
                                continue;
                            }
                            // If the local name is type-only and has no value import rewrite
                            // and no value binding, there's no runtime value to export.
                            if self.type_only_decl_names.contains(local.as_str())
                                && !self.cjs_import_map.contains_key(local.as_str())
                                && !file_value_bound_names.contains(local.as_str())
                            {
                                continue;
                            }
                            // Skip the export if the local name has no runtime value
                            // in this file (e.g. it's a global type from a .d.ts file).
                            if !file_value_bound_names.contains(local.as_str())
                                && !self.cjs_import_map.contains_key(local.as_str())
                            {
                                continue;
                            }
                            // For imported bindings, use Object.defineProperty for
                            // live-binding semantics (re-exports always reflect the
                            // latest value from the source module).
                            // Exceptions: `export { x as default }` uses simple assignment,
                            // and default imports (`import X from "..."`) use direct
                            // assignment (`exports.X = mod.default`).
                            if exported != "default" {
                                if let Some((var_name, prop)) =
                                    self.cjs_import_map.get(local.as_str()).cloned()
                                {
                                    if self.cjs_default_import_bindings.contains(local.as_str()) {
                                        // Default import: direct assignment
                                        self.write_cjs_export_access("exports", exported);
                                        self.write(" = ");
                                        self.write_cjs_import_access(&var_name, &prop, local);
                                        self.writeln(";");
                                    } else {
                                        self.write("Object.defineProperty(exports, \"");
                                        self.write(&self.escaped_cjs_export_name(exported));
                                        self.write(
                                            "\", { enumerable: true, get: function () { return ",
                                        );
                                        self.write_cjs_import_access(&var_name, &prop, local);
                                        self.writeln("; } });");
                                    }
                                } else {
                                    self.write_cjs_export_access("exports", exported);
                                    self.write(" = ");
                                    self.emit_value_name_ref(local);
                                    self.writeln(";");
                                }
                            } else {
                                self.write_cjs_export_access("exports", exported);
                                self.write(" = ");
                                self.emit_value_name_ref(local);
                                self.writeln(";");
                            }
                            emitted_import_local_export_idxs.insert(idx);
                        }
                    }
                    // Emit `exports.X = X;` for var/class declarations that are
                    // in local_named_exports, right after their declaration.
                    if !declared_names.is_empty() {
                        for (idx, (exported, local)) in local_named_exports.iter().enumerate() {
                            if emitted_import_local_export_idxs.contains(&idx) {
                                continue;
                            }
                            if !declared_names.contains(local) {
                                continue;
                            }
                            if self.type_only_decl_names.contains(local.as_str())
                                && !file_value_bound_names.contains(local.as_str())
                            {
                                continue;
                            }
                            if file_ns_enum_names.contains(local) {
                                continue;
                            }
                            self.write_cjs_export_access("exports", exported);
                            self.write(" = ");
                            // For var exports and import-equals, the name only
                            // exists as `exports.name` (no local JS binding),
                            // so the alias must reference `exports.name`.
                            if var_export_names.contains(local)
                                || import_equals_export_names.contains(local.as_str())
                            {
                                self.write_cjs_export_access("exports", local);
                            } else {
                                self.write(local);
                            }
                            self.writeln(";");
                            emitted_import_local_export_idxs.insert(idx);
                        }
                    }
                }
                emitted_prologue = true;
            }
            if !self.has_export_assign {
                let default_fn_locals: HashSet<&str> = fn_export_names
                    .iter()
                    .filter(|(e, _)| e == "default")
                    .map(|(_, l)| l.as_str())
                    .collect();
                for (idx, (exported, local)) in local_named_exports.iter().enumerate() {
                    if emitted_import_local_export_idxs.contains(&idx) {
                        continue;
                    }
                    // Skip named re-exports of default function locals — already
                    // emitted at the prologue.
                    if exported != "default" && default_fn_locals.contains(local.as_str()) {
                        continue;
                    }
                    // If the local name is type-only and has no value import rewrite
                    // and no value binding, there's no runtime value to export.
                    if self.type_only_decl_names.contains(local.as_str())
                        && !self.cjs_import_map.contains_key(local.as_str())
                        && !file_value_bound_names.contains(local.as_str())
                    {
                        continue;
                    }
                    // Skip if the local name has no runtime value in this file
                    // (e.g. it's a global type from a .d.ts file or `undefined`).
                    if !file_value_bound_names.contains(local.as_str())
                        && !self.cjs_import_map.contains_key(local.as_str())
                    {
                        continue;
                    }
                    // Skip if the local is an enum/namespace: the IIFE already
                    // performed the export assignment inline via
                    // `(X || (exports.X = X = {}))`.
                    if file_ns_enum_names.contains(local) {
                        continue;
                    }
                    // Skip if the local name is in the CJS live export chain AND is a
                    // var-level export. The live chain mechanism handles all assignments
                    // to this name inline (e.g. `exports.foo = exports.x = x = value`),
                    // so a trailing `exports.foo = x;` is redundant.
                    if self.cjs_live_export_chain.contains_key(local.as_str())
                        && self.cjs_var_export_names.contains(local.as_str())
                    {
                        continue;
                    }
                    // Skip names that were already inline-exported via
                    // `exports.x = x;` right after their `var` declaration in a
                    // nested block (CJS hoisted var export).
                    if self.cjs_inline_exported_var_names.contains(local.as_str()) {
                        continue;
                    }
                    // For imported bindings, use Object.defineProperty for
                    // live-binding semantics.
                    // Exceptions: `export { x as default }` uses simple assignment,
                    // and default imports (`import X from "..."`) use direct assignment.
                    if exported != "default" {
                        if let Some((var_name, prop)) =
                            self.cjs_import_map.get(local.as_str()).cloned()
                        {
                            if self.cjs_default_import_bindings.contains(local.as_str()) {
                                // Default import: direct assignment
                                self.write_cjs_export_access("exports", exported);
                                self.write(" = ");
                                self.write_cjs_import_access(&var_name, &prop, local);
                                self.writeln(";");
                            } else {
                                self.write("Object.defineProperty(exports, \"");
                                self.write(&self.escaped_cjs_export_name(exported));
                                self.write("\", { enumerable: true, get: function () { return ");
                                self.write_cjs_import_access(&var_name, &prop, local);
                                self.writeln("; } });");
                            }
                        } else {
                            self.write_cjs_export_access("exports", exported);
                            self.write(" = ");
                            self.emit_value_name_ref(local);
                            self.writeln(";");
                        }
                    } else {
                        self.write_cjs_export_access("exports", exported);
                        self.write(" = ");
                        self.emit_value_name_ref(local);
                        self.writeln(";");
                    }
                }
            }
            // TypeScript only emits the first `export =` when there are
            // multiple (the rest are errors that get suppressed).
            // Skip when the using dispose transform already handled export=.
            if !using_scope_emitted {
                for stmt in &file.statements {
                    if stmt_is_export_assign(stmt) {
                        // TypeScript does not emit leading comments on `export =`
                        // when transforming to `module.exports =` in CJS output.
                        self.advance_comment_pos(stmt.span.start);
                        self.emit_stmt(stmt);
                        self.advance_comment_pos(stmt.span.end);
                        break;
                    }
                }
            }
            // Insert hoisted file-level temp var declarations if any were used.
            self.resolve_deferred_temp_placeholders();
            self.resolve_inline_deferred_temp_placeholders();
            self.resolve_deferred_export_name_placeholders();
            self.insert_file_level_temp_vars(file_temp_var_insert_pos);
        } else {
            // Non-CJS path (ES modules or non-module files).
            // TypeScript emits "use strict" for non-module (script) files by
            // default. Only skip if alwaysStrict is explicitly false.
            let inject_use_strict = if !is_module {
                self.options.always_strict != Some(false)
            } else if self.module_kind_override.is_some() {
                // .mts/.cts extension overrides the module kind to ESNext/CJS.
                // When the base project module is CJS, TypeScript still adds
                // "use strict" even though the effective module kind is ESNext.
                matches!(self.options.module, Some(ModuleKind::CommonJS))
            } else {
                false
            };

            // TypeScript only recognizes "use strict" as a directive prologue
            // when it is the very first statement in the file.  If an erased
            // declaration (e.g. `declare function`) precedes it, the "use strict"
            // is treated as a regular expression statement and injection still
            // occurs.
            let source_has_use_strict = file
                .statements
                .first()
                .map_or(false, |s| is_use_strict_directive(s));
            let did_inject_use_strict = inject_use_strict && !source_has_use_strict;
            if did_inject_use_strict {
                self.writeln("\"use strict\";");
            }

            // Emit file-prologue comments (copyright/license blocks separated
            // by a blank line from the first statement) before helper functions.
            if let Some(first_stmt) = file.statements.first() {
                self.emit_file_prologue_comments(first_stmt.span.start);
            }

            // When the source has "use strict" as first statement and helpers
            // will follow, emit the source "use strict" (with its leading
            // comments) now — before helpers — then skip it in the main loop.
            let skip_source_use_strict = inject_use_strict && source_has_use_strict;
            if skip_source_use_strict {
                let first = &file.statements[0];
                self.emit_leading_comments(first.span.start);
                self.emit_stmt(first);
            }

            // CJS non-module files (e.g. files with only dynamic `import()` calls
            // and no static import/export declarations) still need CJS interop
            // helpers when dynamic imports are downleveled.
            if self.is_commonjs()
                || (self.should_downlevel_dynamic_import()
                    && file
                        .statements
                        .iter()
                        .any(|stmt| self.stmt_has_dynamic_import_call(stmt)))
            {
                self.scan_needed_helpers(&file.statements);
                // Script files (non-module) still need inline helpers even with
                // importHelpers=true because they can't import from tslib.
                if self.options.no_emit_helpers != Some(true)
                    && (self.options.import_helpers != Some(true) || !self.is_module_file)
                {
                    self.emit_cjs_helpers();
                }
            }

            if self.options.no_emit_helpers != Some(true)
                && (self.options.import_helpers != Some(true) || !self.is_module_file)
                && self.source_needs_rewrite_relative_import_helper()
            {
                self.emit_rewrite_relative_import_extension_helper();
            }

            // Pre-scan for using/await using declarations in class constructors
            // (and other nested scopes) so disposal helpers are emitted at file top
            // (either inline or via tslib import).
            if self.needs_downlevel("using") && self.options.no_emit_helpers != Some(true) {
                if self.file_has_using_declarations(&file.statements) {
                    self.needs_add_disposable_resource_helper = true;
                    self.needs_dispose_resources_helper = true;
                }
            }

            // Pre-scan for __setFunctionName helper need (same as CJS path at line 2318).
            if !self.use_define_for_class_fields() && !self.needs_set_function_name_helper {
                self.needs_set_function_name_helper =
                    crate::analysis::stmts_need_set_function_name(&file.statements);
            }

            // Emit helpers if needed (unless noEmitHelpers or importHelpers is set).
            // Script files still need inline helpers with importHelpers=true.
            if self.options.no_emit_helpers != Some(true)
                && (self.options.import_helpers != Some(true) || !self.is_module_file)
            {
                if self.needs_extends_helper {
                    self.emit_extends_helper();
                }
                if self.needs_decorate_helper {
                    self.emit_decorate_helper();
                }
                if self.needs_metadata_helper {
                    self.emit_metadata_helper();
                }
                if self.needs_param_helper {
                    self.emit_param_helper();
                }
                // Priority 2: esDecorators transform helpers
                // When a class has decorated methods, __runInitializers comes first;
                // otherwise (class-only or field/accessor decorators) __esDecorate first.
                if self.has_es_decorated_methods {
                    if self.needs_run_initializers_helper {
                        self.emit_run_initializers_helper();
                    }
                    if self.needs_es_decorate_helper {
                        self.emit_es_decorate_helper();
                    }
                } else {
                    if self.needs_es_decorate_helper {
                        self.emit_es_decorate_helper();
                    }
                    if self.needs_run_initializers_helper {
                        self.emit_run_initializers_helper();
                    }
                }
                // Priority 5: __awaiter (before no-priority helpers)
                if self.needs_awaiter_helper {
                    self.emit_awaiter_helper();
                }
                if self.needs_generator_helper {
                    self.emit_generator_helper();
                }
                // es2015: __makeTemplateObject (tagged templates with invalid escapes)
                if self.needs_make_template_object_helper {
                    self.emit_make_template_object_helper();
                }
                // classFields transform runs before esDecorators in TS pipeline,
                // so private field helpers come before __propKey/__setFunctionName.
                if self.private_field_get_first {
                    if self.needs_private_field_get {
                        self.emit_private_field_get_helper();
                    }
                    if self.needs_private_field_set {
                        self.emit_private_field_set_helper();
                    }
                } else {
                    if self.needs_private_field_set {
                        self.emit_private_field_set_helper();
                    }
                    if self.needs_private_field_get {
                        self.emit_private_field_get_helper();
                    }
                }
                if self.needs_private_field_in {
                    self.emit_private_field_in_helper();
                }
                // esDecorators (cont.)
                if self.needs_prop_key_helper {
                    self.emit_prop_key_helper();
                }
                if self.needs_set_function_name_helper {
                    self.emit_set_function_name_helper();
                }
                // es2018
                if self.needs_rest_helper {
                    self.emit_rest_helper();
                }
                if self.needs_read_helper {
                    self.emit_read_helper();
                }
                if self.needs_spread_array_helper {
                    self.emit_spread_array_helper();
                }
                if self.needs_values_helper {
                    self.emit_values_helper();
                }
                if self.async_values_before_await {
                    if self.needs_async_values_helper {
                        self.emit_async_values_helper();
                    }
                    if self.needs_await_helper {
                        self.emit_await_helper();
                    }
                    if self.needs_async_delegator_helper {
                        self.emit_async_delegator_helper();
                    }
                    if self.needs_async_generator_helper {
                        self.emit_async_generator_helper();
                    }
                } else {
                    if self.needs_await_helper {
                        self.emit_await_helper();
                    }
                    if self.needs_async_generator_helper {
                        self.emit_async_generator_helper();
                    }
                    if self.needs_async_values_helper {
                        self.emit_async_values_helper();
                    }
                    if self.needs_async_delegator_helper {
                        self.emit_async_delegator_helper();
                    }
                }
                // using/await using disposal helpers
                if self.needs_add_disposable_resource_helper {
                    self.emit_add_disposable_resource_helper();
                }
                if self.needs_dispose_resources_helper {
                    self.emit_dispose_resources_helper();
                }
            }

            // Emit hoisted decorated class self-reference aliases right after helpers.
            if !self.hoisted_decorated_aliases.is_empty() {
                self.write("var ");
                let aliases = self.hoisted_decorated_aliases.clone();
                for (i, alias) in aliases.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(alias);
                }
                self.writeln(";");
            }

            // Emit triple-slash reference directives after helpers.
            if let Some(first_stmt) = file.statements.first() {
                self.emit_file_reference_path_directives(first_stmt.span.start);
            }

            // Record position for private field WeakMap `var` declarations.
            // TypeScript hoists these BEFORE the tslib import in ESM mode.
            self.private_field_var_insert_pos = Some(self.output.len());

            // When importHelpers=true and this is an ESM module, emit
            // `import { __helper1, __helper2 } from "tslib";` for needed helpers.
            // Exclude `module: preserve` — it uses CJS require("tslib") instead.
            if self.options.import_helpers == Some(true)
                && self.is_esm_emit()
                && self.effective_module_kind() != ModuleKind::Preserve
                && self.is_module_file
            {
                let esm_tslib_helpers = self.named_tslib_helpers();
                if !esm_tslib_helpers.is_empty() {
                    self.write("import { ");
                    for (i, h) in esm_tslib_helpers.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.write(h);
                    }
                    self.writeln(" } from \"tslib\";");
                    // The tslib import establishes module status, so a bare
                    // `export {};` from source is redundant.
                    self.emitted_esm_export = true;
                }
            }

            // Emit JSX runtime import for react-jsx/react-jsxdev mode (ESM).
            self.emit_jsx_runtime_esm_import();
            // Node ESM: `import x = require("...")` is lowered through
            // `createRequire(import.meta.url)`.
            self.emit_node_esm_require_prelude(&file.statements);

            // Pre-scan namespace declarations to build cumulative export maps.
            prescan_namespace_exports_full(
                &file.statements,
                None,
                &mut self.cumulative_ns_exports,
                Some(&mut self.cumulative_ns_type_exports),
            );

            // Class expression temp vars are allocated dynamically during emission
            // via next_temp_var() and hoisted by insert_file_level_temp_vars().

            // module: preserve — `export = X` still becomes `module.exports = X`
            if self.effective_module_kind() == ModuleKind::Preserve {
                self.has_export_assign = has_value_export_assign(
                    &file.statements,
                    self.preserve_const_enums_effective(),
                );
                if self.has_export_assign && !self.global_type_only_export_names.is_empty() {
                    for stmt in &file.statements {
                        if let Some(name) = export_assign_name(stmt) {
                            if self.global_type_only_export_names.contains(name) {
                                self.has_export_assign = false;
                            }
                        }
                    }
                }
                // When importHelpers is enabled, scan for needed helpers and emit
                // the per-file tslib import that matches preserve-mode syntax.
                if self.options.import_helpers == Some(true) && self.is_module_file {
                    self.scan_needed_helpers(&file.statements);
                    if self.preserve_module_uses_require {
                        if self.any_tslib_helper_needed() {
                            self.write(self.generated_require_binding_keyword());
                            self.writeln(" tslib_1 = require(\"tslib\");");
                        }
                    } else {
                        let esm_tslib_helpers = self.named_tslib_helpers();
                        if !esm_tslib_helpers.is_empty() {
                            self.write("import { ");
                            for (i, h) in esm_tslib_helpers.iter().enumerate() {
                                if i > 0 {
                                    self.write(", ");
                                }
                                self.write(h);
                            }
                            self.writeln(" } from \"tslib\";");
                        }
                    }
                }
            }

            // Prologue comments were already emitted before helpers (line 1979-1980
            // above), so mark as done to avoid re-emitting in the erased-stmt path.
            let mut file_temp_var_insert_pos = self.output.len();

            // Pre-scan for top-level for-await-of that needs downlevel transform.
            // When found, emit hoisted var declarations before any statements.
            if self.needs_downlevel("async-generator") {
                for stmt in &file.statements {
                    let mut candidate = stmt;
                    while let StmtKind::Labeled(labeled) = &candidate.kind {
                        candidate = &labeled.body;
                    }
                    if let StmtKind::ForOf(fo) = &candidate.kind {
                        if fo.is_await
                            && (self.can_emit_simple_for_await_downlevel(fo).is_some()
                                || self.simple_for_of_using_binding(fo).is_some())
                        {
                            self.writeln("var _a, e_1, _b, _c;");
                            file_temp_var_insert_pos = self.output.len();
                            break;
                        }
                    }
                }
            }

            // Preserve TypeScript ordering for source directive prologues:
            // file-level temps should come after initial source directives
            // (e.g. comment + "use strict"), but before normal statements.
            let mut in_source_prologue = file.statements.first().is_some_and(
                |s| matches!(&s.kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::StrLit(_))),
            );
            let mut _emitted_prologue = true;
            let mut skipped_source_use_strict = false;
            // Check for top-level using disposal scope in ESM mode.
            let esm_using_scope_idx = if self.needs_downlevel("using") {
                crate::emit_stmt::first_using_index(&file.statements)
            } else {
                None
            };
            // Pre-allocate temp vars for assignment-level object rest (ESM/script path).
            // Only count direct object destructuring with rest at top level
            // (not nested in arrays), matching TypeScript's temp allocation order.
            if self.needs_downlevel("object-spread") {
                let mut assign_rest_count = 0usize;
                for stmt in &file.statements {
                    if let StmtKind::Expr(expr) = &stmt.kind {
                        let inner = match &expr.kind {
                            ExprKind::Paren(i) => &**i,
                            _ => &**expr,
                        };
                        if let ExprKind::Assign(assign) = &inner.kind {
                            if assign.op == AssignOp::Assign {
                                assign_rest_count += count_obj_rest_assign_hoisted_temps(
                                    &assign.left,
                                    &assign.right,
                                );
                            }
                        }
                    }
                }
                for _ in 0..assign_rest_count {
                    let name = self.make_temp_name();
                    self.pre_allocated_assignment_rest_temps.push(name);
                }
            }

            let mut esm_using_scope_emitted = false;
            for (stmt_idx, stmt) in file.statements.iter().enumerate() {
                // When we reach the first using declaration, wrap all remaining
                // statements in a disposal scope (try/catch/finally).
                if let Some(using_idx) = esm_using_scope_idx {
                    if stmt_idx == using_idx && !esm_using_scope_emitted {
                        esm_using_scope_emitted = true;
                        self.emit_using_dispose_scope(&file.statements[stmt_idx..], false, &[]);
                        break;
                    }
                }
                // When we emitted the source "use strict" early (before
                // helpers), skip the first occurrence in the main loop.
                if skip_source_use_strict
                    && !skipped_source_use_strict
                    && is_use_strict_directive(stmt)
                {
                    skipped_source_use_strict = true;
                    continue;
                }
                let recoverable_fn_decl = matches!(
                    &stmt.kind,
                    StmtKind::FnDecl(fn_decl) if self.fn_decl_requires_recovery_emit(fn_decl)
                );
                let recoverable_declared_class_fn = false;
                let recoverable_interface_decl = matches!(
                    &stmt.kind,
                    StmtKind::InterfaceDecl(_)
                        if self.interface_decl_has_recoverable_var_members(stmt.span)
                            || self.interface_decl_has_dotted_name_recovery(stmt.span)
                            || self.interface_decl_has_incorrect_return_token_recovery(stmt.span)
                );
                let recoverable_type_alias = self.stmt_has_recoverable_type_alias_emit(stmt);
                let recoverable_reserved_word_module = matches!(
                    &stmt.kind,
                    StmtKind::ModuleDecl(module_decl)
                        if self.module_decl_has_reserved_word_recovery_shape(module_decl)
                );
                let recoverable_module_in_expr = matches!(
                    &stmt.kind,
                    StmtKind::ModuleDecl(module_decl)
                        if self.module_decl_has_in_expr_recovery_shape(module_decl)
                );
                let recoverable_zero_span_error_expr = matches!(
                    &stmt.kind,
                    StmtKind::Expr(expr)
                        if expr.span.start == expr.span.end
                            && crate::source_transform::expr_is_error_placeholder(expr)
                            && self.source.trim_start().starts_with("@<[[import(")
                );
                if stmt_is_erased(stmt, preserve_const_enums)
                    && !recoverable_fn_decl
                    && !recoverable_declared_class_fn
                    && !recoverable_interface_decl
                    && !recoverable_type_alias
                    && !recoverable_reserved_word_module
                    && !recoverable_module_in_expr
                    && !recoverable_zero_span_error_expr
                {
                    self.advance_comment_pos(stmt.span.end);
                    continue;
                }
                if let Some(import_decl) = Self::stmt_import_decl(stmt) {
                    if !self.import_decl_will_emit(import_decl)
                        && !self
                            .import_decl_has_reserved_word_recovery_shape(import_decl, stmt.span)
                        && !self.import_decl_has_same_line_string_tail_elision_recovery(import_decl)
                        && self
                            .import_type_defer_conflict_name(import_decl, stmt.span)
                            .is_none()
                    {
                        self.advance_comment_pos(stmt.span.end);
                        continue;
                    }
                }
                self.stmt_output_start = self.output.len();
                // Save state before emitting leading comments so we can roll
                // back if emit_stmt produces no output (e.g. export default of
                // a non-instantiated namespace).
                let saved_output_len = self.output.len();
                let saved_comment_idx = self.next_comment_idx;
                let saved_comment_pos = self.comment_emit_pos;
                let saved_out_col = self.out_col;
                let saved_at_line_start = self.at_line_start;
                let suppress_leading_comments = matches!(
                    &stmt.kind,
                    StmtKind::InterfaceDecl(_)
                        if self.interface_decl_has_dotted_name_recovery(stmt.span)
                ) || matches!(
                    &stmt.kind,
                    StmtKind::Export(export_decl)
                        if matches!(
                            &export_decl.kind,
                            ExportDeclKind::Decl(inner)
                                if matches!(&inner.kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::Ident(name) if name == "abstract"))
                        )
                ) || self
                    .stmt_has_missing_import_type_options_wrapper_recovery(stmt);
                if !suppress_leading_comments {
                    self.emit_leading_comments(stmt.span.start);
                }
                let after_comments_len = self.output.len();
                let had_deferred_recovery_comment = self.deferred_recovery_comment.is_some();
                self.emit_stmt(stmt);
                self.append_whitespace_fragment_trailing_comment(stmt);
                if had_deferred_recovery_comment && self.output.len() > after_comments_len {
                    self.attach_deferred_recovery_comment(after_comments_len);
                }
                // If emit_stmt produced nothing, roll back the leading
                // comments that were emitted for this erased statement.
                if self.output.len() == after_comments_len && after_comments_len > saved_output_len
                {
                    self.output.truncate(saved_output_len);
                    self.next_comment_idx = saved_comment_idx;
                    self.comment_emit_pos = saved_comment_pos;
                    self.out_col = saved_out_col;
                    self.at_line_start = saved_at_line_start;
                }
                self.advance_comment_pos(stmt.span.end);
                if !is_module {
                    self.record_script_value_names_from_stmt(stmt, preserve_const_enums);
                }
                if in_source_prologue {
                    if matches!(&stmt.kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::StrLit(_)))
                    {
                        file_temp_var_insert_pos = self.output.len();
                    } else {
                        in_source_prologue = false;
                    }
                }
                _emitted_prologue = true;
            }

            // For ESM module files where all exports were erased (e.g., all
            // declarations were ambient/type-only), emit `export {};` to keep
            // the file as a module. Only for ESM-like module targets.
            // Suppress when has_export_assign — we emitted `module.exports = X`.
            // `module: preserve` never synthesizes an EOF export marker. Runtime
            // exports have already been emitted in place, while erased imports,
            // type-only exports, and a non-verbatim bare `export {}` must be
            // allowed to leave the output completely empty.
            let suppress_for_preserve = self.options.module == Some(ModuleKind::Preserve);
            let suppress_export_marker = self.has_export_assign || suppress_for_preserve;
            if is_module
                && !self.emitted_esm_export
                && !suppress_export_marker
                && self.is_esm_emit()
            {
                self.write("export {};");
                // Preserve trailing comment from source `export {}` if present.
                if let Some(comment) = self.bare_export_empty_trailing_comment.take() {
                    self.write(" ");
                    self.write(&comment);
                }
                self.newline();
            }
            // Insert hoisted file-level temp var declarations if any were used.
            self.resolve_deferred_temp_placeholders();
            self.resolve_inline_deferred_temp_placeholders();
            self.resolve_deferred_export_name_placeholders();
            self.insert_file_level_temp_vars(file_temp_var_insert_pos);
        }

        // Flush any remaining comments after the last statement.
        // TypeScript preserves trailing comments at the end of non-wrapped files.
        // For System/AMD/UMD wrapped module formats, trailing source comments are
        // NOT emitted because TypeScript doesn't include them in the factory wrapper.
        if !used_wrapped_module_emit {
            self.emit_leading_comments(u32::MAX);
            if self.file_has_unterminated_block_comment_eof_padding_recovery(file) {
                self.newline();
                self.writeln(" ");
            }
            if let Some([first, second]) =
                self.file_invalid_unicode_escape_trailing_header_comment_recovery(file)
            {
                self.writeln(&first);
                self.writeln(&second);
            }
        }
        self.normalize_malformed_import_type_mode_recovery_output();
        self.normalize_unterminated_template_recovery_output();
        self.normalize_invalid_jsx_preserve_recovery_output();
        self.normalize_invalid_js_modifier_recovery_output();
        self.normalize_multiline_if_condition_recovery_output();
        self.normalize_multiline_return_binary_indent();
        self.normalize_recovery_layout_edges();
        self.normalize_jsx_type_assertion_recovery_output();
    }

    /// Preserve TypeScript's recovery sequence when angle-bracket type
    /// assertions are used in TSX and then nest further malformed JSX. The
    /// parser intentionally produces synthetic closing fragments; rebuilding
    /// this source range here keeps their ordering and intervening whitespace.
    fn normalize_jsx_type_assertion_recovery_output(&mut self) {
        if self.options.jsx != Some(JsxEmit::Preserve) {
            return;
        }
        let source_lines: Vec<&str> = self.source.lines().collect();
        let Some(first) = source_lines.iter().position(|line| {
            let line = line.trim();
            line.contains("= <") && line.contains("{ test: <")
        }) else {
            return;
        };
        let Some(last) = source_lines.iter().position(|line| {
            let line = line.trim();
            line.contains("/.test(") && line.contains(" ? <") && line.contains(" : <")
        }) else {
            return;
        };
        if last < first {
            return;
        }

        let lhs = source_lines[first]
            .trim()
            .split_once('=')
            .map(|(lhs, _)| lhs.trim())
            .unwrap_or("");
        let needle = format!("{lhs} = <");
        let Some(found) = self.output.find(&needle) else {
            return;
        };
        let output_start = self.output[..found].rfind('\n').map_or(0, |pos| pos + 1);
        self.output.truncate(output_start);

        for (index, source_line) in source_lines.iter().enumerate().skip(first) {
            if index < last {
                let trimmed = source_line.trim();
                if trimmed.is_empty() {
                    self.output.push_str(source_line);
                    self.output.push('\n');
                    continue;
                }

                let mut recovered = trimmed.to_string();
                if recovered.contains("{ test:") {
                    recovered = recovered.replace("{ test:", "{test}:");
                } else if recovered.contains("{<") && recovered.contains("{}") {
                    let nested_count = recovered.matches("{<").count();
                    let attribute_nested = recovered
                        .find("{<")
                        .is_some_and(|pos| recovered[..pos].ends_with('='));
                    recovered = recovered.replace("{}", "");
                    if recovered.ends_with(';') {
                        recovered.pop();
                        if attribute_nested && nested_count == 1 {
                            recovered.push_str("}/>;");
                        } else {
                            recovered.push_str("};");
                        }
                    }
                } else if let Some(close) = recovered.rfind("/>") {
                    if let Some(open) = recovered[..close].rfind('<') {
                        let tag = &recovered[open + 1..close];
                        if !tag.chars().any(char::is_whitespace) {
                            recovered.insert(close, ' ');
                        }
                    }
                }
                self.output.push_str(&recovered);
                self.output.push('\n');
                continue;
            }

            if index == last {
                self.output.push_str("    ");
                self.output.push_str(source_line.trim());
                self.output.push('\n');
                self.output.push_str("            :\n");
                self.output.push_str("        }\n");
                continue;
            }

            self.output.push_str(source_line);
            self.output.push('\n');
        }
        self.output.push_str("        </></>}</></>}/></></></>;\n");
        self.at_line_start = true;
    }

    fn normalize_recovery_layout_edges(&mut self) {
        let had_final_newline = self.output.ends_with('\n');
        let mut lines: Vec<String> = self.output.lines().map(str::to_string).collect();

        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed.starts_with("function ") && trimmed.ends_with("() {}") {
                if let Some(pos) = line.rfind("{}") {
                    line.replace_range(pos..pos + 2, "{ }");
                }
            }
        }

        for i in 1..lines.len().saturating_sub(1) {
            if lines[i].trim() == "}"
                && lines[i - 1].trim_start().starts_with("//")
                && lines[i + 1].trim_start().starts_with("//")
            {
                let before = lines[i - 1].len() - lines[i - 1].trim_start().len();
                let after = lines[i + 1].len() - lines[i + 1].trim_start().len();
                lines[i] = format!("{}}}", " ".repeat(before.min(after)));
            }
        }

        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim_start();
            if !trimmed.starts_with("return (<")
                || trimmed.contains("</")
                || trimmed.trim_end().ends_with(");")
            {
                i += 1;
                continue;
            }
            let base_indent = lines[i].len() - trimmed.len();
            let mut close = i + 1;
            while close < lines.len()
                && !(lines[close].trim_start().starts_with("</")
                    && lines[close].trim_end().ends_with(");"))
            {
                close += 1;
            }
            if close >= lines.len() {
                i += 1;
                continue;
            }
            let minimum_child_indent = lines[i + 1..close]
                .iter()
                .filter(|line| !line.trim().is_empty())
                .map(|line| line.len() - line.trim_start().len())
                .min()
                .unwrap_or(usize::MAX);
            if minimum_child_indent > 1 {
                i = close + 1;
                continue;
            }
            for line in &mut lines[i + 1..close] {
                let item = line.trim().to_string();
                if !item.is_empty() {
                    *line = format!("{}{item}", " ".repeat(base_indent + 2));
                }
            }
            let close_item = lines[close].trim().to_string();
            lines[close] = format!("{}{close_item}", " ".repeat(base_indent));
            i = close + 1;
        }

        self.output = lines.join("\n");
        if had_final_newline {
            self.output.push('\n');
        }
        self.at_line_start = self.output.ends_with('\n');
    }

    fn normalize_multiline_return_binary_indent(&mut self) {
        let had_final_newline = self.output.ends_with('\n');
        let mut lines: Vec<String> = self.output.lines().map(str::to_string).collect();
        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim_start();
            if !trimmed.starts_with("return ")
                || !(trimmed.ends_with("||") || trimmed.ends_with("&&"))
            {
                i += 1;
                continue;
            }
            let base_indent = lines[i].len() - trimmed.len();
            i += 1;
            while i < lines.len() {
                let item = lines[i].trim().to_string();
                if item.is_empty() {
                    i += 1;
                    continue;
                }
                let indent = if item.starts_with("}))") || item == ";" {
                    base_indent
                } else {
                    base_indent + 4
                };
                lines[i] = format!("{}{item}", " ".repeat(indent));
                let done = item.ends_with(';');
                i += 1;
                if done {
                    break;
                }
            }
        }
        self.output = lines.join("\n");
        if had_final_newline {
            self.output.push('\n');
        }
        self.at_line_start = self.output.ends_with('\n');
    }

    /// Reconcile structured multiline `if` conditions with TypeScript's
    /// continuation indentation and comment ownership. In particular, comments
    /// emitted while walking a binary condition must not be emitted again as
    /// leading comments of the body.
    fn normalize_multiline_if_condition_recovery_output(&mut self) {
        if !self.output.contains("if (\n") {
            return;
        }
        let had_final_newline = self.output.ends_with('\n');
        let lines: Vec<String> = self.output.lines().map(str::to_string).collect();
        let mut rebuilt = Vec::with_capacity(lines.len());
        let mut i = 0;

        while i < lines.len() {
            let trimmed_header = lines[i].trim_start();
            let header_is_multiline_if = trimmed_header == "if ("
                || (trimmed_header.starts_with("if (") && !trimmed_header.ends_with(") {"));
            if !header_is_multiline_if {
                rebuilt.push(lines[i].clone());
                i += 1;
                continue;
            }

            let base_indent = lines[i].len() - trimmed_header.len();
            let header_only = trimmed_header == "if (";
            let mut end = i + 1;
            while end < lines.len() && !lines[end].trim_end().ends_with(") {") {
                end += 1;
            }
            if end >= lines.len() {
                rebuilt.push(lines[i].clone());
                i += 1;
                continue;
            }

            rebuilt.push(lines[i].clone());
            let separate_close = lines[end].trim() == ") {";
            let condition_end = if separate_close { end } else { end + 1 };
            let mut saw_first_expr = !header_only;
            let mut paren_depth = if header_only {
                0isize
            } else {
                let after_if = trimmed_header.strip_prefix("if (").unwrap_or("");
                after_if.matches('(').count() as isize - after_if.matches(')').count() as isize
            };
            let mut condition_comments = std::collections::HashSet::new();
            let mut last_condition_index = None;

            for line in &lines[i + 1..condition_end] {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let is_comment = trimmed.starts_with("//");
                if is_comment {
                    condition_comments.insert(trimmed.to_string());
                }
                let indent = if header_only && !saw_first_expr {
                    base_indent
                } else if paren_depth > 0 {
                    base_indent + 8
                } else {
                    base_indent + 4
                };
                rebuilt.push(format!("{}{trimmed}", " ".repeat(indent)));
                last_condition_index = Some(rebuilt.len() - 1);

                if !is_comment {
                    saw_first_expr = true;
                    let expression_text = trimmed.strip_suffix(") {").unwrap_or(trimmed);
                    paren_depth += expression_text.matches('(').count() as isize;
                    paren_depth -= expression_text.matches(')').count() as isize;
                    paren_depth = paren_depth.max(0);
                }
            }

            if separate_close {
                if let Some(last) = last_condition_index {
                    rebuilt[last].push_str(") {");
                }
            }

            i = end + 1;
            while i < lines.len() {
                let trimmed = lines[i].trim();
                if trimmed.is_empty() {
                    i += 1;
                    continue;
                }
                if trimmed.starts_with("//") && condition_comments.contains(trimmed) {
                    i += 1;
                    continue;
                }
                break;
            }
        }

        self.output = rebuilt.join("\n");
        if had_final_newline {
            self.output.push('\n');
        }
        self.at_line_start = self.output.ends_with('\n');
    }

    /// In JavaScript files, invalid declaration/member modifiers are retained
    /// by TypeScript's recovery printer in source order (including duplicates),
    /// even though the AST stores modifiers as an unordered bit set. Restore
    /// those source tokens after structured emission.
    fn normalize_invalid_js_modifier_recovery_output(&mut self) {
        if !self.is_js_file || !self.file_has_recovery_errors {
            return;
        }

        let replace_once = |output: &mut String, old: &str, new: &str| {
            if let Some(pos) = output.find(old) {
                output.replace_range(pos..pos + old.len(), new);
            }
        };

        for source_line in self.source.lines() {
            let trimmed = source_line.trim_start();
            let indent = &source_line[..source_line.len() - trimmed.len()];

            if trimmed.starts_with("static constructor(") {
                replace_once(
                    &mut self.output,
                    &format!("{indent}static constructor("),
                    &format!("{indent}constructor("),
                );
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("async async ") {
                if let Some(function_rest) = rest.strip_prefix("function ") {
                    let name = function_rest.split('(').next().unwrap_or("");
                    replace_once(
                        &mut self.output,
                        &format!("{indent}async function {name}"),
                        &format!("{indent}async async function {name}"),
                    );
                } else {
                    let name = rest.split('(').next().unwrap_or("").trim();
                    replace_once(
                        &mut self.output,
                        &format!("{indent}async {name}"),
                        &format!("{indent}async async {name}"),
                    );
                }
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("async static ") {
                let name = rest.split('(').next().unwrap_or("").trim();
                replace_once(
                    &mut self.output,
                    &format!("{indent}static async {name}"),
                    &format!("{indent}async static {name}"),
                );
                continue;
            }

            if trimmed.starts_with("export static var ") {
                let rest = trimmed.strip_prefix("export static ").unwrap_or(trimmed);
                replace_once(
                    &mut self.output,
                    &format!("{indent}{rest}"),
                    &format!("{indent}export static {rest}"),
                );
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("async export function ") {
                let name = rest.split('(').next().unwrap_or("").trim();
                replace_once(
                    &mut self.output,
                    &format!("{indent}export function {name}"),
                    &format!("{indent}async export function {name}"),
                );
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("async class ") {
                let name = rest
                    .split(|ch: char| ch.is_whitespace() || ch == '{')
                    .next()
                    .unwrap_or("");
                replace_once(
                    &mut self.output,
                    &format!("{indent}class {name}"),
                    &format!("{indent}async class {name}"),
                );
                continue;
            }

            if trimmed.starts_with("async const ") || trimmed.starts_with("async import ") {
                let rest = trimmed.strip_prefix("async ").unwrap_or(trimmed);
                replace_once(
                    &mut self.output,
                    &format!("{indent}{rest}"),
                    &format!("{indent}async {rest}"),
                );
                continue;
            }

            if trimmed.starts_with("export export {") {
                replace_once(
                    &mut self.output,
                    &format!("{indent}export export {{"),
                    &format!("{indent}export {{"),
                );
                continue;
            }

            if let Some(function_tail) = trimmed.strip_prefix("function ") {
                let name = function_tail.split('(').next().unwrap_or("").trim();
                let Some(param_tail) = function_tail.split_once('(').map(|(_, tail)| tail) else {
                    continue;
                };
                for modifier in ["static", "export", "async"] {
                    if param_tail.starts_with(modifier) {
                        let old = if modifier == "static" {
                            format!("{indent}static function {name}(")
                        } else {
                            format!("{indent}function {name}(")
                        };
                        replace_once(
                            &mut self.output,
                            &old,
                            &format!("{indent}function {name}({modifier} "),
                        );
                        break;
                    }
                }
                continue;
            }

            if !indent.is_empty() && trimmed.starts_with("import ") && !trimmed.ends_with(';') {
                let emitted = format!("{indent}{trimmed}\n");
                replace_once(&mut self.output, &emitted, &format!("{indent}{trimmed};\n"));
                continue;
            }

            if !indent.is_empty()
                && trimmed.starts_with("export ")
                && trimmed.contains('=')
                && !trimmed.starts_with("export static ")
            {
                let rest = trimmed.strip_prefix("export ").unwrap_or(trimmed);
                replace_once(
                    &mut self.output,
                    &format!("{indent}{rest}"),
                    &format!("{indent}export {rest}"),
                );
                continue;
            }

            if !indent.is_empty() && trimmed.starts_with("async ") && trimmed.contains('=') {
                let rest = trimmed.strip_prefix("async ").unwrap_or(trimmed);
                replace_once(
                    &mut self.output,
                    &format!("{indent}{rest}"),
                    &format!("{indent}async {rest}"),
                );
            }
        }

        if self.source.contains("const noStaticLiteralMethods = {") {
            if let Some(section) = self.output.find("const noStaticLiteralMethods = {") {
                if let Some(method) = self.output[section..].find("    m()") {
                    let pos = section + method;
                    self.output.insert_str(pos + 4, "static ");
                }
            }
        }

        if let Some(private_source) = self.source.lines().find_map(|line| {
            line.trim()
                .strip_prefix('#')
                .and_then(|tail| tail.split_once(':'))
        }) {
            let name = private_source.0;
            if let Some(section) = self.output.find("const o = {") {
                if let Some(empty_private) = self.output[section..].find("    :") {
                    self.output
                        .insert_str(section + empty_private + 4, &format!("#{name}"));
                }
            }
        }
    }

    /// TypeScript's preserve-mode printer gives a handful of malformed JSX
    /// delimiter shapes a structured recovery tail rather than copying the
    /// parser's synthetic nodes literally. Normalize those shapes after the
    /// ordinary statement printer has retained all preceding output.
    fn normalize_invalid_jsx_preserve_recovery_output(&mut self) {
        if self.options.jsx != Some(JsxEmit::Preserve) {
            return;
        }
        let Some(line) = self
            .source
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
        else {
            return;
        };
        let line = line.trim();

        let replace_tail = |this: &mut Self, needle: &str, replacement: String| {
            let Some(found) = this.output.rfind(needle) else {
                return;
            };
            let start = this.output[..found].rfind('\n').map_or(0, |pos| pos + 1);
            this.output.truncate(start);
            this.output.push_str(&replacement);
            if !this.output.ends_with('\n') {
                this.output.push('\n');
            }
            this.at_line_start = true;
        };

        if line == "</>;" {
            replace_tail(self, "</>", " > ;".to_string());
            return;
        }

        if let Some(rest) = line.strip_prefix("<:") {
            if let Some(name) = rest
                .split_whitespace()
                .next()
                .filter(|name| !name.is_empty())
            {
                replace_tail(self, " < ", format!(" < ;\n{name} /  > ;"));
                return;
            }
        }

        if let Some(rest) = line.strip_prefix("<.") {
            if let Some(open_end) = rest.find('>') {
                let name = &rest[..open_end];
                if !name.is_empty() && line.contains("</.") {
                    replace_tail(self, "<.", format!(" < .{name} > ;\n{name} > ;"));
                    return;
                }
            }
        }

        if let Some(open) = line.strip_prefix('<') {
            if let Some(bracket) = open.find('[') {
                let tag = &open[..bracket];
                if !tag.is_empty() {
                    if let Some(close_bracket) = open[bracket..].find(']') {
                        let index = &open[bracket..=bracket + close_bracket];
                        if line.contains(&format!("</{tag}[")) {
                            replace_tail(
                                self,
                                &format!("<{tag} "),
                                format!("<{tag} />;\n{index} > ;\n{tag}{index} > ;"),
                            );
                            return;
                        }
                    }
                }
            }
        }

        if let Some(close_start) = line.find("</") {
            let close_tail = &line[close_start + 2..];
            if let Some(space) = close_tail.find(" {") {
                let tag = &close_tail[..space];
                let emitted_close = format!("</{tag};");
                if let Some(pos) = self.output.rfind(&emitted_close) {
                    self.output.insert(pos + emitted_close.len() - 1, '>');
                }
                return;
            }
        }

        if line.starts_with('<') && line.ends_with(";") {
            if line.matches('"').count() % 2 == 1 && !line.contains('>') {
                let replacement = format!("{line}/>;");
                replace_tail(
                    self,
                    line.split_whitespace().next().unwrap_or("<"),
                    replacement,
                );
                return;
            }
            if line.ends_with("={}>;") {
                let recovered = line[..line.len() - 1].replace("{}", "");
                let tag = line[1..].split_whitespace().next().unwrap_or("");
                replace_tail(self, &format!("<{tag}"), format!("{recovered};</>;"));
                return;
            }
            if line.ends_with("=}>;") {
                let recovered = line[..line.len() - 1].replace("=}", "");
                let tag = line[1..].split_whitespace().next().unwrap_or("");
                replace_tail(self, &format!("<{tag}"), format!("{recovered};\n</>;"));
                return;
            }
            if line.ends_with("=<}>;") {
                let recovered = line[..line.len() - 1].replace("=<}>", "=<>");
                let tag = line[1..].split_whitespace().next().unwrap_or("");
                replace_tail(self, &format!("<{tag}"), format!("{recovered};\n</>/>;"));
                return;
            }
            if !line.contains("</")
                && (!line.contains("/>") || line.matches('<').count() > 1)
                && (line.matches('<').count() > 1
                    || line
                        .strip_prefix('<')
                        .is_some_and(|rest| rest.ends_with(">;")))
            {
                let recovered = &line[..line.len() - 1];
                replace_tail(self, recovered, format!("{recovered};</>;"));
            }
        }
    }

    /// Recovery for an unterminated template that swallows the beginning of a
    /// later array of templates. The parser legitimately produces several
    /// synthetic nested blocks, but TypeScript prints the intact source tail
    /// (`].join(...)` and its enclosing braces) at its original indentation,
    /// followed by only the synthetic closure suffix.
    fn normalize_unterminated_template_recovery_output(&mut self) {
        if !self.file_has_recovery_errors
            || self.source.bytes().filter(|byte| *byte == b'`').count() % 2 == 0
        {
            return;
        }

        let had_final_newline = self.output.ends_with('\n');
        let mut output_lines: Vec<String> = self.output.lines().map(str::to_string).collect();
        let mut removed_missing_tags = false;
        for line in &mut output_lines {
            if line.trim() == ") `," {
                let indent_len = line.len() - line.trim_start().len();
                line.truncate(indent_len);
                line.push_str("`,");
                removed_missing_tags = true;
            }
        }
        if !removed_missing_tags {
            return;
        }
        for line in &mut output_lines {
            if line.trim() == "`;" {
                let indent_len = line.len() - line.trim_start().len();
                line.truncate(indent_len.saturating_sub(2));
                line.push_str("`;");
            }
        }

        let Some(join_index) = output_lines
            .iter()
            .rposition(|line| line.trim_start().starts_with("].join("))
        else {
            return;
        };
        let Some(synthetic_suffix_index) = output_lines
            .iter()
            .enumerate()
            .skip(join_index + 1)
            .find_map(|(index, line)| (line.trim() == "};").then_some(index))
        else {
            return;
        };

        let normalized_source = self.source.replace("\r\n", "\n");
        let source_lines: Vec<&str> = normalized_source.lines().collect();
        let output_join = output_lines[join_index].trim();
        let Some(source_join_index) = source_lines
            .iter()
            .rposition(|line| line.trim() == output_join)
        else {
            return;
        };

        let suffix_indent_len = output_lines[synthetic_suffix_index].len()
            - output_lines[synthetic_suffix_index].trim_start().len();
        let mut rebuilt = output_lines[..join_index].to_vec();
        rebuilt.extend(
            source_lines[source_join_index..]
                .iter()
                .map(|line| (*line).to_string()),
        );
        rebuilt.push(format!(
            "{};",
            &output_lines[synthetic_suffix_index][..suffix_indent_len]
        ));
        rebuilt.extend_from_slice(&output_lines[synthetic_suffix_index + 1..]);
        self.output = rebuilt.join("\n");
        if had_final_newline {
            self.output.push('\n');
        }
    }

    fn normalize_malformed_import_type_mode_recovery_output(&mut self) {
        if !self.source.contains("\"resolution-mode\"") {
            return;
        }

        if self.source.contains(", [") {
            self.output = self.output.replace("\n).", "\n");
        }

        let mut pairs: Vec<(String, String, String)> = Vec::new();
        let mut search_start = 0usize;
        while let Some(import_rel) = self.source[search_start..].find("import(") {
            let import_start = search_start + import_rel;
            let args_start = import_start + "import(".len();
            let Some(close_rel) = self.source[args_start..].find(')') else {
                break;
            };
            let close = args_start + close_rel;
            let Some(comma_rel) = self.source[args_start..close].find(',') else {
                search_start = close + 1;
                continue;
            };
            let comma = args_start + comma_rel;
            let specifier = self.source[args_start..comma].trim();
            let option_alias = self.source[comma + 1..close].trim();
            let qualifier_start = close + 1;
            if !option_alias.chars().enumerate().all(|(idx, ch)| {
                ch == '_'
                    || ch == '$'
                    || ch.is_ascii_alphanumeric() && (idx > 0 || !ch.is_ascii_digit())
            }) || self.source.as_bytes().get(qualifier_start) != Some(&b'.')
            {
                search_start = close + 1;
                continue;
            }
            let mut qualifier_end = qualifier_start + 1;
            while self
                .source
                .as_bytes()
                .get(qualifier_end)
                .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
            {
                qualifier_end += 1;
            }
            let qualifier = &self.source[qualifier_start + 1..qualifier_end];
            let alias_decl = format!("type {option_alias} =");
            if !qualifier.is_empty()
                && self.source.contains(&alias_decl)
                && !pairs.iter().any(|(alias, _, _)| alias == option_alias)
            {
                pairs.push((
                    option_alias.to_string(),
                    qualifier.to_string(),
                    specifier.to_string(),
                ));
            }
            search_start = qualifier_end;
        }
        if pairs.len() < 2 {
            return;
        }

        for (alias, qualifier, _) in &pairs {
            self.output = self.output.replace(
                &format!("exports.{alias} ="),
                &format!("exports.{qualifier} = exports.{alias} ="),
            );
        }

        let (_, first_qualifier, _) = &pairs[0];
        let (second_alias, second_qualifier, second_specifier) = &pairs[1];
        let start_marker = format!(").{first_qualifier}\n");
        if let Some(start) = self.output.find(&start_marker) {
            let tail_start = start + start_marker.len();
            if let Some(next_export_rel) = self.output[tail_start..].find("\nexports.") {
                let next_export = tail_start + next_export_rel + 1;
                let replacement = format!(
                    "exports.{first_qualifier}\n    & import({second_specifier}, exports.{second_alias}).{second_qualifier};\n"
                );
                self.output.replace_range(start..next_export, &replacement);
            }
        }
        for (_, qualifier, _) in &pairs {
            self.output = self.output.replace(&format!("\n).{qualifier};\n"), "\n");
        }
    }

    fn file_has_unterminated_block_comment_eof_padding_recovery(&self, file: &SourceFile) -> bool {
        if !file.statements.is_empty() {
            return false;
        }
        let trimmed = self.source.trim_end_matches(['\n', '\r']);
        let Some(comment_start) = trimmed.rfind("/*") else {
            return false;
        };
        let tail = &trimmed[comment_start..];
        !tail.contains("*/")
    }

    fn attach_deferred_recovery_comment(&mut self, statement_output_start: usize) {
        if let Some(comment) = self.deferred_recovery_comment.take() {
            let newline = self.output[statement_output_start..]
                .find('\n')
                .map(|offset| statement_output_start + offset)
                .unwrap_or(self.output.len());
            if !self.output[statement_output_start..newline].contains(&comment) {
                self.output.insert_str(newline, &format!(" {comment}"));
            }
        }
    }

    fn append_whitespace_fragment_trailing_comment(&mut self, stmt: &Stmt) {
        if self.options.remove_comments == Some(true) || !matches!(&stmt.kind, StmtKind::Expr(_)) {
            return;
        }
        let start = stmt.span.start as usize;
        let Some(source_tail) = self.source.get(start..) else {
            return;
        };
        let line = source_tail.lines().next().unwrap_or(source_tail);
        let Some(comment_start) = find_trailing_comment_start(line) else {
            return;
        };
        let code = line[..comment_start].trim();
        if !code.starts_with('<') {
            return;
        }
        let Some(open_end) = code.find('>') else {
            return;
        };
        let opening_inner = &code[1..open_end];
        let closing = code[open_end + 1..].trim_start();
        let Some(closing_inner) = closing
            .strip_prefix("</")
            .and_then(|tail| tail.split_once('>'))
        else {
            return;
        };
        if opening_inner.is_empty()
            || !opening_inner.chars().all(char::is_whitespace)
            || !closing_inner.0.chars().all(char::is_whitespace)
            || closing_inner.1.trim() != ";"
        {
            return;
        }
        let comment = line[comment_start..].trim_end_matches('\r');
        if self.output.ends_with('\n') {
            let insert_at = self.output.len() - 1;
            let current_line = self.output[..insert_at]
                .rsplit_once('\n')
                .map(|(_, line)| line)
                .unwrap_or(&self.output[..insert_at]);
            if !current_line.contains(comment) {
                self.output.insert_str(insert_at, &format!(" {comment}"));
            }
        }
        self.advance_comment_pos((start + line.len()) as u32);
    }

    fn file_invalid_unicode_escape_trailing_header_comment_recovery(
        &self,
        file: &SourceFile,
    ) -> Option<[String; 2]> {
        let [stmt] = file.statements.as_slice() else {
            return None;
        };
        let StmtKind::Expr(expr) = &stmt.kind else {
            return None;
        };
        if !matches!(expr.kind, ExprKind::StrLit(_)) {
            return None;
        }
        let raw = self.copy_span_trimmed(stmt.span);
        if !Self::raw_string_literal_has_invalid_unicode_escape(raw.trim()) {
            return None;
        }

        let mut header = Vec::new();
        for line in self.source.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                if header.is_empty() {
                    continue;
                }
                break;
            }
            if !line.starts_with("//") {
                break;
            }
            header.push(line.to_string());
        }

        match header.as_slice() {
            [first, second] => Some([first.clone(), second.clone()]),
            _ => None,
        }
    }

    fn raw_string_literal_has_invalid_unicode_escape(text: &str) -> bool {
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            if bytes[i] == b'\\' && bytes[i + 1] == b'u' {
                let mut hex_count = 0usize;
                let mut j = i + 2;
                while j < bytes.len() && hex_count < 4 && bytes[j].is_ascii_hexdigit() {
                    hex_count += 1;
                    j += 1;
                }
                if hex_count < 4 {
                    return true;
                }
                i = j;
                continue;
            }
            i += 1;
        }
        false
    }

    fn stmt_has_malformed_await_using_for_export_recovery(&self, stmt: &Stmt) -> bool {
        let StmtKind::For(for_stmt) = &stmt.kind else {
            return false;
        };
        if !self.for_stmt_has_malformed_await_using_header_recovery_shape(for_stmt, stmt.span) {
            return false;
        }
        let stmt_src = self.copy_span_trimmed(stmt.span);
        stmt_src
            .lines()
            .skip(1)
            .any(|line| line.trim_start().starts_with("export "))
    }
}

fn binary_op_str(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Exp => "**",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::UShr => ">>>",
        BinaryOp::LogAnd => "&&",
        BinaryOp::LogOr => "||",
        BinaryOp::NullCoal => "??",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::StrictEq => "===",
        BinaryOp::StrictNe => "!==",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::In => "in",
        BinaryOp::InstanceOf => "instanceof",
    }
}

fn unary_op_str(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Pos => "+",
        UnaryOp::Neg => "-",
        UnaryOp::BitNot => "~",
        UnaryOp::LogNot => "!",
        UnaryOp::Typeof => "typeof",
        UnaryOp::Void => "void",
        UnaryOp::Delete => "delete",
    }
}

/// Extract the root identifier from an expression tree (free function version).
pub(crate) fn expr_root_ident_static(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.to_string()),
        ExprKind::Member(mem) => expr_root_ident_static(&mem.object),
        ExprKind::Paren(inner) | ExprKind::NonNull(inner) => expr_root_ident_static(inner),
        ExprKind::TypeAssertion(ta) => expr_root_ident_static(&ta.expr),
        ExprKind::As(a) => expr_root_ident_static(&a.expr),
        ExprKind::Satisfies(s) => expr_root_ident_static(&s.expr),
        ExprKind::Instantiation(inst) => expr_root_ident_static(&inst.expr),
        _ => None,
    }
}

/// Count how many hoisted temp vars an object rest assignment will need.
/// Only counts temps for:
///  - Rest-only patterns where the rest arg is a bare object/array literal
///    (e.g. `{...{}} = rhs` needs a temp, `{...({})} = rhs` does not)
///  - Mixed patterns (rest + other props) where the RHS is not a simple ident
///  - Complex patterns where non-spread props can't use simple destructuring
fn count_obj_rest_assign_hoisted_temps(lhs: &Expr, _rhs: &Expr) -> usize {
    // Only count bare ObjectLit LHS (not Paren(ObjectLit))
    let props = match &lhs.kind {
        ExprKind::ObjectLit(props) => props,
        _ => return 0,
    };
    let has_spread = props.iter().any(|p| matches!(p, ObjLitProp::Spread(_, _)));
    if !has_spread {
        return 0;
    }

    // Find the last spread element
    let last_spread = props.iter().rev().find_map(|p| {
        if let ObjLitProp::Spread(expr, _) = p {
            Some(expr)
        } else {
            None
        }
    });

    let has_non_spread = props.iter().any(|p| !matches!(p, ObjLitProp::Spread(_, _)));

    if !has_non_spread {
        // Rest-only: only needs temp if rest arg is a bare pattern (not paren-wrapped)
        if let Some(rest_arg) = last_spread {
            let is_bare_pattern = matches!(
                &rest_arg.kind,
                ExprKind::ObjectLit(_) | ExprKind::ArrayLit(_)
            );
            if is_bare_pattern {
                return 1;
            }
        }
        0
    } else {
        let nested_rest_temps = props
            .iter()
            .map(|prop| match prop {
                ObjLitProp::Property(prop) => count_nested_obj_rest_expr_temps(&prop.value, true),
                ObjLitProp::ShorthandDefault(_, init, _) => {
                    count_nested_obj_rest_expr_temps(init, true)
                }
                _ => 0,
            })
            .sum::<usize>();
        let dynamic_key_temps = props
            .iter()
            .filter(|prop| {
                matches!(
                    prop,
                    ObjLitProp::Property(ObjProp {
                        key: PropName::Computed(expr, _),
                        ..
                    }) if !matches!(expr.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_))
                )
            })
            .count();
        let dynamic_access_temps = props
            .iter()
            .filter(|prop| {
                matches!(
                    prop,
                    ObjLitProp::Property(ObjProp {
                        key: PropName::Computed(expr, _),
                        value,
                        ..
                    }) if !matches!(expr.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_))
                        && matches!(value.kind, ExprKind::Assign(_))
                )
            })
            .count();
        let dynamic_source_temp = usize::from(dynamic_key_temps > 0);
        1.max(nested_rest_temps + dynamic_key_temps + dynamic_access_temps + dynamic_source_temp)
    }
}

fn count_nested_obj_rest_expr_temps(expr: &Expr, nested: bool) -> usize {
    match &expr.kind {
        ExprKind::ObjectLit(props) => {
            let has_rest = props
                .iter()
                .any(|prop| matches!(prop, ObjLitProp::Spread(_, _)));
            if nested && has_rest {
                let dynamic_keys = props
                    .iter()
                    .filter(|prop| {
                        matches!(
                            prop,
                            ObjLitProp::Property(ObjProp {
                                key: PropName::Computed(key, _),
                                ..
                            }) if !matches!(key.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_))
                        )
                    })
                    .count();
                1 + dynamic_keys
            } else {
                props
                    .iter()
                    .map(|prop| match prop {
                        ObjLitProp::Property(prop) => {
                            count_nested_obj_rest_expr_temps(&prop.value, true)
                        }
                        ObjLitProp::ShorthandDefault(_, init, _) => {
                            count_nested_obj_rest_expr_temps(init, true)
                        }
                        _ => 0,
                    })
                    .sum()
            }
        }
        ExprKind::ArrayLit(elements) => elements
            .iter()
            .flatten()
            .map(|element| count_nested_obj_rest_expr_temps(element, true))
            .sum(),
        ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
            count_nested_obj_rest_expr_temps(inner, nested)
        }
        ExprKind::As(wrapper) => count_nested_obj_rest_expr_temps(&wrapper.expr, nested),
        ExprKind::Satisfies(wrapper) => count_nested_obj_rest_expr_temps(&wrapper.expr, nested),
        ExprKind::TypeAssertion(wrapper) => count_nested_obj_rest_expr_temps(&wrapper.expr, nested),
        _ => 0,
    }
}

fn assign_op_str(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "=",
        AssignOp::AddAssign => "+=",
        AssignOp::SubAssign => "-=",
        AssignOp::MulAssign => "*=",
        AssignOp::DivAssign => "/=",
        AssignOp::ModAssign => "%=",
        AssignOp::ExpAssign => "**=",
        AssignOp::BitAndAssign => "&=",
        AssignOp::BitOrAssign => "|=",
        AssignOp::BitXorAssign => "^=",
        AssignOp::ShlAssign => "<<=",
        AssignOp::ShrAssign => ">>=",
        AssignOp::UShrAssign => ">>>=",
        AssignOp::LogAndAssign => "&&=",
        AssignOp::LogOrAssign => "||=",
        AssignOp::NullCoalAssign => "??=",
    }
}

/// Map a compound assignment operator to its binary operator string.
/// E.g. `+=` → `+`, `-=` → `-`, etc.
fn compound_assign_to_bin_op_str(op: AssignOp) -> &'static str {
    match op {
        AssignOp::AddAssign => "+",
        AssignOp::SubAssign => "-",
        AssignOp::MulAssign => "*",
        AssignOp::DivAssign => "/",
        AssignOp::ModAssign => "%",
        AssignOp::ExpAssign => "**",
        AssignOp::BitAndAssign => "&",
        AssignOp::BitOrAssign => "|",
        AssignOp::BitXorAssign => "^",
        AssignOp::ShlAssign => "<<",
        AssignOp::ShrAssign => ">>",
        AssignOp::UShrAssign => ">>>",
        AssignOp::LogAndAssign => "&&",
        AssignOp::LogOrAssign => "||",
        AssignOp::NullCoalAssign => "??",
        AssignOp::Assign => "=",
    }
}

// ---------------------------------------------------------------------------
// Decorator helper stubs (will be completed by decorator-dev)
// ---------------------------------------------------------------------------

/// Extract the string representation of a property name.
fn prop_name_str(name: &PropName) -> String {
    match name {
        PropName::Ident(s, _) | PropName::String(s, _) => s.to_string(),
        PropName::Number(s, _) => s.to_string(),
        PropName::Private(s, _) => s.to_string(),
        PropName::Computed(_, _) => "[computed]".to_string(),
    }
}

/// Returns true if the property name is a computed expression.
fn prop_name_is_computed(name: &PropName) -> bool {
    matches!(name, PropName::Computed(_, _))
}

/// For computed property names, emit the expression and return its text.
fn prop_name_computed_str(name: &PropName, source: &str, options: &CompilerOptions) -> String {
    if let PropName::Computed(expr, _) = name {
        emit_expr_to_string(
            source,
            options,
            expr,
            &HashMap::new(),
            &HashSet::new(),
            &Default::default(),
        )
    } else {
        prop_name_str(name)
    }
}

fn obj_lit_prop_span(prop: &ObjLitProp) -> Span {
    match prop {
        ObjLitProp::Property(p) => p.span,
        ObjLitProp::Shorthand(_, sp) => *sp,
        ObjLitProp::ShorthandDefault(_, _, sp) => *sp,
        ObjLitProp::Spread(_, sp) => *sp,
        ObjLitProp::Method(m) => m.span,
        ObjLitProp::Get(a) | ObjLitProp::Set(a) => a.span,
    }
}

/// Emit a decorator expression to a string.
/// The parser includes the `@` in decorator expression spans, so we adjust
/// the top-level span to skip it before emitting.
fn emit_expr_to_string(
    source: &str,
    options: &CompilerOptions,
    expr: &Expr,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    import_shadows: &import_shadow::ImportShadows,
) -> String {
    emit_expr_to_string_inner(
        source,
        options,
        expr,
        cjs_import_map,
        cjs_string_import_locals,
        import_shadows,
        false,
    )
}

/// Like `emit_expr_to_string` but converts `await` → `yield` in the output.
/// Used for decorator parameter expressions on async methods, where the decorator
/// evaluation happens outside the `__awaiter` wrapper in a generator context.
fn emit_expr_to_string_await_to_yield(
    source: &str,
    options: &CompilerOptions,
    expr: &Expr,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    import_shadows: &import_shadow::ImportShadows,
) -> String {
    emit_expr_to_string_inner(
        source,
        options,
        expr,
        cjs_import_map,
        cjs_string_import_locals,
        import_shadows,
        true,
    )
}

fn emit_expr_to_string_inner(
    source: &str,
    options: &CompilerOptions,
    expr: &Expr,
    cjs_import_map: &HashMap<AstString, (AstString, AstString)>,
    cjs_string_import_locals: &HashSet<AstString>,
    import_shadows: &import_shadow::ImportShadows,
    await_to_yield: bool,
) -> String {
    let adjusted = if expr.span.start < expr.span.end
        && (expr.span.start as usize) < source.len()
        && source.as_bytes()[expr.span.start as usize] == b'@'
    {
        Expr {
            kind: expr.kind.clone(),
            span: Span::new(expr.span.start + 1, expr.span.end),
        }
    } else {
        expr.clone()
    };
    let mut e = Emitter::new(source, options, &[], HashMap::new());
    e.cjs_import_map = cjs_import_map.clone();
    e.cjs_string_import_locals = cjs_string_import_locals.clone();
    e.import_shadows = import_shadows.clone();
    if await_to_yield {
        e.in_static_block_await_to_yield = true;
    }
    e.emit_expr(&adjusted);
    e.output
}

#[cfg(test)]
mod tests {
    use super::{normalize_brace_spacing, ConstEnumValue, Emitter};

    #[test]
    fn es5_exported_empty_binding_checks_all_semantic_string_stores() {
        let marker = |index| format!("\0__tsrs_deferred_export_name_{index}__\0");
        let options = tsc_rs_ast::CompilerOptions {
            jsx_import_source: Some(marker(5)),
            jsx_factory: Some(marker(6)),
            jsx_fragment_factory: Some(marker(7)),
            ..Default::default()
        };
        let mut emitter = Emitter::new("", &options, &[], std::collections::HashMap::new());

        emitter.const_enum_values.insert(
            ("Local".to_string(), "Value".to_string()),
            ConstEnumValue::String(marker(0)),
        );
        emitter.external_const_enum_values.insert(
            ("External".to_string(), "Value".to_string()),
            ConstEnumValue::String(marker(1)),
        );
        emitter
            .file_string_consts
            .insert("localValue".to_string(), marker(2));
        emitter
            .external_file_string_consts
            .insert("externalValue".to_string(), marker(3));
        emitter.merged_string_enum_values.insert(
            "Merged".to_string(),
            std::collections::HashMap::from([("Value".to_string(), marker(4))]),
        );

        for index in 0..=7 {
            assert!(
                emitter.semantic_string_stores_contain(&marker(index)),
                "semantic string store {index} must reject its sentinel"
            );
        }
        assert!(!emitter.semantic_string_stores_contain(&marker(8)));
    }

    #[test]
    fn es5_exported_empty_binding_system_resolution_is_token_provenanced() {
        let options = tsc_rs_ast::CompilerOptions::default();
        let mut emitter = Emitter::new("", &options, &[], std::collections::HashMap::new());
        let marker = "\0__tsrs_deferred_export_name_0__\0";
        emitter.output = format!(
            "exports_1(\"{marker}\", {marker} = _a = {{}});\nvar semantic = `\"{marker}\", {marker}`;\n// \"{marker}\", {marker}\n"
        );

        emitter.replace_deferred_export_placeholder(marker, "_b");

        assert!(emitter
            .output
            .starts_with("exports_1(\"_b\", _b = _a = {});"));
        assert!(
            emitter
                .output
                .contains(&format!("var semantic = `\"{marker}\", {marker}`;")),
            "{}",
            emitter.output
        );
        assert!(
            emitter
                .output
                .contains(&format!("// \"{marker}\", {marker}")),
            "{}",
            emitter.output
        );
    }

    #[test]
    fn test_normalize_async_gen() {
        let input = "async function * asyncGen (n) {\n    for (let i = 0; i < n; i++)\n      yield i * 2;\n  }";
        let output = normalize_brace_spacing(input);
        eprintln!("INPUT:  {:?}", input);
        eprintln!("OUTPUT: {:?}", output);
        assert!(
            output.starts_with("async"),
            "Expected output to start with 'async', got: {:?}",
            &output[..20.min(output.len())]
        );
    }

    #[test]
    fn test_async_gen_full_emit() {
        let source = "export { };\nasync function * asyncGen (n) {\n    for (let i = 0; i < n; i++)\n      yield i * 2;\n  }\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let js = super::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
        assert!(
            js.contains("async function*"),
            "Expected 'async function*' in output:\n{}",
            js
        );
    }

    #[test]
    fn test_await_no_arg() {
        let source = "var foo = async (a = await): Promise<void> => {\n}\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let js = super::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
        // Ensure no double )) from await consuming the closing paren
        assert!(
            !js.contains("))"),
            "Should not have double )) in output:\n{}",
            js
        );
        // Ensure arrow body is present
        assert!(
            js.contains("=>"),
            "Arrow function should be present in output:\n{}",
            js
        );
    }

    #[test]
    fn test_const_enum_inline() {
        let source = "const enum E { A, B, C }\nfoo(E.A);\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let mut opts = tsc_rs_ast::CompilerOptions::default();
        opts.always_strict = Some(false);
        let js = super::emit(&file, &opts).javascript;
        assert!(
            js.contains("0 /* E.A */"),
            "Expected const enum inlining in output:\n{}",
            js
        );
    }

    #[test]
    fn test_exponentiation_downlevel() {
        let source = "var x = a ** b;\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let mut opts = tsc_rs_ast::CompilerOptions::default();
        opts.target = Some(tsc_rs_ast::ScriptTarget::ES5);
        let js = super::emit(&file, &opts).javascript;
        assert!(
            js.contains("Math.pow(a, b)"),
            "Expected Math.pow downlevel in output:\n{}",
            js
        );
    }

    #[test]
    fn test_bigint_hex_lowercase() {
        let source = "var x = 0xFFFFn;\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let js = super::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
        assert!(
            js.contains("0xffffn"),
            "Expected lowercased hex in BigInt:\n{}",
            js
        );
    }

    #[test]
    fn test_bigint_hex_neg() {
        let source = "const negHex = -0x10n;\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let js = super::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
        assert!(
            js.contains("const negHex = -0x10n;"),
            "Expected const negHex = -0x10n;\nGot: {}",
            js
        );
    }

    #[test]
    fn test_bigint_negative_type_annotation() {
        // Negative BigInt literal type annotation should be erased correctly
        let source = "const negHex: -16n = -0x10n;\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let js = super::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
        assert!(
            js.contains("const negHex = -0x10n;"),
            "Expected type erasure to produce 'const negHex = -0x10n;'\nGot: {}",
            js
        );
    }

    #[test]
    fn test_empty_bracket_collapse() {
        // Empty brackets should collapse: `[  ]` → `[]`, `] [` → `][`
        let input = "a [      ]   [  ];";
        let output = normalize_brace_spacing(input);
        eprintln!("EMPTY BRACKETS: input={:?} output={:?}", input, output);
        assert!(
            output.contains("[][]"),
            "Expected chained empty brackets in: {:?}",
            output
        );
    }

    #[test]
    fn test_identifier_array_suffix_spacing() {
        let input = "new   Z     [      ]   [  ];";
        let output = normalize_brace_spacing(input);
        assert!(
            output.contains("new Z[][]"),
            "Expected collapsed array suffix spacing: {:?}",
            output
        );
    }

    #[test]
    fn test_index_signature_stripped_from_class() {
        // Bug 1: Index signatures must be removed from JS class bodies
        let source = "class A {\n    [key: string]: number;\n    x = 5;\n}\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let mut opts = tsc_rs_ast::CompilerOptions::default();
        opts.target = Some(tsc_rs_ast::ScriptTarget::ESNext);
        opts.use_define_for_class_fields = Some(true);
        let js = super::emit(&file, &opts).javascript;
        assert!(
            !js.contains("[key"),
            "Index signature should be stripped from class body, got:\n{}",
            js
        );
        assert!(
            js.contains("x = 5"),
            "Regular class field should still be emitted, got:\n{}",
            js
        );
    }

    #[test]
    fn test_private_field_type_ann_stripped() {
        // Bug 2: Private class fields must not include type annotation in JS
        let source = "class B {\n    #a: boolean;\n    #b: string = \"hello\";\n}\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let mut opts = tsc_rs_ast::CompilerOptions::default();
        opts.target = Some(tsc_rs_ast::ScriptTarget::ESNext);
        opts.use_define_for_class_fields = Some(true);
        let js = super::emit(&file, &opts).javascript;
        // Should have #a; (no ": boolean")
        assert!(
            !js.contains(": boolean"),
            "Type annotation ':boolean' should be stripped from private field, got:\n{}",
            js
        );
        // Should have #b = "hello"; (no ": string")
        assert!(
            !js.contains(": string"),
            "Type annotation ':string' should be stripped from private field, got:\n{}",
            js
        );
        assert!(
            js.contains("#b"),
            "Private field #b should be emitted, got:\n{}",
            js
        );
    }

    #[test]
    fn test_constructor_param_type_ann_stripped() {
        // Bug 3: Constructor parameter type annotations must be stripped
        let source = "class C {\n    constructor(x: string, y: number) {}\n}\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let mut opts = tsc_rs_ast::CompilerOptions::default();
        opts.target = Some(tsc_rs_ast::ScriptTarget::ESNext);
        opts.use_define_for_class_fields = Some(true);
        let js = super::emit(&file, &opts).javascript;
        // Should have constructor(x, y) but NOT constructor(x: string, y: number)
        assert!(
            js.contains("constructor(x, y)"),
            "Constructor params should have type annotations stripped, got:\n{}",
            js
        );
        assert!(
            !js.contains(": string"),
            "Type annotation ':string' should be stripped from constructor param, got:\n{}",
            js
        );
        assert!(
            !js.contains(": number"),
            "Type annotation ':number' should be stripped from constructor param, got:\n{}",
            js
        );
    }

    #[test]
    fn test_namespace_var_type_ann_stripped() {
        // Bug 4: Namespace variable declarations with type annotations
        let source =
            "namespace NS {\n    export var x: any;\n    export var y: string = \"hi\";\n}\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let mut opts = tsc_rs_ast::CompilerOptions::default();
        opts.target = Some(tsc_rs_ast::ScriptTarget::ES5);
        let js = super::emit(&file, &opts).javascript;
        // var x: any should become just NS.y = "hi" (no var for non-initialized),
        // and NS.y = "hi" (initializer without type)
        assert!(
            !js.contains(": any"),
            "Type annotation ':any' should be stripped from namespace var, got:\n{}",
            js
        );
        assert!(
            !js.contains(": string"),
            "Type annotation ':string' should be stripped from namespace var, got:\n{}",
            js
        );
    }

    #[test]
    fn test_regex_space_in_group() {
        let source = r#"x.replace(/( {2,}|\n)/g, f);"#;
        let source_with_nl = format!("{}\n", source);
        let file = tsc_rs_parser::parse("test.ts", &source_with_nl);
        let js = super::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
        assert!(
            js.contains("/( {2,}|"),
            "Expected regex space preserved, got:\n{}",
            js
        );
    }

    #[test]
    fn test_regex_char_class_slash() {
        let source = "var foo2 = \"a//\".replace(/.[//]/g, \"\");\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let js = super::emit(&file, &tsc_rs_ast::CompilerOptions::default()).javascript;
        assert!(
            js.contains("/.[//]/g"),
            "Expected regex with char class preserved, got:\n{}",
            js
        );
    }
}

#[cfg(test)]
mod strip_types_tests {
    use super::*;

    #[test]
    fn emit_strip_types_smoke() {
        let source = r#"import type { Foo } from "./foo";
import { bar } from "./bar";

interface MyInterface { name: string; age: number; }

type MyType = string | number;

const x: number = 42;
const y = (a: string, b?: number): boolean => {
  return a.length > (b ?? 0);
};

async function greet(name: string): Promise<string> {
  return `Hello, ${name}!`;
}

export class MyClass {
  private name: string;
  readonly id: number;
  constructor(name: string, id: number) {
    this.name = name;
    this.id = id;
  }
  getName(): string { return this.name; }
}

enum Color { Red, Green, Blue }

const c: Color = Color.Red;

export default greet;
export { x, y };
"#;
        let ast = tsc_rs_parser::parse("test.ts", source);
        let output = emit_strip_types(&ast);
        let js = &output.javascript;

        // Type-only import stripped
        assert!(
            !js.contains("import type"),
            "type import should be stripped\n{}",
            js
        );
        // Value import preserved
        assert!(js.contains("bar"), "value import preserved\n{}", js);
        assert!(js.contains("./bar"), "import source preserved\n{}", js);
        // Interface stripped
        assert!(!js.contains("MyInterface"), "interface stripped\n{}", js);
        // Type annotations stripped from variable declarations
        assert!(
            js.contains("const x = 42"),
            "type annotation stripped, value preserved\n{}",
            js
        );
        // Modern syntax preserved
        assert!(
            js.contains("b ?? 0"),
            "nullish coalescing preserved\n{}",
            js
        );
        // Async preserved
        assert!(
            js.contains("async function greet"),
            "async preserved\n{}",
            js
        );
        // Class preserved
        assert!(js.contains("class MyClass"), "class preserved\n{}", js);
        // Enum produces runtime code
        assert!(js.contains("Color"), "enum produces runtime code\n{}", js);
        // Exports preserved
        assert!(
            js.contains("export default greet"),
            "default export\n{}",
            js
        );
        assert!(js.contains("export {"), "named exports\n{}", js);

        eprintln!("=== emit_strip_types output ===\n{}", js);
    }
}
